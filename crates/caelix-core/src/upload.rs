use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use bytes::Bytes;
use futures_util::stream;
use tokio::io::AsyncWriteExt;

use crate::{
    BadRequestException, HttpException, InternalServerErrorException, PayloadTooLargeException,
    Result,
};

static NEXT_UPLOAD_ID: AtomicU64 = AtomicU64::new(0);

/// Controls where multipart file parts are staged before an application moves
/// them to durable storage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadConfig {
    temp_dir: PathBuf,
}

impl Default for UploadConfig {
    fn default() -> Self {
        Self {
            temp_dir: std::env::temp_dir().join("caelix-uploads"),
        }
    }
}

impl UploadConfig {
    /// Changes the isolated temporary directory used for uploaded file parts.
    pub fn upload_temp_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.temp_dir = path.into();
        self
    }

    /// Returns the directory used for temporary uploaded files.
    pub fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }
}

/// A file part received from a multipart request.
#[derive(Debug)]
pub struct UploadedFile {
    field_name: String,
    file_name: Option<String>,
    content_type: Option<String>,
    headers: Vec<(String, String)>,
    size: u64,
    temp_path: Option<PathBuf>,
}

impl UploadedFile {
    /// Returns the multipart field name.
    pub fn field_name(&self) -> &str {
        &self.field_name
    }

    /// Returns the client-provided filename, when present.
    pub fn file_name(&self) -> Option<&str> {
        self.file_name.as_deref()
    }

    /// Returns the client-provided content type, when present.
    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    /// Returns the original multipart headers.
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    /// Returns the number of bytes written for this file.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Returns the temporary file path while this handle owns it.
    pub fn temp_path(&self) -> &Path {
        self.temp_path
            .as_deref()
            .expect("uploaded file path is available until it is persisted")
    }

    /// Reads the staged file into memory.
    ///
    /// Call this before [`persist_to`](Self::persist_to) when both the bytes
    /// and durable persistence are required, because persistence consumes the
    /// upload handle.
    pub async fn read_bytes(&self) -> Result<Bytes> {
        tokio::fs::read(self.temp_path())
            .await
            .map(Bytes::from)
            .map_err(storage_error)
    }

    /// Moves this upload to `destination` without replacing an existing file.
    ///
    /// The destination's parent directory must already exist. This consumes
    /// the upload handle; use [`read_bytes`](Self::read_bytes) first when the
    /// content must also be inspected in memory.
    pub async fn persist_to(mut self, destination: impl AsRef<Path>) -> Result<PathBuf> {
        let destination = destination.as_ref().to_path_buf();
        let source = self.temp_path().to_path_buf();
        let mut source_file = tokio::fs::File::open(&source)
            .await
            .map_err(storage_error)?;
        let mut destination_file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .await
            .map_err(storage_error)?;
        tokio::io::copy(&mut source_file, &mut destination_file)
            .await
            .map_err(storage_error)?;
        destination_file.flush().await.map_err(storage_error)?;
        tokio::fs::remove_file(&source)
            .await
            .map_err(storage_error)?;
        self.temp_path.take();
        Ok(destination)
    }
}

