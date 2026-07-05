use std::sync::Arc;

use actix_web::{App, http::StatusCode, test as actix_test, web};
use caelix::prelude::build_container;
use caelix_core::register_module_controllers;
use hello_world::{AppModule, Service};

#[actix_web::test]
async fn resolves_service_through_hello_world_app_module() {
    let container = build_container::<AppModule>().await;

    let service = container.resolve::<Service>();

    assert_eq!(service.call_repo(), "hello from Repo");
    assert_eq!(
        service.call_async_provider(),
        "hello from Repo + hello from async factory"
    );
    assert!(service.expensive_provider_warmed());
}

#[actix_web::test]
async fn mounts_imported_controller_routes_with_extractors() {
    let container = Arc::new(build_container::<AppModule>().await);
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(container))
            .configure(|cfg| register_module_controllers::<AppModule>(cfg)),
    )
    .await;

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get()
            .uri("/users/async-provider")
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        actix_test::read_body(response).await,
        "hello from Repo + hello from async factory"
    );

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get()
            .uri("/users/intercepted")
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        actix_test::read_body(response).await,
        "outer(inner(handler))"
    );

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get().uri("/users/42").to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        actix_test::read_body(response).await,
        "hello from Repo: user 42"
    );

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get()
            .uri("/users/?term=ronnie")
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        actix_test::read_body(response).await,
        "hello from Repo: search ronnie"
    );

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::post()
            .uri("/users/")
            .insert_header(("content-type", "application/json"))
            .set_payload(r#"{"name":"Ronnie","email":"r@x.com"}"#)
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        actix_test::read_body(response).await,
        "hello from Repo: created Ronnie <r@x.com>"
    );
}

#[actix_web::test]
async fn guarded_route_threads_context_into_user_extractor() {
    let container = Arc::new(build_container::<AppModule>().await);
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(container))
            .configure(|cfg| register_module_controllers::<AppModule>(cfg)),
    )
    .await;

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get()
            .uri("/profile/me")
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get()
            .uri("/profile/me")
            .insert_header(("authorization", "Bearer wrong"))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get()
            .uri("/profile/me")
            .insert_header(("authorization", "Bearer secret-token"))
            .to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        actix_test::read_body(response).await,
        "hello from Repo: user 7"
    );
}
