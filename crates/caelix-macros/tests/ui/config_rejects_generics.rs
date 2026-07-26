use caelix_core as caelix;
use caelix_macros::Config;
use serde::Deserialize;
use validator::Validate;

#[derive(Config, Deserialize, Validate)]
struct ExampleConfig<T> {
    #[config(env = "VALUE")]
    value: T,
}

fn main() {}
