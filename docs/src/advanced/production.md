# Production Operations

Caelix applications start as one explicit module graph and one HTTP runtime.
Production readiness comes from configuring that graph deliberately: bind the
right address, bound request sizes and concurrency, make logs actionable, and
provide application-owned health checks and deployment configuration.

## Build and run

Build a release binary, provide configuration through the deployment
environment, and run the binary directly or in your platform’s container
image:

```sh
cargo build --release
./target/release/my-service
```

Keep the runtime adapter choice and all Caelix features in `Cargo.toml`. The
binary does not discover modules or configuration files implicitly; startup
builds the `AppModule` graph and returns a failure before listening if a
provider, import, lifecycle hook, or application configuration is invalid.

## Configure the HTTP runtime

Set limits before calling `listen`. The worker setting is Actix-specific:

```rust
use caelix::Application;

Application::new::<AppModule>()
    .await?
    .workers(8)
    .body_limit(2 * 1024 * 1024)
    .upload_temp_dir("/var/tmp/my-service/uploads")
    .listen("0.0.0.0:8080")
    .await?;
```

`body_limit` applies to JSON and multipart controller routes. Use the smallest
limit that supports the API, and set a writable, isolated temporary directory
when uploads are enabled. Actix uses one worker by default; size `.workers(n)`
to the process CPU allocation and workload. Axum runtime configuration and
Tower integration are covered in [Axum and Tower](axum.md).

## Health and readiness endpoints

Caelix does not reserve or create a health endpoint. Add application-owned
routes so their meaning matches the service:

```rust
use caelix::{Response, Result, StatusCode, controller, injectable};

#[injectable]
pub struct HealthController;

#[controller("")]
impl HealthController {
    #[get("/healthz")]
    async fn health(&self) -> Result<Response<()>> {
        Ok(Response::text(StatusCode::OK, "ok"))
    }

    #[get("/readyz")]
    async fn ready(&self) -> Result<Response<()>> {
        // Check dependencies that must be available before taking traffic.
        Ok(Response::text(StatusCode::OK, "ready"))
    }
}
```

Register `HealthController` in an application module like every other
controller. Keep these routes inexpensive and make their exposure intentional;
some deployments expose them only to the load balancer or cluster network.

Use a liveness endpoint only to show that the process can make progress. Make
readiness reflect dependencies that must be available to accept traffic, such
as a required database or broker. Do not expose credentials, internal topology,
or expensive diagnostics from public health routes.

## Configure logs and tracing

Set `CAELIX_SERVICE_NAME` and `CAELIX_ENVIRONMENT` for deployment identity.
`CAELIX_LOG` controls framework and application log verbosity. Add native
Actix or Tower/OpenTelemetry middleware when the deployment requires request
identifiers or distributed tracing.

For Actix access logs, opt in explicitly with `Logging::default()` or
`Logging::info()`. For Axum, attach the Tower layer that matches your tracing
and access-log stack. See [Logging](logging.md) for the exact environment
variables and adapter behavior.

## Reverse proxies and TLS

Terminate TLS at the application only when your deployment requires it;
otherwise terminate it at a managed load balancer or reverse proxy and forward
HTTP to the Caelix listener. Treat forwarded headers as infrastructure input:
configure trusted-proxy handling explicitly before using an address derived from
them for security or throttling. The built-in IP throttle tracker documents the
trusted-proxy model in [Request Throttling](throttling.md).

Keep cookie `Secure` enabled in production. If a reverse proxy changes a path,
host, or scheme, test authentication cookies and generated OpenAPI URLs through
the public route, not only against the local listener.

## Shutdown and rolling deployments

`Application::listen` runs provider shutdown hooks after the server exits.
Design long-running providers so `on_shutdown` closes external clients,
stops background work, and completes quickly. For microservices, shutdown also
stops intake and waits for configured transport work before lifecycle hooks.

Your process manager should remove an instance from load balancing before its
termination deadline. Keep request, database, broker, and reverse-proxy timeouts
compatible so in-flight work has a clear owner during a rollout.

## Operational checklist

- Bind to the deployment network interface and expose only the intended port.
- Set CPU, memory, body-size, upload-storage, and database-pool bounds.
- Supply secrets through the platform secret store or environment, not source
  code or generated configuration files.
- Expose authenticated application metrics through your chosen observability
  stack; Caelix does not impose a metrics backend.
- Monitor non-2xx rate, latency, process memory, file-system capacity for
  uploads, database/broker capacity, and request IDs from client reports.
- Run database migrations and broker topology changes as controlled deployment
  operations.
- Exercise startup failure, graceful shutdown, and health/readiness behavior in
  the same deployment environment used for releases.
