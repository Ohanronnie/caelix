use std::{
    env,
    ffi::OsString,
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Child, Command as ProcessCommand},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

use clap::{Args, Parser, Subcommand, ValueEnum};
use heck::{ToKebabCase, ToPascalCase, ToSnakeCase};
use notify::RecursiveMode;
use notify_debouncer_mini::new_debouncer;
use toml_edit::{DocumentMut, Item, Value};

pub type Result<T> = std::result::Result<T, CliError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliOutcome {
    Output(String),
    Exit(i32),
}

#[derive(Debug)]
pub enum CliError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    MissingCargoManifest(PathBuf),
    AlreadyExists(PathBuf),
    InvalidName(String),
    TomlParse {
        path: PathBuf,
        source: toml_edit::TomlError,
    },
    MissingDependency(String),
    UnsupportedDependencyFormat(String),
    CratesIo(reqwest::Error),
    CratesIoResponse,
    CargoUpdateFailed(Option<i32>),
    Watcher(String),
    SignalHandler(ctrlc::Error),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::MissingCargoManifest(path) => write!(
                f,
                "{} was not found; run `caelix generate` from a Cargo project",
                path.display()
            ),
            Self::AlreadyExists(path) => {
                write!(
                    f,
                    "{} already exists; refusing to overwrite",
                    path.display()
                )
            }
            Self::InvalidName(name) => write!(f, "invalid project or feature name `{name}`"),
            Self::TomlParse { path, source } => write!(f, "{}: {source}", path.display()),
            Self::MissingDependency(name) => {
                write!(f, "`{name}` dependency was not found in Cargo.toml")
            }
            Self::UnsupportedDependencyFormat(name) => {
                write!(
                    f,
                    "`{name}` dependency uses an unsupported Cargo.toml format"
                )
            }
            Self::CratesIo(source) => write!(f, "failed to fetch latest caelix version: {source}"),
            Self::CratesIoResponse => write!(f, "crates.io response did not include max_version"),
            Self::CargoUpdateFailed(code) => match code {
                Some(code) => write!(f, "`cargo update -p caelix` failed with exit code {code}"),
                None => write!(f, "`cargo update -p caelix` was terminated by a signal"),
            },
            Self::Watcher(message) => write!(f, "{message}"),
            Self::SignalHandler(source) => write!(f, "failed to install Ctrl+C handler: {source}"),
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
    /// Create a new Caelix application
    New(NewArgs),
    /// Generate a Caelix service, controller, or module
    #[command(alias = "g")]
    Generate(GenerateArgs),
    /// Update the caelix dependency in the current Cargo.toml
    Update,
    /// Run the current Caelix application
    Run(RunArgs),
}

#[derive(Args, Debug)]
struct NewArgs {
    name: String,
    /// Runtime adapter for the generated application
    #[arg(long, value_enum, default_value_t = BackendChoice::Actix)]
    backend: BackendChoice,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum BackendChoice {
    Actix,
    Axum,
}

#[derive(Args, Debug)]
struct GenerateArgs {
    #[command(subcommand)]
    kind: GenerateKind,
}

#[derive(Args, Debug)]
struct RunArgs {
    /// Restart the application when source files change
    #[arg(long)]
    watch: bool,

