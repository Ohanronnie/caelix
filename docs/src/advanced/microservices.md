# Microservices: NATS and Redis

Caelix's microservice macros and handler signatures are transport-neutral. Enable either or both broker features; each running `MicroserviceApplication` selects exactly one transport.

```toml
caelix = { version = "…", features = ["microservices-nats", "microservices-redis"] }
```

NATS uses Core NATS for competing command consumers and JetStream for durable events:

```rust
let options = NatsTransportOptions::new("nats://127.0.0.1:4222")
    .service_name("users-service")
    .jetstream_stream("CAELIX_EVENTS");
MicroserviceApplication::<AppModule>::new(options).await?.run().await?;
```

Redis 6.2 or newer uses a deterministic Stream per command subject and a shared event Stream. Command responses use a unique temporary Pub/Sub channel; the client subscribes before appending the command.

```rust
let options = RedisTransportOptions::new("redis://127.0.0.1/")
    .service_name("users-service")
    .event_stream("caelix:events");
MicroserviceApplication::<AppModule>::new(options).await?.run().await?;
```

Commands are executed by one replica in a service consumer group. Events are delivered once to every service group and to one replica within that group. Failed broker deliveries are at least once, so command and event handlers must be idempotent.

Redis event retention is unlimited by default. `approximate_max_event_entries` explicitly opts into `MAXLEN ~`; Redis can trim entries that remain pending for another consumer group, so only enable it with an operational retention policy that accounts for the slowest service. An optional, distinct dead-letter Stream can capture events that exhaust their configured deliveries.

Request timeouts are end-to-end deadlines. A Redis command whose deadline elapsed before invocation is acknowledged without running user code. Remote failures contain only sanitized client-safe details; internal server messages are not sent across either transport.
