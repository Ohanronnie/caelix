# Minimal Application

A minimal app needs a module and an entry point for the selected backend (Actix is the default; Axum is enabled with the `axum` feature).

```rust
use caelix::{Module, ModuleMetadata};

pub struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
    }
}
```

```rust
use caelix::Application;
use demo_api::AppModule;

#[caelix::main]
async fn main() -> std::io::Result<()> {
    Application::new::<AppModule>()
        .await
        .map_err(|err| std::io::Error::other(err.message))?
        .listen("127.0.0.1:8080")
        .await
}
```

`Application::new::<AppModule>()` builds the dependency container, validates module metadata, registers controller routes, runs startup lifecycle hooks, and logs the route table.

Startup errors are returned from `Application::new`, so application code decides whether to propagate or handle them.
