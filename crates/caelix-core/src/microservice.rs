//! Transport-neutral microservice handler metadata.

use crate::{BoxFuture, Container, Injectable, ProviderDef, Result};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc, time::SystemTime};
use tokio_util::sync::CancellationToken;

/// The kind of a message handler.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageHandlerKind {
    /// A request/reply command handler.
    Command,
    /// A durable, at-least-once event handler.
    Event,
}

/// Durable transport delivery details attached to an event handler invocation.
#[derive(Clone, Debug, Default)]
pub struct MessageDelivery {
    /// The stream that delivered the message, when applicable.
    pub stream: Option<String>,
    /// The durable consumer that delivered the message, when applicable.
    pub consumer: Option<String>,
    /// The one-based delivery attempt.
    pub attempt: u64,
}

/// Context made available to a microservice handler.
#[derive(Clone, Debug)]
pub struct MessageContext {
    subject: String,
    headers: BTreeMap<String, String>,
    correlation_id: Option<String>,
    deadline: Option<SystemTime>,
    cancellation: CancellationToken,
    delivery: Option<MessageDelivery>,
    event_id: Option<String>,
}

impl MessageContext {
    /// Creates a context for a transport delivery.
    pub fn new(
        subject: impl Into<String>,
        headers: BTreeMap<String, String>,
        correlation_id: Option<String>,
        deadline: Option<SystemTime>,
        cancellation: CancellationToken,
        delivery: Option<MessageDelivery>,
        event_id: Option<String>,
    ) -> Self {
        Self {
            subject: subject.into(),
            headers,
            correlation_id,
            deadline,
            cancellation,
            delivery,
            event_id,
        }
    }

    /// The subject that delivered this message.
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// Application headers propagated by the transport.
    pub fn headers(&self) -> &BTreeMap<String, String> {
        &self.headers
    }

    /// The request correlation identifier, when this is a command.
    pub fn correlation_id(&self) -> Option<&str> {
        self.correlation_id.as_deref()
    }

    /// The requested deadline, when one was propagated.
    pub fn deadline(&self) -> Option<SystemTime> {
        self.deadline
    }

    /// A token cancelled when the application begins shutdown.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation
    }

    /// Delivery metadata for durable events.
    pub fn delivery(&self) -> Option<&MessageDelivery> {
        self.delivery.as_ref()
    }

    /// The one-based delivery attempt, or zero for non-durable commands.
    pub fn delivery_attempt(&self) -> u64 {
        self.delivery
            .as_ref()
            .map_or(0, |delivery| delivery.attempt)
    }

    /// The stable event envelope ID, when this is a durable event delivery.
    pub fn event_id(&self) -> Option<&str> {
        self.event_id.as_deref()
    }
}

type InvokeMessageFn = Arc<
    dyn for<'a> Fn(&'a Container, MessageContext, Value) -> BoxFuture<'a, Result<Option<Value>>>
        + Send
        + Sync,
>;

/// One transport-neutral message handler declared by a microservice.
#[derive(Clone)]
pub struct MessageHandlerDef {
    /// Whether the handler is a command or event handler.
    pub kind: MessageHandlerKind,
    /// The logical subject pattern.
    pub pattern: &'static str,
    invoke: InvokeMessageFn,
}

impl MessageHandlerDef {
    /// Builds a handler definition. This is primarily used by `#[microservice]`.
    pub fn new(
        kind: MessageHandlerKind,
        pattern: &'static str,
        invoke: impl for<'a> Fn(
            &'a Container,
            MessageContext,
            Value,
        ) -> BoxFuture<'a, Result<Option<Value>>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            kind,
            pattern,
            invoke: Arc::new(invoke),
        }
    }

    /// Invokes the typed handler after the transport has decoded its envelope.
    pub fn invoke<'a>(
        &self,
        container: &'a Container,
        context: MessageContext,
        payload: Value,
    ) -> BoxFuture<'a, Result<Option<Value>>> {
        (self.invoke)(container, context, payload)
    }
}

/// Metadata for one dependency-injected microservice class.
pub struct MicroserviceDef {
    /// The provider definition used for normal module lifecycle management.
    pub provider: ProviderDef,
    handlers_fn: fn() -> Vec<MessageHandlerDef>,
}

impl MicroserviceDef {
    /// Creates metadata for `T` and its generated handler factory.
    pub fn of<T: Injectable>(handlers_fn: fn() -> Vec<MessageHandlerDef>) -> Self {
        Self {
            provider: ProviderDef::of::<T>(),
            handlers_fn,
        }
    }

    /// Materializes handler definitions for a runtime transport.
    pub fn handlers(&self) -> Vec<MessageHandlerDef> {
        (self.handlers_fn)()
    }
}

/// Implemented by `#[microservice]` classes.
pub trait Microservice: Injectable {
    /// Returns provider and handler metadata for module registration.
    fn definition() -> MicroserviceDef;
}

/// Resolves a handler's owning service as an erased singleton.
///
/// This helper is intentionally small; generated handler code resolves the
/// concrete service itself so type checking remains at the declaration site.
#[doc(hidden)]
pub fn _assert_microservice_send_sync<T: Send + Sync + 'static>() {
    let _ = std::marker::PhantomData::<Arc<T>>;
}
