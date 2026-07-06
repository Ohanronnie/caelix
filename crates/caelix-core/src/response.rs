use crate::exception::HttpException;
use http::StatusCode;

pub struct HttpResponse {
    pub status: StatusCode,
    pub body: Vec<u8>,
    pub content_type: &'static str,
}

impl HttpResponse {
    pub fn new(status: StatusCode, body: Vec<u8>, content_type: &'static str) -> Self {
        Self {
            status,
            body,
            content_type,
        }
    }

    /// Serializes `body` as JSON. This is the single place JSON encoding
    /// happens — everything else below routes through here.
    pub fn json(status: StatusCode, body: impl serde::Serialize) -> Self {
        match serde_json::to_vec(&body) {
            Ok(body) => Self::new(status, body, "application/json"),
            Err(_) => json_serialization_error_response(),
        }
    }

    pub fn text(status: StatusCode, body: impl Into<String>) -> Self {
        Self::new(status, body.into().into_bytes(), "text/plain")
    }

    pub fn bytes(status: StatusCode, body: impl Into<Vec<u8>>) -> Self {
        Self::new(status, body.into(), "application/octet-stream")
    }
}

pub trait IntoCaelixResponse {
    fn into_response(self) -> HttpResponse;
}

/// A body whose shape isn't known at the `Response<T>` type level —
/// used by `Response::Raw` so a handler can return e.g. `Response<()>`
/// while still sending an arbitrary JSON/text/bytes payload.
pub enum Body {
    Json(Vec<u8>),
    Text(String),
    Bytes(Vec<u8>),
}

impl Body {
    /// Renamed from `into_response` to avoid reading like the trait
    /// method of the same name a few lines below — this one takes an
    /// extra `status` arg and isn't part of `IntoCaelixResponse`.
    fn respond_with(self, status: StatusCode) -> HttpResponse {
        match self {
            Body::Json(bytes) => HttpResponse::new(status, bytes, "application/json"),
            Body::Text(text) => HttpResponse::text(status, text),
            Body::Bytes(bytes) => HttpResponse::bytes(status, bytes),
        }
    }
}

pub enum Response<T> {
    Body(T),
    WithStatus(StatusCode, T),
    Raw(StatusCode, Body),
    Empty,
}

impl Response<()> {
    pub fn no_content() -> Self {
        Response::Empty
    }

    pub fn text(status: StatusCode, value: impl Into<String>) -> Self {
        Response::Raw(status, Body::Text(value.into()))
    }

    pub fn bytes(status: StatusCode, value: impl Into<Vec<u8>>) -> Self {
        Response::Raw(status, Body::Bytes(value.into()))
    }

    pub fn json(status: StatusCode, value: impl serde::Serialize) -> Self {
        let bytes = match serde_json::to_vec(&value) {
            Ok(bytes) => bytes,
            Err(_) => {
                return Response::Raw(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Body::Json(json_serialization_error_body()),
                );
            }
        };
        Response::Raw(status, Body::Json(bytes))
    }
}

fn json_serialization_error_response() -> HttpResponse {
    HttpResponse::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        json_serialization_error_body(),
        "application/json",
    )
}

fn json_serialization_error_body() -> Vec<u8> {
    br#"{"status":500,"error":"Internal Server Error","message":"Internal Server Error"}"#.to_vec()
}

impl IntoCaelixResponse for HttpResponse {
    fn into_response(self) -> HttpResponse {
        self
    }
}

impl IntoCaelixResponse for String {
    fn into_response(self) -> HttpResponse {
        HttpResponse::text(StatusCode::OK, self)
    }
}

impl IntoCaelixResponse for &'static str {
    fn into_response(self) -> HttpResponse {
        HttpResponse::text(StatusCode::OK, self)
    }
}

impl IntoCaelixResponse for HttpException {
    fn into_response(self) -> HttpResponse {
        #[derive(serde::Serialize)]
        struct ErrorBody {
            status: u16,
            error: &'static str,
            message: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            errors: Option<std::collections::BTreeMap<String, Vec<String>>>,
        }

        let (message, errors) = if self.status.is_server_error() {
            ("Internal Server Error".to_string(), None)
        } else {
            (self.message, self.errors)
        };

        HttpResponse::json(
            self.status,
            ErrorBody {
                status: self.status.as_u16(),
                error: self.error,
                message,
                errors,
            },
        )
    }
}

impl<T: serde::Serialize> IntoCaelixResponse for Response<T> {
    fn into_response(self) -> HttpResponse {
        match self {
            Response::Body(value) => HttpResponse::json(StatusCode::OK, value),
            Response::WithStatus(status, value) => HttpResponse::json(status, value),
            Response::Raw(status, body) => body.respond_with(status),
            Response::Empty => {
                HttpResponse::new(StatusCode::NO_CONTENT, Vec::new(), "application/json")
            }
        }
    }
}
