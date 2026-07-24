# WebSockets

Caelix supports two different real-time protocols:

- RFC 6455 WebSockets, exposed through `caelix::websocket` and available with
  both the Actix and Axum runtimes.
- Socket.IO, exposed through `caelix::socket_io` and available only when the
  Axum-selecting `socketio` feature is enabled.

They share Caelix's gateway registration model, but they are not interchangeable.
An RFC 6455 client speaks raw WebSocket frames and receives text or binary
messages. A Socket.IO client speaks the Socket.IO protocol and can use events,
acknowledgements, namespaces, and rooms.

## RFC 6455 WebSocket gateways

The gateway API below is runtime-neutral. The same `WebSocketGateway`
implementation works with Actix and Axum; each runtime mounts the registered
`#[gateway]` path as a WebSocket upgrade route. Runtime-specific setup is
covered in the [Axum WebSocket support](#axum-websocket-support) section
below.

### Define a gateway

A WebSocket gateway is an injectable provider that implements
`WebSocketGateway`. The `#[gateway("/path")]` attribute supplies the route;
the module explicitly registers the gateway just like a controller or service.

This gateway echoes text and binary messages and sends a greeting after the
handshake:

```rust
use std::sync::Arc;

use caelix::{
    gateway, injectable, BoxFuture, Bytes, Module, ModuleMetadata, Result,
};
use caelix::websocket::{WebSocketGateway, WebSocketRequest, WebSocketSession};

#[injectable]
struct ChatGateway;

#[gateway("/chat")]
impl WebSocketGateway for ChatGateway {
    fn on_connect(
        &self,
        session: Arc<WebSocketSession>,
        request: WebSocketRequest,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            println!("client {} connected to {}", session.id(), request.path());
            session.send_text("connected").await
        })
    }

    fn on_text(
        &self,
        session: Arc<WebSocketSession>,
        text: String,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            session.send_text(format!("received text: {text}")).await
        })
    }

    fn on_binary(
        &self,
        session: Arc<WebSocketSession>,
        data: Bytes,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { session.send_binary(data).await })
    }
}

struct ChatModule;

impl Module for ChatModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().gateway::<ChatGateway>()
    }
}
```

The `BoxFuture` return type is part of the framework-neutral callback trait.
For a callback that does not need to perform asynchronous work, return a
boxed async block:

```rust
fn on_text(
    &self,
    _session: Arc<WebSocketSession>,
    _text: String,
) -> BoxFuture<'_, Result<()>> {
    Box::pin(async { Ok(()) })
}
```

Gateways are ordinary injectable providers. Constructor injection, imported
modules, provider lifecycle hooks, and explicit module registration work the
same way as they do for the rest of Caelix:

```rust
struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().import::<ChatModule>()
    }
}
```

Register a gateway in either an application module or an imported module, but
do not register the same gateway path twice in the module tree.

### Callback lifecycle

`WebSocketGateway` provides five callbacks. All except `on_error` and
`on_close` can return a `caelix::Result<()>` through `BoxFuture`.

| Callback | When it runs | Typical use |
| --- | --- | --- |
| `on_connect` | After the WebSocket handshake | Inspect the request, initialize state, send a welcome message |
| `on_text` | For each complete text message | Decode commands or JSON text and send a response |
| `on_binary` | For each complete binary message | Process bytes or forward a binary payload |
| `on_error` | When a callback fails or the transport reports a protocol error | Log the failure and collect metrics |
| `on_close` | Once when the connection ends | Release application-level resources |

Fragmented frames are reassembled before `on_text` or `on_binary` runs. Ping
frames receive an automatic pong reply; application code normally only needs
to use `session.ping(...)` when it wants an explicit heartbeat.

A gateway can send messages at any time while the session is open:

```rust
fn send_heartbeat(
    &self,
    session: Arc<WebSocketSession>,
) -> BoxFuture<'_, Result<()>> {
    Box::pin(async move {
        session.ping(b"heartbeat").await?;
        session.send_text("server heartbeat").await
    })
}
```

`WebSocketSession` is cloneable and safe to share with tasks owned by the
gateway. Use `session.id()` for a per-connection identifier and
`session.is_open()` before scheduling work that may outlive a callback.

### Reading request metadata

`on_connect` receives a `WebSocketRequest` containing the path, raw query
string, optional peer address, and case-insensitive request headers:

```rust
fn on_connect(
    &self,
    session: Arc<WebSocketSession>,
    request: WebSocketRequest,
) -> BoxFuture<'_, Result<()>> {
    Box::pin(async move {
        let token = request
            .header("authorization")
            .or_else(|| request.header("x-api-key"));

        println!(
            "{} connected with query {:?} and token present: {}",
            session.id(),
            request.query_string(),
            token.is_some()
        );

        Ok(())
    })
}
```

Browser WebSocket clients cannot set arbitrary handshake headers. For browser
authentication, use a short-lived token in the query string or authenticate
the normal HTTP request before issuing the upgrade. Server-side clients can
usually send an `Authorization` header.

### Closing and handling failures

Use `WebSocketCloseFrame` and `WebSocketCloseCode` for an application-initiated
close:

```rust
use caelix::websocket::{WebSocketCloseCode, WebSocketCloseFrame};

fn stop_session(
    &self,
    session: Arc<WebSocketSession>,
) -> BoxFuture<'_, Result<()>> {
    Box::pin(async move {
        session
            .close(Some(WebSocketCloseFrame::new(
                WebSocketCloseCode::GoingAway,
                "server is shutting down",
            )))
            .await
    })
}
```

The most useful close codes are `Normal` (`1000`), `GoingAway` (`1001`),
`Policy` (`1008`), `MessageTooBig` (`1009`), and `Internal` (`1011`). A
callback error invokes `on_error` and closes the connection with `1011`. A
malformed frame invokes `on_error` and closes with `Protocol` (`1002`).

`on_close` receives an optional close frame when the runtime has one (for
example, for a peer close or a handler/protocol close). It also runs when the
transport disappears, even if there is no close frame. Keep cleanup in
`on_close` rather than in only one of the message callbacks.

```rust
fn on_error(
    &self,
    session: Arc<WebSocketSession>,
    error: caelix::websocket::WebSocketError,
) -> BoxFuture<'_, ()> {
    Box::pin(async move {
        eprintln!("websocket {} failed: {error}", session.id());
    })
}

fn on_close(
    &self,
    session: Arc<WebSocketSession>,
    frame: Option<WebSocketCloseFrame>,
) -> BoxFuture<'_, ()> {
    Box::pin(async move {
        println!("websocket {} closed: {frame:?}", session.id());
    })
}
```

### Limit message size

The default maximum complete message size is 1 MiB. Configure it before
calling `listen` or `into_router`:

```rust
let app = Application::new::<ChatModule>()
    .await?
    .websocket_max_message_size(8 * 1024 * 1024)
    .into_router();
```

The limit applies to the assembled text or binary message, not just one
fragment. Keep it proportional to the memory available to the service, and
prefer application-level limits for JSON fields or uploads that should be
smaller than the transport limit.

### Browser client

The browser's built-in `WebSocket` client speaks RFC 6455 directly:

```html
<script>
  const socket = new WebSocket(
    "ws://localhost:3000/chat?token=temporary-token"
  );
  socket.binaryType = "arraybuffer";

  socket.addEventListener("open", () => {
    socket.send("hello");
    socket.send(new Uint8Array([0, 1, 2, 255]));
  });

  socket.addEventListener("message", ({ data }) => {
    if (typeof data === "string") {
      console.log("text from server:", data);
    } else {
      console.log("binary from server:", new Uint8Array(data));
    }
  });

  socket.addEventListener("close", ({ code, reason }) => {
    console.log("closed", code, reason);
  });
</script>
```

The server's `on_text` callback receives `hello` and `on_binary` receives the
four bytes. WebSocket itself does not define event names or acknowledgements;
if an application needs those, define a message format explicitly or use
Socket.IO.

## Axum WebSocket support

This section covers the Axum runtime adapter only. The gateway callbacks,
session methods, close frames, and message semantics are the runtime-neutral
API described above.

### Enable Axum

Axum is selected explicitly because Actix is Caelix's default runtime:

```toml
[dependencies]
caelix = { version = "0.0.26", default-features = false, features = ["axum"] }
```

The `axum` feature and the `actix` feature are mutually exclusive. With this
configuration, the gateway source above is mounted by Caelix's Axum adapter.
If an application selects the Actix feature instead, the same gateway source
is mounted by the Actix adapter; it does not use Axum types.

### Start an Axum WebSocket application

`Application::new` builds the container and validates gateway metadata.
`into_router` mounts the regular HTTP routes and the WebSocket upgrade routes;
`listen` does both steps for a production server:

```rust
use caelix::Application;

#[caelix::main]
async fn main() -> std::io::Result<()> {
    Application::new::<ChatModule>()
        .await
        .map_err(|error| std::io::Error::other(error.message))?
        .listen("127.0.0.1:3000")
        .await
}
```

The `/chat` route from the gateway example is now available as an RFC 6455
upgrade endpoint. The browser client connects to it with `ws://` or `wss://`;
it does not use Socket.IO framing.

### Mount the Axum router yourself

If another Axum server owns the listener, obtain the fully configured router:

```rust
let app = Application::new::<ChatModule>()
    .await?
    .into_router();

let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
axum::serve(listener, app).await?;
```

`into_router` includes Caelix controller routes, the WebSocket gateway routes,
the configured body limit, and the fallback response. Calling it is the
Axum-specific escape hatch for embedding Caelix in a larger Axum application.

### Add Axum and Tower middleware

The application exposes Axum's Tower-layer boundary. Middleware added before
`into_router` applies to the HTTP upgrade request and to the resulting Axum
router:

```rust
use tower_http::trace::TraceLayer;

let app = Application::new::<ChatModule>()
    .await?
    .layer(TraceLayer::new_for_http())
    .into_router();
```

Use Axum/Tower middleware for concerns such as tracing, compression of normal
HTTP responses, or request-level policy. WebSocket message authorization and
connection cleanup belong in the gateway callbacks because they occur after
the HTTP upgrade.

### Configure the Axum message limit

Axum applies a default maximum complete WebSocket message size of 1 MiB. Set
the limit before building the router:

```rust
let app = Application::new::<ChatModule>()
    .await?
    .websocket_max_message_size(8 * 1024 * 1024)
    .into_router();
```

The limit is for the assembled message, including fragmented messages. A
message that exceeds it is rejected by the WebSocket transport rather than
being delivered to `on_text` or `on_binary` as a partial payload.

## Socket.IO support (Axum only)

Socket.IO is a higher-level protocol built on top of Engine.IO. It is useful
when the client needs named events, acknowledgement callbacks, namespaces,
rooms, and Socket.IO's connection behavior. It is not a drop-in replacement
for a raw WebSocket endpoint.

The Caelix integration is backed by
[`socketioxide`](https://crates.io/crates/socketioxide) and selects Axum
transitively:

```toml
[dependencies]
caelix = { version = "0.0.26", default-features = false, features = ["socketio"] }
```

This feature cannot be combined with the default Actix backend. It exposes
`caelix::socket_io::{SocketIoHandle, SocketRef, Data, AckSender}` and the
`Application::with_socket_io::<AppModule>()` method.

### Define namespaces and events

For Socket.IO, put `#[gateway("/namespace")]` on an inherent implementation
and annotate each async event method with `#[on_message("event")]`. A handler
accepts either `payload: T` or `socket: SocketRef, payload: T` and returns
`Result<Reply>`.

The payload type must be deserializable by Socket.IO. It can be a scalar such
as `String`, or a structured type:

```rust
use caelix::{gateway, injectable, Module, ModuleMetadata, Result};
use caelix::socket_io::SocketRef;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ChatMessage {
    text: String,
}

#[injectable]
struct ChatGateway;

#[gateway("/chat")]
impl ChatGateway {
    #[on_message("echo")]
    async fn echo(&self, message: ChatMessage) -> Result<ChatMessage> {
        Ok(message)
    }

    #[on_message("join")]
    async fn join(&self, socket: SocketRef, room: String) -> Result<String> {
        socket.join(room);
        Ok("joined".to_owned())
    }

    #[on_message("announce")]
    async fn announce(&self, socket: SocketRef, message: ChatMessage) -> Result<String> {
        let _ = socket
            .within("general")
            .emit("chat-message", &message)
            .await;

        Ok("sent".to_owned())
    }
}

struct ChatModule;

impl Module for ChatModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().gateway::<ChatGateway>()
    }
}
```

The gateway path is a Socket.IO namespace, so this example listens on
`/chat`. Add another gateway with `#[gateway("/admin")]` to register another
namespace. Namespace paths must be distinct from one another, just as raw
WebSocket gateway paths must be distinct.

If the server does not need the socket itself, omit the `SocketRef` argument:

```rust
#[on_message("ping")]
async fn ping(&self, value: String) -> Result<String> {
    Ok(format!("pong: {value}"))
}
```

Handlers must be async and must take one payload argument. The macro registers
the Socket.IO `Data<T>` extractor for that argument and supplies an
`AckSender` internally.

### Start Socket.IO

Socket.IO is created during `Application::new`, but it is mounted only when
`with_socket_io` is called. Call it before `listen` or `into_router`:

```rust
use caelix::Application;

#[caelix::main]
async fn main() -> std::io::Result<()> {
    Application::new::<ChatModule>()
        .await
        .map_err(|error| std::io::Error::other(error.message))?
        .with_socket_io::<ChatModule>()
        .listen("127.0.0.1:3000")
        .await
}
```

`with_socket_io::<ChatModule>()` visits the module tree, registers every
Socket.IO gateway with its namespace, and attaches the Socket.IO Tower layer.
Calling it twice on the same application is an error. A Socket.IO gateway is
not mounted by the raw RFC 6455 route builder; it needs this explicit step.

### Acknowledgements and errors

When a client supplies an acknowledgement callback, a successful handler
return is sent to that callback. The return value is still useful without an
acknowledgement callback: the handler runs normally, but there is no reply to
deliver.

For a handler that returns an error, Caelix serializes this shape when an ack
was requested and emits the same value through the socket's generic `error`
event:

```json
{
  "error": "Bad Request",
  "message": "invalid chat message"
}
```

For example, a validation handler can return a normal Caelix exception:

```rust
#[on_message("validate")]
async fn validate(&self, message: ChatMessage) -> Result<String> {
    if message.text.trim().is_empty() {
        return Err(caelix::BadRequestException::new("message is empty"));
    }

    Ok("valid".to_owned())
}
```

The error is also emitted as an event named `error`, so clients should attach
an error listener when they need to observe failures independently of an ack.

### Rooms and broadcasts

`SocketRef` exposes Socket.IO's native room and namespace operations. Joining
a room is synchronous; emitting to a room is asynchronous:

```rust
#[on_message("subscribe")]
async fn subscribe(&self, socket: SocketRef, room: String) -> Result<String> {
    socket.join(room.clone());
    Ok(format!("subscribed to {room}"))
}

#[on_message("broadcast")]
async fn broadcast(&self, socket: SocketRef, message: String) -> Result<String> {
    let _ = socket
        .within("announcements")
        .emit("announcement", &message)
        .await;

    Ok("broadcast complete".to_owned())
}
```

Use the room name supplied by the client only after applying the application's
authorization rules. A room is not an authorization boundary by itself.

### Inject the Socket.IO server handle

`SocketIoHandle` is registered in the Caelix container before application
providers are built. Services can inject it with the normal `Arc<T>` provider
pattern:

```rust
use std::sync::Arc;
use caelix::{injectable, Module, ModuleMetadata};
use caelix::socket_io::SocketIoHandle;

#[injectable]
struct NotificationService {
    socket_io: Arc<SocketIoHandle>,
}

impl NotificationService {
    fn server(&self) -> &caelix::socket_io::SocketIo {
        self.socket_io.io()
    }
}

struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<NotificationService>()
            .gateway::<ChatGateway>()
    }
}
```

Use `SocketRef` inside a handler when the operation belongs to the current
connection. Inject `SocketIoHandle` when a long-lived service needs access to
the Socket.IO server while processing application events. Keep those services
independent of a particular client socket where possible.

### JavaScript client

Install a Socket.IO client, then connect to the namespace that matches the
gateway path:

```sh
npm install socket.io-client
```

```javascript
import { io } from "socket.io-client";

const socket = io("http://localhost:3000/chat", {
  transports: ["websocket"],
});

socket.on("connect", () => {
  socket.emit("echo", { text: "hello" }, (reply) => {
    console.log("echo acknowledgement:", reply);
  });

  socket.timeout(3000).emit("join", "general", (error, reply) => {
    if (error) console.error("join timed out", error);
    else console.log(reply);
  });
});

socket.on("chat-message", (message) => {
  console.log("message from the general room:", message);
});

socket.on("error", (error) => {
  console.error("server handler error:", error);
});
```

The `io` URL's `/chat` suffix is the Socket.IO namespace and must match the
Rust gateway path. It is not merely an HTTP route prefix. Socket.IO may use
polling and then upgrade by default; forcing `transports: ["websocket"]` is
optional and is useful when the deployment only permits WebSocket transport.

### Test the integration

The Socket.IO integration is compiled only with its feature. Run its Rust
compatibility test with:

```sh
cargo test -p caelix --no-default-features --features socketio --test socketio
```

The repository test uses the official `socket.io-client` package to verify
namespace connections, acknowledgements, room broadcasts, and the error
event. For an application test, start `Application::new::<AppModule>()`, call
`with_socket_io`, use `into_router()` with an ephemeral listener, and connect
with a real Socket.IO client so the transport and protocol are exercised
together.

## Choosing between the integrations

Use raw WebSockets when the protocol should be small and explicit, clients
already use the browser `WebSocket` API, or the application only needs text
and binary messages. Use Socket.IO when named events, acknowledgements, room
broadcasts, or Socket.IO client compatibility are central requirements.

Both integrations use injectable gateways and explicit module registration.
Only Socket.IO requires `with_socket_io`, and only Socket.IO gateways use
`#[on_message]`.
