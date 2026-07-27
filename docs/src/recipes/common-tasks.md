# Common Tasks

## Create A Project

```sh
cargo install caelix-cli
caelix new demo-api
```

## Generate A Feature

```sh
caelix g module greetings
```

Add the generated module to your app:

```rust
pub mod greetings;

use greetings::GreetingsModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().import::<GreetingsModule>()
    }
}
```

## Generate Pieces Separately

```sh
caelix g service users
caelix g controller users
```

If the service exists before the controller is generated, the controller injects it.

## Convert Library Errors

```rust
let user = repository
    .find(id)
    .await
    .map_err(InternalServerErrorException::new)?;
```

For 5xx errors, the client response message stays generic.

## Validate A Request Body

Derive `Validate` and mark the extractor:

```rust
#[post("")]
async fn create(&self, #[body] #[validate] input: CreateUser) -> Result<Response<UserDto>> {
    self.users.create(input).await
}
```

Enable the `validator` feature and define the field rules on the DTO. The full
guide covers body, query, and path validation plus client error responses:
[Validation](../concepts/validation.md).

## Emit Events After Writes

Import `EventModule`, inject or resolve `EventBus` in a service, perform the write, then emit a cloned event type.

```rust
ModuleMetadata::new()
    .import::<EventModule>()
    .provider::<UsersService>()
    .provider::<SendWelcomeEmail>()
    .event_handler::<SendWelcomeEmail>();

self.events.emit(UserCreated { id: user.id }).await?;
```

## Cache Service Results Explicitly

Import `CacheModule`, inject `Arc<Cache>`, and call `get`, `set`, `set_with_ttl`, `delete`, or `clear` inside service methods.

## Set and Clear a Session Cookie

Create a response with `Response::Body(value).with_cookie(Cookie::new("session",
signed))`. Clear it with `Cookie::removal("session")`, matching the original
path and domain. See [Cookies and Sessions](../concepts/cookies-and-sessions.md).

## Call a Microservice

Inject `Arc<MicroserviceClient>` and call `request::<Payload, Reply>` or `emit`.
See [Handlers and Client](../advanced/microservices-handlers-and-client.md).
