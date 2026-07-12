//! Public Caelix framework crate.

pub use caelix_core::*;
// Explicit re-exports so `test` / `main` are not pulled into `prelude` (which
// would shadow Rust's `#[test]`).
pub use caelix_macros::{controller, gateway, guard, injectable, on_message};

/// OpenAPI generation types and controller documentation marker attributes.
#[cfg(feature = "openapi")]
pub mod openapi {
    pub use caelix_core::openapi::*;
    pub use caelix_macros::{errors, request_header, response, security};
}

/// RFC 6455 WebSocket gateway APIs.
///
/// The same types remain re-exported at the crate root for backwards
/// compatibility; prefer this namespace for new gateway code.
pub mod websocket {
    pub use caelix_core::{
        WebSocketCloseCode, WebSocketCloseFrame, WebSocketError, WebSocketGateway,
        WebSocketRequest, WebSocketSession,
    };
}

#[cfg(all(feature = "actix", feature = "axum"))]
compile_error!("Caelix backends `actix` and `axum` are mutually exclusive; select exactly one");

#[cfg(any(feature = "actix", feature = "axum"))]
pub use caelix_macros::{main, test};

/// Hidden Actix re-export for macro-generated code. Prefer `caelix` public APIs
/// (`Application`, `TestApplication`, `#[caelix::main]`, `#[caelix::test]`).
#[cfg(feature = "actix")]
#[doc(hidden)]
pub use caelix_actix::__actix_web;

#[cfg(feature = "actix")]
pub use caelix_actix::{
    Application, Logging, TestApplication, TestApplicationBuilder, TestRequestBuilder,
    TestResponse, to_actix_response,
};

/// Hidden Axum and Tokio re-exports for generated controller and runtime code.
#[cfg(feature = "axum")]
#[doc(hidden)]
pub use caelix_axum::{__axum, __tokio};

#[cfg(feature = "axum")]
pub use caelix_axum::{
    Application, AxumRequestInfo, AxumRouterBuilder, DEFAULT_BODY_LIMIT_BYTES, TestApplication,
    TestApplicationBuilder, TestRequestBuilder, TestResponse, to_axum_response,
};

/// Socket.IO APIs, available only with the Axum-selecting `socketio` feature.
#[cfg(feature = "socketio")]
pub mod socket_io {
    pub use caelix_socketio::*;
}

pub mod prelude {
    pub use caelix_core::*;
    pub use caelix_macros::{controller, gateway, guard, injectable, on_message};
}
