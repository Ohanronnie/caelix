use caelix_core::{Result, UploadedFile};
use caelix_macros::{controller, injectable};

mod caelix {
    pub use caelix_actix::{__actix_web, RequestPayload, to_actix_response};
    pub use caelix_core::*;
}

#[injectable]
struct UploadController;

#[controller("/uploads")]
impl UploadController {
    #[post("")]
    async fn create(
        &self,
        #[file(validate = validate_upload)] upload: UploadedFile,
    ) -> Result<String> {
        Ok(String::new())
    }
}

fn main() {}
