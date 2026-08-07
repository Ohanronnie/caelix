use axum::{Json, Router, http::header, response::IntoResponse, routing::get};

async fn hello() -> axum::response::Response {
    let mut response = Json("Hello, world!").into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/json".parse().expect("static header is valid"),
    );
    response
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let app = Router::new().route("/hello", get(hello));
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 4101)).await?;
    axum::serve(listener, app).await
}
