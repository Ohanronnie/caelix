# Design Decisions

Caelix keeps framework-neutral concepts separate from adapter-specific HTTP server behavior internally, while the public `caelix` package exports the application-facing API.

Interceptors are framework-neutral. They receive `RequestContext`, `Next`, and the already converted `HttpResponse`, so they can inspect or transform responses without depending on a handler's original return type.

Middleware remains adapter-specific. The Actix layer can continue to use native Actix middleware without adding a framework-neutral middleware abstraction before another backend exists.

Lifecycle hooks live on `Injectable`. Normal providers use `.provider::<T>()`; async factory providers keep construction-only behavior.

Events are opt-in through `EventModule`, which registers `EventBus`. Event handlers must be providers before being registered as handlers.

Cache support is explicit service-level caching through `Cache`, `CacheStore`, `MemoryCacheStore`, and `CacheModule`.
