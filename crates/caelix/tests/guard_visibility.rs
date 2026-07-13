#![cfg(feature = "actix")]

use caelix::{
    BoxFuture, Guard, Module, ModuleMetadata, RequestContext, Result, build_container, controller,
    guard, injectable,
};

#[guard]
struct AuthGuard;

impl Guard for AuthGuard {
    fn can_activate<'a>(&'a self, _: &'a RequestContext) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async { Ok(true) })
    }
}

#[injectable]
struct GuardedController;

#[controller("/guarded")]
#[use_guard(AuthGuard)]
impl GuardedController {
    #[get("")]
    async fn index(&self) -> Result<&'static str> {
        Ok("ok")
    }
}

struct PrivateGuardModule;

impl Module for PrivateGuardModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().provider::<AuthGuard>()
    }
}

struct PrivateGuardConsumerModule;

impl Module for PrivateGuardConsumerModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<PrivateGuardModule>()
            .controller::<GuardedController>()
    }
}

#[caelix::test]
async fn guarded_controllers_reject_private_guards() {
    let error = match build_container::<PrivateGuardConsumerModule>().await {
        Ok(_) => panic!("a private guard must not be visible to an importing controller"),
        Err(error) => error,
    };

    assert!(error.message.contains("AuthGuard"));
    assert!(error.message.contains("not visible"));
}

struct ExportedGuardModule;

impl Module for ExportedGuardModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<AuthGuard>()
            .export::<AuthGuard>()
    }
}

struct ExportedGuardConsumerModule;

impl Module for ExportedGuardConsumerModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<ExportedGuardModule>()
            .controller::<GuardedController>()
    }
}

#[caelix::test]
async fn guarded_controllers_accept_exported_guards() {
    assert!(
        build_container::<ExportedGuardConsumerModule>()
            .await
            .is_ok()
    );
}

struct GlobalGuardModule;

impl Module for GlobalGuardModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::global()
            .provider::<AuthGuard>()
            .export::<AuthGuard>()
    }
}

struct GlobalGuardConsumerModule;

impl Module for GlobalGuardConsumerModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().controller::<GuardedController>()
    }
}

struct GlobalGuardRootModule;

impl Module for GlobalGuardRootModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<GlobalGuardConsumerModule>()
            .import::<GlobalGuardModule>()
    }
}

#[caelix::test]
async fn guarded_controllers_accept_global_guard_exports() {
    assert!(build_container::<GlobalGuardRootModule>().await.is_ok());
}
