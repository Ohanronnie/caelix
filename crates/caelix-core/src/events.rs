use crate::{BoxFuture, Container, Injectable, Module, ModuleMetadata, Result};
use futures_util::StreamExt;
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    marker::PhantomData,
    sync::{Arc, RwLock},
};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

/// Default capacity for per-event-type broadcast channels used by [`EventBus::subscribe`].
const EVENT_BROADCAST_CAPACITY: usize = 256;

#[diagnostic::on_unimplemented(
    message = "`{Self}` is not an event handler for `{E}`",
    label = "missing `impl EventHandler<{E}> for {Self}`",
    note = "add `impl EventHandler<{E}> for {Self}` or use the correct event type in `.event_handler_for::<Event, Handler>()`"
)]
pub trait EventHandler<E>: Send + Sync + 'static {
    fn handle(&self, event: E) -> BoxFuture<'_, Result<()>>;
}

#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be registered with `.event_handler::<{Self}>()` yet",
    label = "missing `impl RegisterableEventHandler for {Self}`",
    note = "add `impl RegisterableEventHandler for {Self} {{ type Event = YourEvent; }}` or use `.event_handler_for::<YourEvent, {Self}>()`"
)]
pub trait RegisterableEventHandler: Injectable {
    type Event: Clone + Send + Sync + 'static;

    fn register_into(handler: Arc<Self>, bus: &EventBus)
    where
        Self: Sized + EventHandler<Self::Event>,
    {
        bus.register::<Self::Event, Self>(handler);
    }
}

trait ErasedHandler: Send + Sync {
    fn handle_erased(&self, event: &dyn Any) -> BoxFuture<'_, Result<()>>;
}

struct TypedHandler<E, H> {
    handler: Arc<H>,
    _marker: PhantomData<E>,
}

impl<E, H> ErasedHandler for TypedHandler<E, H>
where
    E: Clone + Send + Sync + 'static,
    H: EventHandler<E>,
{
    fn handle_erased(&self, event: &dyn Any) -> BoxFuture<'_, Result<()>> {
        let event = event
            .downcast_ref::<E>()
            .expect("event type mismatch")
            .clone();
        let handler = self.handler.clone();

        Box::pin(async move { handler.handle(event).await })
    }
}

struct EventChannel<E> {
    tx: broadcast::Sender<E>,
}

