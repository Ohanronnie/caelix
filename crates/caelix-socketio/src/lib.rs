#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Socket.IO support for Caelix's Axum runtime.
//!
//! This crate intentionally has no Actix dependency. It is exposed through
//! `caelix::socket_io` only when the `socketio` feature selects Axum.

use caelix_core::{Container, HttpException, Result};
use serde::Serialize;

/// Re-exported public API.
pub use socketioxide::layer::SocketIoLayer;
/// Re-exported public API.
pub use socketioxide::{
    SocketIo,
    extract::{AckSender, Data, SocketRef},
};

/// The Socket.IO server handle registered as a first-class Caelix provider by
/// `caelix_axum::Application::with_socket_io`.
#[derive(Clone)]
/// Public Caelix type `SocketIoHandle`.
pub struct SocketIoHandle {
    io: SocketIo,
}

impl SocketIoHandle {
    /// Builds the Tower layer and its matching injectable handle exactly once.
    pub fn build() -> (SocketIoLayer, Self) {
        let (layer, io) = SocketIo::builder().build_layer();
        (layer, Self { io })
    }

    /// Runs the `io` public API operation.
    pub fn io(&self) -> &SocketIo {
        &self.io
    }
}

/// Implemented by `#[gateway("/namespace")]` for Socket.IO gateways.
#[doc(hidden)]
pub trait SocketIoGateway: Send + Sync + 'static {
    /// Public Caelix API.
    fn register_socket_io(io: &SocketIo, container: &Container) -> Result<()>;
}

/// Serializable failure payload used for both acknowledgement replies and the
/// mandatory `"error"` event emitted for failed handlers.
#[derive(Serialize)]
/// Public Caelix type `SocketIoError`.
pub struct SocketIoError {
    /// The `error` value.
    pub error: String,
    /// The `message` value.
    pub message: String,
}

impl From<HttpException> for SocketIoError {
    fn from(exception: HttpException) -> Self {
        Self {
            error: exception.error.to_owned(),
            message: exception.message,
        }
    }
}
