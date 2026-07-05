use caelix_actix::Application;
use sqlx_crud::AppModule;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let addr = std::env::var("ADDR").unwrap_or_else(|_| "127.0.0.1:8081".to_string());

    Application::new::<AppModule>().await.listen(&addr).await
}
