use crate::{
    BoxFuture, Container, Controller, EventHandler, EventHandlerDef, Injectable,
    RegisterableEventHandler,
};
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    fmt::Debug,
    future::Future,
    sync::Arc,
    time::Instant,
};

type ProviderValue = Arc<dyn Any + Send + Sync>;
type BuildProviderFn =
    Box<dyn for<'a> Fn(&'a Container) -> BoxFuture<'a, crate::Result<ProviderValue>> + Send + Sync>;
type LifecycleFn =
    Box<dyn for<'a> Fn(&'a ProviderValue) -> BoxFuture<'a, crate::Result<()>> + Send + Sync>;

pub struct ProviderDef {
    type_id: TypeId,
    type_name: &'static str,
    build: BuildProviderFn,
    init_fn: LifecycleFn,
    bootstrap_fn: LifecycleFn,
    shutdown_fn: LifecycleFn,
}

impl ProviderDef {
    pub fn of<T: Injectable>() -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            build: Box::new(|container| {
                Box::pin(async move {
                    let value = T::create(container).await?;
                    Ok(Arc::new(value) as Arc<dyn Any + Send + Sync>)
                })
            }),
            init_fn: Box::new(|value| {
                let value = downcast_provider::<T>(value);
                Box::pin(async move { value?.on_module_init().await })
            }),
            bootstrap_fn: Box::new(|value| {
                let value = downcast_provider::<T>(value);
                Box::pin(async move { value?.on_bootstrap().await })
            }),
            shutdown_fn: Box::new(|value| {
                let value = downcast_provider::<T>(value);
                Box::pin(async move { value?.on_shutdown().await })
            }),
        }
    }

    /// Pre-built provider value for tests and other manual registration paths.
    ///
    /// Lifecycle hooks are no-ops (NestJS `useValue` semantics).
    pub fn instance<T: Send + Sync + 'static>(value: T) -> Self {
        let value = Arc::new(value) as Arc<dyn Any + Send + Sync>;
        Self {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            build: Box::new(move |_container| {
                let value = value.clone();
                Box::pin(async move { Ok(value) })
            }),
            init_fn: noop_lifecycle(),
            bootstrap_fn: noop_lifecycle(),
            shutdown_fn: noop_lifecycle(),
        }
    }

    pub fn async_factory<T, Fut, E>(
        factory: impl Fn(Arc<Container>) -> Fut + Send + Sync + 'static,
    ) -> Self
    where
        T: Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<T, E>> + Send + 'static,
        E: Debug + Send + 'static,
    {
        Self {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            build: Box::new(move |container| {
                let container = Arc::new(container.clone());
                let future = factory(container);

                Box::pin(async move {
                    let value = future.await.map_err(|err| {
                        crate::exception::startup_error(format!(
                            "async factory failed for {}: {:?}",
                            std::any::type_name::<T>(),
                            err
                        ))
                    })?;

                    Ok(Arc::new(value) as Arc<dyn Any + Send + Sync>)
                })
            }),
            init_fn: noop_lifecycle(),
            bootstrap_fn: noop_lifecycle(),
            shutdown_fn: noop_lifecycle(),
        }
    }

    pub fn type_id(&self) -> TypeId {
        self.type_id
    }

    pub fn type_name(&self) -> &'static str {
        self.type_name
    }

    fn assert_registered(&self, container: &Container) -> crate::Result<()> {
        if container.contains_type_id(self.type_id) {
            return Ok(());
        }

        Err(crate::exception::startup_error(format!(
            "missing provider at startup: {} was declared by module metadata but was not registered",
            self.type_name
        )))
    }

    async fn run_lifecycle(
        &self,
        value: &ProviderValue,
        hook_name: &'static str,
        lifecycle_fn: &LifecycleFn,
    ) -> crate::Result<()> {
        lifecycle_fn(value).await.map_err(|err| {
            crate::exception::startup_error(format!(
                "{hook_name} failed for {}: {}: {}",
                self.type_name, err.error, err.message
            ))
        })
    }

    async fn run_lifecycle_from_container(
        &self,
        container: &Container,
        hook_name: &'static str,
        lifecycle_fn: &LifecycleFn,
    ) -> crate::Result<()> {
        let value = container.resolve_erased(self.type_id).ok_or_else(|| {
            crate::exception::startup_error(format!(
                "missing provider during {hook_name}: {} was declared by module metadata but was not registered",
                self.type_name
            ))
        })?;

        self.run_lifecycle(&value, hook_name, lifecycle_fn).await
    }
}

/// Provider replacements applied while building a container (primarily for tests).
///
/// Overrides match by [`TypeId`]: the replacement must be the same concrete type
/// that modules register and inject via `Arc<T>`.
pub struct ProviderOverrides {
    defs: HashMap<TypeId, ProviderDef>,
}

