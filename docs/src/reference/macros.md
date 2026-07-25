# Macro Attributes

## `#[injectable]`

Applies to unit structs or named-field structs. Named fields must be `Arc<T>`.

```rust
#[injectable]
pub struct UsersService;
```

```rust
#[injectable]
pub struct UsersController {
    service: Arc<UsersService>,
}
```

Expansion behavior:

- Implements `caelix::Injectable`.
- Resolves each `Arc<T>` field with `container.resolve::<T>()`.
- Uses `container.resolve_logger(stringify!(TypeName))` for `Arc<Logger>`.
- Leaves lifecycle hooks as default no-ops.

Invalid patterns:

```rust
#[injectable]
pub struct Bad(String); // tuple structs are rejected

#[injectable]
pub enum Bad { A } // non-struct items are rejected

#[injectable]
pub struct Bad {
    value: String, // named fields must be Arc<T>
}
```

## `#[guard]`

Uses the same dependency injection expansion as `#[injectable]`. The type must implement `Guard`.

## `#[controller("/base")]`

Applies to an `impl` block. Route methods may use:

- `#[get("...")]`
- `#[post("...")]`
- `#[patch("...")]`
- `#[put("...")]`
- `#[delete("...")]`

Controller and method attributes:

- `#[use_guard(Type)]`
- `#[use_interceptor(Type)]`

Extractor attributes:

- `#[param]`
- `#[query]`
- `#[body]`
- `#[file]`
- `#[files]`
- `#[multipart]`
- `#[user]`
- `#[cookie("name")]`
- `#[validate]`

### Bodies and uploads

`#[body]` accepts `application/json`, omitted content type, and
`multipart/form-data`. JSON and multipart text fields both decode into the
declared type. Add `#[validate]` to run `validator::Validate` after either
decode path.

`#[file] name: UploadedFile` requires exactly one named multipart file; use
`Option<UploadedFile>` when it may be absent. `#[files] files: Vec<UploadedFile>`
collects repeated file parts. File fields default to their argument name and
accept `name = "..."`, for example `#[file(name = "avatar")] avatar:
UploadedFile`.

`#[multipart] form: MultipartForm` provides direct full-form access and is
intentionally incompatible with `#[body]`, `#[file]`, and `#[files]` on the
same route. Use `#[upload(limit = bytes)]` on a route to set a stricter
multipart body limit than the application limit.

`#[file]`/`#[files]` additionally accept `max_size`, `content_type`,
`trust_content_type_header`, and an async controller `validate` method. See
[Extractors](../concepts/extractors.md).

Example:

```rust
#[controller("/users")]
#[use_guard(AuthGuard)]
#[use_interceptor(AuditInterceptor)]
impl UsersController {
    #[get("/{id}")]
    pub async fn find(&self, #[param] id: i64) -> Result<UserDto> {
        self.users.find(id).await
    }

    #[post("")]
    pub async fn create(
        &self,
        #[body] #[validate] input: CreateUserDto,
    ) -> Result<Response<UserDto>> {
        let user = self.users.create(input).await?;
        Ok(Response::WithStatus(StatusCode::CREATED, user))
    }
}
```

`#[param]` and `#[query]` use the selected runtime's native path and query
extractors. Body and upload routes use Caelix's shared negotiated request
wrapper so the same controller source works on Actix and Axum. `#[user]` reads
`RequestContext::get::<T>()` and returns `401 Unauthorized` if the typed value
is missing.

Invalid extractor pattern:

```rust
#[get("/{id}")]
pub async fn find(&self, #[param] Some(id): Option<i64>) -> Result<String> {
    Ok(id.to_string())
}
```

Extractor arguments must be simple identifiers because the macro moves extracted values into the controller method call.

## `#[gateway("/path")]`

Decorates a WebSocket or Socket.IO gateway implementation and supplies the explicit path metadata
used by `ModuleMetadata::gateway::<Gateway>()`.

For RFC 6455 sockets, place it on `impl WebSocketGateway for Gateway`; callback methods use the
`WebSocketGateway` trait. This works with both Actix and Axum.

With the Axum-only `socketio` feature, place it on an inherent implementation and annotate each
async event method with `#[on_message("event")]`. Such methods accept either `payload: T` or
`socket: SocketRef, payload: T` and return `Result<Reply>`; successful replies become Socket.IO
acks, while failures also emit `"error"`.
### `#[cookie("name")]`

Controller arguments may use `String` for a required cookie or
`Option<String>` for an optional cookie. The name must be a non-empty string
literal. Extraction reads the generated `RequestContext`, so the same
controller source works with Actix and Axum.

## OpenAPI markers

With `openapi`, controller expansion consumes `#[response(...)]`,
`#[errors(...)]`, `#[request_header(...)]`, and `#[security(...)]`. They declare
successful/error responses, header parameters, and security requirements; they
do not perform runtime authentication or extraction. Controller extractors,
including cookies and uploads, also contribute request metadata.

## `#[upload(limit = bytes)]`

This route marker requires `uploads` and supplies a stricter multipart limit
than the application body limit. It is meaningful only on a controller route.

## Microservice macros

`#[microservice]` decorates an inherent impl. Its methods use exactly one
`#[message_pattern("subject")]` or `#[event_pattern("subject")]`, one bare
`#[payload]`, and optionally one bare `#[context] MessageContext`.

Handlers must be async, non-generic, take `&self`, use named parameters, and
return `Result<T>`. Payloads require `DeserializeOwned + Send`; command response
types require `Serialize + Send`; events must return `Result<()>`. Command
subjects reject wildcards. Event subjects accept whole-token `*` and terminal
`>`. The macro generates `Microservice` metadata and resolves the handler class
from the module container.
