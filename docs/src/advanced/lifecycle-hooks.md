# Lifecycle Hooks

`Injectable` has default lifecycle hooks:

```rust
fn on_module_init(&self) -> BoxFuture<'_, Result<()>>
fn on_bootstrap(&self) -> BoxFuture<'_, Result<()>>
fn on_shutdown(&self) -> BoxFuture<'_, Result<()>>
```

The `#[injectable]` macro uses the default no-op hooks. Implement `Injectable` manually when a provider needs custom lifecycle behavior.

```rust
use std::sync::atomic::{AtomicBool, Ordering};

use caelix::{BoxFuture, Container, Injectable, ProviderDependency, Result, provider_dependencies};

pub struct Worker {
    started: AtomicBool,
}

impl Injectable for Worker {
    fn dependencies() -> Vec<ProviderDependency> {
        provider_dependencies![]
    }

    fn create(_container: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async {
            Ok(Self {
                started: AtomicBool::new(false),
            })
        })
    }

    fn on_module_init(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            // Run after this provider is constructed, during registration.
            Ok(())
        })
    }

    fn on_bootstrap(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.started.store(true, Ordering::SeqCst);
            Ok(())
        })
    }

    fn on_shutdown(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.started.store(false, Ordering::SeqCst);
            Ok(())
        })
    }
}
```

## When Hooks Run

- `on_module_init` runs when an injectable provider is registered.
- `on_bootstrap` runs after module provider validation and event handler registration.
- `on_shutdown` runs when `Application::listen` exits or when startup reaches a bind failure after the application was built.

Controllers are providers, so controller types can define lifecycle hooks when they implement `Injectable` manually.

## Failures

Lifecycle failures are converted into startup or shutdown errors that include the hook name and provider type. `Application::new` returns startup failures as `caelix::Result<Application>`.

```rust
fn on_bootstrap(&self) -> BoxFuture<'_, Result<()>> {
    Box::pin(async {
        Err(caelix::ServiceUnavailableException::new("worker failed to start"))
    })
}
```

For 5xx exceptions, the client response body is sanitized. Startup errors are returned to the caller, and generated controller routes log returned 5xx exceptions server-side before sending the sanitized response.

## Async Factory Limitation

Providers registered with `.provider_async_factory::<T, _, _>(provider_dependencies![...], ...)` are construction-only. Caelix stores no lifecycle callbacks for the concrete type, so factory providers use no-op `on_module_init`, `on_bootstrap`, and `on_shutdown`. If a provider needs lifecycle hooks, implement `Injectable` and register it with `.provider::<T>()`.
