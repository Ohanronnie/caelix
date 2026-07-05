use crate::{BoxFuture, Container, Controller, Injectable};
use std::{
    any::{Any, TypeId},
    fmt::Debug,
    future::Future,
    sync::Arc,
    time::Instant,
};

pub struct ProviderDef {
    type_id: TypeId,
    type_name: &'static str,
    build: Box<
        dyn for<'a> Fn(&'a Container) -> BoxFuture<'a, Arc<dyn Any + Send + Sync>> + Send + Sync,
    >,
}

impl ProviderDef {
    pub fn of<T: Injectable>() -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            build: Box::new(|container| {
                Box::pin(async move {
                    let value = T::create(container).await;
                    Arc::new(value) as Arc<dyn Any + Send + Sync>
                })
            }),
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
                    let value = future.await.unwrap_or_else(|err| {
                        panic!(
                            "async factory failed for {}: {:?}",
                            std::any::type_name::<T>(),
                            err
                        )
                    });

                    Arc::new(value) as Arc<dyn Any + Send + Sync>
                })
            }),
        }
    }

    fn assert_registered(&self, container: &Container) {
        assert!(
            container.contains_type_id(self.type_id),
            "missing provider at startup: {} was declared by module metadata but was not registered",
            self.type_name
        );
    }
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
    pub(crate) register_fn: for<'a> fn(&'a mut Container) -> BoxFuture<'a, ()>,
    pub(crate) controller_register_fn: fn(&mut dyn Any),
    pub(crate) route_log_fn: fn(),
    pub(crate) validate_fn: fn(&Container),
}

impl ModuleDef {
    pub fn of<M: Module + 'static>() -> Self {
        Self {
            register_fn: |container| Box::pin(async move { register_module::<M>(container).await }),
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
}

impl ModuleMetadata {
    pub fn new() -> Self {
        Self {
            imports: vec![],
            providers: vec![],
            controllers: vec![],
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
}

pub async fn register_module<M: Module>(container: &mut Container) {
    let module_start = Instant::now();
    let metadata = M::register();

    for import in &metadata.imports {
        (import.register_fn)(container).await
    }

    for provider in &metadata.providers {
        let provider_start = Instant::now();
        let value = (provider.build)(container).await;
        container.register_erased(provider.type_id, value);
        crate::log_provider_initialized(provider.type_name, provider_start.elapsed());
    }

    for controller in &metadata.controllers {
        let provider_start = Instant::now();
        let value = (controller.provider.build)(container).await;
        container.register_erased(controller.provider.type_id, value);
        crate::log_provider_initialized(controller.provider.type_name, provider_start.elapsed());
    }

    crate::log_module_initialized(std::any::type_name::<M>(), module_start.elapsed());
}

pub async fn build_container<M: Module>() -> Container {
    crate::log_application_starting();
    let mut container = Container::new();
    register_module::<M>(&mut container).await;
    validate_module_providers::<M>(&container);
    container
}

pub fn validate_module_providers<M: Module>(container: &Container) {
    let metadata = M::register();

    for import in &metadata.imports {
        (import.validate_fn)(container)
    }

    for provider in &metadata.providers {
        provider.assert_registered(container);
    }

    for controller in &metadata.controllers {
        controller.provider.assert_registered(container);
    }
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
