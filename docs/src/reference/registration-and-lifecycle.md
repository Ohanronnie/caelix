# Registration And Lifecycle Order

Module registration follows the metadata graph. Each module is discovered once;
repeated imports share one provider graph and module-import cycles are startup
errors.

1. The complete module graph, exports, duplicates, and dependency metadata are validated.
2. Providers are constructed in dependency order and initialized once.
3. Event handlers are registered after their provider exists.
4. `on_bootstrap` runs in dependency order.

`on_module_init` runs when an injectable provider is registered. `on_shutdown`
runs once in reverse successful startup order. If startup fails, Caelix shuts
down providers already initialized and preserves the original error. Shutdown
continues after individual hook failures and returns the first such error.

Async factory providers are construction-only and use no-op lifecycle callbacks.

Controllers participate in dependency injection because registering a controller creates a provider definition for that controller type.

## Visibility

Providers are visible to their declaring module, to direct importers only when
explicitly exported, and through explicit exports of reachable global modules.
An import does not expose a module's private providers. Re-exporting is allowed
only from a direct import.

## Failures

Startup can fail for:

- Missing providers declared in module metadata.
- Missing event handler providers.
- Missing `EventModule` import before resolving `Arc<EventBus>` or registering event handlers.
- A dependency that is private, unexported, or declared by a module that was not imported.
- A handwritten provider or factory resolving a type absent from its declared dependencies.
- Duplicate production provider, factory, controller, or gateway registrations.
- Async factory errors.
- Lifecycle hook errors.
- Dependency resolution failures during provider construction.

`Application::new::<AppModule>()` returns startup errors as `caelix::Result<Application>`, so callers can choose whether to propagate, map, or unwrap them.
