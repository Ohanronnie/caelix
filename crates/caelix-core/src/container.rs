use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::Arc,
};

pub trait Injectable: Send + Sync + 'static {
    fn create(container: &Container) -> Self;
}

pub struct Container {
    services: HashMap<TypeId, Arc<dyn Any + Send + Sync>>,
}

impl Container {
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
        }
    }

    pub fn register_instance<T: Send + Sync + 'static>(&mut self, value: T) {
        self.services.insert(TypeId::of::<T>(), Arc::new(value));
    }

    pub fn register<T: Injectable>(&mut self) {
        let instance = T::create(self);
        self.services.insert(TypeId::of::<T>(), Arc::new(instance));
    }

    pub fn resolve<T: Send + Sync + 'static>(&self) -> Arc<T> {
        self.services
            .get(&TypeId::of::<T>())
            .unwrap_or_else(|| panic!("no provider registered for {}", std::any::type_name::<T>()))
            .clone()
            .downcast::<T>()
            .unwrap_or_else(|_| panic!("type mismatch resolving {}", std::any::type_name::<T>()))
    }
}
