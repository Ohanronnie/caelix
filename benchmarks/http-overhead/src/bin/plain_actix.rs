use actix_web::{App, HttpResponse, HttpServer, http::header, web};

async fn hello() -> HttpResponse {
    let request_id = uuid::Uuid::new_v4().to_string();
    HttpResponse::Ok()
        .insert_header((header::CONTENT_TYPE, "application/json"))
        .insert_header(("x-request-id", request_id.clone()))
        .insert_header(("x-trace-id", request_id))
        .body(r#""Hello, world!""#)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let workers = std::env::var("BENCH_WORKERS")
        .ok()
        .and_then(|workers| workers.parse().ok())
        .unwrap_or(4);
    HttpServer::new(|| App::new().route("/hello", web::get().to(hello)))
        .workers(workers)
        .bind(("127.0.0.1", 4101))?
        .run()
        .await
}
