use caelix_core as caelix;
use caelix_macros::Config;
use serde::Deserialize;

#[derive(Config, Deserialize)]
struct ExampleConfig {
    #[config(env = "PORT")]
    port: u16,
}

fn main() {}
