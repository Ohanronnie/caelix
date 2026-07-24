use std::{
    convert::Infallible,
    ffi::{OsStr, OsString},
    sync::Arc,
    time::Instant,
};

#[cfg(feature = "openapi")]
use axum::http::header;
use axum::{
    Router,
    body::Body,
    extract::{Extension, FromRequestParts},
    http::{HeaderMap, Method, Request, Uri, request::Parts},
    response::Response,
};
#[cfg(feature = "uploads")]
use caelix_core::UploadConfig;
#[cfg(not(feature = "socketio"))]
use caelix_core::build_container;
#[cfg(feature = "socketio")]
use caelix_core::build_container_with_setup;
#[cfg(feature = "openapi")]
use caelix_core::openapi::{OpenApiConfig, build_openapi};
#[cfg(feature = "socketio")]
use caelix_core::visit_module_gateways;
use caelix_core::{
    BoxFuture, Container, HttpResponse as CaelixHttpResponse, IntoCaelixResponse, Module,
    NotFoundException, ResponseBody, Result, log_application_started, log_listening,
    log_module_routes, register_module_controllers, shutdown_module,
};
use futures_util::StreamExt;
use tower::{Layer, Service};

/// Public Caelix constant `DEFAULT_BODY_LIMIT_BYTES`.
pub const DEFAULT_BODY_LIMIT_BYTES: usize = 1024 * 1024;

/// Application-scoped multipart storage and limit configuration.
#[derive(Clone)]
pub(crate) struct UploadRuntimeConfig {
    #[cfg(feature = "uploads")]
    pub(crate) config: UploadConfig,
    pub(crate) body_limit: usize,
}

impl Default for UploadRuntimeConfig {
    fn default() -> Self {
        Self {
            #[cfg(feature = "uploads")]
            config: UploadConfig::default(),
            body_limit: DEFAULT_BODY_LIMIT_BYTES,
        }
    }
}

#[cfg(feature = "openapi")]
#[derive(Clone)]
struct OpenApiServices {
    config: OpenApiConfig,
    document: String,
}

/// The request data Caelix needs to construct a framework-neutral
/// [`caelix_core::RequestContext`]. It is a parts-only Axum extractor so it
/// can be combined with `Json` and other body-consuming extractors.
pub struct AxumRequestInfo {
    method: Method,
    uri: Uri,
    headers: HeaderMap,
}

impl AxumRequestInfo {
    /// Runs the `method` public API operation.
    pub fn method(&self) -> &Method {
        &self.method
    }
    /// Runs the `path` public API operation.
    pub fn path(&self) -> &str {
        self.uri.path()
    }
    /// Runs the `headers` public API operation.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }
}

impl<S> FromRequestParts<S> for AxumRequestInfo
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        _: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        Ok(Self {
            method: parts.method.clone(),
            uri: parts.uri.clone(),
            headers: parts.headers.clone(),
        })
    }
}

/// Mutable route collector used by framework-neutral controller metadata.
///
/// Axum's [`Router`] builder consumes itself when registering a route. This
/// wrapper preserves Caelix's existing `Controller::register_routes(&mut dyn Any)`
/// contract without putting an HTTP framework type in `caelix-core`.
pub struct AxumRouterBuilder {
    router: Router<Arc<Container>>,
}

impl AxumRouterBuilder {
    /// Runs the `new` public API operation.
    pub fn new() -> Self {
        Self {
            router: Router::new(),
        }
    }

    /// Runs the `route` public API operation.
    pub fn route(
        &mut self,
        path: &str,
        method_router: axum::routing::MethodRouter<Arc<Container>>,
    ) {
        self.router = std::mem::take(&mut self.router).route(path, method_router);
    }

    /// Runs the `into_router` public API operation.
    pub fn into_router(self, container: Arc<Container>) -> Router {
        self.router.with_state(container)
    }
}

