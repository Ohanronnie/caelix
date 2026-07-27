use caelix_core::Result;
use caelix_macros::controller;

mod caelix {
    pub use caelix_actix::{__actix_web, to_actix_response};
    pub use caelix_core::*;
}

mod admin {
    pub struct UserController;
}

mod public {
    pub struct UserController;
}

#[controller("/admin/users")]
impl admin::UserController {
    #[get("/")]
    async fn list(&self) -> Result<String> {
        Ok("admin".into())
    }
}

#[controller("/public/users")]
impl public::UserController {
    #[get("/")]
    async fn list(&self) -> Result<String> {
        Ok("public".into())
    }
}

fn main() {}
