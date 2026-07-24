#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Transport-neutral typed commands and durable-event envelopes for Caelix.
//!
//! Event delivery is at least once. Event handlers must be idempotent.

use async_nats::{Client, Message, Request, RequestErrorKind};
use caelix_core::{
    Container, MessageContext, MessageDelivery, MessageHandlerDef, MessageHandlerKind, Module,
    build_container_with_setup, collect_module_message_handlers_with_container, shutdown_module,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tokio::{task::JoinSet, time::timeout};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[cfg(feature = "redis")]
mod redis_transport;

const PROTOCOL_VERSION: u8 = 1;
const MINIMUM_RESPONSE_ENVELOPE_BYTES: usize = 256;

/// Hidden serde re-export used by generated handler code.
#[doc(hidden)]
pub use serde as __serde;
/// Hidden serde_json re-export used by generated handler code.
#[doc(hidden)]
pub use serde_json as __serde_json;

/// Configuration used to connect and supervise a NATS microservice transport.
#[derive(Clone, Debug)]
pub struct NatsTransportOptions {
    server: String,
    service_name: Option<String>,
    rpc_timeout: Duration,
    max_request_bytes: usize,
    max_response_bytes: usize,
    max_event_bytes: usize,
    max_handler_concurrency: usize,
    shutdown_timeout: Duration,
    jetstream_stream: Option<String>,
    dead_letter_subject: Option<String>,
    max_event_deliveries: i64,
    event_retry_delay: Duration,
    event_ack_wait: Duration,
}

impl NatsTransportOptions {
    /// Creates options for a NATS server URL.
    pub fn new(server: impl Into<String>) -> Self {
        Self {
            server: server.into(),
            service_name: None,
            rpc_timeout: Duration::from_secs(5),
            max_request_bytes: 1024 * 1024,
            max_response_bytes: 1024 * 1024,
            max_event_bytes: 1024 * 1024,
            max_handler_concurrency: 64,
            shutdown_timeout: Duration::from_secs(10),
            jetstream_stream: None,
            dead_letter_subject: None,
            max_event_deliveries: 5,
            event_retry_delay: Duration::from_secs(1),
            event_ack_wait: Duration::from_secs(30),
        }
    }

    /// Sets the stable queue-group identity used by command consumers.
    pub fn service_name(mut self, name: impl Into<String>) -> Self {
        self.service_name = Some(name.into());
        self
    }

    /// Sets the maximum time a request waits for a response.
    pub fn rpc_timeout(mut self, value: Duration) -> Self {
        self.rpc_timeout = value;
        self
    }

    /// Sets the configured JetStream stream name for durable event topology.
    pub fn jetstream_stream(mut self, value: impl Into<String>) -> Self {
        self.jetstream_stream = Some(value.into());
        self
    }

    /// Sets the subject used for final-failure dead-letter envelopes.
    pub fn dead_letter_subject(mut self, value: impl Into<String>) -> Self {
        self.dead_letter_subject = Some(value.into());
        self
    }

    /// Limits request envelope bytes.
    pub fn max_request_bytes(mut self, value: usize) -> Self {
        self.max_request_bytes = value.max(1);
        self
    }

    /// Limits response envelope bytes.
    pub fn max_response_bytes(mut self, value: usize) -> Self {
        self.max_response_bytes = value.max(MINIMUM_RESPONSE_ENVELOPE_BYTES);
        self
    }

    /// Limits event envelope bytes.
    pub fn max_event_bytes(mut self, value: usize) -> Self {
        self.max_event_bytes = value.max(1);
        self
    }

    /// Bounds simultaneous handler invocation per subscription.
    pub fn max_handler_concurrency(mut self, value: usize) -> Self {
        self.max_handler_concurrency = value.max(1);
        self
    }

    /// Sets the amount of time shutdown waits for transport tasks.
    pub fn shutdown_timeout(mut self, value: Duration) -> Self {
        self.shutdown_timeout = value;
        self
    }

    /// Sets the delivery attempt at which a retryable event is dead-lettered.
    pub fn max_event_deliveries(mut self, value: i64) -> Self {
        self.max_event_deliveries = value.max(1);
        self
    }

    /// Sets the JetStream delayed NAK interval for retryable event failures.
    pub fn event_retry_delay(mut self, value: Duration) -> Self {
        self.event_retry_delay = value;
        self
    }

    /// Sets how long JetStream waits before redelivering an unacknowledged event.
    pub fn event_ack_wait(mut self, value: Duration) -> Self {
        self.event_ack_wait = value.max(Duration::from_millis(10));
        self
    }

    fn queue_group(&self) -> Result<&str, MicroserviceError> {
        self.service_name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| {
                MicroserviceError::Configuration(
                    "service_name is required for command queue groups".into(),
                )
            })
    }
}

/// Configuration for the Redis Streams microservice transport.
#[cfg(feature = "redis")]
pub use redis_transport::RedisTransportOptions;

/// Selects the transport used by a microservice application or client.
#[derive(Clone, Debug)]
pub enum MicroserviceTransportOptions {
    /// Core NATS with JetStream event delivery.
    Nats(NatsTransportOptions),
    /// Redis Streams with Pub/Sub command replies.
    #[cfg(feature = "redis")]
    Redis(RedisTransportOptions),
}

impl From<NatsTransportOptions> for MicroserviceTransportOptions {
    fn from(value: NatsTransportOptions) -> Self {
        Self::Nats(value)
    }
}

