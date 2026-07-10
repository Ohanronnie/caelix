mod application;
mod test_app;

/// Re-export of `actix_web` for macro-generated code (`#[caelix::main]`, `#[caelix::test]`,
/// `#[controller]`). Consumers only need a `caelix` dependency; they should not depend on
/// `actix-web` solely to satisfy expanded paths.
#[doc(hidden)]
pub use actix_web as __actix_web;

pub use application::{Application, Logging, to_actix_response};
pub use test_app::{TestApplication, TestApplicationBuilder, TestRequestBuilder, TestResponse};
