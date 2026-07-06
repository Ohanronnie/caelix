# Testing

Use normal Rust tests for services and guards. Build a `Container`, register dependencies, and call methods directly.

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

For controller behavior, prefer adapter-level tests that initialize an Actix app with the same module metadata used in production.

CLI-generated projects can be checked with normal Cargo commands after generation:

```sh
caelix new demo-api
cd demo-api
cargo test
```

Useful application checks:

```sh
cargo metadata --no-deps --format-version 1
cargo test
cargo check
```
