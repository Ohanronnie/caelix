#![allow(clippy::new_ret_no_self)]

use std::collections::BTreeMap;

use http::StatusCode;

#[derive(Debug)]
/// Public Caelix type `HttpException`.
pub struct HttpException {
    /// The `status` value.
    pub status: StatusCode,
    /// The `message` value.
    pub message: String,
    /// The `error` value.
    pub error: &'static str,
    /// The `errors` value.
    pub errors: Option<BTreeMap<String, Vec<String>>>,
    /// The `source` value.
    pub source: Option<anyhow::Error>,
}

impl HttpException {
    /// Runs the `new` public API operation.
    pub fn new(status: StatusCode, error: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            error,
            message: message.into(),
            errors: None,
            source: None,
        }
    }

    /// Runs the `with_source` public API operation.
    pub fn with_source(mut self, err: impl Into<anyhow::Error>) -> Self {
        self.source = Some(err.into());
        self
    }

    /// Runs the `with_errors` public API operation.
    pub fn with_errors(mut self, errors: BTreeMap<String, Vec<String>>) -> Self {
        self.errors = Some(errors);
        self
    }
}

pub(crate) fn startup_error(message: impl Into<String>) -> HttpException {
    HttpException::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Internal Server Error",
        message,
    )
}

/// Public Caelix type `BadRequestException`.
pub struct BadRequestException;
impl BadRequestException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::BAD_REQUEST, "Bad Request", message)
    }
}

/// Public Caelix type `UnauthorizedException`.
pub struct UnauthorizedException;
impl UnauthorizedException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::UNAUTHORIZED, "Unauthorized", message)
    }
}

/// Public Caelix type `PaymentRequiredException`.
pub struct PaymentRequiredException;
impl PaymentRequiredException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::PAYMENT_REQUIRED, "Payment Required", message)
    }
}

/// Public Caelix type `ForbiddenException`.
pub struct ForbiddenException;
impl ForbiddenException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::FORBIDDEN, "Forbidden", message)
    }
}

/// Public Caelix type `NotFoundException`.
pub struct NotFoundException;
impl NotFoundException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::NOT_FOUND, "Not Found", message)
    }
}

/// Public Caelix type `MethodNotAllowedException`.
pub struct MethodNotAllowedException;
impl MethodNotAllowedException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "Method Not Allowed",
            message,
        )
    }
}

/// Public Caelix type `NotAcceptableException`.
pub struct NotAcceptableException;
impl NotAcceptableException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::NOT_ACCEPTABLE, "Not Acceptable", message)
    }
}

/// Public Caelix type `ProxyAuthenticationRequiredException`.
pub struct ProxyAuthenticationRequiredException;
impl ProxyAuthenticationRequiredException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::PROXY_AUTHENTICATION_REQUIRED,
            "Proxy Authentication Required",
            message,
        )
    }
}

/// Public Caelix type `RequestTimeoutException`.
pub struct RequestTimeoutException;
impl RequestTimeoutException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::REQUEST_TIMEOUT, "Request Timeout", message)
    }
}

/// Public Caelix type `ConflictException`.
pub struct ConflictException;
impl ConflictException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::CONFLICT, "Conflict", message)
    }
}

/// Public Caelix type `GoneException`.
pub struct GoneException;
impl GoneException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::GONE, "Gone", message)
    }
}

/// Public Caelix type `LengthRequiredException`.
pub struct LengthRequiredException;
impl LengthRequiredException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::LENGTH_REQUIRED, "Length Required", message)
    }
}

/// Public Caelix type `PreconditionFailedException`.
pub struct PreconditionFailedException;
impl PreconditionFailedException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::PRECONDITION_FAILED,
            "Precondition Failed",
            message,
        )
    }
}

/// Public Caelix type `PayloadTooLargeException`.
pub struct PayloadTooLargeException;
impl PayloadTooLargeException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::PAYLOAD_TOO_LARGE, "Payload Too Large", message)
    }
}

/// Public Caelix type `UriTooLongException`.
pub struct UriTooLongException;
impl UriTooLongException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::URI_TOO_LONG, "URI Too Long", message)
    }
}

/// Public Caelix type `UnsupportedMediaTypeException`.
pub struct UnsupportedMediaTypeException;
impl UnsupportedMediaTypeException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Unsupported Media Type",
            message,
        )
    }
}

/// Public Caelix type `RangeNotSatisfiableException`.
pub struct RangeNotSatisfiableException;
impl RangeNotSatisfiableException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "Range Not Satisfiable",
            message,
        )
    }
}

