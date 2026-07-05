use crate::{BoxFuture, Container, Controller, Injectable};
use std::{
    any::{Any, TypeId},
    fmt::Debug,
    future::Future,
    sync::Arc,
};

pub struct ProviderDef {
    type_id: TypeId,
    build: Box<
        dyn for<'a> Fn(&'a Container) -> BoxFuture<'a, Arc<dyn Any + Send + Sync>> + Send + Sync,
    >,
}

impl ProviderDef {
    pub fn of<T: Injectable>() -> Self {
        Self {
            type_id: TypeId::of::<T>(),
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
}

pub struct ControllerDef {
    pub register_fn: fn(&mut dyn Any),
    provider: ProviderDef,
}

impl ControllerDef {
    pub fn of<C: Controller + Injectable + 'static>() -> Self {
        Self {
            register_fn: |any| C::register_routes(any),
            provider: ProviderDef::of::<C>(),
        }
    }
}
pub struct ModuleDef {
    register_fn: for<'a> fn(&'a mut Container) -> BoxFuture<'a, ()>,
    controller_register_fn: fn(&mut dyn Any),
}

impl ModuleDef {
    pub fn of<M: Module + 'static>() -> Self {
        Self {
            register_fn: |container| Box::pin(async move { register_module::<M>(container).await }),
            controller_register_fn: |any| register_module_controllers::<M>(any),
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
    let metadata = M::register();

    for import in &metadata.imports {
        (import.register_fn)(container).await
    }

    for provider in &metadata.providers {
        let value = (provider.build)(container).await;
        container.register_erased(provider.type_id, value);
    }

    for controller in &metadata.controllers {
        let value = (controller.provider.build)(container).await;
        container.register_erased(controller.provider.type_id, value);
    }
}

pub async fn build_container<M: Module>() -> Container {
    let mut container = Container::new();
    register_module::<M>(&mut container).await;
    container
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
