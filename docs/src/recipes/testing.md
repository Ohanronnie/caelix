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

CLI output can be tested through `caelix_cli::run_from` with a temporary directory:

```rust
let output = caelix_cli::run_from(
    ["caelix", "g", "module", "users"],
    tempdir.path(),
)?;

assert!(output.contains("Created"));
```

Useful workspace checks:

```sh
cargo metadata --no-deps --format-version 1
cargo test --workspace
cargo test -p caelix-cli
cargo check --workspace --all-features
mdbook build
```
