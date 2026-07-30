# OpenAPI and Swagger UI

Enable the `openapi` feature. Caelix re-exports `utoipa`, including `ToSchema`, so no separate dependency is required:

```sh
cargo add caelix --features openapi
```

Opt in when building the application. Caelix serves OpenAPI 3.1 JSON at `/openapi.json` and Swagger UI at `/docs`.

```rust
use caelix::openapi::OpenApiConfig;

let app = Application::new::<AppModule>()
    .await?
    .with_openapi(OpenApiConfig::new("Payments API", "1.0.0"))?;
```

`OpenApiConfig::json_path(...)` and `OpenApiConfig::ui_path(...)` customize these paths. They must not collide with controller routes.

## What Caelix publishes

At startup, Caelix walks the complete imported module graph and builds one
OpenAPI 3.1 document from registered controller routes. Each operation has its
HTTP method, final path, operation ID, inferred request/response metadata, and
any explicit documentation markers on the controller method.

The document is served from `json_path` and Swagger UI is served from `ui_path`:

```rust
use caelix::openapi::OpenApiConfig;

let openapi = OpenApiConfig::new("Payments API", "2026.07")
    .json_path("/api/openapi.json")
    .ui_path("/api/docs");

let app = Application::new::<AppModule>()
    .await?
    .with_openapi(openapi)?;
```

Paths are normalized to begin with `/`. Keep the JSON and UI endpoints outside
your public controller route space so startup can reject collisions clearly.

## Schemas, parameters, and inferred operations

Derive `ToSchema` for DTOs that appear in explicit OpenAPI request or response
documentation. Caelix re-exports `utoipa`, so the feature does not require a
separate direct dependency:

```rust
use caelix::openapi::ToSchema;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, ToSchema)]
struct CreatePayment {
    amount_cents: i64,
    currency: String,
}

#[derive(Serialize, ToSchema)]
struct Payment {
    id: String,
    amount_cents: i64,
    currency: String,
}
```

Caelix documents these route declarations automatically:

| Controller declaration              | OpenAPI contribution                                           |
| ----------------------------------- | -------------------------------------------------------------- |
| `#[get("/{id}")]`                   | method and path operation                                      |
| `#[param] id: i64`                  | required path parameter                                        |
| `#[query] page: u32`                | query parameter                                                |
| `#[cookie("session")]`              | cookie parameter                                               |
| `#[body] input: Dto`                | JSON request body                                              |
| `#[file]` / `#[files]`              | multipart binary property, including declared file constraints |
| `#[multipart]`                      | free-form multipart body                                       |
| `Result<T>` / `Result<Response<T>>` | successful `200` response when it can be inferred              |

Use explicit markers whenever the operation has a non-`200` success status,
raw/streaming body, response headers, known errors, or security requirements.
That makes the public contract precise instead of relying on return-type
inference.

## Describe responses, headers, and errors

`#[response]` controls the successful response. It accepts a positional body
type or `body = Type`, an optional integer `status`, a `content_type`, and
optional response `headers` tuples of `(name, schema, description)`:

```rust
#[post("")]
#[response(
    status = 201,
    body = Payment,
    headers(("Location", String, "Canonical payment URL"))
)]
#[errors(BadRequestException, ConflictException)]
async fn create(&self, #[body] input: CreatePayment) -> Result<Response<Payment>> {
    // ...
}
```

`#[errors(...)]` produces the standard Caelix error envelope for each listed
exception status. Global or route throttling also documents `429 Too Many
Requests` automatically. Use `#[request_header]` for headers the client sends
outside the body:

```rust
#[request_header(
    name = "Idempotency-Key",
    schema = String,
    required,
    description = "Unique key for safely retrying a write"
)]
#[post("")]
async fn create(&self, #[body] input: CreatePayment) -> Result<Response<Payment>> {
    // ...
}
```

The marker documents a header; it does not extract, authenticate, or enforce
it at runtime. Implement that behavior in a guard, interceptor, or controller
extractor/service policy.

## Keep the contract and runtime aligned

OpenAPI records the route contract; it does not change the controller runtime.
Keep these concerns paired in source:

| Contract declaration             | Runtime behavior to keep beside it                                 |
| -------------------------------- | ------------------------------------------------------------------ |
| `#[security(...)]`               | `#[use_guard(...)]` and the credential-verification guard          |
| `#[errors(...)]`                 | Typed exceptions returned by the service/controller                |
| `#[response(status = 201, ...)]` | `Response::WithStatus(StatusCode::CREATED, ...)`                   |
| `#[request_header(...)]`         | The guard, interceptor, or service policy that requires the header |
| `#[body] #[validate]`            | DTO field rules and the `validator` feature                        |

Validation rejects invalid request data with a documented `400`
`ValidationErrorEnvelope`, including an `errors` map of field messages. Other
`#[errors(...)]` responses use the shared `ErrorEnvelope`, which contains only
`status`, `error`, and `message`. Document additional client errors with
`#[errors(...)]`; the full validation rules are in
[Validation](../concepts/validation.md).

Document DTOs with `utoipa::ToSchema`. The controller macro infers JSON request
bodies from `#[body]`, multipart request bodies from upload routes, and
successful `200` responses from `Result<T>` and `Result<Response<T>>`.
`#[validate]` preserves the same typed OpenAPI schema; it only adds runtime
validation.

