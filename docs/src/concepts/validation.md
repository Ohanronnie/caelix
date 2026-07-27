# Validation

Caelix validates request data where it enters your application. Define the
rules on a Serde DTO with the [`validator`](https://docs.rs/validator) crate,
then add `#[validate]` beside the extractor argument. A request with invalid
data never reaches the controller method.

Validation is shared by the Actix and Axum adapters. Your DTOs and controller
signatures stay the same when you change HTTP runtimes.

## Enable validation

Enable the `validator` feature, then use the `Validate` derive from Caelix's
re-export:

```sh
cargo add caelix --features validator
cargo add serde --features derive
```

The feature exposes both the controller `#[validate]` marker and
`caelix::validator`. See [Feature Flags](../reference/feature-flags.md) for
combining it with uploads, OpenAPI, or an Axum-only application.

## Validate a JSON body

Put rules on the DTO fields. This request model requires a useful title and
body, checks the author's email address, and limits the selected category.

```rust
use caelix::{controller, Result};
use caelix::validator::Validate;
use serde::Deserialize;

#[derive(Debug, Deserialize, Validate)]
struct CreatePost {
    #[validate(length(min = 3, max = 120))]
    title: String,

    #[validate(length(min = 1, max = 10_000))]
    body: String,

    #[validate(email)]
    author_email: String,

    #[validate(range(min = 1, max = 3))]
    category: u8,
}

pub struct PostsController;

#[controller("/posts")]
impl PostsController {
    #[post("")]
    async fn create(
        &self,
        #[body] #[validate] input: CreatePost,
    ) -> Result<()> {
        // `input` is already decoded and valid here.
        Ok(())
    }
}
```

`#[body]` decodes JSON first. If it succeeds, `#[validate]` calls
`Validate::validate` on the decoded `CreatePost`. The controller receives the
same plain Rust value it would receive without validation.

## Validate query and path data

Validation is not limited to JSON bodies. Use it with a query struct to keep
pagination and filtering rules at the route boundary, or with a path struct
when a route parameter has domain rules beyond basic parsing.

```rust
use caelix::{controller, Result};
use caelix::validator::Validate;
use serde::Deserialize;

#[derive(Deserialize, Validate)]
struct ListPosts {
    #[validate(range(min = 1, max = 100))]
    limit: u16,

    #[validate(range(min = 0))]
    offset: u32,
}

#[derive(Deserialize, Validate)]
struct PostPath {
    #[validate(range(min = 1))]
    id: i64,
}

#[controller("/posts")]
impl PostsController {
    #[get("")]
    async fn list(
        &self,
        #[query] #[validate] options: ListPosts,
    ) -> Result<()> {
        Ok(())
    }

    #[get("/{id}")]
    async fn find_one(
        &self,
        #[param] #[validate] path: PostPath,
    ) -> Result<()> {
        Ok(())
    }
}
```

Use a scalar `#[param] id: i64` when parsing is the only constraint. Use a
struct when you need validation rules, as in `PostPath` above.

## Useful field rules

The `Validate` derive belongs to the `validator` crate, so its field attributes
describe the constraints. These are common choices for HTTP DTOs:

| Rule                                      | Typical use                                  |
| ----------------------------------------- | -------------------------------------------- |
| `#[validate(length(min = 1, max = 120))]` | required text with a bounded length          |
| `#[validate(email)]`                      | email-address format                         |
| `#[validate(range(min = 1, max = 100))]`  | pagination, enum-like numbers, or quantities |
| `#[validate(required)]`                   | an `Option<T>` that must be present          |

Keep transport validation focused on the shape and immediate constraints of
incoming data. Checks that need a database lookup or business state—such as
whether an email address is already registered—belong in an injected service,
where they can return a domain-specific error.

## What clients receive

When a decoded value fails `#[validate]`, Caelix returns `400 Bad Request` with
the standard error envelope. The `errors` object groups the rule messages by
field:

```json
{
  "status": 400,
  "error": "Bad Request",
  "message": "Validation failed",
  "errors": {
    "title": ["must be between 3 and 120 characters"],
    "author_email": ["must be a valid email"]
  }
}
```

The exact field messages come from the rules supplied by `validator`. Treat the
field names and messages as a client-facing API: keep them stable if a web or
mobile client displays them directly. For the complete response contract, see
[Error Response Shape](../reference/error-response-shape.md).

## Execution order

Caelix performs the work in this order:

1. Match the route and extract the path, query, or body value.
2. Deserialize that value into the declared Rust type.
3. Run `Validate::validate` when the argument has `#[validate]`.
4. Invoke the controller method only if every step succeeded.

Malformed JSON, a non-numeric path parameter, or an invalid query encoding
fails during extraction and returns `400 Bad Request` before validation can
run. A well-formed value that violates a DTO rule also returns `400`, with the
validation field errors shown above.

## Validation and uploads

For multipart routes, `#[body] #[validate] input: CreatePost` validates the
decoded text fields after they are parsed. File constraints are separate:
declared size and MIME checks run for `#[file]` and `#[files]`, followed by an
optional asynchronous controller file validator. See [Multipart Uploads](../advanced/multipart-uploads.md)
for the complete upload flow.

## Keep validation close to the route

Put request-specific DTOs beside the controller or feature they serve. This
makes the public HTTP contract easy to find, avoids repeating checks throughout
services, and lets services assume their command data has already passed basic
validation. Reuse a DTO only when the endpoints genuinely share the same input
contract; similar-looking create and update requests often need different
required fields.

For the full extractor matrix and content-type behavior, continue with
[Extractors](extractors.md).
