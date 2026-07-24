# NestJS / Caelix NATS interoperability

This is a real Nest microservice using a custom Nest transport strategy. Nest's
built-in NATS transport uses Nest packet serialization, while Caelix uses a
versioned JSON envelope, so the built-in `Transport.NATS` configuration cannot
directly interoperate with `MicroserviceClient`.

Run the verification from the repository root:

```sh
bun install --cwd examples/nestjs-nats-interoperability
docker compose -f examples/nestjs-nats-interoperability/docker-compose.yml up -d
bun --cwd examples/nestjs-nats-interoperability start &
cargo run -p caelix-microservices --example nestjs_request_client
```

The Rust client sends `{ value: "hello" }` to `interop.echo`. Nest returns
`{ value: "nest:hello" }` through the Caelix response envelope.
