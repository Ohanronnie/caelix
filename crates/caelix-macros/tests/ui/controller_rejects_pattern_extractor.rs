use caelix_macros::controller;

struct UserController;

#[controller("/users")]
impl UserController {
    #[get("/{id}")]
    async fn get_user(&self, #[param] (id): i64) -> caelix_core::Result<String> {
        Ok(id.to_string())
    }
}

fn main() {}
