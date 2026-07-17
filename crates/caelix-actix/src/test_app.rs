use std::{
    future::{Future, IntoFuture},
    marker::PhantomData,
    pin::Pin,
    rc::Rc,
    sync::Arc,
};

use actix_http::Request;
use actix_web::{
    App, Error,
    dev::{Service, ServiceResponse},
    test as actix_test, web,
};
use bytes::Bytes;
use caelix_core::{
    BoxFuture, Container, Module, ProviderDependency, ProviderOverrides, Result, StatusCode,
    UploadConfig, build_container_with_overrides, log_application_started, log_module_routes,
    register_module_controllers, shutdown_module,
};
use serde::{Serialize, de::DeserializeOwned};

#[cfg(feature = "openapi")]
use crate::application::OpenApiServices;
use crate::application::{DEFAULT_BODY_LIMIT_BYTES, configure_caelix_services};
#[cfg(feature = "openapi")]
use caelix_core::openapi::{OpenApiConfig, build_openapi};

type CallFuture = Pin<Box<dyn Future<Output = std::result::Result<ServiceResponse, Error>>>>;
type CallFn = Box<dyn Fn(Request) -> CallFuture>;

/// In-process Caelix application for integration tests.
///
/// Builds the same DI container and route table as production, then serves
/// requests through Actix's test service (no TCP listener).
pub struct TestApplication {
    container: Arc<Container>,
    call: CallFn,
    shutdown_fn: for<'a> fn(&'a Container) -> BoxFuture<'a, caelix_core::Result<()>>,
}

/// Builder for [`TestApplication`]. Await it (or call [`compile`](Self::compile))
/// to finish startup.
pub struct TestApplicationBuilder<M> {
    overrides: ProviderOverrides,
    body_limit: usize,
    upload_config: UploadConfig,
    #[cfg(feature = "openapi")]
    openapi: Option<OpenApiConfig>,
    _module: PhantomData<M>,
}

