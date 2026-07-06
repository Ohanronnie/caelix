use caelix_actix::Application;
use avera::AppModule;

#[caelix::main]
async fn main() -> std::io::Result<()> {
    Application::new::<AppModule>()
        .await
        .listen("127.0.0.1:8080")
        .await
}
