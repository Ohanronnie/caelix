# Caelix

Caelix is a Rust backend framework with explicit modules, dependency injection, controllers, lifecycle hooks, validation, events, WebSockets, and Actix or Axum runtime support.

```sh
cargo add caelix
```

Actix is enabled by default. For Axum:

```toml
caelix = { version = "0.0.23", default-features = false, features = ["axum"] }
```

[Documentation](https://ohanronnie.github.io/caelix/) · [API docs](https://docs.rs/caelix) · [crates.io](https://crates.io/crates/caelix) · [GitHub](https://github.com/Ohanronnie/caelix)
