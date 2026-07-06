use std::{
    env, fmt, fs, io,
    path::{Path, PathBuf},
};

use clap::{Args, Parser, Subcommand};
use heck::{ToKebabCase, ToPascalCase, ToSnakeCase};

pub type Result<T> = std::result::Result<T, CliError>;

#[derive(Debug)]
pub enum CliError {
    Io { path: PathBuf, source: io::Error },
    AlreadyExists(PathBuf),
    InvalidName(String),
    MissingCaelixPath,
    InvalidCaelixPath(PathBuf),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::AlreadyExists(path) => {
                write!(
                    f,
                    "{} already exists; refusing to overwrite",
                    path.display()
                )
            }
            Self::InvalidName(name) => write!(f, "invalid project or feature name `{name}`"),
            Self::MissingCaelixPath => write!(
                f,
                "could not find a Caelix workspace; pass --caelix-path <path>"
            ),
            Self::InvalidCaelixPath(path) => write!(
                f,
                "{} is not a Caelix workspace root with crates/caelix, crates/caelix-core, and crates/caelix-actix",
                path.display()
            ),
        }
    }
}

impl std::error::Error for CliError {}

#[derive(Parser, Debug)]
#[command(
    name = "caelix",
    version,
    about = "Generate Caelix applications and features"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    New(NewArgs),
    #[command(alias = "g")]
    Generate(GenerateArgs),
}

#[derive(Args, Debug)]
struct NewArgs {
    name: String,
    #[arg(long, value_name = "PATH")]
    caelix_path: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct GenerateArgs {
    #[command(subcommand)]
    kind: GenerateKind,
}

#[derive(Subcommand, Debug)]
enum GenerateKind {
    Service(NameArgs),
    Controller(NameArgs),
    Module(NameArgs),
}

#[derive(Args, Debug)]
struct NameArgs {
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureName {
    raw: String,
    module_name: String,
    route_path: String,
    type_prefix: String,
}

impl FeatureName {
    pub fn parse(name: impl Into<String>) -> Result<Self> {
        let raw = name.into();
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.contains(['/', '\\']) {
            return Err(CliError::InvalidName(raw));
        }

        let module_name = trimmed.to_snake_case();
        let route_path = trimmed.to_kebab_case();
        let type_prefix = trimmed.to_pascal_case();

        if module_name.is_empty()
            || route_path.is_empty()
            || type_prefix.is_empty()
            || module_name
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_digit())
        {
            return Err(CliError::InvalidName(trimmed.to_string()));
        }

        Ok(Self {
            raw: trimmed.to_string(),
            module_name,
            route_path,
            type_prefix,
        })
    }

    pub fn module_name(&self) -> &str {
        &self.module_name
    }

    pub fn route_path(&self) -> &str {
        &self.route_path
    }

    pub fn service_type(&self) -> String {
        format!("{}Service", self.type_prefix)
    }

    pub fn controller_type(&self) -> String {
        format!("{}Controller", self.type_prefix)
    }

    pub fn module_type(&self) -> String {
        format!("{}Module", self.type_prefix)
    }
}

pub fn run_from_env() -> Result<String> {
    let cwd = env::current_dir().map_err(|source| CliError::Io {
        path: PathBuf::from("."),
        source,
    })?;
    run_from(env::args_os(), cwd)
}

pub fn run_from<I, T>(args: I, cwd: impl AsRef<Path>) -> Result<String>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    run(cli, cwd.as_ref())
}

fn run(cli: Cli, cwd: &Path) -> Result<String> {
    match cli.command {
        Command::New(args) => generate_new(args, cwd),
        Command::Generate(args) => match args.kind {
            GenerateKind::Service(args) => generate_service(&FeatureName::parse(args.name)?, cwd),
            GenerateKind::Controller(args) => {
                generate_controller(&FeatureName::parse(args.name)?, cwd)
            }
            GenerateKind::Module(args) => generate_module(&FeatureName::parse(args.name)?, cwd),
        },
    }
}

