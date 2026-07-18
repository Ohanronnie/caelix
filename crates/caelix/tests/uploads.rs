#![cfg(all(feature = "uploads", any(feature = "actix", feature = "axum")))]

#[cfg(feature = "openapi")]
use caelix::openapi::utoipa;
use caelix::{
    ModuleMetadata, MultipartForm, Response, Result, TestApplication, UploadedFile, controller,
    injectable,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "openapi", derive(caelix::openapi::ToSchema))]
struct UploadDto {
    title: String,
    count: u32,
    enabled: bool,
    #[serde(default)]
    tags: Vec<String>,
}

#[injectable]
struct UploadValidationService {
    seen_paths: Arc<UploadPathStore>,
}

struct UploadPathStore {
    paths: Mutex<Vec<PathBuf>>,
}

impl caelix::Injectable for UploadPathStore {
    fn dependencies() -> Vec<caelix::ProviderDependency> {
        caelix::provider_dependencies![]
    }

    fn create(_: &caelix::Container) -> caelix::BoxFuture<'_, Result<Self>> {
        Box::pin(async {
            Ok(Self {
                paths: Mutex::new(Vec::new()),
            })
        })
    }
}

impl UploadValidationService {
    async fn inspect(&self, file: &UploadedFile) -> Result<()> {
        assert!(file.temp_path().exists());
        self.seen_paths
            .paths
            .lock()
            .expect("upload path store is not poisoned")
            .push(file.temp_path().to_path_buf());
        if file.file_name() == Some("reject.txt") {
            return Err(caelix::BadRequestException::new(
                "validator rejected upload",
            ));
        }
        Ok(())
    }

    fn seen_paths(&self) -> Vec<PathBuf> {
        self.seen_paths
            .paths
            .lock()
            .expect("upload path store is not poisoned")
            .clone()
    }
}

#[injectable]
struct UploadController {
    validation_service: Arc<UploadValidationService>,
}

#[controller("/uploads")]
impl UploadController {
    async fn validate_document(&self, file: &UploadedFile) -> Result<()> {
        self.validation_service.inspect(file).await
    }

    #[post("/required")]
    async fn required(
        &self,
        #[body] body: UploadDto,
        #[file] avatar: UploadedFile,
    ) -> Result<Response<Value>> {
        let bytes = avatar.read_bytes().await?;
        Ok(Response::Body(json!({
            "title": body.title,
            "count": body.count,
            "enabled": body.enabled,
            "tags": body.tags,
            "file": avatar.file_name(),
            "size": bytes.len(),
        })))
    }

    #[post("/optional")]
    async fn optional(
        &self,
        #[body] body: UploadDto,
        #[file] avatar: Option<UploadedFile>,
    ) -> Result<Response<Value>> {
        Ok(Response::Body(
            json!({"title": body.title, "file": avatar.is_some()}),
        ))
    }

    #[post("/form")]
    async fn form(&self, #[multipart] mut form: MultipartForm) -> Result<Response<Value>> {
        let file = form.take_file("document")?.expect("test file is present");
        Ok(Response::Body(
            json!({"note": form.text("note"), "size": file.size()}),
        ))
    }

    #[post("/limit")]
    async fn limit(
        &self,
        #[file(max_size = "5B")] document: UploadedFile,
    ) -> Result<Response<Value>> {
        Ok(Response::Body(json!({"size": document.size()})))
    }

    #[post("/route-limit")]
    #[upload(limit = 4)]
    async fn route_limit(&self, #[file] document: UploadedFile) -> Result<Response<Value>> {
        Ok(Response::Body(json!({"size": document.size()})))
    }

    #[post("/mime")]
    async fn mime(
        &self,
        #[file(content_type = "image/png")] document: UploadedFile,
    ) -> Result<Response<Value>> {
        Ok(Response::Body(json!({"size": document.size()})))
    }

    #[post("/trusted-mime")]
    async fn trusted_mime(
        &self,
        #[file(content_type = "text/plain", trust_content_type_header = true)]
        document: UploadedFile,
    ) -> Result<Response<Value>> {
        Ok(Response::Body(json!({"size": document.size()})))
    }