#[cfg(feature = "redis")]
impl From<RedisTransportOptions> for MicroserviceTransportOptions {
    fn from(value: RedisTransportOptions) -> Self {
        Self::Redis(value)
    }
}

/// A sanitized application failure returned across a command boundary.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RemoteError {
    /// Stable public error code.
    pub code: String,
    /// Safe public error message.
    pub message: String,
    /// Optional structured, safe details.
    pub details: Option<Value>,
    /// Whether a caller may safely retry according to the handler result.
    pub retryable: bool,
}

impl fmt::Display for RemoteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

/// Errors produced by the typed microservice client.
#[derive(Debug, Error)]
pub enum MicroserviceClientError {
    /// The configured request deadline elapsed.
    #[error("microservice request timed out")]
    Timeout,
    /// No command service subscribed to the requested subject.
    #[error("no responder for microservice request")]
    NoResponder,
    /// A JSON envelope or typed payload could not be decoded.
    #[error("microservice decode failure: {0}")]
    Decode(String),
    /// A response violated the Caelix wire protocol.
    #[error("microservice protocol failure: {0}")]
    Protocol(String),
    /// The selected broker rejected or interrupted the transport operation.
    #[error("microservice transport failure: {0}")]
    Transport(String),
    /// The remote handler returned a sanitized application failure.
    #[error("remote microservice failure: {0}")]
    Remote(RemoteError),
}

/// Errors produced while constructing or supervising a microservice application.
#[derive(Debug, Error)]
pub enum MicroserviceError {
    /// Transport connection or subscription setup failed.
    #[error("microservice transport failure: {0}")]
    Transport(String),
    /// Module construction or lifecycle failed.
    #[error("Caelix startup failure: {0}")]
    Framework(String),
    /// The supplied transport configuration is incomplete or inconsistent.
    #[error("invalid microservice configuration: {0}")]
    Configuration(String),
}

/// Injectable typed client used for request/reply commands and events.
#[derive(Clone)]
pub struct MicroserviceClient {
    transport: ClientTransport,
}

#[derive(Clone)]
enum ClientTransport {
    Nats {
        client: Client,
        options: NatsTransportOptions,
    },
    #[cfg(feature = "redis")]
    Redis(redis_transport::RedisClient),
}

impl MicroserviceClient {
    /// Connects a standalone client.
    pub async fn connect(
        options: impl Into<MicroserviceTransportOptions>,
    ) -> Result<Self, MicroserviceError> {
        match options.into() {
            MicroserviceTransportOptions::Nats(options) => {
                let client = async_nats::connect(&options.server)
                    .await
                    .map_err(|error| MicroserviceError::Transport(error.to_string()))?;
                Ok(Self {
                    transport: ClientTransport::Nats { client, options },
                })
            }
            #[cfg(feature = "redis")]
            MicroserviceTransportOptions::Redis(options) => Ok(Self {
                transport: ClientTransport::Redis(
                    redis_transport::RedisClient::connect(options).await?,
                ),
            }),
        }
    }

    fn from_connected(client: Client, options: NatsTransportOptions) -> Self {
        Self {
            transport: ClientTransport::Nats { client, options },
        }
    }

    /// Sends a typed Core NATS command and decodes its typed response.
    pub async fn request<P, R>(
        &self,
        subject: impl AsRef<str>,
        payload: P,
    ) -> Result<R, MicroserviceClientError>
    where
        P: Serialize + Send,
        R: DeserializeOwned + Send,
    {
        #[cfg(feature = "redis")]
        if let ClientTransport::Redis(client) = &self.transport {
            return client.request(subject.as_ref(), payload).await;
        }
        let (client, options) = match &self.transport {
            ClientTransport::Nats { client, options } => (client, options),
            #[cfg(feature = "redis")]
            ClientTransport::Redis(_) => unreachable!(),
        };
        let correlation_id = Uuid::new_v4().to_string();
        let request = RequestEnvelope {
            version: PROTOCOL_VERSION,
            correlation_id: correlation_id.clone(),
            deadline_unix_millis: deadline_millis(options.rpc_timeout),
            reply_channel: None,
            headers: BTreeMap::new(),
            payload: serde_json::to_value(payload)
                .map_err(|error| MicroserviceClientError::Decode(error.to_string()))?,
        };
        let bytes = encode_limited(&request, options.max_request_bytes)
            .map_err(MicroserviceClientError::Protocol)?;
        let request = Request::new()
            .headers(caelix_headers(
                &request.headers,
                Some(&request.correlation_id),
                request.deadline_unix_millis,
            ))
            .timeout(Some(options.rpc_timeout))
            .payload(bytes.into());
        let response = timeout(
            options.rpc_timeout,
            client.send_request(subject.as_ref().to_owned(), request),
        )
        .await
        .map_err(|_| MicroserviceClientError::Timeout)?
        .map_err(request_transport_error)?;
        if response.payload.len() > options.max_response_bytes {
            return Err(MicroserviceClientError::Protocol(
                "response envelope exceeds configured maximum".into(),
            ));
        }
        let envelope: ResponseEnvelope = serde_json::from_slice(&response.payload)
            .map_err(|error| MicroserviceClientError::Decode(error.to_string()))?;
        if envelope.version != PROTOCOL_VERSION {
            return Err(MicroserviceClientError::Protocol(
                "unsupported response protocol version".into(),
            ));
        }
        if envelope.correlation_id != correlation_id {
            return Err(MicroserviceClientError::Protocol(
                "response correlation ID does not match request".into(),
            ));
        }
        match envelope.body {
            ResponseBody::Success { payload } => serde_json::from_value(payload)
                .map_err(|error| MicroserviceClientError::Decode(error.to_string())),
            ResponseBody::Error(error) => Err(MicroserviceClientError::Remote(error)),
        }
    }

