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

#[injectable]
pub struct Service {
    repo: Arc<Repo>,
    async_greeting: Arc<AsyncGreetingProvider>,
}

impl Service {
    pub fn call_repo(&self) -> String {
        self.repo.greet()
    }

    pub fn call_async_provider(&self) -> String {
        self.async_greeting.greet().to_string()
    }

    pub fn find_user(&self, id: i64) -> String {
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
