# Common Tasks

## Create A Project

```sh
cargo run -p caelix-cli -- new demo-api --caelix-path .
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

Enable the `validator` feature on `caelix-core`, derive `Validate`, and mark the extractor:

```rust
#[post("")]
async fn create(&self, #[body] #[validate] input: CreateUser) -> Result<Response<UserDto>> {
    self.users.create(input).await
}
```

## Emit Events After Writes

Inject or resolve `EventBus` in a service, perform the write, then emit a cloned event type.

```rust
self.events.emit(UserCreated { id: user.id }).await?;
```

## Cache Service Results Explicitly

Import `CacheModule`, inject `Arc<Cache>`, and call `get`, `set`, `set_with_ttl`, `delete`, or `clear` inside service methods.
