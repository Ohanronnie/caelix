# Modules

A module implements `Module` and returns `ModuleMetadata`. Metadata is the only thing Caelix needs to build the dependency graph and route table.

```rust
use caelix::{CacheModule, Module, ModuleMetadata};

pub struct UsersModule;

impl Module for UsersModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<CacheModule>()
            .provider::<UsersService>()
            .controller::<UsersController>()
    }
}
```

`ModuleMetadata` is a builder:

```rust
pub struct ModuleMetadata {
    pub imports: Vec<ModuleDef>,
    pub providers: Vec<ProviderDef>,
    pub controllers: Vec<ControllerDef>,
    pub event_handlers: Vec<EventHandlerDef>,
}

impl ModuleMetadata {
    pub fn new() -> Self;
    pub fn import<M: Module + 'static>(self) -> Self;
    pub fn provider<T: Injectable>(self) -> Self;
    pub fn provider_async_factory<T, Fut, E>(self, factory: impl Fn(Arc<Container>) -> Fut + Send + Sync + 'static) -> Self;
    pub fn controller<C: Controller + Injectable + 'static>(self) -> Self;
    pub fn event_handler<H>(self) -> Self;
    pub fn event_handler_for<E, H>(self) -> Self;
}
```

The common registrations are:

- `.import::<OtherModule>()`
- `.provider::<T>()`
- `.provider_async_factory::<T, _, _>(factory)`
- `.controller::<C>()`
- `.event_handler::<H>()`
- `.event_handler_for::<Event, H>()`

## Registration Order

Imported modules are registered before the module that imports them. Inside a module, providers are registered before controllers because controllers are providers too. Event handlers are registered after their provider instances exist.

This means an imported module's providers are visible to later providers and controllers:

```rust
pub struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<DatabaseModule>()
            .import::<UsersModule>()
    }
}
```

If `DatabaseModule` registers `DatabasePool`, a provider in `UsersModule` can resolve `Arc<DatabasePool>`.

Registration order is also overwrite order. The container stores one provider instance per concrete Rust type. If two imported modules register the same type, the later registration replaces the earlier instance.

## Controllers Are Providers

`.controller::<UsersController>()` adds a controller definition and a provider definition for the same type. Controller dependencies are resolved from the container exactly like service dependencies.

```rust
#[injectable]
pub struct UsersController {
    users: Arc<UsersService>,
}
```

Do not also register the same controller with `.provider::<UsersController>()`; `.controller::<UsersController>()` already constructs it.

## Startup Failures

Application startup fails when metadata references a provider that was not registered into the container. Common causes are:

- A controller depends on `Arc<UsersService>`, but `UsersService` is not registered with `.provider::<UsersService>()`.
- An event handler is listed with `.event_handler::<SendWelcomeEmail>()`, but the handler was not also registered as a provider.
- A factory provider returns an error.
- A lifecycle hook returns an `Err(HttpException)`.

`Application::new::<AppModule>()` panics on these startup errors. `Application::try_new::<AppModule>()` returns them as `caelix::Result<Application>`.
