# Design Decisions

Caelix keeps framework-neutral concepts separate from adapter-specific HTTP server behavior internally, while the public `caelix` package exports the application-facing API.

Interceptors are framework-neutral. They receive `RequestContext`, `Next`, and the already converted `HttpResponse`, so they can inspect or transform responses without depending on a handler's original return type.

`HttpResponse` body is either buffered bytes or a streaming `Stream` of `Bytes` chunks (`ResponseBody`). This is a breaking change from `body: Vec<u8>` (acceptable on early 0.0.x; migrate with `body_bytes()` / `as_buffered_mut()`). Optional headers live on `HttpResponse.headers` as owned `(String, String)` pairs (dynamic values allowed; not a full `HeaderMap` API). SSE and file downloads are thin helpers on the same primitive — SSE is data-frame framing plus cache/proxy headers, not the full SSE protocol. Adapters (today Actix) branch on `ResponseBody` at the boundary — buffered uses `.body(...)`, streaming uses `.streaming(...)`. Streaming handlers return `Result<HttpResponse>` via the existing identity `IntoCaelixResponse` impl; no separate controller attribute is required. Interceptors may rewrite buffered bodies; streaming bodies stay opaque after the handler returns.

`EventBus::emit` runs registered handlers first; only on full success does it publish to `subscribe` streams. Broadcast channels are created by `subscribe`, not by `emit`.

Middleware remains adapter-specific. The Actix layer can continue to use native Actix middleware without adding a framework-neutral middleware abstraction before another backend exists.

Lifecycle hooks live on `Injectable`. Normal providers use `.provider::<T>()`; async factory providers keep construction-only behavior.

Events are opt-in through `EventModule`, which registers `EventBus`. Event handlers must be providers before being registered as handlers.

Cache support is explicit service-level caching through `Cache`, `CacheStore`, `MemoryCacheStore`, and `CacheModule`.
## Cookies are explicit response data

Request cookies are parsed into `RequestContext`, while response cookies live
in a dedicated ordered collection on `HttpResponse`. This keeps controllers
runtime-neutral and preserves multiple `Set-Cookie` headers without changing
the intentionally simple generic response-header collection. Caelix does not
track cookie mutations or provide an application session store.
