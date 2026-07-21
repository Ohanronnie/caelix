# Feature Flags

`caelix` enables only the Actix runtime by default:

```toml
caelix = "0.0.25"
```

The default feature is:

- `actix`: enables `Application`, `#[caelix::main]`, and Actix Web runtime support.

Database error conversion, request validation, multipart uploads, and OpenAPI
generation are opt-in so they do not add dependencies to an Actix-only build:

```toml
[dependencies]
caelix = { version = "0.0.25", features = ["sqlx", "validator", "uploads", "openapi"] }
```

To state the minimal Actix selection explicitly, disable defaults and select
`actix`:

```toml
[dependencies]
caelix = { version = "0.0.25", default-features = false, features = ["actix"] }
```

Generated applications depend directly on `caelix`; they do not need additional Caelix package dependencies.

## Axum backend

Actix remains the default backend. To use Axum, disable defaults and select `axum`; `actix` and `axum` are mutually exclusive.

```toml
[dependencies]
caelix = { version = "0.0.25", default-features = false, features = ["axum"] }
```

The same `#[controller]`, route, extractor, guard, interceptor, `#[gateway]`, and
`#[caelix::main]` source works on either backend.

## Socket.IO backend extension

The `socketio` feature selects Axum automatically and is structurally unavailable with the
default Actix-only build. It exposes `caelix::socket_io` and
`Application::with_socket_io::<AppModule>()`.

```toml
[dependencies]
caelix = { version = "0.0.25", default-features = false, features = ["socketio"] }
```

Example generated dependencies:

```toml
[dependencies]
actix-web = "4.14.0"
caelix = "0.0.25"
serde = { version = "1.0.228", features = ["derive"] }
```
