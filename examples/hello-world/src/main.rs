use caelix_actix::Application;
use hello_world::AppModule;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    Application::new::<AppModule>()
        .await
        .listen("127.0.0.1:8080")
        .await
}
