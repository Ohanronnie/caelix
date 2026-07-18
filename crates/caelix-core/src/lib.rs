#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Core primitives for Caelix.

extern crate self as caelix_core;

mod cache;
mod container;
mod context;
mod controller;
mod events;
mod exception;
mod guard;
mod interceptor;
mod logging;
mod module;
#[cfg(feature = "openapi")]
/// Public Caelix module `openapi`.
pub mod openapi;
mod response;
mod result;
#[cfg(feature = "uploads")]
mod upload;
mod websocket;

/// Re-exported public API.
pub use bytes::Bytes;
/// Re-exported public API.
pub use cache::*;
/// Re-exported public API.
pub use container::*;
/// Re-exported public API.
pub use context::*;
/// Re-exported public API.
pub use controller::*;
/// Re-exported public API.
pub use events::*;
/// Re-exported public API.
pub use exception::*;
/// Re-exported public API.
pub use futures_util::StreamExt;
/// Re-exported public API.
pub use guard::*;
/// Re-exported public API.
pub use http::StatusCode;
/// Re-exported public API.
pub use interceptor::*;
/// Re-exported public API.
pub use logging::*;
/// Re-exported public API.
pub use module::*;
/// Re-exported public API.
pub use response::*;
/// Re-exported public API.
pub use result::Result;
/// Re-exported public API.
#[cfg(feature = "uploads")]
/// Re-exported public API.
pub use upload::*;
/// Re-exported public API.
pub use websocket::*;

#[cfg(feature = "validator")]
/// Re-exported public API.
pub use validator;
