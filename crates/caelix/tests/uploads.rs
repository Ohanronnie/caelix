#![cfg(any(feature = "actix", feature = "axum"))]

#[cfg(feature = "openapi")]
use caelix::openapi::utoipa;
use caelix::{
    ModuleMetadata, MultipartForm, Response, Result, TestApplication, UploadedFile, controller,
    injectable,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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
struct UploadController;

#[controller("/uploads")]
impl UploadController {
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
}

struct UploadModule;

impl caelix::Module for UploadModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().controller::<UploadController>()
    }
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

#[cfg(feature = "openapi")]
#[test]
fn openapi_describes_binary_multipart_file_fields() {
    let document = caelix::openapi::build_openapi::<UploadModule>(
        &caelix::openapi::OpenApiConfig::new("Uploads", "1.0"),
    )
    .unwrap();
    let document: Value = serde_json::from_str(&document.to_json().unwrap()).unwrap();
    let content = &document["paths"]["/uploads/required"]["post"]["requestBody"]["content"];
    assert!(content.get("multipart/form-data").is_some());
    assert!(content.get("application/json").is_none());
    let file_schema = &content["multipart/form-data"]["schema"]["allOf"][1]["properties"]["avatar"];
    assert_eq!(file_schema["type"], "string");
    assert_eq!(file_schema["format"], "binary");
    assert_eq!(
        content["multipart/form-data"]["schema"]["allOf"][1]["required"],
        json!(["avatar"])
    );
}