/// Public Caelix type `ExpectationFailedException`.
pub struct ExpectationFailedException;
impl ExpectationFailedException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::EXPECTATION_FAILED,
            "Expectation Failed",
            message,
        )
    }
}

/// Public Caelix type `ImATeapotException`.
pub struct ImATeapotException;
impl ImATeapotException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::IM_A_TEAPOT, "I'm a teapot", message)
    }
}

/// Public Caelix type `MisdirectedRequestException`.
pub struct MisdirectedRequestException;
impl MisdirectedRequestException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::MISDIRECTED_REQUEST,
            "Misdirected Request",
            message,
        )
    }
}

/// Public Caelix type `UnprocessableEntityException`.
pub struct UnprocessableEntityException;
impl UnprocessableEntityException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Unprocessable Entity",
            message,
        )
    }
}

/// Public Caelix type `LockedException`.
pub struct LockedException;
impl LockedException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::LOCKED, "Locked", message)
    }
}

/// Public Caelix type `FailedDependencyException`.
pub struct FailedDependencyException;
impl FailedDependencyException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::FAILED_DEPENDENCY, "Failed Dependency", message)
    }
}

/// Public Caelix type `TooEarlyException`.
pub struct TooEarlyException;
impl TooEarlyException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::TOO_EARLY, "Too Early", message)
    }
}

/// Public Caelix type `UpgradeRequiredException`.
pub struct UpgradeRequiredException;
impl UpgradeRequiredException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::UPGRADE_REQUIRED, "Upgrade Required", message)
    }
}

/// Public Caelix type `PreconditionRequiredException`.
pub struct PreconditionRequiredException;
impl PreconditionRequiredException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::PRECONDITION_REQUIRED,
            "Precondition Required",
            message,
        )
    }
}

/// Public Caelix type `TooManyRequestsException`.
pub struct TooManyRequestsException;
impl TooManyRequestsException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::TOO_MANY_REQUESTS, "Too Many Requests", message)
    }
}

/// Public Caelix type `RequestHeaderFieldsTooLargeException`.
pub struct RequestHeaderFieldsTooLargeException;
impl RequestHeaderFieldsTooLargeException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            "Request Header Fields Too Large",
            message,
        )
    }
}

/// Public Caelix type `UnavailableForLegalReasonsException`.
pub struct UnavailableForLegalReasonsException;
impl UnavailableForLegalReasonsException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS,
            "Unavailable For Legal Reasons",
            message,
        )
    }
}

/// Public Caelix type `InternalServerErrorException`.
pub struct InternalServerErrorException;
impl InternalServerErrorException {
    /// Runs the `new` public API operation.
    pub fn new(err: impl Into<anyhow::Error>) -> HttpException {
        HttpException::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Internal Server Error",
            "Internal Server Error",
        )
        .with_source(err)
    }
}

#[cfg(feature = "sqlx")]
impl From<sqlx::Error> for HttpException {
    fn from(err: sqlx::Error) -> Self {
        InternalServerErrorException::new(err)
    }
}

#[cfg(feature = "validator")]
impl From<validator::ValidationErrors> for HttpException {
    fn from(err: validator::ValidationErrors) -> Self {
        BadRequestException::new("Validation failed").with_errors(format_validation_errors(&err))
    }
}

#[cfg(feature = "validator")]
fn format_validation_errors(err: &validator::ValidationErrors) -> BTreeMap<String, Vec<String>> {
    let mut errors = BTreeMap::new();
    collect_validation_errors("", err, &mut errors);
    errors
}

#[cfg(feature = "validator")]
fn collect_validation_errors(
    prefix: &str,
    err: &validator::ValidationErrors,
    field_errors: &mut BTreeMap<String, Vec<String>>,
) {
    let mut fields = err.errors().iter().collect::<Vec<_>>();
    fields.sort_by(|(left, _), (right, _)| left.as_ref().cmp(right.as_ref()));

    for (field, kind) in fields {
        let path = if prefix.is_empty() {
            (*field).to_string()
        } else {
            format!("{prefix}.{field}")
        };

        collect_validation_error_kind(&path, kind, field_errors);
    }
}

#[cfg(feature = "validator")]
fn collect_validation_error_kind(
    path: &str,
    kind: &validator::ValidationErrorsKind,
    field_errors: &mut BTreeMap<String, Vec<String>>,
) {
    match kind {
        validator::ValidationErrorsKind::Field(errors) => {
            for error in errors {
                field_errors
                    .entry(path.to_string())
                    .or_default()
                    .push(format_validation_error(error));
            }
        }
        validator::ValidationErrorsKind::Struct(errors) => {
            collect_validation_errors(path, errors, field_errors);
        }
        validator::ValidationErrorsKind::List(errors) => {
            for (index, errors) in errors {
                collect_validation_errors(&format!("{path}[{index}]"), errors, field_errors);
            }
        }
    }
}

