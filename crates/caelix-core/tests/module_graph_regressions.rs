use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use caelix_core::*;

fn block_on<F: Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, Waker};

    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let mut future = std::pin::pin!(future);

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

struct SharedService;

impl Injectable for SharedService {
    fn dependencies() -> Vec<ProviderDependency> {
        provider_dependencies![]
    }

    fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async { Ok(Self) })
    }
}

struct SharedConsumer {
    _shared: Arc<SharedService>,
}

impl Injectable for SharedConsumer {
    fn dependencies() -> Vec<ProviderDependency> {
        provider_dependencies![SharedService]
    }

    fn create(container: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async move {
            Ok(Self {
                _shared: container.resolve::<SharedService>()?,
            })
        })
    }
}

struct PrivateSharedModule;

impl Module for PrivateSharedModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().provider::<SharedService>()
    }
}

struct PrivateConsumerModule;

impl Module for PrivateConsumerModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<PrivateSharedModule>()
            .provider::<SharedConsumer>()
    }
}

#[test]
fn private_imports_are_not_visible_to_consumers() {
    let error = match block_on(build_container::<PrivateConsumerModule>()) {
        Ok(_) => panic!("private imported provider should not be visible"),
        Err(error) => error,
    };

    assert!(error.message.contains("not visible"));
    assert!(error.message.contains("SharedService"));
}

struct ExportingSharedModule;

impl Module for ExportingSharedModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<SharedService>()
            .export::<SharedService>()
    }
}

struct ExportingConsumerModule;

impl Module for ExportingConsumerModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<ExportingSharedModule>()
            .provider::<SharedConsumer>()
    }
}

#[test]
fn direct_import_exports_are_visible_to_consumers() {
    let container = block_on(build_container::<ExportingConsumerModule>()).unwrap();

    assert!(container.resolve::<SharedConsumer>().is_ok());
}

struct GlobalSharedModule;

impl Module for GlobalSharedModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::global()
            .provider::<SharedService>()
            .export::<SharedService>()
    }
}

struct GlobalConsumerModule;

impl Module for GlobalConsumerModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().provider::<SharedConsumer>()
    }
}

struct GlobalRootModule;

impl Module for GlobalRootModule {
    fn register() -> ModuleMetadata {
        // The global module intentionally comes second: visibility must not depend
        // on depth-first registration order.
        ModuleMetadata::new()
            .import::<GlobalConsumerModule>()
            .import::<GlobalSharedModule>()
    }
}

#[test]
fn global_exports_are_visible_independently_of_import_order() {
    let container = block_on(build_container::<GlobalRootModule>()).unwrap();

    assert!(container.resolve::<SharedConsumer>().is_ok());
}

static LIFECYCLE_CREATES: AtomicUsize = AtomicUsize::new(0);
static LIFECYCLE_INITS: AtomicUsize = AtomicUsize::new(0);
static LIFECYCLE_BOOTSTRAPS: AtomicUsize = AtomicUsize::new(0);
static LIFECYCLE_SHUTDOWNS: AtomicUsize = AtomicUsize::new(0);

struct SingletonLifecycleService;

impl Injectable for SingletonLifecycleService {
    fn dependencies() -> Vec<ProviderDependency> {
        provider_dependencies![]
    }

    fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async {
            LIFECYCLE_CREATES.fetch_add(1, Ordering::SeqCst);
            Ok(Self)
        })
    }

    fn on_module_init(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            LIFECYCLE_INITS.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }

    fn on_bootstrap(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            LIFECYCLE_BOOTSTRAPS.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }

    fn on_shutdown(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            LIFECYCLE_SHUTDOWNS.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

struct SingletonModule;

impl Module for SingletonModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().provider::<SingletonLifecycleService>()
    }
}

struct RepeatedImportRootModule;

impl Module for RepeatedImportRootModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<SingletonModule>()
            .import::<SingletonModule>()
    }
}

