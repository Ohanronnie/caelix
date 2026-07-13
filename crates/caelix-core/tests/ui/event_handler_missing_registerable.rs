use caelix_core::*;

#[derive(Clone)]
struct UserCreated;

struct SendWelcomeEmail;

impl Injectable for SendWelcomeEmail {
    fn dependencies() -> Vec<ProviderDependency> {
        provider_dependencies![]
    }

    fn create(_container: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async { Ok(Self) })
    }
}

impl EventHandler<UserCreated> for SendWelcomeEmail {
    fn handle(&self, _event: UserCreated) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<SendWelcomeEmail>()
            .event_handler::<SendWelcomeEmail>()
    }
}

fn main() {}
