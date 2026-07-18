use std::{collections::BTreeMap, future::Future, pin::Pin};

use actix_web::{FromRequest, HttpRequest, dev::Payload, web};
use bytes::{Bytes, BytesMut};
use caelix_core::{BadRequestException, IntoCaelixResponse, Result};
#[cfg(feature = "uploads")]
use caelix_core::{MultipartForm, UploadConfig, upload_limit_error};
use futures_util::StreamExt;

use crate::{application::UploadRuntimeConfig, to_actix_response};

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

impl FromRequest for RequestPayload {
    type Error = actix_web::Error;
    type Future = Pin<Box<dyn Future<Output = std::result::Result<Self, Self::Error>>>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let content_type = req
            .headers()
            .get(actix_web::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        let upload = req
            .app_data::<web::Data<UploadRuntimeConfig>>()
            .map(|config| config.get_ref().clone())
            .unwrap_or_else(|| UploadRuntimeConfig {
                #[cfg(feature = "uploads")]
                config: UploadConfig::default(),
                body_limit: crate::application::DEFAULT_BODY_LIMIT_BYTES,
            });
        let body_limit = upload.body_limit;
        let mut payload = payload.take();
        Box::pin(async move {
            let mut body = BytesMut::new();
            while let Some(chunk) = payload.next().await {
                let chunk = chunk
                    .map_err(|_| actix_error(BadRequestException::new("invalid request body")))?;
                if body.len().saturating_add(chunk.len()) > body_limit {
                    #[cfg(feature = "uploads")]
                    return Err(actix_error(upload_limit_error(body_limit)));
                    #[cfg(not(feature = "uploads"))]
                    return Err(actix_error(caelix_core::PayloadTooLargeException::new(
                        format!("request body exceeds the configured limit of {body_limit} bytes"),
                    )));
                }
                body.extend_from_slice(&chunk);
            }
            Ok(Self {
                content_type,
                body: body.freeze(),
                #[cfg(feature = "uploads")]
                upload,
            })
        })
    }
}

fn actix_error(error: caelix_core::HttpException) -> actix_web::Error {
    actix_web::error::InternalError::from_response(
        "Caelix request payload error",
        to_actix_response(error.into_response()),
    )
    .into()
}
