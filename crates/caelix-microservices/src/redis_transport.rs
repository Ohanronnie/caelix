use super::*;
use redis::{
    AsyncCommands,
    aio::ConnectionManager,
    streams::{
        StreamAutoClaimOptions, StreamAutoClaimReply, StreamPendingCountReply, StreamReadOptions,
        StreamReadReply,
    },
};
use tokio::time::{Instant, MissedTickBehavior};

type StreamEntry = (String, BTreeMap<String, redis::Value>);

#[derive(Serialize)]
struct DeadLetterEnvelope {
    version: u8,
    subject: String,
    event: EventEnvelope,
    source_stream: String,
    source_group: String,
    consumer: String,
    deliveries: u64,
    failed_at_unix_millis: u64,
    error: RemoteError,
}

/// Configuration for Redis Streams command and event delivery.
#[derive(Clone, Debug)]
pub struct RedisTransportOptions {
    pub(crate) server: String,
    pub(crate) service_name: Option<String>,
    pub(crate) event_stream: String,
    pub(crate) dead_letter_stream: Option<String>,
    pub(crate) rpc_timeout: Duration,
    pub(crate) max_request_bytes: usize,
    pub(crate) max_response_bytes: usize,
    pub(crate) max_event_bytes: usize,
    pub(crate) max_handler_concurrency: usize,
    pub(crate) shutdown_timeout: Duration,
    pub(crate) max_event_deliveries: u64,
    pub(crate) event_retry_delay: Duration,
    pub(crate) command_recovery_delay: Duration,
    pub(crate) approximate_max_event_entries: Option<usize>,
}

impl RedisTransportOptions {
    /// Creates Redis transport options for a Redis URL.
    pub fn new(server: impl Into<String>) -> Self {
        Self {
            server: server.into(),
            service_name: None,
            event_stream: "caelix:events".into(),
            dead_letter_stream: None,
            rpc_timeout: Duration::from_secs(5),
            max_request_bytes: 1024 * 1024,
            max_response_bytes: 1024 * 1024,
            max_event_bytes: 1024 * 1024,
            max_handler_concurrency: 64,
            shutdown_timeout: Duration::from_secs(10),
            max_event_deliveries: 5,
            event_retry_delay: Duration::from_secs(1),
            command_recovery_delay: Duration::from_secs(30),
            approximate_max_event_entries: None,
        }
    }
    /// Sets the stable service consumer-group name.
    pub fn service_name(mut self, value: impl Into<String>) -> Self {
        self.service_name = Some(value.into());
        self
    }
    /// Sets the shared event Stream key.
    pub fn event_stream(mut self, value: impl Into<String>) -> Self {
        self.event_stream = value.into();
        self
    }
    /// Sets the optional dead-letter Stream key.
    pub fn dead_letter_stream(mut self, value: impl Into<String>) -> Self {
        self.dead_letter_stream = Some(value.into());
        self
    }
    /// Sets the request/reply deadline.
    pub fn rpc_timeout(mut self, value: Duration) -> Self {
        self.rpc_timeout = value;
        self
    }
    /// Limits encoded request envelopes.
    pub fn max_request_bytes(mut self, value: usize) -> Self {
        self.max_request_bytes = value.max(1);
        self
    }
    /// Limits encoded response envelopes.
    pub fn max_response_bytes(mut self, value: usize) -> Self {
        self.max_response_bytes = value.max(MINIMUM_RESPONSE_ENVELOPE_BYTES);
        self
    }
    /// Limits encoded event envelopes.
    pub fn max_event_bytes(mut self, value: usize) -> Self {
        self.max_event_bytes = value.max(1);
        self
    }
    /// Bounds concurrently executing handlers.
    pub fn max_handler_concurrency(mut self, value: usize) -> Self {
        self.max_handler_concurrency = value.max(1);
        self
    }
    /// Sets the graceful shutdown deadline.
    pub fn shutdown_timeout(mut self, value: Duration) -> Self {
        self.shutdown_timeout = value;
        self
    }
    /// Sets the delivery attempt that dead-letters an event.
    pub fn max_event_deliveries(mut self, value: u64) -> Self {
        self.max_event_deliveries = value.max(1);
        self
    }
    /// Sets the idle interval before pending work is reclaimed.
    pub fn event_retry_delay(mut self, value: Duration) -> Self {
        self.event_retry_delay = value.max(Duration::from_millis(10));
        self
    }
    /// Sets how long an abandoned command remains idle before another replica recovers it.
    pub fn command_recovery_delay(mut self, value: Duration) -> Self {
        self.command_recovery_delay = value.max(Duration::from_millis(10));
        self
    }
    /// Opts into approximate event Stream trimming. Trimming may remove entries pending in another group.
    pub fn approximate_max_event_entries(mut self, value: usize) -> Self {
        self.approximate_max_event_entries = Some(value.max(1));
        self
    }

    fn validate_client(&self) -> Result<(), MicroserviceError> {
        if self.server.trim().is_empty() {
            return Err(MicroserviceError::Configuration(
                "Redis server URL must not be empty".into(),
            ));
        }
        if self.event_stream.trim().is_empty() {
            return Err(MicroserviceError::Configuration(
                "event_stream must not be empty".into(),
            ));
        }
        if self
            .dead_letter_stream
            .as_deref()
            .is_some_and(|stream| stream.trim().is_empty())
        {
            return Err(MicroserviceError::Configuration(
                "dead-letter Stream must not be empty".into(),
            ));
        }
        if self.dead_letter_stream.as_deref() == Some(self.event_stream.as_str()) {
            return Err(MicroserviceError::Configuration(
                "dead-letter Stream must differ from the event Stream".into(),
            ));
        }
        if self.rpc_timeout.is_zero() {
            return Err(MicroserviceError::Configuration(
                "rpc_timeout must be greater than zero".into(),
            ));
        }
        if self.shutdown_timeout.is_zero() {
            return Err(MicroserviceError::Configuration(
                "shutdown_timeout must be greater than zero".into(),
            ));
        }
        Ok(())
    }

    fn validate(&self, handlers: &[MessageHandlerDef]) -> Result<&str, MicroserviceError> {
        self.validate_client()?;
        let service = self
            .service_name
            .as_deref()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| {
                MicroserviceError::Configuration(
                    "service_name is required for Redis consumers".into(),
                )
            })?;
        if handlers.iter().any(|h| h.pattern.trim().is_empty()) {
            return Err(MicroserviceError::Configuration(
                "message patterns must not be empty".into(),
            ));
        }
        Ok(service)
    }
}

#[derive(Clone)]
pub(crate) struct RedisClient {
    client: redis::Client,
    connection: ConnectionManager,
    options: RedisTransportOptions,
}

impl RedisClient {
    pub(crate) async fn connect(options: RedisTransportOptions) -> Result<Self, MicroserviceError> {
        options.validate_client()?;
        let client = redis::Client::open(options.server.clone()).map_err(transport)?;
        let connection = client.get_connection_manager().await.map_err(transport)?;
        Ok(Self {
            client,
            connection,
            options,
        })
    }

    pub(crate) async fn request<P: Serialize + Send, R: DeserializeOwned + Send>(
        &self,
        subject: &str,
        payload: P,
    ) -> Result<R, MicroserviceClientError> {
        let started = Instant::now();
        timeout(
            self.options.rpc_timeout,
            self.request_inner(subject, payload, started),
        )
        .await
        .map_err(|_| MicroserviceClientError::Timeout)?
    }

