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
    fn sync_validator(&self, _: &UploadedFile) -> Result<()> { Ok(()) }
    async fn mutable_self(&mut self, _: &UploadedFile) -> Result<()> { Ok(()) }
    async fn owned_file(&self, _: UploadedFile) -> Result<()> { Ok(()) }
    async fn wrong_result(&self, _: &UploadedFile) -> Result<String> { Ok(String::new()) }
    async fn extra_argument(&self, _: &UploadedFile, _: bool) -> Result<()> { Ok(()) }

    #[post("/sync")]
    async fn sync(&self, #[file(validate = sync_validator)] upload: UploadedFile) -> Result<String> { Ok(String::new()) }
    #[post("/mut")]
    async fn mutable(&self, #[file(validate = mutable_self)] upload: UploadedFile) -> Result<String> { Ok(String::new()) }
    #[post("/owned")]
    async fn owned(&self, #[file(validate = owned_file)] upload: UploadedFile) -> Result<String> { Ok(String::new()) }
    #[post("/result")]
    async fn result(&self, #[file(validate = wrong_result)] upload: UploadedFile) -> Result<String> { Ok(String::new()) }
    #[post("/extra")]
    async fn extra(&self, #[file(validate = extra_argument)] upload: UploadedFile) -> Result<String> { Ok(String::new()) }
}

fn main() {}
