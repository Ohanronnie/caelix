# Agent Instructions

- Before making non-trivial code changes, read `AI_REFERENCE.md` for the workspace structure, architecture patterns, and project conventions.
- When a feature is suggested or agreed on, build it directly. Do not turn it into staged language such as "v1", "v2", "MVP", or a separate build plan unless the user explicitly asks for planning.
- Build new features in Caelix's established object-oriented, dependency-injection, NestJS-inspired style. Use classes/structs with clear responsibilities, injectable services, modules, controllers, and explicit registration; do not introduce primarily functional-programming syntax or patterns. This applies to every feature, including WebSockets and future transports or integrations.
