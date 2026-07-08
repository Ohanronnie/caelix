# Guards And Interceptors

Guards decide whether a request may continue.

```rust
use std::sync::Arc;

use caelix::{BoxFuture, Guard, RequestContext, Result, UnauthorizedException, guard};

#[guard]
pub struct AuthGuard {
    auth: Arc<AuthService>,
}

impl Guard for AuthGuard {
    fn can_activate<'a>(&'a self, ctx: &'a RequestContext) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let Some(token) = ctx.bearer_token() else {
                return Ok(false);
            };

            let user = self.auth
                .verify(token)
                .await
                .map_err(|_| UnauthorizedException::new("invalid token"))?;

            ctx.set(CurrentUser { id: user.id });
            Ok(true)
        })
    }
}
```

`#[guard]` uses the same expansion as `#[injectable]`, so named fields must be `Arc<T>`.

Attach guards at the controller or method level:

```rust
use caelix::{Result, controller};

#[controller("/admin")]
#[use_guard(AuthGuard)]
impl AdminController {
    #[get("")]
    async fn index(&self) -> Result<&'static str> {
        Ok("ok")
    }
}
```

Method-level guards are appended after controller-level guards. Guards run before extractors are passed to the controller method. If a guard returns `Ok(false)`, the generated wrapper returns `403 Forbidden` with message `Access denied`. If it returns `Err(HttpException)`, that exception is returned to the client.

## Request Context Enrichment

Guards are a good place to authenticate and attach typed request state:

```rust
#[derive(Clone)]
pub struct CurrentUser {
    pub id: i64,
}

impl Guard for AuthGuard {
    fn can_activate<'a>(&'a self, ctx: &'a RequestContext) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let Some(token) = ctx.bearer_token() else {
                return Ok(false);
            };

            let claims = self.auth.verify(token).await?;
            ctx.set(CurrentUser { id: claims.sub });
            Ok(true)
        })
    }
}
```

Controllers can then request `CurrentUser` with `#[user]`.

Interceptors wrap the converted `HttpResponse`.

```rust
use caelix::{BoxFuture, HttpResponse, Interceptor, Next, RequestContext, Result, injectable};

#[injectable]
pub struct AuditInterceptor;

impl Interceptor for AuditInterceptor {
    fn intercept<'a>(&'a self, ctx: &'a RequestContext, next: Next<'a>) -> BoxFuture<'a, Result<HttpResponse>> {
        Box::pin(async move {
            let response = next.run().await?;
            println!("{} {} -> {}", ctx.method(), ctx.path(), response.status);
            Ok(response)
        })
    }
}
```

Interceptors can also transform the response:

```rust
use caelix::HttpResponse;

#[injectable]
pub struct HeaderInterceptor;

impl Interceptor for HeaderInterceptor {
    fn intercept<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Result<HttpResponse>> {
        Box::pin(async move {
            let mut response = next.run().await?;
            if response.content_type == "application/json" {
                if let Some(body) = response.body.as_buffered_mut() {
                    body.extend_from_slice(b"\n");
                }
            }
            Ok(response)
        })
    }
}
```

Body transforms apply only to buffered responses. Streaming bodies (`ResponseBody::Streaming`) are opaque after the handler returns — interceptors can still change status or content type, but should not assume `body_bytes()` is present.

`HttpResponse` stores status, body, content type, and a simple list of owned header name/value pairs (`headers: Vec<(String, String)>`). Use `with_header` when chaining builders, or `insert_header` to mutate an existing response (typical in interceptors). That is enough for dynamic values such as `Content-Disposition` filenames or `X-Request-Id`. It is not a full typed `HeaderMap` (no multi-value merge helpers, no typed header enums). Use native Actix middleware when you need richer header-level response rewriting.

```rust
let mut response = next.run().await?;
response.insert_header("X-Request-Id", request_id);
Ok(response)
```

## Execution Order

For each request, the generated wrapper:

1. Builds `RequestContext` from method, path, and headers.
2. Runs controller-level guards in listed order.
3. Runs method-level guards in listed order.
4. Resolves the controller.
5. Builds the interceptor chain from controller-level then method-level interceptors.
6. Extracts handler arguments and calls the method inside the innermost `Next`.
7. Converts the handler return value into `HttpResponse`.
8. Runs interceptors back outward.

Interceptors run in onion order. The first listed interceptor sees the request first and the response last.
