#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Core primitives for Caelix.

extern crate self as caelix_core;

mod cache;
#[cfg(feature = "config")]
mod config;
mod container;
mod context;
mod controller;
mod cookie;
mod events;
mod exception;
mod guard;
mod interceptor;
mod logging;
mod microservice;
mod module;
#[cfg(feature = "openapi")]
/// Public Caelix module `openapi`.
pub mod openapi;
mod response;
mod result;
mod throttle;
#[cfg(feature = "uploads")]
mod upload;
mod websocket;

/// Re-exported public API.
pub use bytes::Bytes;
/// Re-exported public API.
pub use cache::*;
#[cfg(feature = "config")]
/// Typed configuration loading and module registration.
pub use config::*;
/// Re-exported public API.
pub use container::*;
/// Re-exported public API.
pub use context::*;
/// Re-exported public API.
pub use controller::*;
/// Framework-owned response cookie primitives.
pub use cookie::{Cookie, SameSite};
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
pub use microservice::*;
/// Re-exported public API.
pub use module::*;
/// Re-exported public API.
pub use response::*;
/// Re-exported public API.
pub use result::Result;
/// Request throttling primitives.
pub use throttle::*;
/// Re-exported public API.
#[cfg(feature = "uploads")]
/// Re-exported public API.
pub use upload::*;
/// Re-exported public API.
pub use websocket::*;

#[cfg(feature = "config")]
#[doc(hidden)]
pub use serde::de::DeserializeOwned as __DeserializeOwned;
#[cfg(feature = "config")]
#[doc(hidden)]
pub use serde::de::value::{
    Error as __ConfigValueError, StrDeserializer as __ConfigStrDeserializer,
};
#[cfg(feature = "validator")]
/// Re-exported public API.
pub use validator;
#[cfg(feature = "config")]
#[doc(hidden)]
pub use validator::Validate as __Validate;
