# Typed Configuration

Enable typed configuration with the `config` feature:

```toml
[dependencies]
caelix = { version = "0.0.32", features = ["config"] }
```

## Define a configuration type

Derive `Config`, the re-exported Serde `Deserialize`, and `Validate` on a named,
non-generic struct. Every field must have one case-sensitive
`#[config(env = "NAME")]` mapping. The optional `default` is a textual fallback:

```rust
use caelix::{Config, Deserialize, Validate};

#[derive(Config, Deserialize, Validate)]
pub struct AppConfig {
    #[config(env = "PORT")]
    #[validate(range(min = 1, max = 65535))]
    pub port: u16,

    #[config(env = "DATABASE_URL")]
    pub database_url: String,

    #[config(env = "ENVIRONMENT", default = "development")]
    pub environment: String,

    // Missing OPTIONAL_LABEL becomes None.
    #[config(env = "OPTIONAL_LABEL")]
    pub optional_label: Option<String>,
}

// Associated constants and methods are ordinary Rust. They are not config
// fields, do not need an environment mapping, and can coexist with loaded
// fields on the same type.
impl AppConfig {
    pub const SERVICE_NAME: &'static str = "billing-api";

    pub fn is_production(&self) -> bool {
        self.environment == "production"
    }
}
```

The derive processes only struct fields. Adding another field without
`#[config(...)]` is a compile-time error; associated constants and methods in an
`impl` block are the way to keep literal policy alongside environment-backed
settings.

Each mapped value starts as text. Scalar types (`String`, booleans, and numeric
types), directly declared `Option<T>`, and types accepted by the generated
Serde/plain-text deserializer are supported. Each field is decoded from one
textual value; nested configuration objects are not supported. For a custom
textual representation, attach Serde's `deserialize_with` (or `with`) to the
field:

```rust
fn lowercase<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize as _;
    String::deserialize(deserializer).map(|value| value.to_lowercase())
}

#[derive(Config, Deserialize, Validate)]
pub struct WorkerConfig {
    #[config(env = "WORKER_MODE", default = "SAFE")]
    #[serde(deserialize_with = "lowercase")]
    pub mode: String,
}
```

Serde conversion happens before validation. Conversion and validation errors
identify the Rust field and environment variable, but never include the input
value.

## Register and inject it

Import one `ConfigModule` from the application root:

```rust
use std::sync::Arc;
use caelix::{injectable, ConfigModule, Module, ModuleMetadata};

#[injectable]
pub struct BillingService {
    config: Arc<AppConfig>,
}

pub struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<ConfigModule<AppConfig>>()
            .provider::<BillingService>()
    }
}
```

`ConfigModule<AppConfig>` is global: it constructs and validates `AppConfig`,
exports it, and makes it available as `Arc<AppConfig>` before a listener binds.
Importing the same module more than once still resolves to one module graph
node. Providers in imported modules can inject the type when the config module
is visible to them.

## Sources and precedence

`AppConfig::load()` reads `.env` in the current working directory when that file
exists. A missing default `.env` is allowed. Values are selected in this order:

1. Process environment (`std::env`)
2. The selected environment file
3. The field's `default = "..."`
4. `None` for a directly declared `Option<T>` field

An explicitly empty value is still a value and is not replaced by a default.
The process environment is overlaid in memory; loading never writes file values
back into the process environment.

Call `load_from` when a one-off path is known by application code:

```rust
use caelix::Config;

let config = AppConfig::load_from("/etc/my-service/production.env")?;
```

Unlike `load`, an explicit path must exist and be readable. Process environment
variables still take precedence over entries in that file.

To use an arbitrary path for dependency-injection startup, provide a
`ConfigFile` type as `ConfigModule`'s second parameter:

```rust
use std::path::PathBuf;
use caelix::{ConfigFile, ConfigModule, Module, ModuleMetadata};

pub struct ProductionEnvFile;

impl ConfigFile for ProductionEnvFile {
    fn path() -> Option<PathBuf> {
        Some(PathBuf::from("/etc/my-service/production.env"))
    }
}

pub struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<ConfigModule<AppConfig, ProductionEnvFile>>()
    }
}
```

`ConfigFile::path()` may calculate a path at runtime. Returning `Some(path)`
uses the required-file behavior of `load_from`; returning `None` uses the
optional working-directory `.env` behavior. The source type only selects the
file—`ConfigModule` still owns construction, validation, export, and DI.

## Startup failures and overrides

Configuration is loaded before the application starts listening. The following
conditions fail startup (or return an error from `load_from`):

| Condition                                                   | Result                                                                 |
| ----------------------------------------------------------- | ---------------------------------------------------------------------- |
| Required field has no process value, file value, or default | Missing-field startup error naming the field and env variable          |
| Text cannot be deserialized as the field type               | Conversion startup error naming the field and env variable             |
| `Validate::validate` rejects the completed struct           | Validation startup error listing failed fields and validator codes     |
| Explicit file is missing, unreadable, or malformed          | Environment-file startup error (the default missing `.env` is allowed) |

For tests, seed the container with an instance of the exact configuration type:

```rust
use std::sync::Arc;
use caelix::{ProviderOverrides, build_container_with_overrides};

let overrides = ProviderOverrides::new().insert_instance(AppConfig {
    port: 0,
    database_url: "in-memory".to_owned(),
    environment: "test".to_owned(),
    optional_label: None,
});

let container = build_container_with_overrides::<AppModule>(overrides).await?;
let config: Arc<AppConfig> = container.resolve()?;
assert_eq!(config.environment, "test");
```

An override replaces the provider before its factory runs, so this path does not
read environment files or invoke `Config::load`/derive validation. Construct
values deliberately (or call your validator yourself) and use the same concrete
`AppConfig` type that production providers inject. `TestApplication` exposes the
same operation as `.override_provider(AppConfig { ... })`.
