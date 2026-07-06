# Agent Instructions

- When a feature is suggested or agreed on, build it directly. Do not turn it into staged language such as "v1", "v2", "MVP", or a separate build plan unless the user explicitly asks for planning.
- Cache support is explicit service-level caching. Do not add automatic HTTP response caching; users can implement that themselves with an interceptor that reads from and writes to the cache.