For a route with `#[body]` and `#[file]`, OpenAPI adds
`multipart/form-data`: the DTO fields remain schema-backed and file fields are
binary properties. Required single files are listed as required; optional file
properties are not. Routes whose files are all optional retain their JSON
request content type as well. A direct `#[multipart] MultipartForm` route is
documented as a free-form multipart request body.

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

Security schemes are documentation-only. Register reusable schemes on `OpenApiConfig`, then add an imported `#[security(...)]` marker to the routes that require them. Caelix adds the resulting `components.securitySchemes` and operation-level `security` objects to `/openapi.json`, which makes Swagger UI expose its **Authorize** controls.

This does not add guards, extract credentials, verify tokens, read cookies, or alter authorization. Keep runtime authorization explicit with `#[use_guard(...)]`, `#[user]`, and your own authentication services.

### Standard schemes

Caelix reserves four standard security-scheme names. Register the builder that corresponds to the route marker you plan to use:

| Builder                      | OpenAPI component                     | Route requirement          |
| ---------------------------- | ------------------------------------- | -------------------------- |
| `.bearer_auth()`             | `BearerAuth`: HTTP bearer, JWT format | `Security::BearerAuth`     |
| `.api_key_auth("X-API-Key")` | `ApiKeyAuth`: API key in a header     | `Security::ApiKeyAuth`     |
| `.cookie_auth("session")`    | `CookieAuth`: API key in a cookie     | `Security::CookieAuth`     |
| `.oauth2(...)`               | `OAuth2`: supplied OAuth2 flows       | `Security::OAuth2(&[...])` |

For example, this configures bearer, header API-key, cookie, and OAuth2 client-credentials components in Swagger UI:

```rust
use caelix::openapi::{OpenApiConfig, security};

let openapi = OpenApiConfig::new("Payments API", "1.0.0")
    .bearer_auth()
    .api_key_auth("X-API-Key")
    .cookie_auth("session")
    .oauth2(security::OAuth2::new([
        security::Flow::ClientCredentials(security::ClientCredentials::new(
            "https://identity.example.com/oauth/token",
            security::Scopes::from_iter([
                ("payments:read", "Read payments"),
                ("payments:write", "Create and update payments"),
            ]),
        )),
    ]));
```

Pass that configuration to the normal fallible application builder:

```rust
use caelix::openapi::{OpenApiConfig, Security, security};

let app = Application::new::<AppModule>()
    .await?
    .with_openapi(openapi)?;

#[controller("/account")]
impl AccountController {
    #[security(Security::BearerAuth)]
    #[get("/me")]
    async fn me(&self, #[user] user: CurrentUser) -> Result<UserDto> {
        // ...
    }
}
```

### OAuth2 scopes and combined requirements

`Security::OAuth2(&["payments:read"])` requests those OAuth2 scopes for one operation. Keep the scope names aligned with the OAuth2 flow you registered. API-key, bearer, and cookie requirements always carry an empty OpenAPI scope list.

Multiple `#[security(...)]` attributes on the same method produce one OpenAPI Security Requirement Object, so they are combined as **AND**. This is useful when an endpoint requires both a user token and a tenant key:

```rust
#[security(Security::BearerAuth)]
#[security(Security::ApiKeyAuth)]
#[security(Security::OAuth2(&["payments:write"]))]
#[use_guard(WritePaymentGuard)]
#[post("")]
async fn create(&self, #[body] input: CreatePaymentDto) -> Result<Response<PaymentDto>> {
    // Runtime guard behavior is unchanged by the documentation attributes.
}
```

OpenAPI also supports OR alternatives, but Caelix’s current marker syntax intentionally documents the common AND case only. Model a route with optional or alternative authentication explicitly in your application and register its custom documentation scheme as appropriate.

### Application-defined schemes

Use `.security_scheme(...)` and `Security::Custom` when the standard names do not fit—for example, a tenant key, a signed request, or an OpenID Connect component:

```rust
use caelix::openapi::{OpenApiConfig, Security, security};

let openapi = OpenApiConfig::new("Payments API", "1.0.0").security_scheme(
    "TenantAuth",
    security::SecurityScheme::ApiKey(security::ApiKey::Header(
        security::ApiKeyValue::new("X-Tenant-Key"),
    )),
);

#[security(Security::Custom {
    name: "TenantAuth",
    scopes: &[],
})]
#[get("/tenant/payments")]
async fn list(&self) -> Result<Vec<PaymentDto>> {
    // ...
}
```

`Security::Custom` may include scopes for an application-defined OAuth2 scheme. Its `name` must exactly match the component name registered with `.security_scheme(...)`.

### Configuration validation

`with_openapi(...)` returns `caelix::Result<Self>`. Caelix validates the completed document before the application starts and returns an `OpenAPI Configuration Error` when:

- a route names a scheme that has not been registered;
- `BearerAuth`, `ApiKeyAuth`, `CookieAuth`, or `OAuth2` is registered under the standard name with the wrong builder; or
- the OpenAPI JSON or Swagger UI paths collide with controller routes.

This catches documentation drift early while keeping security configuration and runtime authentication intentionally separate. Use `#[request_header(...)]` for ordinary request metadata such as idempotency keys and tenant identifiers; do not use it to describe bearer tokens, API keys, or session cookies.