fn generate_new(args: NewArgs, cwd: &Path) -> Result<String> {
    let target_dir = cwd.join(&args.name);
    ensure_missing(&target_dir)?;

    let package_name = package_name_for_path(&target_dir, &args.name)?;
    let crate_name = package_name.to_snake_case();
    let caelix_root = resolve_caelix_root(args.caelix_path.as_deref(), cwd)?;

    fs::create_dir_all(target_dir.join("src")).map_err(|source| CliError::Io {
        path: target_dir.join("src"),
        source,
    })?;

    let cargo_toml = render_app_cargo_toml(&package_name, &target_dir, &caelix_root);
    create_file(target_dir.join("Cargo.toml"), &cargo_toml)?;
    create_file(target_dir.join("src/main.rs"), &render_main_rs(&crate_name))?;
    create_file(target_dir.join("src/lib.rs"), render_lib_rs())?;
    create_file(target_dir.join("src/app.rs"), render_app_rs())?;

    Ok(format!(
        "Created Caelix application `{}` in {}\n\nNext steps:\n- cd {}\n- cargo run\n",
        package_name,
        target_dir.display(),
        target_dir.display()
    ))
}

fn generate_service(feature: &FeatureName, cwd: &Path) -> Result<String> {
    let feature_dir = src_dir(cwd).join(feature.module_name());
    let service_path = feature_dir.join("service.rs");
    let mod_path = feature_dir.join("mod.rs");

    ensure_missing(&service_path)?;
    fs::create_dir_all(&feature_dir).map_err(|source| CliError::Io {
        path: feature_dir.clone(),
        source,
    })?;
    create_file(&service_path, &render_service(feature))?;

    let mut created = vec![service_path];
    if !mod_path.exists() {
        create_file(
            &mod_path,
            &render_feature_mod(feature, FeatureModKind::Service),
        )?;
        created.push(mod_path);
    }

    Ok(format!(
        "{}\n\n{}",
        created_files(&created),
        service_instructions(feature)
    ))
}

fn generate_controller(feature: &FeatureName, cwd: &Path) -> Result<String> {
    let feature_dir = src_dir(cwd).join(feature.module_name());
    let service_path = feature_dir.join("service.rs");
    let controller_path = feature_dir.join("controller.rs");
    let mod_path = feature_dir.join("mod.rs");
    let has_service = service_path.exists();

    ensure_missing(&controller_path)?;
    fs::create_dir_all(&feature_dir).map_err(|source| CliError::Io {
        path: feature_dir.clone(),
        source,
    })?;
    create_file(&controller_path, &render_controller(feature, has_service))?;

    let mut created = vec![controller_path];
    if !mod_path.exists() {
        let kind = if has_service {
            FeatureModKind::ControllerWithService
        } else {
            FeatureModKind::Controller
        };
        create_file(&mod_path, &render_feature_mod(feature, kind))?;
        created.push(mod_path);
    }

    let mut output = format!(
        "{}\n\n{}",
        created_files(&created),
        controller_instructions(feature, has_service)
    );
    if !has_service {
        output.push_str(&format!(
            "\n\nNote: src/{}/service.rs was not found, so {} was generated without a {} dependency.\n",
            feature.module_name(),
            feature.controller_type(),
            feature.service_type()
        ));
    }
    Ok(output)
}

fn generate_module(feature: &FeatureName, cwd: &Path) -> Result<String> {
    let feature_dir = src_dir(cwd).join(feature.module_name());
    let mod_path = feature_dir.join("mod.rs");
    let service_path = feature_dir.join("service.rs");
    let controller_path = feature_dir.join("controller.rs");

    ensure_missing(&mod_path)?;
    ensure_missing(&service_path)?;
    ensure_missing(&controller_path)?;

    fs::create_dir_all(&feature_dir).map_err(|source| CliError::Io {
        path: feature_dir.clone(),
        source,
    })?;
    create_file(&mod_path, &render_feature_module(feature))?;
    create_file(&service_path, &render_service(feature))?;
    create_file(&controller_path, &render_controller(feature, true))?;

    Ok(format!(
        "{}\n\n{}",
        created_files(&[mod_path, service_path, controller_path]),
        module_instructions(feature)
    ))
}

fn src_dir(cwd: &Path) -> PathBuf {
    cwd.join("src")
}

fn package_name_for_path(path: &Path, fallback: &str) -> Result<String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(fallback)
        .trim();

    if name.is_empty() {
        return Err(CliError::InvalidName(fallback.to_string()));
    }

    Ok(name.to_kebab_case())
}

fn resolve_caelix_root(explicit: Option<&Path>, cwd: &Path) -> Result<PathBuf> {
    if let Some(path) = explicit {
        let path = absolutize(path, cwd);
        return validate_caelix_root(path);
    }

    if let Some(path) = find_caelix_root(cwd) {
        return Ok(path);
    }

    let build_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf);

    if let Some(path) = build_root {
        if is_caelix_root(&path) {
            return Ok(path);
        }
    }

    Err(CliError::MissingCaelixPath)
}