    async fn request_inner<P: Serialize + Send, R: DeserializeOwned + Send>(
        &self,
        subject: &str,
        payload: P,
        started: Instant,
    ) -> Result<R, MicroserviceClientError> {
        if !valid_nats_subject(subject) {
            return Err(MicroserviceClientError::Protocol(
                "command subject must contain non-empty dot-separated tokens without wildcards"
                    .into(),
            ));
        }
        let correlation_id = Uuid::new_v4().to_string();
        let reply_channel = format!("caelix:reply:{correlation_id}");
        let mut pubsub = self
            .client
            .get_async_pubsub()
            .await
            .map_err(client_transport)?;
        pubsub
            .subscribe(&reply_channel)
            .await
            .map_err(client_transport)?;
        let request = RequestEnvelope {
            version: PROTOCOL_VERSION,
            correlation_id: correlation_id.clone(),
            deadline_unix_millis: deadline_millis(
                self.options.rpc_timeout.saturating_sub(started.elapsed()),
            ),
            reply_channel: Some(reply_channel),
            headers: BTreeMap::new(),
            payload: serde_json::to_value(payload)
                .map_err(|e| MicroserviceClientError::Decode(e.to_string()))?,
        };
        let bytes = encode_limited(&request, self.options.max_request_bytes)
            .map_err(MicroserviceClientError::Protocol)?;
        let stream = command_stream(subject);
        let mut connection = self.connection.clone();
        let _: String = redis::cmd("XADD")
            .arg(&stream)
            .arg("*")
            .arg("envelope")
            .arg(bytes)
            .query_async(&mut connection)
            .await
            .map_err(client_transport)?;
        let mut messages = pubsub.on_message();
        let message = messages.next().await.ok_or_else(|| {
            MicroserviceClientError::Transport("Redis reply subscription ended".into())
        })?;
        let bytes: Vec<u8> = message
            .get_payload()
            .map_err(|e| MicroserviceClientError::Decode(e.to_string()))?;
        if bytes.len() > self.options.max_response_bytes {
            return Err(MicroserviceClientError::Protocol(
                "response envelope exceeds configured maximum".into(),
            ));
        }
        let response: ResponseEnvelope = serde_json::from_slice(&bytes)
            .map_err(|e| MicroserviceClientError::Decode(e.to_string()))?;
        if response.version != PROTOCOL_VERSION {
            return Err(MicroserviceClientError::Protocol(
                "unsupported response protocol version".into(),
            ));
        }
        if response.correlation_id != correlation_id {
            return Err(MicroserviceClientError::Protocol(
                "response correlation ID does not match request".into(),
            ));
        }
        match response.body {
            ResponseBody::Success { payload } => serde_json::from_value(payload)
                .map_err(|e| MicroserviceClientError::Decode(e.to_string())),
            ResponseBody::Error(error) => Err(MicroserviceClientError::Remote(error)),
        }
    }

    pub(crate) async fn emit<P: Serialize + Send>(
        &self,
        subject: &str,
        payload: P,
    ) -> Result<(), MicroserviceClientError> {
        if !valid_nats_subject(subject) {
            return Err(MicroserviceClientError::Protocol(
                "event subject must contain non-empty dot-separated tokens without wildcards"
                    .into(),
            ));
        }
        let event = EventEnvelope {
            version: PROTOCOL_VERSION,
            event_id: Uuid::new_v4().to_string(),
            headers: BTreeMap::new(),
            payload: serde_json::to_value(payload)
                .map_err(|e| MicroserviceClientError::Decode(e.to_string()))?,
            published_at_unix_millis: now_millis(),
        };
        let bytes = encode_limited(&event, self.options.max_event_bytes)
            .map_err(MicroserviceClientError::Protocol)?;
        let mut connection = self.connection.clone();
        let mut command = redis::cmd("XADD");
        command.arg(&self.options.event_stream);
        if let Some(max) = self.options.approximate_max_event_entries {
            command.arg("MAXLEN").arg("~").arg(max);
        }
        let _: String = command
            .arg("*")
            .arg("subject")
            .arg(subject)
            .arg("envelope")
            .arg(bytes)
            .query_async(&mut connection)
            .await
            .map_err(client_transport)?;
        Ok(())
    }
}

pub(crate) struct RedisRuntime<M: Module + 'static> {
    container: Arc<Container>,
    client: RedisClient,
    handlers: Vec<MessageHandlerDef>,
    cancellation: CancellationToken,
    marker: std::marker::PhantomData<M>,
}

impl<M: Module + 'static> RedisRuntime<M> {
    pub(crate) async fn new(options: RedisTransportOptions) -> Result<Self, MicroserviceError> {
        options.validate(&[])?;
        let client = RedisClient::connect(options).await?;
        let injectable = MicroserviceClient {
            transport: ClientTransport::Redis(client.clone()),
        };
        let container =
            build_container_with_setup::<M>(|container| container.register_instance(injectable))
                .await
                .map_err(|e| MicroserviceError::Framework(e.message))?;
        let handlers = collect_module_message_handlers_with_container::<M>(Some(&container))
            .map_err(|e| MicroserviceError::Framework(e.message))?;
        client.options.validate(&handlers)?;
        Ok(Self {
            container: Arc::new(container),
            client,
            handlers,
            cancellation: CancellationToken::new(),
            marker: std::marker::PhantomData,
        })
    }
    pub(crate) fn container(&self) -> Arc<Container> {
        self.container.clone()
    }
    pub(crate) async fn run(self) -> Result<(), MicroserviceError> {
        let mut tasks = JoinSet::new();
        let runtime_result = async {
            let service = self.client.options.validate(&self.handlers)?.to_owned();
            let consumer = Uuid::new_v4().to_string();
            for handler in self
                .handlers
                .iter()
                .filter(|h| h.kind == MessageHandlerKind::Command)
                .cloned()
            {
                create_group(
                    &self.client.connection,
                    &command_stream(handler.pattern),
                    &command_group(&service, handler.pattern),
                )
                .await?;
                tasks.spawn(command_loop(
                    handler,
                    self.container.clone(),
                    self.client.clone(),
                    service.clone(),
                    consumer.clone(),
                    self.cancellation.child_token(),
                ));
            }
            if self
                .handlers
                .iter()
                .any(|h| h.kind == MessageHandlerKind::Event)
            {
                create_group(
                    &self.client.connection,
                    &self.client.options.event_stream,
                    &event_group(&service),
                )
                .await?;
                tasks.spawn(event_loop(
                    self.handlers.clone(),
                    self.container.clone(),
                    self.client.clone(),
                    service,
                    consumer,
                    self.cancellation.child_token(),
                ));
            }
            tokio::select! {
                signal = tokio::signal::ctrl_c() => signal.map_err(transport),
                result = tasks.join_next(), if !tasks.is_empty() => match result {
                    Some(Ok(Ok(()))) => Err(MicroserviceError::Transport("Redis transport task ended unexpectedly".into())),
                    Some(Ok(Err(error))) => Err(error),
                    Some(Err(error)) => Err(MicroserviceError::Transport(format!("Redis transport task panicked: {error}"))),
                    None => Ok(()),
                },
            }
        }
        .await;
        self.cancellation.cancel();
        let task_shutdown =
            shutdown_redis_tasks(&mut tasks, self.client.options.shutdown_timeout).await;
        let module_shutdown = shutdown_module::<M>(&self.container)
            .await
            .map_err(|e| MicroserviceError::Framework(e.message));
        runtime_result.and(task_shutdown).and(module_shutdown)
    }
}

