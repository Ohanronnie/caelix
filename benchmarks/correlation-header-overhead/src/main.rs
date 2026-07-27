#[cfg(any(feature = "baseline-actix", feature = "baseline-axum"))]
use caelix_baseline as caelix;
#[cfg(any(feature = "current-actix", feature = "current-axum"))]
use caelix_current as caelix;

#[caelix::injectable]
struct BenchmarkController;

#[caelix::controller("")]
impl BenchmarkController {
    #[get("/hello")]
    async fn hello(&self) -> caelix::Result<caelix::Response<&'static str>> {
        Ok(caelix::Response::Body("Hello, world!"))
    }
}

struct BenchmarkModule;

impl caelix::Module for BenchmarkModule {
    fn register() -> caelix::ModuleMetadata {
        caelix::ModuleMetadata::new().controller::<BenchmarkController>()
    }
}

#[caelix::main]
async fn main() -> std::io::Result<()> {
    let application = caelix::Application::new::<BenchmarkModule>()
        .await
        .map_err(|error| std::io::Error::other(error.message))?;

    #[cfg(any(feature = "baseline-actix", feature = "current-actix"))]
    let application = application.workers(
        std::env::var("BENCH_WORKERS")
            .ok()
            .and_then(|workers| workers.parse().ok())
            .unwrap_or(4),
    );

    application.listen("127.0.0.1:4101").await
}
