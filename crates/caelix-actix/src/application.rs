use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    sync::Arc,
    time::Instant,
};

use actix_web::{
    App, HttpRequest, HttpResponse, HttpServer,
    body::{BodySize, MessageBody},
    dev::{Service, ServiceResponse},
    error::{JsonPayloadError, PathError, QueryPayloadError},
    http::header,
    web,
};
#[cfg(feature = "uploads")]
use caelix_core::UploadConfig;
#[cfg(feature = "openapi")]
use caelix_core::openapi::{OpenApiConfig, build_openapi};
use caelix_core::{
    BadRequestException, BoxFuture, Container, HttpException, HttpResponse as CaelixHttpResponse,
    IntoCaelixResponse, Module, NotFoundException, PayloadTooLargeException, ResponseBody, Result,
    build_container, http_request_logging_enabled, log_application_started, log_http_request,
    log_http_request_info, log_listening, log_module_routes, register_module_controllers,
    shutdown_module,
};
use futures_util::StreamExt;

/// Public Caelix constant `DEFAULT_BODY_LIMIT_BYTES`.
pub const DEFAULT_BODY_LIMIT_BYTES: usize = 1024 * 1024;

/// Application-scoped multipart storage and limit configuration.
#[derive(Clone)]
pub(crate) struct UploadRuntimeConfig {
    #[cfg(feature = "uploads")]
    pub(crate) config: UploadConfig,
    pub(crate) body_limit: usize,
}

#[cfg(feature = "openapi")]
#[derive(Clone)]
pub(crate) struct OpenApiServices {
    /// The `config` value.
    pub config: OpenApiConfig,
    /// The `document` value.
    pub document: String,
}

#[cfg(not(feature = "openapi"))]
#[derive(Clone)]
pub(crate) struct OpenApiServices;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AccessLogFormat {
    Compact,
    Info,
}

/// Configures Actix runtime logging for an [`Application`].
///
/// `Logging::default()` enables Caelix's asynchronous HTTP access log.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Public Caelix type `Logging`.
pub struct Logging {
    access_log: bool,
    access_log_format: AccessLogFormat,
}

impl Default for Logging {
    fn default() -> Self {
        Self {
            access_log: true,
            access_log_format: AccessLogFormat::Compact,
        }
    }
}

impl Logging {
    /// Enables Actix-compatible detailed HTTP access logs.
    ///
    /// The output includes client address, request line and protocol, status,
    /// response size, referrer, user agent, and duration.
    pub fn info() -> Self {
        Self {
            access_log: true,
            access_log_format: AccessLogFormat::Info,
        }
    }

    /// Enables or disables HTTP access logging.
    pub fn access_log(mut self, enabled: bool) -> Self {
        self.access_log = enabled;
        self
    }

    /// Runs the `access_log_enabled` public API operation.
    pub fn access_log_enabled(&self) -> bool {
        self.access_log
    }

    fn access_log_format(&self) -> AccessLogFormat {
        self.access_log_format
    }
}

/// Runs the `to_actix_response` public API operation.
pub fn to_actix_response(response: CaelixHttpResponse) -> HttpResponse {
    // Caelix core uses http 1.x while Actix 4 still builds responses with http 0.2.
    let status = actix_web::http::StatusCode::from_u16(response.status.as_u16())
        .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR);

    let mut builder = HttpResponse::build(status);
    builder.content_type(response.content_type);
    for (name, value) in response.headers {
        builder.insert_header((name, value));
    }

    match response.body {
        ResponseBody::Buffered(bytes) => builder.body(bytes),
        ResponseBody::Streaming(stream) => {
            // Mid-stream errors cannot rewrite an already-sent status line.
            let stream = stream.map(|chunk| {
                chunk.map_err(|err| {
                    caelix_core::log_http_exception(&err);
                    actix_web::error::ErrorInternalServerError("Internal Server Error")
                })
            });
            builder.streaming(stream)
        }
    }
}

/// Public Caelix type `Application`.
pub struct Application {
    container: Arc<Container>,
    configure_fn: fn(&mut web::ServiceConfig),
    gateway_configure_fn: fn(&mut web::ServiceConfig, Arc<Container>, usize),
    shutdown_fn: for<'a> fn(&'a Container) -> BoxFuture<'a, caelix_core::Result<()>>,
    body_limit: usize,
    #[cfg(feature = "uploads")]
    upload_config: UploadConfig,
    websocket_max_message_size: usize,
    workers: usize,
    logging: Option<Logging>,
    openapi: Option<OpenApiServices>,
    #[cfg(feature = "openapi")]
    openapi_build_fn:
        fn(&OpenApiConfig) -> caelix_core::Result<caelix_core::openapi::utoipa::openapi::OpenApi>,
}

