use crate::{Logger, ProviderDependency, Result};
use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

/// Public Caelix type alias `BoxFuture`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Public Caelix extension trait `Injectable`.
pub trait Injectable: Send + Sync + 'static {
    /// Public Caelix API.
    fn create(container: &Container) -> BoxFuture<'_, Result<Self>>
    where
        Self: Sized;

    /// Dependencies resolved while constructing this provider.
    ///
    /// `#[injectable]` supplies this automatically. Handwritten implementations
    /// must return `provider_dependencies![...]`; Caelix rejects construction-time
    /// resolution of a provider that is absent from this declaration.
    fn dependencies() -> Vec<ProviderDependency>
    where
        Self: Sized;

    /// Public Caelix API.
    fn on_module_init(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    /// Public Caelix API.
    fn on_bootstrap(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    /// Public Caelix API.
    fn on_shutdown(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

/// Public Caelix type `Container`.
pub struct Container {
    services: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
    /// Pending provider overrides consumed while registering the module tree.
    pub(crate) pending_overrides: HashMap<TypeId, crate::ProviderDef>,
    /// TypeIds that were satisfied by an override (skip declared lifecycle hooks).
    pub(crate) applied_overrides: HashSet<TypeId>,
    declared_provider_types: HashSet<TypeId>,
    /// Successful production initializations, in startup order.
    pub(crate) initialized_providers: Arc<Mutex<Vec<TypeId>>>,
    pub(crate) bootstrapped_providers: Arc<Mutex<HashSet<TypeId>>>,
    dependency_scope: Option<Arc<DependencyScope>>,
}

struct DependencyScope {
    provider_type_name: &'static str,
    allowed_type_ids: HashSet<TypeId>,
}

impl Clone for Container {
    fn clone(&self) -> Self {
        Self {
            services: self.services.clone(),
            // Overrides are build-time only; clones used by factories do not need them.
            pending_overrides: HashMap::new(),
            applied_overrides: self.applied_overrides.clone(),
            declared_provider_types: self.declared_provider_types.clone(),
            initialized_providers: self.initialized_providers.clone(),
            bootstrapped_providers: self.bootstrapped_providers.clone(),
            dependency_scope: self.dependency_scope.clone(),
        }
    }
}

impl Container {
    /// Runs the `new` public API operation.
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
            declared_provider_types: HashSet::new(),
            initialized_providers: Arc::new(Mutex::new(Vec::new())),
            bootstrapped_providers: Arc::new(Mutex::new(HashSet::new())),
            dependency_scope: None,
        }
    }

    /// Runs the `register_instance` public API operation.
    pub fn register_instance<T: Send + Sync + 'static>(&mut self, value: T) {
        self.services.insert(TypeId::of::<T>(), Arc::new(value));
    }

    /// Runs the `register` public API operation.
    pub async fn register<T: Injectable>(&mut self) -> Result<()> {
        let instance = T::create(self).await?;
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

    pub(crate) fn take_pending_override(&mut self, type_id: TypeId) -> Option<crate::ProviderDef> {
        self.pending_overrides.remove(&type_id)
    }

    pub(crate) fn mark_override_applied(&mut self, type_id: TypeId) {
        self.applied_overrides.insert(type_id);
    }

    pub(crate) fn was_overridden(&self, type_id: TypeId) -> bool {
        self.applied_overrides.contains(&type_id)
    }

    pub(crate) fn begin_provider_bootstrap(&self, type_id: TypeId) -> bool {
        self.bootstrapped_providers
            .lock()
            .expect("provider lifecycle lock poisoned")
            .insert(type_id)
    }

    pub(crate) fn mark_provider_declared(&mut self, type_id: TypeId) {
        self.declared_provider_types.insert(type_id);
    }

    pub(crate) fn seed_overrides(&mut self, overrides: crate::ProviderOverrides) {
        self.pending_overrides = overrides.into_inner();
    }

    pub(crate) fn has_pending_override(&self, type_id: TypeId) -> bool {
        self.pending_overrides.contains_key(&type_id)
    }

    pub(crate) fn pending_override(&self, type_id: TypeId) -> Option<&crate::ProviderDef> {
        self.pending_overrides.get(&type_id)
    }

    pub(crate) fn record_initialized_provider(&self, type_id: TypeId) {
        self.initialized_providers
            .lock()
            .expect("provider lifecycle lock poisoned")
            .push(type_id);
    }

    pub(crate) fn take_initialized_providers(&self) -> Vec<TypeId> {
        std::mem::take(
            &mut *self
                .initialized_providers
                .lock()
                .expect("provider lifecycle lock poisoned"),
        )
    }

    pub(crate) fn initialized_provider_types(&self) -> Vec<TypeId> {
        self.initialized_providers
            .lock()
            .expect("provider lifecycle lock poisoned")
            .clone()
    }

    pub(crate) fn scoped_for_provider(
        &self,
        provider_type_name: &'static str,
        dependencies: &[crate::ProviderDependency],
    ) -> Self {
        let mut scoped = self.clone();
        scoped.dependency_scope = Some(Arc::new(DependencyScope {
            provider_type_name,
            allowed_type_ids: dependencies
                .iter()
                .map(|dependency| dependency.type_id())
                .collect(),
        }));
        scoped
    }

    pub(crate) fn assert_no_unused_overrides(&self) -> crate::Result<()> {
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

    /// Runs the `resolve` public API operation.
    pub fn resolve<T: Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        let type_id = TypeId::of::<T>();
        if let Some(scope) = &self.dependency_scope
            && type_id != TypeId::of::<crate::Logger>()
            && !scope.allowed_type_ids.contains(&type_id)
        {
            return Err(crate::exception::startup_error(format!(
                "{} resolved {} without declaring it in dependencies()",
                scope.provider_type_name,
                std::any::type_name::<T>()
            )));
        }

        let value = self.services.get(&type_id).ok_or_else(|| {
            crate::exception::startup_error(format!(
                "no provider registered for {}",
                std::any::type_name::<T>()
            ))
        })?;

        value.clone().downcast::<T>().map_err(|_| {
            crate::exception::startup_error(format!(
                "type mismatch resolving {}",
                std::any::type_name::<T>()
            ))
        })
    }

    /// Runs the `resolve_logger` public API operation.
    pub fn resolve_logger(&self, context: impl Into<String>) -> Arc<Logger> {
        Arc::new(Logger::new(context))
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}
