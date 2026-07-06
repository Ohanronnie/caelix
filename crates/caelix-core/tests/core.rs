use std::{
    any::Any,
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};

use caelix_core::*;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::json;

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

#[derive(Debug)]
struct Config {
    greeting: &'static str,
}

struct Greeter {
    config: Arc<Config>,
}

impl Injectable for Greeter {
    fn create(container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async move {
            Self {
                config: container.resolve::<Config>(),
            }
        })
    }
}

#[test]
fn container_registers_instances_and_injectable_providers() {
    let mut container = Container::new();
    container.register_instance(Config { greeting: "hello" });
    block_on(container.register::<Greeter>());

    let greeter = container.resolve::<Greeter>();

    assert_eq!(greeter.config.greeting, "hello");
}

#[test]
fn container_provides_framework_logger_by_default() {
    let container = Container::new();

    let logger = container.resolve::<Logger>();

    assert_eq!(logger.context(), "Application");
}

#[test]
#[should_panic(expected = "no provider registered for")]
fn container_does_not_provide_event_bus_by_default() {
    let container = Container::new();

    let _ = container.resolve::<EventBus>();
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AuthenticatedUser {
    id: i64,
}

#[test]
fn request_context_exposes_request_metadata_headers_and_typed_extensions() {
    let ctx = RequestContext::new(
        "GET".to_string(),
        "/users/me".to_string(),
        [("Authorization".to_string(), "Bearer token-123".to_string())]
            .into_iter()
            .collect(),
    );

    assert_eq!(ctx.method(), "GET");
    assert_eq!(ctx.path(), "/users/me");
    assert_eq!(ctx.header("authorization"), Some("Bearer token-123"));
    assert_eq!(ctx.header("AUTHORIZATION"), Some("Bearer token-123"));
    assert_eq!(ctx.bearer_token(), Some("token-123"));

    ctx.set(AuthenticatedUser { id: 42 });

    assert_eq!(ctx.get::<AuthenticatedUser>().unwrap().id, 42);
    assert!(ctx.get::<String>().is_none());
}

struct PrefixInterceptor;

impl Interceptor for PrefixInterceptor {
    fn intercept<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Result<HttpResponse>> {
        Box::pin(async move {
            let mut response = next.run().await?;
            let body = String::from_utf8(response.body).expect("expected UTF-8 text response");
            response.body = format!("prefix:{body}").into_bytes();
            Ok(response)
        })
    }
}

#[test]
fn interceptor_wraps_next_and_can_transform_response() {
    let ctx = RequestContext::new("GET".to_string(), "/hello".to_string(), Default::default());
    let next = Next::new(|| {
        Box::pin(async {
            Ok(HttpResponse::text(
                StatusCode::OK,
                "handler response".to_string(),
            ))
        })
    });
    let response = block_on(PrefixInterceptor.intercept(&ctx, next)).unwrap();

    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body, b"prefix:handler response");
}

struct AwaitingProvider {
    greeting: &'static str,
}

impl Injectable for AwaitingProvider {
    fn create(container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async move {
            std::future::ready(()).await;
            let config = container.resolve::<Config>();

            Self {
                greeting: config.greeting,
            }
        })
    }
}

#[test]
fn hand_written_provider_can_await_during_creation() {
    let mut container = Container::new();
    container.register_instance(Config {
        greeting: "connected",
    });
    block_on(container.register::<AwaitingProvider>());

    let provider = container.resolve::<AwaitingProvider>();

    assert_eq!(provider.greeting, "connected");
}

static LIFECYCLE_EVENTS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

struct LifecycleProvider;

impl Injectable for LifecycleProvider {
    fn create(_container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async move { Self })
    }

    fn on_module_init(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            LIFECYCLE_EVENTS.lock().unwrap().push("init");
            Ok(())
        })
    }

    fn on_bootstrap(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            LIFECYCLE_EVENTS.lock().unwrap().push("bootstrap");
            Ok(())
        })
    }

    fn on_shutdown(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async {
            LIFECYCLE_EVENTS.lock().unwrap().push("shutdown");
            Ok(())
        })
    }
}

