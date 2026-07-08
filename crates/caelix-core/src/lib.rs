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
mod response;
mod result;

pub use bytes::Bytes;
pub use cache::*;
pub use container::*;
pub use context::*;
pub use controller::*;
pub use events::*;
pub use exception::*;
pub use futures_util::StreamExt;
pub use guard::*;
pub use http::StatusCode;
pub use interceptor::*;
pub use logging::*;
pub use module::*;
pub use response::*;
pub use result::Result;

#[cfg(feature = "validator")]
pub use validator;
