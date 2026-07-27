# Error Response Shape

Caelix normalizes controller failures into one JSON envelope. Clients can rely
on `status`, `error`, and `message`; structured client failures may also include
an `errors` object keyed by input field or application-defined key.

```json
{
  "status": 404,
  "error": "Not Found",
  "message": "user not found"
}
```

## Client errors

Use typed exception constructors when an expected application condition should
be visible to the client:

```rust
use caelix::{ConflictException, NotFoundException, Result};

async fn find_user(id: i64) -> Result<User> {
    let user = repository.find(id).await?;
    user.ok_or_else(|| NotFoundException::new("User not found"))
}

async fn create_user(input: CreateUser) -> Result<User> {
    if repository.email_exists(&input.email).await? {
        return Err(ConflictException::new("Email is already registered"));
    }
    repository.insert(input).await
}
```

The most common constructors are `BadRequestException` (`400`),
`UnauthorizedException` (`401`), `ForbiddenException` (`403`),
`NotFoundException` (`404`), `ConflictException` (`409`),
`UnprocessableEntityException` (`422`), `TooManyRequestsException` (`429`),
and `ServiceUnavailableException` (`503`). Caelix also exposes constructors for
the standard HTTP status families when a more precise response is warranted.

Use `401` for missing or invalid credentials and `403` for an authenticated
identity that lacks permission. Do not use a `500` exception to report normal
business conflicts or validation failures.

## Field errors

Validation and other structured client errors add an `errors` object:

```json
{
  "status": 400,
  "error": "Bad Request",
  "message": "Validation failed",
  "errors": {
    "email": ["must be a valid email"],
    "password": ["must be at least 12 characters"]
  }
}
```

Build the same shape for an application-specific client error with
`HttpException::with_errors`:

```rust
use std::collections::BTreeMap;
use caelix::BadRequestException;

let mut errors = BTreeMap::new();
errors.insert("invite_code".into(), vec!["has expired".into()]);
return Err(BadRequestException::new("Invalid invitation").with_errors(errors));
```

`#[validate]` generates this envelope automatically after successful request
decoding. Its rules and generated field messages are documented in
[Validation](../concepts/validation.md).

## Unexpected failures and 5xx safety

`caelix::Result<T>` accepts `?` from ordinary errors convertible to
`anyhow::Error`. An unexpected IO, database, serialization, or library error
becomes `500 Internal Server Error`:

```json
{
  "status": 500,
  "error": "Internal Server Error",
  "message": "Internal Server Error"
}
```

Caelix deliberately hides internal messages and sources for every 5xx response.
Generated controller routes log server failures, including the attached source
chain when one exists. Give clients the response request identifiers so an
operator can find the matching log entry; see [Logging](../advanced/logging.md).

## Error contract guidance

- Treat `error` and `message` as client-facing API fields; use stable wording
  when clients display or branch on them.
- Put field-level feedback in `errors`, not in a string that clients must parse.
- Never put access tokens, session identifiers, SQL, credentials, or internal
  topology in a client error message.
- Return a typed exception at the service boundary where the application knows
  the intended HTTP meaning; allow unexpected infrastructure errors to remain
  unexpected.
- Document expected non-success responses with `#[errors(...)]` when OpenAPI is
  enabled. See [OpenAPI and Swagger UI](../advanced/openapi.md).