struct LifecycleModule;
impl Module for LifecycleModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().provider::<LifecycleProvider>()
    }
}

#[test]
fn injectable_lifecycle_hooks_run_without_special_provider_registration() {
    LIFECYCLE_EVENTS.lock().unwrap().clear();

    let mut container = Container::new();
    block_on(register_module::<LifecycleModule>(&mut container));
    assert_eq!(*LIFECYCLE_EVENTS.lock().unwrap(), vec!["init"]);

    block_on(bootstrap_module::<LifecycleModule>(&container));
    assert_eq!(*LIFECYCLE_EVENTS.lock().unwrap(), vec!["init", "bootstrap"]);

    block_on(shutdown_module::<LifecycleModule>(&container));
    assert_eq!(
        *LIFECYCLE_EVENTS.lock().unwrap(),
        vec!["init", "bootstrap", "shutdown"]
    );
}

struct FactoryBuiltProvider {
    greeting: &'static str,
}

async fn build_factory_provider(
    container: Arc<Container>,
) -> std::result::Result<FactoryBuiltProvider, &'static str> {
    std::future::ready(()).await;
    let config = container.resolve::<Config>();

    Ok(FactoryBuiltProvider {
        greeting: config.greeting,
    })
}

struct FactoryModule;
impl Module for FactoryModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<Greeter>()
            .provider_async_factory::<FactoryBuiltProvider, _, _>(build_factory_provider)
    }
}

#[test]
fn module_can_register_async_factory_provider() {
    let mut container = Container::new();
    container.register_instance(Config {
        greeting: "factory",
    });
    block_on(register_module::<FactoryModule>(&mut container));

    let provider = container.resolve::<FactoryBuiltProvider>();

    assert_eq!(provider.greeting, "factory");
}

struct FailingFactoryProvider;

async fn fail_factory_provider(
    _container: Arc<Container>,
) -> std::result::Result<FailingFactoryProvider, &'static str> {
    Err("connection refused")
}

struct FailingFactoryModule;
impl Module for FailingFactoryModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider_async_factory::<FailingFactoryProvider, _, _>(fail_factory_provider)
    }
}

#[test]
fn try_build_container_returns_startup_errors_without_panicking() {
    let result = block_on(try_build_container::<FailingFactoryModule>());
    let err = match result {
        Ok(_) => panic!("expected factory failure"),
        Err(err) => err,
    };

    assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(err.message.contains("async factory failed"));
    assert!(err.message.contains("connection refused"));
}

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
struct CachedUser {
    id: i64,
    email: String,
}

#[test]
fn cache_sets_gets_deletes_and_clears_serializable_values_by_string_key() {
    let cache = Cache::new(Arc::new(MemoryCacheStore::new()));

    block_on(cache.set(
        "user:1",
        CachedUser {
            id: 1,
            email: "ronnie@example.com".to_string(),
        },
    ))
    .unwrap();

    assert_eq!(
        block_on(cache.get::<CachedUser>("user:1")).unwrap(),
        Some(CachedUser {
            id: 1,
            email: "ronnie@example.com".to_string(),
        })
    );

    block_on(cache.delete("user:1")).unwrap();
    assert_eq!(block_on(cache.get::<CachedUser>("user:1")).unwrap(), None);

    block_on(cache.set("a", "one")).unwrap();
    block_on(cache.set("b", 2_i64)).unwrap();
    block_on(cache.clear()).unwrap();
    assert_eq!(block_on(cache.get::<String>("a")).unwrap(), None);
    assert_eq!(block_on(cache.get::<i64>("b")).unwrap(), None);
}

