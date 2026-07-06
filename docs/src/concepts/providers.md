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

Implement `Injectable` manually when construction needs owned state, custom initialization, or lifecycle hooks.

```rust
use std::{collections::BTreeMap, sync::Mutex};

use caelix::{BoxFuture, Container, Injectable, Result};

pub struct UsersService {
    users: Mutex<BTreeMap<i64, UserDto>>,
}

impl Injectable for UsersService {
    fn create(_container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async {
            Self {
                users: Mutex::new(BTreeMap::new()),
            }
        })
    }

    fn on_module_init(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
```

Manual providers resolve dependencies through `Container`:

```rust
impl Injectable for UsersService {
    fn create(container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async move {
            Self {
                repository: container.resolve::<UsersRepository>(),
                logger: container.resolve_logger("UsersService"),
            }
        })
    }
}
```

## Async Factories

Use an async factory when construction needs fallible async work, such as opening a database pool.

```rust
use std::sync::Arc;

use caelix::{Container, Module, ModuleMetadata};

pub struct DatabasePool;

pub struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider_async_factory::<DatabasePool, _, _>(|_container: Arc<Container>| async move {
                DatabasePool::connect("postgres://localhost/app").await
            })
    }
}
```

The factory receives an `Arc<Container>`, so it can resolve providers registered earlier:

```rust
ModuleMetadata::new()
    .provider::<Config>()
    .provider_async_factory::<DatabasePool, _, _>(|container: Arc<Container>| async move {
        let config = container.resolve::<Config>();
        DatabasePool::connect(&config.database_url).await
    })
```

Async factory providers are construction-only. They use no-op lifecycle callbacks, so providers that need `on_module_init`, `on_bootstrap`, or `on_shutdown` should implement `Injectable` directly and use `.provider::<T>()`.

## Provider Visibility

Providers are visible after registration. Imports are processed first, then the importing module's providers, then its controllers. This allows controllers to inject providers from the same module or any earlier imported module.

If a dependency is missing, `container.resolve::<T>()` panics during provider construction. During `Application::try_new`, metadata validation also catches missing provider definitions and returns startup errors for declared-but-unregistered providers.
