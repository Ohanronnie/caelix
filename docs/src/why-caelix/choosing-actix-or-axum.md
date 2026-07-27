# Choosing Actix or Axum

Caelix supports one HTTP adapter per application: Actix or Axum. Start from
the application structure you want—modules, providers, and controllers—then
choose the adapter that best fits the runtime boundary and integrations you
need.

Actix is the default Caelix integration. Axum is the right choice when the
application needs native Axum router composition, Tower middleware, or
Caelix's Axum-only Socket.IO support.

## What stays the same

Caelix application code is shared across the adapters. The following remain
the same when they use Caelix abstractions:

| Area                       | Shared Caelix behavior                                              |
| -------------------------- | ------------------------------------------------------------------- |
| Composition                | Modules, imports, providers, and explicit registration              |
| HTTP application code      | Controllers, route attributes, extractors, guards, and interceptors |
| Dependencies and lifecycle | Typed injection plus initialization, bootstrap, and shutdown hooks  |
| Testing                    | `TestApplication`, provider overrides, and request/response helpers |
| Responses and failures     | Caelix responses and exception-to-error-response behavior           |

Keep business logic and controller code on these abstractions when portability
matters. Native Actix or Axum types can still be used at the boundary, but that
integration code belongs to the adapter it targets.

## Use Actix by default

The ordinary `caelix` dependency selects Actix:

```sh
cargo add caelix
```

Choose it when Caelix's default application runtime suits the service and no
Axum-specific integration is required. The usual `Application`,
`TestApplication`, and `#[caelix::main]` APIs are available through the
`caelix` facade.

## Choose Axum for native router and Tower access

Select Axum explicitly when the application needs to compose its Caelix router
with native Axum routes, use Axum extractors at the integration boundary, or
apply Tower layers such as tracing, compression, CORS, or request IDs.

```sh
cargo add caelix --no-default-features --features axum
```

The `actix` and `axum` features are mutually exclusive. Direct Axum and Tokio
dependencies are only needed when application code calls their native APIs.

```rust
let router = caelix::Application::new::<AppModule>()
    .await?
    .into_router()
    .route("/native", axum::routing::get(native_health));
```

For the router boundary, native response conversion, and compatible Tower
layers, read [Axum and Tower](../advanced/axum.md). For the complete feature
matrix and combinations, read [Feature Flags](../reference/feature-flags.md).

## Choose Axum for Socket.IO

Caelix's Socket.IO integration is available only with Axum. Disable default
features and enable `socketio`, which selects the Axum adapter:

```sh
cargo add caelix --no-default-features --features socketio
```

Standard RFC 6455 WebSocket gateways work with both adapters. See
[WebSockets](../advanced/websockets.md) for the Socket.IO and WebSocket APIs.

## A practical boundary

Choose an adapter for its runtime-facing capabilities, not to rewrite the
application. Keep module registration, providers, controllers, guards,
interceptors, and tests in Caelix; isolate native router, middleware, and
transport code at the edge. This gives an application a stable internal model
while still allowing it to use the ecosystem of its selected backend.
