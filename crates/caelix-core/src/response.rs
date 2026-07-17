use std::pin::Pin;

use bytes::Bytes;
use futures_core::Stream;
use futures_util::StreamExt;
use http::StatusCode;
use tokio_util::io::ReaderStream;

use crate::exception::{
    ForbiddenException, HttpException, InternalServerErrorException, NotFoundException,
};
use crate::result::Result;

/// Async sequence of body chunks. Errors use framework [`HttpException`].
pub type BoxBodyStream = Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>>;

/// Response body: fully buffered, or a live stream of chunks.
pub enum ResponseBody {
    /// Public Caelix API.
    Buffered(Vec<u8>),
    /// Public Caelix API.
    Streaming(BoxBodyStream),
}

impl ResponseBody {
    /// Runs the `as_buffered` public API operation.
    pub fn as_buffered(&self) -> Option<&[u8]> {
        match self {
            ResponseBody::Buffered(bytes) => Some(bytes.as_slice()),
            ResponseBody::Streaming(_) => None,
        }
    }

    /// Runs the `as_buffered_mut` public API operation.
    pub fn as_buffered_mut(&mut self) -> Option<&mut Vec<u8>> {
        match self {
            ResponseBody::Buffered(bytes) => Some(bytes),
            ResponseBody::Streaming(_) => None,
        }
    }

    /// Runs the `is_streaming` public API operation.
    pub fn is_streaming(&self) -> bool {
        matches!(self, ResponseBody::Streaming(_))
    }

    /// Runs the `is_empty` public API operation.
    pub fn is_empty(&self) -> bool {
        match self {
            ResponseBody::Buffered(bytes) => bytes.is_empty(),
            ResponseBody::Streaming(_) => false,
        }
    }
}

/// Public Caelix type `HttpResponse`.
pub struct HttpResponse {
    /// The `status` value.
    pub status: StatusCode,
    /// The `body` value.
    pub body: ResponseBody,
    /// The `content_type` value.
    pub content_type: &'static str,
    /// Extra response headers applied by the HTTP adapter.
    ///
    /// Owned name/value pairs so callers can set dynamic values
    /// (e.g. `Content-Disposition` with a generated filename). This is a
    /// simple list, not a full `HeaderMap` with multi-value / typed APIs.
    pub headers: Vec<(String, String)>,
}

impl HttpResponse {
    /// Runs the `new` public API operation.
    pub fn new(status: StatusCode, body: Vec<u8>, content_type: &'static str) -> Self {
        Self {
            status,
            body: ResponseBody::Buffered(body),
            content_type,
            headers: Vec::new(),
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

    /// Runs the `text` public API operation.
    pub fn text(status: StatusCode, body: impl Into<String>) -> Self {
        Self::new(status, body.into().into_bytes(), "text/plain")
    }

    /// Runs the `bytes` public API operation.
    pub fn bytes(status: StatusCode, body: impl Into<Vec<u8>>) -> Self {
        Self::new(status, body.into(), "application/octet-stream")
    }

    /// Buffered body bytes, if this response is not streaming.
    pub fn body_bytes(&self) -> Option<&[u8]> {
        self.body.as_buffered()
    }

    /// Append a response header. Values may be dynamic (`String` or `&str`).
    /// Prefer this when building a response by chaining.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.insert_header(name, value);
        self
    }

    /// Append a response header in place (e.g. from an interceptor).
    pub fn insert_header(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.headers.push((name.into(), value.into()));
    }
}

/// Public Caelix extension trait `IntoCaelixResponse`.
pub trait IntoCaelixResponse {
    /// Public Caelix API.
    fn into_response(self) -> HttpResponse;
}

/// A body whose shape isn't known at the `Response<T>` type level —
/// used by `Response::Raw` so a handler can return e.g. `Response<()>`
/// while still sending an arbitrary JSON/text/bytes payload.
pub enum Body {
    /// Public Caelix API.
    Json(Vec<u8>),
    /// Public Caelix API.
    Text(String),
    /// Public Caelix API.
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

/// Public Caelix enumeration `Response`.
pub enum Response<T> {
    /// Public Caelix API.
    Body(T),
    /// Public Caelix API.
    WithStatus(StatusCode, T),
    /// Public Caelix API.
    Raw(StatusCode, Body),
    /// Public Caelix API.
    Empty,
}

impl Response<()> {
    /// Runs the `no_content` public API operation.
    pub fn no_content() -> Self {
        Response::Empty
    }

