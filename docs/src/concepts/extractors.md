# Extractors

Controller extractors turn HTTP request data into typed method arguments. Their
behavior is shared by Actix and Axum; applications do not use backend extractor
types in controller signatures.

| Attribute           | Accepted Rust type                                   | Feature                          |
| ------------------- | ---------------------------------------------------- | -------------------------------- |
| `#[param]`          | any Serde-deserializable scalar, tuple, or struct    | runtime                          |
| `#[query]`          | any Serde-deserializable type                        | runtime                          |
| `#[body]`           | any Serde-deserializable DTO                         | runtime; `uploads` for multipart |
| `#[user]`           | a cloneable concrete type stored in `RequestContext` | runtime                          |
| `#[cookie("name")]` | `String` or `Option<String>`                         | runtime                          |
| `#[file]`           | `UploadedFile` or `Option<UploadedFile>`             | `uploads`                        |
| `#[files]`          | `Vec<UploadedFile>`                                  | `uploads`                        |
| `#[multipart]`      | `MultipartForm`                                      | `uploads`                        |
| `#[validate]`       | a value implementing `validator::Validate`           | `validator`                      |

Arguments must be simple identifiers. Each extracted parameter has exactly one
extractor marker; `#[validate]` is an additional marker on a decoded value.
See [Validation](validation.md) for DTO rules, error responses, and examples
for body, query, and path values.

## Path, query, body, user, and cookie values

```rust
#[post("/{account_id}")]
async fn update(
    &self,
    #[param] account_id: i64,
    #[query] options: UpdateOptions,
    #[body] #[validate] input: UpdateAccount,
    #[user] actor: CurrentUser,
    #[cookie("session")] session: String,
) -> Result<Response<Account>> {
    self.accounts.update(account_id, options, input, actor, session).await
}
```

Path and query types use Serde conversion. A body accepts JSON when
`Content-Type` is `application/json` or omitted. With `uploads`, it also accepts
multipart text fields using Serde form semantics. `#[user]` clones a typed value
previously inserted by a guard or interceptor. A missing user returns `401
Unauthorized` with `Not authenticated`.

Cookie extraction is described fully in [Cookies and Sessions](cookies-and-sessions.md).
A required cookie is `String`; an optional cookie is `Option<String>`.

## Upload combinations

`#[body]` may be combined with `#[file]` or `#[files]`. A required file makes
multipart mandatory. If every file is optional, JSON remains valid and supplies
`None`. `Vec<UploadedFile>` collects repeated parts and requires multipart.
`#[multipart] MultipartForm` owns the complete form and cannot be combined with
`#[body]`, `#[file]`, or `#[files]`.

File checks run in request order: declared size, declared or detected MIME type,
then the async controller validator named by `validate = method`. DTO
`#[validate]` runs after successful decoding. See [Validation](validation.md)
for DTO validation and [Multipart Uploads](../advanced/multipart-uploads.md)
for file types, lifecycle, and `#[upload(limit = bytes)]`.

## Failure normalization and order

Extraction finishes before the controller method runs. Caelix normalizes failures:

- `400 Bad Request`: malformed path/query/body/multipart data, missing required
  cookie or file, duplicate single-file parts, validation failure, or rejected MIME.
- `401 Unauthorized`: missing `#[user]` context value.
- `413 Payload Too Large`: application/route body limit or file-size violation.
- `415 Unsupported Media Type`: a declared content type the route cannot accept.

Malformed input cannot reach validation because no typed value exists. After
decoding, DTO validation and then file validation complete before method
invocation. A failed multipart request drops all staged files.

With `openapi`, body, path, query, file, and cookie declarations contribute
their corresponding operation parameters or request body. Runtime errors keep
the standard [error response shape](../reference/error-response-shape.md).
