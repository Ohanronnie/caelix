#![cfg(feature = "config")]

use caelix::{
    BoxFuture, Config, ConfigFile, ConfigModule, Container, Deserialize, Injectable, Module,
    ModuleMetadata, ProviderDependency, ProviderOverrides, Result, build_container,
    build_container_with_overrides, provider_dependencies,
};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};
use validator::Validate;

fn deserialize_lowercase<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize as _;
    String::deserialize(deserializer).map(|value| value.to_lowercase())
}

#[derive(Debug, Config, Deserialize, Validate)]
struct AppConfig {
    #[config(env = "CAELIX_TEST_PORT")]
    #[validate(range(min = 1, max = 65535))]
    port: u16,

    #[config(env = "CAELIX_TEST_DATABASE_URL")]
    database_url: String,

    #[config(env = "CAELIX_TEST_ENVIRONMENT", default = "development")]
    environment: String,

    #[config(env = "CAELIX_TEST_MODE", default = "SAFE")]
    #[serde(deserialize_with = "deserialize_lowercase")]
    mode: String,

    #[config(env = "CAELIX_TEST_OPTIONAL_LABEL")]
    optional_label: Option<String>,
}

#[derive(Debug, Config, Deserialize, Validate)]
struct MultipleValidationConfig {
    #[config(env = "CAELIX_TEST_FIRST_PORT", default = "0")]
    #[validate(range(min = 1, max = 65535))]
    first_port: u32,

    #[config(env = "CAELIX_TEST_SECOND_PORT", default = "70000")]
    #[validate(range(min = 1, max = 65535))]
    second_port: u32,
}

struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().import::<FeatureModule>()
    }
}

struct FeatureService {
    config: Arc<AppConfig>,
}

impl Injectable for FeatureService {
    fn create(container: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async move {
            Ok(Self {
                config: container.resolve()?,
            })
        })
    }

    fn dependencies() -> Vec<ProviderDependency> {
        provider_dependencies![AppConfig]
    }
}

struct FeatureModule;

impl Module for FeatureModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<ConfigModule<AppConfig>>()
            .import::<ConfigModule<AppConfig>>()
            .provider::<FeatureService>()
    }
}

static EXPLICIT_MODULE_PATH: OnceLock<PathBuf> = OnceLock::new();

struct ExplicitFileModule;

impl Module for ExplicitFileModule {
    fn register() -> ModuleMetadata {
        struct Source;
        impl ConfigFile for Source {
            fn path() -> Option<PathBuf> {
                EXPLICIT_MODULE_PATH.get().cloned()
            }
        }
        ModuleMetadata::new().import::<ConfigModule<AppConfig, Source>>()
    }
}

fn temporary_env(contents: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "caelix-config-{}-{}.env",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&path, contents).unwrap();
    path
}

fn environment_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn loads_an_arbitrary_file_and_defaults() {
    let _guard = environment_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let path =
        temporary_env("CAELIX_TEST_PORT=4100\nCAELIX_TEST_DATABASE_URL=postgres://localhost/app\n");
    let config = AppConfig::load_from(&path).unwrap();
    fs::remove_file(path).unwrap();

    assert_eq!(config.port, 4100);
    assert_eq!(config.database_url, "postgres://localhost/app");
    assert_eq!(config.environment, "development");
    assert_eq!(config.mode, "safe");
    assert_eq!(config.optional_label, None);
}

#[test]
fn process_environment_overlays_the_file() {
    let _guard = environment_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = temporary_env(
        "CAELIX_TEST_PORT=4100\nCAELIX_TEST_DATABASE_URL=postgres://localhost/file\n",
    );
    // SAFETY: this test serializes all environment mutation in this test module.
    unsafe { std::env::set_var("CAELIX_TEST_PORT", "4200") };
    let config = AppConfig::load_from(&path).unwrap();
    // SAFETY: protected by the same process-wide test lock.
    unsafe { std::env::remove_var("CAELIX_TEST_PORT") };
    fs::remove_file(path).unwrap();

    assert_eq!(config.port, 4200);
}

#[test]
fn diagnostics_are_field_specific_and_secret_safe() {
    let _guard = environment_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let missing = temporary_env("CAELIX_TEST_PORT=4100\n");
    let error = AppConfig::load_from(&missing).unwrap_err();
    fs::remove_file(missing).unwrap();
    assert!(error.message.contains("database_url"));
    assert!(error.message.contains("CAELIX_TEST_DATABASE_URL"));

    let invalid =
        temporary_env("CAELIX_TEST_PORT=not-a-port\nCAELIX_TEST_DATABASE_URL=super-secret-value\n");
    let error = AppConfig::load_from(&invalid).unwrap_err();
    fs::remove_file(invalid).unwrap();
    assert!(error.message.contains("port"));
    assert!(error.message.contains("CAELIX_TEST_PORT"));
    assert!(!error.message.contains("not-a-port"));
    assert!(!error.message.contains("super-secret-value"));
}

