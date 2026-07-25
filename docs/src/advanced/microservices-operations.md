# Microservice Operations, Testing, and Interoperability

## Reliability

Broker delivery is at least once after consumer failure. Make command writes and
event effects idempotent. Store `MessageContext::event_id()` with the business
transaction and reject an already-committed ID. For commands, use a business
idempotency key in the payload because correlation IDs identify attempts, not
business operations.

Concurrency is bounded per subscription by `max_handler_concurrency`; also size
database pools and downstream limits. Respect the propagated deadline and
`cancellation_token()` in long-running work. Shutdown stops intake, waits for
tasks, then runs normal provider/module hooks.

Remote failures are sanitized. Log internal exception sources on the service;
return stable public codes and safe details to callers. Monitor broker
connectivity, request latency/timeouts, no-responder errors, pending/redelivered
events, delivery attempts, dead-letter destinations, handler saturation, and
shutdown timeouts.

## Tests

Handler metadata and invocation can be tested without a broker:

```rust
let container = caelix::build_container::<AppModule>().await?;
let handler = OrdersMessages::definition()
    .handlers()
    .into_iter()
    .find(|handler| handler.pattern == "orders.create")
    .unwrap();
let result = handler.invoke(&container, context, serde_json::json!({"id": 1})).await?;
```

Use broker-backed integration tests for envelopes, queue/consumer competition,
deadlines, reconnection, retry, dead letters, and shutdown. Run isolated NATS
with JetStream or Redis 6.2+, use unique stream/subject prefixes, start
`MicroserviceRuntime`, create a real `MicroserviceClient`, and always call
`shutdown()`. The repository integration tests and event probes cover the
supported transports.

## NestJS interoperability

Maintained executable examples live in
[`examples/nestjs-nats-interoperability`](https://github.com/Ohanronnie/caelix/tree/main/examples/nestjs-nats-interoperability)
and
[`examples/nestjs-redis-interoperability`](https://github.com/Ohanronnie/caelix/tree/main/examples/nestjs-redis-interoperability).
They are the authoritative cross-framework demonstrations.

Caelix uses its own versioned JSON envelope for correlation IDs, deadlines,
headers, typed payloads, safe failures, and event IDs. NestJS's stock transport
serializer is therefore not wire-compatible by itself; the examples install
the matching custom envelope adapter. Keep that adapter aligned whenever the
protocol version changes.
