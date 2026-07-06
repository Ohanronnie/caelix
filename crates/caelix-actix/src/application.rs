use std::{sync::Arc, time::Instant};

use actix_web::{App, HttpResponse, HttpServer, dev::Service, error::JsonPayloadError, web};
use caelix_core::{
    BadRequestException, BoxFuture, Container, HttpResponse as CaelixHttpResponse,
    IntoCaelixResponse, Module, PayloadTooLargeException, build_container, log_application_started,
    log_http_request, log_listening, log_module_routes, register_module_controllers,
    shutdown_module,
};

pub const DEFAULT_BODY_LIMIT_BYTES: usize = 1024 * 1024;

pub fn to_actix_response(response: CaelixHttpResponse) -> HttpResponse {
    // Caelix core uses http 1.x while Actix 4 still builds responses with http 0.2.
    let status = actix_web::http::StatusCode::from_u16(response.status.as_u16())
        .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR);

    HttpResponse::build(status)
        .content_type(response.content_type)
        .body(response.body)
}

pub struct Application {
    container: Arc<Container>,
    configure_fn: fn(&mut web::ServiceConfig),
    shutdown_fn: for<'a> fn(&'a Container) -> BoxFuture<'a, ()>,
    body_limit: usize,
}

fn json_config(body_limit: usize) -> web::JsonConfig {
    web::JsonConfig::default()
        .limit(body_limit)
        .error_handler(move |err, _req| {
            let exception = if matches!(
                &err,
                JsonPayloadError::Overflow { .. } | JsonPayloadError::OverflowKnownLength { .. }
            ) {
                PayloadTooLargeException::new(format!(
                    "request body exceeds the configured limit of {body_limit} bytes"
                ))
            } else {
                BadRequestException::new("invalid JSON request body")
            };
            let response = to_actix_response(exception.into_response());

            actix_web::error::InternalError::from_response(err, response).into()
        })
}

fn configure_caelix_services(
    cfg: &mut web::ServiceConfig,
    body_limit: usize,
    configure_fn: fn(&mut web::ServiceConfig),
) {
    cfg.app_data(json_config(body_limit));
    configure_fn(cfg);
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{http::StatusCode, test as actix_test};
    use caelix_core::{Controller, Injectable, ModuleMetadata};
    use serde_json::{Value, json};
    use std::{
        any::Any,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static SHUTDOWN_COUNT: AtomicUsize = AtomicUsize::new(0);

    struct HealthService {
        status: &'static str,
    }

    impl Injectable for HealthService {
        fn create(_container: &Container) -> caelix_core::BoxFuture<'_, Self> {
            Box::pin(async move { Self { status: "ok" } })
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
        fn create(_container: &Container) -> caelix_core::BoxFuture<'_, Self> {
            Box::pin(async move { Self })
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

    struct ShutdownService;

    impl Injectable for ShutdownService {
        fn create(_container: &Container) -> caelix_core::BoxFuture<'_, Self> {
            Box::pin(async move { Self })
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
        let app = Application::new::<TestModule>().await;

        let service = app.container.resolve::<HealthService>();

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
    async fn application_enforces_configured_body_limit() {
        let application = Application::new::<JsonModule>().await.body_limit(8);
        let app = actix_test::init_service(
            App::new()
                .app_data(web::Data::new(application.container.clone()))
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
    async fn application_runs_module_shutdown_hook() {
        SHUTDOWN_COUNT.store(0, Ordering::SeqCst);

        let application = Application::new::<ShutdownModule>().await;
        application.shutdown().await;

        assert_eq!(SHUTDOWN_COUNT.load(Ordering::SeqCst), 1);
    }
}

impl Application {
    pub async fn new<M: Module + 'static>() -> Self {
        let start = Instant::now();
        let container = build_container::<M>().await;
        log_module_routes::<M>();
        log_application_started(start.elapsed());

        Self {
            container: Arc::new(container),
            configure_fn: |cfg| register_module_controllers::<M>(cfg),
            shutdown_fn: |container| Box::pin(async move { shutdown_module::<M>(container).await }),
            body_limit: DEFAULT_BODY_LIMIT_BYTES,
        }
    }

    pub fn body_limit(mut self, bytes: usize) -> Self {
        self.body_limit = bytes;
        self
    }

    #[cfg(test)]
    fn configure_services(&self, cfg: &mut web::ServiceConfig) {
        configure_caelix_services(cfg, self.body_limit, self.configure_fn);
    }

    async fn shutdown(&self) {
        (self.shutdown_fn)(&self.container).await;
    }

    pub async fn listen(self, addr: &str) -> std::io::Result<()> {
        let container = self.container.clone();
        let configure_fn = self.configure_fn;
        let body_limit = self.body_limit;
        let addr = addr.to_string();

        log_listening(&addr);

        let server = match HttpServer::new(move || {
            App::new()
                .app_data(web::Data::new(container.clone()))
                .wrap_fn(move |req, service| {
                    let method = req.method().to_string();
                    let path = req.path().to_string();
                    let start = Instant::now();
                    let future = service.call(req);

                    async move {
                        let response = future.await?;
                        let status = response.status().as_u16();
                        log_http_request(&method, &path, status, start.elapsed());
                        Ok(response)
                    }
                })
                .configure(move |cfg| configure_caelix_services(cfg, body_limit, configure_fn))
        })
        .bind(addr.as_str())
        {
            Ok(server) => server.run(),
            Err(err) => {
                self.shutdown().await;
                return Err(err);
            }
        };

        let result = server.await;
        self.shutdown().await;
        result
    }
}