fn absolutize(path: &Path, cwd: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn validate_caelix_root(path: PathBuf) -> Result<PathBuf> {
    if is_caelix_root(&path) {
        Ok(path)
    } else {
        Err(CliError::InvalidCaelixPath(path))
    }
}

fn find_caelix_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|path| is_caelix_root(path))
        .map(Path::to_path_buf)
}

fn is_caelix_root(path: &Path) -> bool {
    path.join("crates/caelix/Cargo.toml").is_file()
        && path.join("crates/caelix-core/Cargo.toml").is_file()
        && path.join("crates/caelix-actix/Cargo.toml").is_file()
}

fn dependency_path(app_dir: &Path, caelix_root: &Path, crate_dir: &str) -> String {
    let target = caelix_root.join("crates").join(crate_dir);
    let relative = pathdiff::diff_paths(&target, app_dir).unwrap_or(target);
    relative.to_string_lossy().replace('\\', "/")
}

pub fn render_app_cargo_toml(package_name: &str, app_dir: &Path, caelix_root: &Path) -> String {
    let caelix = dependency_path(app_dir, caelix_root, "caelix");
    let caelix_core = dependency_path(app_dir, caelix_root, "caelix-core");
    let caelix_actix = dependency_path(app_dir, caelix_root, "caelix-actix");

    format!(
        r#"[package]
name = "{package_name}"
version = "0.1.0"
edition = "2024"

[workspace]

[dependencies]
actix-web = "4.14.0"
caelix = {{ path = "{caelix}", features = ["actix"] }}
caelix-core = {{ path = "{caelix_core}" }}
caelix-actix = {{ path = "{caelix_actix}" }}
serde = {{ version = "1.0.228", features = ["derive"] }}
"#
    )
}

fn render_main_rs(crate_name: &str) -> String {
    format!(
        r#"use caelix_actix::Application;
use {crate_name}::AppModule;

#[caelix::main]
async fn main() -> std::io::Result<()> {{
    Application::new::<AppModule>()
        .await
        .listen("127.0.0.1:8080")
        .await
}}
"#,
        crate_name = crate_name
    )
}

fn render_lib_rs() -> &'static str {
    "pub mod app;\n\npub use app::AppModule;\n"
}

fn render_app_rs() -> &'static str {
    r#"use caelix::prelude::*;

pub struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
    }
}
"#
}

pub fn render_service(feature: &FeatureName) -> String {
    let service = feature.service_type();
    format!(
        r#"use caelix::prelude::*;

#[injectable]
pub struct {service};

impl {service} {{
    pub fn hello(&self) -> String {{
        "Hello from {service}".to_string()
    }}
}}
"#
    )
}

pub fn render_controller(feature: &FeatureName, has_service: bool) -> String {
    let controller = feature.controller_type();
    let route = feature.route_path();

    if has_service {
        let service = feature.service_type();
        format!(
            r#"use std::sync::Arc;

use caelix::prelude::*;

use super::{service};

#[injectable]
pub struct {controller} {{
    service: Arc<{service}>,
}}

#[controller("/{route}")]
impl {controller} {{
    #[get("")]
    pub async fn hello(&self) -> Result<String> {{
        Ok(self.service.hello())
    }}
}}
"#
        )
    } else {
        format!(
            r#"use caelix::prelude::*;

#[injectable]
pub struct {controller};

#[controller("/{route}")]
impl {controller} {{
    #[get("")]
    pub async fn hello(&self) -> Result<String> {{
        Ok("Hello from {controller}".to_string())
    }}
}}
"#
        )
    }
}

#[derive(Clone, Copy)]
enum FeatureModKind {
    Service,
    Controller,
    ControllerWithService,
}

fn render_feature_mod(feature: &FeatureName, kind: FeatureModKind) -> String {
    let service = feature.service_type();
    let controller = feature.controller_type();

    match kind {
        FeatureModKind::Service => format!(
            r#"pub mod service;

pub use service::{service};
"#
        ),
        FeatureModKind::Controller => format!(
            r#"pub mod controller;

pub use controller::{controller};
"#
        ),
        FeatureModKind::ControllerWithService => format!(
            r#"pub mod controller;
pub mod service;

pub use controller::{controller};
pub use service::{service};
"#
        ),
    }
}

