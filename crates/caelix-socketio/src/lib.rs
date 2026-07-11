//! Socket.IO support for Caelix's Axum runtime.
//!
//! This crate intentionally has no Actix dependency. It is exposed through
//! `caelix::socket_io` only when the `socketio` feature selects Axum.

use caelix_core::{Container, HttpException, Result};
use serde::Serialize;

pub use socketioxide::layer::SocketIoLayer;
pub use socketioxide::{
    SocketIo,
    extract::{AckSender, Data, SocketRef},
};

/// The Socket.IO server handle registered as a first-class Caelix provider by
/// [`caelix_axum::Application::with_socket_io`].
#[derive(Clone)]
pub struct SocketIoHandle {
    io: SocketIo,
}

impl SocketIoHandle {
    /// Builds the Tower layer and its matching injectable handle exactly once.
    pub fn build() -> (SocketIoLayer, Self) {
        let (layer, io) = SocketIo::builder().build_layer();
        (layer, Self { io })
    }

    pub fn io(&self) -> &SocketIo {
        &self.io
    }
}

/// Implemented by `#[gateway("/namespace")]` for Socket.IO gateways.
#[doc(hidden)]
pub trait SocketIoGateway: Send + Sync + 'static {
    fn register_socket_io(io: &SocketIo, container: &Container) -> Result<()>;
}

/// Serializable failure payload used for both acknowledgement replies and the
/// mandatory `"error"` event emitted for failed handlers.
#[derive(Serialize)]
pub struct SocketIoError {
    pub error: String,
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
