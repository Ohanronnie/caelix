//! Run with `cargo run -p caelix --example upload_validation_server`.
//!
//! Then upload a PNG with:
//! `curl --fail-with-body -F 'image=@image.png;type=image/png' http://127.0.0.1:3000/uploads/image`

use std::sync::Arc;

use caelix::{
    Application, BadRequestException, Module, ModuleMetadata, Response, Result, UploadedFile,
    controller, injectable,
};

#[injectable]
struct ImageValidationService;

impl ImageValidationService {
    async fn verify(&self, file: &UploadedFile) -> Result<()> {
        if file.file_name().is_none() {
            return Err(BadRequestException::new("an uploaded filename is required"));
        }
        Ok(())
    }
}

#[injectable]
struct UploadController {
    image_validation: Arc<ImageValidationService>,
}

#[controller("/uploads")]
impl UploadController {
    async fn validate_image(&self, file: &UploadedFile) -> Result<()> {
        self.image_validation.verify(file).await
    }

    #[post("/image")]
    async fn upload_image(
        &self,
        #[file(
            name = "image",
            max_size = "5MiB",
            content_type = "image/png, image/jpeg",
            validate = validate_image,
        )]
        image: UploadedFile,
    ) -> Result<Response<String>> {
        Ok(Response::Body(format!(
            "accepted {} bytes from {}",
            image.size(),
            image.file_name().unwrap_or("unnamed upload")
        )))
    }
}

struct UploadModule;

impl Module for UploadModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<ImageValidationService>()
            .controller::<UploadController>()
    }
}

#[caelix::main]
async fn main() -> std::io::Result<()> {
    Application::new::<UploadModule>()
        .await
        .map_err(|error| std::io::Error::other(error.message))?
        .body_limit(6 * 1024 * 1024)
        .listen("127.0.0.1:3000")
        .await
}
