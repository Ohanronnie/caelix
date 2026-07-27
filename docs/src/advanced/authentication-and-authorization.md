# Authentication And Authorization

Caelix gives authentication a clear place in the request lifecycle without
choosing your identity provider, token format, or session database. Put
credential verification in a guard, attach the authenticated identity to the
request context, and make authorization decisions close to the controller or
service that owns the resource.

This keeps HTTP credentials at the edge while services receive an explicit
application identity instead of parsing headers themselves.

## The request flow

A protected route usually follows this sequence:

1. A guard reads a bearer token, API key, or session cookie from `RequestContext`.
2. An injected authentication service verifies the credential and loads the
   application identity.
3. The guard stores a cloneable `CurrentUser` in the request context.
4. The controller receives it with `#[user]`; services enforce ownership and
   permission rules using that explicit identity.

Guards run before controller extraction and controller methods. A controller
never sees an unauthenticated request when its guard has rejected it.

## Authenticate a bearer token

Keep token parsing and verification in an injectable service. The guard only
coordinates the HTTP request and stores the resulting identity.

```rust
use std::sync::Arc;

use caelix::{
    BoxFuture, Guard, RequestContext, Result, UnauthorizedException, guard,
    injectable,
};

#[derive(Clone)]
pub struct CurrentUser {
    pub id: i64,
    pub role: Role,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Member,
    Administrator,
}

#[injectable]
pub struct AuthService;

impl AuthService {
    async fn verify_bearer(&self, token: &str) -> Result<CurrentUser> {
        // Verify the signature, expiry, issuer, audience, and any application
        // policy with your identity provider or token library.
        let _ = token;
        Ok(CurrentUser { id: 42, role: Role::Member })
    }
}

#[guard]
pub struct AuthGuard {
    auth: Arc<AuthService>,
}

impl Guard for AuthGuard {
    fn can_activate<'a>(&'a self, ctx: &'a RequestContext) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let Some(token) = ctx.bearer_token() else {
                return Err(UnauthorizedException::new("Missing bearer token"));
            };

            let user = self.auth.verify_bearer(token).await?;
            ctx.set(user)?;
            Ok(true)
        })
    }
}
```

`ctx.bearer_token()` reads an `Authorization: Bearer <token>` header. Invalid
credentials should return `UnauthorizedException`; `Ok(false)` is reserved for
a deliberate `403 Forbidden` response from the guard wrapper.

## Use the authenticated user

Attach `AuthGuard` to a controller when every route needs authentication, then
receive the identity as a normal typed argument.

```rust
use caelix::{ForbiddenException, Result, controller};

#[controller("/posts")]
#[use_guard(AuthGuard)]
impl PostsController {
    #[post("")]
    async fn create(
        &self,
        #[user] user: CurrentUser,
        #[body] input: CreatePost,
    ) -> Result<Post> {
        self.posts.create(user.id, input).await
    }

    #[delete("/{id}")]
    async fn delete(
        &self,
        #[user] user: CurrentUser,
        #[param] id: i64,
    ) -> Result<()> {
        let post = self.posts.find(id).await?;
        if post.author_id != user.id && user.role != Role::Administrator {
            return Err(ForbiddenException::new("You cannot delete this post"));
        }

        self.posts.delete(id).await
    }
}
```

`#[user]` clones the concrete value stored by the guard. If no guard or
interceptor stored that type, Caelix returns `401 Unauthorized` with `Not
authenticated`. Keep `CurrentUser` small: identifiers, tenancy, roles, and
claims needed by the request are appropriate; database handles and raw tokens
are not.

## Register the security module

Guards are providers and follow normal module visibility rules. Export the
guard from the module that owns it, then import that module wherever a
controller uses it.

```rust
use caelix::{Module, ModuleMetadata};

pub struct AuthModule;

impl Module for AuthModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<AuthService>()
            .provider::<AuthGuard>()
            .export::<AuthGuard>()
    }
}

pub struct PostsModule;

impl Module for PostsModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<AuthModule>()
            .controller::<PostsController>()
    }
}
```

Use method-level guards for exceptional routes such as an administrative
operation. Controller-level guards run first, followed by method-level guards
in declaration order. See [Guards and Interceptors](guards-and-interceptors.md)
for the complete lifecycle.

## Session cookies

For browser applications, a guard can read an opaque session identifier from a
cookie instead of a bearer token:

```rust
impl Guard for SessionGuard {
    fn can_activate<'a>(&'a self, ctx: &'a RequestContext) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let Some(session_id) = ctx.cookie("session") else {
                return Err(UnauthorizedException::new("Sign in required"));
            };

            let user = self.sessions.find_user(session_id).await?;
            ctx.set(user)?;
            Ok(true)
        })
    }
}
```

Caelix creates and reads cookies but does not sign, encrypt, rotate, revoke, or
persist sessions. Store opaque, high-entropy session identifiers in an
application-owned session service. Keep `Secure`, `HttpOnly`, and a deliberate
`SameSite` setting on cookies, and add CSRF protection whenever browsers send
an authentication cookie on state-changing requests. See [Cookies and Sessions](../concepts/cookies-and-sessions.md)
for cookie construction and removal rules.

## Authorization belongs with the resource

Authentication establishes _who_ made the request. Authorization decides
whether that identity may perform one operation on one resource. Use a guard
for broad route policy—signed-in user, required role, tenant membership—and use
a service or controller check when the answer depends on the loaded resource.

This prevents a common error: putting an ownership check in a global guard
before the route has loaded the resource. Let `PostsService::delete(user_id,
post_id)` own that database-aware policy when several delivery paths (HTTP,
events, or microservices) must enforce it.

## Document protected endpoints

The `openapi` feature documents security independently from runtime guards.
Register a scheme on `OpenApiConfig`, add `#[security(...)]` to the operation,
and still keep `#[use_guard(...)]` on the route:

```rust
#[security(Security::BearerAuth)]
#[use_guard(AuthGuard)]
#[get("/me")]
async fn me(&self, #[user] user: CurrentUser) -> Result<User> {
    self.users.find(user.id).await
}
```

Swagger UI’s **Authorize** button only describes and sends credentials. It
does not authenticate requests on the server. The full setup is in [OpenAPI and
Swagger UI](openapi.md).

## Security checklist

- Verify token signature, expiry, issuer, audience, and tenant before storing
  `CurrentUser`.
- Return `401` for absent or invalid credentials; use `403` only for an
  authenticated identity that lacks permission.
- Keep raw credentials out of logs, errors, response bodies, and request
  context extensions.
- Use opaque, `Secure`, `HttpOnly` cookies for browser sessions; protect
  cookie-authenticated write requests from CSRF.
- Enforce resource ownership and tenant boundaries in the service that owns the
  write or read policy.
- Test unauthenticated, authenticated-but-forbidden, and permitted requests
  with `TestApplication`.