    /// Publishes a typed event envelope.
    pub async fn emit<P>(
        &self,
        subject: impl AsRef<str>,
        payload: P,
    ) -> Result<(), MicroserviceClientError>
    where
        P: Serialize + Send,
    {
        #[cfg(feature = "redis")]
        if let ClientTransport::Redis(client) = &self.transport {
            return client.emit(subject.as_ref(), payload).await;
        }
        let (client, options) = match &self.transport {
            ClientTransport::Nats { client, options } => (client, options),
            #[cfg(feature = "redis")]
            ClientTransport::Redis(_) => unreachable!(),
        };
        let event = EventEnvelope {
            version: PROTOCOL_VERSION,
            event_id: Uuid::new_v4().to_string(),
            headers: BTreeMap::new(),
            payload: serde_json::to_value(payload)
                .map_err(|error| MicroserviceClientError::Decode(error.to_string()))?,
            published_at_unix_millis: now_millis(),
        };
        let bytes = encode_limited(&event, options.max_event_bytes)
            .map_err(MicroserviceClientError::Protocol)?;
        let jetstream = async_nats::jetstream::new(client.clone());
        let acknowledgement = jetstream
            .publish_with_headers(
                subject.as_ref().to_owned(),
                caelix_headers(&event.headers, Some(&event.event_id), None),
                bytes.into(),
            )
            .await
            .map_err(|error| MicroserviceClientError::Transport(error.to_string()))?;
        acknowledgement
            .await
            .map(|_| ())
            .map_err(|error| MicroserviceClientError::Transport(error.to_string()))
    }
}

/// A standalone NATS application using Caelix modules and DI.
struct NatsMicroserviceApplication<M: Module + 'static> {
    runtime: MicroserviceRuntime<M>,
}

impl<M: Module + 'static> NatsMicroserviceApplication<M> {
    /// Connects NATS, registers `MicroserviceClient`, and builds one module container.
    pub async fn new(options: NatsTransportOptions) -> Result<Self, MicroserviceError> {
        let client = async_nats::connect(&options.server)
            .await
            .map_err(|error| MicroserviceError::Transport(error.to_string()))?;
        let injectable_client = MicroserviceClient::from_connected(client.clone(), options.clone());
        let container = build_container_with_setup::<M>(|container| {
            container.register_instance(injectable_client);
        })
        .await
        .map_err(|error| MicroserviceError::Framework(error.message))?;
        let handlers = collect_module_message_handlers_with_container::<M>(Some(&container))
            .map_err(|error| MicroserviceError::Framework(error.message))?;
        Ok(Self {
            runtime: MicroserviceRuntime::new(Arc::new(container), client, options, handlers),
        })
    }

    /// Takes ownership of the runtime for explicit startup and shutdown control.
    fn into_runtime(self) -> MicroserviceRuntime<M> {
        self.runtime
    }
}

/// A standalone microservice application using one selected transport.
pub struct MicroserviceApplication<M: Module + 'static> {
    runtime: ApplicationRuntime<M>,
}

enum ApplicationRuntime<M: Module + 'static> {
    Nats(MicroserviceRuntime<M>),
    #[cfg(feature = "redis")]
    Redis(redis_transport::RedisRuntime<M>),
}

// Deliberately private: transport implementations are an internal framework
// boundary, while applications depend only on the unified public types.
trait TransportAdapter<M: Module + 'static>: Sized {
    fn adapter_container(&self) -> Arc<Container>;
    async fn adapter_run(self) -> Result<(), MicroserviceError>;
}

impl<M: Module + 'static> TransportAdapter<M> for MicroserviceRuntime<M> {
    fn adapter_container(&self) -> Arc<Container> {
        self.container()
    }

    async fn adapter_run(self) -> Result<(), MicroserviceError> {
        self.run().await
    }
}

#[cfg(feature = "redis")]
impl<M: Module + 'static> TransportAdapter<M> for redis_transport::RedisRuntime<M> {
    fn adapter_container(&self) -> Arc<Container> {
        self.container()
    }

    async fn adapter_run(self) -> Result<(), MicroserviceError> {
        self.run().await
    }
}

impl<M: Module + 'static> MicroserviceApplication<M> {
    /// Connects the selected transport, registers the unified client, and builds the module.
    pub async fn new(
        options: impl Into<MicroserviceTransportOptions>,
    ) -> Result<Self, MicroserviceError> {
        let runtime = match options.into() {
            MicroserviceTransportOptions::Nats(options) => ApplicationRuntime::Nats(
                NatsMicroserviceApplication::<M>::new(options)
                    .await?
                    .into_runtime(),
            ),
            #[cfg(feature = "redis")]
            MicroserviceTransportOptions::Redis(options) => {
                ApplicationRuntime::Redis(redis_transport::RedisRuntime::<M>::new(options).await?)
            }
        };
        Ok(Self { runtime })
    }

    /// Returns the shared dependency container.
    pub fn container(&self) -> Arc<Container> {
        match &self.runtime {
            ApplicationRuntime::Nats(runtime) => runtime.adapter_container(),
            #[cfg(feature = "redis")]
            ApplicationRuntime::Redis(runtime) => runtime.adapter_container(),
        }
    }

    /// Starts consumers and waits for shutdown.
    pub async fn run(self) -> Result<(), MicroserviceError> {
        match self.runtime {
            ApplicationRuntime::Nats(runtime) => runtime.adapter_run().await,
            #[cfg(feature = "redis")]
            ApplicationRuntime::Redis(runtime) => runtime.adapter_run().await,
        }
    }
}