    /// Arguments passed to the application after `--`
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    app_args: Vec<OsString>,
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

pub fn run_from_env() -> Result<CliOutcome> {
    let cwd = env::current_dir().map_err(|source| CliError::Io {
        path: PathBuf::from("."),
        source,
    })?;
    let cli = Cli::parse_from(env::args_os());
    run_cli(cli, cwd.as_path())
}

pub fn run_from<I, T>(args: I, cwd: impl AsRef<Path>) -> Result<String>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    run_text_command(cli, cwd.as_ref())
}

fn run_cli(cli: Cli, cwd: &Path) -> Result<CliOutcome> {
    match cli.command {
        Command::Run(args) => run_application(args, cwd).map(CliOutcome::Exit),
        command => run_text_command(Cli { command }, cwd).map(CliOutcome::Output),
    }
}

fn run_text_command(cli: Cli, cwd: &Path) -> Result<String> {
    match cli.command {
        Command::New(args) => generate_new(args, cwd),
        Command::Generate(args) => {
            ensure_cargo_manifest(cwd)?;
            match args.kind {
                GenerateKind::Service(args) => {
                    generate_service(&FeatureName::parse(args.name)?, cwd)
                }
                GenerateKind::Controller(args) => {
                    generate_controller(&FeatureName::parse(args.name)?, cwd)
                }
                GenerateKind::Module(args) => generate_module(&FeatureName::parse(args.name)?, cwd),
            }
        }
        Command::Update => update_caelix_dependency(cwd),
        Command::Run(args) => Ok(format_cargo_run_command(&args.app_args)),
    }
}

fn run_application(args: RunArgs, cwd: &Path) -> Result<i32> {
    if args.watch {
        run_application_watch(cwd, args.app_args)
    } else {
        run_application_once(cwd, &args.app_args)
    }
}

fn run_application_once(cwd: &Path, app_args: &[OsString]) -> Result<i32> {
    clear_screen()?;
    let status = cargo_run_command(cwd, app_args)
        .status()
        .map_err(|source| CliError::Io {
            path: cwd.to_path_buf(),
            source,
        })?;

    Ok(status.code().unwrap_or(1))
}

fn run_application_watch(cwd: &Path, app_args: Vec<OsString>) -> Result<i32> {
    let process = Arc::new(Mutex::new(RunningProcess::new(cwd.to_path_buf(), app_args)));

    {
        let process = Arc::clone(&process);
        ctrlc::set_handler(move || {
            if let Ok(mut process) = process.lock() {
                process.kill();
            }
            std::process::exit(130);
        })
        .map_err(CliError::SignalHandler)?;
    }

    process
        .lock()
        .map_err(|_| CliError::Watcher("process mutex poisoned".into()))?
        .start()?;

    let (tx, rx) = mpsc::channel();
    let mut debouncer = new_debouncer(Duration::from_millis(300), tx)
        .map_err(|source| CliError::Watcher(format!("failed to start file watcher: {source}")))?;

    let src = cwd.join("src");
    if src.exists() {
        debouncer
            .watcher()
            .watch(&src, RecursiveMode::Recursive)
            .map_err(|source| {
                CliError::Watcher(format!("failed to watch {}: {source}", src.display()))
            })?;
    }

    let cargo_toml = cwd.join("Cargo.toml");
    if cargo_toml.exists() {
        debouncer
            .watcher()
            .watch(&cargo_toml, RecursiveMode::NonRecursive)
            .map_err(|source| {
                CliError::Watcher(format!(
                    "failed to watch {}: {source}",
                    cargo_toml.display()
                ))
            })?;
    }

    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Ok(events)) => {
                if events.is_empty() {
                    continue;
                }
                println!("Change detected. Restarting...");
                process
                    .lock()
                    .map_err(|_| CliError::Watcher("process mutex poisoned".into()))?
                    .start()?;
            }
            Ok(Err(errors)) => {
                return Err(CliError::Watcher(format!("file watcher failed: {errors}")));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                process
                    .lock()
                    .map_err(|_| CliError::Watcher("process mutex poisoned".into()))?
                    .reap_finished();
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(CliError::Watcher(
                    "file watcher stopped unexpectedly".into(),
                ));
            }
        }
    }
}

struct RunningProcess {
    cwd: PathBuf,
    app_args: Vec<OsString>,
    child: Option<Child>,
}

impl RunningProcess {
    fn new(cwd: PathBuf, app_args: Vec<OsString>) -> Self {
        Self {
            cwd,
            app_args,
            child: None,
        }
    }

    fn start(&mut self) -> Result<()> {
        self.kill();
        clear_screen()?;
        self.child = Some(
            cargo_run_command(&self.cwd, &self.app_args)
                .spawn()
                .map_err(|source| CliError::Io {
                    path: self.cwd.clone(),
                    source,
                })?,
        );
        Ok(())
    }

    fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn reap_finished(&mut self) {
        let finished = self
            .child
            .as_mut()
            .and_then(|child| child.try_wait().ok())
            .flatten()
            .is_some();

        if finished {
            self.child = None;
        }
    }
}

impl Drop for RunningProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

fn cargo_run_command(cwd: &Path, app_args: &[OsString]) -> ProcessCommand {
    let mut command = ProcessCommand::new("cargo");
    command.arg("run").current_dir(cwd);
    if !app_args.is_empty() {
        command.arg("--").args(app_args);
    }
    command
}

