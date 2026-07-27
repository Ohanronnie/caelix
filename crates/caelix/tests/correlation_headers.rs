#![cfg(any(feature = "actix", feature = "axum"))]

use caelix::{
    BoxFuture, Container, Injectable, Module, ModuleMetadata, Response, Result, StatusCode,
    TestApplication, controller,
};

struct CorrelationController;

impl Injectable for CorrelationController {
    fn dependencies() -> Vec<caelix::ProviderDependency> {
        caelix::provider_dependencies![]
    }

    fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async { Ok(Self) })
    }
}

#[controller("/correlation")]
impl CorrelationController {
    #[get("/")]
    async fn get(&self) -> Result<Response<&'static str>> {
        Ok(Response::Body("ok"))
    }
}

struct CorrelationModule;

impl Module for CorrelationModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().controller::<CorrelationController>()
    }
}

#[caelix::test]
async fn generated_routes_reject_duplicate_correlation_headers() {
    let app = TestApplication::new::<CorrelationModule>().await.unwrap();

    for (name, first, second) in [
        ("x-request-id", "request-one", "request-two"),
        (
            "traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-7bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        ),
        ("x-trace-id", "trace-one", "trace-two"),
    ] {
        let error: serde_json::Value = app
            .get("/correlation/")
            .append_header(name, first)
            .append_header(name, second)
            .send()
            .await
            .unwrap()
            .assert_status(StatusCode::BAD_REQUEST)
            .json()
            .await;

        assert_eq!(
            error["message"],
            format!("duplicate correlation header '{name}'")
        );
    }
}

#[caelix::test]
async fn generated_routes_reject_mixed_validity_duplicate_correlation_headers() {
    let app = TestApplication::new::<CorrelationModule>().await.unwrap();

    let error: serde_json::Value = app
        .get("/correlation/")
        .append_header("x-request-id", "request-one")
        .append_header("x-request-id", "not valid as a correlation identifier")
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::BAD_REQUEST)
        .json()
        .await;

    assert_eq!(
        error["message"],
        "duplicate correlation header 'x-request-id'"
    );
}

#[caelix::test]
async fn generated_routes_accept_single_and_absent_correlation_headers() {
    let app = TestApplication::new::<CorrelationModule>().await.unwrap();

    app.get("/correlation/")
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::OK);

    for (name, value) in [
        ("x-request-id", "request-one"),
        (
            "traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        ),
        ("x-trace-id", "trace-one"),
    ] {
        app.get("/correlation/")
            .header(name, value)
            .send()
            .await
            .unwrap()
            .assert_status(StatusCode::OK);
    }
}
