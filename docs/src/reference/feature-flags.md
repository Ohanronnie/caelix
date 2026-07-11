# Feature Flags

`caelix` enables its current integrations by default:

```toml
caelix = "0.0.9"
```

The default features include:

- `actix`: enables `Application`, `#[caelix::main]`, and Actix Web runtime support.
- `sqlx`: enables SQLx-related error conversion support when available.
- `validator`: enables request validation support used by `#[validate]`.

Generated applications depend directly on `caelix`; they do not need additional Caelix package dependencies.

## Axum backend

Actix remains the default backend. To use Axum, disable defaults and select `axum`; `actix` and `axum` are mutually exclusive.

```toml
[dependencies]
caelix = { version = "0.0.11", default-features = false, features = ["axum", "sqlx", "validator"] }
```

The same `#[controller]`, route, extractor, guard, interceptor, `#[gateway]`, and
`#[caelix::main]` source works on either backend.

## Socket.IO backend extension

The `socketio` feature selects Axum automatically and is structurally unavailable with the
default Actix-only build. It exposes `caelix::socket_io` and
`Application::with_socket_io::<AppModule>()`.

```toml
[dependencies]
caelix = { version = "0.0.11", default-features = false, features = ["socketio"] }
```

Example generated dependencies:

```toml
[dependencies]
actix-web = "4.14.0"
caelix = "0.0.9"
serde = { version = "1.0.228", features = ["derive"] }
```
