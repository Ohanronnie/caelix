use std::sync::Arc;

use actix_web::{App, http::StatusCode, test as actix_test, web};
use caelix::prelude::*;
use caelix_core::register_module_controllers;
use serde_json::Value;
use sqlx_crud::{
    Database, UserController, UserRepository, UserService,
    connect_database as connect_real_database,
};

async fn connect_test_database(
    _container: Arc<Container>,
) -> std::result::Result<Database, sqlx::Error> {
    Database::connect("sqlite::memory:").await
}

struct TestUserModule;

impl Module for TestUserModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider_async_factory::<Database, _, _>(connect_test_database)
            .provider::<UserRepository>()
            .provider::<UserService>()
            .controller::<UserController>()
    }
}

struct TestAppModule;

impl Module for TestAppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().import::<TestUserModule>()
    }
}

#[actix_web::test]
async fn database_provider_can_connect_and_initialize_schema() {
    let database = connect_test_database(Arc::new(Container::new()))
        .await
        .expect("test database should connect");

    sqlx::query("INSERT INTO users (name, email) VALUES (?, ?)")
        .bind("Ronnie")
        .bind("ronnie@example.com")
        .execute(database.pool())
        .await
        .expect("insert should use initialized users table");

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(database.pool())
        .await
        .expect("count should query initialized users table");

    assert_eq!(count.0, 1);
}

#[actix_web::test]
async fn full_user_crud_routes_work_against_sqlite() {
    let container = Arc::new(build_container::<TestAppModule>().await);
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(container))
            .configure(|cfg| register_module_controllers::<TestAppModule>(cfg)),
    )
    .await;

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get().uri("/users").to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let users: Value = actix_test::read_body_json(response).await;
    assert_eq!(users, Value::Array(vec![]));

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::post()
            .uri("/users")
            .insert_header(("content-type", "application/json"))
            .set_payload(r#"{"name":"Ada Lovelace","email":"ada@example.com"}"#)
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: Value = actix_test::read_body_json(response).await;
    assert_eq!(created["id"], 1);
    assert_eq!(created["name"], "Ada Lovelace");
    assert_eq!(created["email"], "ada@example.com");

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get().uri("/users/1").to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let fetched: Value = actix_test::read_body_json(response).await;
    assert_eq!(fetched, created);

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::patch()
            .uri("/users/1")
            .insert_header(("content-type", "application/json"))
            .set_payload(r#"{"email":"ada@caelix.dev"}"#)
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let updated: Value = actix_test::read_body_json(response).await;
    assert_eq!(updated["name"], "Ada Lovelace");
    assert_eq!(updated["email"], "ada@caelix.dev");

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::delete()
            .uri("/users/1")
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get().uri("/users/1").to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[actix_web::test]
async fn real_database_factory_symbol_is_public_for_the_app_module() {
    let _factory = connect_real_database;
}
