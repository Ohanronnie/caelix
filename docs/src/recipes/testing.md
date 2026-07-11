# Testing

Caelix supports two testing styles: unit tests against the DI container, and full in-process HTTP tests with `TestApplication`.

## Unit tests

Build a `Container`, register dependencies, and call methods directly:

```rust
#[tokio::test]
async fn service_returns_user() -> caelix::Result<()> {
    let mut container = Container::new();
    container.register::<UsersService>().await?;

    let service = container.resolve::<UsersService>()?;
    let user = service.find(1).await?;

    assert_eq!(user.id, 1);
    Ok(())
}
```

You can also use `build_container::<AppModule>()` or `build_container_with_overrides` when you want the full module graph without HTTP.

## Integration tests with `TestApplication`

`TestApplication` builds the same container and route table as production, then serves requests through the selected runtime's in-memory service (no TCP listener). The API is the same when the Actix or Axum backend is selected.

```rust
use caelix::{StatusCode, TestApplication};

#[caelix::test]
async fn should_create_user() -> caelix::Result<()> {
    let app = TestApplication::new::<AppModule>().await?;

    let response = app
        .post("/users")
        .json(CreateUserDto {
            name: "Ronnie".into(),
            email: "ronnie@example.com".into(),
        })
        .send()
        .await?;

    response.assert_status(StatusCode::CREATED);
    Ok(())
}
```

`#[caelix::test]` runs the selected backend runtime, so it is suitable for
`TestApplication` tests with either backend. It expands through Caelix's hidden
runtime re-exports, so a direct Actix or Axum runtime dependency is not needed
just to use the test macro.

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
async fn creates_user_without_database() -> caelix::Result<()> {
    let app = TestApplication::new::<AppModule>()
        .override_provider(UserRepository::in_memory())
        .await?;

    let response = app
        .post("/users")
        .json(CreateUserDto {
            name: "Ronnie".into(),
            email: "ronnie@example.com".into(),
        })
        .send()
        .await?;

    response.assert_status(StatusCode::CREATED);
    // resolve the same type the app injects
    let _repo = app.resolve::<UserRepository>()?;
    Ok(())
}
```

Overrides match by `TypeId`. The replacement **must be the same concrete type** `T` that modules register and inject as `Arc<T>`. Typical pattern: give the production type a test constructor (for example `UserRepository::in_memory()`) rather than a separate mock struct type.

You can also use `.override_provider_factory(...)` for async construction, and `.body_limit(n)` to match `Application::body_limit`.

An override for a type that never appears in the module tree is a startup error.

Instance overrides use NestJS-style `useValue` semantics: declared lifecycle hooks (`on_module_init`, `on_bootstrap`, `on_shutdown`) for that type are skipped.

### Shutdown

```rust
app.shutdown().await?;
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
