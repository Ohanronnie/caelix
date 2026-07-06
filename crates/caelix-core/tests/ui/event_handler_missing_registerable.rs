use caelix_core::*;

#[derive(Clone)]
struct UserCreated;

struct SendWelcomeEmail;

impl Injectable for SendWelcomeEmail {
    fn create(_container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async { Self })
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