impl TestApplication {
    /// Start configuring a test application for module `M`.
    ///
    /// ```ignore
    /// let app = TestApplication::new::<AppModule>().await;
    /// let app = TestApplication::new::<AppModule>()
    ///     .override_provider(UserRepository::in_memory())
    ///     .await;
    /// ```
    pub fn new<M: Module + 'static>() -> TestApplicationBuilder<M> {
        TestApplicationBuilder {
            overrides: ProviderOverrides::new(),
            body_limit: DEFAULT_BODY_LIMIT_BYTES,
            upload_config: UploadConfig::default(),
            #[cfg(feature = "openapi")]
            openapi: None,
            _module: PhantomData,
        }
    }

    /// Runs the `get` public API operation.
    pub fn get(&self, path: &str) -> TestRequestBuilder<'_> {
        TestRequestBuilder::new(self, actix_test::TestRequest::get().uri(path))
    }

    /// Runs the `post` public API operation.
    pub fn post(&self, path: &str) -> TestRequestBuilder<'_> {
        TestRequestBuilder::new(self, actix_test::TestRequest::post().uri(path))
    }

    /// Runs the `put` public API operation.
    pub fn put(&self, path: &str) -> TestRequestBuilder<'_> {
        TestRequestBuilder::new(self, actix_test::TestRequest::put().uri(path))
    }

    /// Runs the `patch` public API operation.
    pub fn patch(&self, path: &str) -> TestRequestBuilder<'_> {
        TestRequestBuilder::new(self, actix_test::TestRequest::patch().uri(path))
    }

    /// Runs the `delete` public API operation.
    pub fn delete(&self, path: &str) -> TestRequestBuilder<'_> {
        TestRequestBuilder::new(self, actix_test::TestRequest::delete().uri(path))
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

    async fn call(&self, request: Request) -> caelix_core::Result<ServiceResponse> {
        (self.call)(request).await.map_err(|err| {
            caelix_core::InternalServerErrorException::new(std::io::Error::other(format!(
                "test request failed: {err}"
            )))
        })
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
        let body_limit = self.body_limit;
        let upload_config = self.upload_config;
        let configure_fn: fn(&mut web::ServiceConfig) = |cfg| register_module_controllers::<M>(cfg);
        #[cfg(feature = "openapi")]
        let openapi = match self.openapi {
            Some(config) => {
                let document = build_openapi::<M>(&config)?;
                Some(OpenApiServices {
                    config,
                    document: document.to_json().expect("OpenAPI document must serialize"),
                })
            }
            None => None,
        };

        let app = App::new()
            .app_data(web::Data::from(container.clone()))
            .configure(move |cfg| {
                configure_caelix_services(cfg, body_limit, upload_config.clone(), configure_fn, {
                    #[cfg(feature = "openapi")]
                    {
                        openapi.as_ref()
                    }
                    #[cfg(not(feature = "openapi"))]
                    {
                        None
                    }
                })
            });

        let service = Rc::new(actix_test::init_service(app).await);
        let call: CallFn = Box::new(move |request| {
            let service = service.clone();
            Box::pin(async move { service.call(request).await })
        });

        Ok(TestApplication {
            container,
            call,
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
    request: actix_test::TestRequest,
}

impl<'a> TestRequestBuilder<'a> {
    fn new(app: &'a TestApplication, request: actix_test::TestRequest) -> Self {
        Self { app, request }
    }

    /// Runs the `json` public API operation.
    pub fn json(mut self, body: impl Serialize) -> Self {
        self.request = self.request.set_json(body);
        self
    }

    /// Runs the `header` public API operation.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.request = self.request.insert_header((name, value));
        self
    }

    /// Runs the `set_payload` public API operation.
    pub fn set_payload(mut self, bytes: impl Into<Bytes>) -> Self {
        self.request = self.request.set_payload(bytes);
        self
    }

    /// Runs the `send` public API operation.
    pub async fn send(self) -> Result<TestResponse> {
        let response = self.app.call(self.request.to_request()).await?;
        Ok(TestResponse { response })
    }
}

/// Public Caelix type `TestResponse`.
pub struct TestResponse {
    response: ServiceResponse,
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
        actix_test::read_body_json(self.response).await
    }

    /// Runs the `body` public API operation.
    pub async fn body(self) -> Bytes {
        actix_test::read_body(self.response).await
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
    use crate::to_actix_response;
    use actix_web::HttpResponse;
    use caelix_core::{
        Controller, Injectable, IntoCaelixResponse, ModuleMetadata, Response,
        StatusCode as CaelixStatus,
    };
    use serde::Deserialize;
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

    impl GreetingController {
        async fn greet(service: web::Data<Container>) -> HttpResponse {
            let controller = service.resolve::<GreetingController>().unwrap();
            to_actix_response(
                Response::Body(json!({ "message": controller.service.message })).into_response(),
            )
        }

        async fn echo(body: web::Json<Value>) -> HttpResponse {
            to_actix_response(
                Response::WithStatus(CaelixStatus::CREATED, body.into_inner()).into_response(),
            )
        }
    }

    impl Controller for GreetingController {
        fn base_path() -> &'static str {
            "/greet"
        }

        fn register_routes(cfg_any: &mut dyn Any) {
            let cfg = cfg_any
                .downcast_mut::<web::ServiceConfig>()
                .expect("expected actix ServiceConfig");

            cfg.route(
                "/greet",
                web::get().to(|container: web::Data<Container>| async move {
                    GreetingController::greet(container).await
                }),
            );
            cfg.route(
                "/greet/echo",
                web::post().to(|body: web::Json<Value>| async move {
                    GreetingController::echo(body).await
                }),
            );
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

    #[actix_web::test]
    async fn test_application_get_json() {
        let app = TestApplication::new::<GreetingModule>().await.unwrap();

        let body: Value = app
            .get("/greet")
            .send()
            .await
            .unwrap()
            .assert_status(CaelixStatus::OK)
            .json()
            .await;
        assert_eq!(body, json!({ "message": "hello from production" }));
    }

    #[actix_web::test]
    async fn test_application_post_json_created() {
        let app = TestApplication::new::<GreetingModule>().await.unwrap();

        #[derive(Deserialize)]
        struct Echo {
            name: String,
        }

        let response = app
            .post("/greet/echo")
            .json(json!({ "name": "Ronnie" }))
            .send()
            .await
            .unwrap()
            .assert_status(CaelixStatus::CREATED);

        let body: Echo = response.json().await;
        assert_eq!(body.name, "Ronnie");
    }

    #[actix_web::test]
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

    #[actix_web::test]
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

        response.assert_status(CaelixStatus::PAYLOAD_TOO_LARGE);
    }

    #[actix_web::test]
    async fn test_application_not_found_is_caelix_json() {
        let app = TestApplication::new::<GreetingModule>().await.unwrap();

        let body: Value = app
            .get("/missing")
            .send()
            .await
            .unwrap()
            .assert_status(CaelixStatus::NOT_FOUND)
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

    #[actix_web::test]
    async fn test_application_shutdown_runs_hooks() {
        SHUTDOWN_COUNT.store(0, Ordering::SeqCst);

        let app = TestApplication::new::<ShutdownModule>().await.unwrap();
        app.shutdown().await.unwrap();

        assert_eq!(SHUTDOWN_COUNT.load(Ordering::SeqCst), 1);
    }
}
