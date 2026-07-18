use axum::{
    body::{Body, to_bytes},
    extract::FromRequest,
    http::Request,
    response::Response,
};
use bytes::Bytes;
use caelix_core::{BadRequestException, IntoCaelixResponse, Result};
#[cfg(feature = "uploads")]
use caelix_core::{MultipartForm, upload_limit_error};
use std::collections::BTreeMap;

use crate::application::UploadRuntimeConfig;
use crate::to_axum_response;

/// Buffered request data used by generated body and multipart controller wrappers.
#[doc(hidden)]
pub struct RequestPayload {
    content_type: Option<String>,
    body: Bytes,
    #[cfg(feature = "uploads")]
    upload: UploadRuntimeConfig,
}

impl RequestPayload {
    #[cfg(feature = "uploads")]
    /// Returns whether the request declares a multipart body.
    pub fn is_multipart(&self) -> bool {
        self.content_type.as_deref().is_some_and(|value| {
            value
                .to_ascii_lowercase()
                .starts_with("multipart/form-data")
        })
    }

    /// Returns whether JSON is allowed for this request body.
    pub fn is_json_or_missing_content_type(&self) -> bool {
        self.content_type.as_deref().is_none_or(|value| {
            value.split(';').next().is_some_and(|media_type| {
                media_type.trim().eq_ignore_ascii_case("application/json")
            })
        })
    }

    /// Decodes a JSON request body using Caelix's normalized error shape.
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.body).map_err(|error| json_error(&error.to_string()))
    }

    #[cfg(feature = "uploads")]
    /// Parses this request as multipart, using the application or route limit.
    pub async fn multipart(self, route_limit: Option<usize>) -> Result<MultipartForm> {
        let limit = route_limit.unwrap_or(self.upload.body_limit);
        MultipartForm::parse(
            self.content_type.as_deref().unwrap_or_default(),
            self.body,
            &self.upload.config,
            limit,
        )
        .await
    }
}

fn json_error(message: &str) -> caelix_core::HttpException {
    let field = message
        .find("missing field `")
        .and_then(|start| message[start + "missing field `".len()..].split('`').next())
        .filter(|field| !field.is_empty());
    if let Some(field) = field {
        let mut errors = BTreeMap::new();
        errors.insert(field.to_string(), vec!["is required".to_string()]);
        BadRequestException::new("Validation failed").with_errors(errors)
    } else {
        BadRequestException::new("invalid JSON request body")
    }
}

impl<S> FromRequest<S> for RequestPayload
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(
        request: Request<Body>,
        _: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let (parts, body) = request.into_parts();
        let upload = parts
            .extensions
            .get::<UploadRuntimeConfig>()
            .cloned()
            .unwrap_or_else(|| UploadRuntimeConfig {
                #[cfg(feature = "uploads")]
                config: caelix_core::UploadConfig::default(),
                body_limit: crate::application::DEFAULT_BODY_LIMIT_BYTES,
            });
        let content_type = parts
            .headers
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let body_limit = upload.body_limit;
        let body = to_bytes(body, body_limit).await.map_err(|_| {
            #[cfg(feature = "uploads")]
            return to_axum_response(upload_limit_error(body_limit).into_response());
            #[cfg(not(feature = "uploads"))]
            to_axum_response(
                caelix_core::PayloadTooLargeException::new(format!(
                    "request body exceeds the configured limit of {body_limit} bytes"
                ))
                .into_response(),
            )
        })?;
        Ok(Self {
            content_type,
            body,
            #[cfg(feature = "uploads")]
            upload,
        })
    }
}