/// Owns NATS subscriptions and coordinated module shutdown.
pub struct MicroserviceRuntime<M: Module + 'static> {
    container: Arc<Container>,
    client: Client,
    options: NatsTransportOptions,
    handlers: Vec<MessageHandlerDef>,
    cancellation: CancellationToken,
    start_lock: tokio::sync::Mutex<()>,
    tasks: Mutex<JoinSet<Result<(), MicroserviceError>>>,
    started: Mutex<bool>,
    shutdown: Mutex<bool>,
    marker: std::marker::PhantomData<M>,
}

impl<M: Module + 'static> MicroserviceRuntime<M> {
    fn new(
        container: Arc<Container>,
        client: Client,
        options: NatsTransportOptions,
        handlers: Vec<MessageHandlerDef>,
    ) -> Self {
        Self {
            container,
            client,
            options,
            handlers,
            cancellation: CancellationToken::new(),
            start_lock: tokio::sync::Mutex::new(()),
            tasks: Mutex::new(JoinSet::new()),
            started: Mutex::new(false),
            shutdown: Mutex::new(false),
            marker: std::marker::PhantomData,
        }
    }

    /// Starts all declared subscriptions. Calling this repeatedly is harmless.
    pub async fn start(&self) -> Result<(), MicroserviceError> {
        let _start_guard = self.start_lock.lock().await;
        if self.cancellation.is_cancelled() {
            return Err(MicroserviceError::Transport(
                "microservice runtime is shutting down".into(),
            ));
        }
        if *self
            .started
            .lock()
            .expect("microservice start lock poisoned")
        {
            return Ok(());
        }
        let mut new_tasks = JoinSet::new();
        let service_name = if self.handlers.is_empty() {
            None
        } else {
            Some(self.options.queue_group()?.to_owned())
        };
        let mut event_subjects: Vec<String> = self
            .handlers
            .iter()
            .filter(|handler| handler.kind == MessageHandlerKind::Event)
            .map(|handler| handler.pattern.to_owned())
            .collect();
        if let Some(subject) = &self.options.dead_letter_subject {
            event_subjects.push(subject.clone());
        }
        if !event_subjects.is_empty() {
            if let Some(subject) = &self.options.dead_letter_subject {
                if !valid_nats_subject(subject)
                    || self
                        .handlers
                        .iter()
                        .any(|handler| nats_pattern_matches(handler.pattern, subject))
                {
                    return Err(MicroserviceError::Configuration(format!(
                        "dead-letter subject `{subject}` must be valid and disjoint from all command and event handlers"
                    )));
                }
            }
            let stream_name = self.options.jetstream_stream.as_ref().ok_or_else(|| {
                MicroserviceError::Configuration(
                    "jetstream_stream is required when registering event handlers".into(),
                )
            })?;
            let stream = async_nats::jetstream::new(self.client.clone())
                .get_or_create_stream(async_nats::jetstream::stream::Config {
                    name: stream_name.clone(),
                    subjects: event_subjects.clone(),
                    ..Default::default()
                })
                .await
                .map_err(|error| MicroserviceError::Transport(error.to_string()))?;
            if event_subjects.iter().any(|subject| {
                !stream
                    .cached_info()
                    .config
                    .subjects
                    .iter()
                    .any(|configured| nats_pattern_covers(configured, subject))
            }) {
                return Err(MicroserviceError::Configuration(format!(
                    "JetStream stream {stream_name} does not cover every declared event or dead-letter subject"
                )));
            }
            for handler in self
                .handlers
                .iter()
                .filter(|handler| handler.kind == MessageHandlerKind::Event)
            {
                let durable = durable_name(
                    service_name
                        .as_deref()
                        .expect("event handlers require a service name"),
                    handler.pattern,
                );
                let consumer = stream
                    .get_or_create_consumer(
                        &durable,
                        async_nats::jetstream::consumer::pull::Config {
                            durable_name: Some(durable.clone()),
                            filter_subject: handler.pattern.to_owned(),
                            ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                            // Caelix triggers the final delivery policy itself so a
                            // failed DLQ publication can be NAKed without losing the
                            // original message at a broker-enforced delivery limit.
                            max_deliver: -1,
                            max_ack_pending: self.options.max_handler_concurrency as i64,
                            ack_wait: self.options.event_ack_wait,
                            ..Default::default()
                        },
                    )
                    .await
                    .map_err(|error| MicroserviceError::Transport(error.to_string()))?;
                let config = &consumer.cached_info().config;
                if config.filter_subject != handler.pattern
                    || config.ack_policy != async_nats::jetstream::consumer::AckPolicy::Explicit
                    || config.max_deliver != -1
                    || config.max_ack_pending != self.options.max_handler_concurrency as i64
                    || config.ack_wait != self.options.event_ack_wait
                {
                    return Err(MicroserviceError::Configuration(format!(
                        "JetStream consumer {durable} is incompatible with the declared event handler"
                    )));
                }
                let messages = consumer
                    .messages()
                    .await
                    .map_err(|error| MicroserviceError::Transport(error.to_string()))?;
                let handler = handler.clone();
                let container = self.container.clone();
                let client = self.client.clone();
                let options = self.options.clone();
                let cancellation = self.cancellation.child_token();
                new_tasks.spawn(async move {
                    consume_event(messages, handler, container, client, options, cancellation).await
                });
            }
        }
        for handler in self
            .handlers
            .iter()
            .filter(|handler| handler.kind == MessageHandlerKind::Command)
        {
            let subscriber = self
                .client
                .queue_subscribe(
                    handler.pattern,
                    service_name
                        .as_ref()
                        .expect("command handlers require a queue group")
                        .clone(),
                )
                .await
                .map_err(|error| MicroserviceError::Transport(error.to_string()))?;
            let handler = handler.clone();
            let container = self.container.clone();
            let client = self.client.clone();
            let cancellation = self.cancellation.child_token();
            let options = self.options.clone();
            new_tasks.spawn(async move {
                consume_command(
                    subscriber,
                    handler,
                    container,
                    client,
                    options,
                    cancellation,
                )
                .await
            });
        }
        self.client
            .flush()
            .await
            .map_err(|error| MicroserviceError::Transport(error.to_string()))?;
        *self.tasks.lock().expect("microservice task lock poisoned") = new_tasks;
        *self
            .started
            .lock()
            .expect("microservice start lock poisoned") = true;
        Ok(())
    }

    /// Starts subscriptions and waits for application cancellation.
    pub async fn run(self) -> Result<(), MicroserviceError> {
        if let Err(error) = self.start().await {
            let _ = self.shutdown().await;
            return Err(error);
        }
        let mut tasks = {
            let mut guard = self.tasks.lock().expect("microservice task lock poisoned");
            std::mem::replace(&mut *guard, JoinSet::new())
        };
        let mut runtime_error = None;
        loop {
            tokio::select! {
                _ = self.cancellation.cancelled() => break,
                signal = tokio::signal::ctrl_c() => {
                    signal.map_err(|error| MicroserviceError::Transport(error.to_string()))?;
                    self.cancellation.cancel();
                    break;
                }
                completed = tasks.join_next() => match completed {
                    Some(Ok(Ok(()))) => {
                        runtime_error = Some(MicroserviceError::Transport("transport task ended unexpectedly".into()));
                        self.cancellation.cancel();
                        break;
                    }
                    Some(Ok(Err(error))) => {
                        runtime_error = Some(error);
                        self.cancellation.cancel();
                        break;
                    }
                    Some(Err(error)) => {
                        runtime_error = Some(MicroserviceError::Transport(format!("transport task panicked: {error}")));
                        self.cancellation.cancel();
                        break;
                    }
                    None => break,
                },
            }
        }
        if timeout(self.options.shutdown_timeout, async {
            while tasks.join_next().await.is_some() {}
        })
        .await
        .is_err()
        {
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            runtime_error.get_or_insert_with(|| {
                MicroserviceError::Transport(
                    "transport shutdown deadline elapsed; unfinished handlers were aborted".into(),
                )
            });
        }
        if let Some(error) = runtime_error {
            let _ = self.shutdown().await;
            Err(error)
        } else {
            self.shutdown().await
        }
    }

    /// Signals subscriptions to stop, drains NATS, and shuts down providers once.
    pub async fn shutdown(&self) -> Result<(), MicroserviceError> {
        let _start_guard = self.start_lock.lock().await;
        {
            let mut shutdown = self
                .shutdown
                .lock()
                .expect("microservice shutdown lock poisoned");
            if *shutdown {
                return Ok(());
            }
            *shutdown = true;
        }
        self.cancellation.cancel();
        let mut tasks = {
            let mut guard = self.tasks.lock().expect("microservice task lock poisoned");
            std::mem::replace(&mut *guard, JoinSet::new())
        };
        let mut runtime_error = None;
        if timeout(self.options.shutdown_timeout, async {
            while let Some(result) = tasks.join_next().await {
                if let Err(error) = task_result(result) {
                    runtime_error.get_or_insert(error);
                }
            }
        })
        .await
        .is_err()
        {
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            runtime_error.get_or_insert_with(|| {
                MicroserviceError::Transport(
                    "transport shutdown deadline elapsed; unfinished handlers were aborted".into(),
                )
            });
        }
        let module_error = shutdown_module::<M>(&self.container)
            .await
            .err()
            .map(|error| MicroserviceError::Framework(error.message));
        let drain_error = self
            .client
            .drain()
            .await
            .err()
            .map(|error| MicroserviceError::Transport(error.to_string()));
        runtime_error
            .or(module_error)
            .or(drain_error)
            .map_or(Ok(()), Err)
    }

    /// Requests a graceful shutdown from another task or signal handler.
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    /// Returns the shared dependency container.
    pub fn container(&self) -> Arc<Container> {
        self.container.clone()
    }
}

