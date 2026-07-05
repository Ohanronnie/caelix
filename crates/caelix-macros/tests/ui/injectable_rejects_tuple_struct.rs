use caelix_macros::injectable;

struct Repo;

#[injectable]
struct Service(std::sync::Arc<Repo>);

fn main() {}
