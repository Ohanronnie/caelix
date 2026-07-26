# Feature Flags

`caelix` defaults to `actix`. Disable defaults when selecting Axum or building a
broker-only process.

| Feature | Enables and exports | Compatibility |
| --- | --- | --- |
| `actix` | Actix `Application`, `TestApplication`, runtime macros, `Logging`, `to_actix_response` | mutually exclusive with `axum` |
| `axum` | Axum `Application`, `TestApplication`, runtime macros, `AxumRouterBuilder`, `to_axum_response` | mutually exclusive with `actix` |
| `socketio` | Socket.IO types and `Application::with_socket_io`; selects `axum` | unavailable with Actix |
| `sqlx` | Compatibility feature; SQLx errors work with `?` without enabling it | combines with either runtime |
| `validator` | `#[validate]` and `validator` re-export | combines with either runtime |
| `openapi` | OpenAPI metadata/config, Swagger UI, controller markers | requires an HTTP runtime to serve docs |
| `uploads` | `UploadedFile`, `MultipartForm`, upload extractors/configuration | combines with either runtime |
| `microservices-nats` | microservice macros, application/runtime/client, NATS options, Tokio runtime macros | no HTTP runtime required |
| `microservices-redis` | same shared microservice API plus `RedisTransportOptions` | no HTTP runtime required; Redis 6.2+ |

```toml
# Default Actix
caelix = "0.0.32"

# Axum
caelix = { version = "0.0.32", default-features = false, features = ["axum"] }

# Actix with request facilities
caelix = { version = "0.0.32", features = ["uploads", "validator", "openapi"] }

# Broker-only process with both transports
caelix = { version = "0.0.32", default-features = false, features = [
  "microservices-nats",
  "microservices-redis",
] }
```

`actix` and `axum` must never be enabled together. `socketio` selects Axum, so
disable defaults. NATS and Redis may be enabled separately or together and may
also coexist with one HTTP runtime in a combined process. Generated macro code
uses facade-only hidden exports; applications should depend on `caelix`, not
adapter or macro crates directly.