impl Default for AxumRouterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Converts a framework-neutral response into an Axum response.
pub fn to_axum_response(response: CaelixHttpResponse) -> Response {
    let mut builder = Response::builder()
        .status(response.status)
        .header("content-type", response.content_type);
    for (name, value) in response.headers {
        builder = builder.header(name, value);
    }
    for cookie in response.cookies {
        builder = builder.header(axum::http::header::SET_COOKIE, cookie.to_header_value());
    }

    let body = match response.body {
        ResponseBody::Buffered(bytes) => Body::from(bytes),
        ResponseBody::Streaming(stream) => {
            Body::from_stream(stream.filter_map(|chunk| async move {
                match chunk {
                    Ok(chunk) => Some(Ok::<_, Infallible>(axum::body::Bytes::from(chunk))),
                    Err(err) => {
                        caelix_core::log_http_exception(&err);
                        // HTTP status and headers have already been committed when
                        // a stream fails, so end the response body cleanly.
                        None
                    }
                }
            }))
        }
    };

    builder
        .body(body)
        .unwrap_or_else(|_| Response::new(Body::from("Internal Server Error")))
}

type RouterLayer = Box<dyn FnOnce(Router) -> Router + Send>;

/// Public Caelix type `Application`.
pub struct Application {
    container: Arc<Container>,
    configure_fn: fn(&mut dyn std::any::Any),
    gateway_configure_fn: fn(&mut AxumRouterBuilder, Arc<Container>, usize),
    shutdown_fn: for<'a> fn(&'a Container) -> BoxFuture<'a, caelix_core::Result<()>>,
    body_limit: usize,
    #[cfg(feature = "uploads")]
    upload_config: UploadConfig,
    websocket_max_message_size: usize,
    layers: Vec<RouterLayer>,
    #[cfg(feature = "openapi")]
    openapi: Option<OpenApiServices>,
    #[cfg(feature = "openapi")]
    openapi_build_fn:
        fn(&OpenApiConfig) -> caelix_core::Result<caelix_core::openapi::utoipa::openapi::OpenApi>,
    #[cfg(feature = "socketio")]
    socket_io_layer: Option<caelix_socketio::SocketIoLayer>,
}

impl Application {
    /// Runs the `new` public API operation.
    pub async fn new<M: Module + 'static>() -> Result<Self> {
        let start = Instant::now();
        #[cfg(not(feature = "socketio"))]
        let container = build_container::<M>().await?;
        #[cfg(feature = "socketio")]
        let (container, socket_io_layer) = {
            let (layer, handle) = caelix_socketio::SocketIoHandle::build();
            let container = build_container_with_setup::<M>(|container| {
                container.register_instance(handle);
            })
            .await?;
            (container, layer)
        };
        log_module_routes::<M>();
        log_application_started(start.elapsed());

