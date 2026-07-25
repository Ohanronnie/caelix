# Redis Microservices

Redis 6.2 or newer is required because recovery uses Streams consumer-group
commands including automatic claiming. Each command subject maps to a
deterministic Stream. The client subscribes to a unique temporary Pub/Sub reply
channel before appending the command. Events share a configured Stream.

Replicas with the same `service_name` share consumer groups. One replica handles
each command; each service group receives every event, with one replica handling
it inside that group.

```rust
let options = RedisTransportOptions::new("redis://127.0.0.1/")
    .service_name("billing")
    .event_stream("caelix:events")
    .dead_letter_stream("caelix:dead");
```

## Options

| Method | Default | Meaning |
| --- | --- | --- |
| `new(server)` | required | Redis URL |
| `service_name(name)` | none | required stable consumer-group identity |
| `event_stream(key)` | `caelix:events` | shared event Stream |
| `dead_letter_stream(key)` | none | distinct final-failure Stream |
| `rpc_timeout(duration)` | 5 s | complete request/reply deadline |
| `max_request_bytes(n)` | 1 MiB | command envelope limit; minimum 1 |
| `max_response_bytes(n)` | 1 MiB | response envelope limit; minimum 256 |
| `max_event_bytes(n)` | 1 MiB | event envelope limit; minimum 1 |
| `max_handler_concurrency(n)` | 64 | concurrent handler bound; minimum 1 |
| `shutdown_timeout(duration)` | 10 s | graceful task deadline |
| `max_event_deliveries(n)` | 5 | final delivery attempt; minimum 1 |
| `event_retry_delay(duration)` | 1 s | idle interval before event reclaim; minimum 10 ms |
| `command_recovery_delay(duration)` | 30 s | idle interval before abandoned command reclaim; minimum 10 ms |
| `approximate_max_event_entries(n)` | unlimited | opt-in `MAXLEN ~` event trimming |

An expired command is acknowledged without invoking user code. Pending commands
from a failed replica are reclaimed after `command_recovery_delay`; handlers
must therefore be idempotent even though a healthy request normally executes
once. Events are at least once and retry through pending-entry recovery.

Event retention is unlimited unless trimming is enabled. Approximate trimming
can delete entries still pending in a slower service group; set it only with a
retention policy covering the slowest consumer. A dead-letter Stream must be
non-empty and differ from the event Stream. Shutdown stops intake, drains within
the configured timeout, and runs lifecycle hooks.
