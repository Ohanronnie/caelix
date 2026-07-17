#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Actix Web runtime adapter for Caelix applications.

mod actix_ws;
mod application;
mod test_app;
mod websocket;

/// Re-export of `actix_web` for macro-generated code (`#[caelix::main]`, `#[caelix::test]`,
/// `#[controller]`). Consumers only need a `caelix` dependency; they should not depend on
/// `actix-web` solely to satisfy expanded paths.
#[doc(hidden)]
pub use actix_web as __actix_web;

/// Re-exported public API.
pub use application::{Application, Logging, to_actix_response};
/// Re-exported public API.
pub use test_app::{TestApplication, TestApplicationBuilder, TestRequestBuilder, TestResponse};
/// Re-exported public API.
pub use websocket::DEFAULT_WEBSOCKET_MAX_MESSAGE_SIZE;
