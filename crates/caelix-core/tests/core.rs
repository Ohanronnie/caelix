use std::{any::Any, future::Future, sync::Arc};

use caelix_core::*;
use serde::Serialize;
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