impl Drop for UploadedFile {
    fn drop(&mut self) {
        if let Some(path) = self.temp_path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The text and file fields of a multipart request.
#[derive(Debug, Default)]
pub struct MultipartForm {
    text_fields: BTreeMap<String, Vec<String>>,
    file_fields: BTreeMap<String, Vec<UploadedFile>>,
}

impl MultipartForm {
    /// Parses a complete multipart request body into text fields and staged
    /// file uploads.
    pub async fn parse(
        content_type: &str,
        body: Bytes,
        config: &UploadConfig,
        limit: usize,
    ) -> Result<Self> {
        let boundary = multer::parse_boundary(content_type)
            .map_err(|_| BadRequestException::new("invalid multipart request boundary"))?;
        if body.len() > limit {
            return Err(limit_error(limit));
        }
        tokio::fs::create_dir_all(config.temp_dir())
            .await
            .map_err(storage_error)?;

        let stream = stream::once(async move { Ok::<Bytes, std::io::Error>(body) });
        let mut multipart = multer::Multipart::new(stream, boundary);
        let mut form = Self::default();
        let mut received = 0usize;

        while let Some(mut field) = multipart
            .next_field()
            .await
            .map_err(|_| BadRequestException::new("invalid multipart request body"))?
        {
            let name = field.name().map(ToOwned::to_owned).ok_or_else(|| {
                BadRequestException::new("multipart part is missing a field name")
            })?;
            let file_name = field.file_name().map(ToOwned::to_owned);
            let content_type = field.content_type().map(ToString::to_string);
            let headers = field
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.as_str().to_string(),
                        String::from_utf8_lossy(value.as_bytes()).into_owned(),
                    )
                })
                .collect::<Vec<_>>();

            if file_name.is_some() {
                let path = temporary_path(config);
                let mut output = tokio::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .await
                    .map_err(storage_error)?;
                let mut size = 0u64;
                while let Some(chunk) = field
                    .chunk()
                    .await
                    .map_err(|_| BadRequestException::new("invalid multipart request body"))?
                {
                    received = received.saturating_add(chunk.len());
                    if received > limit {
                        return Err(limit_error(limit));
                    }
                    output.write_all(&chunk).await.map_err(storage_error)?;
                    size += chunk.len() as u64;
                }
                output.flush().await.map_err(storage_error)?;
                form.file_fields
                    .entry(name.clone())
                    .or_default()
                    .push(UploadedFile {
                        field_name: name,
                        file_name,
                        content_type,
                        headers,
                        size,
                        temp_path: Some(path),
                    });
            } else {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| BadRequestException::new("invalid multipart request body"))?;
                received = received.saturating_add(bytes.len());
                if received > limit {
                    return Err(limit_error(limit));
                }
                let value = String::from_utf8(bytes.to_vec())
                    .map_err(|_| BadRequestException::new("multipart text fields must be UTF-8"))?;
                form.text_fields.entry(name).or_default().push(value);
            }
        }
        Ok(form)
    }

    /// Deserializes non-file fields with `serde_html_form`, including repeated
    /// values and normal Serde field conversions.
    pub fn deserialize<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        let mut serializer = url::form_urlencoded::Serializer::new(String::new());
        for (name, values) in &self.text_fields {
            for value in values {
                serializer.append_pair(name, value);
            }
        }
        serde_html_form::from_str(&serializer.finish())
            .map_err(|_| BadRequestException::new("invalid multipart form fields"))
    }

    /// Returns all text values for `name`.
    pub fn text(&self, name: &str) -> Option<&[String]> {
        self.text_fields.get(name).map(Vec::as_slice)
    }

    /// Returns all uploaded files for `name`.
    pub fn files(&self, name: &str) -> Option<&[UploadedFile]> {
        self.file_fields.get(name).map(Vec::as_slice)
    }

    /// Removes and returns one file, rejecting duplicate values for a
    /// single-file extractor.
    pub fn take_file(&mut self, name: &str) -> Result<Option<UploadedFile>> {
        let Some(mut files) = self.file_fields.remove(name) else {
            return Ok(None);
        };
        if files.len() != 1 {
            return Err(BadRequestException::new(format!(
                "multipart field `{name}` must contain exactly one file"
            )));
        }
        Ok(files.pop())
    }

    /// Removes and returns all files for `name`.
    pub fn take_files(&mut self, name: &str) -> Vec<UploadedFile> {
        self.file_fields.remove(name).unwrap_or_default()
    }
}

fn temporary_path(config: &UploadConfig) -> PathBuf {
    let id = NEXT_UPLOAD_ID.fetch_add(1, Ordering::Relaxed);
    config.temp_dir().join(format!(
        "upload-{}-{}-{}",
        std::process::id(),
        id,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}

/// Builds a normalized multipart body-limit error.
pub fn upload_limit_error(limit: usize) -> HttpException {
    limit_error(limit)
}

fn limit_error(limit: usize) -> HttpException {
    PayloadTooLargeException::new(format!(
        "request body exceeds the configured limit of {limit} bytes"
    ))
}

fn storage_error(error: std::io::Error) -> HttpException {
    InternalServerErrorException::new(error)
}
