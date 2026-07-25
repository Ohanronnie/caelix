# Microservices

Caelix uses the same dependency-injected module graph for HTTP applications and
broker consumers. Handler metadata is transport-neutral; a running
`MicroserviceApplication` selects NATS or Redis.

## Select a transport

```toml
[dependencies]
caelix = { version = "0.0.26", default-features = false, features = ["microservices-nats"] }
serde = { version = "1", features = ["derive"] }
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

Continue with [Handlers and Client](microservices-handlers-and-client.md), then
the [NATS](microservices-nats.md) or [Redis](microservices-redis.md) transport
guide. Production concerns and tests are in
[Operations, Testing, and Interoperability](microservices-operations.md).
