# Redis microservices probe

Start Redis and the Caelix probe service:

```sh
docker compose up -d
cargo run -p caelix --example redis_event_probe --no-default-features --features microservices-redis
```

In another terminal, append a typed event through the same unified client API:

```sh
cargo run -p caelix --example redis_event_probe --no-default-features --features microservices-redis -- emit
```

The service prints the event after consuming it from `caelix:events`. You can also inspect the portable envelope with `redis-cli XRANGE caelix:events - +`.
