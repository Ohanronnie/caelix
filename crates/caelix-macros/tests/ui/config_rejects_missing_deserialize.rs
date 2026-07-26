use caelix_core as caelix;
use caelix_macros::Config;
use validator::Validate;

#[derive(Config, Validate)]
struct ExampleConfig {
    #[config(env = "PORT")]
    port: u16,
}

fn main() {}
