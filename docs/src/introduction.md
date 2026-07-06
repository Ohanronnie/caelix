# Caelix

Caelix is a Rust web framework layer built around modules, dependency injection, controllers, guards, interceptors, lifecycle hooks, domain events, and explicit service-level caching.

The public workspace crates are:

- `caelix`: the public framework crate.
- `caelix-core`: framework-neutral traits and runtime types.
- `caelix-actix`: the Actix Web adapter and `Application`.
- `caelix-macros`: `#[injectable]`, `#[guard]`, and `#[controller]`.
- `caelix-cli`: the `caelix` project and feature generator.

The fastest way to start an app from this repository is:

```sh
cargo run -p caelix-cli -- new demo-api --caelix-path .
cd demo-api
cargo run
```

Generated applications use path dependencies back to the local Caelix workspace. The generator refuses to overwrite existing files, so it is safe to run against a feature name and stop when a generated file already exists.
