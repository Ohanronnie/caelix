use std::{sync::Arc, time::Instant};

use actix_web::{App, HttpResponse, HttpServer, dev::Service, error::JsonPayloadError, web};
use caelix_core::{
    BadRequestException, Container, IntoCaelixResponse, Module, PayloadTooLargeException,
    build_container, log_application_started, log_http_request, log_listening, log_module_routes,
    register_module_controllers,
};

pub const DEFAULT_BODY_LIMIT_BYTES: usize = 1024 * 1024;

pub struct Application {
    container: Arc<Container>,
    configure_fn: fn(&mut web::ServiceConfig),
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
            let response = exception.into_response();
            let status = actix_web::http::StatusCode::from_u16(response.status.as_u16()).unwrap();
            let response = HttpResponse::build(status)
                .content_type(response.content_type)
                .body(response.body);

            actix_web::error::InternalError::from_response(err, response).into()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{http::StatusCode, test as actix_test};
    use caelix_core::{Injectable, ModuleMetadata};
    use serde_json::{Value, json};

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
            body_limit: DEFAULT_BODY_LIMIT_BYTES,
        }
    }

    pub fn body_limit(mut self, bytes: usize) -> Self {
        self.body_limit = bytes;
        self
    }

    pub async fn listen(self, addr: &str) -> std::io::Result<()> {
        let container = self.container.clone();
        let configure_fn = self.configure_fn;
        let body_limit = self.body_limit;
        let addr = addr.to_string();

        log_listening(&addr);

        HttpServer::new(move || {
            App::new()
                .app_data(web::Data::new(container.clone()))
                .app_data(json_config(body_limit))
                .wrap_fn(|req, service| {
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
                .configure(configure_fn)
        })
        .bind(addr.as_str())?
        .run()
        .await
    }
}
