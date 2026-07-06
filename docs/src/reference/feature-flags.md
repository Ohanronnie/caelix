# Feature Flags

`caelix` enables its current integrations by default:

```toml
caelix = "0.0.2"
```

The default features include:

- `actix`: enables `Application`, `#[caelix::main]`, and Actix Web runtime support.
- `sqlx`: enables SQLx-related error conversion support when available.
- `validator`: enables request validation support used by `#[validate]`.

Generated applications depend directly on `caelix`; they do not need additional Caelix package dependencies.

Example generated dependencies:

```toml
[dependencies]
actix-web = "4.14.0"
caelix = "0.0.2"
serde = { version = "1.0.228", features = ["derive"] }
```