#[test]
fn cache_expires_entries_with_ttl_when_read() {
    let store = Arc::new(MemoryCacheStore::new());
    let cache = Cache::new(store.clone());

    block_on(cache.set_with_ttl("short", "alive", Duration::from_millis(5))).unwrap();
    assert_eq!(
        block_on(cache.get::<String>("short")).unwrap(),
        Some("alive".to_string())
    );

    std::thread::sleep(Duration::from_millis(20));

    assert_eq!(block_on(cache.get::<String>("short")).unwrap(), None);
    assert!(store.is_empty());
}

#[test]
fn memory_cache_enforces_configured_capacity_and_value_size() {
    let store = Arc::new(MemoryCacheStore::with_options(MemoryCacheOptions {
        max_entries: 2,
        max_value_bytes: 16,
        default_ttl: None,
    }));
    let cache = Cache::new(store);

    block_on(cache.set("a", "one")).unwrap();
    block_on(cache.set("b", "two")).unwrap();
    block_on(cache.set("c", "three")).unwrap();

    assert_eq!(block_on(cache.get::<String>("a")).unwrap(), None);
    assert_eq!(
        block_on(cache.get::<String>("b")).unwrap(),
        Some("two".to_string())
    );
    assert_eq!(
        block_on(cache.get::<String>("c")).unwrap(),
        Some("three".to_string())
    );

    let err = block_on(cache.set("large", "this value is too large")).unwrap_err();
    assert_eq!(err.status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[test]
fn cache_module_registers_default_memory_cache() {
    let container = block_on(build_container::<CacheModule>());
    let cache = container.resolve::<Cache>();

    block_on(cache.set("answer", 42_i64)).unwrap();

    assert_eq!(block_on(cache.get::<i64>("answer")).unwrap(), Some(42));
}

#[test]
#[should_panic(expected = "missing provider at startup:")]
fn startup_provider_validation_panics_for_unregistered_declared_provider() {
    let container = Container::new();

    validate_module_providers::<FactoryModule>(&container);
}

#[test]
#[should_panic(expected = "no provider registered for")]
fn resolving_missing_provider_panics_with_type_name() {
    let container = Container::new();

    let _ = container.resolve::<Greeter>();
}

struct ImportedProvider;
impl Injectable for ImportedProvider {
    fn create(_container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async move { Self })
    }
}

struct RootProvider {
    imported: Arc<ImportedProvider>,
}
impl Injectable for RootProvider {
    fn create(container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async move {
            Self {
                imported: container.resolve::<ImportedProvider>(),
            }
        })
    }
}

struct ImportedController;
impl Injectable for ImportedController {
    fn create(_container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async move { Self })
    }
}
impl Controller for ImportedController {
    fn base_path() -> &'static str {
        "/imported"
    }

    fn register_routes(any: &mut dyn Any) {
        any.downcast_mut::<Vec<&'static str>>()
            .expect("expected route sink")
            .push("imported");
    }
}

struct RootController;
impl Injectable for RootController {
    fn create(_container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async move { Self })
    }
}
impl Controller for RootController {
    fn base_path() -> &'static str {
        "/root"
    }

    fn register_routes(any: &mut dyn Any) {
        any.downcast_mut::<Vec<&'static str>>()
            .expect("expected route sink")
            .push("root");
    }
}

struct ImportedModule;
impl Module for ImportedModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<ImportedProvider>()
            .controller::<ImportedController>()
    }
}

struct RootModule;
impl Module for RootModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<ImportedModule>()
            .provider::<RootProvider>()
            .controller::<RootController>()
    }
}

#[test]
fn modules_register_imported_providers_before_dependents() {
    let container = block_on(build_container::<RootModule>());

    let provider = container.resolve::<RootProvider>();

    assert!(Arc::strong_count(&provider.imported) >= 2);
}

#[test]
fn modules_register_imported_controllers_before_local_controllers() {
    let mut routes: Vec<&'static str> = Vec::new();

    register_module_controllers::<RootModule>(&mut routes);

    assert_eq!(routes, vec!["imported", "root"]);
}

#[derive(Clone)]
struct CoreUserCreatedEvent {
    user_id: i64,
    email: String,
}

