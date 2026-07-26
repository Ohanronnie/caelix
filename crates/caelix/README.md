# Caelix

Caelix is a Rust backend framework with explicit modules, dependency injection,
controllers, cookies, validated uploads, OpenAPI, structured logging, events,
WebSockets, typed NATS/Redis microservices, and Actix or Axum runtime support.

```sh
cargo add caelix
```

Actix is enabled by default. For Axum:

```toml
caelix = { version = "0.0.31", default-features = false, features = ["axum"] }
```

Broker services can select `microservices-nats`, `microservices-redis`, or both.

[Documentation](https://ohanronnie.github.io/caelix/) · [API docs](https://docs.rs/caelix) · [crates.io](https://crates.io/crates/caelix) · [GitHub](https://github.com/Ohanronnie/caelix)
