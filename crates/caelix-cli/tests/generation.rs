use std::fs;

use tempfile::{TempDir, tempdir};

fn cargo_project() -> TempDir {
    let tmp = tempdir().unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    tmp
}

#[test]
fn new_creates_application_structure_with_crates_io_dependency() {
    let tmp = tempdir().unwrap();

    let output = caelix_cli::run_from(["caelix", "new", "demo-api"], tmp.path()).unwrap();

    let app_dir = tmp.path().join("demo-api");
    let cargo_toml = fs::read_to_string(app_dir.join("Cargo.toml")).unwrap();
    let agents_md = fs::read_to_string(app_dir.join("AGENTS.md")).unwrap();
    let main_rs = fs::read_to_string(app_dir.join("src/main.rs")).unwrap();
    let lib_rs = fs::read_to_string(app_dir.join("src/lib.rs")).unwrap();
    let app_rs = fs::read_to_string(app_dir.join("src/app.rs")).unwrap();

    assert!(output.contains("Created Caelix application `demo-api`"));
    assert!(cargo_toml.contains("edition = \"2024\""));
    assert!(!cargo_toml.contains("[workspace]"));
    assert!(cargo_toml.contains("caelix = \"0.0.25\""));
    assert!(!cargo_toml.contains("path = "));
    assert!(!cargo_toml.contains("caelix-core"));
    assert!(!cargo_toml.contains("caelix-actix"));
    assert!(agents_md.contains("Use this file as the quick working reference"));
    assert!(agents_md.contains("https://ohanronnie.github.io/caelix/"));
    assert!(agents_md.contains("Prefer the Caelix CLI for new framework files"));
    assert!(agents_md.contains("caelix g module name"));
    assert!(agents_md.contains("## Registration Model"));
    assert!(agents_md.contains("A module implements `Module` and returns `ModuleMetadata`."));
    assert!(agents_md.contains("## Controllers"));
    assert!(agents_md.contains("Supported extractor attributes"));
    assert!(agents_md.contains("## Responses And Errors"));
    assert!(agents_md.contains("Do not add automatic HTTP response caching"));
    assert!(main_rs.contains("use caelix::Application;"));
    assert!(main_rs.contains("use demo_api::AppModule;"));
    assert!(main_rs.contains("#[caelix::main]"));
    assert!(main_rs.contains("Application::new::<AppModule>()"));
    assert!(!app_dir.join("src/bin/doctor.rs").exists());
    assert!(!app_dir.join("src/bin").exists());
    assert_eq!(lib_rs, "pub mod app;\n\npub use app::AppModule;\n");
    assert!(app_rs.contains("use caelix::{Module, ModuleMetadata};"));
    assert!(app_rs.contains("ModuleMetadata::new()"));
}

#[test]
fn new_with_axum_backend_enables_the_axum_feature() {
    let tmp = tempdir().unwrap();
    caelix_cli::run_from(
        ["caelix", "new", "demo-api", "--backend", "axum"],
        tmp.path(),
    )
    .unwrap();
    let cargo_toml = fs::read_to_string(tmp.path().join("demo-api/Cargo.toml")).unwrap();

    assert!(cargo_toml.contains("default-features = false"));
    assert!(cargo_toml.contains("features = [\"axum\"]"));
    assert!(cargo_toml.contains("tower-http"));
    assert!(!cargo_toml.contains("actix-web"));
}

#[test]
fn new_rejects_paths_keywords_and_other_non_component_names_without_writing() {
    let tmp = tempdir().unwrap();
    let outside = tmp.path().parent().unwrap().join("caelix-escape-check");
    let invalid = [
        "../caelix-escape-check",
        "/tmp/caelix-escape-check",
        "",
        "two words",
        "fn",
        ".",
        "a.b",
        "a/b",
        "a\\b",
    ];

    for name in invalid {
        let error = caelix_cli::run_from(["caelix", "new", name], tmp.path()).unwrap_err();
        assert!(error.to_string().contains("invalid project"), "{name}");
    }

    assert!(!outside.exists());
    assert!(!std::path::Path::new("/tmp/caelix-escape-check").exists());
    assert!(fs::read_dir(tmp.path()).unwrap().next().is_none());
}

