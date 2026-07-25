# NATS Microservices

Commands subscribe through Core NATS queue groups. All replicas with the same
`service_name` compete for each command. Events publish to JetStream and use a
durable consumer per service, so every service group receives an event and one
replica in each group handles it.

```rust
let options = NatsTransportOptions::new("nats://127.0.0.1:4222")
    .service_name("billing")
    .jetstream_stream("CAELIX_EVENTS")
    .dead_letter_subject("caelix.dead");
```

`service_name` is required when command handlers exist. `jetstream_stream` is
required for event topology and names an existing stream whose subjects cover
the event patterns.

## Options

| Method | Default | Meaning |
| --- | --- | --- |
| `new(server)` | required | NATS server URL |
| `service_name(name)` | none | stable command queue group and event consumer identity |
| `rpc_timeout(duration)` | 5 s | command request deadline |
| `jetstream_stream(name)` | none | durable event stream |
| `dead_letter_subject(subject)` | none | final-failure event destination |
| `max_request_bytes(n)` | 1 MiB | encoded command envelope limit; minimum 1 |
| `max_response_bytes(n)` | 1 MiB | encoded response limit; minimum 256 |
| `max_event_bytes(n)` | 1 MiB | encoded event envelope limit; minimum 1 |
| `max_handler_concurrency(n)` | 64 | simultaneous invocations per subscription; minimum 1 |
| `shutdown_timeout(duration)` | 10 s | wait for transport tasks |
| `max_event_deliveries(n)` | 5 | attempt that dead-letters; minimum 1 |
| `event_retry_delay(duration)` | 1 s | delayed NAK interval |
| `event_ack_wait(duration)` | 30 s | unacknowledged redelivery wait; minimum 10 ms |

Commands are ordinary NATS request/reply messages and require an active
responder. Events are acknowledged only after a successful handler. Retryable
failures are negatively acknowledged with the configured delay. At the delivery
limit, a configured dead-letter subject receives the event and safe failure
metadata; otherwise the transport terminates the retry cycle according to its
consumer handling.

Keep the JetStream stream durable and size/age-limited according to broker
operations. During shutdown, Caelix cancels intake, waits up to
`shutdown_timeout` for tasks, then runs module shutdown hooks.
