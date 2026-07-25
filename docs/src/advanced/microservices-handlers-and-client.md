# Microservice Handlers and Client

`#[microservice]` applies to an inherent impl of an injectable class.
`#[message_pattern("subject")]` declares request/reply; `#[event_pattern]`
declares an event. Subjects are non-empty dot-separated tokens. Commands cannot
use wildcards; events may use a whole-token `*` or terminal `>`.

Handlers must be non-generic `async fn` methods with `&self`, exactly one named
`#[payload]` parameter, and at most one `#[context] MessageContext` in either
argument order. Payloads implement `DeserializeOwned + Send`. Commands return
`Result<T>` where `T: Serialize + Send`; events return `Result<()>`.

`MessageContext` exposes `subject`, application `headers`, command
`correlation_id`, propagated `deadline`, shutdown `cancellation_token`, durable
`delivery` metadata, one-based `delivery_attempt`, and stable event `event_id`.
Use `event_id` as a deduplication key.

## Typed client

```rust
let client = MicroserviceClient::connect(
    NatsTransportOptions::new("nats://127.0.0.1:4222")
        .service_name("orders"),
).await?;

let order: Order = client.request("orders.create", input).await?;
client.emit("orders.created", event).await?;
```

`MicroserviceClient` is also registered in the microservice container, so
providers can inject `Arc<MicroserviceClient>`. `request<P, R>` serializes a
typed payload, applies `rpc_timeout` as an end-to-end deadline, and decodes a
typed response. `emit<P>` publishes a durable event and returns after broker
acceptance, not handler completion.

Current public request methods create the envelope headers internally; handlers
can read propagated headers from `MessageContext`, but the typed client does not
currently expose a per-request header builder.

Client failures are categorized as `Timeout`, `NoResponder`, `Decode`,
`Protocol`, `Transport`, or `Remote(RemoteError)`. A remote error contains a
stable code, safe message, optional safe JSON details, and a `retryable` flag.
Internal exception sources and unsafe server messages never cross the broker.
