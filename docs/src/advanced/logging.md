# Logging

Every container provides `Logger`. An `Arc<Logger>` field in an `#[injectable]`
class is resolved specially with the class name as its scope:

```rust
#[caelix::injectable]
struct OrdersService {
    logger: Arc<Logger>,
}

self.logger.info("order accepted");
self.logger.warn("inventory is low");
self.logger.error("payment failed");
self.logger.debug("retry state updated");
```

`Logger::new("scope")`, `Logger::for_type::<T>()`, and `context()` are available
for manual construction. `CAELIX_LOG` selects `error`, `warn`, `info`/`log`, or
`debug`. If absent or invalid, Caelix reads an applicable `caelix`,
`caelix_core`, or `caelix-core` directive from `RUST_LOG`, then defaults to
info. Framework startup, module/provider initialization, route mapping,
listening, shutdown, and internal server failures use the same logger.

## Actix access logs

```rust
Application::new::<AppModule>()
    .await?
    .logging(Logging::default())
    .listen("127.0.0.1:3000")
    .await?;
```

`Logging::default()` enables compact method, path, status, and duration logs.
`Logging::info()` enables the detailed Actix-compatible format: peer address,
request line/protocol, status, response size, referrer, user agent, and duration.
Use `.access_log(false)` to disable middleware completely.

Explicit `.logging(...)` wins. If it is omitted, `CAELIX_HTTP_LOG` is checked,
then the legacy `CAELIX_ACCESS_LOG`; recognized boolean values decide whether
compact access logging is enabled. The default fallback is disabled.

Request workers enqueue access entries to a dedicated buffered writer. The
queue capacity is 65,536; when full, new entries are dropped instead of blocking
requests. Monitor `dropped_http_request_logs()`. Caelix reports drops to stderr
at most once per second. Buffered responses report their byte count; a streaming
response whose final size is unknown logs `-`.

## Axum

Axum has the scoped application/framework `Logger`, but does not expose Actix's
`Logging` application configuration or install this access-log middleware.
Attach a Tower HTTP tracing/access-log layer with `Application::layer` when
Axum request logs are required.