async fn consume_command(
    mut subscriber: async_nats::Subscriber,
    handler: MessageHandlerDef,
    container: Arc<Container>,
    client: Client,
    options: NatsTransportOptions,
    cancellation: CancellationToken,
) -> Result<(), MicroserviceError> {
    let concurrency = options.max_handler_concurrency;
    let mut in_flight = JoinSet::new();
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => {
                return wait_for_handler_tasks(&mut in_flight).await;
            }
            completed = in_flight.join_next(), if !in_flight.is_empty() => {
                task_result(completed.expect("non-empty handler task set returned no result"))?;
            }
            next = subscriber.next(), if in_flight.len() < concurrency => {
                let Some(message) = next else { return Err(MicroserviceError::Transport("NATS command subscription ended unexpectedly".into())) };
                in_flight.spawn(process_command(message, handler.clone(), container.clone(), client.clone(), options.clone(), cancellation.child_token()));
            }
        }
    }
}

async fn process_command(
    message: Message,
    handler: MessageHandlerDef,
    container: Arc<Container>,
    client: Client,
    options: NatsTransportOptions,
    cancellation: CancellationToken,
) -> Result<(), MicroserviceError> {
    let response = dispatch_command(
        &message,
        &handler,
        &container,
        cancellation,
        options.max_request_bytes,
        options.max_response_bytes,
    )
    .await;
    if let (Some(reply), Ok(bytes)) = (&message.reply, response) {
        client
            .publish(reply.clone(), bytes.into())
            .await
            .map_err(|error| MicroserviceError::Transport(error.to_string()))?;
    }
    Ok(())
}

