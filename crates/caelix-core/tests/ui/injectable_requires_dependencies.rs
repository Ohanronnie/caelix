use caelix_core::*;

struct ManualService;

impl Injectable for ManualService {
    fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async { Ok(Self) })
    }
}

fn main() {}
