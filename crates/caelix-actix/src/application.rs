use std::sync::Arc;

use actix_web::{App, HttpServer, web};
use caelix_core::{Container, Module, build_container, register_module_controllers};

pub struct Application {
    container: Arc<Container>,
    configure_fn: fn(&mut web::ServiceConfig),
}

impl Application {
    pub fn new<M: Module + 'static>() -> Self {
        let container = build_container::<M>();
        Self {
            container: Arc::new(container),
            configure_fn: |cfg| register_module_controllers::<M>(cfg),
        }
    }

    pub async fn listen(self, addr: &str) -> std::io::Result<()> {
        let container = self.container.clone();
        let configure_fn = self.configure_fn;

        HttpServer::new(move || {
            App::new()
                .app_data(web::Data::new(container.clone()))
                .configure(configure_fn)
        })
        .bind(addr)?
        .run()
        .await
    }
}
