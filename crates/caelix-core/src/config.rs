use crate::{Injectable, Module, ModuleMetadata, Result};
use std::{
    collections::HashMap,
    marker::PhantomData,
    path::{Path, PathBuf},
};
use validator::ValidationErrors;

/// Describes one field handled by a derived [`Config`] implementation.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct ConfigField {
    /// Rust field name used for diagnostics and deserialization.
    pub field: &'static str,
    /// Process environment variable supplying the field.
    pub env: &'static str,
    /// Optional textual fallback.
    pub default: Option<&'static str>,
    /// Whether absence is represented as `None`.
    pub optional: bool,
}

/// A typed, validated application configuration provider.
pub trait Config: Injectable + Sized {
    /// Loads configuration from `./.env`, then overlays process environment.
    fn load() -> Result<Self>;

    /// Loads configuration from an explicit environment file, then overlays
    /// process environment.
    ///
    /// Unlike [`Config::load`], the explicitly selected file must exist.
    fn load_from(path: impl AsRef<Path>) -> Result<Self>;
}

/// Selects the environment file used by [`ConfigModule`].
pub trait ConfigFile: Send + Sync + 'static {
    /// Returns an explicit file path, or `None` to use the optional
    /// working-directory `.env`.
    fn path() -> Option<PathBuf>;
}

/// The default optional working-directory `.env` source.
pub struct WorkingDirectoryEnvFile;

impl ConfigFile for WorkingDirectoryEnvFile {
    fn path() -> Option<PathBuf> {
        None
    }
}

/// A global module that constructs and exports a typed configuration.
///
/// Supply a custom [`ConfigFile`] as the second type parameter when application
/// startup should use an arbitrary environment-file location.
pub struct ConfigModule<T: Config, F: ConfigFile = WorkingDirectoryEnvFile>(PhantomData<(T, F)>);

impl<T: Config, F: ConfigFile> Module for ConfigModule<T, F> {
    fn register() -> ModuleMetadata {
        ModuleMetadata::global()
            .provider_async_factory::<T, _, _>(Vec::new(), |_| async {
                match F::path() {
                    Some(path) => T::load_from(path),
                    None => T::load(),
                }
            })
            .export::<T>()
    }
}

/// Loads and validates a configuration for derive-generated code.
#[doc(hidden)]
pub fn __load_config_values(
    fields: &'static [ConfigField],
    path: Option<&Path>,
) -> Result<Vec<String>> {
    let mut values = HashMap::<String, String>::new();
    let env_path = path
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".env"));

    match dotenvy::from_path_iter(&env_path) {
        Ok(iter) => {
            for entry in iter {
                let (name, value) = entry.map_err(|_| {
                    crate::exception::startup_error(format!(
                        "failed to parse environment file {}",
                        env_path.display()
                    ))
                })?;
                values.insert(name, value);
            }
        }
        Err(error) if path.is_none() && error.not_found() => {}
        Err(error) => {
            return Err(crate::exception::startup_error(format!(
                "failed to read environment file {}: {error}",
                env_path.display()
            )));
        }
    }

    let mut deserialize_values = Vec::with_capacity(fields.len());
    for field in fields {
        let process_value = match std::env::var(field.env) {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(crate::exception::startup_error(format!(
                    "configuration field `{}` (environment variable `{}`) is not valid Unicode",
                    field.field, field.env
                )));
            }
        };
        let value = process_value
            .or_else(|| values.get(field.env).cloned())
            .or_else(|| field.default.map(str::to_owned))
            .or_else(|| field.optional.then(String::new))
            .ok_or_else(|| {
                crate::exception::startup_error(format!(
                    "missing required configuration field `{}` (environment variable `{}`)",
                    field.field, field.env
                ))
            })?;
        deserialize_values.push(value);
    }
    Ok(deserialize_values)
}

/// Converts one textual environment value through Serde.
#[doc(hidden)]
pub fn __deserialize_config_field<T>(field: &str, env: &str, value: String) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_plain::from_str(&value).map_err(|_| __config_conversion_error(field, env))
}

/// Creates a secret-safe scalar conversion error for derive-generated code.
#[doc(hidden)]
pub fn __config_conversion_error(field: &str, env: &str) -> crate::HttpException {
    crate::exception::startup_error(format!(
        "failed to convert configuration field `{field}` (environment variable `{env}`)"
    ))
}

/// Converts validator errors into secret-safe startup diagnostics.
#[doc(hidden)]
pub fn __config_validation_error(errors: &ValidationErrors) -> crate::HttpException {
    let mut fields: Vec<_> = errors
        .field_errors()
        .into_iter()
        .map(|(field, errors)| {
            let codes = errors
                .iter()
                .map(|error| error.code.as_ref())
                .collect::<Vec<_>>()
                .join(", ");
            format!("`{field}` ({codes})")
        })
        .collect();
    fields.sort();
    crate::exception::startup_error(format!(
        "configuration validation failed for {}",
        fields.join("; ")
    ))
}