#[test]
fn repeated_module_imports_construct_and_lifecycle_once() {
    LIFECYCLE_CREATES.store(0, Ordering::SeqCst);
    LIFECYCLE_INITS.store(0, Ordering::SeqCst);
    LIFECYCLE_BOOTSTRAPS.store(0, Ordering::SeqCst);
    LIFECYCLE_SHUTDOWNS.store(0, Ordering::SeqCst);

    let container = block_on(build_container::<RepeatedImportRootModule>()).unwrap();
    block_on(shutdown_module::<RepeatedImportRootModule>(&container)).unwrap();

    assert_eq!(LIFECYCLE_CREATES.load(Ordering::SeqCst), 1);
    assert_eq!(LIFECYCLE_INITS.load(Ordering::SeqCst), 1);
    assert_eq!(LIFECYCLE_BOOTSTRAPS.load(Ordering::SeqCst), 1);
    assert_eq!(LIFECYCLE_SHUTDOWNS.load(Ordering::SeqCst), 1);
}

static DUPLICATE_CREATES: AtomicUsize = AtomicUsize::new(0);

struct DuplicateService;

impl Injectable for DuplicateService {
    fn dependencies() -> Vec<ProviderDependency> {
        provider_dependencies![]
    }

    fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async {
            DUPLICATE_CREATES.fetch_add(1, Ordering::SeqCst);
            Ok(Self)
        })
    }
}

struct FirstDuplicateModule;

impl Module for FirstDuplicateModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().provider::<DuplicateService>()
    }
}

struct SecondDuplicateModule;

impl Module for SecondDuplicateModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().provider::<DuplicateService>()
    }
}

struct DuplicateRootModule;

impl Module for DuplicateRootModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<FirstDuplicateModule>()
            .import::<SecondDuplicateModule>()
    }
}

#[test]
fn duplicate_provider_declarations_fail_before_construction() {
    DUPLICATE_CREATES.store(0, Ordering::SeqCst);

    let error = match block_on(build_container::<DuplicateRootModule>()) {
        Ok(_) => panic!("duplicate provider declarations should fail"),
        Err(error) => error,
    };

    assert!(error.message.contains("duplicate provider registration"));
    assert_eq!(DUPLICATE_CREATES.load(Ordering::SeqCst), 0);
}

struct DuplicateGateway;

impl Injectable for DuplicateGateway {
    fn dependencies() -> Vec<ProviderDependency> {
        provider_dependencies![]
    }

    fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async { Ok(Self) })
    }
}

impl WebSocketGateway for DuplicateGateway {}

impl Gateway for DuplicateGateway {
    fn definition() -> GatewayDef {
        GatewayDef::websocket::<Self>("/duplicate")
    }
}

struct ProviderAndGatewayModule;

impl Module for ProviderAndGatewayModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<DuplicateGateway>()
            .gateway::<DuplicateGateway>()
    }
}

#[test]
fn provider_and_gateway_duplicate_declarations_fail_before_construction() {
    let error = match block_on(build_container::<ProviderAndGatewayModule>()) {
        Ok(_) => panic!("provider and gateway duplicate declarations should fail"),
        Err(error) => error,
    };

    assert!(error.message.contains("duplicate provider registration"));
}

struct UndeclaredDependency;

impl Injectable for UndeclaredDependency {
    fn dependencies() -> Vec<ProviderDependency> {
        provider_dependencies![]
    }

    fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async { Ok(Self) })
    }
}

struct IncompleteDependencyDeclaration;

impl Injectable for IncompleteDependencyDeclaration {
    fn dependencies() -> Vec<ProviderDependency> {
        // This is intentionally wrong: construction below resolves a provider
        // that is absent from the declaration.
        provider_dependencies![]
    }

    fn create(container: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async move {
            let _dependency = container.resolve::<UndeclaredDependency>()?;
            Ok(Self)
        })
    }
}

struct IncompleteDependencyModule;

impl Module for IncompleteDependencyModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<UndeclaredDependency>()
            .provider::<IncompleteDependencyDeclaration>()
    }
}

#[test]
fn construction_cannot_resolve_an_undeclared_dependency() {
    let error = match block_on(build_container::<IncompleteDependencyModule>()) {
        Ok(_) => panic!("undeclared dependency resolution should fail"),
        Err(error) => error,
    };

    assert!(
        error
            .message
            .contains("without declaring it in dependencies()")
    );
    assert!(error.message.contains("UndeclaredDependency"));
}