fn json_config(body_limit: usize) -> web::JsonConfig {
    web::JsonConfig::default()
        .limit(body_limit)
        .content_type_required(false)
        .error_handler(move |err, _req| {
            let exception = json_exception(&err, body_limit);
            let response = to_actix_response(exception.into_response());

            actix_web::error::InternalError::from_response(err, response).into()
        })
}

fn json_exception(err: &JsonPayloadError, body_limit: usize) -> HttpException {
    if matches!(
        err,
        JsonPayloadError::Overflow { .. } | JsonPayloadError::OverflowKnownLength { .. }
    ) {
        return PayloadTooLargeException::new(format!(
            "request body exceeds the configured limit of {body_limit} bytes"
        ));
    }

    if let JsonPayloadError::Deserialize(source) = err {
        if let Some(exception) = missing_field_exception(&source.to_string()) {
            return exception;
        }
    }

    BadRequestException::new("invalid JSON request body")
}

fn path_config() -> web::PathConfig {
    web::PathConfig::default().error_handler(|err: PathError, _req| {
        let exception = missing_field_exception(&err.to_string())
            .unwrap_or_else(|| BadRequestException::new(err.to_string()));
        let response = to_actix_response(exception.into_response());

        actix_web::error::InternalError::from_response(err, response).into()
    })
}

fn query_config() -> web::QueryConfig {
    web::QueryConfig::default().error_handler(|err: QueryPayloadError, _req| {
        let exception = missing_field_exception(&err.to_string())
            .unwrap_or_else(|| BadRequestException::new(err.to_string()));
        let response = to_actix_response(exception.into_response());

        actix_web::error::InternalError::from_response(err, response).into()
    })
}

fn missing_field_exception(message: &str) -> Option<HttpException> {
    let field = missing_field_name(message)?;
    let mut errors = BTreeMap::new();
    errors.insert(field, vec!["is required".to_string()]);

    Some(BadRequestException::new("Validation failed").with_errors(errors))
}

fn missing_field_name(message: &str) -> Option<String> {
    let start = message.find("missing field `")? + "missing field `".len();
    let rest = &message[start..];
    let end = rest.find('`')?;
    let field = &rest[..end];

    if field.is_empty() {
        None
    } else {
        Some(field.to_string())
    }
}

async fn not_found(req: HttpRequest) -> HttpResponse {
    to_actix_response(
        NotFoundException::new(format!("Cannot {} {}", req.method(), req.path())).into_response(),
    )
}

fn log_access_request<B: MessageBody>(
    format: AccessLogFormat,
    response: &ServiceResponse<B>,
    elapsed: std::time::Duration,
) {
    let request = response.request();

    match format {
        AccessLogFormat::Compact => log_http_request(
            request.method().as_str(),
            request.path(),
            response.status().as_u16(),
            elapsed,
        ),
        AccessLogFormat::Info => {
            let path_and_query = if request.query_string().is_empty() {
                request.path().to_string()
            } else {
                format!("{}?{}", request.path(), request.query_string())
            };
            let response_size = match response.response().body().size() {
                BodySize::None => Some(0),
                BodySize::Sized(size) => Some(size),
                BodySize::Stream => None,
            };

            log_http_request_info(
                request.connection_info().peer_addr().unwrap_or("-"),
                request.method().as_str(),
                &path_and_query,
                &format!("{:?}", request.version()),
                response.status().as_u16(),
                response_size,
                request_header(request, &header::REFERER).as_str(),
                request_header(request, &header::USER_AGENT).as_str(),
                elapsed,
            );
        }
    }
}

fn request_header(request: &HttpRequest, name: &header::HeaderName) -> String {
    request
        .headers()
        .get(name)
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
        .unwrap_or_else(|| "-".to_string())
}

