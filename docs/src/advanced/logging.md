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
debug. Framework startup, module/provider initialization, route mapping,
readiness, shutdown, and internal server failures use the same logger.

Generated controller routes include correlation metadata on every logged
server error:

```text
[2026-07-26T14:32:08.190Z ERROR caelix::error] 500 Internal Server Error: Internal Server Error | source: database unavailable | method=GET path="/orders" request_id="9e285a20-9405-4cc8-8941-0f758d84f524" trace_id="4bf92f3577b34da6a3ce929d0e0e4736" service="orders-api" environment="production"
```

Caelix accepts `X-Request-Id` and `X-Trace-Id`, and understands the trace ID
inside a W3C `traceparent` header. Missing request IDs are generated as UUIDs;
invalid correlation headers are ignored. `X-Request-Id` and `X-Trace-Id` are
included on the response so clients can report them.

Set `CAELIX_SERVICE_NAME` and `CAELIX_ENVIRONMENT` to identify the deployment.
`OTEL_SERVICE_NAME` is also accepted for the service name; `APP_ENV` and
`ENVIRONMENT` are environment fallbacks. Defaults are `application` and
`development`.

Default startup output is intentionally compact:

```text
[2026-07-26T14:32:08.184Z INFO  caelix::bootstrap] Building application
[2026-07-26T14:32:08.186Z INFO  caelix::routes] 3 routes registered across 1 controller
[2026-07-26T14:32:08.187Z INFO  caelix::server] Listening on http://127.0.0.1:3000 in 12ms
```

The `ready` event is written only after the server successfully binds.
Individual module, provider, controller, and route events are included by
default. Set `CAELIX_LOG=info` for compact startup output. In-process
`TestApplication` compilation does not emit server startup events.

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