#[test]
fn validates_before_provider_construction_completes() {
    let _guard = environment_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // SAFETY: environment mutation is serialized for this test module.
    unsafe {
        std::env::set_var("CAELIX_TEST_PORT", "0");
        std::env::set_var(
            "CAELIX_TEST_DATABASE_URL",
            "postgres://localhost/application",
        );
    }
    let result = futures::executor::block_on(build_container::<AppModule>());
    // SAFETY: protected by the same process-wide test lock.
    unsafe {
        std::env::remove_var("CAELIX_TEST_PORT");
        std::env::remove_var("CAELIX_TEST_DATABASE_URL");
    }

    let error = result
        .err()
        .expect("invalid configuration must fail startup");
    assert!(error.message.contains("port"));
}

#[test]
fn reports_multiple_validation_fields_without_values() {
    let path = temporary_env("");
    let error = MultipleValidationConfig::load_from(&path).unwrap_err();
    fs::remove_file(path).unwrap();
    assert!(error.message.contains("first_port"));
    assert!(error.message.contains("second_port"));
    assert!(!error.message.contains("70000"));
}

#[test]
fn selected_runtime_builds_configuration_before_listening() {
    let _guard = environment_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // SAFETY: environment mutation is serialized for this test module.
    unsafe {
        std::env::set_var("CAELIX_TEST_PORT", "4400");
        std::env::set_var("CAELIX_TEST_DATABASE_URL", "postgres://localhost/runtime");
    }
    let application = futures::executor::block_on(caelix::Application::new::<AppModule>());
    // SAFETY: protected by the same process-wide test lock.
    unsafe {
        std::env::remove_var("CAELIX_TEST_PORT");
        std::env::remove_var("CAELIX_TEST_DATABASE_URL");
    }
    assert!(application.is_ok());
}

#[test]
fn provider_override_bypasses_environment_loading() {
    let overrides = ProviderOverrides::new().insert_instance(AppConfig {
        port: 8080,
        database_url: "in-memory".to_owned(),
        environment: "test".to_owned(),
        mode: "safe".to_owned(),
        optional_label: None,
    });
    let container =
        futures::executor::block_on(build_container_with_overrides::<AppModule>(overrides))
            .unwrap();
    let config: Arc<AppConfig> = container.resolve().unwrap();
    assert_eq!(config.port, 8080);
    let feature: Arc<FeatureService> = container.resolve().unwrap();
    assert_eq!(feature.config.environment, "test");
}

#[test]
fn arbitrary_file_can_drive_module_startup() {
    let _guard = environment_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = temporary_env(
        "CAELIX_TEST_PORT=4300\nCAELIX_TEST_DATABASE_URL=postgres://localhost/module\n",
    );
    EXPLICIT_MODULE_PATH
        .set(path.clone())
        .expect("the explicit module path is configured once");
    let container = futures::executor::block_on(build_container::<ExplicitFileModule>()).unwrap();
    fs::remove_file(path).unwrap();
    let config: Arc<AppConfig> = container.resolve().unwrap();
    assert_eq!(config.port, 4300);
}

#[test]
fn an_explicit_missing_file_is_an_error() {
    let path = std::env::temp_dir().join("caelix-this-env-file-does-not-exist");
    let error = AppConfig::load_from(path).unwrap_err();
    assert!(error.message.contains("failed to read environment file"));
}

#[test]
fn malformed_files_fail_without_exposing_values() {
    let _guard = environment_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = temporary_env("CAELIX_TEST_PORT='unterminated-secret\n");
    let error = AppConfig::load_from(&path).unwrap_err();
    fs::remove_file(path).unwrap();
    assert!(error.message.contains("failed to parse environment file"));
    assert!(!error.message.contains("unterminated-secret"));
}

#[cfg(unix)]
#[test]
fn unreadable_files_fail_startup() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = environment_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = temporary_env("CAELIX_TEST_PORT=4100\n");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    let result = AppConfig::load_from(&path);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::remove_file(path).unwrap();

    assert!(result.is_err(), "an unreadable environment file must fail");
}

#[test]
fn a_missing_default_file_is_accepted() {
    let _guard = environment_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let original = std::env::current_dir().unwrap();
    let directory =
        std::env::temp_dir().join(format!("caelix-config-directory-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    std::env::set_current_dir(&directory).unwrap();
    let error = AppConfig::load().unwrap_err();
    std::env::set_current_dir(original).unwrap();
    fs::remove_dir(directory).unwrap();
    assert!(
        error
            .message
            .contains("missing required configuration field")
    );
}

#[test]
fn empty_string_values_are_preserved() {
    let _guard = environment_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = temporary_env("CAELIX_TEST_PORT=4100\nCAELIX_TEST_DATABASE_URL=\n");
    let config = AppConfig::load_from(&path).unwrap();
    fs::remove_file(path).unwrap();
    assert!(config.database_url.is_empty());
}