async fn dispatch_command(
    message: &Message,
    handler: &MessageHandlerDef,
    container: &Container,
    cancellation: CancellationToken,
    maximum_request_bytes: usize,
    maximum_response_bytes: usize,
) -> Result<Vec<u8>, MicroserviceClientError> {
    if message.payload.len() > maximum_request_bytes {
        if let Some(correlation_id) = correlation_id_from_headers(message) {
            return bounded_command_error(
                correlation_id,
                "Payload Too Large",
                "request envelope exceeds configured maximum",
                maximum_response_bytes,
            )
            .map_err(MicroserviceClientError::Protocol);
        }
        return Err(MicroserviceClientError::Protocol(
            "request envelope exceeds configured maximum".into(),
        ));
    }
    let request: RequestEnvelope = serde_json::from_slice(&message.payload)
        .map_err(|error| MicroserviceClientError::Decode(error.to_string()))?;
    if request.correlation_id.is_empty() {
        return Err(MicroserviceClientError::Protocol(
            "request correlation ID is missing".into(),
        ));
    }
    if request.version != PROTOCOL_VERSION {
        return bounded_command_error(
            &request.correlation_id,
            "Protocol",
            "unsupported request protocol version",
            maximum_response_bytes,
        )
        .map_err(MicroserviceClientError::Protocol);
    }
    if request
        .deadline_unix_millis
        .is_some_and(|deadline| deadline < now_millis())
    {
        return bounded_command_error(
            &request.correlation_id,
            "Deadline Exceeded",
            "request deadline elapsed before handler invocation",
            maximum_response_bytes,
        )
        .map_err(MicroserviceClientError::Protocol);
    }
    let context = MessageContext::new(
        message.subject.to_string(),
        propagated_headers(request.headers, message.headers.as_ref()),
        Some(request.correlation_id.clone()),
        request.deadline_unix_millis.map(unix_millis_to_system_time),
        cancellation,
        None,
        None,
    );
    let body = match handler.invoke(container, context, request.payload).await {
        Ok(response) => ResponseBody::Success {
            payload: response.unwrap_or(Value::Null),
        },
        Err(error) => ResponseBody::Error(remote_from_exception(error).into_remote()),
    };
    let correlation_id = request.correlation_id;
    encode_limited(
        &ResponseEnvelope {
            version: PROTOCOL_VERSION,
            correlation_id: correlation_id.clone(),
            body,
        },
        maximum_response_bytes,
    )
    .or_else(|_| {
        bounded_command_error(
            &correlation_id,
            "Protocol",
            "response envelope exceeds configured maximum",
            maximum_response_bytes,
        )
    })
    .map_err(MicroserviceClientError::Protocol)
}

fn bounded_command_error(
    correlation_id: &str,
    code: &str,
    message: &str,
    maximum_response_bytes: usize,
) -> Result<Vec<u8>, String> {
    encode_limited(
        &ResponseEnvelope {
            version: PROTOCOL_VERSION,
            correlation_id: correlation_id.to_owned(),
            body: ResponseBody::Error(RemoteError {
                code: code.into(),
                message: message.into(),
                details: None,
                retryable: false,
            }),
        },
        maximum_response_bytes,
    )
}

async fn consume_event(
    mut messages: async_nats::jetstream::consumer::pull::Stream,
    handler: MessageHandlerDef,
    container: Arc<Container>,
    client: Client,
    options: NatsTransportOptions,
    cancellation: CancellationToken,
) -> Result<(), MicroserviceError> {
    let concurrency = options.max_handler_concurrency;
    let mut in_flight = JoinSet::new();
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => {
                return wait_for_handler_tasks(&mut in_flight).await;
            }
            completed = in_flight.join_next(), if !in_flight.is_empty() => {
                task_result(completed.expect("non-empty handler task set returned no result"))?;
            }
            next = messages.next(), if in_flight.len() < concurrency => {
                let Some(message) = next else { return Err(MicroserviceError::Transport("JetStream event consumer ended unexpectedly".into())) };
                let message = message.map_err(|error| MicroserviceError::Transport(error.to_string()))?;
                in_flight.spawn(process_event(message, handler.clone(), container.clone(), client.clone(), options.clone(), cancellation.child_token()));
            }
        }
    }
}

