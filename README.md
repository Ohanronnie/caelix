# Caelix

Caelix is a Rust backend framework with explicit modules, dependency injection,
controllers, lifecycle hooks, validation, events, WebSockets, and Actix or Axum
runtime support.

[Documentation](https://ohanronnie.github.io/caelix/) · [API docs](https://docs.rs/caelix) · [crates.io](https://crates.io/crates/caelix) · [GitHub](https://github.com/Ohanronnie/caelix)

## Install

```sh
cargo add caelix
```

Caelix uses Actix by default. For Axum:

```toml
caelix = { version = "0.0.18", default-features = false, features = ["axum"] }
```

## CLI

```sh
cargo install caelix-cli
caelix new my-app
```

See the [CLI guide](https://ohanronnie.github.io/caelix/cli/guide.html) for generation, runtime, and update commands.

## Packages

| Package | Purpose |
| --- | --- |
| [caelix](https://crates.io/crates/caelix) | Public framework facade |
| [caelix-core](https://crates.io/crates/caelix-core) | Modules, DI, lifecycle, events, and HTTP primitives |
| [caelix-actix](https://crates.io/crates/caelix-actix) | Actix Web runtime adapter |
| [caelix-axum](https://crates.io/crates/caelix-axum) | Axum runtime adapter |
| [caelix-socketio](https://crates.io/crates/caelix-socketio) | Socket.IO integration for Axum |
| [caelix-macros](https://crates.io/crates/caelix-macros) | `#[injectable]`, `#[controller]`, and related macros |
| [caelix-cli](https://crates.io/crates/caelix-cli) | Application and feature generator |
