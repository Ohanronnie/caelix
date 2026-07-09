use std::{collections::BTreeMap, sync::Arc, time::Instant};

use actix_web::{
    App, HttpRequest, HttpResponse, HttpServer,
    dev::Service,
    error::{JsonPayloadError, PathError, QueryPayloadError},
    web,
};
use caelix_core::{
    BadRequestException, BoxFuture, Container, HttpException, HttpResponse as CaelixHttpResponse,
    IntoCaelixResponse, Module, NotFoundException, PayloadTooLargeException, ResponseBody,
    build_container, http_request_logging_enabled, log_application_started, log_http_request,
    log_listening, log_module_routes, register_module_controllers, shutdown_module,
};
use futures_util::StreamExt;

pub const DEFAULT_BODY_LIMIT_BYTES: usize = 1024 * 1024;

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

pub struct Application {
    container: Arc<Container>,
    configure_fn: fn(&mut web::ServiceConfig),
    shutdown_fn: for<'a> fn(&'a Container) -> BoxFuture<'a, caelix_core::Result<()>>,
    body_limit: usize,
    workers: usize,
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

pub(crate) fn configure_caelix_services(
    cfg: &mut web::ServiceConfig,
    body_limit: usize,
    configure_fn: fn(&mut web::ServiceConfig),
) {
    cfg.app_data(json_config(body_limit));
    cfg.app_data(path_config());
    cfg.app_data(query_config());
    configure_fn(cfg);
    cfg.default_service(web::route().to(not_found));
}

impl Application {
    pub async fn new<M: Module + 'static>() -> caelix_core::Result<Self> {
        let start = Instant::now();
        let container = build_container::<M>().await?;
        log_module_routes::<M>();
        log_application_started(start.elapsed());

        Ok(Self {
            container: Arc::new(container),
            configure_fn: |cfg| register_module_controllers::<M>(cfg),
            shutdown_fn: |container| Box::pin(async move { shutdown_module::<M>(container).await }),
            body_limit: DEFAULT_BODY_LIMIT_BYTES,
            workers: num_cpus::get(),
        })
    }

    pub fn body_limit(mut self, bytes: usize) -> Self {
        self.body_limit = bytes;
        self
    }

    pub fn workers(mut self, workers: usize) -> Self {
        self.workers = workers.max(1);
        self
    }

    #[cfg(test)]
    fn configure_services(&self, cfg: &mut web::ServiceConfig) {
        configure_caelix_services(cfg, self.body_limit, self.configure_fn);
    }

    async fn shutdown(&self) -> caelix_core::Result<()> {
        (self.shutdown_fn)(&self.container).await
    }

    pub async fn listen(self, addr: &str) -> std::io::Result<()> {
        let container = self.container.clone();
        let configure_fn = self.configure_fn;
        let body_limit = self.body_limit;
        let workers = self.workers;
        let addr = addr.to_string();

        log_listening(&addr);

        let server = match HttpServer::new(move || {
            App::new()
                .app_data(web::Data::from(container.clone()))
                .wrap_fn(move |req, service| {
                    let request_log_start = http_request_logging_enabled().then(Instant::now);
                    let future = service.call(req);

                    async move {
                        let response = future.await?;
                        if let Some(start) = request_log_start {
                            let status = response.status().as_u16();
                            log_http_request(
                                response.request().method().as_str(),
                                response.request().path(),
                                status,
                                start.elapsed(),
                            );
                        }
                        Ok(response)
                    }
                })
                .configure(move |cfg| configure_caelix_services(cfg, body_limit, configure_fn))
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

        let result = server.await;
        self.shutdown().await.map_err(to_io_error)?;
        result
    }
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

    struct HealthService {
        status: &'static str,
    }

    impl Injectable for HealthService {
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
        fn create(_container: &Container) -> caelix_core::BoxFuture<'_, caelix_core::Result<Self>> {
            Box::pin(async move { Ok(Self) })
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
    async fn new_builds_container_from_module_metadata() {
        let app = Application::new::<TestModule>().await.unwrap();

        let service = app.container.resolve::<HealthService>().unwrap();

        assert_eq!(service.status, "ok");
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
        let app = actix_test::init_service(
            App::new()
                .configure(|cfg| configure_caelix_services(cfg, DEFAULT_BODY_LIMIT_BYTES, |_| {})),
        )
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
