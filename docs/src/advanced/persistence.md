# Database And Persistence

Caelix does not prescribe an ORM, SQL driver, migration tool, or database. It
does provide the application structure needed to integrate any of them cleanly:
create the client or pool during startup, register it explicitly, inject it
into repositories and services, and replace it in tests.

This guide uses a `DatabasePool` placeholder so the pattern works with SQLx,
Diesel, SeaORM, a Redis client, or an application-specific data client.

The startup example uses Caelix typed configuration:

```sh
cargo add caelix --features config
```

## Create a pool at startup

External clients commonly need fallible asynchronous construction. Register
them with `provider_async_factory` and declare every provider dependency the
factory resolves.

```rust
use std::sync::Arc;

use caelix::{
    Config, ConfigModule, Container, Deserialize, Module, ModuleMetadata,
    Result, Validate, provider_dependencies,
};

#[derive(Config, Deserialize, Validate)]
pub struct DatabaseConfig {
    #[config(env = "DATABASE_URL")]
    pub url: String,
}

pub struct DatabasePool;

impl DatabasePool {
    async fn connect(url: &str) -> Result<Self> {
        let _ = url;
        Ok(Self)
    }
}

pub struct DatabaseModule;

impl Module for DatabaseModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<ConfigModule<DatabaseConfig>>()
            .provider_async_factory::<DatabasePool, _, _>(
                provider_dependencies![DatabaseConfig],
                |container: Arc<Container>| async move {
                    let config = container.resolve::<DatabaseConfig>()?;
                    DatabasePool::connect(&config.url).await
                },
            )
            .export::<DatabasePool>()
    }
}
```

Startup fails before the server accepts requests if the factory cannot create
the pool. An application’s configuration provider should supply the real URL;
see [Typed Configuration](configuration.md) for environment and file loading.

`provider_async_factory` is the right choice for a foreign pool type because
Rust’s orphan rules normally prevent implementing Caelix’s `Injectable` trait
for both a foreign trait and a foreign type. Factory providers are
construction-only; use an application-owned wrapper with `#[injectable]` if
the resource needs Caelix lifecycle hooks.

## Put queries behind a repository

Inject the pool into a repository, then inject that repository into the service
that owns application behavior. Controllers remain responsible only for HTTP.

```rust
use std::sync::Arc;

use caelix::{Result, injectable};

#[injectable]
pub struct PostsRepository {
    pool: Arc<DatabasePool>,
}

impl PostsRepository {
    pub async fn find(&self, id: i64) -> Result<Post> {
        let _ = &self.pool;
        // Run a parameterized query with the selected database library.
        Ok(Post { id, author_id: 42 })
    }

    pub async fn delete(&self, id: i64) -> Result<()> {
        let _ = (&self.pool, id);
        Ok(())
    }
}

#[injectable]
pub struct PostsService {
    posts: Arc<PostsRepository>,
}

impl PostsService {
    pub async fn delete(&self, actor_id: i64, id: i64) -> Result<()> {
        let post = self.posts.find(id).await?;
        if post.author_id != actor_id {
            return Err(caelix::ForbiddenException::new("Not allowed"));
        }
        self.posts.delete(id).await
    }
}
```

Register the repository in the feature module and import `DatabaseModule`.
The pool must be explicitly exported by `DatabaseModule`; imports alone do not
make a provider visible to another module.

## Transactions and units of work

Keep a transaction’s lifetime inside the service method that owns the business
operation. Pass a transaction or repository-specific executor to the
repositories that participate in that operation; do not store a request-bound
transaction in a singleton provider or in `RequestContext`.

For an operation that writes a record and publishes an event, commit the
database transaction before calling `EventBus::emit`, or use your database
library’s outbox pattern when the write and delivery must survive process
failure together. Caelix event handlers run after `emit`; they are not a
database transaction coordinator.

## Migrations

Run migrations as an explicit deployment or startup decision. A production
application usually uses one of these patterns:

- a separate migration command/job run before rolling out application replicas;
- an application-owned migration provider that runs once under a deployment
  lock; or
- an orchestration job managed by the platform.

Avoid having every replica independently apply schema changes during normal
startup unless the migration tool and deployment process make that safe.

## Test with a real database or an override

Use `TestApplication` with a database configured for the test when a test must
exercise SQL, migrations, transactions, or driver mapping. For a service-level
HTTP test that does not need the real database, replace the exact concrete
provider type that the module injects:

```rust
let app = TestApplication::new::<AppModule>()
    .override_provider(DatabasePool::for_test())
    .await?;
```

The override must have the same concrete type as production registration.
Alternatively, register an application-owned `PostsRepository` with an
in-memory constructor and override that repository. See [Testing](../recipes/testing.md)
for the complete override rules.

## Persistence checklist

- Keep connection URLs and credentials in configuration, never source code.
- Build pools once at startup and inject `Arc<Pool>` into long-lived providers.
- Use parameterized queries and map driver/database errors to deliberate
  Caelix exceptions at the service boundary where appropriate.
- Give writes an idempotency strategy when they can be retried by HTTP clients
  or microservice delivery.
- Bound pool size and query timeouts to the capacity of the database.
- Run migrations deliberately and test schema changes against the selected
  database engine.