pub(crate) fn configure_caelix_services(
    cfg: &mut web::ServiceConfig,
    body_limit: usize,
    #[cfg(feature = "uploads")] upload_config: UploadConfig,
    configure_fn: fn(&mut web::ServiceConfig),
    openapi: Option<&OpenApiServices>,
) {
    cfg.app_data(json_config(body_limit));
    cfg.app_data(web::Data::new(UploadRuntimeConfig {
        #[cfg(feature = "uploads")]
        config: upload_config,
        body_limit,
    }));
    cfg.app_data(path_config());
    cfg.app_data(query_config());
    configure_fn(cfg);
    #[cfg(feature = "openapi")]
    if let Some(openapi) = openapi {
        let ui_base = openapi.config.ui_path.trim_end_matches('/');
        let ui_redirect = format!("{ui_base}/");
        let document = openapi.document.clone();
        cfg.route(
            &openapi.config.json_path,
            web::get().to(move || {
                let document = document.clone();
                async move {
                    HttpResponse::Ok()
                        .content_type("application/json")
                        .body(document)
                }
            }),
        );
        let html = swagger_ui_html(&openapi.config.json_path);
        cfg.route(
            &format!("{ui_base}/"),
            web::get().to(move || {
                let html = html.clone();
                async move {
                    HttpResponse::Ok()
                        .content_type("text/html; charset=utf-8")
                        .body(html)
                }
            }),
        );
        cfg.route(
            ui_base,
            web::get().to(move || {
                let ui_redirect = ui_redirect.clone();
                async move {
                    HttpResponse::TemporaryRedirect()
                        .insert_header((header::LOCATION, ui_redirect))
                        .finish()
                }
            }),
        );
    }
    #[cfg(not(feature = "openapi"))]
    let _ = openapi;
    cfg.default_service(web::route().to(not_found));
}

#[cfg(feature = "openapi")]
fn swagger_ui_html(json_path: &str) -> String {
    let json_path = serde_json::to_string(json_path).expect("OpenAPI path must serialize");
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>Swagger UI</title><link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css"></head><body><div id="swagger-ui"></div><script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script><script>SwaggerUIBundle({{url:{json_path},dom_id:'#swagger-ui'}});</script></body></html>"#
    )
}

impl Application {
    /// Runs the `new` public API operation.
    pub async fn new<M: Module + 'static>() -> Result<Self> {
        let start = Instant::now();
        let container = build_container::<M>().await?;
        log_module_routes::<M>();
        log_application_started(start.elapsed());

