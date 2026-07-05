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
}

#[derive(Clone)]
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

    pub async fn register<T: Injectable>(&mut self) {
        let instance = T::create(self).await;
        self.services.insert(TypeId::of::<T>(), Arc::new(instance));
    }

    pub(crate) fn register_erased(&mut self, type_id: TypeId, value: Arc<dyn Any + Send + Sync>) {
        self.services.insert(type_id, value);
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