fn clear_screen() -> Result<()> {
    let mut stdout = io::stdout();
    stdout
        .write_all(b"\x1B[2J\x1B[H")
        .map_err(|source| CliError::Io {
            path: PathBuf::from("<stdout>"),
            source,
        })?;
    stdout.flush().map_err(|source| CliError::Io {
        path: PathBuf::from("<stdout>"),
        source,
    })
}

fn format_cargo_run_command(app_args: &[OsString]) -> String {
    let mut parts = vec!["cargo".to_string(), "run".to_string()];
    if !app_args.is_empty() {
        parts.push("--".to_string());
        parts.extend(
            app_args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned()),
        );
    }

    format!("{}\n", parts.join(" "))
}

fn update_caelix_dependency(cwd: &Path) -> Result<String> {
    let cargo_toml_path = cwd.join("Cargo.toml");
    let latest = fetch_latest_caelix_version()?;
    let outcome = update_caelix_version(&cargo_toml_path, &latest)?;

    match outcome {
        UpdateOutcome::AlreadyLatest { current } => {
            Ok(format!("Already on the latest version ({current}).\n"))
        }
        UpdateOutcome::Updated { previous, latest } => {
            run_cargo_update(cwd)?;
            Ok(format!(
                "caelix {previous} -> {latest}\nUpdated Cargo.toml. Ran `cargo update -p caelix`.\n"
            ))
        }
    }
}

fn fetch_latest_caelix_version() -> Result<String> {
    let response = reqwest::blocking::Client::new()
        .get("https://crates.io/api/v1/crates/caelix")
        .header(reqwest::header::USER_AGENT, "caelix-cli")
        .send()
        .map_err(CliError::CratesIo)?
        .error_for_status()
        .map_err(CliError::CratesIo)?;
    let json: serde_json::Value = response.json().map_err(CliError::CratesIo)?;

    json["crate"]["max_version"]
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or(CliError::CratesIoResponse)
}

