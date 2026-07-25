# Caelix

Caelix is a Rust backend framework built around modules, dependency injection,
controllers, guards, interceptors, lifecycle hooks, cookies, validated uploads,
OpenAPI, structured logging, domain events, WebSockets, typed NATS/Redis
microservices, and explicit service-level caching.

The public package is:

- `caelix`: the facade exporting traits, macros, optional HTTP runtimes, and
  microservice transports.

The fastest way to start an app is:

```sh
cargo install caelix-cli
caelix new demo-api
cd demo-api
cargo run
```

Generated applications depend on `caelix = "0.0.27"` from crates.io. The generator refuses to overwrite existing files, so it is safe to run against a feature name and stop when a generated file already exists.
