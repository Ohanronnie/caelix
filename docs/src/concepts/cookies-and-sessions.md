# Cookies and Sessions

Caelix provides runtime-neutral incoming cookie lookup and secure response-cookie
construction. It deliberately does not own session storage or cryptography.

## Read cookies

```rust
#[get("/me")]
async fn me(
    &self,
    #[cookie("session")] session: String,
    #[cookie("theme")] theme: Option<String>,
) -> Result<Response<User>> {
    Ok(Response::Body(self.sessions.user(session, theme).await?))
}
```

`#[cookie]` requires a non-empty string literal. `String` is required and a
missing value returns `400 Bad Request`; `Option<String>` becomes `None`.
Guards and interceptors can use `ctx.cookie("session") -> Option<&str>`.

Caelix combines repeated `Cookie` headers, percent-decodes names and values, and
keeps the first value when a name occurs more than once. Parsing happens once
when `RequestContext` is created. With `openapi`, the macro emits a string cookie
parameter whose `required` flag matches the Rust type.

## Set and remove cookies

```rust
use caelix::{Cookie, Response, SameSite};
use std::time::Duration;

let response = Response::Body(user)
    .with_cookie(
        Cookie::new("session", token)
            .same_site(SameSite::Strict)
            .max_age(Duration::from_secs(60 * 60)),
    )
    .with_cookie(Cookie::new("theme", "dark").http_only(false));
```

`Cookie::new` defaults to `HttpOnly`, `Secure`, `SameSite=Lax`, and `Path=/`.
Builder methods are `http_only`, `secure`, `same_site`, `path`, `domain`,
`max_age`, and `expires`. `Response::with_cookie` and
`HttpResponse::with_cookie` append in call order; adapters preserve every cookie
as a separate `Set-Cookie` header.

Remove a stored cookie with matching scope:

```rust
Response::no_content().with_cookie(
    Cookie::removal("session").path("/auth").domain("example.com"),
)
```

`removal` uses an empty value, `Max-Age=0`, and the Unix epoch. Its path and
domain must match the original cookie or the browser will retain the original.

## Local HTTP and security boundaries

Browsers do not send `Secure` cookies over plain local HTTP. For local-only
development, explicitly use `.secure(false)`; keep the default behind HTTPS.
Choose `SameSite` according to the request flow and add application-level CSRF
protection wherever cookies authenticate cross-site requests.

Cookie values are opaque to Caelix. A session provider owns lookup, rotation,
expiry, revocation, and server-side state. Caelix does not sign or encrypt
values. Use an audited signing/encryption library in an injectable session
service, keep keys outside source control, and do not confuse signing
(integrity) with encryption (confidentiality).

## Test the complete exchange

Send incoming cookies with `.header("cookie", "...")`. Inspect response headers
when asserting `Set-Cookie`; multiple cookies are multiple header values, not a
comma-separated field. Test required/optional extraction, percent decoding,
secure attributes, and removal scope. The repository's cookie integration tests
exercise the same controller expansion on both runtimes.
