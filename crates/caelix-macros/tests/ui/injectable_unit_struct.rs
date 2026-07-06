use caelix_core as caelix;
use caelix_core::Container;
use caelix_macros::injectable;

#[injectable]
struct Logger;

async fn exercise() {
    let mut container = Container::new();
    container.register::<Logger>().await;

    let _logger = container.resolve::<Logger>();
}

fn main() {
    std::mem::drop(exercise());
}
