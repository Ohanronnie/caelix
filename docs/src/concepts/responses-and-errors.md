# Responses And Errors

Handlers usually return `caelix::Result<T>`, where `T` implements `IntoCaelixResponse`. The controller macro converts successful values and `HttpException` errors into adapter responses.

```rust
#[get("")]
pub async fn list(&self) -> Result<Vec<UserDto>> {
    Ok(self.users.list().await?)
}
```

Common response forms:

```rust
use http::StatusCode;
use caelix::Response;

Response::Body(value)
Response::WithStatus(StatusCode::CREATED, value)
Response::no_content()
Response::text(StatusCode::OK, "plain text")
Response::bytes(StatusCode::OK, bytes)
```

Handlers can also return `String`, `&'static str`, `Response<T>`, raw `HttpResponse`, or any `Result<T>` where `T: IntoCaelixResponse`.

## JSON Responses

`Response::Body(value)` serializes `value` as JSON with status `200 OK`.

```rust
#[get("/{id}")]
pub async fn find(&self, #[param] id: i64) -> Result<Response<UserDto>> {
    Ok(Response::Body(self.users.find(id).await?))
}
```

`Response::WithStatus(status, value)` serializes JSON with a custom status:

```rust
#[post("")]
pub async fn create(&self, #[body] input: CreateUserDto) -> Result<Response<UserDto>> {
    let user = self.users.create(input).await?;
    Ok(Response::WithStatus(StatusCode::CREATED, user))
}
```

If JSON serialization fails, Caelix returns a generic `500 Internal Server Error` JSON body.

Actix adapter errors for supported controller extractors are also returned in the Caelix JSON error shape. Invalid route params, such as a malformed `Uuid` passed to a `#[param]` argument, invalid `#[query]` values, and invalid JSON bodies return `400 Bad Request` JSON responses. Missing required body, query, or path fields use the validation error shape with an `errors` object. Requests that do not match any registered route return a `404 Not Found` JSON response.

## Empty, Text, Bytes, And Raw

Use `Response<()>` helpers for non-JSON response bodies:

```rust
#[delete("/{id}")]
pub async fn delete(&self, #[param] id: i64) -> Result<Response<()>> {
    self.users.delete(id).await?;
    Ok(Response::no_content())
}

#[get("/health")]
pub async fn health(&self) -> Result<Response<()>> {
    Ok(Response::text(StatusCode::OK, "ok"))
}
```

`Response::bytes` uses `application/octet-stream`. `HttpResponse::new`, `HttpResponse::json`, `HttpResponse::text`, and `HttpResponse::bytes` are available when you need the fully materialized response type.

## Streaming Responses

`HttpResponse` holds a `ResponseBody` that is either fully buffered (`Vec<u8>`) or a streaming async sequence of `Bytes` chunks. Use streaming when the body is not ready as one blob — large exports, live feeds, or files on disk.

**Migration note (breaking in 0.0.x):** `HttpResponse.body` is no longer `Vec<u8>`. Use `response.body_bytes()` / `response.body.as_buffered()` / `as_buffered_mut()` for buffered bodies. Direct `response.body.extend(...)` or `== b"..."` comparisons need updating. Optional response headers live in `response.headers` as owned `(String, String)` pairs (`with_header(name, value)` accepts dynamic values) and are applied by the Actix adapter.

Streaming helpers return `HttpResponse` directly. Handlers typically return `Result<HttpResponse>` (which already implements `IntoCaelixResponse` as identity):

```rust
use caelix::{Bytes, HttpResponse, Response, Result, StreamExt};

#[get("/export")]
async fn export_csv(&self) -> Result<HttpResponse> {
    let rows = self.repo.stream_all_users();
    let bytes_stream = rows.map(|row| {
        row.map(|r| Bytes::from(format!("{},{}\n", r.id, r.name)))
    });
    Ok(Response::stream("text/csv", bytes_stream))
}
```

`StreamExt` (for `.map` / `.filter` on streams) is re-exported from `caelix` so you do not need a direct `futures-util` dependency for the common helpers.

### Server-Sent Events

`Response::sse` frames each stream item as JSON in SSE wire format (`data: …\n\n`) with content type `text/event-stream`, and sets `Cache-Control: no-cache` plus `X-Accel-Buffering: no`. It does not yet implement the full SSE protocol (`id:`, `event:`, `retry:`, Last-Event-ID resume):

```rust
#[get("/live-orders")]
async fn live_orders(&self) -> Result<HttpResponse> {
    let stream = self.events.subscribe::<OrderPlacedEvent>();
    Ok(Response::sse(stream))
}
```

### File streaming

`Response::file` opens a path asynchronously and streams disk chunks (not the whole file in memory). Open errors are mapped by kind: missing path → `404 Not Found`, permission denied → `403 Forbidden`, other IO failures → `500 Internal Server Error`.

```rust
#[get("/report.pdf")]
async fn report(&self) -> Result<HttpResponse> {
    Response::file("/var/data/report.pdf", "application/pdf").await
}
```

The Actix adapter maps buffered bodies with `.body(...)` and streaming bodies with `.streaming(...)` (chunked transfer encoding), and applies `HttpResponse.headers`. Mid-stream errors cannot change the already-sent status line; they close the stream after logging.

## Exceptions

Use typed exception constructors for client errors:

```rust
return Err(NotFoundException::new("user not found"));
```

Most HTTP client and server status families have a matching exception constructor, for example `BadRequestException`, `UnauthorizedException`, `ForbiddenException`, `ConflictException`, `UnprocessableEntityException`, `TooManyRequestsException`, and `ServiceUnavailableException`.

Validation errors can include field details:

```rust
use std::collections::BTreeMap;

let mut errors = BTreeMap::new();
errors.insert("email".to_string(), vec!["must be a valid email".to_string()]);

return Err(BadRequestException::new("Validation failed").with_errors(errors));
```

Server error responses are production-safe: if an `HttpException` has a 5xx status, the response body message is `Internal Server Error` rather than the internal error text. Generated controller routes log returned 5xx exceptions through the `ExceptionHandler` logger, including the internal `source` when one is attached.

```rust
let user = repository
    .find(id)
    .await
    .map_err(InternalServerErrorException::new)?;
```

The internal source is retained on `HttpException::source`, but the client receives only:

```json
{
  "status": 500,
  "error": "Internal Server Error",
  "message": "Internal Server Error"
}
```
## Cookies

`Cookie::new` defaults to `HttpOnly`, `Secure`, `SameSite::Lax`, and `Path=/`.
These defaults reduce script access, accidental cleartext transport, and common
cross-site request risks. Local plain-HTTP development can explicitly use
`.secure(false)`; production cookies should normally retain `Secure`.

```rust
let response = Response::Body(user)
    .with_cookie(Cookie::new("session", opaque_session_token))
    .with_cookie(Cookie::new("theme", "dark").http_only(false));
```

Cookies can also be attached to raw, file, streaming, and SSE `HttpResponse`
values with `.with_cookie(...)`. Every cookie becomes its own `Set-Cookie`
header and call order is preserved.

To log out, match the original path and domain:

```rust
Response::no_content().with_cookie(
    Cookie::removal("session").path("/").domain("example.com"),
)
```

Caelix does not maintain a cookie jar or create, persist, rotate, sign, encrypt,
or resolve sessions. Applications should use opaque random session tokens and
validate them in their own session service. CSRF token generation and
verification are separate concerns.