        Ok(Self {
            container: Arc::new(container),
            configure_fn: |router| register_module_controllers::<M>(router),
            gateway_configure_fn: |routes, container, max_message_size| {
                crate::websocket::configure_gateway_routes::<M>(routes, container, max_message_size)
            },
            shutdown_fn: |container| Box::pin(async move { shutdown_module::<M>(container).await }),
            body_limit: DEFAULT_BODY_LIMIT_BYTES,
            #[cfg(feature = "uploads")]
            upload_config: UploadConfig::default(),
            websocket_max_message_size: crate::websocket::DEFAULT_WEBSOCKET_MAX_MESSAGE_SIZE,
            layers: Vec::new(),
            #[cfg(feature = "openapi")]
            openapi: None,
            #[cfg(feature = "openapi")]
            openapi_build_fn: |config| build_openapi::<M>(config),
            #[cfg(feature = "socketio")]
            socket_io_layer: Some(socket_io_layer),
        })
    }

    /// Runs the `body_limit` public API operation.
    pub fn body_limit(mut self, bytes: usize) -> Self {
        self.body_limit = bytes;
        self
    }

    #[cfg(feature = "uploads")]
    /// Changes the directory used to stage multipart uploads before they are persisted.
    pub fn upload_temp_dir(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.upload_config = self.upload_config.upload_temp_dir(path);
        self
    }

    /// Generates and serves OpenAPI JSON plus Swagger UI for this application.
    #[cfg(feature = "openapi")]
    /// Runs the `with_openapi` public API operation.
    pub fn with_openapi(mut self, config: OpenApiConfig) -> Result<Self> {
        let document = (self.openapi_build_fn)(&config)?;
        self.openapi = Some(OpenApiServices {
            config,
            document: document.to_json().expect("OpenAPI document must serialize"),
        });
        Ok(self)
    }

    /// Sets the maximum assembled RFC 6455 message size accepted by decorated
    /// WebSocket gateways.
    pub fn websocket_max_message_size(mut self, bytes: usize) -> Self {
        self.websocket_max_message_size = bytes.max(1);
        self
    }

    /// Adds a native Tower layer to the underlying Axum router.
    pub fn layer<L>(mut self, layer: L) -> Self
    where
        L: Layer<axum::routing::Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request<Body>> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request<Body>>>::Response: axum::response::IntoResponse + 'static,
        <L::Service as Service<Request<Body>>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request<Body>>>::Future: Send + 'static,
    {
        self.layers
            .push(Box::new(move |router| router.layer(layer)));
        self
    }

    /// Attaches Socket.IO to this Axum application and registers its handle as
    /// an injectable provider. This method only exists when the `socketio`
    /// feature is enabled, which itself requires the Axum backend.
    #[cfg(feature = "socketio")]
    /// Runs the `with_socket_io` public API operation.
    pub fn with_socket_io<M: Module + 'static>(mut self) -> Self {
        let handle = self
            .container
            .resolve::<caelix_socketio::SocketIoHandle>()
            .expect("Socket.IO handle must be registered before module providers are built");
        visit_module_gateways::<M>(&mut |gateway| {
            gateway
                .register_socket_io(&self.container, handle.as_ref())
                .expect("Socket.IO gateway metadata must be valid");
        });
        let layer = self
            .socket_io_layer
            .take()
            .expect("Socket.IO can only be attached once per application");
        self.layer(layer)
    }

    /// Runs the `into_router` public API operation.
    pub fn into_router(self) -> Router {
        let mut routes = AxumRouterBuilder::new();
        (self.configure_fn)(&mut routes);
        (self.gateway_configure_fn)(
            &mut routes,
            self.container.clone(),
            self.websocket_max_message_size,
        );
        let mut router = routes.into_router(self.container);
        #[cfg(feature = "openapi")]
        if let Some(openapi) = self.openapi {
            router = mount_openapi(router, openapi.config, openapi.document);
        }
        router = router.layer(Extension(UploadRuntimeConfig {
            #[cfg(feature = "uploads")]
            config: self.upload_config,
            body_limit: self.body_limit,
        }));
        router = router.layer(axum::extract::DefaultBodyLimit::max(self.body_limit));
        router = router.fallback(|request: Request<Body>| async move {
            to_axum_response(
                NotFoundException::new(format!(
                    "Cannot {} {}",
                    request.method(),
                    request.uri().path()
                ))
                .into_response(),
            )
        });
        for layer in self.layers {
            router = layer(router);
        }
        router
    }

    async fn shutdown(&self) -> caelix_core::Result<()> {
        (self.shutdown_fn)(&self.container).await
    }

    /// Runs the `listen` public API operation.
    pub async fn listen(self, addr: &str) -> std::io::Result<()> {
        self.listen_with_doctor_mode(addr, has_doctor_argument(std::env::args_os()))
            .await
    }

    async fn listen_with_doctor_mode(self, addr: &str, doctor_mode: bool) -> std::io::Result<()> {
        if doctor_mode {
            let shutdown_fn = self.shutdown_fn;
            let container = self.container.clone();
            let _router = self.into_router();
            return shutdown_fn(&container).await.map_err(to_io_error);
        }

        log_listening(addr);
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(error) => {
                let _ = self.shutdown().await;
                return Err(error);
            }
        };
        let shutdown_fn = self.shutdown_fn;
        let container = self.container.clone();
        let router = self.into_router();
        let result = axum::serve(listener, router).await;
        shutdown_fn(&container).await.map_err(to_io_error)?;
        result
    }
}

fn has_doctor_argument<I>(args: I) -> bool
where
    I: IntoIterator<Item = OsString>,
{
    args.into_iter().any(|arg| arg == OsStr::new("--doctor"))
}

#[cfg(feature = "openapi")]
pub(crate) fn mount_openapi(router: Router, config: OpenApiConfig, document: String) -> Router {
    let json_document = document.clone();
    let json_path = config.json_path.clone();
    let html = swagger_ui_html(&json_path);
    router
        .route(
            &json_path,
            axum::routing::get(move || {
                let document = json_document.clone();
                async move { ([(header::CONTENT_TYPE, "application/json")], document) }
            }),
        )
        .route(
            &config.ui_path,
            axum::routing::get({
                let html = html.clone();
                move || {
                    let html = html.clone();
                    async move { ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html) }
                }
            }),
        )
        .route(
            &format!("{}/", config.ui_path.trim_end_matches('/')),
            axum::routing::get(move || {
                let html = html.clone();
                async move { ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html) }
            }),
        )
}

