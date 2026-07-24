# Axum and Tower

Caelix has an Axum runtime adapter for applications that want Axum's router,
extractors, and Tower middleware ecosystem. The framework-level source stays
the same: modules, providers, controllers, guards, interceptors, and route
attributes are still registered through Caelix metadata.

This page focuses on what the Axum adapter does at the runtime boundary. For
the protocol-specific APIs, see [WebSockets](websockets.md) and
[Socket.IO support](websockets.md#socketio-support-axum-only).

## Select the Axum backend

Actix is the default backend, so an Axum application disables default features
and enables `axum` explicitly:

```toml
[dependencies]
caelix = { version = "0.0.26", default-features = false, features = ["axum"] }
```

The `actix` and `axum` features are mutually exclusive. Most applications
only need the `caelix` dependency and the `#[caelix::main]` macro. Add direct
Axum and Tokio dependencies when your application calls native Axum APIs such
as `axum::serve` or `tokio::net::TcpListener`:

```toml
[dependencies]
caelix = { version = "0.0.26", default-features = false, features = ["axum"] }
axum = "0.8.8"
tokio = { version = "1.47.1", features = ["macros", "net", "rt-multi-thread"] }
```

Caelix's generated controller code uses its own hidden Axum re-exports, so
controllers do not need to import Axum types just to use `#[controller]`.
Direct Axum dependencies are useful when embedding the returned router or
adding native routes and extractors.

## Minimal Axum application

Every Axum application starts with a module. The module registration model is
the same as on Actix:

```rust
use caelix::{Module, ModuleMetadata};

pub struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
    }
}
```

The Caelix runtime owns the listener and server lifecycle when `listen` is
used:

```rust
use caelix::Application;

#[caelix::main]
async fn main() -> std::io::Result<()> {
    Application::new::<AppModule>()
        .await
        .map_err(|error| std::io::Error::other(error.message))?
        .listen("127.0.0.1:3000")
        .await
}
```

`Application::new` performs the startup work before binding a TCP listener:

1. Builds the dependency container from module metadata.
2. Registers providers, controllers, gateways, and lifecycle handlers.
3. Validates declarations and runs bootstrap hooks.
4. Logs the module routes and returns an Axum `Application`.

If startup fails, the error is returned from `Application::new` and the server
does not start. When `listen` returns after the server stops, Caelix runs
shutdown hooks. If listener binding fails, it also attempts shutdown hooks
before returning the bind error.

## Configure the application

`Application` is a builder. Configure it before calling `listen` or
`into_router`:

```rust
let app = Application::new::<AppModule>()
    .await?
    .body_limit(2 * 1024 * 1024);

app.listen("127.0.0.1:3000").await?;
```

The default HTTP body limit is 1 MiB. It applies to body-consuming Caelix
routes including JSON bodies and multipart uploads, and `body_limit` changes
the limit in bytes. Configure `upload_temp_dir(path)` when multipart file
staging must use an application-specific directory.
The value is applied through Axum's `DefaultBodyLimit` layer when the router
is built.

WebSocket messages have their own limit. Configure that separately with
`websocket_max_message_size`; see [Axum WebSocket support](websockets.md#axum-websocket-support).

## Controllers and Axum extractors

Controller source does not change for Axum. Caelix maps its framework-neutral
extractor attributes to Axum extractors internally:

| Caelix attribute | Axum extractor | Meaning |
| --- | --- | --- |
| `#[param]` | `axum::extract::Path<T>` | Route parameters such as `{id}` |
| `#[query]` | `axum::extract::Query<T>` | Query-string values |
| `#[body]` | Caelix negotiated request wrapper | JSON or typed multipart text fields |
| `#[file]` | Caelix multipart file binding | One required or optional `UploadedFile` |
| `#[files]` | Caelix multipart file binding | Repeated `Vec<UploadedFile>` field |
| `#[multipart]` | Caelix multipart form binding | Direct `MultipartForm` access |
| `#[user]` | `RequestContext` lookup | A typed value installed by a guard or interceptor |

The controller method receives the inner value (`T`), not `Path<T>`,
`Query<T>`, or `Json<T>`:

```rust
use serde::{Deserialize, Serialize};
use caelix::{
    controller, injectable, Module, ModuleMetadata, Response, Result, StatusCode,
};

#[derive(Deserialize)]
struct CreateUser {
    name: String,
}

#[derive(Serialize)]
struct User {
    id: String,
    name: String,
}

#[injectable]
struct UsersController;

#[controller("/users")]
impl UsersController {
    #[get("/{id}")]
    async fn find(&self, #[param] id: String) -> Result<User> {
        Ok(User {
            id,
            name: "Ada".to_owned(),
        })
    }

    #[post("")]
    async fn create(&self, #[body] input: CreateUser) -> Result<Response<User>> {
        Ok(Response::WithStatus(
            StatusCode::CREATED,
            User {
                id: "new-user".to_owned(),
                name: input.name,
            },
        ))
    }
}

struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().controller::<UsersController>()
    }
}
```

The generated Axum handler also receives the shared container through
`State<Arc<Container>>`. Application code normally does not see that state;
Caelix resolves the controller and its dependencies before invoking the
method.

### Request context, guards, and interceptors

When a route uses a guard, interceptor, or `#[user]`, Caelix builds a
framework-neutral `RequestContext` from the Axum request method, path, and
headers. The same guard and interceptor implementations can be used on both
backends:

```rust
use caelix::{
    BoxFuture, Guard, RequestContext, Result, UnauthorizedException, guard,
};

#[guard]
struct ApiKeyGuard;

impl Guard for ApiKeyGuard {
    fn can_activate<'a>(
        &'a self,
        context: &'a RequestContext,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            if context.header("x-api-key") == Some("development-key") {
                Ok(true)
            } else {
                Err(UnauthorizedException::new("invalid API key"))
            }
        })
    }
}
```

Attach it at controller or method level with `#[use_guard(ApiKeyGuard)]`.
Controller-level guards run before method-level guards. Interceptors receive
the same `RequestContext` and can transform the framework-neutral
`HttpResponse`; they do not need to depend on Axum's response type.

### Extractor failures

Path and query extraction happen in Axum before the generated handler body
runs. Caelix handles JSON and multipart body extraction through its shared
negotiated wrapper, yielding the same `400`, `413`, and `415` response
categories as the Actix adapter. A Caelix exception returned by the controller,
guard, interceptor, or provider is converted through Caelix's response adapter.

See [Multipart Uploads](multipart-uploads.md) for field binding, limits,
validation, and file persistence.

Use `#[validate]` when the value was successfully deserialized but its fields
need application-level validation:

```rust
use serde::Deserialize;
use validator::Validate;

#[derive(Deserialize, Validate)]
struct SearchUsers {
    #[validate(length(min = 1))]
    q: String,
}

#[get("/search")]
async fn search(
    &self,
    #[query] #[validate] query: SearchUsers,
) -> Result<Vec<User>> {
    // query.q has already passed validator::Validate::validate.
    Ok(Vec::new())
}
```

## Responses in Axum

Controller return values are first converted to Caelix's framework-neutral
`HttpResponse`, then `caelix::to_axum_response` converts that response into an
Axum `Response`.

Common controller returns include:

```rust
use caelix::{HttpResponse, Response, Result, StatusCode};

let json = Response::Body(value);                         // 200 JSON
let created = Response::WithStatus(StatusCode::CREATED, value);
let empty = Response::no_content();                       // 204
let text = Response::text(StatusCode::OK, "ready");
let bytes = Response::bytes(StatusCode::OK, data);
```

For streaming endpoints, return `Result<HttpResponse>` and use
`Response::stream`, `Response::sse`, or `Response::file`. The Axum adapter
turns buffered bodies into `axum::body::Body` directly and turns streaming
bodies into Axum stream bodies. If a stream yields an error after the response
has started, Caelix logs the error and ends the body; it cannot change the
already-sent status code.

### Native Axum response conversion

When an application adds a native Axum handler, it can still use Caelix's
response helpers and convert the result explicitly:

```rust
use caelix::{HttpResponse, StatusCode};

async fn native_health() -> axum::response::Response {
    caelix::to_axum_response(HttpResponse::text(StatusCode::OK, "ok"))
}
```

`to_axum_response` applies the Caelix content type and all response headers.
This is useful when a native Axum route shares response construction with a
Caelix controller or service.

## Compose with an Axum router

`into_router` is the integration boundary. It returns a fully configured Axum
router containing Caelix controller routes, WebSocket routes, body limits,
and the Caelix JSON fallback for unmatched requests.

Use it when another component owns the listener:

```rust
let app = Application::new::<AppModule>()
    .await?
    .into_router();

let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
axum::serve(listener, app).await?;
```

You can add native Axum routes after obtaining the router:

```rust
use axum::{Router, routing::get};

let app: Router = Application::new::<AppModule>()
    .await?
    .into_router()
    .route("/native", get(native_health));
```

Keep Caelix routes registered through controllers and modules. Add native
routes for Axum-only integrations that do not need Caelix controller metadata.
If a native handler needs shared application state, use Axum's normal `State`
extractor and merge it before serving, taking care to preserve Caelix's
router state requirements.

### `AxumRequestInfo`

`AxumRequestInfo` is a parts-only extractor exposed by Caelix for native Axum
handlers that need request metadata without consuming the body:

```rust
use caelix::AxumRequestInfo;

async fn request_info(info: AxumRequestInfo) -> String {
    format!("{} {}", info.method(), info.path())
}
```

It provides `method()`, `path()`, and `headers()`. Because it implements
`FromRequestParts`, it can be combined with a body-consuming extractor in a
native Axum handler. Caelix uses the same request information internally when
it builds a `RequestContext` for guarded, intercepted, or authenticated
controller routes.

## Tower middleware

`Application::layer` accepts a compatible Tower layer and applies it to the
underlying Axum router:

```rust
use caelix::Application;
use tower_http::{compression::CompressionLayer, trace::TraceLayer};

let app = Application::new::<AppModule>()
    .await?
    .layer(TraceLayer::new_for_http())
    .layer(CompressionLayer::new())
    .into_router();
```

This works with tracing, compression, CORS, request IDs, authentication, and
rate-limiting layers from the Tower ecosystem. Add the relevant crate to your
application:

```toml
[dependencies]
tower-http = { version = "0.6.8", features = ["compression-full", "trace"] }
```

Layers are attached to the router after Caelix has registered its routes, so
they can observe normal controller requests and WebSocket upgrade requests.
Message-level WebSocket authorization still belongs in the gateway; Tower
middleware sees the HTTP upgrade boundary, not each WebSocket message.

A layer must satisfy the adapter's service bounds: its service must return an
Axum `IntoResponse`, use an error convertible to `Infallible`, and expose a
`Send` future. If a third-party layer does not meet those bounds, wrap it in a
compatible Tower service or install it after `into_router` using native Axum
composition.

## WebSockets and Socket.IO

Axum RFC 6455 gateways are registered with the same `#[gateway]` and
`ModuleMetadata::gateway` APIs as Actix gateways. The Axum adapter mounts them
when `into_router` or `listen` builds the application. See the dedicated
[Axum WebSocket support](websockets.md#axum-websocket-support) section for
message callbacks, request metadata, close frames, message limits, and
browser clients.

Socket.IO is an Axum-only optional feature. Enable it with
`features = ["socketio"]`, call `with_socket_io::<AppModule>()` before
`listen` or `into_router`, and use the Socket.IO client protocol. See the
[Socket.IO guide](websockets.md#socketio-support-axum-only) for namespaces,
events, acknowledgements, rooms, and client examples.

## Testing Axum applications

Axum provides the same in-process `TestApplication` API as the Actix adapter.
It builds the production container and routes without opening a TCP listener:

```rust
use caelix::{StatusCode, TestApplication};

#[caelix::test]
async fn health_route_works() {
    let app = TestApplication::new::<AppModule>().await.unwrap();
    let body = app
        .get("/health")
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::OK)
        .json::<serde_json::Value>()
        .await;

    assert_eq!(body["status"], "ok");
}
```

The builder supports `override_provider`, `override_provider_factory`, and
`body_limit`. Request helpers include `get`, `post`, `put`, `patch`, and
`delete`, with `json`, `header`, `set_payload`, and `send`; response helpers
include `status`, `assert_status`, `json`, `body`, and `text`. Call
`app.shutdown().await` when tests need provider shutdown hooks to run.

For lower-level Axum or Tower integration tests, you can still build the router
directly and send requests with `ServiceExt::oneshot`:

```toml
[dev-dependencies]
http-body-util = "0.1.3"
tower = { version = "0.5.3", features = ["util"] }
```

```rust
use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn health_route_works() {
    let app = caelix::Application::new::<AppModule>()
        .await
        .unwrap()
        .into_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), br#"{\"status\":\"ok\"}"#);
}
```

For WebSocket behavior, start an ephemeral `tokio::net::TcpListener`, serve
`into_router()`, and connect with a WebSocket client such as
`tokio-tungstenite`. For Socket.IO behavior, use a real Socket.IO client so
the Engine.IO handshake, namespace, acknowledgement, and room behavior are
tested together.

## Common integration patterns

### Put shared state in providers

Prefer injectable services for database pools, repositories, clients, and
configuration. Controllers resolve them from the Caelix container regardless
of whether Axum or Actix is selected:

```rust
use std::sync::Arc;
use caelix::{injectable, Module, ModuleMetadata};

#[injectable]
struct UsersService {
    repository: Arc<UsersRepository>,
}

#[injectable]
struct UsersRepository;

struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<UsersRepository>()
            .provider::<UsersService>()
            .controller::<UsersController>()
    }
}
```

This keeps Axum-specific router wiring at the application boundary and keeps
business logic portable across runtimes.

### Use native Axum only at the boundary

Use native Axum routes or layers when an integration requires Axum types. Keep
controller methods, providers, guards, and interceptors on Caelix abstractions
when the code should remain portable. This gives one module graph and one
dependency-injection model while still allowing Axum's ecosystem at the edge.