impl ProviderOverrides {
    pub fn new() -> Self {
        Self {
            defs: HashMap::new(),
        }
    }

    pub fn insert_instance<T: Send + Sync + 'static>(mut self, value: T) -> Self {
        self.defs
            .insert(TypeId::of::<T>(), ProviderDef::instance(value));
        self
    }

    pub fn insert_factory<T, Fut, E>(
        mut self,
        factory: impl Fn(Arc<Container>) -> Fut + Send + Sync + 'static,
    ) -> Self
    where
        T: Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<T, E>> + Send + 'static,
        E: Debug + Send + 'static,
    {
        self.defs.insert(
            TypeId::of::<T>(),
            ProviderDef::async_factory::<T, Fut, E>(factory),
        );
        self
    }

    pub fn insert(mut self, def: ProviderDef) -> Self {
        self.defs.insert(def.type_id, def);
        self
    }

    pub(crate) fn into_inner(self) -> HashMap<TypeId, ProviderDef> {
        self.defs
    }
}

impl Default for ProviderOverrides {
    fn default() -> Self {
        Self::new()
    }
}

fn downcast_provider<T: Send + Sync + 'static>(value: &ProviderValue) -> crate::Result<Arc<T>> {
    value.clone().downcast::<T>().map_err(|_| {
        crate::exception::startup_error(format!(
            "type mismatch running lifecycle hook for {}",
            std::any::type_name::<T>()
        ))
    })
}

fn noop_lifecycle() -> LifecycleFn {
    Box::new(|_| Box::pin(async { Ok(()) }))
}

pub struct ControllerDef {
    pub register_fn: fn(&mut dyn Any),
    pub route_log_fn: fn(),
    provider: ProviderDef,
}

impl ControllerDef {
    pub fn of<C: Controller + Injectable + 'static>() -> Self {
        Self {
            register_fn: |any| C::register_routes(any),
            route_log_fn: || crate::log_controller_routes::<C>(),
            provider: ProviderDef::of::<C>(),
        }
    }
}
pub struct ModuleDef {
    pub(crate) register_fn: for<'a> fn(&'a mut Container) -> BoxFuture<'a, crate::Result<()>>,
    pub(crate) bootstrap_fn: for<'a> fn(&'a Container) -> BoxFuture<'a, crate::Result<()>>,
    pub(crate) shutdown_fn: for<'a> fn(&'a Container) -> BoxFuture<'a, crate::Result<()>>,
    pub(crate) controller_register_fn: fn(&mut dyn Any),
    pub(crate) route_log_fn: fn(),
    pub(crate) validate_fn: fn(&Container) -> crate::Result<()>,
}

impl ModuleDef {
    pub fn of<M: Module + 'static>() -> Self {
        Self {
            register_fn: |container| Box::pin(async move { register_module::<M>(container).await }),
            bootstrap_fn: |container| {
                Box::pin(async move { bootstrap_module::<M>(container).await })
            },
            shutdown_fn: |container| Box::pin(async move { shutdown_module::<M>(container).await }),
            controller_register_fn: |any| register_module_controllers::<M>(any),
            route_log_fn: || crate::log_module_routes::<M>(),
            validate_fn: |container| validate_module_providers::<M>(container),
        }
    }
}

pub trait Module {
    fn register() -> ModuleMetadata;
}
pub struct ModuleMetadata {
    pub imports: Vec<ModuleDef>,
    pub providers: Vec<ProviderDef>,
    pub controllers: Vec<ControllerDef>,
    pub event_handlers: Vec<EventHandlerDef>,
}

impl ModuleMetadata {
    pub fn new() -> Self {
        Self {
            imports: vec![],
            providers: vec![],
            controllers: vec![],
            event_handlers: vec![],
        }
    }

    pub fn import<M: Module + 'static>(mut self) -> Self {
        self.imports.push(ModuleDef::of::<M>());
        self
    }

    pub fn provider<T: Injectable>(mut self) -> Self {
        self.providers.push(ProviderDef::of::<T>());
        self
    }

    pub fn provider_async_factory<T, Fut, E>(
        mut self,
        factory: impl Fn(Arc<Container>) -> Fut + Send + Sync + 'static,
    ) -> Self
    where
        T: Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<T, E>> + Send + 'static,
        E: Debug + Send + 'static,
    {
        self.providers
            .push(ProviderDef::async_factory::<T, Fut, E>(factory));
        self
    }

    pub fn controller<C: Controller + Injectable + 'static>(mut self) -> Self {
        self.controllers.push(ControllerDef::of::<C>());
        self
    }

    pub fn event_handler<H>(mut self) -> Self
    where
        H: RegisterableEventHandler + EventHandler<H::Event>,
    {
        self.event_handlers.push(EventHandlerDef::of::<H>());
        self
    }

    pub fn event_handler_for<E, H>(mut self) -> Self
    where
        E: Clone + Send + Sync + 'static,
        H: Injectable + EventHandler<E>,
    {
        self.event_handlers
            .push(EventHandlerDef::for_event::<E, H>());
        self
    }
}

