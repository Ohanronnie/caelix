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

- Implements `caelix_core::Injectable`.
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
- `#[user]`
- `#[validate]`

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

`#[param]`, `#[query]`, and `#[body]` become Actix `Path<T>`, `Query<T>`, and `Json<T>` extractor parameters in the generated route handler. `#[user]` reads `RequestContext::get::<T>()` and returns `401 Unauthorized` if the typed value is missing.

Invalid extractor pattern:

```rust
#[get("/{id}")]
pub async fn find(&self, #[param] Some(id): Option<i64>) -> Result<String> {
    Ok(id.to_string())
}
```

Extractor arguments must be simple identifiers because the macro moves extracted values into the controller method call.
