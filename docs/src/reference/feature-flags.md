# Feature Flags

Caelix keeps its public package small by making HTTP adapters and optional
application capabilities explicit Cargo features. `actix` is the default. Add
only the capabilities used by the application; all controller and module source
continues to depend on the `caelix` facade, not an adapter crate.

## All available features

| Feature               | What it enables                                                                                                       | Use it when                                              | Compatibility                                             |
| --------------------- | --------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- | --------------------------------------------------------- |
| `actix`               | The default HTTP adapter: `Application`, `TestApplication`, `Logging`, Actix response conversion, and runtime macros. | You want Caelix’s default server integration.            | Mutually exclusive with `axum`.                           |
| `axum`                | Axum `Application`, `TestApplication`, `AxumRouterBuilder`, Axum response conversion, and runtime macros.             | You need native Axum routing or Tower layers.            | Mutually exclusive with `actix`.                          |
| `socketio`            | Socket.IO gateway types and `Application::with_socket_io`.                                                            | The service exposes Socket.IO namespaces/events.         | Selects `axum`; unavailable with Actix.                   |
| `sqlx`                | Compatibility conversion for SQLx errors through `?`.                                                                 | Your application uses SQLx.                              | Combines with either HTTP adapter or a broker-only app.   |
| `validator`           | Controller `#[validate]` support and the `validator` re-export.                                                       | HTTP DTOs need request validation.                       | Combines with either HTTP adapter.                        |
| `config`              | `Config`, `ConfigFile`, `ConfigModule`, `Deserialize`, and `Validate` derives for typed configuration.                | Configuration comes from environment variables or files. | Combines with HTTP or microservice features.              |
| `openapi`             | OpenAPI document generation, Swagger UI, `OpenApiConfig`, `ToSchema`, and documentation marker attributes.            | The application publishes an OpenAPI contract.           | Needs an HTTP adapter to serve its routes.                |
| `uploads`             | `UploadedFile`, `MultipartForm`, upload extractors, and multipart request configuration.                              | A controller accepts files or multipart fields.          | Combines with either HTTP adapter.                        |
| `microservices-nats`  | Typed microservice macros, runtime, client, and NATS transport options.                                               | The process handles NATS commands or JetStream events.   | Does not require an HTTP adapter.                         |
| `microservices-redis` | The same typed microservice API plus Redis transport options.                                                         | The process handles Redis Stream commands/events.        | Does not require an HTTP adapter; Redis 6.2+ is required. |

There are no hidden application features. This table is the complete feature
set exported by the public `caelix` package.

## Choose an HTTP adapter

The default dependency selects Actix:

```sh
cargo add caelix
```

To select Axum, disable the default before enabling it:

```sh
cargo add caelix --no-default-features --features axum
```

Never enable `actix` and `axum` together. If the application needs Socket.IO,
start from Axum because `socketio` selects it:

```sh
cargo add caelix --no-default-features --features axum,socketio
```

See [Choosing Actix or Axum](../why-caelix/choosing-actix-or-axum.md) for the
runtime decision and [Axum and Tower](../advanced/axum.md) for native Axum
composition.

## Add HTTP capabilities

Features compose with the chosen HTTP adapter. This is a typical Actix API with
typed configuration, validation, upload handling, and OpenAPI:

```sh
cargo add caelix --features config,validator,uploads,openapi
```

For an Axum application, retain `--no-default-features` and list everything in
one place:

```sh
cargo add caelix --no-default-features --features axum,config,validator,uploads,openapi
```

Each capability has its own guide:

- [Typed Configuration](../advanced/configuration.md)
- [Validation](../concepts/validation.md)
- [Multipart Uploads](../advanced/multipart-uploads.md)
- [OpenAPI and Swagger UI](../advanced/openapi.md)

`config` supplies the derives needed by configuration types. `validator` is
still the explicit feature for controller `#[validate]` and the public
`caelix::validator` re-export.

## SQLx, without a framework database layer

Caelix does not select an ORM or database driver. Enable `sqlx` only when an
application uses SQLx and wants its error type to convert through `?` into a
Caelix result:

```sh
cargo add caelix --features sqlx
```

Bring the SQLx driver/database features in through the application’s own SQLx
dependency. The integration pattern is documented in [Database and
Persistence](../advanced/persistence.md).

## Build a broker-only process

Microservice features do not start an HTTP runtime. Disable default features
for a broker consumer:

```sh
# NATS command and JetStream event consumer
cargo add caelix --no-default-features --features microservices-nats

# Redis Streams command and event consumer
cargo add caelix --no-default-features --features microservices-redis

# One process with both transport option types
cargo add caelix --no-default-features --features microservices-nats,microservices-redis
```

An HTTP runtime can coexist with either microservice transport when one binary
owns both boundaries. The module graph remains explicit; register controllers
and microservice handlers in the modules that own them. Start with
[Microservices](../advanced/microservices.md).

## Feature-selection checklist

1. Select exactly one HTTP adapter, or neither for a broker-only process.
2. Add `socketio` only with Axum.
3. Add `config`, `validator`, `uploads`, and `openapi` only when application
   code uses their APIs.
4. Add `sqlx` only for SQLx compatibility; it does not create a pool or run
   migrations.
5. Add one or both microservice transports only when the process connects to
   those brokers.

Generated controller and runtime code uses hidden facade re-exports. Depend on
`caelix`, not `caelix-actix`, `caelix-axum`, or `caelix-macros` directly.
