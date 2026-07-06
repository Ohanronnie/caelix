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
