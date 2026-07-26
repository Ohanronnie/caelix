# Request Throttling

Import `ThrottleModule` to protect every macro-generated controller route with
the global fixed-window policy of 60 requests per 60 seconds:

```rust
impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().import::<ThrottleModule>()
    }
}
```

Counters are isolated by client identity, HTTP method, and the static route
template. The window begins on the first attempt, and rejected attempts are
also counted. Rejections use Caelix's normal 429 JSON exception with the
message `Rate limit exceeded`. Only rejected responses receive `Retry-After`;
set `ThrottleOptions::with_retry_after_header(false)` in a custom
`ThrottleConfig` to omit it. `ThrottleConfig::options` receives the DI
container; declare anything it resolves through `ThrottleConfig::dependencies`.

## Route policies

Apply a policy to a whole controller or one method:

```rust
#[controller("/reports")]
#[throttle(limit = 10, window_seconds = 60)]
impl ReportController {
    #[get("")]
    async fn list(&self) -> Result<Response<Vec<Report>>> { /* ... */ }

    #[get("/health")]
    #[skip_throttle]
    async fn health(&self) -> Result<Response<&'static str>> { /* ... */ }
}
```

A method annotation overrides its controller annotation. A method
`#[throttle(...)]` re-enables a controller marked `#[skip_throttle]`, and a
method `#[skip_throttle]` disables an inherited policy. Both throttle values
must be positive integer literals. An explicit policy requires
`ThrottleModule` to be registered and is validated during startup.

Throttling runs before guards, request-body parsing, interceptors, and the
controller method. Tracker or storage errors stop request execution and are
propagated through the normal exception response path.

## Stores, trackers, and proxies

`MemoryThrottleStore` is process-local and holds at most 100,000 active
buckets by default. Use `MemoryThrottleStore::with_capacity` to change the
bound. Multi-worker or multi-instance applications should implement the
atomic `ThrottleStore` trait with shared storage such as Redis and supply it
through `ThrottleOptions::with_store`.

```rust
impl ThrottleConfig for AppThrottleConfig {
    fn imports() -> Vec<ModuleDef> {
        vec![ModuleDef::of::<RedisModule>()]
    }

    fn dependencies() -> Vec<ProviderDependency> {
        provider_dependencies![RedisPool]
    }

    fn options(container: &Container) -> Result<ThrottleOptions> {
        let pool = container.resolve::<RedisPool>()?;
        Ok(ThrottleOptions::default().with_store(Arc::new(
            RedisThrottleStore::new(pool),
        )))
    }
}
```

Implement `ThrottleTracker` to key quotas by an authenticated user, API key,
or another application identity. Trackers run before guards, so they must
derive that identity directly from request credentials, typically using an
injected authentication service; they cannot depend on extensions populated
by a guard. The built-in `IpThrottleTracker` uses the
immediate socket peer. It trusts `X-Forwarded-For` only when that peer belongs
to a range passed to `IpThrottleTracker::with_trusted_proxies`. It walks a
valid chain from right to left and selects the first untrusted address.
Malformed trusted-proxy chains fail closed; forwarding headers from
untrusted peers are ignored.

Axum applications must serve the router with socket connection information.
`Application::listen` configures this automatically. Custom Axum serving code
must use `into_make_service_with_connect_info::<SocketAddr>()`. Both runtime
test clients default to loopback and expose `peer_addr(...)` on their request
builder.

The global policy covers only Caelix macro-generated controller methods.
Native runtime routes, documentation endpoints, missing-route handlers,
WebSocket upgrades, and WebSocket messages are outside its scope.