async fn shutdown_redis_tasks(
    tasks: &mut JoinSet<Result<(), MicroserviceError>>,
    deadline: Duration,
) -> Result<(), MicroserviceError> {
    if timeout(deadline, async {
        while tasks.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        Err(MicroserviceError::Transport(
            "Redis transport shutdown deadline elapsed; unfinished handlers were aborted".into(),
        ))
    } else {
        Ok(())
    }
}

async fn create_group(
    connection: &ConnectionManager,
    stream: &str,
    group: &str,
) -> Result<(), MicroserviceError> {
    let mut connection = connection.clone();
    let result: redis::RedisResult<()> = redis::cmd("XGROUP")
        .arg("CREATE")
        .arg(stream)
        .arg(group)
        .arg("0")
        .arg("MKSTREAM")
        .query_async(&mut connection)
        .await;
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.to_string().contains("BUSYGROUP") => Ok(()),
        Err(e) => Err(transport(e)),
    }
}

async fn command_loop(
    handler: MessageHandlerDef,
    container: Arc<Container>,
    client: RedisClient,
    service: String,
    consumer: String,
    cancellation: CancellationToken,
) -> Result<(), MicroserviceError> {
    let stream = command_stream(handler.pattern);
    let group = command_group(&service, handler.pattern);
    // Blocking Stream reads require a dedicated connection. Sharing the
    // multiplexed command manager would stall replies, acknowledgements, and
    // heartbeats for every clone while XREADGROUP is blocked.
    let subscriber = client
        .client
        .get_connection_manager()
        .await
        .map_err(transport)?;
    let concurrency = client.options.max_handler_concurrency;
    let mut in_flight = JoinSet::new();
    while !cancellation.is_cancelled() {
        while let Some(result) = in_flight.try_join_next() {
            task_result(result)?;
        }
        if in_flight.len() >= concurrency {
            tokio::select! {
                _ = cancellation.cancelled() => break,
                result = in_flight.join_next() => {
                    task_result(result.expect("non-empty Redis command task set returned no result"))?;
                }
            }
            continue;
        }
        let capacity = concurrency - in_flight.len();
        let mut entries = reclaim(
            &subscriber,
            &stream,
            &group,
            &consumer,
            client.options.command_recovery_delay,
            capacity,
        )
        .await?;
        if entries.is_empty() {
            entries = read_group(&subscriber, &stream, &group, &consumer, capacity).await?;
        }
        for (id, fields) in entries {
            in_flight.spawn(process_command_entry(
                handler.clone(),
                container.clone(),
                client.clone(),
                stream.clone(),
                group.clone(),
                consumer.clone(),
                id,
                fields,
                cancellation.child_token(),
            ));
        }
    }
    wait_for_handler_tasks(&mut in_flight).await
}

async fn process_command_entry(
    handler: MessageHandlerDef,
    container: Arc<Container>,
    client: RedisClient,
    stream: String,
    group: String,
    consumer: String,
    id: String,
    fields: BTreeMap<String, redis::Value>,
    cancellation: CancellationToken,
) -> Result<(), MicroserviceError> {
    let bytes: Vec<u8> = match field(&fields, "envelope") {
        Ok(bytes) => bytes,
        Err(_) => return ack_delete(&client.connection, &stream, &group, &id).await,
    };
    if bytes.len() > client.options.max_request_bytes {
        return ack_delete(&client.connection, &stream, &group, &id).await;
    }
    let request: RequestEnvelope = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => {
            ack_delete(&client.connection, &stream, &group, &id).await?;
            return Ok(());
        }
    };
    if request.correlation_id.is_empty()
        || request.correlation_id.len() > 128
        || request
            .reply_channel
            .as_deref()
            .is_none_or(|channel| channel.is_empty() || channel.len() > 256)
    {
        return ack_delete(&client.connection, &stream, &group, &id).await;
    }
    if request.version != PROTOCOL_VERSION {
        return publish_command_error_and_complete(
            &client,
            &stream,
            &group,
            &id,
            request,
            "Protocol",
            "unsupported request protocol version",
        )
        .await;
    }
    if request.deadline_unix_millis.is_some_and(deadline_elapsed) {
        return publish_command_error_and_complete(
            &client,
            &stream,
            &group,
            &id,
            request,
            "Deadline Exceeded",
            "request deadline elapsed before handler invocation",
        )
        .await;
    }
    let context = MessageContext::new(
        handler.pattern,
        request.headers.clone(),
        Some(request.correlation_id.clone()),
        request.deadline_unix_millis.map(unix_millis_to_system_time),
        cancellation,
        None,
        None,
    );
    let invocation = handler.invoke(&container, context, request.payload.clone());
    tokio::pin!(invocation);
    let mut heartbeat = pending_heartbeat(client.options.command_recovery_delay);
    let body = match loop {
        tokio::select! {
            result = &mut invocation => break result,
            _ = heartbeat.tick() => {
                touch_pending(&client.connection, &stream, &group, &consumer, &id).await?;
            }
        }
    } {
        Ok(payload) => ResponseBody::Success {
            payload: payload.unwrap_or(Value::Null),
        },
        Err(error) => ResponseBody::Error(remote_from_exception(error).into_remote()),
    };
    publish_command_response_and_complete(&client, &stream, &group, &id, request, body).await
}

async fn publish_command_error_and_complete(
    client: &RedisClient,
    stream: &str,
    group: &str,
    id: &str,
    request: RequestEnvelope,
    code: &str,
    message: &str,
) -> Result<(), MicroserviceError> {
    publish_command_response_and_complete(
        client,
        stream,
        group,
        id,
        request,
        ResponseBody::Error(RemoteError {
            code: code.into(),
            message: message.into(),
            details: None,
            retryable: false,
        }),
    )
    .await
}

async fn publish_command_response_and_complete(
    client: &RedisClient,
    stream: &str,
    group: &str,
    id: &str,
    request: RequestEnvelope,
    body: ResponseBody,
) -> Result<(), MicroserviceError> {
    let correlation_id = request.correlation_id;
    let bytes = encode_limited(
        &ResponseEnvelope {
            version: PROTOCOL_VERSION,
            correlation_id: correlation_id.clone(),
            body,
        },
        client.options.max_response_bytes,
    )
    .or_else(|_| {
        bounded_command_error(
            &correlation_id,
            "Protocol",
            "response envelope exceeds configured maximum",
            client.options.max_response_bytes,
        )
    });
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => {
            let completion_result = ack_delete(&client.connection, stream, group, id).await;
            return completion_result.and(Err(MicroserviceError::Configuration(error)));
        }
    };
    let publish_result = if let Some(channel) = request.reply_channel {
        let mut connection = client.connection.clone();
        connection
            .publish::<_, _, usize>(channel, bytes)
            .await
            .map(|_| ())
            .map_err(transport)
    } else {
        Ok(())
    };
    // Once the user handler has returned, this command is complete. Even a
    // failed or subscriber-less reply must never cause the handler to run again.
    let completion_result = ack_delete(&client.connection, stream, group, id).await;
    completion_result.and(publish_result)
}

async fn event_loop(
    handlers: Vec<MessageHandlerDef>,
    container: Arc<Container>,
    client: RedisClient,
    service: String,
    consumer: String,
    cancellation: CancellationToken,
) -> Result<(), MicroserviceError> {
    let group = event_group(&service);
    let stream = client.options.event_stream.clone();
    let subscriber = client
        .client
        .get_connection_manager()
        .await
        .map_err(transport)?;
    let concurrency = client.options.max_handler_concurrency;
    let mut in_flight = JoinSet::new();
    while !cancellation.is_cancelled() {
        while let Some(result) = in_flight.try_join_next() {
            task_result(result)?;
        }
        if in_flight.len() >= concurrency {
            tokio::select! {
                _ = cancellation.cancelled() => break,
                result = in_flight.join_next() => {
                    task_result(result.expect("non-empty Redis event task set returned no result"))?;
                }
            }
            continue;
        }
        let capacity = concurrency - in_flight.len();
        let mut entries = reclaim(
            &subscriber,
            &stream,
            &group,
            &consumer,
            client.options.event_retry_delay,
            capacity,
        )
        .await?;
        if entries.is_empty() {
            entries = read_group(&subscriber, &stream, &group, &consumer, capacity).await?;
        }
        for (id, fields) in entries {
            in_flight.spawn(process_event_entry(
                handlers.clone(),
                container.clone(),
                client.clone(),
                stream.clone(),
                group.clone(),
                consumer.clone(),
                id,
                fields,
                cancellation.child_token(),
            ));
        }
    }
    wait_for_handler_tasks(&mut in_flight).await
}

