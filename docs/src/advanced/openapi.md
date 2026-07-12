# OpenAPI and Swagger UI

Enable the `openapi` feature. Caelix re-exports `utoipa`, including `ToSchema`, so no separate dependency is required:

```toml
caelix = { version = "0.0.13", features = ["openapi"] }
```

Opt in when building the application. Caelix serves OpenAPI 3.1 JSON at `/openapi.json` and Swagger UI at `/docs`.

```rust
use caelix::openapi::OpenApiConfig;

let app = Application::new::<AppModule>()
    .await?
    .with_openapi(OpenApiConfig::new("Payments API", "1.0.0"))?;
```

`OpenApiConfig::json_path(...)` and `OpenApiConfig::ui_path(...)` customize these paths. They must not collide with controller routes.

Document DTOs with `utoipa::ToSchema`. The controller macro infers JSON request bodies from `#[body]` and successful `200` responses from `Result<T>` and `Result<Response<T>>`.

```rust
use caelix::openapi::{ToSchema, errors, request_header, response, utoipa};

#[derive(ToSchema)]
struct PaymentDto {
    id: String,
}

#[controller("/payments")]
impl PaymentController {
    #[post("")]
    #[request_header(name = "Idempotency-Key", schema = String, required)]
    #[response(status = 201, body = PaymentDto, headers(("Location", String, "Payment URL")))]
    #[errors(BadRequestException, ConflictException)]
    async fn create(&self, #[body] input: PaymentDto) -> Result<Response<PaymentDto>> {
        // ...
    }
}
```

`#[request_header]` only adds documentation; it does not extract or authenticate request headers. `#[response(BodyType)]` overrides inferred response schema, while `content_type` and optional `headers(...)` can document raw or streaming output without inventing a body schema. `#[errors(...)]` documents only the exception markers listed, using Caelix's shared error envelope. Custom error marker types can implement `caelix::openapi::OpenApiError`.

## Security schemes

Security schemes are also documentation-only. Register reusable schemes on `OpenApiConfig`, then add an imported `#[security(...)]` marker to routes. This does not add guards, extract credentials, or alter authorization.

```rust
use caelix::openapi::{Security, security};

let app = Application::new::<AppModule>()
    .await?
    .with_openapi(
        OpenApiConfig::new("Payments API", "1.0.0")
            .bearer_auth()
            .api_key_auth("X-API-Key")
            .cookie_auth("session"),
    )?;

#[controller("/account")]
impl AccountController {
    #[security(Security::BearerAuth)]
    #[get("/me")]
    async fn me(&self, #[user] user: CurrentUser) -> Result<UserDto> {
        // ...
    }
}
```

`Security::OAuth2(&["users:read"])` attaches OAuth scopes to the standard `OAuth2` scheme, registered with `.oauth2(caelix::openapi::security::OAuth2::new(...))`. Use `.security_scheme(name, scheme)` with `Security::Custom { name, scopes }` for application-defined schemes. Multiple `#[security(...)]` entries on one route are combined as OpenAPI **AND** requirements. Caelix fails OpenAPI generation if a route names an unregistered or mismatched standard scheme.