    /// Runs the `text` public API operation.
    pub fn text(status: StatusCode, value: impl Into<String>) -> Self {
        Response::Raw(status, Body::Text(value.into()))
    }

    /// Runs the `bytes` public API operation.
    pub fn bytes(status: StatusCode, value: impl Into<Vec<u8>>) -> Self {
        Response::Raw(status, Body::Bytes(value.into()))
    }

    /// Runs the `json` public API operation.
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

    /// Stream response chunks as they become ready (chunked transfer encoding
    /// is applied by the HTTP adapter).
    pub fn stream(
        content_type: &'static str,
        stream: impl Stream<Item = Result<Bytes>> + Send + 'static,
    ) -> HttpResponse {
        HttpResponse {
            status: StatusCode::OK,
            body: ResponseBody::Streaming(Box::pin(stream)),
            content_type,
            headers: Vec::new(),
        }
    }

    /// Server-Sent Events: each item is serialized as JSON and framed as
    /// `data: <json>\n\n` with content type `text/event-stream`.
    ///
    /// Also sets `Cache-Control: no-cache` and `X-Accel-Buffering: no` so
    /// proxies/browsers do not buffer the stream. Does not yet implement the
    /// full SSE protocol (`id:`, `event:`, `retry:`, Last-Event-ID).
    pub fn sse<T>(stream: impl Stream<Item = Result<T>> + Send + 'static) -> HttpResponse
    where
        T: serde::Serialize + 'static,
    {
        let framed = stream.map(|item| {
            item.and_then(|value| {
                let json = serde_json::to_string(&value).map_err(|err| {
                    InternalServerErrorException::new(anyhow::anyhow!(
                        "failed to serialize SSE event: {err}"
                    ))
                })?;
                Ok(Bytes::from(format!("data: {json}\n\n")))
            })
        });
        Response::stream("text/event-stream", framed)
            .with_header("Cache-Control", "no-cache")
            .with_header("X-Accel-Buffering", "no")
    }

    /// Stream a file from disk in chunks (not loaded fully into memory).
    ///
    /// Open errors: `NotFound` → 404, `PermissionDenied` → 403, other IO → 500.
    pub async fn file(
        path: impl AsRef<std::path::Path>,
        content_type: &'static str,
    ) -> Result<HttpResponse> {
        let file = tokio::fs::File::open(path)
            .await
            .map_err(map_file_open_error)?;

        let stream = ReaderStream::new(file).map(|chunk| {
            chunk
                .map(Bytes::from)
                .map_err(|err| InternalServerErrorException::new(err))
        });

        Ok(Response::stream(content_type, stream))
    }
}

pub(crate) fn map_file_open_error(err: std::io::Error) -> HttpException {
    match err.kind() {
        std::io::ErrorKind::NotFound => NotFoundException::new("file not found"),
        std::io::ErrorKind::PermissionDenied => ForbiddenException::new("permission denied"),
        _ => InternalServerErrorException::new(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, ErrorKind};

    #[test]
    fn map_file_open_error_classifies_io_kinds() {
        let not_found = map_file_open_error(Error::new(ErrorKind::NotFound, "gone"));
        assert_eq!(not_found.status, StatusCode::NOT_FOUND);

        let denied = map_file_open_error(Error::new(ErrorKind::PermissionDenied, "nope"));
        assert_eq!(denied.status, StatusCode::FORBIDDEN);

        let other = map_file_open_error(Error::new(ErrorKind::Other, "disk failed"));
        assert_eq!(other.status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn with_header_accepts_dynamic_values() {
        let filename = format!("report-{}.csv", 123);
        let response = HttpResponse::text(StatusCode::OK, "a,b\n").with_header(
            "Content-Disposition",
            format!("attachment; filename=\"{filename}\""),
        );

        assert_eq!(
            response.headers,
            vec![(
                "Content-Disposition".to_string(),
                "attachment; filename=\"report-123.csv\"".to_string()
            )]
        );
    }

    #[test]
    fn insert_header_mutates_in_place() {
        let mut response = HttpResponse::text(StatusCode::OK, "ok");
        response.insert_header("X-Request-Id", "abc-123");
        assert_eq!(
            response.headers,
            vec![("X-Request-Id".to_string(), "abc-123".to_string())]
        );
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
