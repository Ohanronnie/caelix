# Testing

Caelix supports two testing styles: unit tests against the DI container, and full in-process HTTP tests with `TestApplication`.

## Unit tests

Build a `Container`, register dependencies, and call methods directly:

```rust
#[tokio::test]
async fn service_returns_user() {
    let mut container = Container::new();
    container.register::<UsersService>().await;

    let service = container.resolve::<UsersService>();
    let user = service.find(1).await.unwrap();

    assert_eq!(user.id, 1);
}
```

You can also use `try_build_container::<AppModule>()` or `try_build_container_with_overrides` when you want the full module graph without HTTP.

## Integration tests with `TestApplication`

`TestApplication` builds the same container and route table as production, then serves requests through Actix's in-memory test service (no TCP listener).

```rust
use caelix::{StatusCode, TestApplication};

#[caelix::test]
async fn should_create_user() {
    let app = TestApplication::new::<AppModule>().await;

    let response = app
        .post("/users")
        .json(CreateUserDto {
            name: "Ronnie".into(),
            email: "ronnie@example.com".into(),
        })
        .send()
        .await;

    response.assert_status(StatusCode::CREATED);
}
```

`#[caelix::test]` runs the Actix runtime (equivalent to `#[actix_web::test]`). Prefer it for any test that uses `TestApplication`. It expands through Caelix’s Actix re-export, so a direct `actix-web` dependency is **not** required—only `caelix` with the `actix` feature (the default).

### Request helpers

| Method | Purpose |
|--------|---------|
| `app.get/post/put/patch/delete(path)` | Start a request |
| `.json(value)` | JSON body + content type |
| `.header(name, value)` | Set a header |
| `.set_payload(bytes)` | Raw body |
| `.send().await` | Execute against the in-process app |

### Response helpers

| Method | Purpose |
|--------|---------|
| `.status()` | `http::StatusCode` |
| `.assert_status(code)` | Panic on mismatch; returns `self` for chaining |
| `.json::<T>().await` | Deserialize JSON body |
| `.body().await` / `.text().await` | Raw bytes / UTF-8 text |

### Provider overrides

Swap a concrete provider (including ones registered in nested imports) before the container is built:

```rust
#[caelix::test]
async fn creates_user_without_database() {
    let app = TestApplication::new::<AppModule>()
        .override_provider(UserRepository::in_memory())
        .await;

    let response = app
        .post("/users")
        .json(CreateUserDto {
            name: "Ronnie".into(),
            email: "ronnie@example.com".into(),
        })
        .send()
        .await;

    response.assert_status(StatusCode::CREATED);
    // resolve the same type the app injects
    let _repo = app.resolve::<UserRepository>();
}
```

Overrides match by `TypeId`. The replacement **must be the same concrete type** `T` that modules register and inject as `Arc<T>`. Typical pattern: give the production type a test constructor (for example `UserRepository::in_memory()`) rather than a separate mock struct type.

You can also use `.override_provider_factory(...)` for async construction, and `.body_limit(n)` to match `Application::body_limit`.

An override for a type that never appears in the module tree is a startup error.

Instance overrides use NestJS-style `useValue` semantics: declared lifecycle hooks (`on_module_init`, `on_bootstrap`, `on_shutdown`) for that type are skipped.

### Shutdown

```rust
app.shutdown().await.unwrap();
```

Dropping a `TestApplication` without `shutdown` skips `on_shutdown` hooks. Call `shutdown` when your test cares about cleanup.

## CLI-generated projects

```sh
caelix new demo-api
cd demo-api
cargo test
```

Useful checks:

```sh
cargo test
cargo test -p caelix-actix
cargo check
```
