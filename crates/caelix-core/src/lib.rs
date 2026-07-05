//! Core primitives for Caelix.

pub use http::StatusCode;
pub mod exception;
pub mod response;
mod result;
pub use result::Result;
