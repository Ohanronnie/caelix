use std::{
    future::{Future, IntoFuture},
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
};

use axum::{
    Router,
    body::Body,
    http::{HeaderValue, Method, Request, header::HeaderName},
    response::Response,
};
use bytes::Bytes;
#[cfg(feature = "uploads")]
use caelix_core::UploadConfig;
use caelix_core::{
    BoxFuture, Container, IntoCaelixResponse, Module, ProviderDependency, ProviderOverrides,
    Result, StatusCode, build_container_with_overrides, log_application_started, log_module_routes,
    register_module_controllers, shutdown_module,
};
use serde::{Serialize, de::DeserializeOwned};
use tower::ServiceExt;

use crate::{
    AxumRouterBuilder,
    application::{DEFAULT_BODY_LIMIT_BYTES, UploadRuntimeConfig},
    to_axum_response,
};
#[cfg(feature = "openapi")]
use caelix_core::openapi::{OpenApiConfig, build_openapi};

/// In-process Caelix application for integration tests.
///
/// Builds the same DI container and route table as production, then serves
/// requests through Axum's in-memory router (no TCP listener).
pub struct TestApplication {
    container: Arc<Container>,
    router: Router,
    shutdown_fn: for<'a> fn(&'a Container) -> BoxFuture<'a, caelix_core::Result<()>>,
}

/// Builder for [`TestApplication`]. Await it (or call [`compile`](Self::compile))
/// to finish startup.
pub struct TestApplicationBuilder<M> {
    overrides: ProviderOverrides,
    body_limit: usize,
    #[cfg(feature = "uploads")]
    upload_config: UploadConfig,
    #[cfg(feature = "openapi")]
    openapi: Option<OpenApiConfig>,
    _module: PhantomData<M>,
}

impl TestApplication {
    /// Start configuring a test application for module `M`.
    pub fn new<M: Module + 'static>() -> TestApplicationBuilder<M> {
        TestApplicationBuilder {
            overrides: ProviderOverrides::new(),
            body_limit: DEFAULT_BODY_LIMIT_BYTES,
            #[cfg(feature = "uploads")]
            upload_config: UploadConfig::default(),
            #[cfg(feature = "openapi")]
            openapi: None,
            _module: PhantomData,
        }
    }

    /// Runs the `get` public API operation.
    pub fn get(&self, path: &str) -> TestRequestBuilder<'_> {
        TestRequestBuilder::new(self, Method::GET, path)
    }

    /// Runs the `post` public API operation.
    pub fn post(&self, path: &str) -> TestRequestBuilder<'_> {
        TestRequestBuilder::new(self, Method::POST, path)
    }

    /// Runs the `put` public API operation.
    pub fn put(&self, path: &str) -> TestRequestBuilder<'_> {
        TestRequestBuilder::new(self, Method::PUT, path)
    }

    /// Runs the `patch` public API operation.
    pub fn patch(&self, path: &str) -> TestRequestBuilder<'_> {
        TestRequestBuilder::new(self, Method::PATCH, path)
    }

    /// Runs the `delete` public API operation.
    pub fn delete(&self, path: &str) -> TestRequestBuilder<'_> {
        TestRequestBuilder::new(self, Method::DELETE, path)
    }

    /// Runs the `container` public API operation.
    pub fn container(&self) -> &Arc<Container> {
        &self.container
    }

    /// Runs the `resolve` public API operation.
    pub fn resolve<T: Send + Sync + 'static>(&self) -> Result<Arc<T>> {
        self.container.resolve::<T>()
    }

    /// Run module `on_shutdown` hooks. Dropping without this skips shutdown hooks.
    pub async fn shutdown(self) -> Result<()> {
        (self.shutdown_fn)(&self.container).await
    }

    async fn call(&self, request: Request<Body>) -> caelix_core::Result<TestResponse> {
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .map_err(|error| {
                caelix_core::InternalServerErrorException::new(std::io::Error::other(format!(
                    "test request failed: {error}"
                )))
            })?;

        Ok(TestResponse { response })
    }
}