fn run_cargo_update(cwd: &Path) -> Result<()> {
    let status = ProcessCommand::new("cargo")
        .args(["update", "-p", "caelix"])
        .current_dir(cwd)
        .status()
        .map_err(|source| CliError::Io {
            path: cwd.join("Cargo.toml"),
            source,
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(CliError::CargoUpdateFailed(status.code()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UpdateOutcome {
    AlreadyLatest { current: String },
    Updated { previous: String, latest: String },
}

fn update_caelix_version(cargo_toml_path: &Path, latest: &str) -> Result<UpdateOutcome> {
    let content = fs::read_to_string(cargo_toml_path).map_err(|source| CliError::Io {
        path: cargo_toml_path.to_path_buf(),
        source,
    })?;
    let mut doc = content
        .parse::<DocumentMut>()
        .map_err(|source| CliError::TomlParse {
            path: cargo_toml_path.to_path_buf(),
            source,
        })?;

    let current = read_caelix_version(&doc)?;
    if current == latest {
        return Ok(UpdateOutcome::AlreadyLatest { current });
    }

    write_caelix_version(&mut doc, latest)?;
    fs::write(cargo_toml_path, doc.to_string()).map_err(|source| CliError::Io {
        path: cargo_toml_path.to_path_buf(),
        source,
    })?;

    Ok(UpdateOutcome::Updated {
        previous: current,
        latest: latest.to_string(),
    })
}

fn read_caelix_version(doc: &DocumentMut) -> Result<String> {
    let dependency =
        find_caelix_dependency(doc).ok_or_else(|| CliError::MissingDependency("caelix".into()))?;

    if let Some(version) = dependency.as_str() {
        return Ok(version.to_string());
    }

    if let Item::Value(Value::InlineTable(table)) = dependency {
        return table
            .get("version")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| CliError::UnsupportedDependencyFormat("caelix".into()));
    }

    dependency["version"]
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| CliError::UnsupportedDependencyFormat("caelix".into()))
}

fn write_caelix_version(doc: &mut DocumentMut, latest: &str) -> Result<()> {
    let dependency = find_caelix_dependency_mut(doc)
        .ok_or_else(|| CliError::MissingDependency("caelix".into()))?;

    if dependency.as_str().is_some() {
        replace_string_value_preserving_decor(dependency, latest)?;
        return Ok(());
    }

    if let Item::Value(Value::InlineTable(table)) = dependency {
        if table.get("version").is_some() {
            table.insert("version", Value::from(latest));
            return Ok(());
        }
    }

    if dependency["version"].as_str().is_some() {
        replace_string_value_preserving_decor(&mut dependency["version"], latest)?;
        return Ok(());
    }

    Err(CliError::UnsupportedDependencyFormat("caelix".into()))
}

fn replace_string_value_preserving_decor(item: &mut Item, value: &str) -> Result<()> {
    let Some(current) = item.as_value_mut() else {
        return Err(CliError::UnsupportedDependencyFormat("caelix".into()));
    };

    if !current.is_str() {
        return Err(CliError::UnsupportedDependencyFormat("caelix".into()));
    }

    let decor = current.decor().clone();
    let mut replacement = Value::from(value);
    *replacement.decor_mut() = decor;
    *current = replacement;
    Ok(())
}

fn find_caelix_dependency(doc: &DocumentMut) -> Option<&Item> {
    doc.get("dependencies")
        .and_then(|dependencies| dependencies.get("caelix"))
        .or_else(|| {
            doc.get("workspace")
                .and_then(|workspace| workspace.get("dependencies"))
                .and_then(|dependencies| dependencies.get("caelix"))
        })
}

fn find_caelix_dependency_mut(doc: &mut DocumentMut) -> Option<&mut Item> {
    let has_normal_dependency = doc
        .get("dependencies")
        .and_then(|dependencies| dependencies.get("caelix"))
        .is_some();

    if has_normal_dependency {
        return doc
            .get_mut("dependencies")
            .and_then(|dependencies| dependencies.get_mut("caelix"));
    }

    doc.get_mut("workspace")
        .and_then(|workspace| workspace.get_mut("dependencies"))
        .and_then(|dependencies| dependencies.get_mut("caelix"))
}

fn generate_new(args: NewArgs, cwd: &Path) -> Result<String> {
    validate_project_name(&args.name)?;
    let target_dir = cwd.join(&args.name);
    ensure_missing(&target_dir)?;

    let package_name = package_name_for_path(&target_dir, &args.name)?;
    let crate_name = package_name.to_snake_case();

    fs::create_dir_all(target_dir.join("src")).map_err(|source| CliError::Io {
        path: target_dir.join("src"),
        source,
    })?;

    let cargo_toml = render_app_cargo_toml_for_backend(&package_name, args.backend);
    create_file(target_dir.join("Cargo.toml"), &cargo_toml)?;
    create_file(target_dir.join("AGENTS.md"), render_agents_md())?;
    create_file(target_dir.join("src/main.rs"), &render_main_rs(&crate_name))?;
    create_file(target_dir.join("src/lib.rs"), render_lib_rs())?;
    create_file(target_dir.join("src/app.rs"), render_app_rs())?;

    Ok(format!(
        "Created Caelix application `{}` in {}\n\nNext steps:\n- cd {}\n- caelix run\n",
        package_name,
        target_dir.display(),
        target_dir.display()
    ))
}

/// `caelix new` deliberately accepts a project *component*, never a path.
/// Keeping this narrower than Cargo's package syntax prevents traversal and
/// avoids generating identifiers that the generated Rust source cannot use.
fn validate_project_name(name: &str) -> Result<()> {
    const RUST_KEYWORDS: &[&str] = &[
        "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn",
        "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
        "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
        "use", "where", "while", "async", "await", "dyn", "abstract", "become", "box", "do",
        "final", "macro", "override", "priv", "typeof", "unsized", "virtual", "yield", "try",
    ];

    let valid = name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && name
            .bytes()
            .skip(1)
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        && !RUST_KEYWORDS.contains(&name);
    if valid {
        Ok(())
    } else {
        Err(CliError::InvalidName(name.to_owned()))
    }
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

fn ensure_cargo_manifest(cwd: &Path) -> Result<()> {
    let path = cwd.join("Cargo.toml");
    if path.is_file() {
        Ok(())
    } else {
        Err(CliError::MissingCargoManifest(path))
    }
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

pub fn render_app_cargo_toml(package_name: &str) -> String {
    render_app_cargo_toml_for_backend(package_name, BackendChoice::Actix)
}

fn render_app_cargo_toml_for_backend(package_name: &str, backend: BackendChoice) -> String {
    let backend_dependencies = match backend {
        BackendChoice::Actix => "actix-web = \"4.14.0\"\ncaelix = \"0.0.19\"",
        BackendChoice::Axum => {
            "caelix = { version = \"0.0.19\", default-features = false, features = [\"axum\", \"sqlx\", \"validator\"] }\ntower-http = { version = \"0.6\", features = [\"trace\", \"compression-full\"] }"
        }
    };
    format!(
        r#"[package]
name = "{package_name}"
version = "0.0.1"
edition = "2024"

[dependencies]
{backend_dependencies}
serde = {{ version = "1.0.228", features = ["derive"] }}
"#
    )
}

fn render_main_rs(crate_name: &str) -> String {
    format!(
        r#"use caelix::Application;
use {crate_name}::AppModule;

#[caelix::main]
async fn main() -> std::io::Result<()> {{
    Application::new::<AppModule>()
        .await
        .map_err(|err| std::io::Error::other(err.message))?
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
    r#"use caelix::{Module, ModuleMetadata};

pub struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
    }
}
"#
}

fn render_agents_md() -> &'static str {
    r#"# Agent Instructions

This is a Caelix application. Use this file as the quick working reference when changing generated app code.

For fuller documentation, refer to https://ohanronnie.github.io/caelix/.

## App Structure

- `src/main.rs` starts the selected Caelix runtime with `Application::new::<AppModule>()`.
- `src/lib.rs` exports the root `AppModule` and should declare feature modules with `pub mod feature_name;`.
- `src/app.rs` owns the root `AppModule`.
- Feature folders usually contain `mod.rs`, `service.rs`, and `controller.rs`.
- Prefer the Caelix CLI for new framework files: `caelix g module name`, `caelix g service name`, and `caelix g controller name`.

## Registration Model

Caelix uses explicit module metadata. Do not rely on filesystem discovery or hidden auto-registration.

- A module implements `Module` and returns `ModuleMetadata`.
- Add generated feature modules to `src/lib.rs`.
- Import feature modules in `src/app.rs`.
- Add `.import::<FeatureModule>()` inside `AppModule::register()`.
- Register services with `.provider::<Service>()`.
- Register controllers with `.controller::<Controller>()`.
- Register async factory values with `.provider_async_factory::<T, _, _>(provider_dependencies![...], ...)` when construction cannot be expressed as `#[injectable]`.
- Export a service with `.export::<Service>()` before another module may inject it; consumers must import that module unless the service is explicitly exported by a global module.

Example:

```rust
use caelix::{Module, ModuleMetadata};
use crate::users::UsersModule;

pub struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().import::<UsersModule>()
    }
}
```

## Providers And Injection

Prefer `#[injectable]` for services, controllers, guards, and interceptors.

- Injectable fields must be `Arc<T>`.
- `Arc<Logger>` is provided automatically with the struct name as context.
- Unit structs are valid injectables.
- Tuple structs are not supported by `#[injectable]`.
- Manual `Injectable` implementations are acceptable for custom async construction.
- Lifecycle hooks can be implemented on providers: `on_module_init`, `on_bootstrap`, and `on_shutdown`.

Example:

```rust
use std::sync::Arc;
use caelix::{injectable, Logger};

#[injectable]
pub struct UsersService {
    logger: Arc<Logger>,
}
```

## Controllers

Use `#[controller("/base-path")]` on an impl block. Route handlers are async methods.

- Supported route attributes: `#[get]`, `#[post]`, `#[patch]`, `#[put]`, `#[delete]`.
- Supported extractor attributes: `#[param]`, `#[body]`, `#[query]`, `#[user]`.
- Add `#[validate]` to extracted DTOs that implement `validator::Validate`.
- `#[user]` reads from `RequestContext`; missing users become `UnauthorizedException`.

Example:

```rust
use std::sync::Arc;
use caelix::{controller, injectable, Result};
use super::UsersService;

#[injectable]
pub struct UsersController {
    service: Arc<UsersService>,
}

#[controller("/users")]
impl UsersController {
    #[get("/{id}")]
    async fn find_one(&self, #[param] id: String) -> Result<String> {
        Ok(self.service.find_one(id).await?)
    }
}
```

## Guards, Interceptors, And Context

- Guards implement `Guard` and return whether a request may continue.
- Interceptors implement `Interceptor` and can wrap handler execution.
- Apply them with `#[use_guard(Type)]` or `#[use_interceptor(Type)]` at controller or method level.
- Controller-level guards/interceptors apply before method-level ones.
- Use `RequestContext` for request method, path, headers, and per-request values such as authenticated users.

## Responses And Errors

Handlers should return values that implement `IntoCaelixResponse`.

- `Result<String>` returns `200 text/plain`.
- `Result<Response<T>>` is the usual JSON response path.
- `Response::Body(value)` returns `200` JSON.
- `Response::WithStatus(status, value)` returns JSON with a custom status.
- `Response::json`, `Response::text`, and `Response::bytes` return explicit raw payloads.
- `Response::stream`, `Response::sse`, and `Response::file` return streaming `HttpResponse` values.
- `Response::no_content()` returns `204`.
- Use Caelix exception types such as `BadRequestException`, `UnauthorizedException`, `ForbiddenException`, `NotFoundException`, and `InternalServerErrorException` for errors.
- Server error messages are intentionally hidden from HTTP responses.

## Cache

Cache support is explicit service-level caching.

- Import `CacheModule` into a module that needs cache support.
- Inject `Arc<Cache>` into services that need cache reads/writes.
- Do not add automatic HTTP response caching.
- If the app needs response caching, implement it with an interceptor that reads from and writes to `Cache`.

## Checks

- Run `cargo test` after code changes when feasible.
- Keep public app code using the `caelix` facade (`use caelix::...`) instead of internal Caelix crate paths.
- When using CLI-generated files, keep the manual registration steps in the command output aligned with `src/lib.rs` and `src/app.rs`.
"#
}

pub fn render_service(feature: &FeatureName) -> String {
    let service = feature.service_type();
    format!(
        r#"use caelix::injectable;

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

use caelix::{{controller, injectable, Result}};

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
            r#"use caelix::{{controller, injectable, Result}};

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

use caelix::{{Module, ModuleMetadata}};

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
    use tempfile::tempdir;

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

    #[test]
    fn run_command_passes_app_args_after_separator() {
        let output = run_from(
            [
                "caelix",
                "run",
                "--watch",
                "--",
                "--rubbish-tes",
                "true",
                "--hshsh",
                "jsj",
                "--shshs",
                "q",
                "-h",
                "nnsjs",
                "dyg?",
            ],
            ".",
        )
        .unwrap();

        assert_eq!(
            output,
            "cargo run -- --rubbish-tes true --hshsh jsj --shshs q -h nnsjs dyg?\n"
        );
    }

    #[test]
    fn run_command_without_app_args_delegates_to_cargo_run() {
        let output = run_from(["caelix", "run"], ".").unwrap();

        assert_eq!(output, "cargo run\n");
    }

    #[test]
    fn update_caelix_version_preserves_cargo_toml_comments_and_other_dependencies() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("Cargo.toml");
        fs::write(
            &path,
            r#"[package]
name = "demo"
version = "0.0.1"

# Keep this dependency comment.
[dependencies]
actix-web = "4.14.0"
caelix = "0.0.2" # framework
serde = { version = "1.0", features = ["derive"] }
"#,
        )
        .unwrap();

        let outcome = update_caelix_version(&path, "0.0.3").unwrap();
        let updated = fs::read_to_string(path).unwrap();

        assert_eq!(
            outcome,
            UpdateOutcome::Updated {
                previous: "0.0.2".into(),
                latest: "0.0.3".into()
            }
        );
        assert!(updated.contains("# Keep this dependency comment."));
        assert!(updated.contains(r#"caelix = "0.0.3" # framework"#));
        assert!(updated.contains(r#"serde = { version = "1.0", features = ["derive"] }"#));
    }

    #[test]
    fn update_caelix_version_preserves_inline_table_features() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("Cargo.toml");
        fs::write(
            &path,
            r#"[package]
name = "demo"
version = "0.0.1"

[dependencies]
caelix = { version = "0.0.2", default-features = false, features = ["actix"] }
"#,
        )
        .unwrap();

        update_caelix_version(&path, "0.0.3").unwrap();
        let updated = fs::read_to_string(path).unwrap();

        assert!(updated.contains(
            r#"caelix = { version = "0.0.3", default-features = false, features = ["actix"] }"#
        ));
    }

    #[test]
    fn update_caelix_version_reports_already_latest_without_writing() {
        let tmp = tempdir().unwrap();
        let path = tmp.path().join("Cargo.toml");
        let content = r#"[dependencies]
caelix = "0.0.3"
"#;
        fs::write(&path, content).unwrap();

        let outcome = update_caelix_version(&path, "0.0.3").unwrap();

        assert_eq!(
            outcome,
            UpdateOutcome::AlreadyLatest {
                current: "0.0.3".into()
            }
        );
        assert_eq!(fs::read_to_string(path).unwrap(), content);
    }
}
