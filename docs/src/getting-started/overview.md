# Overview

Caelix applications are built from a root module. Modules describe imported modules, injectable providers, controllers, and event handlers. The Actix adapter reads that metadata, builds a dependency container, registers controller routes, runs lifecycle hooks, and starts the HTTP server.

The public package is `caelix`. It exports the runtime, framework traits, controller and provider macros, responses, guards, interceptors, events, cache types, and Actix application entry point.

## Start A Project

Install the CLI from crates.io:

```sh
cargo install caelix-cli
```

Create and run an application:

```sh
caelix new demo-api
cd demo-api
cargo run
```

The generated application uses `caelix = "0.0.1"` from crates.io:

```text
demo-api/
  Cargo.toml
  src/
    app.rs
    lib.rs
    main.rs
```

`src/main.rs` starts the Actix adapter. `src/app.rs` contains the root `AppModule`.

## Build A Users Feature

Generate a feature module:

```sh
caelix g module users
```

The command creates:

```text
src/users/
  controller.rs
  mod.rs
  service.rs
```

Generated feature files are not wired into the root app automatically. Register the module in `src/lib.rs` and `src/app.rs`:

```rust
// src/lib.rs
pub mod app;
pub mod users;

pub use app::AppModule;
```

```rust
// src/app.rs
use caelix::{Module, ModuleMetadata};

use crate::users::UsersModule;

pub struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().import::<UsersModule>()
    }
}
```

Now replace the generated placeholder with real DTOs and route methods.

```rust
// src/users/service.rs
use std::{collections::BTreeMap, sync::Mutex};

use caelix::{BoxFuture, ConflictException, Container, Injectable, NotFoundException, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize)]
pub struct UserDto {
    pub id: i64,
    pub email: String,
    pub name: String,
}

#[derive(Deserialize)]
pub struct CreateUserDto {
    pub email: String,
    pub name: String,
}

#[derive(Deserialize)]
pub struct ListUsersQuery {
    pub limit: Option<usize>,
}

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
}

impl UsersService {
    pub async fn list(&self, limit: Option<usize>) -> Result<Vec<UserDto>> {
        let users = self.users.lock().expect("users lock poisoned");
        Ok(users.values().take(limit.unwrap_or(50)).cloned().collect())
    }

    pub async fn find(&self, id: i64) -> Result<UserDto> {
        let users = self.users.lock().expect("users lock poisoned");
        users
            .get(&id)
            .cloned()
            .ok_or_else(|| NotFoundException::new(format!("user {id} not found")))
    }

    pub async fn create(&self, input: CreateUserDto) -> Result<UserDto> {
        let mut users = self.users.lock().expect("users lock poisoned");
        if users.values().any(|user| user.email == input.email) {
            return Err(ConflictException::new("email already exists"));
        }

        let id = users.len() as i64 + 1;
        let user = UserDto {
            id,
            email: input.email,
            name: input.name,
        };
        users.insert(id, user.clone());
        Ok(user)
    }
}
```

Then add controller routes using path params, query params, JSON bodies, typed responses, and typed errors:

```rust
// src/users/controller.rs
use std::sync::Arc;

use caelix::{Response, Result, controller, injectable};
use caelix::StatusCode;

use super::{CreateUserDto, ListUsersQuery, UserDto, UsersService};

#[injectable]
pub struct UsersController {
    users: Arc<UsersService>,
}

#[controller("/users")]
impl UsersController {
    #[get("")]
    pub async fn list(&self, #[query] query: ListUsersQuery) -> Result<Vec<UserDto>> {
        self.users.list(query.limit).await
    }

    #[get("/{id}")]
    pub async fn find(&self, #[param] id: i64) -> Result<UserDto> {
        self.users.find(id).await
    }

    #[post("")]
    pub async fn create(&self, #[body] input: CreateUserDto) -> Result<Response<UserDto>> {
        let user = self.users.create(input).await?;
        Ok(Response::WithStatus(StatusCode::CREATED, user))
    }
}
```

Export the DTOs from the feature module:

```rust
// src/users/mod.rs
mod controller;
mod service;

pub use controller::UsersController;
pub use service::{CreateUserDto, ListUsersQuery, UserDto, UsersService};

use caelix::{Module, ModuleMetadata};

pub struct UsersModule;

impl Module for UsersModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<UsersService>()
            .controller::<UsersController>()
    }
}
```

Run the app and try the feature:

```sh
cargo run
```

```sh
curl -i -X POST http://127.0.0.1:8080/users \
  -H 'content-type: application/json' \
  -d '{"email":"ada@example.com","name":"Ada"}'
```

```http
HTTP/1.1 201 Created
content-type: application/json

{"id":1,"email":"ada@example.com","name":"Ada"}
```

```sh
curl -i 'http://127.0.0.1:8080/users?limit=10'
curl -i http://127.0.0.1:8080/users/1
curl -i http://127.0.0.1:8080/users/404
```

Missing users return Caelix's standard error shape:

```json
{
  "status": 404,
  "error": "Not Found",
  "message": "user 404 not found"
}
```