struct EventAuditLog {
    entries: Mutex<Vec<String>>,
}

impl EventAuditLog {
    fn push(&self, entry: String) {
        self.entries.lock().unwrap().push(entry);
    }

    fn entries(&self) -> Vec<String> {
        self.entries.lock().unwrap().clone()
    }
}

impl Injectable for EventAuditLog {
    fn create(_container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async move {
            Self {
                entries: Mutex::new(Vec::new()),
            }
        })
    }
}

struct WelcomeEmailHandler {
    log: Arc<EventAuditLog>,
}

impl Injectable for WelcomeEmailHandler {
    fn create(container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async move {
            Self {
                log: container.resolve::<EventAuditLog>(),
            }
        })
    }
}

impl EventHandler<CoreUserCreatedEvent> for WelcomeEmailHandler {
    fn handle(&self, event: CoreUserCreatedEvent) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.log.push(format!("welcome:{}", event.email));
            Ok(())
        })
    }
}

impl RegisterableEventHandler for WelcomeEmailHandler {
    type Event = CoreUserCreatedEvent;
}

struct AuditUserHandler {
    log: Arc<EventAuditLog>,
}

impl Injectable for AuditUserHandler {
    fn create(container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async move {
            Self {
                log: container.resolve::<EventAuditLog>(),
            }
        })
    }
}

impl EventHandler<CoreUserCreatedEvent> for AuditUserHandler {
    fn handle(&self, event: CoreUserCreatedEvent) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.log.push(format!("audit:{}", event.user_id));
            Ok(())
        })
    }
}

struct UserEventsModule;
impl Module for UserEventsModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<EventModule>()
            .provider::<EventAuditLog>()
            .provider::<WelcomeEmailHandler>()
            .provider::<AuditUserHandler>()
            .event_handler::<WelcomeEmailHandler>()
            .event_handler_for::<CoreUserCreatedEvent, AuditUserHandler>()
    }
}

#[test]
fn module_event_handlers_fan_out_for_the_same_event() {
    let container = block_on(build_container::<UserEventsModule>());
    let bus = container.resolve::<EventBus>();

    assert_eq!(bus.handler_count::<CoreUserCreatedEvent>(), 2);

    block_on(bus.emit(CoreUserCreatedEvent {
        user_id: 1,
        email: "a@b.com".to_string(),
    }))
    .unwrap();

    let log = container.resolve::<EventAuditLog>();
    assert_eq!(log.entries(), vec!["welcome:a@b.com", "audit:1"]);
}

struct ForgotEventHandlerRegistrationModule;

impl Module for ForgotEventHandlerRegistrationModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<EventModule>()
            .provider::<EventAuditLog>()
            .provider::<WelcomeEmailHandler>()
    }
}

#[test]
fn event_handler_provider_does_not_fire_until_registered_as_an_event_handler() {
    let container = block_on(build_container::<ForgotEventHandlerRegistrationModule>());
    let bus = container.resolve::<EventBus>();

    assert_eq!(bus.handler_count::<CoreUserCreatedEvent>(), 0);

    block_on(bus.emit(CoreUserCreatedEvent {
        user_id: 1,
        email: "a@b.com".to_string(),
    }))
    .unwrap();

    assert!(container.resolve::<EventAuditLog>().entries().is_empty());
}

struct MissingEventHandler;

impl Injectable for MissingEventHandler {
    fn create(_container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async move { Self })
    }
}

impl EventHandler<CoreUserCreatedEvent> for MissingEventHandler {
    fn handle(&self, _event: CoreUserCreatedEvent) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

struct MissingEventHandlerModule;

impl Module for MissingEventHandlerModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<EventModule>()
            .event_handler_for::<CoreUserCreatedEvent, MissingEventHandler>()
    }
}

#[test]
#[should_panic(expected = "missing event handler provider at startup:")]
fn event_handler_registration_requires_a_registered_provider() {
    let mut container = Container::new();

    block_on(register_module::<MissingEventHandlerModule>(&mut container));
}

