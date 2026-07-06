use std::sync::Arc;

use caelix_core as caelix;
use caelix_core::Container;
use caelix_macros::injectable;

#[injectable]
struct Repo;

#[injectable]
struct Service {
    repo: Arc<Repo>,
}

async fn exercise() {
    let mut container = Container::new();
    container.register::<Repo>().await;
    container.register::<Service>().await;

    let service = container.resolve::<Service>();
    let _repo = service.repo.clone();
}

fn main() {
    std::mem::drop(exercise());
}
