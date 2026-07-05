use crate::{Container, Injectable};

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

pub struct ModuleDef {
    register_fn: fn(&mut Container),
}

impl ModuleDef {
    pub fn of<M: Module + 'static>() -> Self {
        Self {
            register_fn: |container| register_module::<M>(container),
        }
    }
}

pub trait Module {
    fn register() -> ModuleMetadata;
}
pub struct ModuleMetadata {
    pub imports: Vec<ModuleDef>,
    pub providers: Vec<ProviderDef>,
}

impl ModuleMetadata {
    pub fn new() -> Self {
        Self {
            imports: vec![],
            providers: vec![],
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

    pub fn providers<T: Injectable>(self) -> Self {
        self.provider::<T>()
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
}

pub fn build_container<M: Module>() -> Container {
    let mut container = Container::new();
    register_module::<M>(&mut container);
    container
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    pub struct Repo;

    impl Repo {
        pub fn greet(&self) -> String {
            "hello from Repo".to_string()
        }
    }

    impl Injectable for Repo {
        fn create(_container: &Container) -> Self {
            Repo
        }
    }

    pub struct Service {
        repo: Arc<Repo>,
    }

    impl Service {
        pub fn call_repo(&self) -> String {
            self.repo.greet()
        }
    }

    impl Injectable for Service {
        fn create(container: &Container) -> Self {
            Self {
                repo: container.resolve::<Repo>(),
            }
        }
    }

    pub struct UserModule;

    impl Module for UserModule {
        fn register() -> ModuleMetadata {
            ModuleMetadata::new()
                .provider::<Repo>()
                .provider::<Service>()
        }
    }

    pub struct AppModule;

    impl Module for AppModule {
        fn register() -> ModuleMetadata {
            ModuleMetadata::new().import::<UserModule>()
        }
    }

    #[test]
    fn resolves_deeply_nested_provider_through_app_module() {
        let container = build_container::<AppModule>();

        let service = container.resolve::<Service>();
        assert_eq!(service.call_repo(), "hello from Repo");
    }
}
