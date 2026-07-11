# WebSockets

Caelix supports RFC 6455 WebSockets through the Actix runtime. Register an injectable gateway with its module:

```rust
use std::sync::Arc;
use caelix::{injectable, BoxFuture, Bytes, Module, ModuleMetadata, Result,
    WebSocketGateway, WebSocketSession};

#[injectable]
struct ChatGateway;

impl WebSocketGateway for ChatGateway {
    fn path() -> &'static str { "/chat" }

    fn on_text(&self, session: Arc<WebSocketSession>, text: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { session.send_text(format!("received: {text}")).await })
    }

    fn on_binary(&self, session: Arc<WebSocketSession>, data: Bytes) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { session.send_binary(data).await })
    }
}

struct ChatModule;
impl Module for ChatModule {
    fn register() -> ModuleMetadata { ModuleMetadata::new().gateway::<ChatGateway>() }
}
```

Gateways support `on_connect`, `on_text`, `on_binary`, `on_error`, and `on_close`. They are normal injectable providers: constructor injection and lifecycle hooks work, imported-module gateways are discovered automatically, and shutdown is performed in reverse registration order.

Text and binary are kept distinct. Complete fragmented messages are reassembled before a callback runs, ping frames receive automatic pong replies, and `on_close` runs once for local closes, remote closes, and transport loss. A callback failure invokes `on_error` and closes the connection with code `1011`.

The complete-message limit defaults to 1 MiB and can be changed before listening:

```rust
let app = caelix::Application::new::<ChatModule>().await?
    .websocket_max_message_size(8 * 1024 * 1024);
```

Browser clients use the standard API:

```javascript
const socket = new WebSocket("ws://localhost:3000/chat");
socket.binaryType = "arraybuffer";
socket.addEventListener("open", () => {
  socket.send("hello");
  socket.send(new Uint8Array([0, 1, 2, 255]));
});
socket.addEventListener("message", ({ data }) => {
  if (typeof data === "string") console.log("text", data);
  else console.log("binary", new Uint8Array(data));
});
```