#[test]
fn new_accepts_identifier_like_hyphen_and_underscore_names() {
    let tmp = tempdir().unwrap();
    caelix_cli::run_from(["caelix", "new", "demo-api_2"], tmp.path()).unwrap();
    assert!(tmp.path().join("demo-api_2/Cargo.toml").exists());
}

#[test]
fn generate_service_creates_service_and_refuses_overwrite() {
    let tmp = cargo_project();

    let output = caelix_cli::run_from(["caelix", "g", "service", "users"], tmp.path()).unwrap();
    let service_path = tmp.path().join("src/users/service.rs");
    let mod_path = tmp.path().join("src/users/mod.rs");
    let service = fs::read_to_string(&service_path).unwrap();
    let module = fs::read_to_string(&mod_path).unwrap();

    assert!(output.contains("Manual registration:"));
    assert!(service.contains("use caelix::injectable;"));
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
    let tmp = cargo_project();
    caelix_cli::run_from(["caelix", "g", "service", "users"], tmp.path()).unwrap();

    let output =
        caelix_cli::run_from(["caelix", "generate", "controller", "users"], tmp.path()).unwrap();
    let controller = fs::read_to_string(tmp.path().join("src/users/controller.rs")).unwrap();

    assert!(controller.contains("service: Arc<UsersService>"));
    assert!(controller.contains("use caelix::{controller, injectable, Result};"));
    assert!(controller.contains("#[controller(\"/users\")]"));
    assert!(controller.contains("Ok(self.service.hello())"));
    assert!(output.contains("Add `.controller::<UsersController>()`"));
    assert!(output.contains("Ensure `UsersService` is registered"));
}

#[test]
fn generate_controller_without_service_prints_note() {
    let tmp = cargo_project();

    let output = caelix_cli::run_from(["caelix", "g", "controller", "users"], tmp.path()).unwrap();
    let controller = fs::read_to_string(tmp.path().join("src/users/controller.rs")).unwrap();

    assert!(controller.contains("pub struct UsersController;"));
    assert!(controller.contains("use caelix::{controller, injectable, Result};"));
    assert!(!controller.contains("Arc<UsersService>"));
    assert!(output.contains("generated without a UsersService dependency"));
}

#[test]
fn generate_module_creates_complete_feature_folder() {
    let tmp = cargo_project();

    let output =
        caelix_cli::run_from(["caelix", "g", "module", "auth-session"], tmp.path()).unwrap();
    let module = fs::read_to_string(tmp.path().join("src/auth_session/mod.rs")).unwrap();
    let service = fs::read_to_string(tmp.path().join("src/auth_session/service.rs")).unwrap();
    let controller = fs::read_to_string(tmp.path().join("src/auth_session/controller.rs")).unwrap();

    assert!(module.contains("pub struct AuthSessionModule;"));
    assert!(module.contains("use caelix::{Module, ModuleMetadata};"));
    assert!(module.contains(".provider::<AuthSessionService>()"));
    assert!(module.contains(".controller::<AuthSessionController>()"));
    assert!(service.contains("use caelix::injectable;"));
    assert!(service.contains("pub struct AuthSessionService;"));
    assert!(controller.contains("use caelix::{controller, injectable, Result};"));
    assert!(controller.contains("#[controller(\"/auth-session\")]"));
    assert!(output.contains("Add `.import::<AuthSessionModule>()`"));
}

#[test]
fn generate_requires_a_cargo_manifest() {
    let tmp = tempdir().unwrap();

    let error = caelix_cli::run_from(["caelix", "g", "service", "users"], tmp.path())
        .unwrap_err()
        .to_string();

    assert!(error.contains("Cargo.toml was not found"));
    assert!(!tmp.path().join("src").exists());
}
