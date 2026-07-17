use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use caelix_core::*;

macro_rules! injectable_without_dependencies {
    ($type:ident) => {
        impl Injectable for $type {
            fn dependencies() -> Vec<ProviderDependency> {
                provider_dependencies![]
            }

            fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
                Box::pin(async { Ok(Self) })
            }
        }
    };
}

static CONSTRUCTIONS: AtomicUsize = AtomicUsize::new(0);

struct MetadataOnlyProvider;

impl Injectable for MetadataOnlyProvider {
    fn dependencies() -> Vec<ProviderDependency> {
        provider_dependencies![]
    }

    fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async {
            CONSTRUCTIONS.fetch_add(1, Ordering::SeqCst);
            Ok(Self)
        })
    }
}

struct ValidModule;

impl Module for ValidModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().provider::<MetadataOnlyProvider>()
    }
}

#[test]
fn validates_metadata_without_constructing_providers() {
    CONSTRUCTIONS.store(0, Ordering::SeqCst);

    validate_module::<ValidModule>().unwrap();

    assert_eq!(CONSTRUCTIONS.load(Ordering::SeqCst), 0);
}

struct NotRegistered;
injectable_without_dependencies!(NotRegistered);

struct InvalidExportModule;

impl Module for InvalidExportModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().export::<NotRegistered>()
    }
}

#[test]
fn rejects_invalid_exports() {
    let error = validate_module::<InvalidExportModule>().unwrap_err();

    assert!(error.message.contains("cannot export"));
}

struct PrivateProvider;
injectable_without_dependencies!(PrivateProvider);

struct PrivateConsumer {
    _provider: Arc<PrivateProvider>,
}

impl Injectable for PrivateConsumer {
    fn dependencies() -> Vec<ProviderDependency> {
        provider_dependencies![PrivateProvider]
    }

    fn create(container: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async move {
            Ok(Self {
                _provider: container.resolve::<PrivateProvider>()?,
            })
        })
    }
}

struct PrivateProviderModule;

impl Module for PrivateProviderModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().provider::<PrivateProvider>()
    }
}

struct PrivateConsumerModule;

impl Module for PrivateConsumerModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<PrivateProviderModule>()
            .provider::<PrivateConsumer>()
    }
}

#[test]
fn rejects_dependencies_that_are_not_visible_from_an_import() {
    let error = validate_module::<PrivateConsumerModule>().unwrap_err();

    assert!(error.message.contains("not visible"));
}

struct DuplicateProvider;
injectable_without_dependencies!(DuplicateProvider);

struct DuplicateProviderModule;

impl Module for DuplicateProviderModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<DuplicateProvider>()
            .provider::<DuplicateProvider>()
    }
}

#[test]
fn rejects_duplicate_provider_declarations() {
    let error = validate_module::<DuplicateProviderModule>().unwrap_err();

    assert!(error.message.contains("duplicate provider registration"));
}

struct MissingProvider;
injectable_without_dependencies!(MissingProvider);

struct MissingProviderConsumer;

impl Injectable for MissingProviderConsumer {
    fn dependencies() -> Vec<ProviderDependency> {
        provider_dependencies![MissingProvider]
    }

    fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async { Ok(Self) })
    }
}

struct MissingProviderModule;

impl Module for MissingProviderModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().provider::<MissingProviderConsumer>()
    }
}

#[test]
fn rejects_missing_declared_dependencies() {
    let error = validate_module::<MissingProviderModule>().unwrap_err();

    assert!(error.message.contains("missing provider at startup"));
    assert!(error.message.contains("MissingProvider"));
}

struct CircularImportA;
struct CircularImportB;

impl Module for CircularImportA {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().import::<CircularImportB>()
    }
}

impl Module for CircularImportB {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().import::<CircularImportA>()
    }
}

#[test]
fn rejects_circular_module_imports() {
    let error = validate_module::<CircularImportA>().unwrap_err();

    assert!(error.message.contains("circular module import"));
}

struct CycleProviderA;
struct CycleProviderB;

impl Injectable for CycleProviderA {
    fn dependencies() -> Vec<ProviderDependency> {
        provider_dependencies![CycleProviderB]
    }

    fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Injectable for CycleProviderB {
    fn dependencies() -> Vec<ProviderDependency> {
        provider_dependencies![CycleProviderA]
    }

    fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async { Ok(Self) })
    }
}

struct ProviderCycleModule;

impl Module for ProviderCycleModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<CycleProviderA>()
            .provider::<CycleProviderB>()
    }
}

#[test]
fn rejects_provider_dependency_cycles() {
    let error = validate_module::<ProviderCycleModule>().unwrap_err();

    assert!(error.message.contains("provider dependency cycle"));
}

#[derive(Clone)]
struct ValidationEvent;

struct UnregisteredEventHandler;
injectable_without_dependencies!(UnregisteredEventHandler);

impl EventHandler<ValidationEvent> for UnregisteredEventHandler {
    fn handle(&self, _: ValidationEvent) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

struct InvalidEventHandlerModule;

impl Module for InvalidEventHandlerModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<EventModule>()
            .event_handler_for::<ValidationEvent, UnregisteredEventHandler>()
    }
}

#[test]
fn rejects_event_handlers_without_a_provider_declaration() {
    let error = validate_module::<InvalidEventHandlerModule>().unwrap_err();

    assert!(error.message.contains("missing event handler provider"));
}

struct InvalidPathGateway;
injectable_without_dependencies!(InvalidPathGateway);
impl WebSocketGateway for InvalidPathGateway {}
impl Gateway for InvalidPathGateway {
    fn definition() -> GatewayDef {
        GatewayDef::websocket::<Self>("missing-slash")
    }
}

struct InvalidGatewayPathModule;

impl Module for InvalidGatewayPathModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().gateway::<InvalidPathGateway>()
    }
}

#[test]
fn rejects_invalid_gateway_paths() {
    let error = validate_module::<InvalidGatewayPathModule>().unwrap_err();

    assert!(error.message.contains("gateway path must start with '/'"));
}

struct FirstGateway;
injectable_without_dependencies!(FirstGateway);
impl WebSocketGateway for FirstGateway {}
impl Gateway for FirstGateway {
    fn definition() -> GatewayDef {
        GatewayDef::websocket::<Self>("/same-path")
    }
}

struct SecondGateway;
injectable_without_dependencies!(SecondGateway);
impl WebSocketGateway for SecondGateway {}
impl Gateway for SecondGateway {
    fn definition() -> GatewayDef {
        GatewayDef::websocket::<Self>("/same-path")
    }
}

struct DuplicateGatewayPathModule;

impl Module for DuplicateGatewayPathModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .gateway::<FirstGateway>()
            .gateway::<SecondGateway>()
    }
}

#[test]
fn rejects_duplicate_gateway_paths() {
    let error = validate_module::<DuplicateGatewayPathModule>().unwrap_err();

    assert!(error.message.contains("duplicate gateway path"));
}
