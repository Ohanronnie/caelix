# Request Context

`RequestContext` contains the request method, path, headers, bearer token helper, and typed extensions.

```rust
let method = ctx.method();
let path = ctx.path();
let request_id = ctx.header("x-request-id");
let token = ctx.bearer_token();
```

Header lookup is case-insensitive. Caelix lowercases header names when the context is created.

`bearer_token()` reads the `Authorization` header and strips the exact `Bearer ` prefix:

```text
Authorization: Bearer eyJhbGciOi...
```

Guards and interceptors can attach typed values:

```rust
#[derive(Clone)]
pub struct CurrentUser {
    pub id: i64,
}

ctx.set(CurrentUser { id: 42 });
```

Typed values are stored by concrete Rust type. A later `ctx.set::<CurrentUser>(...)` replaces the earlier value for that type.

Guards and interceptors can read values back:

```rust
if let Some(user) = ctx.get::<CurrentUser>() {
    tracing::info!("user {}", user.id);
}
```

Controllers can read a typed value with `#[user]`:

```rust
#[get("/me")]
pub async fn me(&self, #[user] user: CurrentUser) -> Result<String> {
    Ok(user.id.to_string())
}
```

If the typed value is missing, the generated wrapper returns `401 Unauthorized` with message `Not authenticated`.

`#[user]` clones the value out of the context, so user types must be cloneable when used this way.