async fn process_event(
    message: async_nats::jetstream::Message,
    handler: MessageHandlerDef,
    container: Arc<Container>,
    client: Client,
    options: NatsTransportOptions,
    cancellation: CancellationToken,
) -> Result<(), MicroserviceError> {
    let info = message
        .info()
        .map_err(|error| MicroserviceError::Transport(error.to_string()))?;
    let delivery = MessageDelivery {
        stream: Some(info.stream.into()),
        consumer: Some(info.consumer.into()),
        attempt: info.delivered.max(0) as u64,
    };
    if message.payload.len() > options.max_event_bytes {
        message
            .ack_with(async_nats::jetstream::AckKind::Term)
            .await
            .map_err(|error| MicroserviceError::Transport(error.to_string()))?;
        return Ok(());
    }
    let event: EventEnvelope = match serde_json::from_slice::<EventEnvelope>(&message.payload) {
        Ok(event) if event.version == PROTOCOL_VERSION && !event.event_id.is_empty() => event,
        _ => {
            message
                .ack_with(async_nats::jetstream::AckKind::Term)
                .await
                .map_err(|error| MicroserviceError::Transport(error.to_string()))?;
            return Ok(());
        }
    };
    let context = MessageContext::new(
        message.subject.to_string(),
        propagated_headers(event.headers, message.headers.as_ref()),
        None,
        None,
        cancellation,
        Some(delivery),
        Some(event.event_id.clone()),
    );
    let progress_interval = options.event_ack_wait / 2;
    let invocation = handler.invoke(&container, context, event.payload.clone());
    tokio::pin!(invocation);
    let result = loop {
        tokio::select! {
            result = &mut invocation => break result,
            _ = tokio::time::sleep(progress_interval) => {
                message
                    .ack_with(async_nats::jetstream::AckKind::Progress)
                    .await
                    .map_err(|error| MicroserviceError::Transport(error.to_string()))?;
            }
        }
    };
    match result {
        Ok(_) => message
            .ack()
            .await
            .map_err(|error| MicroserviceError::Transport(error.to_string()))?,
        Err(error) if error.status.is_client_error() => message
            .ack_with(async_nats::jetstream::AckKind::Term)
            .await
            .map_err(|error| MicroserviceError::Transport(error.to_string()))?,
        Err(error) if info.delivered >= options.max_event_deliveries => {
            if let Some(subject) = &options.dead_letter_subject {
                let dead_letter = serde_json::json!({"subject": message.subject.to_string(), "event_id": event.event_id, "payload": event.payload, "stream": info.stream, "consumer": info.consumer, "deliveries": info.delivered, "error": remote_from_exception(error).into_remote()});
                let published = async {
                    let bytes =
                        serde_json::to_vec(&dead_letter).map_err(|error| error.to_string())?;
                    let acknowledgement = async_nats::jetstream::new(client.clone())
                        .publish(subject.clone(), bytes.into())
                        .await
                        .map_err(|error| error.to_string())?;
                    acknowledgement.await.map_err(|error| error.to_string())
                }
                .await;
                if published.is_err() {
                    message
                        .ack_with(async_nats::jetstream::AckKind::Nak(Some(
                            options.event_retry_delay,
                        )))
                        .await
                        .map_err(|error| MicroserviceError::Transport(error.to_string()))?;
                    return Ok(());
                }
            }
            message
                .ack_with(async_nats::jetstream::AckKind::Term)
                .await
                .map_err(|error| MicroserviceError::Transport(error.to_string()))?;
        }
        Err(_) => message
            .ack_with(async_nats::jetstream::AckKind::Nak(Some(
                options.event_retry_delay,
            )))
            .await
            .map_err(|error| MicroserviceError::Transport(error.to_string()))?,
    }
    Ok(())
}

async fn wait_for_handler_tasks(
    tasks: &mut JoinSet<Result<(), MicroserviceError>>,
) -> Result<(), MicroserviceError> {
    while let Some(result) = tasks.join_next().await {
        task_result(result)?;
    }
    Ok(())
}

fn task_result(
    result: Result<Result<(), MicroserviceError>, tokio::task::JoinError>,
) -> Result<(), MicroserviceError> {
    match result {
        Ok(result) => result,
        Err(error) => Err(MicroserviceError::Transport(format!(
            "transport task panicked: {error}"
        ))),
    }
}

fn durable_name(service_name: &str, subject: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(service_name.as_bytes());
    hash.update([0]);
    hash.update(subject.as_bytes());
    format!("caelix-{:x}", hash.finalize())
}

fn nats_pattern_matches(pattern: &str, subject: &str) -> bool {
    let pattern: Vec<_> = pattern.split('.').collect();
    let subject: Vec<_> = subject.split('.').collect();
    let mut index = 0;
    while index < pattern.len() {
        match pattern[index] {
            ">" => return index < subject.len(),
            "*" if index < subject.len() => {}
            token if subject.get(index).is_some_and(|value| *value == token) => {}
            _ => return false,
        }
        index += 1;
    }
    index == subject.len()
}

fn nats_pattern_covers(configured: &str, declared: &str) -> bool {
    let configured: Vec<_> = configured.split('.').collect();
    let declared: Vec<_> = declared.split('.').collect();
    let mut index = 0;
    loop {
        match (configured.get(index).copied(), declared.get(index).copied()) {
            (None, None) => return true,
            (Some(">"), Some(_)) => return true,
            (Some(">"), None) => return false,
            (None, _) => return false,
            (_, Some(">")) => return false,
            (Some("*"), Some(_)) => index += 1,
            (Some(configured), Some(declared)) if configured == declared => index += 1,
            _ => return false,
        }
    }
}

fn valid_nats_subject(subject: &str) -> bool {
    !subject.is_empty()
        && !subject.chars().any(char::is_whitespace)
        && subject
            .split('.')
            .all(|token| !token.is_empty() && token != "*" && token != ">")
}

