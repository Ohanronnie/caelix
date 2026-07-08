# Events

`EventBus` is opt-in. Import `EventModule` in a module that emits events or registers event handlers. Event handlers are normal injectable providers and must be registered as providers before being registered as event handlers.

```rust
#[derive(Clone)]
pub struct UserCreated {
    pub id: i64,
}

use std::sync::Arc;

use caelix::{
    BoxFuture, EventBus, EventHandler, EventModule, Module, ModuleMetadata,
    RegisterableEventHandler, Result, injectable,
};

#[injectable]
pub struct SendWelcomeEmail;

impl EventHandler<UserCreated> for SendWelcomeEmail {
    fn handle(&self, event: UserCreated) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            println!("created user {}", event.id);
            Ok(())
        })
    }
}

impl RegisterableEventHandler for SendWelcomeEmail {
    type Event = UserCreated;
}

#[injectable]
pub struct UsersService {
    events: Arc<EventBus>,
}

impl UsersService {
    pub async fn create(&self, input: CreateUserDto) -> Result<UserDto> {
        let user = UserDto {
            id: 1,
            email: input.email,
        };

        self.events.emit(UserCreated { id: user.id }).await?;
        Ok(user)
    }
}

pub struct UsersModule;

impl Module for UsersModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<EventModule>()
            .provider::<UsersService>()
            .provider::<SendWelcomeEmail>()
            .event_handler::<SendWelcomeEmail>()
    }
}
```

Use `.event_handler_for::<Event, Handler>()` when explicit event registration is clearer.

```rust
ModuleMetadata::new()
    .import::<EventModule>()
    .provider::<SendWelcomeEmail>()
    .event_handler_for::<UserCreated, SendWelcomeEmail>()
```

Emit events by resolving `EventBus` from a provider and calling `emit`.

```rust
let events = container.resolve::<EventBus>();
events.emit(UserCreated { id: 42 }).await?;
```

`emit` runs handlers registered for the event type in registration order. If a handler returns an error, `emit` stops and returns that error; later handlers for the same event are not run, and **live subscribers do not receive the event**. Only after all handlers succeed does `emit` fan out to stream subscribers.

Event payloads must be `Clone + Send + Sync + 'static` because the same event value may be passed to multiple handlers and broadcast subscribers.

## Live subscriptions

`EventBus::subscribe::<E>()` returns an async `Stream` of events of type `E` and creates the broadcast channel for that type. Events emitted before the subscription are not replayed. Slow consumers may lag; lagged events are dropped (with a warning log) rather than blocking emit.

`emit` alone never allocates a broadcast channel — it only sends when someone has already called `subscribe` for that event type.

```rust
use caelix::{EventBus, HttpResponse, Response, Result};

#[get("/live-orders")]
async fn live_orders(&self) -> Result<HttpResponse> {
    let stream = self.events.subscribe::<OrderPlacedEvent>();
    Ok(Response::sse(stream))
}
```

Handlers registered with `.event_handler` / `.event_handler_for` still run on every `emit`. Live subscribers are an additional fan-out after successful handler processing.

## Registration Forms

`.event_handler::<H>()` requires:

```rust
impl EventHandler<UserCreated> for SendWelcomeEmail { /* ... */ }

impl RegisterableEventHandler for SendWelcomeEmail {
    type Event = UserCreated;
}
```

`.event_handler_for::<UserCreated, SendWelcomeEmail>()` avoids the `RegisterableEventHandler` implementation:

```rust
ModuleMetadata::new()
    .provider::<SendWelcomeEmail>()
    .event_handler_for::<UserCreated, SendWelcomeEmail>()
```

Both forms still require the handler to be registered as a provider.