#[allow(clippy::too_many_arguments)]
async fn process_event_entry(
    handlers: Vec<MessageHandlerDef>,
    container: Arc<Container>,
    client: RedisClient,
    stream: String,
    group: String,
    consumer: String,
    id: String,
    fields: BTreeMap<String, redis::Value>,
    cancellation: CancellationToken,
) -> Result<(), MicroserviceError> {
    let subject: String = match field(&fields, "subject") {
        Ok(subject) => subject,
        Err(_) => return ack(&client.connection, &stream, &group, &id).await,
    };
    let bytes: Vec<u8> = match field(&fields, "envelope") {
        Ok(bytes) => bytes,
        Err(_) => return ack(&client.connection, &stream, &group, &id).await,
    };
    if bytes.len() > client.options.max_event_bytes {
        return ack(&client.connection, &stream, &group, &id).await;
    }
    let event: EventEnvelope = match serde_json::from_slice::<EventEnvelope>(&bytes) {
        Ok(event) if event.version == PROTOCOL_VERSION && !event.event_id.is_empty() => event,
        _ => return ack(&client.connection, &stream, &group, &id).await,
    };
    let Some(handler) = handlers.into_iter().find(|handler| {
        handler.kind == MessageHandlerKind::Event && nats_pattern_matches(handler.pattern, &subject)
    }) else {
        return ack(&client.connection, &stream, &group, &id).await;
    };
    let attempt = pending_attempt(&client.connection, &stream, &group, &id).await?;
    let delivery = MessageDelivery {
        stream: Some(stream.clone()),
        consumer: Some(consumer.clone()),
        attempt,
    };
    let context = MessageContext::new(
        subject.clone(),
        event.headers.clone(),
        None,
        None,
        cancellation,
        Some(delivery),
        Some(event.event_id.clone()),
    );
    let invocation = handler.invoke(&container, context, event.payload.clone());
    tokio::pin!(invocation);
    let mut heartbeat = pending_heartbeat(client.options.event_retry_delay);
    let result = loop {
        tokio::select! {
            result = &mut invocation => break result,
            _ = heartbeat.tick() => {
                touch_pending(&client.connection, &stream, &group, &consumer, &id).await?;
            }
        }
    };
    match result {
        Ok(_) => ack(&client.connection, &stream, &group, &id).await,
        Err(error) if error.status.is_client_error() => {
            ack(&client.connection, &stream, &group, &id).await
        }
        Err(error) if attempt >= client.options.max_event_deliveries => {
            if let Some(dead_letter_stream) = &client.options.dead_letter_stream {
                let dead_letter = DeadLetterEnvelope {
                    version: PROTOCOL_VERSION,
                    subject,
                    event,
                    source_stream: stream.clone(),
                    source_group: group.clone(),
                    consumer,
                    deliveries: attempt,
                    failed_at_unix_millis: now_millis(),
                    error: remote_from_exception(error).into_remote(),
                };
                let failure = serde_json::to_vec(&dead_letter)
                    .map_err(|error| MicroserviceError::Transport(error.to_string()))?;
                let mut connection = client.connection.clone();
                let publication: redis::RedisResult<String> = redis::cmd("XADD")
                    .arg(dead_letter_stream)
                    .arg("*")
                    .arg("envelope")
                    .arg(failure)
                    .query_async(&mut connection)
                    .await;
                if publication.is_err() {
                    // Preserve the original pending event so DLQ publication can
                    // be retried after the broker connection recovers.
                    return Ok(());
                }
            }
            ack(&client.connection, &stream, &group, &id).await
        }
        Err(_) => Ok(()),
    }
}

async fn read_group(
    connection: &ConnectionManager,
    stream: &str,
    group: &str,
    consumer: &str,
    count: usize,
) -> Result<Vec<StreamEntry>, MicroserviceError> {
    let mut connection = connection.clone();
    let options = StreamReadOptions::default()
        .group(group, consumer)
        .count(count)
        .block(250);
    let reply: StreamReadReply = connection
        .xread_options(&[stream], &[">"], &options)
        .await
        .map_err(transport)?;
    Ok(reply
        .keys
        .into_iter()
        .flat_map(|key| {
            key.ids
                .into_iter()
                .map(|id| (id.id, id.map.into_iter().collect()))
        })
        .collect())
}

async fn reclaim(
    connection: &ConnectionManager,
    stream: &str,
    group: &str,
    consumer: &str,
    idle: Duration,
    count: usize,
) -> Result<Vec<StreamEntry>, MicroserviceError> {
    let mut connection = connection.clone();
    let reply: StreamAutoClaimReply = connection
        .xautoclaim_options(
            stream,
            group,
            consumer,
            idle.as_millis() as usize,
            "0-0",
            StreamAutoClaimOptions::default().count(count),
        )
        .await
        .map_err(transport)?;
    Ok(reply
        .claimed
        .into_iter()
        .map(|id| (id.id, id.map.into_iter().collect()))
        .collect())
}

fn pending_heartbeat(reclaim_after: Duration) -> tokio::time::Interval {
    let interval = (reclaim_after / 2).max(Duration::from_millis(1));
    let mut heartbeat = tokio::time::interval(interval);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    heartbeat
}

async fn touch_pending(
    connection: &ConnectionManager,
    stream: &str,
    group: &str,
    consumer: &str,
    id: &str,
) -> Result<(), MicroserviceError> {
    let mut connection = connection.clone();
    let _: Vec<String> = redis::cmd("XCLAIM")
        .arg(stream)
        .arg(group)
        .arg(consumer)
        .arg(0)
        .arg(id)
        .arg("IDLE")
        .arg(0)
        .arg("JUSTID")
        .query_async(&mut connection)
        .await
        .map_err(transport)?;
    Ok(())
}

async fn pending_attempt(
    connection: &ConnectionManager,
    stream: &str,
    group: &str,
    id: &str,
) -> Result<u64, MicroserviceError> {
    let mut connection = connection.clone();
    let reply: StreamPendingCountReply = connection
        .xpending_count(stream, group, id, id, 1)
        .await
        .map_err(transport)?;
    Ok(reply.ids.first().map_or(1, |pending| {
        delivery_attempt(pending.times_delivered as u64)
    }))
}

fn delivery_attempt(times_delivered: u64) -> u64 {
    times_delivered.max(1)
}

fn deadline_elapsed(deadline_unix_millis: u64) -> bool {
    deadline_unix_millis <= now_millis()
}