    #[post("/validate")]
    async fn validate(
        &self,
        #[file(validate = validate_document)] document: UploadedFile,
    ) -> Result<Response<Value>> {
        Ok(Response::Body(json!({"size": document.size()})))
    }

    #[post("/validate-many")]
    async fn validate_many(
        &self,
        #[files(validate = validate_document)] documents: Vec<UploadedFile>,
    ) -> Result<Response<Value>> {
        Ok(Response::Body(json!({"count": documents.len()})))
    }

    #[post("/documented")]
    async fn documented(
        &self,
        #[file(max_size = "2MiB", content_type = "application/pdf, image/png")]
        document: UploadedFile,
    ) -> Result<Response<Value>> {
        Ok(Response::Body(json!({"size": document.size()})))
    }
}

struct UploadModule;

impl caelix::Module for UploadModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<UploadPathStore>()
            .provider::<UploadValidationService>()
            .controller::<UploadController>()
    }
}

fn multipart_file(
    boundary: &str,
    field: &str,
    file_name: &str,
    content_type: &str,
    content: &[u8],
) -> Vec<u8> {
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{file_name}\"\r\nContent-Type: {content_type}\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

fn multipart_files(boundary: &str, field: &str, files: &[(&str, &str, &[u8])]) -> Vec<u8> {
    let mut body = Vec::new();
    for (file_name, content_type, content) in files {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{file_name}\"\r\nContent-Type: {content_type}\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(content);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

fn multipart_body(boundary: &str) -> String {
    format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nCaelix\r\n\
--{boundary}\r\nContent-Disposition: form-data; name=\"count\"\r\n\r\n7\r\n\
--{boundary}\r\nContent-Disposition: form-data; name=\"enabled\"\r\n\r\ntrue\r\n\
--{boundary}\r\nContent-Disposition: form-data; name=\"tags\"\r\n\r\nrust\r\n\
--{boundary}\r\nContent-Disposition: form-data; name=\"tags\"\r\n\r\nweb\r\n\
--{boundary}\r\nContent-Disposition: form-data; name=\"avatar\"; filename=\"avatar.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n\
--{boundary}--\r\n"
    )
}

#[caelix::test]
async fn multipart_body_binds_typed_fields_and_file() {
    let app = TestApplication::new::<UploadModule>().await.unwrap();
    let boundary = "caelix-upload-boundary";
    let response = app
        .post("/uploads/required")
        .header(
            "content-type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .set_payload(multipart_body(boundary))
        .send()
        .await
        .unwrap()
        .assert_status(caelix::StatusCode::OK)
        .json::<Value>()
        .await;
    assert_eq!(response["title"], "Caelix");
    assert_eq!(response["count"], 7);
    assert_eq!(response["enabled"], true);
    assert_eq!(response["tags"], json!(["rust", "web"]));
    assert_eq!(response["file"], "avatar.txt");
    assert_eq!(response["size"], 5);
    app.shutdown().await.unwrap();
}

#[caelix::test]
async fn optional_files_allow_json_requests() {
    let app = TestApplication::new::<UploadModule>().await.unwrap();
    let response = app
        .post("/uploads/optional")
        .json(json!({"title": "JSON", "count": 1, "enabled": false}))
        .send()
        .await
        .unwrap()
        .assert_status(caelix::StatusCode::OK)
        .json::<Value>()
        .await;
    assert_eq!(response, json!({"title": "JSON", "file": false}));
    app.shutdown().await.unwrap();
}

#[caelix::test]
async fn required_files_reject_json_requests() {
    let app = TestApplication::new::<UploadModule>().await.unwrap();
    let response = app
        .post("/uploads/required")
        .json(json!({"title": "JSON", "count": 1, "enabled": false}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        caelix::StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    app.shutdown().await.unwrap();
}

#[caelix::test]
async fn file_extractors_enforce_declared_size_and_mime_rules() {
    let app = TestApplication::new::<UploadModule>().await.unwrap();
    let boundary = "caelix-validation-boundary";
    let content_type = format!("multipart/form-data; boundary={boundary}");

    app.post("/uploads/limit")
        .header("content-type", &content_type)
        .set_payload(multipart_file(
            boundary,
            "document",
            "small.txt",
            "text/plain",
            b"hello",
        ))
        .send()
        .await
        .unwrap()
        .assert_status(caelix::StatusCode::OK);
    let limit = app
        .post("/uploads/limit")
        .header("content-type", &content_type)
        .set_payload(multipart_file(
            boundary,
            "document",
            "large.txt",
            "text/plain",
            b"toolong",
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(limit.status(), caelix::StatusCode::PAYLOAD_TOO_LARGE);
    let route_limit = app
        .post("/uploads/route-limit")
        .header("content-type", &content_type)
        .set_payload(multipart_file(
            boundary,
            "document",
            "large.txt",
            "text/plain",
            b"hello",
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(route_limit.status(), caelix::StatusCode::PAYLOAD_TOO_LARGE);

    let png = b"\x89PNG\r\n\x1a\n";
    app.post("/uploads/mime")
        .header("content-type", &content_type)
        .set_payload(multipart_file(
            boundary,
            "document",
            "image.png",
            "text/plain",
            png,
        ))
        .send()
        .await
        .unwrap()
        .assert_status(caelix::StatusCode::OK);
    let mime = app
        .post("/uploads/mime")
        .header("content-type", &content_type)
        .set_payload(multipart_file(
            boundary,
            "document",
            "text.txt",
            "image/png",
            b"plain text",
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(mime.status(), caelix::StatusCode::BAD_REQUEST);

    app.post("/uploads/trusted-mime")
        .header("content-type", &content_type)
        .set_payload(multipart_file(
            boundary,
            "document",
            "text.txt",
            "text/plain; charset=utf-8",
            b"plain text",
        ))
        .send()
        .await
        .unwrap()
        .assert_status(caelix::StatusCode::OK);
    app.shutdown().await.unwrap();
}

#[caelix::test]
async fn controller_file_validators_run_before_handlers_and_cleanup_failed_collections() {
    let app = TestApplication::new::<UploadModule>().await.unwrap();
    let boundary = "caelix-validator-boundary";
    let content_type = format!("multipart/form-data; boundary={boundary}");
    let response = app
        .post("/uploads/validate-many")
        .header("content-type", &content_type)
        .set_payload(multipart_files(
            boundary,
            "documents",
            &[
                ("accepted.txt", "text/plain", b"accepted"),
                ("reject.txt", "text/plain", b"rejected"),
            ],
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), caelix::StatusCode::BAD_REQUEST);
    let service = app.resolve::<UploadValidationService>().unwrap();
    let seen_paths = service.seen_paths();
    assert_eq!(seen_paths.len(), 2);
    assert!(seen_paths.iter().all(|path| !path.exists()));
    app.shutdown().await.unwrap();
}

#[cfg(feature = "openapi")]
#[test]
fn openapi_describes_binary_multipart_file_fields() {
    let document = caelix::openapi::build_openapi::<UploadModule>(
        &caelix::openapi::OpenApiConfig::new("Uploads", "1.0"),
    )
    .unwrap();
    let document: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    let content = &document["paths"]["/uploads/documented"]["post"]["requestBody"]["content"];
    assert!(content.get("multipart/form-data").is_some());
    assert!(content.get("application/json").is_none());
    let file_schema = &content["multipart/form-data"]["schema"]["properties"]["document"];
    assert_eq!(file_schema["type"], "string");
    assert_eq!(file_schema["format"], "binary");
    assert_eq!(
        content["multipart/form-data"]["schema"]["required"],
        json!(["document"])
    );
    assert_eq!(file_schema["maxLength"], 2 * 1024 * 1024);
    assert_eq!(
        file_schema["description"],
        "Allowed MIME types: application/pdf, image/png."
    );
}
