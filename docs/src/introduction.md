# Caelix

Caelix is a Rust backend framework for services that need a visible
architecture. Modules register providers, controllers, event handlers, and
gateways explicitly; services receive dependencies through the container; HTTP
is kept at the controller boundary.

The public `caelix` package provides the framework facade, macros, and one
selected HTTP adapter. It supports controllers, dependency injection, lifecycle
hooks, guards, interceptors, typed request extraction and validation, cookies,
uploads, OpenAPI, logging, events, WebSockets, service-level caching, and typed
NATS/Redis microservices.

## Start here

Create and run a generated application:

```sh
cargo install caelix-cli
caelix new demo-api
cd demo-api
cargo run
```

The generator uses the latest Caelix release from crates.io and refuses to
overwrite existing files. Follow [Overview](getting-started/overview.md) for a
complete application walkthrough, including a blog application with modules,
providers, database integration, controller routes, validation, and events.

## Learn the architecture

Read [Why Caelix](why-caelix/overview.md) for the design model, then choose the
HTTP boundary in [Choosing Actix or Axum](why-caelix/choosing-actix-or-axum.md).
Most applications keep the same modules, providers, controllers, guards, tests,
and response behavior whichever adapter they select.

## Build a service

Use this path when learning or building a new API:

1. Define the root and feature [Modules](concepts/modules.md).
2. Put application behavior in injectable [Providers](concepts/providers.md).
3. Expose HTTP contracts through [Controllers](concepts/controllers.md).
4. Decode and validate input with [Extractors](concepts/extractors.md) and
   [Validation](concepts/validation.md).
5. Test the real module graph with [Testing](recipes/testing.md).
6. Configure [Production Operations](advanced/production.md) before deployment.

The [Feature Flags](reference/feature-flags.md) reference lists every optional
capability and its compatible runtime combinations.
