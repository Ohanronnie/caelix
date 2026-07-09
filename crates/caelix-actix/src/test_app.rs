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
    BoxFuture, Container, Module, ProviderOverrides, StatusCode, build_container_with_overrides,
    log_application_started, log_module_routes, register_module_controllers, shutdown_module,
};
use serde::{Serialize, de::DeserializeOwned};

use crate::application::{DEFAULT_BODY_LIMIT_BYTES, configure_caelix_services};

type CallFuture = Pin<Box<dyn Future<Output = Result<ServiceResponse, Error>>>>;
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
            _module: PhantomData,
        }
    }

    pub fn get(&self, path: &str) -> TestRequestBuilder<'_> {
        TestRequestBuilder::new(self, actix_test::TestRequest::get().uri(path))
    }

    pub fn post(&self, path: &str) -> TestRequestBuilder<'_> {
        TestRequestBuilder::new(self, actix_test::TestRequest::post().uri(path))
    }

    pub fn put(&self, path: &str) -> TestRequestBuilder<'_> {
        TestRequestBuilder::new(self, actix_test::TestRequest::put().uri(path))
    }

    pub fn patch(&self, path: &str) -> TestRequestBuilder<'_> {
        TestRequestBuilder::new(self, actix_test::TestRequest::patch().uri(path))
    }

    pub fn delete(&self, path: &str) -> TestRequestBuilder<'_> {
        TestRequestBuilder::new(self, actix_test::TestRequest::delete().uri(path))
    }

    pub fn container(&self) -> &Arc<Container> {
        &self.container
    }

    pub fn resolve<T: Send + Sync + 'static>(&self) -> caelix_core::Result<Arc<T>> {
        self.container.resolve::<T>()
    }

    /// Run module `on_shutdown` hooks. Dropping without this skips shutdown hooks.
    pub async fn shutdown(self) -> caelix_core::Result<()> {
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
        factory: impl Fn(Arc<Container>) -> Fut + Send + Sync + 'static,
    ) -> Self
    where
        T: Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<T, E>> + Send + 'static,
        E: std::fmt::Debug + Send + 'static,
    {
        self.overrides = std::mem::take(&mut self.overrides).insert_factory::<T, Fut, E>(factory);
        self
    }

    pub fn body_limit(mut self, bytes: usize) -> Self {
        self.body_limit = bytes;
        self
    }

    pub async fn compile(self) -> caelix_core::Result<TestApplication> {
        let start = std::time::Instant::now();
        let container = build_container_with_overrides::<M>(self.overrides).await?;
        log_module_routes::<M>();
        log_application_started(start.elapsed());

        let container = Arc::new(container);
        let body_limit = self.body_limit;
        let configure_fn: fn(&mut web::ServiceConfig) = |cfg| register_module_controllers::<M>(cfg);

        let app = App::new()
            .app_data(web::Data::from(container.clone()))
            .configure(move |cfg| configure_caelix_services(cfg, body_limit, configure_fn));

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

pub struct TestRequestBuilder<'a> {
    app: &'a TestApplication,
    request: actix_test::TestRequest,
}

impl<'a> TestRequestBuilder<'a> {
    fn new(app: &'a TestApplication, request: actix_test::TestRequest) -> Self {
        Self { app, request }
    }

    pub fn json(mut self, body: impl Serialize) -> Self {
        self.request = self.request.set_json(body);
        self
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.request = self.request.insert_header((name, value));
        self
    }

    pub fn set_payload(mut self, bytes: impl Into<Bytes>) -> Self {
        self.request = self.request.set_payload(bytes);
        self
    }

    pub async fn send(self) -> caelix_core::Result<TestResponse> {
        let response = self.app.call(self.request.to_request()).await?;
        Ok(TestResponse { response })
    }
}

pub struct TestResponse {
    response: ServiceResponse,
}

impl TestResponse {
    pub fn status(&self) -> StatusCode {
        StatusCode::from_u16(self.response.status().as_u16())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    pub fn assert_status(self, expected: StatusCode) -> Self {
        let actual = self.status();
        assert_eq!(
            actual, expected,
            "unexpected HTTP status: expected {expected}, got {actual}"
        );
        self
    }

    pub async fn json<T: DeserializeOwned>(self) -> T {
        actix_test::read_body_json(self.response).await
    }

    pub async fn body(self) -> Bytes {
        actix_test::read_body(self.response).await
    }

    pub async fn text(self) -> caelix_core::Result<String> {
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
            ModuleMetadata::new().provider::<GreetingService>()
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
