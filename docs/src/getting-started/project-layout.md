# Project Layout

The generator creates a small root module. Feature code usually lives under
`src/<feature>/`, with each feature owning its HTTP controller, service logic,
and module registration.

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

| File                      | Responsibility                                                   |
| ------------------------- | ---------------------------------------------------------------- |
| `main.rs`                 | Starts the selected Caelix `Application` and binds the listener. |
| `lib.rs`                  | Exposes the root module to the binary and integration tests.     |
| `app.rs`                  | Defines `AppModule` and imports application feature modules.     |
| `<feature>/mod.rs`        | Defines the feature module and exports its public feature types. |
| `<feature>/service.rs`    | Holds injectable business logic and dependency orchestration.    |
| `<feature>/controller.rs` | Defines request/response routes and delegates to services.       |

This layout is a convention, not a runtime requirement. Caelix does not scan
directories: only the types imported and registered in `ModuleMetadata` become
part of the application.

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

Feature names are normalized by the CLI. `users` maps to `src/users`;
`auth-session` maps to `src/auth_session`, route path `/auth-session`, and types
such as `AuthSessionModule`, `AuthSessionService`, and `AuthSessionController`.

As a feature grows, keep the module near its controller, services,
repositories, DTOs, and tests:

```text
posts/
  mod.rs
  controller.rs
  service.rs
  repository.rs
  dto.rs
  tests.rs
```

Import feature modules from `AppModule`; import infrastructure modules such as
database, configuration, or authentication only in the features that need their
exported providers. See [Modules](../concepts/modules.md) for visibility rules.
