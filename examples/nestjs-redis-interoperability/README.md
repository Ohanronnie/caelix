# NestJS / Caelix Redis interoperability

This example runs a real NestJS microservice with a custom Redis Streams transport implementing Caelix's public request envelope.

From the repository root:

```sh
bun install --cwd examples/nestjs-redis-interoperability
docker compose -f examples/nestjs-redis-interoperability/docker-compose.yml up -d
bun --cwd examples/nestjs-redis-interoperability start
```

In another terminal:

```sh
cargo run -p caelix-microservices --no-default-features --features redis --example nestjs_redis_request_client
```

The Rust client sends `{ value: "hello" }` through Redis Streams. NestJS returns `{ value: "nest:hello" }` on the request's temporary Redis Pub/Sub reply channel.
