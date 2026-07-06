use crate::{BoxFuture, Container, Injectable, Result};
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    marker::PhantomData,
    sync::{Arc, RwLock},
};

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

pub struct EventBus {
    handlers: RwLock<HashMap<TypeId, Vec<Arc<dyn ErasedHandler>>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            handlers: RwLock::new(HashMap::new()),
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

    pub(crate) fn assert_registered(&self, container: &Container) {
        assert!(
            container.contains_type_id(self.type_id),
            "missing event handler provider at startup: {} was declared by module metadata but was not registered as a provider",
            self.type_name
        );
    }

    pub(crate) fn register(&self, container: &Container) {
        (self.register_fn)(container);
    }
}
