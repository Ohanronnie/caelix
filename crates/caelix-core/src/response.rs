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

    pub fn json(status: StatusCode, body: impl serde::Serialize) -> Self {
        let body = serde_json::to_vec(&body).expect("failed to serialize response body");
        Self::new(status, body, "application/json")
    }
}

pub trait IntoCaelixResponse {
    fn into_response(self) -> HttpResponse;
}

pub enum Body {
    Json(Vec<u8>),
    Text(String),
    Bytes(Vec<u8>),
}

impl Body {
    fn into_response(self, status: StatusCode) -> HttpResponse {
        match self {
            Body::Json(body) => HttpResponse::new(status, body, "application/json"),
            Body::Text(text) => HttpResponse::new(status, text.into_bytes(), "text/plain"),
            Body::Bytes(body) => HttpResponse::new(status, body, "application/octet-stream"),
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
        let body = serde_json::to_vec(&value).expect("serialize failed");
        Response::Raw(status, Body::Json(body))
    }
}

impl IntoCaelixResponse for HttpResponse {
    fn into_response(self) -> HttpResponse {
        self
    }
}

impl IntoCaelixResponse for HttpException {
    fn into_response(self) -> HttpResponse {
        #[derive(serde::Serialize)]
        struct ErrorBody {
            status: u16,
            error: &'static str,
            message: String,
        }

        HttpResponse::json(
            self.status,
            ErrorBody {
                status: self.status.as_u16(),
                error: self.error,
                message: self.message,
            },
        )
    }
}

impl<T: serde::Serialize> IntoCaelixResponse for Response<T> {
    fn into_response(self) -> HttpResponse {
        match self {
            Response::Body(value) => {
                let body = serde_json::to_vec(&value).expect("serialize failed");
                HttpResponse {
                    status: StatusCode::OK,
                    body,
                    content_type: "application/json",
                }
            }
            Response::WithStatus(status, value) => {
                let body = serde_json::to_vec(&value).expect("serialize failed");
                HttpResponse {
                    status,
                    body,
                    content_type: "application/json",
                }
            }
            Response::Raw(status, body) => body.into_response(status),
            Response::Empty => HttpResponse {
                status: StatusCode::NO_CONTENT,
                body: Vec::new(),
                content_type: "application/json",
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exception::NotFoundException;

    #[test]
    fn converts_json_body_response() {
        let response = Response::Body("ok").into_response();

        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.body, br#""ok""#);
        assert_eq!(response.content_type, "application/json");
    }

    #[test]
    fn converts_text_response_without_json_encoding() {
        let response = Response::text(StatusCode::OK, "ok").into_response();

        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(response.body, b"ok");
        assert_eq!(response.content_type, "text/plain");
    }

    #[test]
    fn converts_empty_response() {
        let response = Response::no_content().into_response();

        assert_eq!(response.status, StatusCode::NO_CONTENT);
        assert!(response.body.is_empty());
        assert_eq!(response.content_type, "application/json");
    }

    #[test]
    fn converts_not_found_exception_response() {
        let response = NotFoundException::new("user not found").into_response();

        assert_eq!(response.status, StatusCode::NOT_FOUND);
        assert_eq!(
            response.body,
            br#"{"status":404,"error":"Not Found","message":"user not found"}"#
        );
    }
}