impl Default for ModuleMetadata {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn register_module<M: Module>(container: &mut Container) -> crate::Result<()> {
    let module_start = Instant::now();
    let metadata = M::register();

    for import in &metadata.imports {
        (import.register_fn)(container).await?;
    }

    for provider in &metadata.providers {
        register_provider_def(container, provider).await?;
    }

    for controller in &metadata.controllers {
        register_provider_def(container, &controller.provider).await?;
    }

    for handler in &metadata.event_handlers {
        handler.assert_registered(container)?;
        handler.register(container)?;
    }

    crate::log_module_initialized(std::any::type_name::<M>(), module_start.elapsed());
    Ok(())
}

async fn register_provider_def(
    container: &mut Container,
    declared: &ProviderDef,
) -> crate::Result<()> {
    // Once a type has been satisfied by an override, ignore later declarations
    // of the same TypeId so production registrations cannot replace the test
    // double (registration overwrites by TypeId).
    if container.was_overridden(declared.type_id) {
        return Ok(());
    }

    let provider_start = Instant::now();
    let override_def = container.take_pending_override(declared.type_id);
    if override_def.is_some() {
        container.mark_override_applied(declared.type_id);
    }
    let effective = override_def.as_ref().unwrap_or(declared);

    let value = (effective.build)(container).await?;
    container.register_erased(effective.type_id, value.clone());
    effective
        .run_lifecycle(&value, "on_module_init", &effective.init_fn)
        .await?;
    crate::log_provider_initialized(effective.type_name, provider_start.elapsed());
    Ok(())
}

pub async fn bootstrap_module<M: Module>(container: &Container) -> crate::Result<()> {
    let metadata = M::register();

    for import in &metadata.imports {
        (import.bootstrap_fn)(container).await?;
    }

    for provider in &metadata.providers {
        if container.was_overridden(provider.type_id) {
            continue;
        }
        provider
            .run_lifecycle_from_container(container, "on_bootstrap", &provider.bootstrap_fn)
            .await?;
    }

    for controller in &metadata.controllers {
        if container.was_overridden(controller.provider.type_id) {
            continue;
        }
        controller
            .provider
            .run_lifecycle_from_container(
                container,
                "on_bootstrap",
                &controller.provider.bootstrap_fn,
            )
            .await?;
    }

    Ok(())
}

pub async fn shutdown_module<M: Module>(container: &Container) -> crate::Result<()> {
    let metadata = M::register();

    for controller in metadata.controllers.iter().rev() {
        if container.was_overridden(controller.provider.type_id) {
            continue;
        }
        controller
            .provider
            .run_lifecycle_from_container(
                container,
                "on_shutdown",
                &controller.provider.shutdown_fn,
            )
            .await?;
    }

    for provider in metadata.providers.iter().rev() {
        if container.was_overridden(provider.type_id) {
            continue;
        }
        provider
            .run_lifecycle_from_container(container, "on_shutdown", &provider.shutdown_fn)
            .await?;
    }

    for import in metadata.imports.iter().rev() {
        (import.shutdown_fn)(container).await?;
    }

    Ok(())
}

pub async fn build_container<M: Module>() -> crate::Result<Container> {
    build_container_with_overrides::<M>(ProviderOverrides::new()).await
}

pub async fn build_container_with_overrides<M: Module>(
    overrides: ProviderOverrides,
) -> crate::Result<Container> {
    crate::log_application_starting();
    let mut container = Container::new();
    container.seed_overrides(overrides);
    register_module::<M>(&mut container).await?;
    container.assert_no_unused_overrides()?;
    validate_module_providers::<M>(&container)?;
    bootstrap_module::<M>(&container).await?;
    Ok(container)
}

pub fn validate_module_providers<M: Module>(container: &Container) -> crate::Result<()> {
    let metadata = M::register();

    for import in &metadata.imports {
        (import.validate_fn)(container)?;
    }

    for provider in &metadata.providers {
        provider.assert_registered(container)?;
    }

    for controller in &metadata.controllers {
        controller.provider.assert_registered(container)?;
    }

    for handler in &metadata.event_handlers {
        handler.assert_registered(container)?;
    }

    Ok(())
}

pub fn register_module_controllers<M: Module>(any: &mut dyn Any) {
    let metadata = M::register();

    for import in &metadata.imports {
        (import.controller_register_fn)(any)
    }

    for controller in &metadata.controllers {
        (controller.register_fn)(any)
    }
}
