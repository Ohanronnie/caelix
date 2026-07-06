# Feature Flags

The top-level `caelix` crate exposes the `actix` feature to re-export the Actix runtime macro:

```toml
caelix = { path = "../crates/caelix", features = ["actix"] }
```

`caelix-core` exposes optional integration features:

- `sqlx`: enables SQLx-related error conversion support when available.
- `validator`: enables request validation support used by `#[validate]`.

Generated applications depend on `caelix` with `features = ["actix"]`, plus direct path dependencies on `caelix-core` and `caelix-actix`.

Example generated dependencies:

```toml
[dependencies]
actix-web = "4.14.0"
caelix = { path = "../crates/caelix", features = ["actix"] }
caelix-core = { path = "../crates/caelix-core" }
caelix-actix = { path = "../crates/caelix-actix" }
serde = { version = "1.0.228", features = ["derive"] }
```
