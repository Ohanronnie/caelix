# caelix-axum

Axum runtime adapter for the [Caelix](https://github.com/Ohanronnie/caelix) Rust backend framework. It provides the Axum `Application`, testing support, controller routing, and optional Socket.IO integration.

Use it through the [`caelix`](https://crates.io/crates/caelix) facade with the `axum` feature:

```toml
caelix = { version = "0.0.18", default-features = false, features = ["axum"] }
```

[Guide](https://ohanronnie.github.io/caelix/) · [API docs](https://docs.rs/caelix-axum) · [crates.io](https://crates.io/crates/caelix-axum) · [GitHub](https://github.com/Ohanronnie/caelix)
