use crate::{BoxFuture, HttpException, Injectable, Result};
use bytes::Bytes;
use std::{
    collections::BTreeMap,
    fmt,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebSocketCloseCode {
    Normal,
    GoingAway,
    Protocol,
    Unsupported,
    InvalidData,
    Policy,
    MessageTooBig,
    Internal,
    Restart,
    TryAgainLater,
    MandatoryExtension,
    BadGateway,
    Other(u16),
}

impl WebSocketCloseCode {
    pub fn as_u16(self) -> u16 {
        match self {
            Self::Normal => 1000,
            Self::GoingAway => 1001,
            Self::Protocol => 1002,
            Self::Unsupported => 1003,
            Self::InvalidData => 1007,
            Self::Policy => 1008,
            Self::MessageTooBig => 1009,
            Self::Internal => 1011,
            Self::Restart => 1012,
            Self::TryAgainLater => 1013,
            Self::MandatoryExtension => 1010,
            Self::BadGateway => 1014,
            Self::Other(code) => code,
        }
    }
    pub fn from_u16(code: u16) -> Option<Self> {
        Some(match code {
            1000 => Self::Normal,
            1001 => Self::GoingAway,
            1002 => Self::Protocol,
            1003 => Self::Unsupported,
            1007 => Self::InvalidData,
            1008 => Self::Policy,
            1009 => Self::MessageTooBig,
            1010 => Self::MandatoryExtension,
            1011 => Self::Internal,
            1012 => Self::Restart,
            1013 => Self::TryAgainLater,
            1014 => Self::BadGateway,
            3000..=4999 => Self::Other(code),
            _ => return None,
        })
    }
    pub fn is_valid(self) -> bool {
        Self::from_u16(self.as_u16()).is_some()
    }
    pub fn is_server_sendable(self) -> bool {
        self.is_valid() && self != Self::MandatoryExtension
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebSocketCloseFrame {
    pub code: WebSocketCloseCode,
    pub reason: String,
}
impl WebSocketCloseFrame {
    pub fn new(code: WebSocketCloseCode, reason: impl Into<String>) -> Self {
        Self {
            code,
            reason: reason.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct WebSocketRequest {
    path: String,
    query: String,
    peer_addr: Option<SocketAddr>,
    headers: BTreeMap<String, String>,
}
impl WebSocketRequest {
    pub fn new(
        path: impl Into<String>,
        query: impl Into<String>,
        peer_addr: Option<SocketAddr>,
        headers: BTreeMap<String, String>,
    ) -> Self {
        Self {
            path: path.into(),
            query: query.into(),
            peer_addr,
            headers,
        }
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn query_string(&self) -> &str {
        &self.query
    }
    pub fn peer_addr(&self) -> Option<SocketAddr> {
        self.peer_addr
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
    pub fn headers(&self) -> &BTreeMap<String, String> {
        &self.headers
    }
}

#[derive(Clone, Debug)]
pub struct WebSocketError {
    message: String,
}
impl WebSocketError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}
impl fmt::Display for WebSocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for WebSocketError {}

#[doc(hidden)]
pub trait WebSocketTransport: Send + Sync {
    fn send_text(&self, text: String) -> BoxFuture<'_, Result<()>>;
    fn send_binary(&self, data: Bytes) -> BoxFuture<'_, Result<()>>;
    fn ping(&self, data: Bytes) -> BoxFuture<'_, Result<()>>;
    fn pong(&self, data: Bytes) -> BoxFuture<'_, Result<()>>;
    fn close(&self, frame: Option<WebSocketCloseFrame>) -> BoxFuture<'_, Result<()>>;
}

#[derive(Clone)]
pub struct WebSocketSession {
    id: String,
    open: Arc<AtomicBool>,
    transport: Arc<dyn WebSocketTransport>,
    close_frame: Arc<std::sync::Mutex<Option<WebSocketCloseFrame>>>,
}
impl WebSocketSession {
    #[doc(hidden)]
    pub fn new(
        id: impl Into<String>,
        open: Arc<AtomicBool>,
        transport: Arc<dyn WebSocketTransport>,
    ) -> Self {
        Self {
            id: id.into(),
            open,
            transport,
            close_frame: Arc::new(std::sync::Mutex::new(None)),
        }
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }
    pub async fn send_text(&self, text: impl Into<String>) -> Result<()> {
        self.ensure_open()?;
        self.transport.send_text(text.into()).await
    }
    pub async fn send_binary(&self, data: impl Into<Bytes>) -> Result<()> {
        self.ensure_open()?;
        self.transport.send_binary(data.into()).await
    }
    pub async fn ping(&self, data: impl Into<Bytes>) -> Result<()> {
        self.ensure_open()?;
        self.transport.ping(data.into()).await
    }
    pub async fn pong(&self, data: impl Into<Bytes>) -> Result<()> {
        self.ensure_open()?;
        self.transport.pong(data.into()).await
    }
    pub async fn close(&self, frame: Option<WebSocketCloseFrame>) -> Result<()> {
        *self
            .close_frame
            .lock()
            .expect("websocket close state lock poisoned") = frame.clone();
        self.transport.close(frame).await
    }
    #[doc(hidden)]
    pub fn take_local_close_frame(&self) -> Option<WebSocketCloseFrame> {
        self.close_frame
            .lock()
            .expect("websocket close state lock poisoned")
            .take()
    }
    fn ensure_open(&self) -> Result<()> {
        if self.is_open() {
            Ok(())
        } else {
            Err(HttpException::new(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error",
                "websocket session is closed",
            ))
        }
    }
}

pub trait WebSocketGateway: Injectable {
    fn path() -> &'static str
    where
        Self: Sized;
    fn on_connect(
        &self,
        _session: Arc<WebSocketSession>,
        _request: WebSocketRequest,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn on_text(&self, _session: Arc<WebSocketSession>, _text: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn on_binary(
        &self,
        _session: Arc<WebSocketSession>,
        _data: Bytes,
    ) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
    fn on_error(
        &self,
        _session: Arc<WebSocketSession>,
        _error: WebSocketError,
    ) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
    fn on_close(
        &self,
        _session: Arc<WebSocketSession>,
        _frame: Option<WebSocketCloseFrame>,
    ) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_codes_round_trip_without_corruption() {
        for code in [1000, 1012, 1013, 3000, 3999, 4000, 4999] {
            assert_eq!(WebSocketCloseCode::from_u16(code).unwrap().as_u16(), code);
        }
    }

    #[test]
    fn prohibited_wire_close_codes_are_rejected() {
        for code in [0, 999, 1004, 1005, 1006, 1015, 2000, 2999, 5000] {
            assert_eq!(WebSocketCloseCode::from_u16(code), None);
        }
        assert!(!WebSocketCloseCode::Other(1005).is_valid());
        assert!(!WebSocketCloseCode::Other(5000).is_valid());
        assert!(WebSocketCloseCode::MandatoryExtension.is_valid());
        assert!(!WebSocketCloseCode::MandatoryExtension.is_server_sendable());
    }
}
