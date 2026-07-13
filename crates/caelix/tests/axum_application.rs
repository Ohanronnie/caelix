#![cfg(feature = "axum")]

use caelix::{
    Application, BoxFuture, Container, Guard, Injectable, Interceptor, Module, ModuleMetadata,
    Next, RequestContext, Response, Result, WebSocketGateway, WebSocketSession, controller,
    gateway,
};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tower::ServiceExt;

struct AllowGuard;

impl Injectable for AllowGuard {
    fn dependencies() -> Vec<caelix::ProviderDependency> {
        caelix::provider_dependencies![]
    }

    fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Guard for AllowGuard {
    fn can_activate<'a>(&'a self, _: &'a RequestContext) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async { Ok(true) })
    }
}

struct PassThroughInterceptor;

impl Injectable for PassThroughInterceptor {
    fn dependencies() -> Vec<caelix::ProviderDependency> {
        caelix::provider_dependencies![]
    }

    fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Interceptor for PassThroughInterceptor {
    fn intercept<'a>(
        &'a self,
        _: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Result<caelix::HttpResponse>> {
        next.run()
    }
}

#[derive(Deserialize)]
struct Payload {
    name: String,
}

#[derive(Deserialize)]
struct Search {
    include: bool,
}

#[derive(Serialize)]
struct Output {
    id: String,
    name: String,
    include: bool,
}

struct HealthController;

impl Injectable for HealthController {
    fn dependencies() -> Vec<caelix::ProviderDependency> {
        caelix::provider_dependencies![]
    }

    fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async { Ok(Self) })
    }
}

#[controller("/health")]
#[use_guard(AllowGuard)]
#[use_interceptor(PassThroughInterceptor)]
impl HealthController {
    #[post("/{id}")]
    async fn create(
        &self,
        #[param] id: String,
        #[body] payload: Payload,
        #[query] search: Search,
    ) -> Result<Response<Output>> {
        Ok(Response::Body(Output {
            id,
            name: payload.name,
            include: search.include,
        }))
    }
}

struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<AllowGuard>()
            .provider::<PassThroughInterceptor>()
            .controller::<HealthController>()
    }
}

#[caelix::test]
async fn generated_controller_routes_and_extractors_work_on_axum() {
    let app = Application::new::<AppModule>().await.unwrap().into_router();
    let response = app
        .oneshot(
            caelix::__axum::http::Request::builder()
                .method("POST")
                .uri("/health/42?include=true")
                .header("content-type", "application/json")
                .body(caelix::__axum::body::Body::from(r#"{"name":"Ada"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), caelix::__axum::http::StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), br#"{"id":"42","name":"Ada","include":true}"#);
}

#[caelix::test]
async fn axum_runtime_macro_can_be_used_more_than_once() {}

#[derive(Default)]
struct EchoGateway;

impl Injectable for EchoGateway {
    fn dependencies() -> Vec<caelix::ProviderDependency> {
        caelix::provider_dependencies![]
    }

    fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async { Ok(Self) })
    }
}

#[gateway("/echo")]
impl WebSocketGateway for EchoGateway {
    fn on_text(&self, session: Arc<WebSocketSession>, text: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { session.send_text(text).await })
    }
}

struct WebSocketModule;

impl Module for WebSocketModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().gateway::<EchoGateway>()
    }
}

#[caelix::test]
async fn decorated_websocket_gateway_mounts_on_axum() {
    let app = Application::new::<WebSocketModule>()
        .await
        .unwrap()
        .into_router();
    let listener = caelix::__tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = caelix::__tokio::spawn(async move {
        caelix::__axum::serve(listener, app).await.unwrap();
    });

    assert_eq!(
        {
            let (mut socket, _) = connect_async(format!("ws://{address}/echo")).await.unwrap();
            socket.send(Message::Text("hello".into())).await.unwrap();
            socket.next().await.unwrap().unwrap()
        },
        Message::Text("hello".into())
    );
    server.abort();
}
