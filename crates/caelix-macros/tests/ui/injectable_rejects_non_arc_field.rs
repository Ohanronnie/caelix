use caelix_macros::injectable;

struct Repo;

#[injectable]
struct Service {
    repo: Repo,
}

fn main() {}