impl<M: Module + 'static> TestApplicationBuilder<M> {
    /// Replace a provider by concrete type `T` with a pre-built instance.
    ///
    /// `T` must be the same type modules register and inject via `Arc<T>`.
    pub fn override_provider<T: Send + Sync + 'static>(mut self, value: T) -> Self {
        self.overrides = std::mem::take(&mut self.overrides).insert_instance(value);
        self
    }

    /// Replace a provider with an async factory producing type `T`.
    pub fn override_provider_factory<T, Fut, E>(
        mut self,
        dependencies: Vec<ProviderDependency>,
        factory: impl Fn(Arc<Container>) -> Fut + Send + Sync + 'static,
    ) -> Self
    where
        T: Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<T, E>> + Send + 'static,
        E: std::fmt::Debug + Send + 'static,
    {
        self.overrides =
            std::mem::take(&mut self.overrides).insert_factory::<T, Fut, E>(dependencies, factory);
        self
    }

    /// Runs the `body_limit` public API operation.
    pub fn body_limit(mut self, bytes: usize) -> Self {
        self.body_limit = bytes;
        self
    }

    #[cfg(feature = "uploads")]
    /// Changes the directory used to stage multipart uploads in this test application.
    pub fn upload_temp_dir(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.upload_config = self.upload_config.upload_temp_dir(path);
        self
    }

    /// Serves OpenAPI JSON and Swagger UI in the in-process test application.
    #[cfg(feature = "openapi")]
    /// Runs the `with_openapi` public API operation.
    pub fn with_openapi(mut self, config: OpenApiConfig) -> Self {
        self.openapi = Some(config);
        self
    }

    /// Runs the `compile` public API operation.
    pub async fn compile(self) -> Result<TestApplication> {
        let start = std::time::Instant::now();
        let container = build_container_with_overrides::<M>(self.overrides).await?;
        log_module_routes::<M>();
        log_application_started(start.elapsed());

        let container = Arc::new(container);
        let mut routes = AxumRouterBuilder::new();
        register_module_controllers::<M>(&mut routes);
        crate::websocket::configure_gateway_routes::<M>(
            &mut routes,
            container.clone(),
            crate::websocket::DEFAULT_WEBSOCKET_MAX_MESSAGE_SIZE,
        );

        let mut router = routes.into_router(container.clone());
        #[cfg(feature = "openapi")]
        if let Some(config) = self.openapi {
            let document = build_openapi::<M>(&config)?;
            router = crate::application::mount_openapi(
                router,
                config,
                document.to_json().expect("OpenAPI document must serialize"),
            );
        }
        router = router.layer(axum::Extension(UploadRuntimeConfig {
            #[cfg(feature = "uploads")]
            config: self.upload_config,
            body_limit: self.body_limit,
        }));
        router = router.layer(axum::extract::DefaultBodyLimit::max(self.body_limit));
        router = router.fallback(|request: Request<Body>| async move {
            to_axum_response(
                caelix_core::NotFoundException::new(format!(
                    "Cannot {} {}",
                    request.method(),
                    request.uri().path()
                ))
                .into_response(),
            )
        });

        Ok(TestApplication {
            container,
            router,
            shutdown_fn: |container| Box::pin(async move { shutdown_module::<M>(container).await }),
        })
    }
}

impl<M: Module + 'static> IntoFuture for TestApplicationBuilder<M> {
    type Output = caelix_core::Result<TestApplication>;
    type IntoFuture = Pin<Box<dyn Future<Output = caelix_core::Result<TestApplication>>>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move { self.compile().await })
    }
}

/// Public Caelix type `TestRequestBuilder`.
pub struct TestRequestBuilder<'a> {
    app: &'a TestApplication,
    request: Request<Body>,
}

impl<'a> TestRequestBuilder<'a> {
    fn new(app: &'a TestApplication, method: Method, path: &str) -> Self {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .body(Body::empty())
            .expect("test request path must be a valid URI");
        Self { app, request }
    }

    /// Runs the `json` public API operation.
    pub fn json(mut self, body: impl Serialize) -> Self {
        let body = serde_json::to_vec(&body).expect("test request JSON serialization failed");
        self.request.headers_mut().insert(
            HeaderName::from_static("content-type"),
            HeaderValue::from_static("application/json"),
        );
        *self.request.body_mut() = Body::from(body);
        self
    }

