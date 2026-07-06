use std::{
    any::{Any, TypeId},
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::Arc,
};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait Injectable: Send + Sync + 'static {
    fn create(container: &Container) -> BoxFuture<'_, Self>
    where
        Self: Sized;

    fn on_module_init(&self) -> BoxFuture<'_, crate::Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn on_bootstrap(&self) -> BoxFuture<'_, crate::Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn on_shutdown(&self) -> BoxFuture<'_, crate::Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Clone)]
pub struct Container {
    services: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl Container {
    pub fn new() -> Self {
        crate::logging::init_logging();

        let mut services: HashMap<TypeId, Arc<dyn Any + Send + Sync>> = HashMap::new();
        services.insert(
            TypeId::of::<crate::Logger>(),
            Arc::new(crate::Logger::new("Application")),
        );
        services.insert(
            TypeId::of::<crate::EventBus>(),
            Arc::new(crate::EventBus::new()),
        );

        Self { services }
    }

    pub fn register_instance<T: Send + Sync + 'static>(&mut self, value: T) {
        self.services.insert(TypeId::of::<T>(), Arc::new(value));
    }

    pub async fn register<T: Injectable>(&mut self) {
        let instance = T::create(self).await;
        let instance = Arc::new(instance);
        instance.on_module_init().await.unwrap_or_else(|err| {
            panic!(
                "on_module_init failed for {}: {}: {}",
                std::any::type_name::<T>(),
                err.error,
                err.message
            )
        });
        self.services.insert(TypeId::of::<T>(), instance);
    }

    pub(crate) fn register_erased(&mut self, type_id: TypeId, value: Arc<dyn Any + Send + Sync>) {
        self.services.insert(type_id, value);
    }

    pub(crate) fn resolve_erased(&self, type_id: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        self.services.get(&type_id).cloned()
    }

    pub(crate) fn contains_type_id(&self, type_id: TypeId) -> bool {
        self.services.contains_key(&type_id)
    }

    pub fn resolve<T: Send + Sync + 'static>(&self) -> Arc<T> {
        self.services
            .get(&TypeId::of::<T>())
            .unwrap_or_else(|| panic!("no provider registered for {}", std::any::type_name::<T>()))
            .clone()
            .downcast::<T>()
            .unwrap_or_else(|_| panic!("type mismatch resolving {}", std::any::type_name::<T>()))
    }

    pub fn resolve_logger(&self, context: impl Into<String>) -> Arc<crate::Logger> {
        Arc::new(crate::Logger::new(context))
    }
}