fn remote_from_exception(error: caelix_core::HttpException) -> MicroserviceClientError {
    let client_error = error.status.is_client_error();
    MicroserviceClientError::Remote(RemoteError {
        code: error.error.to_string(),
        message: if client_error {
            error.message
        } else {
            "Internal Server Error".into()
        },
        details: None,
        retryable: error.status.is_server_error(),
    })
}

trait IntoRemoteError {
    fn into_remote(self) -> RemoteError;
}

impl IntoRemoteError for MicroserviceClientError {
    fn into_remote(self) -> RemoteError {
        match self {
            Self::Remote(error) => error,
            _ => RemoteError {
                code: "Internal Server Error".into(),
                message: "Internal Server Error".into(),
                details: None,
                retryable: false,
            },
        }
    }
}

fn request_transport_error(error: async_nats::RequestError) -> MicroserviceClientError {
    match error.kind() {
        RequestErrorKind::TimedOut => MicroserviceClientError::Timeout,
        RequestErrorKind::NoResponders => MicroserviceClientError::NoResponder,
        RequestErrorKind::Other => MicroserviceClientError::Transport(error.to_string()),
    }
}

fn encode_limited<T: Serialize>(value: &T, maximum: usize) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    if bytes.len() > maximum {
        Err("envelope exceeds configured maximum".into())
    } else {
        Ok(bytes)
    }
}

fn caelix_headers(
    headers: &BTreeMap<String, String>,
    correlation_id: Option<&str>,
    deadline_unix_millis: Option<u64>,
) -> async_nats::HeaderMap {
    let mut result = async_nats::HeaderMap::new();
    for (name, value) in headers {
        result.insert(name.clone(), value.clone());
    }
    if let Some(correlation_id) = correlation_id {
        result.insert("Caelix-Correlation-Id", correlation_id);
    }
    if let Some(deadline_unix_millis) = deadline_unix_millis {
        result.insert(
            "Caelix-Deadline-Unix-Millis",
            deadline_unix_millis.to_string(),
        );
    }
    result
}

fn propagated_headers(
    mut envelope_headers: BTreeMap<String, String>,
    headers: Option<&async_nats::HeaderMap>,
) -> BTreeMap<String, String> {
    let Some(headers) = headers else {
        return envelope_headers;
    };
    for (name, values) in headers.iter() {
        if let Some(value) = values.last() {
            envelope_headers.insert(name.to_string(), value.to_string());
        }
    }
    envelope_headers
}

fn correlation_id_from_headers(message: &Message) -> Option<&str> {
    let correlation_id = message
        .headers
        .as_ref()?
        .get_last("Caelix-Correlation-Id")?
        .as_str();
    (correlation_id.len() <= 128 && !correlation_id.is_empty()).then_some(correlation_id)
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn deadline_millis(timeout: Duration) -> Option<u64> {
    now_millis().checked_add(timeout.as_millis() as u64)
}

fn unix_millis_to_system_time(value: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(value)
}

#[derive(Deserialize, Serialize)]
struct RequestEnvelope {
    version: u8,
    correlation_id: String,
    deadline_unix_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reply_channel: Option<String>,
    headers: BTreeMap<String, String>,
    payload: Value,
}

#[derive(Deserialize, Serialize)]
struct ResponseEnvelope {
    version: u8,
    correlation_id: String,
    body: ResponseBody,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ResponseBody {
    Success { payload: Value },
    Error(RemoteError),
}

#[derive(Deserialize, Serialize)]
struct EventEnvelope {
    version: u8,
    event_id: String,
    headers: BTreeMap<String, String>,
    payload: Value,
    published_at_unix_millis: u64,
}

#[cfg(test)]
mod tests {
    use super::{
        MicroserviceClientError, durable_name, nats_pattern_covers, nats_pattern_matches,
        remote_from_exception, valid_nats_subject,
    };
    use caelix_core::{HttpException, StatusCode};

    #[test]
    fn terminal_wildcards_require_a_trailing_subject_token() {
        assert!(!nats_pattern_matches("orders.>", "orders"));
        assert!(nats_pattern_matches("orders.>", "orders.created"));
    }

    #[test]
    fn stream_patterns_must_cover_the_full_declared_pattern() {
        assert!(!nats_pattern_covers("orders.*", "orders.>"));
        assert!(nats_pattern_covers("orders.>", "orders.*"));
    }

    #[test]
    fn durable_names_distinguish_raw_service_identities() {
        assert_ne!(
            durable_name("billing.v1", "orders.created"),
            durable_name("billingv1", "orders.created")
        );
    }

    #[test]
    fn dead_letter_subjects_cannot_contain_wildcards() {
        assert!(valid_nats_subject("caelix.dead-letter"));
        assert!(!valid_nats_subject("caelix.>"));
        assert!(!valid_nats_subject("caelix.*"));
    }

    #[test]
    fn server_failures_are_sanitized_before_crossing_the_transport() {
        let error = HttpException::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Database Error",
            "password=do-not-leak",
        );
        let MicroserviceClientError::Remote(remote) = remote_from_exception(error) else {
            panic!("expected a remote error");
        };
        assert_eq!(remote.message, "Internal Server Error");
        assert!(!remote.message.contains("password"));
        assert!(remote.retryable);
    }

    #[test]
    fn client_failures_preserve_only_the_public_message() {
        let error = HttpException::new(
            StatusCode::BAD_REQUEST,
            "Bad Request",
            "invalid public input",
        );
        let MicroserviceClientError::Remote(remote) = remote_from_exception(error) else {
            panic!("expected a remote error");
        };
        assert_eq!(remote.message, "invalid public input");
        assert!(!remote.retryable);
    }
}
