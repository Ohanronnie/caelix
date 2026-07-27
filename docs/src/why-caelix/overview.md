# Why Caelix

Caelix is for Rust services that benefit from a visible application structure.
It gives modules, providers, and controllers clear jobs, so an application can
grow without its routes, dependencies, and lifecycle work becoming implicit.

Caelix works with Actix or Axum at the HTTP boundary. It does not replace
either backend: it organizes application code above the selected runtime and
leaves backend-specific integrations available when they are useful.

## A module graph you can read

Caelix registration is explicit. A module declares the modules it imports and
the providers, controllers, gateways, and event handlers it owns:

```rust
use caelix::{Module, ModuleMetadata};

struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<DatabaseModule>()
            .provider::<UsersService>()
            .controller::<UsersController>()
    }
}
```

There is no filesystem scanning or hidden registration. The module graph is a
direct description of what starts with the application, which makes ownership
and startup behavior easier to inspect and test.

## Dependencies belong to providers

Providers receive their dependencies through typed dependency injection. A
controller can depend on a service, and that service can depend on a
repository, without constructing either one inside request-handling code.

```rust
use std::sync::Arc;
use caelix::injectable;

#[injectable]
struct UsersService {
    repository: Arc<UsersRepository>,
}
```

This keeps construction in the module/container boundary and leaves services
focused on their work. The same boundary also makes integration tests
straightforward: [`TestApplication`](../recipes/testing.md) can build the
production module graph while replacing a registered provider for a test.

## Controllers keep HTTP at the edge

Controllers declare routes and use framework-neutral Caelix extractors,
responses, guards, and interceptors. Providers hold the application and domain
work. That separation lets the same controller and provider source run on the
selected Actix or Axum adapter.

Caelix provides the application pieces services commonly need: validated
extractors and uploads, consistent responses and errors, lifecycle hooks,
request context, structured logging, OpenAPI metadata, events, WebSockets,
typed NATS and Redis microservices, configuration, throttling, and explicit
service-level caching. Each capability is registered or injected through the
same module and provider model.

## Lifecycle is part of the application model

Caelix builds imports before a module's own providers and controllers, runs
bootstrap hooks after successful registration, and shuts down in reverse
order. Providers and controllers can implement initialization, bootstrap, and
shutdown hooks, so resource management is an explicit part of the application
instead of incidental server setup.

For the exact ordering and failure behavior, see [Registration and Lifecycle
Order](../reference/registration-and-lifecycle.md).

## Choose the runtime at the boundary

Actix is Caelix's default integration. Choose Axum when native router or Tower
composition, or Axum-only Socket.IO support, matters to the application. In
either case, modules, providers, controllers, guards, test APIs, and Caelix
response/error behavior remain the same for framework-level code.

Read [Choosing Actix or Axum](choosing-actix-or-axum.md) for the trade-offs and
feature selection.
