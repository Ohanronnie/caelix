#![cfg(feature = "actix")]

use caelix::{Application, Module, ModuleMetadata};

struct EmptyModule;

impl Module for EmptyModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
    }
}

#[caelix::main]
async fn runtime_entrypoint() -> std::io::Result<()> {
    Ok(())
}

#[test]
fn actix_feature_reexports_runtime_macro() {
    runtime_entrypoint().unwrap();
}

#[caelix::test]
async fn actix_feature_exposes_native_application_factory() {
    let error = Application::new::<EmptyModule>()
        .await
        .unwrap()
        .listen_with_app("127.0.0.1:not-a-port", caelix::actix_web::App::new)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("invalid port value"));
}