async fn ack(
    connection: &ConnectionManager,
    stream: &str,
    group: &str,
    id: &str,
) -> Result<(), MicroserviceError> {
    let mut c = connection.clone();
    let _: usize = c.xack(stream, group, &[id]).await.map_err(transport)?;
    Ok(())
}
async fn ack_delete(
    connection: &ConnectionManager,
    stream: &str,
    group: &str,
    id: &str,
) -> Result<(), MicroserviceError> {
    let mut c = connection.clone();
    let _: () = redis::pipe()
        .atomic()
        .cmd("XACK")
        .arg(stream)
        .arg(group)
        .arg(id)
        .ignore()
        .cmd("XDEL")
        .arg(stream)
        .arg(id)
        .ignore()
        .query_async(&mut c)
        .await
        .map_err(transport)?;
    Ok(())
}
fn field<T: redis::FromRedisValue>(
    fields: &BTreeMap<String, redis::Value>,
    name: &str,
) -> Result<T, MicroserviceError> {
    fields
        .get(name)
        .ok_or_else(|| {
            MicroserviceError::Transport(format!("Redis Stream entry is missing `{name}`"))
        })
        .and_then(|v| redis::from_redis_value(v.clone()).map_err(transport))
}
fn command_stream(subject: &str) -> String {
    format!("caelix:commands:{}", stable_name(subject))
}
fn command_group(service: &str, subject: &str) -> String {
    format!(
        "caelix-command-{}-{}",
        stable_name(service),
        stable_name(subject)
    )
}
fn event_group(service: &str) -> String {
    format!("caelix-events-{}", stable_name(service))
}
fn stable_name(value: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(value.as_bytes());
    format!("{:x}", hash.finalize())[..24].into()
}
fn transport(error: impl fmt::Display) -> MicroserviceError {
    MicroserviceError::Transport(error.to_string())
}
fn client_transport(error: impl fmt::Display) -> MicroserviceClientError {
    MicroserviceClientError::Transport(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use caelix_core::{BadRequestException, HttpException, StatusCode};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tokio::sync::Notify;

    fn redis_test_url() -> Option<String> {
        std::env::var("CAELIX_REDIS_TEST_URL").ok()
    }

    fn unique_subject(label: &str) -> &'static str {
        Box::leak(format!("tests.{label}.{}", Uuid::new_v4().simple()).into_boxed_str())
    }

    async fn test_client(server: String, service: &str, event_stream: &str) -> RedisClient {
        RedisClient::connect(
            RedisTransportOptions::new(server)
                .service_name(service)
                .event_stream(event_stream)
                .rpc_timeout(Duration::from_millis(750))
                .event_retry_delay(Duration::from_millis(25))
                .command_recovery_delay(Duration::from_millis(25)),
        )
        .await
        .unwrap()
    }

    async fn wait_until(mut predicate: impl FnMut() -> bool) {
        timeout(Duration::from_secs(3), async {
            while !predicate() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("condition did not become true before the broker-test deadline");
    }

    async fn append_request(
        client: &RedisClient,
        subject: &str,
        correlation_id: &str,
        deadline_unix_millis: Option<u64>,
    ) -> String {
        let envelope = serde_json::to_vec(&RequestEnvelope {
            version: PROTOCOL_VERSION,
            correlation_id: correlation_id.into(),
            deadline_unix_millis,
            reply_channel: Some(format!("caelix:reply:{correlation_id}")),
            headers: BTreeMap::new(),
            payload: serde_json::json!({"correlation_id": correlation_id}),
        })
        .unwrap();
        let mut connection = client.connection.clone();
        redis::cmd("XADD")
            .arg(command_stream(subject))
            .arg("*")
            .arg("envelope")
            .arg(envelope)
            .query_async(&mut connection)
            .await
            .unwrap()
    }

    async fn append_event(client: &RedisClient, subject: &str, payload: Value) -> String {
        let envelope = serde_json::to_vec(&EventEnvelope {
            version: PROTOCOL_VERSION,
            event_id: Uuid::new_v4().to_string(),
            headers: BTreeMap::new(),
            payload,
            published_at_unix_millis: now_millis(),
        })
        .unwrap();
        let mut connection = client.connection.clone();
        redis::cmd("XADD")
            .arg(&client.options.event_stream)
            .arg("*")
            .arg("subject")
            .arg(subject)
            .arg("envelope")
            .arg(envelope)
            .query_async(&mut connection)
            .await
            .unwrap()
    }

    async fn delete_keys(client: &RedisClient, keys: &[&str]) {
        let mut connection = client.connection.clone();
        let _: usize = connection.del(keys).await.unwrap();
    }

    async fn pending_entries(client: &RedisClient, stream: &str, group: &str) -> usize {
        let mut connection = client.connection.clone();
        let reply: StreamPendingCountReply = connection
            .xpending_count(stream, group, "-", "+", 100)
            .await
            .unwrap();
        reply.ids.len()
    }

    #[test]
    fn names_are_deterministic_and_separated() {
        assert_eq!(command_stream("users.get"), command_stream("users.get"));
        assert_ne!(command_group("a", "x"), command_group("b", "x"));
        assert_ne!(command_stream("users.get"), command_stream("users.list"));
        assert_ne!(event_group("users"), event_group("billing"));
    }

    #[test]
    fn validates_required_service_and_distinct_dlq() {
        assert!(
            RedisTransportOptions::new("redis://localhost")
                .validate(&[])
                .is_err()
        );
        assert!(
            RedisTransportOptions::new("redis://localhost")
                .service_name("x")
                .dead_letter_stream("caelix:events")
                .validate(&[])
                .is_err()
        );
        assert!(
            RedisTransportOptions::new("redis://localhost")
                .service_name("x")
                .dead_letter_stream(" ")
                .validate(&[])
                .is_err()
        );
        assert!(
            RedisTransportOptions::new("redis://localhost")
                .service_name("x")
                .rpc_timeout(Duration::ZERO)
                .validate(&[])
                .is_err()
        );
    }

    #[test]
    fn request_envelopes_round_trip_with_transport_metadata() {
        let request = RequestEnvelope {
            version: PROTOCOL_VERSION,
            correlation_id: "correlation".into(),
            deadline_unix_millis: Some(1234),
            reply_channel: Some("caelix:reply:correlation".into()),
            headers: BTreeMap::from([("traceparent".into(), "trace".into())]),
            payload: serde_json::json!({"id": 42}),
        };
        let bytes = encode_limited(&request, 4096).unwrap();
        let decoded: RequestEnvelope = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.version, PROTOCOL_VERSION);
        assert_eq!(decoded.correlation_id, "correlation");
        assert_eq!(decoded.deadline_unix_millis, Some(1234));
        assert_eq!(
            decoded.reply_channel.as_deref(),
            Some("caelix:reply:correlation")
        );
        assert_eq!(decoded.headers["traceparent"], "trace");
        assert_eq!(decoded.payload, serde_json::json!({"id": 42}));
    }

    #[test]
    fn delivery_attempts_are_one_based() {
        assert_eq!(delivery_attempt(0), 1);
        assert_eq!(delivery_attempt(1), 1);
        assert_eq!(delivery_attempt(7), 7);
    }

    #[test]
    fn deadlines_expire_at_the_declared_millisecond() {
        assert!(deadline_elapsed(now_millis().saturating_sub(1)));
        assert!(!deadline_elapsed(now_millis().saturating_add(10_000)));
    }

    #[tokio::test]
    async fn broker_reply_subscription_precedes_request_publication() {
        let Ok(server) = std::env::var("CAELIX_REDIS_TEST_URL") else {
            return;
        };
        let subject = format!("tests.reply.{}", Uuid::new_v4().simple());
        let stream = command_stream(&subject);
        let group = command_group("test-service", &subject);
        let options = RedisTransportOptions::new(server)
            .service_name("test-service")
            .rpc_timeout(Duration::from_secs(2));
        let client = RedisClient::connect(options).await.unwrap();
        create_group(&client.connection, &stream, &group)
            .await
            .unwrap();

        let requester = client.clone();
        let request_subject = subject.clone();
        let request = tokio::spawn(async move {
            requester
                .request::<_, Value>(&request_subject, serde_json::json!({"ping": true}))
                .await
        });
        let entries = timeout(Duration::from_secs(2), async {
            loop {
                let entries = read_group(&client.connection, &stream, &group, "test-consumer", 1)
                    .await
                    .unwrap();
                if !entries.is_empty() {
                    break entries;
                }
            }
        })
        .await
        .unwrap();
        let (id, fields) = entries.into_iter().next().unwrap();
        let bytes: Vec<u8> = field(&fields, "envelope").unwrap();
        let envelope: RequestEnvelope = serde_json::from_slice(&bytes).unwrap();
        let response = serde_json::to_vec(&ResponseEnvelope {
            version: PROTOCOL_VERSION,
            correlation_id: envelope.correlation_id,
            body: ResponseBody::Success {
                payload: serde_json::json!({"pong": true}),
            },
        })
        .unwrap();
        let mut connection = client.connection.clone();
        let subscribers: usize = connection
            .publish(envelope.reply_channel.unwrap(), response)
            .await
            .unwrap();
        assert_eq!(subscribers, 1);
        ack_delete(&client.connection, &stream, &group, &id)
            .await
            .unwrap();
        assert_eq!(
            request.await.unwrap().unwrap(),
            serde_json::json!({"pong": true})
        );
        let _: usize = connection.del(&stream).await.unwrap();
    }

    #[tokio::test]
    async fn broker_command_replicas_compete_and_return_typed_remote_results() {
        let Some(server) = redis_test_url() else {
            return;
        };
        let subject = unique_subject("command_compete");
        let service = format!("service-{}", Uuid::new_v4().simple());
        let event_stream = format!("caelix:test:events:{}", Uuid::new_v4().simple());
        let client = test_client(server, &service, &event_stream).await;
        let stream = command_stream(subject);
        let group = command_group(&service, subject);
        create_group(&client.connection, &stream, &group)
            .await
            .unwrap();

        let first_calls = Arc::new(AtomicUsize::new(0));
        let second_calls = Arc::new(AtomicUsize::new(0));
        let handler = |calls: Arc<AtomicUsize>| {
            MessageHandlerDef::new(
                MessageHandlerKind::Command,
                subject,
                move |_container, _context, payload| {
                    let calls = calls.clone();
                    Box::pin(async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        if payload.get("fail").and_then(Value::as_bool) == Some(true) {
                            Err(BadRequestException::new("public command failure"))
                        } else {
                            Ok(Some(serde_json::json!({"echo": payload})))
                        }
                    })
                },
            )
        };
        let cancellation = CancellationToken::new();
        let first = tokio::spawn(command_loop(
            handler(first_calls.clone()),
            Arc::new(Container::new()),
            client.clone(),
            service.clone(),
            "replica-one".into(),
            cancellation.child_token(),
        ));
        let second = tokio::spawn(command_loop(
            handler(second_calls.clone()),
            Arc::new(Container::new()),
            client.clone(),
            service.clone(),
            "replica-two".into(),
            cancellation.child_token(),
        ));

        let mut requests = JoinSet::new();
        for index in 0..12 {
            let requester = client.clone();
            requests.spawn(async move {
                let response: Value = requester
                    .request(subject, serde_json::json!({"index": index}))
                    .await
                    .unwrap();
                assert_eq!(response["echo"]["index"], index);
            });
        }
        while let Some(result) = requests.join_next().await {
            result.unwrap();
        }
        let failure = client
            .request::<_, Value>(subject, serde_json::json!({"fail": true}))
            .await
            .unwrap_err();
        let MicroserviceClientError::Remote(failure) = failure else {
            panic!("expected a typed remote failure");
        };
        assert_eq!(failure.message, "public command failure");
        assert!(!failure.retryable);
        assert_eq!(
            first_calls.load(Ordering::SeqCst) + second_calls.load(Ordering::SeqCst),
            13
        );

        cancellation.cancel();
        first.await.unwrap().unwrap();
        second.await.unwrap().unwrap();
        let mut connection = client.connection.clone();
        let length: usize = connection.xlen(&stream).await.unwrap();
        assert_eq!(length, 0);
        delete_keys(&client, &[&stream, &event_stream]).await;
    }

    #[tokio::test]
    async fn broker_expired_missing_responder_is_suppressed_when_service_arrives_late() {
        let Some(server) = redis_test_url() else {
            return;
        };
        let subject = unique_subject("late_responder");
        let service = format!("service-{}", Uuid::new_v4().simple());
        let event_stream = format!("caelix:test:events:{}", Uuid::new_v4().simple());
        let mut client = test_client(server, &service, &event_stream).await;
        client.options.rpc_timeout = Duration::from_millis(80);
        let result = client
            .request::<_, Value>(subject, serde_json::json!({"late": true}))
            .await;
        assert!(matches!(result, Err(MicroserviceClientError::Timeout)));

        let stream = command_stream(subject);
        let group = command_group(&service, subject);
        create_group(&client.connection, &stream, &group)
            .await
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let handler_calls = calls.clone();
        let handler = MessageHandlerDef::new(
            MessageHandlerKind::Command,
            subject,
            move |_container, _context, _payload| {
                let calls = handler_calls.clone();
                Box::pin(async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(Some(Value::Null))
                })
            },
        );
        let cancellation = CancellationToken::new();
        let consumer = tokio::spawn(command_loop(
            handler,
            Arc::new(Container::new()),
            client.clone(),
            service,
            "late-replica".into(),
            cancellation.child_token(),
        ));
        timeout(Duration::from_secs(2), async {
            loop {
                let mut connection = client.connection.clone();
                let length: usize = connection.xlen(&stream).await.unwrap();
                if length == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        cancellation.cancel();
        consumer.await.unwrap().unwrap();
        delete_keys(&client, &[&stream, &event_stream]).await;
    }

    #[tokio::test]
    async fn broker_abandoned_command_is_recovered_with_xautoclaim() {
        let Some(server) = redis_test_url() else {
            return;
        };
        let subject = unique_subject("command_reclaim");
        let service = format!("service-{}", Uuid::new_v4().simple());
        let event_stream = format!("caelix:test:events:{}", Uuid::new_v4().simple());
        let client = test_client(server, &service, &event_stream).await;
        let stream = command_stream(subject);
        let group = command_group(&service, subject);
        create_group(&client.connection, &stream, &group)
            .await
            .unwrap();
        append_request(&client, subject, "abandoned", Some(now_millis() + 5_000)).await;
        let abandoned = read_group(&client.connection, &stream, &group, "crashed", 1)
            .await
            .unwrap();
        assert_eq!(abandoned.len(), 1);
        tokio::time::sleep(Duration::from_millis(40)).await;
        let recovered = reclaim(
            &client.connection,
            &stream,
            &group,
            "replacement",
            Duration::from_millis(25),
            1,
        )
        .await
        .unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].0, abandoned[0].0);
        assert_eq!(
            pending_attempt(&client.connection, &stream, &group, &recovered[0].0)
                .await
                .unwrap(),
            2
        );
        ack_delete(&client.connection, &stream, &group, &recovered[0].0)
            .await
            .unwrap();
        delete_keys(&client, &[&stream, &event_stream]).await;
    }

    #[tokio::test]
    async fn broker_events_reach_each_service_but_only_one_replica_per_service() {
        let Some(server) = redis_test_url() else {
            return;
        };
        let pattern = unique_subject("event_compete");
        let service_a = format!("service-a-{}", Uuid::new_v4().simple());
        let service_b = format!("service-b-{}", Uuid::new_v4().simple());
        let event_stream = format!("caelix:test:events:{}", Uuid::new_v4().simple());
        let client = test_client(server, &service_a, &event_stream).await;
        let group_a = event_group(&service_a);
        let group_b = event_group(&service_b);
        create_group(&client.connection, &event_stream, &group_a)
            .await
            .unwrap();
        create_group(&client.connection, &event_stream, &group_b)
            .await
            .unwrap();

        let a_one = Arc::new(AtomicUsize::new(0));
        let a_two = Arc::new(AtomicUsize::new(0));
        let b = Arc::new(AtomicUsize::new(0));
        let handler = |calls: Arc<AtomicUsize>| {
            MessageHandlerDef::new(
                MessageHandlerKind::Event,
                pattern,
                move |_container, context, _payload| {
                    let calls = calls.clone();
                    Box::pin(async move {
                        assert_eq!(context.delivery_attempt(), 1);
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(None)
                    })
                },
            )
        };
        let cancellation = CancellationToken::new();
        let first = tokio::spawn(event_loop(
            vec![handler(a_one.clone())],
            Arc::new(Container::new()),
            client.clone(),
            service_a.clone(),
            "a-one".into(),
            cancellation.child_token(),
        ));
        let second = tokio::spawn(event_loop(
            vec![handler(a_two.clone())],
            Arc::new(Container::new()),
            client.clone(),
            service_a,
            "a-two".into(),
            cancellation.child_token(),
        ));
        let third = tokio::spawn(event_loop(
            vec![handler(b.clone())],
            Arc::new(Container::new()),
            client.clone(),
            service_b,
            "b-one".into(),
            cancellation.child_token(),
        ));
        append_event(&client, pattern, serde_json::json!({"event": true})).await;
        wait_until(|| {
            a_one.load(Ordering::SeqCst) + a_two.load(Ordering::SeqCst) == 1
                && b.load(Ordering::SeqCst) == 1
        })
        .await;
        tokio::time::sleep(Duration::from_millis(75)).await;
        assert_eq!(
            a_one.load(Ordering::SeqCst) + a_two.load(Ordering::SeqCst),
            1
        );
        assert_eq!(b.load(Ordering::SeqCst), 1);
        cancellation.cancel();
        first.await.unwrap().unwrap();
        second.await.unwrap().unwrap();
        third.await.unwrap().unwrap();
        delete_keys(&client, &[&event_stream]).await;
    }

    #[tokio::test]
    async fn broker_wildcard_event_retries_report_attempts_and_dead_letter() {
        let Some(server) = redis_test_url() else {
            return;
        };
        let prefix = format!("tests.retry.{}", Uuid::new_v4().simple());
        let pattern: &'static str = Box::leak(format!("{prefix}.*").into_boxed_str());
        let subject = format!("{prefix}.created");
        let service = format!("service-{}", Uuid::new_v4().simple());
        let event_stream = format!("caelix:test:events:{}", Uuid::new_v4().simple());
        let dead_letter = format!("caelix:test:dlq:{}", Uuid::new_v4().simple());
        let mut client = test_client(server, &service, &event_stream).await;
        client.options.max_event_deliveries = 2;
        client.options.dead_letter_stream = Some(dead_letter.clone());
        let group = event_group(&service);
        create_group(&client.connection, &event_stream, &group)
            .await
            .unwrap();
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let handler_attempts = attempts.clone();
        let handler = MessageHandlerDef::new(
            MessageHandlerKind::Event,
            pattern,
            move |_container, context, _payload| {
                let attempts = handler_attempts.clone();
                Box::pin(async move {
                    attempts
                        .lock()
                        .expect("attempt lock poisoned")
                        .push(context.delivery_attempt());
                    Err(HttpException::new(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Internal Server Error",
                        "private failure",
                    ))
                })
            },
        );
        let cancellation = CancellationToken::new();
        let consumer = tokio::spawn(event_loop(
            vec![handler],
            Arc::new(Container::new()),
            client.clone(),
            service,
            "retry-replica".into(),
            cancellation.child_token(),
        ));
        append_event(
            &client,
            &subject,
            serde_json::json!({"important": "original payload"}),
        )
        .await;
        timeout(Duration::from_secs(3), async {
            loop {
                let mut connection = client.connection.clone();
                let length: usize = connection.xlen(&dead_letter).await.unwrap();
                if length == 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(*attempts.lock().expect("attempt lock poisoned"), vec![1, 2]);
        assert_eq!(pending_entries(&client, &event_stream, &group).await, 0);

        let dlq_group = format!("inspect-{}", Uuid::new_v4().simple());
        create_group(&client.connection, &dead_letter, &dlq_group)
            .await
            .unwrap();
        let entries = read_group(&client.connection, &dead_letter, &dlq_group, "inspector", 1)
            .await
            .unwrap();
        let bytes: Vec<u8> = field(&entries[0].1, "envelope").unwrap();
        let envelope: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(envelope["subject"], subject);
        assert_eq!(
            envelope["event"]["payload"]["important"],
            "original payload"
        );
        assert_eq!(envelope["deliveries"], 2);
        assert_eq!(envelope["error"]["message"], "Internal Server Error");
        assert!(!String::from_utf8_lossy(&bytes).contains("private failure"));

        cancellation.cancel();
        consumer.await.unwrap().unwrap();
        delete_keys(&client, &[&event_stream, &dead_letter]).await;
    }

    #[tokio::test]
    async fn broker_long_event_heartbeat_prevents_reclaim_and_shutdown_drains_handler() {
        let Some(server) = redis_test_url() else {
            return;
        };
        let pattern = unique_subject("event_heartbeat");
        let service = format!("service-{}", Uuid::new_v4().simple());
        let event_stream = format!("caelix:test:events:{}", Uuid::new_v4().simple());
        let client = test_client(server, &service, &event_stream).await;
        let group = event_group(&service);
        create_group(&client.connection, &event_stream, &group)
            .await
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let attempts = Arc::new(Mutex::new(Vec::new()));
        let started = Arc::new(Notify::new());
        let finished = Arc::new(AtomicBool::new(false));
        let handler = {
            let calls = calls.clone();
            let attempts = attempts.clone();
            let started = started.clone();
            let finished = finished.clone();
            move || {
                let calls = calls.clone();
                let attempts = attempts.clone();
                let started = started.clone();
                let finished = finished.clone();
                MessageHandlerDef::new(
                    MessageHandlerKind::Event,
                    pattern,
                    move |_container, context, _payload| {
                        let calls = calls.clone();
                        let attempts = attempts.clone();
                        let started = started.clone();
                        let finished = finished.clone();
                        Box::pin(async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            attempts
                                .lock()
                                .expect("attempt lock poisoned")
                                .push(context.delivery_attempt());
                            started.notify_waiters();
                            tokio::time::sleep(Duration::from_millis(140)).await;
                            finished.store(true, Ordering::SeqCst);
                            Ok(None)
                        })
                    },
                )
            }
        };
        let cancellation = CancellationToken::new();
        let first = tokio::spawn(event_loop(
            vec![handler()],
            Arc::new(Container::new()),
            client.clone(),
            service.clone(),
            "heartbeat-one".into(),
            cancellation.child_token(),
        ));
        let second = tokio::spawn(event_loop(
            vec![handler()],
            Arc::new(Container::new()),
            client.clone(),
            service,
            "heartbeat-two".into(),
            cancellation.child_token(),
        ));
        let started_wait = started.notified();
        append_event(&client, pattern, Value::Null).await;
        timeout(Duration::from_secs(2), started_wait).await.unwrap();
        cancellation.cancel();
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!first.is_finished() || !second.is_finished());
        first.await.unwrap().unwrap();
        second.await.unwrap().unwrap();
        assert!(finished.load(Ordering::SeqCst));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(*attempts.lock().expect("attempt lock poisoned"), vec![1]);
        assert_eq!(pending_entries(&client, &event_stream, &group).await, 0);
        delete_keys(&client, &[&event_stream]).await;
    }

    #[tokio::test]
    async fn broker_malformed_unsupported_and_oversized_events_are_terminal() {
        let Some(server) = redis_test_url() else {
            return;
        };
        let pattern = unique_subject("event_limits");
        let service = format!("service-{}", Uuid::new_v4().simple());
        let event_stream = format!("caelix:test:events:{}", Uuid::new_v4().simple());
        let mut client = test_client(server, &service, &event_stream).await;
        client.options.max_event_bytes = 512;
        let group = event_group(&service);
        create_group(&client.connection, &event_stream, &group)
            .await
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let handler_calls = calls.clone();
        let handler = MessageHandlerDef::new(
            MessageHandlerKind::Event,
            pattern,
            move |_container, _context, _payload| {
                let calls = handler_calls.clone();
                Box::pin(async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Err(BadRequestException::new("terminal client failure"))
                })
            },
        );
        let cancellation = CancellationToken::new();
        let consumer = tokio::spawn(event_loop(
            vec![handler],
            Arc::new(Container::new()),
            client.clone(),
            service,
            "limits-replica".into(),
            cancellation.child_token(),
        ));
        let mut connection = client.connection.clone();
        let _: String = redis::cmd("XADD")
            .arg(&event_stream)
            .arg("*")
            .arg("subject")
            .arg(pattern)
            .arg("wrong-field")
            .arg("missing envelope")
            .query_async(&mut connection)
            .await
            .unwrap();
        let _: String = redis::cmd("XADD")
            .arg(&event_stream)
            .arg("*")
            .arg("subject")
            .arg(pattern)
            .arg("envelope")
            .arg(b"not-json".as_slice())
            .query_async(&mut connection)
            .await
            .unwrap();
        let unsupported = serde_json::to_vec(&serde_json::json!({
            "version": PROTOCOL_VERSION + 1,
            "event_id": "unsupported",
            "headers": {},
            "payload": null,
            "published_at_unix_millis": now_millis()
        }))
        .unwrap();
        let _: String = redis::cmd("XADD")
            .arg(&event_stream)
            .arg("*")
            .arg("subject")
            .arg(pattern)
            .arg("envelope")
            .arg(unsupported)
            .query_async(&mut connection)
            .await
            .unwrap();
        let _: String = redis::cmd("XADD")
            .arg(&event_stream)
            .arg("*")
            .arg("subject")
            .arg(pattern)
            .arg("envelope")
            .arg(vec![b'x'; 513])
            .query_async(&mut connection)
            .await
            .unwrap();
        append_event(&client, pattern, serde_json::json!({"valid": true})).await;
        wait_until(|| calls.load(Ordering::SeqCst) == 1).await;
        tokio::time::sleep(Duration::from_millis(75)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(pending_entries(&client, &event_stream, &group).await, 0);
        cancellation.cancel();
        consumer.await.unwrap().unwrap();
        delete_keys(&client, &[&event_stream]).await;
    }

    #[tokio::test]
    async fn broker_malformed_unsupported_and_oversized_commands_are_terminal() {
        let Some(server) = redis_test_url() else {
            return;
        };
        let subject = unique_subject("command_limits");
        let service = format!("service-{}", Uuid::new_v4().simple());
        let event_stream = format!("caelix:test:events:{}", Uuid::new_v4().simple());
        let mut client = test_client(server, &service, &event_stream).await;
        client.options.max_request_bytes = 512;
        let stream = command_stream(subject);
        let group = command_group(&service, subject);
        create_group(&client.connection, &stream, &group)
            .await
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let handler_calls = calls.clone();
        let handler = MessageHandlerDef::new(
            MessageHandlerKind::Command,
            subject,
            move |_container, _context, _payload| {
                let calls = handler_calls.clone();
                Box::pin(async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(Some(Value::Null))
                })
            },
        );
        let cancellation = CancellationToken::new();
        let consumer = tokio::spawn(command_loop(
            handler,
            Arc::new(Container::new()),
            client.clone(),
            service,
            "limits-replica".into(),
            cancellation.child_token(),
        ));
        let mut connection = client.connection.clone();
        let _: String = redis::cmd("XADD")
            .arg(&stream)
            .arg("*")
            .arg("wrong-field")
            .arg("missing envelope")
            .query_async(&mut connection)
            .await
            .unwrap();
        let _: String = redis::cmd("XADD")
            .arg(&stream)
            .arg("*")
            .arg("envelope")
            .arg(b"not-json".as_slice())
            .query_async(&mut connection)
            .await
            .unwrap();
        let unsupported = serde_json::to_vec(&RequestEnvelope {
            version: PROTOCOL_VERSION + 1,
            correlation_id: "unsupported".into(),
            deadline_unix_millis: Some(now_millis() + 5_000),
            reply_channel: Some("caelix:reply:unsupported".into()),
            headers: BTreeMap::new(),
            payload: Value::Null,
        })
        .unwrap();
        let _: String = redis::cmd("XADD")
            .arg(&stream)
            .arg("*")
            .arg("envelope")
            .arg(unsupported)
            .query_async(&mut connection)
            .await
            .unwrap();
        let _: String = redis::cmd("XADD")
            .arg(&stream)
            .arg("*")
            .arg("envelope")
            .arg(vec![b'x'; 513])
            .query_async(&mut connection)
            .await
            .unwrap();

        timeout(Duration::from_secs(2), async {
            loop {
                let length: usize = connection.xlen(&stream).await.unwrap();
                if length == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(pending_entries(&client, &stream, &group).await, 0);
        cancellation.cancel();
        consumer.await.unwrap().unwrap();
        delete_keys(&client, &[&stream, &event_stream]).await;
    }

    #[tokio::test]
    async fn broker_shutdown_deadline_aborts_a_stuck_active_handler() {
        let Some(server) = redis_test_url() else {
            return;
        };
        let pattern = unique_subject("forced_shutdown");
        let service = format!("service-{}", Uuid::new_v4().simple());
        let event_stream = format!("caelix:test:events:{}", Uuid::new_v4().simple());
        let client = test_client(server, &service, &event_stream).await;
        let group = event_group(&service);
        create_group(&client.connection, &event_stream, &group)
            .await
            .unwrap();
        let started = Arc::new(Notify::new());
        let handler_started = started.clone();
        let handler = MessageHandlerDef::new(
            MessageHandlerKind::Event,
            pattern,
            move |_container, _context, _payload| {
                let started = handler_started.clone();
                Box::pin(async move {
                    started.notify_waiters();
                    std::future::pending::<caelix_core::Result<Option<Value>>>().await
                })
            },
        );
        let cancellation = CancellationToken::new();
        let mut tasks = JoinSet::new();
        tasks.spawn(event_loop(
            vec![handler],
            Arc::new(Container::new()),
            client.clone(),
            service,
            "stuck-replica".into(),
            cancellation.child_token(),
        ));
        let started_wait = started.notified();
        append_event(&client, pattern, Value::Null).await;
        timeout(Duration::from_secs(2), started_wait).await.unwrap();
        cancellation.cancel();
        let error = shutdown_redis_tasks(&mut tasks, Duration::from_millis(30))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("shutdown deadline elapsed"));
        assert_eq!(pending_entries(&client, &event_stream, &group).await, 1);
        delete_keys(&client, &[&event_stream]).await;
    }
}
