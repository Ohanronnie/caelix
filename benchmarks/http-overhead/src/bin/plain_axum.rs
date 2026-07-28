use axum::{Json, Router, http::header, response::IntoResponse, routing::get};

async fn hello() -> axum::response::Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    let mut response = Json("Hello, world!").into_response();
    response.headers_mut().insert(
        "x-request-id",
        request_id
            .parse()
            .expect("UUID must be a valid header value"),
    );
    response.headers_mut().insert(
        "x-trace-id",
        request_id
            .parse()
            .expect("UUID must be a valid header value"),
    );
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
