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

The generated application uses `caelix = "0.0.3"` from crates.io:

```text
demo-api/
  Cargo.toml
  src/
    app.rs
    lib.rs
    main.rs
```

`src/main.rs` starts the Actix adapter. `src/app.rs` contains the root `AppModule`.

## Build A Blog Feature

Generate a feature module:

```sh
cargo add sqlx --features runtime-tokio-rustls,postgres,macros
caelix g module posts
```

The command creates:

```text
src/posts/
  controller.rs
  mod.rs
  service.rs
```

Add application config as a normal provider. It can own both settings and application resources such as a SQLx pool.

```rust
// src/config.rs
use caelix::{BoxFuture, Container, Injectable};
use sqlx::PgPool;

pub struct AppConfig {
    pub database_url: String,
    pub pool: PgPool,
}

impl Injectable for AppConfig {
    fn create(_container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async {
            let database_url = std::env::var("DATABASE_URL")
                .expect("DATABASE_URL must be set");

            let pool = PgPool::connect(&database_url)
                .await
                .expect("DATABASE_URL must point to a reachable database");

            Self { database_url, pool }
        })
    }
}
```

Generated feature files are not wired into the root app automatically. Register the config and blog module in `src/lib.rs` and `src/app.rs`:

```rust
// src/lib.rs
pub mod app;
pub mod config;
pub mod posts;

pub use app::AppModule;
```

```rust
// src/app.rs
use caelix::{Module, ModuleMetadata};

use crate::{config::AppConfig, posts::PostsModule};

pub struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<AppConfig>()
            .import::<PostsModule>()
    }
}
```

Create a table for posts:

```sql
create table posts (
  id bigserial primary key,
  title text not null,
  body text not null
);
```

Now replace the generated placeholder with DTOs, SQLx writes, and a post-created event emitted after the insert succeeds.

```rust
// src/posts/service.rs
use std::sync::Arc;

use caelix::{EventBus, NotFoundException, Result, injectable};
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct PostDto {
    pub id: i64,
    pub title: String,
    pub body: String,
}

#[derive(Deserialize)]
pub struct CreatePostDto {
    pub title: String,
    pub body: String,
}

#[derive(Deserialize)]
pub struct ListPostsQuery {
    pub limit: Option<i64>,
}

#[derive(Clone)]
pub struct PostCreated {
    pub id: i64,
    pub title: String,
}

#[injectable]
pub struct PostsService {
    config: Arc<AppConfig>,
    events: Arc<EventBus>,
}

impl PostsService {
    pub async fn list(&self, limit: Option<i64>) -> Result<Vec<PostDto>> {
        let posts = sqlx::query_as::<_, PostDto>(
            "select id, title, body from posts order by id desc limit $1",
        )
        .bind(limit.unwrap_or(20))
        .fetch_all(&self.config.pool)
        .await
        .map_err(caelix::InternalServerErrorException::new)?;

        Ok(posts)
    }

    pub async fn find(&self, id: i64) -> Result<PostDto> {
        let post = sqlx::query_as::<_, PostDto>(
            "select id, title, body from posts where id = $1",
        )
        .bind(id)
        .fetch_optional(&self.config.pool)
        .await
        .map_err(caelix::InternalServerErrorException::new)?;

        post.ok_or_else(|| NotFoundException::new(format!("post {id} not found")))
    }

    pub async fn create(&self, input: CreatePostDto) -> Result<PostDto> {
        let post = sqlx::query_as::<_, PostDto>(
            "insert into posts (title, body) values ($1, $2) returning id, title, body",
        )
        .bind(input.title)
        .bind(input.body)
        .fetch_one(&self.config.pool)
        .await
        .map_err(caelix::InternalServerErrorException::new)?;

        self.events
            .emit(PostCreated {
                id: post.id,
                title: post.title.clone(),
            })
            .await?;

        Ok(post)
    }
}
```

Then add controller routes using path params, query params, JSON bodies, typed responses, and typed errors:

```rust
// src/posts/controller.rs
use std::sync::Arc;

use caelix::StatusCode;
use caelix::{Response, Result, controller, injectable};

use super::{CreatePostDto, ListPostsQuery, PostDto, PostsService};

#[injectable]
pub struct PostsController {
    posts: Arc<PostsService>,
}

#[controller("/posts")]
impl PostsController {
    #[get("")]
    pub async fn list(&self, #[query] query: ListPostsQuery) -> Result<Vec<PostDto>> {
        self.posts.list(query.limit).await
    }

    #[get("/{id}")]
    pub async fn find(&self, #[param] id: i64) -> Result<PostDto> {
        self.posts.find(id).await
    }

    #[post("")]
    pub async fn create(&self, #[body] input: CreatePostDto) -> Result<Response<PostDto>> {
        let post = self.posts.create(input).await?;
        Ok(Response::WithStatus(StatusCode::CREATED, post))
    }
}
```

Register event support explicitly with `EventModule`. Import it before `PostsService`, because the service injects `Arc<EventBus>`.

```rust
// src/posts/mod.rs
mod controller;
mod service;

pub use controller::PostsController;
pub use service::{CreatePostDto, ListPostsQuery, PostCreated, PostDto, PostsService};

use caelix::{BoxFuture, EventHandler, EventModule, Module, ModuleMetadata, RegisterableEventHandler, Result, injectable};

#[injectable]
pub struct LogPostCreated;

impl EventHandler<PostCreated> for LogPostCreated {
    fn handle(&self, event: PostCreated) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            println!("created post {}: {}", event.id, event.title);
            Ok(())
        })
    }
}

impl RegisterableEventHandler for LogPostCreated {
    type Event = PostCreated;
}

pub struct PostsModule;

impl Module for PostsModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<EventModule>()
            .provider::<PostsService>()
            .provider::<LogPostCreated>()
            .controller::<PostsController>()
            .event_handler::<LogPostCreated>()
    }
}
```

Run the app and try the feature:

```sh
cargo run
```

```sh
curl -i -X POST http://127.0.0.1:8080/posts \
  -H 'content-type: application/json' \
  -d '{"title":"First post","body":"Hello from Caelix"}'
```

```http
HTTP/1.1 201 Created
content-type: application/json

{"id":1,"title":"First post","body":"Hello from Caelix"}
```

```sh
curl -i 'http://127.0.0.1:8080/posts?limit=10'
curl -i http://127.0.0.1:8080/posts/1
curl -i http://127.0.0.1:8080/posts/404
```

Missing posts return Caelix's standard error shape:

```json
{
  "status": 404,
  "error": "Not Found",
  "message": "post 404 not found"
}
```
