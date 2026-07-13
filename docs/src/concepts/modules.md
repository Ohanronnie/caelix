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
    pub fn global() -> Self;
    pub fn export<T: Send + Sync + 'static>(self) -> Self;
    pub fn provider_async_factory<T, Fut, E>(self, dependencies: Vec<ProviderDependency>, factory: impl Fn(Arc<Container>) -> Fut + Send + Sync + 'static) -> Self;
    pub fn controller<C: Controller + Injectable + 'static>(self) -> Self;
    pub fn event_handler<H>(self) -> Self;
    pub fn event_handler_for<E, H>(self) -> Self;
}
```

The common registrations are:

- `.import::<OtherModule>()`
- `.provider::<T>()`
- `.provider_async_factory::<T, _, _>(provider_dependencies![...], factory)`
- `.controller::<C>()`
- `.event_handler::<H>()`
- `.event_handler_for::<Event, H>()`

## Visibility and Registration Order

Imported modules are registered before the module that imports them. Inside a module, providers are registered before controllers because controllers are providers too. Event handlers are registered after their provider instances exist. Modules that emit events or register handlers must import `EventModule` before providers that inject `Arc<EventBus>`.

An import does not make every provider public. Export the provider from its
owning module, then import that module where it is consumed:

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

If `DatabaseModule` registers `DatabasePool`, it must call
`.export::<DatabasePool>()` before a provider in `UsersModule` can resolve
`Arc<DatabasePool>`. A module may re-export an export from one of its direct
imports. Global modules use `ModuleMetadata::global()` and make only their
explicit exports visible application-wide after they are imported somewhere in
the reachable graph.

Each module and provider type has one owner. Repeated module imports are
deduplicated, while duplicate production provider registrations are startup
errors rather than overwrite order.

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
- A provider injects `Arc<EventBus>` or a module registers event handlers, but the module did not import `EventModule`.
- A provider uses a private or non-exported dependency from another module.
- A factory provider returns an error.
- A lifecycle hook returns an `Err(HttpException)`.

`Application::new::<AppModule>()` returns these startup errors as `caelix::Result<Application>`.
