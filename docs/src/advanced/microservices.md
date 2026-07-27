# Microservices

Caelix uses the same dependency-injected module graph for HTTP applications and
broker consumers. Handler metadata is transport-neutral; a running
`MicroserviceApplication` selects NATS or Redis.

## Select a transport

```sh
cargo add caelix --no-default-features --features microservices-nats
cargo add serde --features derive
```

Use `microservices-redis` for Redis, or enable both when one binary needs both
client option types. These features do not select an HTTP runtime.

NATS commands use Core NATS queue groups and events use JetStream. Redis commands
and events use Streams; temporary Pub/Sub channels carry command replies. Both
provide competing command consumers, durable at-least-once events, typed JSON
envelopes, end-to-end deadlines, bounded concurrency, and graceful shutdown.

## Complete service

```rust
use caelix::{
    MessageContext, MicroserviceApplication, Module, ModuleMetadata,
    NatsTransportOptions, Result,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Sum { left: i64, right: i64 }

#[derive(Serialize)]
struct Total { value: i64 }

#[derive(Deserialize)]
struct AuditEvent { order_id: i64 }

#[caelix::injectable]
struct MathMessages;

#[caelix::microservice]
impl MathMessages {
    #[caelix::message_pattern("math.sum")]
    async fn sum(&self, #[caelix::payload] input: Sum) -> Result<Total> {
        Ok(Total { value: input.left + input.right })
    }

    #[caelix::event_pattern("audit.created")]
    async fn audit(
        &self,
        #[caelix::context] context: MessageContext,
        #[caelix::payload] event: AuditEvent,
    ) -> Result<()> {
        // Persist `(context.event_id(), event.order_id)` atomically in real code.
        let _ = (context.event_id(), event.order_id);
        Ok(())
    }
}

struct AppModule;
impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().microservice::<MathMessages>()
    }
}

#[caelix::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let options = NatsTransportOptions::new("nats://127.0.0.1:4222")
        .service_name("math")
        .jetstream_stream("CAELIX_EVENTS");
    MicroserviceApplication::<AppModule>::new(options).await?.run().await?;
    Ok(())
}
```

`.microservice::<T>()` registers `T` as a normal provider, so constructor
injection and lifecycle hooks work unchanged. Startup builds the container,
collects handler definitions, validates topology, connects, and subscribes.

## Send a command and event

Use the same transport options in a caller. `request` sends a typed command and
waits for a typed reply; `emit` publishes a durable event and returns after the
broker accepts it.

```rust
use caelix::{MicroserviceClient, NatsTransportOptions, Result};

async fn call_math() -> Result<Total> {
    let client = MicroserviceClient::connect(
        NatsTransportOptions::new("nats://127.0.0.1:4222")
            .service_name("gateway")
            .jetstream_stream("CAELIX_EVENTS"),
    )
    .await?;

    let total: Total = client.request("math.sum", Sum { left: 2, right: 3 }).await?;
    client.emit("audit.created", AuditEvent { order_id: 42 }).await?;
    Ok(total)
}
```

Use the command payload’s own idempotency key for writes that may be retried.
For event handlers, persist `MessageContext::event_id()` with the business
effect so an at-least-once delivery cannot apply it twice.

## Start with Redis instead

The handler and module source stays the same. Select Redis, configure a stable
consumer-group name, and name the shared event Stream:

```rust
use caelix::{MicroserviceApplication, RedisTransportOptions};

let options = RedisTransportOptions::new("redis://127.0.0.1/")
    .service_name("math")
    .event_stream("caelix:events");

MicroserviceApplication::<AppModule>::new(options).await?.run().await?;
```

Redis requires version 6.2 or later. Choose the transport guide for its
topology, retry, and operational constraints: [NATS](microservices-nats.md) or
[Redis](microservices-redis.md).

Continue with [Handlers and Client](microservices-handlers-and-client.md), then
the [NATS](microservices-nats.md) or [Redis](microservices-redis.md) transport
guide. Production concerns and tests are in
[Operations, Testing, and Interoperability](microservices-operations.md).