#[test]
fn event_module_registers_event_bus() {
    let container = block_on(build_container::<EventModule>());
    let bus = container.resolve::<EventBus>();

    assert_eq!(bus.handler_count::<CoreUserCreatedEvent>(), 0);
}

struct MissingEventModule;

impl Module for MissingEventModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<EventAuditLog>()
            .provider::<WelcomeEmailHandler>()
            .event_handler::<WelcomeEmailHandler>()
    }
}

#[test]
#[should_panic(expected = "no provider registered for")]
fn event_using_module_without_event_module_fails_at_startup() {
    let mut container = Container::new();

    block_on(register_module::<MissingEventModule>(&mut container));
}

#[derive(Serialize)]
struct Payload {
    name: &'static str,
}

#[test]
fn http_response_helpers_preserve_status_body_and_content_type() {
    let json_response = HttpResponse::json(StatusCode::CREATED, Payload { name: "caelix" });
    assert_eq!(json_response.status, StatusCode::CREATED);
    assert_eq!(json_response.content_type, "application/json");
    assert_eq!(json_response.body, br#"{"name":"caelix"}"#);

    let text_response = HttpResponse::text(StatusCode::ACCEPTED, "queued");
    assert_eq!(text_response.status, StatusCode::ACCEPTED);
    assert_eq!(text_response.content_type, "text/plain");
    assert_eq!(text_response.body, b"queued");

    let bytes_response = HttpResponse::bytes(StatusCode::OK, [1_u8, 2, 3]);
    assert_eq!(bytes_response.status, StatusCode::OK);
    assert_eq!(bytes_response.content_type, "application/octet-stream");
    assert_eq!(bytes_response.body, vec![1, 2, 3]);
}

#[test]
fn into_caelix_response_covers_strings_structured_values_raw_values_and_empty() {
    let response = "hello".into_response();
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.content_type, "text/plain");
    assert_eq!(response.body, b"hello");

    let response = String::from("owned").into_response();
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.content_type, "text/plain");
    assert_eq!(response.body, b"owned");

    let response = Response::Body(Payload { name: "body" }).into_response();
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.content_type, "application/json");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response.body).unwrap(),
        json!({"name": "body"})
    );

    let response =
        Response::WithStatus(StatusCode::CREATED, Payload { name: "created" }).into_response();
    assert_eq!(response.status, StatusCode::CREATED);
    assert_eq!(response.content_type, "application/json");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response.body).unwrap(),
        json!({"name": "created"})
    );

    let response = Response::text(StatusCode::ACCEPTED, "accepted").into_response();
    assert_eq!(response.status, StatusCode::ACCEPTED);
    assert_eq!(response.content_type, "text/plain");
    assert_eq!(response.body, b"accepted");

    let response = Response::bytes(StatusCode::OK, vec![4, 5, 6]).into_response();
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.content_type, "application/octet-stream");
    assert_eq!(response.body, vec![4, 5, 6]);

    let response = Response::json(StatusCode::ACCEPTED, json!({"raw": true})).into_response();
    assert_eq!(response.status, StatusCode::ACCEPTED);
    assert_eq!(response.content_type, "application/json");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response.body).unwrap(),
        json!({"raw": true})
    );

    let response = Response::no_content().into_response();
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    assert_eq!(response.content_type, "application/json");
    assert!(response.body.is_empty());
}

struct FailingSerialize;

impl Serialize for FailingSerialize {
    fn serialize<S>(&self, _serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        Err(serde::ser::Error::custom("serialization failed"))
    }
}

#[test]
fn json_response_helpers_do_not_panic_on_serialization_failure() {
    let response = HttpResponse::json(StatusCode::OK, FailingSerialize);

    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response.body).unwrap(),
        json!({
            "status": 500,
            "error": "Internal Server Error",
            "message": "Internal Server Error"
        })
    );

    let response = Response::json(StatusCode::OK, FailingSerialize).into_response();

    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response.body).unwrap(),
        json!({
            "status": 500,
            "error": "Internal Server Error",
            "message": "Internal Server Error"
        })
    );
}

