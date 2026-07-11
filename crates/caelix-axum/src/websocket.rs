use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{CloseFrame, Message, WebSocket},
    },
    http::{HeaderMap, Uri},
    routing::get,
};
use bytes::Bytes;
use caelix_core::{
    BoxFuture, Container, HttpException, Module, Result, StatusCode, WebSocketCloseCode,
    WebSocketCloseFrame, WebSocketError, WebSocketGateway, WebSocketRequest, WebSocketSession,
    WebSocketTransport, visit_module_gateways,
};
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use tokio::sync::Mutex;

use crate::AxumRouterBuilder;

pub const DEFAULT_WEBSOCKET_MAX_MESSAGE_SIZE: usize = 1024 * 1024;

struct AxumWebSocketTransport {
    sender: Mutex<SplitSink<WebSocket, Message>>,
    open: Arc<AtomicBool>,
}

impl AxumWebSocketTransport {
    async fn send(&self, message: Message) -> Result<()> {
        self.sender
            .lock()
            .await
            .send(message)
            .await
            .map_err(transport_error)
    }
}

impl WebSocketTransport for AxumWebSocketTransport {
    fn send_text(&self, text: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.send(Message::Text(text.into())).await })
    }

    fn send_binary(&self, data: Bytes) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.send(Message::Binary(data)).await })
    }

    fn ping(&self, data: Bytes) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.send(Message::Ping(data)).await })
    }

    fn pong(&self, data: Bytes) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { self.send(Message::Pong(data)).await })
    }

    fn close(&self, frame: Option<WebSocketCloseFrame>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let frame = frame.map(to_axum_close);
            let result = self.send(Message::Close(frame)).await;
            self.open.store(false, Ordering::Release);
            result
        })
    }
}

fn transport_error(error: axum::Error) -> HttpException {
    HttpException::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Internal Server Error",
        error.to_string(),
    )
}

fn to_axum_close(frame: WebSocketCloseFrame) -> CloseFrame {
    let code = if frame.code.is_server_sendable() {
        frame.code.as_u16()
    } else {
        WebSocketCloseCode::Internal.as_u16()
    };
    CloseFrame {
        code,
        reason: frame.reason.into(),
    }
}

fn from_axum_close(frame: CloseFrame) -> Option<WebSocketCloseFrame> {
    Some(WebSocketCloseFrame::new(
        WebSocketCloseCode::from_u16(frame.code)?,
        frame.reason.to_string(),
    ))
}

pub(crate) fn configure_gateway_routes<M: Module + 'static>(
    routes: &mut AxumRouterBuilder,
    container: Arc<Container>,
    max_message_size: usize,
) {
    visit_module_gateways::<M>(&mut |definition| {
        if !definition.is_websocket() {
            return;
        }
        let path = definition.path;
        let gateway = definition
            .resolve(&container)
            .expect("gateway was validated during application bootstrap");
        routes.route(
            path,
            get(
                move |upgrade: WebSocketUpgrade,
                      uri: Uri,
                      headers: HeaderMap,
                      State(_container): State<Arc<Container>>| {
                    let gateway = gateway.clone();
                    async move {
                        upgrade
                            .max_message_size(max_message_size)
                            .on_upgrade(move |socket| {
                                run_connection(gateway, socket, websocket_request(uri, headers))
                            })
                    }
                },
            ),
        );
    });
}

fn websocket_request(uri: Uri, headers: HeaderMap) -> WebSocketRequest {
    WebSocketRequest::new(
        uri.path(),
        uri.query().unwrap_or_default(),
        None,
        headers
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_ascii_lowercase(), value.to_owned()))
            })
            .collect::<BTreeMap<_, _>>(),
    )
}

async fn run_connection(
    gateway: Arc<dyn WebSocketGateway>,
    socket: WebSocket,
    request: WebSocketRequest,
) {
    let open = Arc::new(AtomicBool::new(true));
    let (sender, mut receiver) = socket.split();
    let transport = Arc::new(AxumWebSocketTransport {
        sender: Mutex::new(sender),
        open: open.clone(),
    });
    let session = Arc::new(WebSocketSession::new(
        uuid::Uuid::new_v4().to_string(),
        open.clone(),
        transport,
    ));
    let mut close_frame = None;

    if let Err(error) = gateway.on_connect(session.clone(), request).await {
        close_frame = Some(fail_handler(&gateway, &session, error.message).await);
    } else {
        while session.is_open() {
            match receiver.next().await {
                Some(Ok(Message::Text(text))) => {
                    if let Err(error) = gateway.on_text(session.clone(), text.to_string()).await {
                        close_frame = Some(fail_handler(&gateway, &session, error.message).await);
                        break;
                    }
                }
                Some(Ok(Message::Binary(data))) => {
                    if let Err(error) = gateway.on_binary(session.clone(), data).await {
                        close_frame = Some(fail_handler(&gateway, &session, error.message).await);
                        break;
                    }
                }
                Some(Ok(Message::Ping(data))) => {
                    if session.pong(data).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Pong(_))) => {}
                Some(Ok(Message::Close(frame))) => {
                    close_frame = frame.and_then(from_axum_close);
                    let _ = session.close(close_frame.clone()).await;
                    break;
                }
                Some(Err(error)) => {
                    let message = error.to_string();
                    gateway
                        .on_error(session.clone(), WebSocketError::new(message.clone()))
                        .await;
                    close_frame = Some(WebSocketCloseFrame::new(
                        WebSocketCloseCode::Protocol,
                        "invalid websocket frame",
                    ));
                    let _ = session.close(close_frame.clone()).await;
                    break;
                }
                None => break,
            }
        }
    }
    open.store(false, Ordering::Release);
    gateway.on_close(session, close_frame).await;
}

async fn fail_handler(
    gateway: &Arc<dyn WebSocketGateway>,
    session: &Arc<WebSocketSession>,
    message: String,
) -> WebSocketCloseFrame {
    gateway
        .on_error(session.clone(), WebSocketError::new(message))
        .await;
    let frame = WebSocketCloseFrame::new(WebSocketCloseCode::Internal, "handler failed");
    let _ = session.close(Some(frame.clone())).await;
    frame
}
