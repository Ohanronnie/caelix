use crate::{Container, Controller, Injectable};
use std::any::Any;

pub struct ProviderDef {
    register_fn: fn(&mut Container),
}

impl ProviderDef {
    pub fn of<T: Injectable>() -> Self {
        Self {
            register_fn: |container| container.register::<T>(),
        }
    }
}

pub struct ControllerDef {
    pub register_fn: fn(&mut dyn Any),
    provider_register_fn: fn(&mut Container),
}

impl ControllerDef {
    pub fn of<C: Controller + Injectable + 'static>() -> Self {
        Self {
            register_fn: |any| C::register_routes(any),
            provider_register_fn: |container| container.register::<C>(),
        }
    }
}
pub struct ModuleDef {
    register_fn: fn(&mut Container),
    controller_register_fn: fn(&mut dyn Any),
}

impl ModuleDef {
    pub fn of<M: Module + 'static>() -> Self {
        Self {
            register_fn: |container| register_module::<M>(container),
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
    pub fn controller<C: Controller + Injectable + 'static>(mut self) -> Self {
        self.controllers.push(ControllerDef::of::<C>());
        self
    }
}

pub fn register_module<M: Module>(container: &mut Container) {
    let metadata = M::register();

    for import in &metadata.imports {
        (import.register_fn)(container)
    }

    for provider in &metadata.providers {
        (provider.register_fn)(container)
    }

    for controller in &metadata.controllers {
        (controller.provider_register_fn)(container)
    }
}

pub fn build_container<M: Module>() -> Container {
    let mut container = Container::new();
    register_module::<M>(&mut container);
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
