# Providers

Providers implement `Injectable`. Caelix constructs providers during application startup and stores them as `Arc<T>` in the container.

## Macro Providers

The `#[injectable]` macro implements `Injectable` for unit structs and named-field structs.

```rust
use std::sync::Arc;

use caelix::{Logger, injectable};

#[injectable]
pub struct UsersService {
    logger: Arc<Logger>,
    repository: Arc<UsersRepository>,
}
```

Every named field must be `Arc<T>`. The macro resolves each field from the container. `Arc<Logger>` is special: it receives a logger scoped to the struct name instead of resolving the default application logger.

Unit structs work well for stateless services:

```rust
#[injectable]
pub struct HealthService;

impl HealthService {
    pub fn status(&self) -> &'static str {
        "ok"
    }
}
```

Tuple structs and non-struct items are rejected by the macro. Named fields that are not `Arc<T>` are also rejected.

## Manual Injectable

Implement `Injectable` manually when construction needs owned state, custom
initialization, or lifecycle hooks. A manual implementation has two parts:

- `dependencies()` declares every provider type resolved with
  `container.resolve::<T>()` in `create`.
- `create()` performs the actual resolution and construction.

Use `provider_dependencies![...]` for the declaration. It is required: omitting
`dependencies()` is a compile error, including for a dependency-free provider.
This is also enforced at construction time. Caelix gives `create` a scoped
container and rejects `container.resolve::<T>()` when `T` is absent from the
declaration. The list therefore cannot be used to bypass module visibility.

Caelix uses the declaration before construction to check module visibility,
report missing dependencies, arrange startup order, and reject dependency
cycles. A scoped logger from `container.resolve_logger(...)` is not a provider
dependency, so do not list `Logger`.

```rust
use std::sync::Arc;

use caelix::{
    BoxFuture, Container, Injectable, Logger, ProviderDependency, Result,
    provider_dependencies,
};

pub struct UsersService {
    repository: Arc<UsersRepository>,
    logger: Arc<Logger>,
}

impl Injectable for UsersService {
    fn dependencies() -> Vec<ProviderDependency> {
        provider_dependencies![UsersRepository]
    }

    fn create(container: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async move {
            Ok(Self {
                repository: container.resolve::<UsersRepository>()?,
                logger: container.resolve_logger("UsersService"),
            })
        })
    }

    fn on_module_init(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
```

Keep the declaration in sync with `create`. If a provider resolves two
services, it declares both:

```rust
impl Injectable for ReportService {
    fn dependencies() -> Vec<ProviderDependency> {
        provider_dependencies![UsersRepository, AuditService]
    }

    fn create(container: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async move {
            Ok(Self {
                repository: container.resolve::<UsersRepository>()?,
                audit: container.resolve::<AuditService>()?,
            })
        })
    }
}
```

For a dependency-free manual provider, return `provider_dependencies![]`.
Dependencies must also be visible to the module: declare them locally or
import a module that explicitly exports them. The declaration applies only
during construction; resolving application services later from a request or
runtime-owned container is not part of this provider-construction contract.

## Async Factories

Use an async factory when construction needs fallible async work, such as
opening a database pool. Its first argument is always a dependency declaration,
including `provider_dependencies![]` when the factory resolves nothing. It has
the same scheduling and visibility role as `Injectable::dependencies()`.

```rust
use std::sync::Arc;

use caelix::{Container, Module, ModuleMetadata, provider_dependencies};

pub struct DatabasePool;

pub struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider_async_factory::<DatabasePool, _, _>(
                provider_dependencies![],
                |_container: Arc<Container>| async move {
                    DatabasePool::connect("postgres://localhost/app").await
                },
            )
    }
}
```

The factory receives an `Arc<Container>`. Every provider type resolved from it
must appear in the first argument; closures cannot be inspected to infer this
list. Caelix applies the same scoped-resolution check used for handwritten
`Injectable` implementations. A scoped logger obtained through
`container.resolve_logger(...)` is not a provider dependency.

```rust
ModuleMetadata::new()
    .provider::<Config>()
    .provider_async_factory::<DatabasePool, _, _>(
        provider_dependencies![Config],
        |container: Arc<Container>| async move {
            let config = container.resolve::<Config>()?;
            DatabasePool::connect(&config.database_url).await
        },
    )
```

Factory dependencies follow normal module visibility rules. If `Config` is
owned by `ConfigModule`, `ConfigModule` must export `Config` and the module
declaring the factory must import `ConfigModule`. Declaring `Config` in the
factory list does not make it public or register it.

Async factory providers are construction-only. They use no-op lifecycle callbacks, so providers that need `on_module_init`, `on_bootstrap`, or `on_shutdown` should implement `Injectable` directly and use `.provider::<T>()`.

## Owned Resources

An application-owned provider can store external resources directly:

```rust
pub struct AppConfig {
    pub database_url: String,
    pub pool: PgPool,
}
```

When `AppConfig` is registered with `.provider::<AppConfig>()`, services can inject `Arc<AppConfig>` and use `config.pool`. This does not register `PgPool` as its own provider. Injecting `Arc<PgPool>` only works if `PgPool` itself is registered separately.

Application crates usually cannot write `impl Injectable for PgPool` because Rust's orphan rules prevent implementing a foreign trait for a foreign type. For fallible startup errors instead of `expect` failures, keep using `.provider_async_factory::<PgPool, _, _>(...)` or wrap the pool in an application-owned newtype and implement `Injectable` for that wrapper.

## Provider Visibility

Providers are visible only within their declaring module, through explicit exports
from direct imports, or through explicit exports of global modules. Imports are
not registration-order visibility.

`#[injectable]` records `Arc<T>` fields automatically. Manual implementations
and async factories must declare each resolved dependency with
`provider_dependencies![T, ...]`. The declaration is mandatory for handwritten
providers, and Caelix rejects `container.resolve::<T>()` during construction
when `T` is absent from it.
