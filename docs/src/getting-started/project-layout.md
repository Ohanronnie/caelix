# Project Layout

The generator creates a small root module. Feature code usually lives under `src/<feature>/`.

```text
src/
  app.rs
  lib.rs
  main.rs
  users/
    mod.rs
    service.rs
    controller.rs
```

Feature modules typically export their service, controller, and module types from `mod.rs`, then the root app imports the feature module:

```rust
use caelix::{Module, ModuleMetadata};

use crate::users::UsersModule;

pub struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().import::<UsersModule>()
    }
}
```

Feature names are normalized by the CLI. `users` maps to `src/users`; `auth-session` maps to `src/auth_session`, route path `/auth-session`, and types such as `AuthSessionModule`, `AuthSessionService`, and `AuthSessionController`.
