use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet},
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

pub struct Container {
    services: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
    /// Pending provider overrides consumed while registering the module tree.
    pub(crate) pending_overrides: HashMap<TypeId, crate::ProviderDef>,
    /// TypeIds that were satisfied by an override (skip declared lifecycle hooks).
    pub(crate) applied_overrides: HashSet<TypeId>,
}

impl Clone for Container {
    fn clone(&self) -> Self {
        Self {
            services: self.services.clone(),
            // Overrides are build-time only; clones used by factories do not need them.
            pending_overrides: HashMap::new(),
            applied_overrides: self.applied_overrides.clone(),
        }
    }
}

impl Container {
    pub fn new() -> Self {
        crate::logging::init_logging();

        let mut services: HashMap<TypeId, Arc<dyn Any + Send + Sync>> = HashMap::new();
        services.insert(
            TypeId::of::<crate::Logger>(),
            Arc::new(crate::Logger::new("Application")),
        );

        Self {
            services,
            pending_overrides: HashMap::new(),
            applied_overrides: HashSet::new(),
        }
    }

    pub fn register_instance<T: Send + Sync + 'static>(&mut self, value: T) {
        self.services.insert(TypeId::of::<T>(), Arc::new(value));
    }

    pub async fn register<T: Injectable>(&mut self) {
        self.try_register::<T>()
            .await
            .unwrap_or_else(|err| panic!("{}", err.message));
    }

    pub async fn try_register<T: Injectable>(&mut self) -> crate::Result<()> {
        let instance = T::create(self).await;
        let instance = Arc::new(instance);
        instance.on_module_init().await.map_err(|err| {
            crate::exception::startup_error(format!(
                "on_module_init failed for {}: {}: {}",
                std::any::type_name::<T>(),
                err.error,
                err.message
            ))
        })?;
        self.services.insert(TypeId::of::<T>(), instance);
        Ok(())
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

    pub(crate) fn take_pending_override(
        &mut self,
        type_id: TypeId,
    ) -> Option<crate::ProviderDef> {
        self.pending_overrides.remove(&type_id)
    }

    pub(crate) fn mark_override_applied(&mut self, type_id: TypeId) {
        self.applied_overrides.insert(type_id);
    }

    pub(crate) fn was_overridden(&self, type_id: TypeId) -> bool {
        self.applied_overrides.contains(&type_id)
    }

    pub(crate) fn seed_overrides(&mut self, overrides: crate::ProviderOverrides) {
        self.pending_overrides = overrides.into_inner();
    }

    pub(crate) fn try_assert_no_unused_overrides(&self) -> crate::Result<()> {
        if self.pending_overrides.is_empty() {
            return Ok(());
        }

        let mut names: Vec<&'static str> = self
            .pending_overrides
            .values()
            .map(|provider| provider.type_name())
            .collect();
        names.sort_unstable();

        Err(crate::exception::startup_error(format!(
            "provider override was never applied: {}; is it registered in the module tree?",
            names.join(", ")
        )))
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

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}
