use caelix_core::{Container, Result};
use caelix_macros::{controller, injectable};
use serde::Deserialize;

#[derive(Deserialize)]
struct SearchQuery {
    term: String,
}

#[derive(Deserialize)]
struct CreateUser {
    name: String,
}

#[injectable]
struct UserController;

#[controller("/users")]
impl UserController {
    #[get("/{id}")]
    async fn get_user(&self, #[param] id: i64) -> Result<String> {
        Ok(id.to_string())
    }

    #[get("/")]
    async fn search_users(&self, #[query] query: SearchQuery) -> Result<String> {
        Ok(query.term)
    }

    #[post("/")]
    async fn create_user(&self, #[body] body: CreateUser) -> Result<String> {
        Ok(body.name)
    }
}

async fn exercise() {
    let mut container = Container::new();
    container.register::<UserController>().await;

    let _controller = container.resolve::<UserController>();
}

fn main() {
    std::mem::drop(exercise());
}
