use caelix_core as caelix;
use caelix_macros::Config;
use serde::Deserialize;
use validator::Validate;

#[derive(Config, Deserialize, Validate)]
struct ExampleConfig {
    #[config(env = "PORT")]
    port: u16,
    #[config(env = "PORT")]
    backup_port: u16,
}

fn main() {}
