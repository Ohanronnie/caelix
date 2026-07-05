use http::StatusCode;

#[derive(Debug)]
pub struct HttpException {
    pub status: StatusCode,
    pub message: String,
    pub error: &'static str,
    pub source: Option<anyhow::Error>,
}

impl HttpException {
    pub fn new(status: StatusCode, error: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            error,
            message: message.into(),
            source: None,
        }
    }

    pub fn with_source(mut self, err: impl Into<anyhow::Error>) -> Self {
        self.source = Some(err.into());
        self
    }
}

pub struct BadRequestException;
impl BadRequestException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::BAD_REQUEST, "Bad Request", message)
    }
}

pub struct UnauthorizedException;
impl UnauthorizedException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::UNAUTHORIZED, "Unauthorized", message)
    }
}

pub struct PaymentRequiredException;
impl PaymentRequiredException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::PAYMENT_REQUIRED, "Payment Required", message)
    }
}

pub struct ForbiddenException;
impl ForbiddenException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::FORBIDDEN, "Forbidden", message)
    }
}

pub struct NotFoundException;
impl NotFoundException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::NOT_FOUND, "Not Found", message)
    }
}

pub struct MethodNotAllowedException;
impl MethodNotAllowedException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "Method Not Allowed",
            message,
        )
    }
}

pub struct NotAcceptableException;
impl NotAcceptableException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::NOT_ACCEPTABLE, "Not Acceptable", message)
    }
}

pub struct ProxyAuthenticationRequiredException;
impl ProxyAuthenticationRequiredException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::PROXY_AUTHENTICATION_REQUIRED,
            "Proxy Authentication Required",
            message,
        )
    }
}

pub struct RequestTimeoutException;
impl RequestTimeoutException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::REQUEST_TIMEOUT, "Request Timeout", message)
    }
}

pub struct ConflictException;
impl ConflictException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::CONFLICT, "Conflict", message)
    }
}

pub struct GoneException;
impl GoneException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::GONE, "Gone", message)
    }
}

pub struct LengthRequiredException;
impl LengthRequiredException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::LENGTH_REQUIRED, "Length Required", message)
    }
}

pub struct PreconditionFailedException;
impl PreconditionFailedException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::PRECONDITION_FAILED,
            "Precondition Failed",
            message,
        )
    }
}

pub struct PayloadTooLargeException;
impl PayloadTooLargeException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::PAYLOAD_TOO_LARGE, "Payload Too Large", message)
    }
}

pub struct UriTooLongException;
impl UriTooLongException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::URI_TOO_LONG, "URI Too Long", message)
    }
}

pub struct UnsupportedMediaTypeException;
impl UnsupportedMediaTypeException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Unsupported Media Type",
            message,
        )
    }
}

pub struct RangeNotSatisfiableException;
impl RangeNotSatisfiableException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "Range Not Satisfiable",
            message,
        )
    }
}

pub struct ExpectationFailedException;
impl ExpectationFailedException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::EXPECTATION_FAILED,
            "Expectation Failed",
            message,
        )
    }
}

pub struct ImATeapotException;
impl ImATeapotException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::IM_A_TEAPOT, "I'm a teapot", message)
    }
}

pub struct MisdirectedRequestException;
impl MisdirectedRequestException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::MISDIRECTED_REQUEST,
            "Misdirected Request",
            message,
        )
    }
}

pub struct UnprocessableEntityException;
impl UnprocessableEntityException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Unprocessable Entity",
            message,
        )
    }
}

pub struct LockedException;
impl LockedException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::LOCKED, "Locked", message)
    }
}

pub struct FailedDependencyException;
impl FailedDependencyException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::FAILED_DEPENDENCY, "Failed Dependency", message)
    }
}

pub struct TooEarlyException;
impl TooEarlyException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::TOO_EARLY, "Too Early", message)
    }
}

pub struct UpgradeRequiredException;
impl UpgradeRequiredException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::UPGRADE_REQUIRED, "Upgrade Required", message)
    }
}

pub struct PreconditionRequiredException;
impl PreconditionRequiredException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::PRECONDITION_REQUIRED,
            "Precondition Required",
            message,
        )
    }
}

pub struct TooManyRequestsException;
impl TooManyRequestsException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::TOO_MANY_REQUESTS, "Too Many Requests", message)
    }
}

pub struct RequestHeaderFieldsTooLargeException;
impl RequestHeaderFieldsTooLargeException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            "Request Header Fields Too Large",
            message,
        )
    }
}

pub struct UnavailableForLegalReasonsException;
impl UnavailableForLegalReasonsException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::UNAVAILABLE_FOR_LEGAL_REASONS,
            "Unavailable For Legal Reasons",
            message,
        )
    }
}

pub struct InternalServerErrorException;
impl InternalServerErrorException {
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

pub struct NotImplementedException;
impl NotImplementedException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::NOT_IMPLEMENTED, "Not Implemented", message)
    }
}

pub struct BadGatewayException;
impl BadGatewayException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::BAD_GATEWAY, "Bad Gateway", message)
    }
}

pub struct ServiceUnavailableException;
impl ServiceUnavailableException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Service Unavailable",
            message,
        )
    }
}

pub struct GatewayTimeoutException;
impl GatewayTimeoutException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::GATEWAY_TIMEOUT, "Gateway Timeout", message)
    }
}

pub struct HttpVersionNotSupportedException;
impl HttpVersionNotSupportedException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::HTTP_VERSION_NOT_SUPPORTED,
            "HTTP Version Not Supported",
            message,
        )
    }
}

pub struct VariantAlsoNegotiatesException;
impl VariantAlsoNegotiatesException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::VARIANT_ALSO_NEGOTIATES,
            "Variant Also Negotiates",
            message,
        )
    }
}

pub struct InsufficientStorageException;
impl InsufficientStorageException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::INSUFFICIENT_STORAGE,
            "Insufficient Storage",
            message,
        )
    }
}

pub struct LoopDetectedException;
impl LoopDetectedException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::LOOP_DETECTED, "Loop Detected", message)
    }
}

pub struct NotExtendedException;
impl NotExtendedException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(StatusCode::NOT_EXTENDED, "Not Extended", message)
    }
}

pub struct NetworkAuthenticationRequiredException;
impl NetworkAuthenticationRequiredException {
    pub fn new(message: impl Into<String>) -> HttpException {
        HttpException::new(
            StatusCode::NETWORK_AUTHENTICATION_REQUIRED,
            "Network Authentication Required",
            message,
        )
    }
}