#[cfg(feature = "validator")]
fn format_validation_error(error: &validator::ValidationError) -> String {
    if let Some(message) = &error.message {
        return message.to_string();
    }

    match error.code.as_ref() {
        "email" => "must be a valid email".to_string(),
        "length" => format_length_validation_error(error),
        "required" => "is required".to_string(),
        "url" => "must be a valid URL".to_string(),
        "regex" => "has an invalid format".to_string(),
        "contains" => match validation_param(error, "needle") {
            Some(needle) => format!("must contain {needle}"),
            None => "must contain the required value".to_string(),
        },
        "does_not_contain" => match validation_param(error, "needle") {
            Some(needle) => format!("must not contain {needle}"),
            None => "contains a forbidden value".to_string(),
        },
        "must_match" => match validation_param(error, "other") {
            Some(other) => format!("must match {other}"),
            None => "does not match".to_string(),
        },
        "ip" => "must be a valid IP address".to_string(),
        "ipv4" => "must be a valid IPv4 address".to_string(),
        "ipv6" => "must be a valid IPv6 address".to_string(),
        code => format!("is invalid ({code})"),
    }
}

#[cfg(feature = "validator")]
fn format_length_validation_error(error: &validator::ValidationError) -> String {
    match (
        validation_param(error, "equal"),
        validation_param(error, "min"),
        validation_param(error, "max"),
    ) {
        (Some(equal), _, _) => format!("must be exactly {equal} characters"),
        (None, Some(min), Some(max)) => {
            format!("must be between {min} and {max} characters")
        }
        (None, Some(min), None) => format!("must be at least {min} characters"),
        (None, None, Some(max)) => format!("must be at most {max} characters"),
        (None, None, None) => "has an invalid length".to_string(),
    }
}

#[cfg(feature = "validator")]
fn validation_param(error: &validator::ValidationError, name: &str) -> Option<String> {
    let value = error.params.get(name)?;

    match value {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        serde_json::Value::Null => None,
        value => Some(value.to_string()),
    }
}

/// Public Caelix type `NotImplementedException`.
pub struct NotImplementedException;
impl NotImplementedException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::NOT_IMPLEMENTED, "Not Implemented", message)
    }
}

/// Public Caelix type `BadGatewayException`.
pub struct BadGatewayException;
impl BadGatewayException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::BAD_GATEWAY, "Bad Gateway", message)
    }
}

/// Public Caelix type `ServiceUnavailableException`.
pub struct ServiceUnavailableException;
impl ServiceUnavailableException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Service Unavailable",
            message,
        )
    }
}

/// Public Caelix type `GatewayTimeoutException`.
pub struct GatewayTimeoutException;
impl GatewayTimeoutException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::GATEWAY_TIMEOUT, "Gateway Timeout", message)
    }
}

/// Public Caelix type `HttpVersionNotSupportedException`.
pub struct HttpVersionNotSupportedException;
impl HttpVersionNotSupportedException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::HTTP_VERSION_NOT_SUPPORTED,
            "HTTP Version Not Supported",
            message,
        )
    }
}

/// Public Caelix type `VariantAlsoNegotiatesException`.
pub struct VariantAlsoNegotiatesException;
impl VariantAlsoNegotiatesException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::VARIANT_ALSO_NEGOTIATES,
            "Variant Also Negotiates",
            message,
        )
    }
}

/// Public Caelix type `InsufficientStorageException`.
pub struct InsufficientStorageException;
impl InsufficientStorageException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::INSUFFICIENT_STORAGE,
            "Insufficient Storage",
            message,
        )
    }
}

/// Public Caelix type `LoopDetectedException`.
pub struct LoopDetectedException;
impl LoopDetectedException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::LOOP_DETECTED, "Loop Detected", message)
    }
}

/// Public Caelix type `NotExtendedException`.
pub struct NotExtendedException;
impl NotExtendedException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::NOT_EXTENDED, "Not Extended", message)
    }
}

/// Public Caelix type `NetworkAuthenticationRequiredException`.
pub struct NetworkAuthenticationRequiredException;
impl NetworkAuthenticationRequiredException {
    /// Runs the `new` public API operation.
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::NETWORK_AUTHENTICATION_REQUIRED,
            "Network Authentication Required",
            message,
        )
    }
}
