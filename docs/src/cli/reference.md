# Command Reference

## `caelix new`

```sh
caelix new <name>
```

Creates a new Caelix application directory named `<name>`.

The command creates `Cargo.toml`, `src/main.rs`, `src/lib.rs`, and `src/app.rs`. The package name comes from the target directory and is converted to kebab case.

## `caelix generate module`

```sh
caelix generate module <name>
caelix g module <name>
```

Creates a feature module, service, and controller below `src/<normalized-name>/`.

## `caelix generate service`

```sh
caelix generate service <name>
caelix g service <name>
```

Creates `service.rs` and creates `mod.rs` only when missing.

## `caelix generate controller`

```sh
caelix generate controller <name>
caelix g controller <name>
```

Creates `controller.rs` and creates `mod.rs` only when missing. A matching service is injected only when `src/<feature>/service.rs` already exists.

If the service does not exist, the controller is generated without a service dependency and the CLI prints a note.

## Name Rules

Names are trimmed and cannot be empty or contain `/` or `\`. The normalized Rust module name cannot start with an ASCII digit.

Examples:

| Input | Directory | Route | Types |
| --- | --- | --- | --- |
| `users` | `src/users` | `/users` | `UsersModule`, `UsersService`, `UsersController` |
| `auth-session` | `src/auth_session` | `/auth-session` | `AuthSessionModule`, `AuthSessionService`, `AuthSessionController` |
| `admin users` | `src/admin_users` | `/admin-users` | `AdminUsersModule`, `AdminUsersService`, `AdminUsersController` |

Invalid names include an empty string, names containing `/` or `\`, and names whose normalized Rust module name starts with an ASCII digit.

## Overwrite Rules

The CLI refuses to overwrite files. Typical errors:

```text
src/users/service.rs already exists; refusing to overwrite
```

## Registration

Generation does not edit `src/app.rs` or `src/lib.rs`. Register generated modules manually:

```rust
pub mod users;
```

```rust
use crate::users::UsersModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().import::<UsersModule>()
    }
}
```