#[test]
fn http_exception_into_response_serializes_error_body() {
    let response = NotFoundException::new("missing user").into_response();

    assert_eq!(response.status, StatusCode::NOT_FOUND);
    assert_eq!(response.content_type, "application/json");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response.body).unwrap(),
        json!({
            "status": 404,
            "error": "Not Found",
            "message": "missing user"
        })
    );
}

#[test]
fn server_error_responses_do_not_serialize_source_or_internal_messages() {
    let response = BadGatewayException::new("upstream database password leaked")
        .with_source(anyhow::anyhow!("driver error with internal details"));
    let response = response.into_response();

    assert_eq!(response.status, StatusCode::BAD_GATEWAY);
    assert_eq!(response.content_type, "application/json");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response.body).unwrap(),
        json!({
            "status": 502,
            "error": "Bad Gateway",
            "message": "Internal Server Error"
        })
    );
}

#[test]
fn logging_http_exceptions_preserves_sanitized_server_error_responses() {
    let exception = InternalServerErrorException::new(anyhow::anyhow!("database down"));
    log_http_exception(&exception);
    let response = exception.into_response();

    assert_eq!(response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&response.body).unwrap(),
        json!({
            "status": 500,
            "error": "Internal Server Error",
            "message": "Internal Server Error"
        })
    );

    log_http_exception(&BadRequestException::new("bad input"));
}

#[test]
fn client_exception_constructors_use_expected_status_and_error_labels() {
    macro_rules! assert_exception {
        ($ctor:ident, $status:expr, $error:expr) => {{
            let exception = $ctor::new("message");
            assert_eq!(exception.status, $status);
            assert_eq!(exception.error, $error);
            assert_eq!(exception.message, "message");
            assert!(exception.source.is_none());
        }};
    }

    assert_exception!(BadRequestException, StatusCode::BAD_REQUEST, "Bad Request");
    assert_exception!(
        UnauthorizedException,
        StatusCode::UNAUTHORIZED,
        "Unauthorized"
    );
    assert_exception!(
        PaymentRequiredException,
        StatusCode::PAYMENT_REQUIRED,
        "Payment Required"
    );
    assert_exception!(ForbiddenException, StatusCode::FORBIDDEN, "Forbidden");
    assert_exception!(NotFoundException, StatusCode::NOT_FOUND, "Not Found");
    assert_exception!(
        MethodNotAllowedException,
        StatusCode::METHOD_NOT_ALLOWED,
        "Method Not Allowed"
    );
    assert_exception!(
        NotAcceptableException,
        StatusCode::NOT_ACCEPTABLE,
        "Not Acceptable"
    );
    assert_exception!(
        ProxyAuthenticationRequiredException,
        StatusCode::PROXY_AUTHENTICATION_REQUIRED,
        "Proxy Authentication Required"
    );
    assert_exception!(
        RequestTimeoutException,
        StatusCode::REQUEST_TIMEOUT,
        "Request Timeout"
    );
    assert_exception!(ConflictException, StatusCode::CONFLICT, "Conflict");
    assert_exception!(GoneException, StatusCode::GONE, "Gone");
    assert_exception!(
        LengthRequiredException,
        StatusCode::LENGTH_REQUIRED,
        "Length Required"
    );
    assert_exception!(
        PreconditionFailedException,
        StatusCode::PRECONDITION_FAILED,
        "Precondition Failed"
    );
    assert_exception!(
        PayloadTooLargeException,
        StatusCode::PAYLOAD_TOO_LARGE,
        "Payload Too Large"
    );
    assert_exception!(
        UriTooLongException,
        StatusCode::URI_TOO_LONG,
        "URI Too Long"
    );
    assert_exception!(
        UnsupportedMediaTypeException,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "Unsupported Media Type"
    );
    assert_exception!(
        RangeNotSatisfiableException,
        StatusCode::RANGE_NOT_SATISFIABLE,
        "Range Not Satisfiable"
    );
    assert_exception!(
        ExpectationFailedException,
        StatusCode::EXPECTATION_FAILED,
        "Expectation Failed"
    );
    assert_exception!(ImATeapotException, StatusCode::IM_A_TEAPOT, "I'm a teapot");
    assert_exception!(
        MisdirectedRequestException,
        StatusCode::MISDIRECTED_REQUEST,
        "Misdirected Request"
    );
    assert_exception!(
        UnprocessableEntityException,
        StatusCode::UNPROCESSABLE_ENTITY,
        "Unprocessable Entity"
    );
    assert_exception!(LockedException, StatusCode::LOCKED, "Locked");
    assert_exception!(
        FailedDependencyException,
        StatusCode::FAILED_DEPENDENCY,
        "Failed Dependency"
    );
    assert_exception!(TooEarlyException, StatusCode::TOO_EARLY, "Too Early");
    assert_exception!(
        UpgradeRequiredException,
        StatusCode::UPGRADE_REQUIRED,
        "Upgrade Required"
    );
    assert_exception!(
        PreconditionRequiredException,
        StatusCode::PRECONDITION_REQUIRED,
        "Precondition Required"
    );
    assert_exception!(
        TooManyRequestsException,
        StatusCode::TOO_MANY_REQUESTS,
        "Too Many Requests"
    );
    assert_exception!(
        RequestHeaderFieldsTooLargeException,
        StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
        "Request Header Fields Too Large"
    );
    assert_exception!(
        UnavailableForLegalReasonsException,
        StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS,
        "Unavailable For Legal Reasons"
    );
}

