#![cfg(feature = "actix")]

use std::sync::Arc;

use caelix::{
    Container, Controller, Injectable, Module, ModuleMetadata, Response, StatusCode,
    TestApplication,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

struct NamedRepository {
    name: &'static str,
}

impl Injectable for NamedRepository {
    fn create(_container: &Container) -> caelix::BoxFuture<'_, caelix::Result<Self>> {
        Box::pin(async move { Ok(Self { name: "production" }) })
    }
}

struct UsersService {
    repo: Arc<NamedRepository>,
}

impl Injectable for UsersService {
    fn create(container: &Container) -> caelix::BoxFuture<'_, caelix::Result<Self>> {
        Box::pin(async move {
            Ok(Self {
                repo: container.resolve::<NamedRepository>()?,
            })
        })
    }
}

struct UsersController {
    service: Arc<UsersService>,
}

impl Injectable for UsersController {
    fn create(container: &Container) -> caelix::BoxFuture<'_, caelix::Result<Self>> {
        Box::pin(async move {
            Ok(Self {
                service: container.resolve::<UsersService>()?,
            })
        })
    }
}

#[derive(Deserialize, Serialize)]
struct CreateUserDto {
    name: String,
    email: String,
}

#[derive(Deserialize, Serialize)]
struct UserDto {
    name: String,
    email: String,
    backend: String,
}

impl UsersController {
    async fn create(
        container: actix_web::web::Data<Container>,
        body: actix_web::web::Json<CreateUserDto>,
    ) -> actix_web::HttpResponse {
        let controller = container.resolve::<UsersController>().unwrap();
        let body = body.into_inner();
        let dto = UserDto {
            name: body.name,
            email: body.email,
            backend: controller.service.repo.name.to_string(),
        };
        caelix::to_actix_response(caelix::IntoCaelixResponse::into_response(
            Response::WithStatus(StatusCode::CREATED, dto),
        ))
    }

    async fn backend(container: actix_web::web::Data<Container>) -> actix_web::HttpResponse {
        let controller = container.resolve::<UsersController>().unwrap();
        caelix::to_actix_response(caelix::IntoCaelixResponse::into_response(Response::Body(
            json!({ "backend": controller.service.repo.name }),
        )))
    }
}

impl Controller for UsersController {
    fn base_path() -> &'static str {
        "/users"
    }

    fn register_routes(cfg_any: &mut dyn std::any::Any) {
        let cfg = cfg_any
            .downcast_mut::<actix_web::web::ServiceConfig>()
            .expect("expected actix ServiceConfig");

        cfg.route("/users", actix_web::web::post().to(UsersController::create));
        cfg.route(
            "/users/backend",
            actix_web::web::get().to(UsersController::backend),
        );
    }
}

struct UsersModule;
impl Module for UsersModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<NamedRepository>()
            .provider::<UsersService>()
            .controller::<UsersController>()
    }
}

struct AppModule;
impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().import::<UsersModule>()
    }
}

#[caelix::test]
async fn should_create_user() {
    let app = TestApplication::new::<AppModule>().await.unwrap();

    let response = app
        .post("/users")
        .json(CreateUserDto {
            name: "Ronnie".into(),
            email: "ronnie@example.com".into(),
        })
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::CREATED);

    let body: UserDto = response.json().await;
    assert_eq!(body.name, "Ronnie");
    assert_eq!(body.email, "ronnie@example.com");
    assert_eq!(body.backend, "production");
}

#[caelix::test]
async fn should_override_nested_provider() {
    let app = TestApplication::new::<AppModule>()
        .override_provider(NamedRepository { name: "in-memory" })
        .await
        .unwrap();

    let body: Value = app
        .get("/users/backend")
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::OK)
        .json()
        .await;

    assert_eq!(body, json!({ "backend": "in-memory" }));
    assert_eq!(app.resolve::<NamedRepository>().unwrap().name, "in-memory");
}
