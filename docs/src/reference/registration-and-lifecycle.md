# Registration And Lifecycle Order

Module registration follows the metadata graph.

1. Imported modules are registered.
2. Providers are constructed and inserted.
3. Controllers are constructed as providers.
4. Provider metadata is validated.
5. Event handlers are registered after their provider exists.
6. `on_bootstrap` runs after provider validation.

`on_module_init` runs when an injectable provider is registered. `on_shutdown` runs in reverse startup order through the root module shutdown path.

Async factory providers are construction-only and use no-op lifecycle callbacks.

Controllers participate in dependency injection because registering a controller creates a provider definition for that controller type.

## Visibility

The container is shared while modules are registered. Providers from imported modules are available to modules registered later. Providers from the current module are available to controllers in the same module because providers are registered before controllers.

## Failures

Startup can fail for:

- Missing providers declared in module metadata.
- Missing event handler providers.
- Missing `EventModule` import before resolving `Arc<EventBus>` or registering event handlers.
- Async factory errors.
- Lifecycle hook errors.
- Dependency resolution panics during provider construction.

Use `Application::try_new::<AppModule>()` when callers should handle startup errors instead of panics.
