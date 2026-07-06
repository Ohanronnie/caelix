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

Server error responses are production-safe: if an `HttpException` has a 5xx status, the response body message is `Internal Server Error` rather than the internal error text.

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
