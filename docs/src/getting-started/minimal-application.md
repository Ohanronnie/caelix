# Minimal Application

A minimal app needs a module and an Actix entry point.

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
        .listen("127.0.0.1:8080")
        .await
}
```

`Application::new::<AppModule>()` builds the dependency container, validates module metadata, registers controller routes, runs startup lifecycle hooks, and logs the route table.

Use `Application::try_new::<AppModule>()` when startup errors should be returned instead of panicking.