pub fn render_feature_module(feature: &FeatureName) -> String {
    let module = feature.module_type();
    let service = feature.service_type();
    let controller = feature.controller_type();

    format!(
        r#"pub mod controller;
pub mod service;

pub use controller::{controller};
pub use service::{service};

use caelix::prelude::*;

pub struct {module};

impl Module for {module} {{
    fn register() -> ModuleMetadata {{
        ModuleMetadata::new()
            .provider::<{service}>()
            .controller::<{controller}>()
    }}
}}
"#
    )
}

fn service_instructions(feature: &FeatureName) -> String {
    format!(
        r#"Manual registration:
- Ensure `src/{}/mod.rs` contains `pub mod service;` and `pub use service::{};`.
- Add `pub mod {};` to `src/lib.rs` if it is not already declared.
- Add `use crate::{}::{};` to the module that should own the service.
- Add `.provider::<{}>()` inside that module's `register()`."#,
        feature.module_name(),
        feature.service_type(),
        feature.module_name(),
        feature.module_name(),
        feature.service_type(),
        feature.service_type()
    )
}

fn controller_instructions(feature: &FeatureName, has_service: bool) -> String {
    let mut instructions = format!(
        r#"Manual registration:
- Ensure `src/{}/mod.rs` contains `pub mod controller;` and `pub use controller::{};`.
- Add `pub mod {};` to `src/lib.rs` if it is not already declared.
- Add `use crate::{}::{};` to the module that should own the controller.
- Add `.controller::<{}>()` inside that module's `register()`."#,
        feature.module_name(),
        feature.controller_type(),
        feature.module_name(),
        feature.module_name(),
        feature.controller_type(),
        feature.controller_type()
    );

    if has_service {
        instructions.push_str(&format!(
            "\n- Ensure `{}` is registered as a provider in the same module.",
            feature.service_type()
        ));
    }

    instructions
}

fn module_instructions(feature: &FeatureName) -> String {
    format!(
        r#"Manual registration:
- Add `pub mod {};` to `src/lib.rs`.
- Add `use crate::{}::{};` to `src/app.rs`.
- Add `.import::<{}>()` inside `AppModule::register()`."#,
        feature.module_name(),
        feature.module_name(),
        feature.module_type(),
        feature.module_type()
    )
}

fn created_files(paths: &[PathBuf]) -> String {
    let mut output = String::from("Created files:\n");
    for path in paths {
        output.push_str(&format!("- {}\n", path.display()));
    }
    output
}

fn ensure_missing(path: &Path) -> Result<()> {
    if path.exists() {
        Err(CliError::AlreadyExists(path.to_path_buf()))
    } else {
        Ok(())
    }
}

fn create_file(path: impl AsRef<Path>, contents: impl AsRef<str>) -> Result<()> {
    let path = path.as_ref();
    ensure_missing(path)?;
    fs::write(path, contents.as_ref()).map_err(|source| CliError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_name_converts_to_rust_and_route_names() {
        let feature = FeatureName::parse("auth-session").unwrap();

        assert_eq!(feature.module_name(), "auth_session");
        assert_eq!(feature.route_path(), "auth-session");
        assert_eq!(feature.service_type(), "AuthSessionService");
        assert_eq!(feature.controller_type(), "AuthSessionController");
        assert_eq!(feature.module_type(), "AuthSessionModule");
    }

    #[test]
    fn service_template_uses_injectable_struct() {
        let feature = FeatureName::parse("users").unwrap();
        let rendered = render_service(&feature);

        assert!(rendered.contains("#[injectable]\npub struct UsersService;"));
        assert!(rendered.contains("Hello from UsersService"));
    }

    #[test]
    fn controller_template_omits_service_when_missing() {
        let feature = FeatureName::parse("users").unwrap();
        let rendered = render_controller(&feature, false);

        assert!(rendered.contains("pub struct UsersController;"));
        assert!(!rendered.contains("Arc<UsersService>"));
        assert!(rendered.contains("#[controller(\"/users\")]"));
    }

    #[test]
    fn module_template_registers_provider_and_controller() {
        let feature = FeatureName::parse("users").unwrap();
        let rendered = render_feature_module(&feature);

        assert!(rendered.contains("pub struct UsersModule;"));
        assert!(rendered.contains(".provider::<UsersService>()"));
        assert!(rendered.contains(".controller::<UsersController>()"));
    }
}
