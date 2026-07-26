//! Run with `cargo run -p caelix --example anyhow_error_server`.
//!
//! Then trigger the error with:
//! `curl --fail-with-body -H 'X-Request-Id: demo-request-123' http://127.0.0.1:3100/demo/fail`

use anyhow::Context;
use caelix::{Application, Module, ModuleMetadata, Response, Result, controller, injectable};

#[injectable]
struct FailureController;

#[controller("/demo")]
impl FailureController {
    #[get("/ok")]
    async fn ok(&self) -> Result<Response<&'static str>> {
        Ok(Response::Body("Caelix is running"))
    }

    #[get("/fail")]
    async fn fail(&self) -> Result<Response<String>> {
        let contents = load_missing_file()?;
        Ok(Response::Body(contents))
    }
}

fn load_missing_file() -> anyhow::Result<String> {
    std::fs::read_to_string("this-file-does-not-exist.txt")
        .context("the demo deliberately failed to load its file")
}

struct FailureModule;

impl Module for FailureModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().controller::<FailureController>()
    }
}

#[caelix::main]
async fn main() -> std::io::Result<()> {
    Application::new::<FailureModule>()
        .await
        .map_err(|error| std::io::Error::other(error.message))?
        .listen("127.0.0.1:3100")
        .await
}