pub struct EventBus {
    handlers: RwLock<HashMap<TypeId, Vec<Arc<dyn ErasedHandler>>>>,
    broadcasts: RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            handlers: RwLock::new(HashMap::new()),
            broadcasts: RwLock::new(HashMap::new()),
        }
    }

    pub fn register<E, H>(&self, handler: Arc<H>)
    where
        E: Clone + Send + Sync + 'static,
        H: EventHandler<E>,
    {
        let wrapped = Arc::new(TypedHandler::<E, H> {
            handler,
            _marker: PhantomData,
        });

        self.handlers
            .write()
            .expect("event handler registry lock poisoned")
            .entry(TypeId::of::<E>())
            .or_default()
            .push(wrapped);
    }

    /// Create the per-type broadcast channel if missing; used only by [`Self::subscribe`].
    fn ensure_sender<E>(&self) -> broadcast::Sender<E>
    where
        E: Clone + Send + Sync + 'static,
    {
        let type_id = TypeId::of::<E>();

        {
            let broadcasts = self
                .broadcasts
                .read()
                .expect("event broadcast registry lock poisoned");
            if let Some(channel) = broadcasts.get(&type_id) {
                return channel
                    .downcast_ref::<EventChannel<E>>()
                    .expect("event broadcast type mismatch")
                    .tx
                    .clone();
            }
        }

        let mut broadcasts = self
            .broadcasts
            .write()
            .expect("event broadcast registry lock poisoned");
        let channel = broadcasts.entry(type_id).or_insert_with(|| {
            let (tx, _rx) = broadcast::channel::<E>(EVENT_BROADCAST_CAPACITY);
            Box::new(EventChannel { tx })
        });

        channel
            .downcast_ref::<EventChannel<E>>()
            .expect("event broadcast type mismatch")
            .tx
            .clone()
    }

    /// Existing channel only — does not allocate. Used by [`Self::emit`].
    fn existing_sender<E>(&self) -> Option<broadcast::Sender<E>>
    where
        E: Clone + Send + Sync + 'static,
    {
        self.broadcasts
            .read()
            .expect("event broadcast registry lock poisoned")
            .get(&TypeId::of::<E>())
            .map(|channel| {
                channel
                    .downcast_ref::<EventChannel<E>>()
                    .expect("event broadcast type mismatch")
                    .tx
                    .clone()
            })
    }

    /// Live stream of events of type `E`. Receives events after subscription;
    /// earlier events are not replayed. Slow consumers may lag and drop events
    /// (see broadcast capacity).
    ///
    /// Creates the broadcast channel for `E` on first subscribe. [`Self::emit`]
    /// only fans out when a channel already exists (i.e. someone has subscribed).
    pub fn subscribe<E>(&self) -> impl futures_core::Stream<Item = Result<E>> + Send + 'static
    where
        E: Clone + Send + Sync + 'static,
    {
        let rx = self.ensure_sender::<E>().subscribe();
        BroadcastStream::new(rx).filter_map(|item| async move {
            match item {
                Ok(event) => Some(Ok(event)),
                Err(BroadcastStreamRecvError::Lagged(_)) => {
                    tracing::warn!("event bus subscriber lagged; dropped events");
                    None
                }
            }
        })
    }

    /// Run registered handlers in order, then fan out to live subscribers.
    ///
    /// Handlers run first so a failed handler aborts `emit` and does **not**
    /// publish to stream subscribers. Broadcast channels are only used when
    /// created by a prior [`Self::subscribe`] — emit alone never allocates one.
    pub async fn emit<E>(&self, event: E) -> Result<()>
    where
        E: Clone + Send + Sync + 'static,
    {
        let handlers = self
            .handlers
            .read()
            .expect("event handler registry lock poisoned")
            .get(&TypeId::of::<E>())
            .cloned();

        if let Some(handlers) = handlers {
            for handler in handlers {
                handler.handle_erased(&event).await?;
            }
        }

        if let Some(tx) = self.existing_sender::<E>() {
            let _ = tx.send(event);
        }

        Ok(())
    }

    pub fn handler_count<E>(&self) -> usize
    where
        E: Clone + Send + Sync + 'static,
    {
        self.handlers
            .read()
            .expect("event handler registry lock poisoned")
            .get(&TypeId::of::<E>())
            .map_or(0, Vec::len)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl Injectable for EventBus {
    fn create(_container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async { Self::new() })
    }
}

pub struct EventModule;

impl Module for EventModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().provider::<EventBus>()
    }
}

pub struct EventHandlerDef {
    type_id: TypeId,
    type_name: &'static str,
    register_fn: Box<dyn Fn(&Container) + Send + Sync>,
}

impl EventHandlerDef {
    pub(crate) fn of<H>() -> Self
    where
        H: RegisterableEventHandler + EventHandler<H::Event>,
    {
        Self {
            type_id: TypeId::of::<H>(),
            type_name: std::any::type_name::<H>(),
            register_fn: Box::new(|container| {
                let handler = container.resolve::<H>();
                let bus = container.resolve::<EventBus>();
                H::register_into(handler, &bus);
            }),
        }
    }

    pub(crate) fn for_event<E, H>() -> Self
    where
        E: Clone + Send + Sync + 'static,
        H: Injectable + EventHandler<E>,
    {
        Self {
            type_id: TypeId::of::<H>(),
            type_name: std::any::type_name::<H>(),
            register_fn: Box::new(|container| {
                let handler = container.resolve::<H>();
                let bus = container.resolve::<EventBus>();
                bus.register::<E, H>(handler);
            }),
        }
    }

    pub(crate) fn try_assert_registered(&self, container: &Container) -> Result<()> {
        if container.contains_type_id(self.type_id) {
            return Ok(());
        }

        Err(crate::exception::startup_error(format!(
            "missing event handler provider at startup: {} was declared by module metadata but was not registered as a provider",
            self.type_name
        )))
    }

    pub(crate) fn register(&self, container: &Container) {
        (self.register_fn)(container);
    }
}
