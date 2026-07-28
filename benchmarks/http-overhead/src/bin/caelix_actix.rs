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
    caelix::Application::new::<BenchmarkModule>()
        .await
        .map_err(|error| std::io::Error::other(error.message))?
        .workers(
            std::env::var("BENCH_WORKERS")
                .ok()
                .and_then(|workers| workers.parse().ok())
                .unwrap_or(4),
        )
        .listen("127.0.0.1:4101")
        .await
}
