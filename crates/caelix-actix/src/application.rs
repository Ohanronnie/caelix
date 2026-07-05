use std::{sync::Arc, time::Instant};

use actix_web::{App, HttpServer, dev::Service, web};
use caelix_core::{
    Container, Module, build_container, log_application_started, log_http_request, log_listening,
    log_module_routes, register_module_controllers,
};

pub struct Application {
    container: Arc<Container>,
    configure_fn: fn(&mut web::ServiceConfig),
}

#[cfg(test)]
mod tests {
    use super::*;
    use caelix_core::{Injectable, ModuleMetadata};

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
        }
    }

    pub async fn listen(self, addr: &str) -> std::io::Result<()> {
        let container = self.container.clone();
        let configure_fn = self.configure_fn;
        let addr = addr.to_string();

        log_listening(&addr);

        HttpServer::new(move || {
            App::new()
                .app_data(web::Data::new(container.clone()))
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