    /// Runs the `header` public API operation.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        let name = HeaderName::from_bytes(name.as_bytes()).expect("invalid test request header");
        let value = HeaderValue::from_str(value).expect("invalid test request header value");
        self.request.headers_mut().insert(name, value);
        self
    }

    /// Runs the `set_payload` public API operation.
    pub fn set_payload(mut self, bytes: impl Into<Bytes>) -> Self {
        *self.request.body_mut() = Body::from(bytes.into());
        self
    }

    /// Runs the `send` public API operation.
    pub async fn send(self) -> Result<TestResponse> {
        self.app.call(self.request).await
    }
}

/// Public Caelix type `TestResponse`.
pub struct TestResponse {
    response: Response,
}

impl TestResponse {
    /// Runs the `status` public API operation.
    pub fn status(&self) -> StatusCode {
        StatusCode::from_u16(self.response.status().as_u16())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    /// Runs the `assert_status` public API operation.
    pub fn assert_status(self, expected: StatusCode) -> Self {
        let actual = self.status();
        assert_eq!(
            actual, expected,
            "unexpected HTTP status: expected {expected}, got {actual}"
        );
        self
    }

    /// Runs the `json` public API operation.
    pub async fn json<T: DeserializeOwned>(self) -> T {
        let bytes = self.body().await;
        serde_json::from_slice(&bytes).expect("test response was not valid JSON")
    }

    /// Runs the `body` public API operation.
    pub async fn body(self) -> Bytes {
        axum::body::to_bytes(self.response.into_body(), usize::MAX)
            .await
            .expect("failed to read test response body")
    }

    /// Runs the `text` public API operation.
    pub async fn text(self) -> Result<String> {
        let bytes = self.body().await;
        String::from_utf8(bytes.to_vec()).map_err(|err| {
            caelix_core::InternalServerErrorException::new(std::io::Error::other(err))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json,
        extract::State,
        response::Response as AxumResponse,
        routing::{get, post},
    };
    use caelix_core::{Controller, Injectable, IntoCaelixResponse, ModuleMetadata, Response};
    use serde_json::{Value, json};
    use std::{
        any::Any,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static SHUTDOWN_COUNT: AtomicUsize = AtomicUsize::new(0);

    struct GreetingService {
        message: &'static str,
    }

    impl Injectable for GreetingService {
        fn dependencies() -> Vec<caelix_core::ProviderDependency> {
            caelix_core::provider_dependencies![]
        }

        fn create(_container: &Container) -> caelix_core::BoxFuture<'_, caelix_core::Result<Self>> {
            Box::pin(async move {
                Ok(Self {
                    message: "hello from production",
                })
            })
        }
    }

    struct GreetingController {
        service: Arc<GreetingService>,
    }

    impl Injectable for GreetingController {
        fn dependencies() -> Vec<caelix_core::ProviderDependency> {
            caelix_core::provider_dependencies![GreetingService]
        }

        fn create(container: &Container) -> caelix_core::BoxFuture<'_, caelix_core::Result<Self>> {
            Box::pin(async move {
                Ok(Self {
                    service: container.resolve::<GreetingService>()?,
                })
            })
        }
    }

    async fn greet(State(container): State<Arc<Container>>) -> AxumResponse {
        let controller = container.resolve::<GreetingController>().unwrap();
        to_axum_response(
            Response::Body(json!({ "message": controller.service.message })).into_response(),
        )
    }

    async fn echo(
        State(_container): State<Arc<Container>>,
        Json(body): Json<Value>,
    ) -> AxumResponse {
        to_axum_response(
            Response::WithStatus(caelix_core::StatusCode::CREATED, body).into_response(),
        )
    }

    impl Controller for GreetingController {
        fn base_path() -> &'static str {
            "/greet"
        }

        fn register_routes(cfg_any: &mut dyn Any) {
            let cfg = cfg_any
                .downcast_mut::<AxumRouterBuilder>()
                .expect("expected AxumRouterBuilder");
            cfg.route("/greet", get(greet));
            cfg.route("/greet/echo", post(echo));
        }
    }

    struct GreetingModule;
    impl Module for GreetingModule {
        fn register() -> ModuleMetadata {
            ModuleMetadata::new()
                .provider::<GreetingService>()
                .controller::<GreetingController>()
        }
    }

    struct NestedGreetingModule;
    impl Module for NestedGreetingModule {
        fn register() -> ModuleMetadata {
            ModuleMetadata::new()
                .provider::<GreetingService>()
                .export::<GreetingService>()
        }
    }

    struct RootGreetingModule;
    impl Module for RootGreetingModule {
        fn register() -> ModuleMetadata {
            ModuleMetadata::new()
                .import::<NestedGreetingModule>()
                .controller::<GreetingController>()
        }
    }

    struct ShutdownService;

    impl Injectable for ShutdownService {
        fn dependencies() -> Vec<caelix_core::ProviderDependency> {
            caelix_core::provider_dependencies![]
        }

        fn create(_container: &Container) -> caelix_core::BoxFuture<'_, caelix_core::Result<Self>> {
            Box::pin(async move { Ok(Self) })
        }

        fn on_shutdown(&self) -> caelix_core::BoxFuture<'_, caelix_core::Result<()>> {
            Box::pin(async move {
                SHUTDOWN_COUNT.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }
    }

    struct ShutdownModule;
    impl Module for ShutdownModule {
        fn register() -> ModuleMetadata {
            ModuleMetadata::new().provider::<ShutdownService>()
        }
    }

    #[tokio::test]
    async fn test_application_get_json() {
        let app = TestApplication::new::<GreetingModule>().await.unwrap();

        let body: Value = app
            .get("/greet")
            .send()
            .await
            .unwrap()
            .assert_status(caelix_core::StatusCode::OK)
            .json()
            .await;
        assert_eq!(body, json!({ "message": "hello from production" }));
    }

    #[tokio::test]
    async fn test_application_post_json_created() {
        let app = TestApplication::new::<GreetingModule>().await.unwrap();
        let response = app
            .post("/greet/echo")
            .json(json!({ "name": "Ronnie" }))
            .send()
            .await
            .unwrap()
            .assert_status(caelix_core::StatusCode::CREATED);

        let body: Value = response.json().await;
        assert_eq!(body, json!({ "name": "Ronnie" }));
    }

    #[tokio::test]
    async fn test_application_override_nested_provider() {
        let app = TestApplication::new::<RootGreetingModule>()
            .override_provider(GreetingService {
                message: "hello from test",
            })
            .await
            .unwrap();

        let body: Value = app.get("/greet").send().await.unwrap().json().await;
        assert_eq!(body, json!({ "message": "hello from test" }));
        assert_eq!(
            app.resolve::<GreetingService>().unwrap().message,
            "hello from test"
        );
    }

    #[tokio::test]
    async fn test_application_enforces_body_limit() {
        let app = TestApplication::new::<GreetingModule>()
            .body_limit(8)
            .await
            .unwrap();

        let response = app
            .post("/greet/echo")
            .header("content-type", "application/json")
            .set_payload(r#"{"too":"large"}"#)
            .send()
            .await
            .unwrap();

        response.assert_status(caelix_core::StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn test_application_not_found_is_caelix_json() {
        let app = TestApplication::new::<GreetingModule>().await.unwrap();

        let body: Value = app
            .get("/missing")
            .send()
            .await
            .unwrap()
            .assert_status(caelix_core::StatusCode::NOT_FOUND)
            .json()
            .await;
        assert_eq!(
            body,
            json!({
                "status": 404,
                "error": "Not Found",
                "message": "Cannot GET /missing"
            })
        );
    }

    #[tokio::test]
    async fn test_application_shutdown_runs_hooks() {
        SHUTDOWN_COUNT.store(0, Ordering::SeqCst);

        let app = TestApplication::new::<ShutdownModule>().await.unwrap();
        app.shutdown().await.unwrap();

        assert_eq!(SHUTDOWN_COUNT.load(Ordering::SeqCst), 1);
    }
}