#[test]
fn server_exception_constructors_use_expected_status_and_error_labels() {
    macro_rules! assert_exception {
        ($ctor:ident, $status:expr, $error:expr) => {{
            let exception = $ctor::new("message");
            assert_eq!(exception.status, $status);
            assert_eq!(exception.error, $error);
            assert_eq!(exception.message, "message");
            assert!(exception.source.is_none());
        }};
    }

    assert_exception!(
        NotImplementedException,
        StatusCode::NOT_IMPLEMENTED,
        "Not Implemented"
    );
    assert_exception!(BadGatewayException, StatusCode::BAD_GATEWAY, "Bad Gateway");
    assert_exception!(
        ServiceUnavailableException,
        StatusCode::SERVICE_UNAVAILABLE,
        "Service Unavailable"
    );
    assert_exception!(
        GatewayTimeoutException,
        StatusCode::GATEWAY_TIMEOUT,
        "Gateway Timeout"
    );
    assert_exception!(
        HttpVersionNotSupportedException,
        StatusCode::HTTP_VERSION_NOT_SUPPORTED,
        "HTTP Version Not Supported"
    );
    assert_exception!(
        VariantAlsoNegotiatesException,
        StatusCode::VARIANT_ALSO_NEGOTIATES,
        "Variant Also Negotiates"
    );
    assert_exception!(
        InsufficientStorageException,
        StatusCode::INSUFFICIENT_STORAGE,
        "Insufficient Storage"
    );
    assert_exception!(
        LoopDetectedException,
        StatusCode::LOOP_DETECTED,
        "Loop Detected"
    );
    assert_exception!(
        NotExtendedException,
        StatusCode::NOT_EXTENDED,
        "Not Extended"
    );
    assert_exception!(
        NetworkAuthenticationRequiredException,
        StatusCode::NETWORK_AUTHENTICATION_REQUIRED,
        "Network Authentication Required"
    );

    let internal = InternalServerErrorException::new(anyhow::anyhow!("database down"));
    assert_eq!(internal.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(internal.error, "Internal Server Error");
    assert_eq!(internal.message, "Internal Server Error");
    assert!(internal.source.is_some());
}