#[cfg(feature = "openapi")]
fn swagger_ui_html(json_path: &str) -> String {
    let json_path = serde_json::to_string(json_path).expect("OpenAPI path must serialize");
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>Swagger UI</title><link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css"></head><body><div id="swagger-ui"></div><script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script><script>SwaggerUIBundle({{url:{json_path},dom_id:'#swagger-ui'}});</script></body></html>"#
    )
}

fn to_io_error(err: caelix_core::HttpException) -> std::io::Error {
    std::io::Error::other(err.message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use caelix_core::{
        Controller, HttpResponse, Injectable, ModuleMetadata, Response as CaelixResponse,
    };
    use http_body_util::BodyExt;
    use std::{
        any::Any,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static DOCTOR_STARTUP_COUNT: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn response_adapter_appends_every_cookie_header() {
        let response = to_axum_response(
            HttpResponse::text(StatusCode::OK, "ok")
                .with_cookie(caelix_core::Cookie::new("session", "a b"))
                .with_cookie(caelix_core::Cookie::removal("preference").path("/settings")),
        );
        let values = response
            .headers()
            .get_all(axum::http::header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(values.len(), 2);
        assert!(values[0].contains("session=a%20b"));
        assert!(values[0].contains("HttpOnly"));
        assert!(values[0].contains("Secure"));
        assert!(values[1].contains("Max-Age=0"));
        assert!(values[1].contains("Path=/settings"));
    }
    static DOCTOR_CONSTRUCTION_COUNT: AtomicUsize = AtomicUsize::new(0);
    static DOCTOR_INIT_COUNT: AtomicUsize = AtomicUsize::new(0);
    static DOCTOR_SHUTDOWN_COUNT: AtomicUsize = AtomicUsize::new(0);
    static DOCTOR_ROUTE_CONFIG_COUNT: AtomicUsize = AtomicUsize::new(0);

    #[tokio::test]
    async fn response_adapter_preserves_status_body_and_content_type() {
        let response = to_axum_response(
            HttpResponse::text(StatusCode::CREATED, "created").with_header("X-Request-Id", "abc"),
        );
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()["content-type"], "text/plain");
        assert_eq!(response.headers()["x-request-id"], "abc");
        assert_eq!(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .as_ref(),
            b"created"
        );
    }

    #[tokio::test]
    async fn response_adapter_streams_chunks() {
        let response = to_axum_response(CaelixResponse::stream(
            "text/plain",
            futures_util::stream::iter(vec![
                Ok(bytes::Bytes::from_static(b"one")),
                Ok(bytes::Bytes::from_static(b"two")),
            ]),
        ));
        assert_eq!(response.headers()["content-type"], "text/plain");
        assert_eq!(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .as_ref(),
            b"onetwo"
        );
    }

    struct EmptyModule;
    impl Module for EmptyModule {
        fn register() -> ModuleMetadata {
            ModuleMetadata::new()
        }
    }

    struct DoctorService;

    impl Injectable for DoctorService {
        fn dependencies() -> Vec<caelix_core::ProviderDependency> {
            caelix_core::provider_dependencies![]
        }

        fn create(_container: &Container) -> caelix_core::BoxFuture<'_, caelix_core::Result<Self>> {
            Box::pin(async move {
                DOCTOR_CONSTRUCTION_COUNT.fetch_add(1, Ordering::SeqCst);
                Ok(Self)
            })
        }

        fn on_module_init(&self) -> caelix_core::BoxFuture<'_, caelix_core::Result<()>> {
            Box::pin(async move {
                DOCTOR_INIT_COUNT.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }

        fn on_bootstrap(&self) -> caelix_core::BoxFuture<'_, caelix_core::Result<()>> {
            Box::pin(async move {
                DOCTOR_STARTUP_COUNT.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }

        fn on_shutdown(&self) -> caelix_core::BoxFuture<'_, caelix_core::Result<()>> {
            Box::pin(async move {
                DOCTOR_SHUTDOWN_COUNT.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }
    }

    struct DoctorController;

    impl Injectable for DoctorController {
        fn dependencies() -> Vec<caelix_core::ProviderDependency> {
            caelix_core::provider_dependencies![]
        }

        fn create(_container: &Container) -> caelix_core::BoxFuture<'_, caelix_core::Result<Self>> {
            Box::pin(async move { Ok(Self) })
        }
    }

    impl Controller for DoctorController {
        fn base_path() -> &'static str {
            "/doctor"
        }

        fn register_routes(routes_any: &mut dyn Any) {
            DOCTOR_ROUTE_CONFIG_COUNT.fetch_add(1, Ordering::SeqCst);
            let routes = routes_any
                .downcast_mut::<AxumRouterBuilder>()
                .expect("expected AxumRouterBuilder");
            routes.route("/doctor", axum::routing::get(|| async {}));
        }
    }

    struct DoctorModule;

    impl Module for DoctorModule {
        fn register() -> ModuleMetadata {
            ModuleMetadata::new()
                .provider::<DoctorService>()
                .controller::<DoctorController>()
        }
    }

    struct FailingShutdownService;

    impl Injectable for FailingShutdownService {
        fn dependencies() -> Vec<caelix_core::ProviderDependency> {
            caelix_core::provider_dependencies![]
        }

        fn create(_container: &Container) -> caelix_core::BoxFuture<'_, caelix_core::Result<Self>> {
            Box::pin(async move { Ok(Self) })
        }

        fn on_shutdown(&self) -> caelix_core::BoxFuture<'_, caelix_core::Result<()>> {
            Box::pin(async move {
                Err(caelix_core::HttpException::new(
                    caelix_core::StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal Server Error",
                    "shutdown failed",
                ))
            })
        }
    }

    struct FailingShutdownModule;

    impl Module for FailingShutdownModule {
        fn register() -> ModuleMetadata {
            ModuleMetadata::new().provider::<FailingShutdownService>()
        }
    }

    #[tokio::test]
    async fn application_accepts_native_tower_layers() {
        let _router = Application::new::<EmptyModule>()
            .await
            .unwrap()
            .layer(tower_http::trace::TraceLayer::new_for_http())
            .layer(tower_http::compression::CompressionLayer::new())
            .into_router();
    }

    #[tokio::test]
    async fn doctor_mode_runs_startup_runtime_setup_and_shutdown_without_binding() {
        DOCTOR_CONSTRUCTION_COUNT.store(0, Ordering::SeqCst);
        DOCTOR_INIT_COUNT.store(0, Ordering::SeqCst);
        DOCTOR_STARTUP_COUNT.store(0, Ordering::SeqCst);
        DOCTOR_SHUTDOWN_COUNT.store(0, Ordering::SeqCst);
        DOCTOR_ROUTE_CONFIG_COUNT.store(0, Ordering::SeqCst);

        let application = Application::new::<DoctorModule>().await.unwrap();
        assert_eq!(DOCTOR_CONSTRUCTION_COUNT.load(Ordering::SeqCst), 1);
        assert_eq!(DOCTOR_INIT_COUNT.load(Ordering::SeqCst), 1);
        assert_eq!(DOCTOR_STARTUP_COUNT.load(Ordering::SeqCst), 1);

        application
            .listen_with_doctor_mode("not a socket address", true)
            .await
            .unwrap();

        assert_eq!(DOCTOR_ROUTE_CONFIG_COUNT.load(Ordering::SeqCst), 1);
        assert_eq!(DOCTOR_SHUTDOWN_COUNT.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn doctor_mode_propagates_shutdown_failures() {
        let error = Application::new::<FailingShutdownModule>()
            .await
            .unwrap()
            .listen_with_doctor_mode("not a socket address", true)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("shutdown failed"));
    }

    #[tokio::test]
    async fn normal_listen_still_attempts_to_bind_the_configured_address() {
        let error = Application::new::<EmptyModule>()
            .await
            .unwrap()
            .listen_with_doctor_mode("127.0.0.1:not-a-port", false)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("invalid port value"));
    }

    #[test]
    fn doctor_mode_requires_the_exact_process_argument() {
        assert!(has_doctor_argument([OsString::from("--doctor")]));
        assert!(!has_doctor_argument([OsString::from("--doctor=true")]));
        assert!(!has_doctor_argument([OsString::from("doctor")]));
    }
}
