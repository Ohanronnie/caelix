use std::{sync::Arc, time::Duration};

use caelix::prelude::*;
use serde::Deserialize;

#[injectable]
pub struct Repo;

impl Repo {
    pub fn greet(&self) -> String {
        "hello from Repo".to_string()
    }
}

pub struct AsyncGreetingProvider {
    greeting: String,
}

impl AsyncGreetingProvider {
    pub fn greet(&self) -> &str {
        &self.greeting
    }
}

async fn connect_async_greeting_provider(
    container: Arc<Container>,
) -> Result<AsyncGreetingProvider> {
    let repo = container.resolve::<Repo>();

    actix_web::rt::time::sleep(Duration::from_millis(1)).await;

    Ok(AsyncGreetingProvider {
        greeting: format!("{} + hello from async factory", repo.greet()),
    })
}

pub struct ExpensiveStartupProvider {
    warmed: bool,
}

impl ExpensiveStartupProvider {
    pub fn warmed(&self) -> bool {
        self.warmed
    }
}

async fn warm_expensive_startup_provider(
    _container: Arc<Container>,
) -> Result<ExpensiveStartupProvider> {
    actix_web::rt::time::sleep(Duration::from_millis(120)).await;

    Ok(ExpensiveStartupProvider { warmed: true })
}

#[injectable]
pub struct Service {
    repo: Arc<Repo>,
    async_greeting: Arc<AsyncGreetingProvider>,
    expensive_startup: Arc<ExpensiveStartupProvider>,
    logger: Arc<Logger>,
}

impl Service {
    pub fn call_repo(&self) -> String {
        self.repo.greet()
    }

    pub fn call_async_provider(&self) -> String {
        self.async_greeting.greet().to_string()
    }

    pub fn expensive_provider_warmed(&self) -> bool {
        self.expensive_startup.warmed()
    }

    pub fn find_user(&self, id: i64) -> String {
        self.logger.info("creating user");
        self.logger.debug("payload validated");
        self.logger.error("failed to create user");
        format!("{}: user {id}", self.repo.greet())
    }

    pub fn search_users(&self, term: &str) -> String {
        format!("{}: search {term}", self.repo.greet())
    }

    pub fn create_user(&self, name: &str, email: &str) -> String {
        format!("{}: created {name} <{email}>", self.repo.greet())
    }
}

#[derive(Deserialize)]
pub struct SearchUsersQuery {
    term: String,
}

#[derive(Deserialize)]
pub struct CreateUserDto {
    name: String,
    email: String,
}

#[injectable]
pub struct UserController {
    service: Arc<Service>,
}

#[controller("/users")]
impl UserController {
    #[get("/async-provider")]
    pub async fn async_provider(&self) -> Result<String> {
        Ok(self.service.call_async_provider())
    }

    #[get("/{id}")]
    pub async fn get_user(&self, #[param] id: i64) -> Result<String> {
        Ok(self.service.find_user(id))
    }

    #[get("/")]
    pub async fn search_users(&self, #[query] query: SearchUsersQuery) -> Result<String> {
        Ok(self.service.search_users(&query.term))
    }

    #[post("/")]
    pub async fn create_user(&self, #[body] body: CreateUserDto) -> Result<String> {
        Ok(self.service.create_user(&body.name, &body.email))
    }
}

pub struct UserModule;

impl Module for UserModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<Repo>()
            .provider_async_factory::<AsyncGreetingProvider, _, _>(connect_async_greeting_provider)
            .provider_async_factory::<ExpensiveStartupProvider, _, _>(
                warm_expensive_startup_provider,
            )
            .provider::<Service>()
            .controller::<UserController>()
    }
}

pub struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().import::<UserModule>()
    }
}
