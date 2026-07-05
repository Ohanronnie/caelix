# Caelix Decisions

## Interceptors vs Middleware

- Interceptors are framework-neutral and live in `caelix-core`.
- `Interceptor` works on `RequestContext` plus the already-converted `HttpResponse`, so it can inspect or transform responses without knowing the handler's original return type.
- `#[use_interceptor(A)]` above `#[use_interceptor(B)]` uses onion order: `A` runs before `B`, then the handler, then `B` after logic, then `A` after logic.
- Middleware remains adapter-specific for now. The Actix layer can keep using native `.wrap()` / `.wrap_fn()` in `Application::listen` instead of introducing a fake-neutral middleware trait before there is a second backend.
