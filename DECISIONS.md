# Caelix Decisions

## Interceptors vs Middleware

- Interceptors are framework-neutral and live in `caelix-core`.
- `Interceptor` works on `RequestContext` plus the already-converted `HttpResponse`, so it can inspect or transform responses without knowing the handler's original return type.
- `#[use_interceptor(A)]` above `#[use_interceptor(B)]` uses onion order: `A` runs before `B`, then the handler, then `B` after logic, then `A` after logic.
- Middleware remains adapter-specific for now. The Actix layer can keep using native `.wrap()` / `.wrap_fn()` in `Application::listen` instead of introducing a fake-neutral middleware trait before there is a second backend.

## Provider Lifecycle Hooks

- Lifecycle hooks live as default methods on `Injectable`: `on_module_init`, `on_bootstrap`, and `on_shutdown`.
- `#[injectable]` only generates `create`; hook defaults are inherited unless a hand-written `impl Injectable` overrides them.
- Normal providers still register with `.provider::<T>()`; no separate lifecycle registration path is needed.
- Async factory providers keep their existing construction-only behavior and receive no-op lifecycle callbacks. Providers that need lifecycle logic should implement `Injectable` directly and use `.provider::<T>()`.
- `register_module` runs `on_module_init`, `build_container` runs `on_bootstrap` after provider validation, and `shutdown_module::<M>(&container)` runs `on_shutdown` in reverse startup order.
