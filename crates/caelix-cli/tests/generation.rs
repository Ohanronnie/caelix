use std::{fs, path::PathBuf};

use tempfile::tempdir;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .unwrap()
        .to_path_buf()
}

#[test]
fn new_creates_application_structure_with_local_paths() {
    let tmp = tempdir().unwrap();
    let root = workspace_root();

    let output = caelix_cli::run_from(
        [
            "caelix",
            "new",
            "demo-api",
            "--caelix-path",
            root.to_str().unwrap(),
        ],
        tmp.path(),
    )
    .unwrap();

    let app_dir = tmp.path().join("demo-api");
    let cargo_toml = fs::read_to_string(app_dir.join("Cargo.toml")).unwrap();
    let main_rs = fs::read_to_string(app_dir.join("src/main.rs")).unwrap();
    let lib_rs = fs::read_to_string(app_dir.join("src/lib.rs")).unwrap();
    let app_rs = fs::read_to_string(app_dir.join("src/app.rs")).unwrap();

    assert!(output.contains("Created Caelix application `demo-api`"));
    assert!(cargo_toml.contains("edition = \"2024\""));
    assert!(cargo_toml.contains("caelix = { path = "));
    assert!(cargo_toml.contains("caelix-core = { path = "));
    assert!(cargo_toml.contains("caelix-actix = { path = "));
    assert!(main_rs.contains("use demo_api::AppModule;"));
    assert!(main_rs.contains("Application::new::<AppModule>()"));
    assert_eq!(lib_rs, "pub mod app;\n\npub use app::AppModule;\n");
    assert!(app_rs.contains("ModuleMetadata::new()"));
}

#[test]
fn generate_service_creates_service_and_refuses_overwrite() {
    let tmp = tempdir().unwrap();

    let output = caelix_cli::run_from(["caelix", "g", "service", "users"], tmp.path()).unwrap();
    let service_path = tmp.path().join("src/users/service.rs");
    let mod_path = tmp.path().join("src/users/mod.rs");
    let service = fs::read_to_string(&service_path).unwrap();
    let module = fs::read_to_string(&mod_path).unwrap();

    assert!(output.contains("Manual registration:"));
    assert!(service.contains("#[injectable]\npub struct UsersService;"));
    assert!(module.contains("pub mod service;"));
    assert!(module.contains("pub use service::UsersService;"));

    let err = caelix_cli::run_from(["caelix", "generate", "service", "users"], tmp.path())
        .unwrap_err()
        .to_string();
    assert!(err.contains("already exists; refusing to overwrite"));
}

#[test]
fn generate_controller_uses_service_when_present_and_prints_instructions() {
    let tmp = tempdir().unwrap();
    caelix_cli::run_from(["caelix", "g", "service", "users"], tmp.path()).unwrap();

    let output =
        caelix_cli::run_from(["caelix", "generate", "controller", "users"], tmp.path()).unwrap();
    let controller = fs::read_to_string(tmp.path().join("src/users/controller.rs")).unwrap();

    assert!(controller.contains("service: Arc<UsersService>"));
    assert!(controller.contains("#[controller(\"/users\")]"));
    assert!(controller.contains("Ok(self.service.hello())"));
    assert!(output.contains("Add `.controller::<UsersController>()`"));
    assert!(output.contains("Ensure `UsersService` is registered"));
}

#[test]
fn generate_controller_without_service_prints_note() {
    let tmp = tempdir().unwrap();

    let output = caelix_cli::run_from(["caelix", "g", "controller", "users"], tmp.path()).unwrap();
    let controller = fs::read_to_string(tmp.path().join("src/users/controller.rs")).unwrap();

    assert!(controller.contains("pub struct UsersController;"));
    assert!(!controller.contains("Arc<UsersService>"));
    assert!(output.contains("generated without a UsersService dependency"));
}

#[test]
fn generate_module_creates_complete_feature_folder() {
    let tmp = tempdir().unwrap();

    let output =
        caelix_cli::run_from(["caelix", "g", "module", "auth-session"], tmp.path()).unwrap();
    let module = fs::read_to_string(tmp.path().join("src/auth_session/mod.rs")).unwrap();
    let service = fs::read_to_string(tmp.path().join("src/auth_session/service.rs")).unwrap();
    let controller = fs::read_to_string(tmp.path().join("src/auth_session/controller.rs")).unwrap();

    assert!(module.contains("pub struct AuthSessionModule;"));
    assert!(module.contains(".provider::<AuthSessionService>()"));
    assert!(module.contains(".controller::<AuthSessionController>()"));
    assert!(service.contains("pub struct AuthSessionService;"));
    assert!(controller.contains("#[controller(\"/auth-session\")]"));
    assert!(output.contains("Add `.import::<AuthSessionModule>()`"));
}
