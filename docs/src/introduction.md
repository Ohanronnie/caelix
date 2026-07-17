# Caelix

Caelix is a Rust web framework layer built around modules, dependency injection, controllers, guards, interceptors, lifecycle hooks, domain events, and explicit service-level caching.

The public package is:

- `caelix`: the framework crate that exports the runtime, traits, macros, and Actix application entry point.

The fastest way to start an app is:

```sh
cargo install caelix-cli
caelix new demo-api
cd demo-api
cargo run
```

Generated applications depend on `caelix = "0.0.23"` from crates.io. The generator refuses to overwrite existing files, so it is safe to run against a feature name and stop when a generated file already exists.
