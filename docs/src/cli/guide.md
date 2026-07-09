# CLI Guide

The CLI binary is named `caelix`.

Install it from crates.io:

```sh
cargo install caelix-cli
```

After installation, run commands with the `caelix` binary.

## Run An Application

```sh
caelix run
```

This clears the terminal, then delegates to `cargo run` in the current project. Use `--watch` to restart the application when files under `src/` or `Cargo.toml` change:

```sh
caelix run --watch
```

Arguments after `--` are passed directly to the application:

```sh
caelix run --watch -- --port 4000 --verbose
```

## Create An Application

```sh
caelix new demo-api
```

The command creates:

- `Cargo.toml`
- `AGENTS.md`
- `src/main.rs`
- `src/lib.rs`
- `src/app.rs`

The generated `Cargo.toml` uses `caelix = "0.0.8"` from crates.io.

The generated `AGENTS.md` gives AI coding agents the app-level Caelix conventions: explicit module registration, provider/controller registration, injectable field shape, service-level cache behavior, and the usual `cargo test` check.

Generated `src/main.rs` starts the Actix adapter:

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

Generated `src/app.rs` defines an empty root module:

```rust
use caelix::{Module, ModuleMetadata};

pub struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
    }
}
```

## Update Caelix

```sh
caelix update
```

This command reads the current `caelix` dependency from `Cargo.toml`, fetches the latest published `caelix` version from crates.io, and updates only that dependency when a newer version exists. It preserves existing `Cargo.toml` comments, formatting, dependency order, and feature settings.

After editing `Cargo.toml`, the CLI runs:

```sh
cargo update -p caelix
```

`caelix update` does not regenerate or re-sync scaffolded source files.

## Generate A Feature Module

```sh
caelix g module users
```

This is equivalent to:

```sh
caelix generate module users
```

It creates:

- `src/users/mod.rs`
- `src/users/service.rs`
- `src/users/controller.rs`

The generated module registers the service as a provider and the controller as a controller. The CLI prints manual registration steps for adding the module to the app.

Add the generated module to `src/lib.rs`:

```rust
pub mod users;
```

Then import it in the root module:

```rust
use crate::users::UsersModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().import::<UsersModule>()
    }
}
```

## Generate A Service

```sh
caelix g service users
```

This creates `src/users/service.rs`. If `src/users/mod.rs` does not exist, it creates a feature `mod.rs` that exports the service.

## Generate A Controller

```sh
caelix g controller users
```

This creates `src/users/controller.rs`. If `src/users/service.rs` exists, the controller injects `Arc<UsersService>` and calls it. If the service does not exist, the controller is generated without that dependency and the CLI prints a note.

Generated controller with a service:

```rust
use std::sync::Arc;

use caelix::{Result, controller, injectable};

use super::UsersService;

#[injectable]
pub struct UsersController {
    service: Arc<UsersService>,
}

#[controller("/users")]
impl UsersController {
    #[get("")]
    pub async fn hello(&self) -> Result<String> {
        Ok(self.service.hello())
    }
}
```

## Name Normalization

| Input | Directory | Route | Types |
| --- | --- | --- | --- |
| `users` | `src/users` | `/users` | `UsersModule`, `UsersService`, `UsersController` |
| `auth-session` | `src/auth_session` | `/auth-session` | `AuthSessionModule`, `AuthSessionService`, `AuthSessionController` |
| `Admin Users` | `src/admin_users` | `/admin-users` | `AdminUsersModule`, `AdminUsersService`, `AdminUsersController` |

## Overwrite Behavior

The CLI refuses to overwrite existing generated files. If any target file already exists, the command returns an error like:

```text
src/users/service.rs already exists; refusing to overwrite
```

For `caelix g module users`, all three target files must be missing: `mod.rs`, `service.rs`, and `controller.rs`.
