# NATS microservices

Enable the NATS transport without changing the HTTP runtime selection:

```toml
caelix = { version = "…", features = ["microservices-nats"] }
```

`#[microservice]` classes are normal Caelix providers. Register one with
`.microservice::<T>()`; do not also register it with `.provider::<T>()`.

```rust
#[caelix::injectable]
struct UsersMicroservice {
    users: Arc<UserService>,
}

#[caelix::microservice]
impl UsersMicroservice {
    #[caelix::message_pattern("users.create")]
    async fn create(&self, #[caelix::payload] input: CreateUser) -> Result<User> {
        self.users.create(input).await
    }

    #[caelix::event_pattern("users.deleted")]
    async fn cleanup(
        &self,
        #[caelix::context] context: MessageContext,
        #[caelix::payload] event: UserDeleted,
    ) -> Result<()> {
        self.users.cleanup(event.id).await?;
        tracing::info!(attempt = context.delivery_attempt(), "cleanup completed");
        Ok(())
    }
}
```

Command handlers use Core NATS request/reply and must return `Result<T>` where
`T` is serializable. Event handlers return `Result<()>`. Payloads must be owned,
deserializable, and `Send`. The generated code checks these constraints at the
handler declaration.

## Starting a service

Use a stable service name. It becomes the command queue group, allowing several
instances of the same service to share command work rather than receiving every
request.

```rust
let options = NatsTransportOptions::new("nats://127.0.0.1:4222")
    .service_name("users-service")
    .rpc_timeout(Duration::from_secs(5))
    .jetstream_stream("CAELIX_EVENTS")
    .dead_letter_subject("caelix.dead-letter");

MicroserviceApplication::<AppModule>::new(options)
    .await?
    .run()
    .await?;
```

Inject `Arc<MicroserviceClient>` into any provider to make typed JSON calls:

```rust
let user: User = client.request("users.create", input).await?;
client.emit("users.deleted", UserDeleted { id: user.id }).await?;
```

Caelix owns a versioned JSON envelope containing protocol version, payload,
correlation ID, deadline, headers, and event publication metadata. Client
failures distinguish timeout, no responder, decoding, protocol, transport, and
sanitized remote errors. Internal server messages are never returned to callers.

Event delivery is at least once, so event handlers must be idempotent. A
`MessageContext` carries the actual subject, propagated headers, optional
correlation/deadline, cancellation token, delivery metadata, and the stable
event envelope ID (`context.event_id()`) used to deduplicate redeliveries.