        Ok(Self {
            container: Arc::new(container),
            configure_fn: |cfg| register_module_controllers::<M>(cfg),
            gateway_configure_fn: |cfg, container, max| {
                crate::websocket::configure_gateway_routes::<M>(cfg, container, max)
            },
            shutdown_fn: |container| Box::pin(async move { shutdown_module::<M>(container).await }),
            body_limit: DEFAULT_BODY_LIMIT_BYTES,
            #[cfg(feature = "uploads")]
            upload_config: UploadConfig::default(),
            websocket_max_message_size: crate::websocket::DEFAULT_WEBSOCKET_MAX_MESSAGE_SIZE,
            workers: num_cpus::get(),
            logging: None,
            openapi: None,
            #[cfg(feature = "openapi")]
            openapi_build_fn: |config| build_openapi::<M>(config),
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

    /// Runs the `websocket_max_message_size` public API operation.
    pub fn websocket_max_message_size(mut self, bytes: usize) -> Self {
        self.websocket_max_message_size = bytes.max(1);
        self
    }

    /// Runs the `workers` public API operation.
    pub fn workers(mut self, workers: usize) -> Self {
        self.workers = workers.max(1);
        self
    }

    /// Configures runtime logging for this application.
    ///
    /// An explicit configuration takes precedence over `CAELIX_HTTP_LOG` and
    /// `CAELIX_ACCESS_LOG`. When omitted, those environment variables remain
    /// supported for backwards compatibility.
    pub fn logging(mut self, logging: Logging) -> Self {
        self.logging = Some(logging);
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

    #[cfg(test)]
    fn configure_services(&self, cfg: &mut web::ServiceConfig) {
        configure_caelix_services(
            cfg,
            self.body_limit,
            #[cfg(feature = "uploads")]
            self.upload_config.clone(),
            self.configure_fn,
            self.openapi.as_ref(),
        );
    }

    async fn shutdown(&self) -> caelix_core::Result<()> {
        (self.shutdown_fn)(&self.container).await
    }

    fn prepare_doctor_runtime(&self) {
        let container = self.container.clone();
        let configure_fn = self.configure_fn;
        let body_limit = self.body_limit;
        #[cfg(feature = "uploads")]
        let upload_config = self.upload_config.clone();
        let websocket_max_message_size = self.websocket_max_message_size;
        let gateway_configure_fn = self.gateway_configure_fn;
        let openapi = self.openapi.clone();

        let _app = App::new()
            .app_data(web::Data::from(container.clone()))
            .configure({
                move |cfg| {
                    configure_caelix_services(
                        cfg,
                        body_limit,
                        #[cfg(feature = "uploads")]
                        upload_config.clone(),
                        configure_fn,
                        openapi.as_ref(),
                    )
                }
            })
            .configure(move |cfg| {
                gateway_configure_fn(cfg, container.clone(), websocket_max_message_size)
            });
    }

    /// Runs the `listen` public API operation.
    pub async fn listen(self, addr: &str) -> std::io::Result<()> {
        self.listen_with_doctor_mode(addr, has_doctor_argument(std::env::args_os()))
            .await
    }

    async fn listen_with_doctor_mode(self, addr: &str, doctor_mode: bool) -> std::io::Result<()> {
        if doctor_mode {
            self.prepare_doctor_runtime();
            return self.shutdown().await.map_err(to_io_error);
        }

        let container = self.container.clone();
        let configure_fn = self.configure_fn;
        let body_limit = self.body_limit;
        #[cfg(feature = "uploads")]
        let upload_config = self.upload_config.clone();
        let websocket_max_message_size = self.websocket_max_message_size;
        let gateway_configure_fn = self.gateway_configure_fn;
        let workers = self.workers;
        let addr = addr.to_string();
        let logging = self.logging.unwrap_or(Logging {
            access_log: http_request_logging_enabled(),
            access_log_format: AccessLogFormat::Compact,
        });
        let openapi = self.openapi.clone();

        log_listening(&addr);

        let result = if logging.access_log_enabled() {
            let logging_container = container.clone();
            let openapi_with_logging = openapi.clone();
            let access_log_format = logging.access_log_format();
            let server = match HttpServer::new(move || {
                App::new()
                    .app_data(web::Data::from(logging_container.clone()))
                    .wrap_fn(move |req, service| {
                        let request_log_start = Instant::now();
                        let future = service.call(req);

                        async move {
                            let response = future.await?;
                            log_access_request(
                                access_log_format,
                                &response,
                                request_log_start.elapsed(),
                            );
                            Ok(response)
                        }
                    })
                    .configure({
                        let openapi = openapi_with_logging.clone();
                        #[cfg(feature = "uploads")]
                        let upload_config = upload_config.clone();
                        move |cfg| {
                            configure_caelix_services(
                                cfg,
                                body_limit,
                                #[cfg(feature = "uploads")]
                                upload_config.clone(),
                                configure_fn,
                                openapi.as_ref(),
                            )
                        }
                    })
                    .configure({
                        let container = logging_container.clone();
                        move |cfg| {
                            gateway_configure_fn(cfg, container.clone(), websocket_max_message_size)
                        }
                    })
            })
            .workers(workers)
            .bind(addr.as_str())
            {
                Ok(server) => server.run(),
                Err(err) => {
                    let _ = self.shutdown().await;
                    return Err(err);
                }
            };

            server.await
        } else {
            let server = match HttpServer::new(move || {
                App::new()
                    .app_data(web::Data::from(container.clone()))
                    .configure({
                        let openapi = openapi.clone();
                        #[cfg(feature = "uploads")]
                        let upload_config = upload_config.clone();
                        move |cfg| {
                            configure_caelix_services(
                                cfg,
                                body_limit,
                                #[cfg(feature = "uploads")]
                                upload_config.clone(),
                                configure_fn,
                                openapi.as_ref(),
                            )
                        }
                    })
                    .configure({
                        let container = container.clone();
                        move |cfg| {
                            gateway_configure_fn(cfg, container.clone(), websocket_max_message_size)
                        }
                    })
            })
            .workers(workers)
            .bind(addr.as_str())
            {
                Ok(server) => server.run(),
                Err(err) => {
                    let _ = self.shutdown().await;
                    return Err(err);
                }
            };

            server.await
        };

        self.shutdown().await.map_err(to_io_error)?;
        result
    }
}

fn has_doctor_argument<I>(args: I) -> bool
where
    I: IntoIterator<Item = OsString>,
{
    args.into_iter().any(|arg| arg == OsStr::new("--doctor"))
}

fn to_io_error(err: caelix_core::HttpException) -> std::io::Error {
    std::io::Error::other(err.message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{http::StatusCode, test as actix_test};
    use caelix_core::{Controller, Injectable, ModuleMetadata};
    use serde::Deserialize;
    use serde_json::{Value, json};
    use std::{
        any::Any,
        sync::atomic::{AtomicUsize, Ordering},
    };
    use uuid::Uuid;

    static SHUTDOWN_COUNT: AtomicUsize = AtomicUsize::new(0);
    static DOCTOR_CONSTRUCTION_COUNT: AtomicUsize = AtomicUsize::new(0);
    static DOCTOR_INIT_COUNT: AtomicUsize = AtomicUsize::new(0);
    static DOCTOR_STARTUP_COUNT: AtomicUsize = AtomicUsize::new(0);
    static DOCTOR_SHUTDOWN_COUNT: AtomicUsize = AtomicUsize::new(0);
    static DOCTOR_ROUTE_CONFIG_COUNT: AtomicUsize = AtomicUsize::new(0);

    struct HealthService {
        status: &'static str,
    }

    impl Injectable for HealthService {
        fn dependencies() -> Vec<caelix_core::ProviderDependency> {
            caelix_core::provider_dependencies![]
        }

        fn create(_container: &Container) -> caelix_core::BoxFuture<'_, caelix_core::Result<Self>> {
            Box::pin(async move { Ok(Self { status: "ok" }) })
        }
    }

    struct TestModule;

    impl Module for TestModule {
        fn register() -> ModuleMetadata {
            ModuleMetadata::new().provider::<HealthService>()
        }
    }

    struct JsonController;

    impl Injectable for JsonController {
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
    }

    impl JsonController {
        async fn accept_json(_payload: web::Json<Value>) -> HttpResponse {
            HttpResponse::Ok().finish()
        }
    }

    impl Controller for JsonController {
        fn base_path() -> &'static str {
            "/json"
        }

        fn register_routes(cfg_any: &mut dyn Any) {
            let cfg = cfg_any
                .downcast_mut::<web::ServiceConfig>()
                .expect("expected actix ServiceConfig");

            cfg.route("/json", web::post().to(Self::accept_json));
        }
    }

    struct JsonModule;

    impl Module for JsonModule {
        fn register() -> ModuleMetadata {
            ModuleMetadata::new().controller::<JsonController>()
        }
    }

    #[derive(Deserialize)]
    struct SearchQuery {
        limit: usize,
    }

    #[derive(Deserialize)]
    struct RequiredBody {
        name: String,
    }

    #[derive(Deserialize)]
    struct RequiredQuery {
        q: String,
    }

    #[derive(Deserialize)]
    struct RequiredPath {
        org_id: Uuid,
        user_id: Uuid,
    }

    async fn accept_uuid(_id: web::Path<Uuid>) -> HttpResponse {
        HttpResponse::Ok().finish()
    }

    async fn accept_required_body(body: web::Json<RequiredBody>) -> HttpResponse {
        let body = body.into_inner();
        let _ = body.name;

        HttpResponse::Ok().finish()
    }

    async fn accept_query(query: web::Query<SearchQuery>) -> HttpResponse {
        let query = query.into_inner();
        let _ = query.limit;

        HttpResponse::Ok().finish()
    }

    async fn accept_required_query(query: web::Query<RequiredQuery>) -> HttpResponse {
        let query = query.into_inner();
        let _ = query.q;

        HttpResponse::Ok().finish()
    }

    async fn accept_required_path(path: web::Path<RequiredPath>) -> HttpResponse {
        let path = path.into_inner();
        let _ = (path.org_id, path.user_id);

        HttpResponse::Ok().finish()
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

    struct DoctorService;

    impl Injectable for DoctorService {
        fn dependencies() -> Vec<caelix_core::ProviderDependency> {
            caelix_core::provider_dependencies![]
        }

        fn create(_container: &Container) -> caelix_core::BoxFuture<'_, caelix_core::Result<Self>> {
            Box::pin(async move { Ok(Self) })
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

        fn register_routes(cfg_any: &mut dyn Any) {
            DOCTOR_ROUTE_CONFIG_COUNT.fetch_add(1, Ordering::SeqCst);
            let cfg = cfg_any
                .downcast_mut::<web::ServiceConfig>()
                .expect("expected actix ServiceConfig");
            cfg.route(
                "/doctor",
                web::get().to(|| async { HttpResponse::Ok().finish() }),
            );
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

    #[actix_web::test]
    async fn new_builds_container_from_module_metadata() {
        let app = Application::new::<TestModule>().await.unwrap();

        let service = app.container.resolve::<HealthService>().unwrap();

        assert_eq!(service.status, "ok");
    }

    #[actix_web::test]
    async fn application_accepts_explicit_logging_configuration() {
        let app = Application::new::<TestModule>()
            .await
            .unwrap()
            .logging(Logging::default().access_log(false));

        assert_eq!(app.logging, Some(Logging::default().access_log(false)));
        assert!(!Logging::default().access_log(false).access_log_enabled());
        assert!(Logging::default().access_log_enabled());
        assert_eq!(Logging::info().access_log_format(), AccessLogFormat::Info);
    }

    #[actix_web::test]
    async fn json_body_limit_rejects_large_payloads_with_json_error() {
        async fn accept_json(_payload: web::Json<Value>) -> HttpResponse {
            HttpResponse::Ok().finish()
        }

        let app = actix_test::init_service(
            App::new()
                .app_data(json_config(8))
                .route("/json", web::post().to(accept_json)),
        )
        .await;

        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::post()
                .uri("/json")
                .insert_header(("content-type", "application/json"))
                .set_payload(r#"{"too":"large"}"#)
                .to_request(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body: Value = actix_test::read_body_json(response).await;
        assert_eq!(
            body,
            json!({
                "status": 413,
                "error": "Payload Too Large",
                "message": "request body exceeds the configured limit of 8 bytes"
            })
        );
    }

    #[actix_web::test]
    async fn json_config_accepts_json_without_content_type_header() {
        async fn accept_json(_payload: web::Json<Value>) -> HttpResponse {
            HttpResponse::Ok().finish()
        }

        let app = actix_test::init_service(
            App::new()
                .app_data(json_config(DEFAULT_BODY_LIMIT_BYTES))
                .route("/json", web::post().to(accept_json)),
        )
        .await;

        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::post()
                .uri("/json")
                .set_payload("{}")
                .to_request(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn json_missing_field_errors_are_validation_shaped() {
        let app = actix_test::init_service(
            App::new()
                .app_data(json_config(DEFAULT_BODY_LIMIT_BYTES))
                .route("/json", web::patch().to(accept_required_body)),
        )
        .await;

        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::patch()
                .uri("/json")
                .insert_header(("content-type", "application/json"))
                .set_payload("{}")
                .to_request(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = actix_test::read_body_json(response).await;
        assert_eq!(
            body,
            json!({
                "status": 400,
                "error": "Bad Request",
                "message": "Validation failed",
                "errors": {
                    "name": ["is required"]
                }
            })
        );
    }

    #[actix_web::test]
    async fn application_enforces_configured_body_limit() {
        let application = Application::new::<JsonModule>()
            .await
            .unwrap()
            .body_limit(8);
        let app = actix_test::init_service(
            App::new()
                .app_data(web::Data::from(application.container.clone()))
                .configure(|cfg| application.configure_services(cfg)),
        )
        .await;

        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::post()
                .uri("/json")
                .insert_header(("content-type", "application/json"))
                .set_payload(r#"{"too":"large"}"#)
                .to_request(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body: Value = actix_test::read_body_json(response).await;
        assert_eq!(
            body,
            json!({
                "status": 413,
                "error": "Payload Too Large",
                "message": "request body exceeds the configured limit of 8 bytes"
            })
        );
    }

    #[actix_web::test]
    async fn path_extractor_errors_are_caelix_json_errors() {
        let app = actix_test::init_service(
            App::new()
                .app_data(path_config())
                .route("/users/{id}", web::get().to(accept_uuid)),
        )
        .await;

        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::get().uri("/users/1").to_request(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = actix_test::read_body_json(response).await;
        assert_eq!(body["status"], 400);
        assert_eq!(body["error"], "Bad Request");
        assert!(
            body["message"]
                .as_str()
                .is_some_and(|message| message.contains("UUID parsing failed"))
        );
    }

    #[actix_web::test]
    async fn path_missing_field_errors_are_validation_shaped() {
        let app = actix_test::init_service(App::new().app_data(path_config()).route(
            "/orgs/{org_id}/users/{user}",
            web::get().to(accept_required_path),
        ))
        .await;

        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::get()
                .uri("/orgs/550e8400-e29b-41d4-a716-446655440000/users/550e8400-e29b-41d4-a716-446655440000")
                .to_request(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = actix_test::read_body_json(response).await;
        assert_eq!(
            body,
            json!({
                "status": 400,
                "error": "Bad Request",
                "message": "Validation failed",
                "errors": {
                    "user_id": ["is required"]
                }
            })
        );
    }

    #[actix_web::test]
    async fn query_extractor_errors_are_caelix_json_errors() {
        let app = actix_test::init_service(
            App::new()
                .app_data(query_config())
                .route("/users", web::get().to(accept_query)),
        )
        .await;

        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::get()
                .uri("/users?limit=abc")
                .to_request(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = actix_test::read_body_json(response).await;
        assert_eq!(body["status"], 400);
        assert_eq!(body["error"], "Bad Request");
        assert!(
            body["message"]
                .as_str()
                .is_some_and(|message| message.contains("invalid digit"))
        );
    }

    #[actix_web::test]
    async fn query_missing_field_errors_are_validation_shaped() {
        let app = actix_test::init_service(
            App::new()
                .app_data(query_config())
                .route("/users", web::get().to(accept_required_query)),
        )
        .await;

        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::get().uri("/users").to_request(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = actix_test::read_body_json(response).await;
        assert_eq!(
            body,
            json!({
                "status": 400,
                "error": "Bad Request",
                "message": "Validation failed",
                "errors": {
                    "q": ["is required"]
                }
            })
        );
    }

    #[actix_web::test]
    async fn unmatched_routes_are_caelix_json_errors() {
        let app = actix_test::init_service(App::new().configure(|cfg| {
            configure_caelix_services(
                cfg,
                DEFAULT_BODY_LIMIT_BYTES,
                #[cfg(feature = "uploads")]
                UploadConfig::default(),
                |_| {},
                None,
            )
        }))
        .await;

        let response = actix_test::call_service(
            &app,
            actix_test::TestRequest::get().uri("/missing").to_request(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body: Value = actix_test::read_body_json(response).await;
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
    async fn application_runs_module_shutdown_hook() {
        SHUTDOWN_COUNT.store(0, Ordering::SeqCst);

        let application = Application::new::<ShutdownModule>().await.unwrap();
        application.shutdown().await.unwrap();

        assert_eq!(SHUTDOWN_COUNT.load(Ordering::SeqCst), 1);
    }

    #[actix_web::test]
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

    #[actix_web::test]
    async fn doctor_mode_propagates_shutdown_failures() {
        let error = Application::new::<FailingShutdownModule>()
            .await
            .unwrap()
            .listen_with_doctor_mode("not a socket address", true)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("shutdown failed"));
    }

    #[actix_web::test]
    async fn normal_listen_still_attempts_to_bind_the_configured_address() {
        let error = Application::new::<TestModule>()
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

    #[actix_web::test]
    async fn to_actix_response_streams_chunked_body() {
        use actix_web::body::to_bytes;
        use caelix_core::{Bytes, Response};

        let stream = futures_util::stream::iter(vec![
            Ok::<_, caelix_core::HttpException>(Bytes::from_static(b"chunk-a-")),
            Ok(Bytes::from_static(b"chunk-b")),
        ]);
        let caelix = Response::stream("text/plain", stream);
        let actix_response = to_actix_response(caelix);

        assert_eq!(actix_response.status(), StatusCode::OK);
        assert_eq!(
            actix_response
                .headers()
                .get(actix_web::http::header::CONTENT_TYPE)
                .unwrap(),
            "text/plain"
        );

        let body = to_bytes(actix_response.into_body()).await.unwrap();
        assert_eq!(&body[..], b"chunk-a-chunk-b");
    }

    #[actix_web::test]
    async fn to_actix_response_applies_sse_headers() {
        use caelix_core::Response;

        let stream = futures_util::stream::iter(Vec::<
            std::result::Result<serde_json::Value, caelix_core::HttpException>,
        >::new());
        let actix_response = to_actix_response(Response::sse(stream));

        assert_eq!(
            actix_response
                .headers()
                .get(actix_web::http::header::CONTENT_TYPE)
                .unwrap(),
            "text/event-stream"
        );
        assert_eq!(
            actix_response.headers().get("cache-control").unwrap(),
            "no-cache"
        );
        assert_eq!(
            actix_response.headers().get("x-accel-buffering").unwrap(),
            "no"
        );
    }
}
