use actix_http::ws::{CloseCode, CloseReason, Item, Message, ProtocolError};
use actix_web::{HttpRequest, HttpResponse, web};
use base64::Engine;
use bytes::Bytes;
use caelix_core::{
    BoxFuture, Container, HttpException, Module, Result, StatusCode, WebSocketCloseCode,
    WebSocketCloseFrame, WebSocketError, WebSocketGateway, WebSocketRequest, WebSocketSession,
    WebSocketTransport, visit_module_gateways,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Mutex;

use crate::actix_ws;

pub const DEFAULT_WEBSOCKET_MAX_MESSAGE_SIZE: usize = 1024 * 1024;

enum Fragment {
    Text(Vec<u8>),
    Binary(Vec<u8>),
}

impl FragmentLength for Fragment {
    fn len(&self) -> usize {
        match self {
            Self::Text(data) | Self::Binary(data) => data.len(),
        }
    }
    fn extend(&mut self, bytes: &[u8]) {
        match self {
            Self::Text(data) | Self::Binary(data) => data.extend_from_slice(bytes),
        }
    }
}

struct ActixWebSocketTransport {
    session: Mutex<actix_ws::Session>,
    open: Arc<AtomicBool>,
}

fn send_error(error: impl std::fmt::Display) -> HttpException {
    HttpException::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Internal Server Error",
        error.to_string(),
    )
}

impl WebSocketTransport for ActixWebSocketTransport {
    fn send_text(&self, text: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.session
                .lock()
                .await
                .text(text)
                .await
                .map_err(send_error)
        })
    }
    fn send_binary(&self, data: Bytes) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.session
                .lock()
                .await
                .binary(data)
                .await
                .map_err(send_error)
        })
    }
    fn ping(&self, data: Bytes) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.session
                .lock()
                .await
                .ping(&data)
                .await
                .map_err(send_error)
        })
    }
    fn pong(&self, data: Bytes) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.session
                .lock()
                .await
                .pong(&data)
                .await
                .map_err(send_error)
        })
    }
    fn close(&self, frame: Option<WebSocketCloseFrame>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            if frame
                .as_ref()
                .is_some_and(|frame| !frame.code.is_server_sendable())
            {
                return Err(HttpException::new(
                    StatusCode::BAD_REQUEST,
                    "Bad Request",
                    "invalid websocket close code",
                ));
            }
            self.open.store(false, Ordering::Release);
            self.session
                .lock()
                .await
                .clone()
                .close(frame.map(to_actix_close))
                .await
                .map_err(send_error)
        })
    }
}

fn to_actix_close(frame: WebSocketCloseFrame) -> CloseReason {
    CloseReason {
        code: CloseCode::from(frame.code.as_u16()),
        description: Some(frame.reason),
    }
}

fn from_actix_close(frame: CloseReason) -> Option<WebSocketCloseFrame> {
    Some(WebSocketCloseFrame::new(
        WebSocketCloseCode::from_u16(u16::from(frame.code))?,
        frame.description.unwrap_or_default(),
    ))
}

pub(crate) fn configure_gateway_routes<M: Module + 'static>(
    cfg: &mut web::ServiceConfig,
    container: Arc<Container>,
    max_size: usize,
) {
    visit_module_gateways::<M>(&mut |definition| {
        if !definition.is_websocket() {
            return;
        }
        let path = definition.path;
        let gateway = definition
            .resolve(&container)
            .expect("gateway was validated during application bootstrap");
        cfg.route(
            path,
            web::get()
                .to(move |request, payload| upgrade(request, payload, gateway.clone(), max_size)),
        );
    });
}

async fn upgrade(
    req: HttpRequest,
    body: web::Payload,
    gateway: Arc<dyn WebSocketGateway>,
    max_size: usize,
) -> std::result::Result<HttpResponse, actix_web::Error> {
    validate_rfc6455_handshake(&req)?;
    let request = WebSocketRequest::new(
        req.path(),
        req.query_string(),
        req.peer_addr(),
        req.headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_ascii_lowercase(), value.to_string()))
            })
            .collect::<BTreeMap<_, _>>(),
    );
    let (response, actix_session, stream) = actix_ws::handle(&req, body)?;
    let open = Arc::new(AtomicBool::new(true));
    let transport = Arc::new(ActixWebSocketTransport {
        session: Mutex::new(actix_session),
        open: open.clone(),
    });
    let session = Arc::new(WebSocketSession::new(
        uuid::Uuid::new_v4().to_string(),
        open.clone(),
        transport,
    ));
    actix_web::rt::spawn(run_connection(
        gateway, session, stream, request, open, max_size,
    ));
    Ok(response)
}

fn validate_rfc6455_handshake(req: &HttpRequest) -> std::result::Result<(), actix_web::Error> {
    let version = req
        .headers()
        .get(actix_web::http::header::SEC_WEBSOCKET_VERSION)
        .and_then(|value| value.to_str().ok());
    if version.is_some() && version != Some("13") {
        let response = HttpResponse::BadRequest()
            .insert_header((actix_web::http::header::SEC_WEBSOCKET_VERSION, "13"))
            .finish();
        return Err(actix_web::error::InternalError::from_response(
            "unsupported WebSocket version",
            response,
        )
        .into());
    }
    if let Some(key) = req
        .headers()
        .get(actix_web::http::header::SEC_WEBSOCKET_KEY)
    {
        let decoded = key
            .to_str()
            .ok()
            .and_then(|key| base64::engine::general_purpose::STANDARD.decode(key).ok());
        if decoded.as_deref().is_none_or(|nonce| nonce.len() != 16) {
            return Err(actix_web::error::ErrorBadRequest(
                "invalid Sec-WebSocket-Key",
            ));
        }
    }
    Ok(())
}

async fn run_connection(
    gateway: Arc<dyn WebSocketGateway>,
    session: Arc<WebSocketSession>,
    stream: actix_ws::MessageStream,
    request: WebSocketRequest,
    open: Arc<AtomicBool>,
    max_size: usize,
) {
    let mut close_frame = None;
    if let Err(error) = gateway.on_connect(session.clone(), request).await {
        close_frame = Some(fail_handler(&gateway, &session, error.message).await);
    } else {
        let mut fragment: Option<Fragment> = None;
        let mut messages = stream.max_frame_size(max_size);
        while session.is_open() {
            match messages.recv().await {
                Some(Ok(Message::Text(text))) => {
                    if fragment.is_some() {
                        close_frame = Some(
                            close_with_protocol_error(
                                &gateway,
                                &session,
                                WebSocketCloseCode::Protocol,
                                "data frame interleaved with fragmented message",
                            )
                            .await,
                        );
                        break;
                    }
                    if let Err(error) = gateway.on_text(session.clone(), text.to_string()).await {
                        close_frame = Some(fail_handler(&gateway, &session, error.message).await);
                        break;
                    }
                }
                Some(Ok(Message::Binary(data))) => {
                    if fragment.is_some() {
                        close_frame = Some(
                            close_with_protocol_error(
                                &gateway,
                                &session,
                                WebSocketCloseCode::Protocol,
                                "data frame interleaved with fragmented message",
                            )
                            .await,
                        );
                        break;
                    }
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
                    match frame {
                        Some(frame) => match from_actix_close(frame) {
                            Some(frame) => close_frame = Some(frame),
                            None => {
                                close_frame = Some(
                                    close_with_protocol_error(
                                        &gateway,
                                        &session,
                                        WebSocketCloseCode::Protocol,
                                        "invalid websocket close code",
                                    )
                                    .await,
                                );
                                break;
                            }
                        },
                        None => close_frame = None,
                    }
                    let _ = session.close(close_frame.clone()).await;
                    break;
                }
                Some(Ok(Message::Continuation(item))) => {
                    let result = match item {
                        Item::FirstText(data) => {
                            start_fragment(&mut fragment, Fragment::Text(data.to_vec()), max_size)
                        }
                        Item::FirstBinary(data) => {
                            start_fragment(&mut fragment, Fragment::Binary(data.to_vec()), max_size)
                        }
                        Item::Continue(data) => {
                            append_fragment(&mut fragment, &data, max_size).map(|_| None)
                        }
                        Item::Last(data) => {
                            append_fragment(&mut fragment, &data, max_size).map(|_| fragment.take())
                        }
                    };
                    match result {
                        Ok(Some(Fragment::Text(data))) => match String::from_utf8(data) {
                            Ok(text) => {
                                if let Err(error) = gateway.on_text(session.clone(), text).await {
                                    close_frame =
                                        Some(fail_handler(&gateway, &session, error.message).await);
                                    break;
                                }
                            }
                            Err(_) => {
                                close_frame = Some(
                                    close_with_protocol_error(
                                        &gateway,
                                        &session,
                                        WebSocketCloseCode::InvalidData,
                                        "invalid UTF-8 text message",
                                    )
                                    .await,
                                );
                                break;
                            }
                        },
                        Ok(Some(Fragment::Binary(data))) => {
                            if let Err(error) =
                                gateway.on_binary(session.clone(), Bytes::from(data)).await
                            {
                                close_frame =
                                    Some(fail_handler(&gateway, &session, error.message).await);
                                break;
                            }
                        }
                        Ok(None) => {}
                        Err(code) => {
                            close_frame = Some(
                                close_with_protocol_error(
                                    &gateway,
                                    &session,
                                    code,
                                    "invalid fragmented message",
                                )
                                .await,
                            );
                            break;
                        }
                    }
                }
                Some(Ok(Message::Nop)) => {}
                Some(Err(error)) => {
                    let message = error.to_string();
                    gateway
                        .on_error(session.clone(), WebSocketError::new(message.clone()))
                        .await;
                    let code = protocol_close_code(&error);
                    close_frame = Some(WebSocketCloseFrame::new(code, "invalid websocket frame"));
                    let _ = session.close(close_frame.clone()).await;
                    break;
                }
                None => {
                    if session.is_open() {
                        gateway
                            .on_error(
                                session.clone(),
                                WebSocketError::new("websocket transport closed unexpectedly"),
                            )
                            .await;
                    }
                    break;
                }
            }
        }
    }
    close_frame = close_frame.or_else(|| session.take_local_close_frame());
    open.store(false, Ordering::Release);
    gateway.on_close(session, close_frame).await;
}

fn start_fragment<T>(
    slot: &mut Option<T>,
    value: T,
    max_size: usize,
) -> std::result::Result<Option<T>, WebSocketCloseCode>
where
    T: FragmentLength,
{
    if slot.is_some() {
        return Err(WebSocketCloseCode::Protocol);
    }
    if value.len() > max_size {
        return Err(WebSocketCloseCode::MessageTooBig);
    }
    *slot = Some(value);
    Ok(None)
}

trait FragmentLength {
    fn len(&self) -> usize;
    fn extend(&mut self, data: &[u8]);
}

fn append_fragment<T: FragmentLength>(
    slot: &mut Option<T>,
    data: &[u8],
    max_size: usize,
) -> std::result::Result<(), WebSocketCloseCode> {
    let value = slot.as_mut().ok_or(WebSocketCloseCode::Protocol)?;
    if value
        .len()
        .checked_add(data.len())
        .is_none_or(|size| size > max_size)
    {
        return Err(WebSocketCloseCode::MessageTooBig);
    }
    value.extend(data);
    Ok(())
}

fn protocol_close_code(error: &ProtocolError) -> WebSocketCloseCode {
    match error {
        ProtocolError::Overflow => WebSocketCloseCode::MessageTooBig,
        ProtocolError::Io(error) if error.kind() == std::io::ErrorKind::InvalidData => {
            WebSocketCloseCode::InvalidData
        }
        _ => WebSocketCloseCode::Protocol,
    }
}

async fn close_with_protocol_error(
    gateway: &Arc<dyn WebSocketGateway>,
    session: &Arc<WebSocketSession>,
    code: WebSocketCloseCode,
    message: &'static str,
) -> WebSocketCloseFrame {
    gateway
        .on_error(session.clone(), WebSocketError::new(message))
        .await;
    let frame = WebSocketCloseFrame::new(code, message);
    let _ = session.close(Some(frame.clone())).await;
    frame
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

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{
        App,
        http::{StatusCode as ActixStatus, header},
        test,
    };
    use caelix_core::{Gateway, GatewayDef, Injectable, ModuleMetadata, build_container};

    #[actix_web::test]
    async fn protocol_errors_map_by_typed_variant() {
        assert_eq!(
            protocol_close_code(&ProtocolError::Overflow),
            WebSocketCloseCode::MessageTooBig
        );
        assert_eq!(
            protocol_close_code(&ProtocolError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "utf8"
            ))),
            WebSocketCloseCode::InvalidData
        );
        assert_eq!(
            protocol_close_code(&ProtocolError::UnmaskedFrame),
            WebSocketCloseCode::Protocol
        );
        assert_eq!(
            protocol_close_code(&ProtocolError::InvalidLength(126)),
            WebSocketCloseCode::Protocol
        );
    }

    #[derive(Default)]
    struct TestGateway {
        close_frames: std::sync::Mutex<Vec<Option<WebSocketCloseFrame>>>,
    }
    impl Injectable for TestGateway {
        fn dependencies() -> Vec<caelix_core::ProviderDependency> {
            caelix_core::provider_dependencies![]
        }

        fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
            Box::pin(async { Ok(Self::default()) })
        }
    }
    impl WebSocketGateway for TestGateway {
        fn on_text(
            &self,
            session: Arc<WebSocketSession>,
            text: String,
        ) -> BoxFuture<'_, Result<()>> {
            Box::pin(async move {
                if text == "local-close" {
                    session
                        .close(Some(WebSocketCloseFrame::new(
                            WebSocketCloseCode::Other(4001),
                            "local reason",
                        )))
                        .await
                } else if text == "server-1010" {
                    session
                        .close(Some(WebSocketCloseFrame::new(
                            WebSocketCloseCode::MandatoryExtension,
                            "not server-sendable",
                        )))
                        .await
                } else {
                    session.send_text(text).await
                }
            })
        }
        fn on_binary(
            &self,
            session: Arc<WebSocketSession>,
            data: Bytes,
        ) -> BoxFuture<'_, Result<()>> {
            Box::pin(async move { session.send_binary(data).await })
        }
        fn on_close(
            &self,
            _session: Arc<WebSocketSession>,
            frame: Option<WebSocketCloseFrame>,
        ) -> BoxFuture<'_, ()> {
            Box::pin(async move {
                self.close_frames.lock().unwrap().push(frame);
            })
        }
    }
    impl Gateway for TestGateway {
        fn definition() -> GatewayDef {
            GatewayDef::websocket::<Self>("/socket")
        }
    }
    struct TestModule;
    impl Module for TestModule {
        fn register() -> ModuleMetadata {
            ModuleMetadata::new().gateway::<TestGateway>()
        }
    }

    #[actix_web::test]
    async fn gateway_route_performs_rfc6455_handshake() {
        let container = Arc::new(build_container::<TestModule>().await.unwrap());
        let app = test::init_service(App::new().configure(move |cfg| {
            configure_gateway_routes::<TestModule>(cfg, container.clone(), 1024)
        }))
        .await;
        let request = test::TestRequest::get()
            .uri("/socket")
            .insert_header((header::UPGRADE, "websocket"))
            .insert_header((header::CONNECTION, "upgrade"))
            .insert_header((header::SEC_WEBSOCKET_VERSION, "13"))
            .insert_header((header::SEC_WEBSOCKET_KEY, "dGhlIHNhbXBsZSBub25jZQ=="))
            .to_request();
        let response = test::call_service(&app, request).await;
        assert_eq!(response.status(), ActixStatus::SWITCHING_PROTOCOLS);
        assert_eq!(
            response.headers().get(header::UPGRADE).unwrap(),
            "websocket"
        );
        assert!(
            response
                .headers()
                .contains_key(header::SEC_WEBSOCKET_ACCEPT)
        );
    }

    #[actix_web::test]
    async fn gateway_route_rejects_normal_http_requests() {
        let container = Arc::new(build_container::<TestModule>().await.unwrap());
        let app = test::init_service(App::new().configure(move |cfg| {
            configure_gateway_routes::<TestModule>(cfg, container.clone(), 1024)
        }))
        .await;
        let response =
            test::call_service(&app, test::TestRequest::get().uri("/socket").to_request()).await;
        assert_eq!(response.status(), ActixStatus::BAD_REQUEST);
    }

    #[actix_web::test]
    async fn gateway_route_rejects_each_invalid_handshake_shape() {
        let container = Arc::new(build_container::<TestModule>().await.unwrap());
        let app = test::init_service(App::new().configure(move |cfg| {
            configure_gateway_routes::<TestModule>(cfg, container.clone(), 1024)
        }))
        .await;
        let cases = [
            vec![
                ("connection", "upgrade"),
                ("sec-websocket-version", "13"),
                ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="),
            ],
            vec![
                ("upgrade", "websocket"),
                ("sec-websocket-version", "13"),
                ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="),
            ],
            vec![
                ("upgrade", "websocket"),
                ("connection", "upgrade"),
                ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="),
            ],
            vec![
                ("upgrade", "websocket"),
                ("connection", "upgrade"),
                ("sec-websocket-version", "12"),
                ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="),
            ],
            vec![
                ("upgrade", "websocket"),
                ("connection", "upgrade"),
                ("sec-websocket-version", "13"),
            ],
            vec![
                ("upgrade", "websocket"),
                ("connection", "upgrade"),
                ("sec-websocket-version", "13"),
                ("sec-websocket-key", "not-base64"),
            ],
        ];
        for headers in cases {
            let mut request = test::TestRequest::get().uri("/socket");
            for (name, value) in headers {
                request = request.insert_header((name, value));
            }
            let response = test::call_service(&app, request.to_request()).await;
            assert_eq!(response.status(), ActixStatus::BAD_REQUEST);
        }

        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/socket")
                .insert_header((header::UPGRADE, "websocket"))
                .insert_header((header::CONNECTION, "upgrade"))
                .insert_header((header::SEC_WEBSOCKET_VERSION, "12"))
                .insert_header((header::SEC_WEBSOCKET_KEY, "dGhlIHNhbXBsZSBub25jZQ=="))
                .to_request(),
        )
        .await;
        assert_eq!(
            response
                .headers()
                .get(header::SEC_WEBSOCKET_VERSION)
                .unwrap(),
            "13"
        );
    }

    #[actix_web::test]
    async fn real_client_preserves_text_binary_and_ping() {
        use actix_web::HttpServer;
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::{
            Message,
            protocol::frame::{
                Frame,
                coding::{Data, OpCode},
            },
        };

        let container = Arc::new(build_container::<TestModule>().await.unwrap());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = HttpServer::new(move || {
            let container = container.clone();
            App::new().configure(move |cfg| {
                configure_gateway_routes::<TestModule>(cfg, container.clone(), 16 * 1024)
            })
        })
        .listen(listener)
        .unwrap()
        .run();
        let handle = server.handle();
        actix_web::rt::spawn(server);

        let (mut socket, response) =
            tokio_tungstenite::connect_async(format!("ws://{address}/socket"))
                .await
                .unwrap();
        assert_eq!(response.status(), 101);
        socket.send(Message::Text("hello".into())).await.unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Text("hello".into())
        );
        let bytes = (0..8192)
            .map(|index| (index % 256) as u8)
            .collect::<Vec<_>>();
        socket
            .send(Message::Binary(bytes.clone().into()))
            .await
            .unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Binary(bytes.into())
        );
        socket.send(Message::Ping(vec![7, 8].into())).await.unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Pong(vec![7, 8].into())
        );
        socket
            .send(Message::Frame(Frame::message(
                "frag".as_bytes().to_vec(),
                OpCode::Data(Data::Text),
                false,
            )))
            .await
            .unwrap();
        socket
            .send(Message::Frame(Frame::message(
                "mented".as_bytes().to_vec(),
                OpCode::Data(Data::Continue),
                true,
            )))
            .await
            .unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Text("fragmented".into())
        );
        socket
            .send(Message::Frame(Frame::message(
                vec![0, 1],
                OpCode::Data(Data::Binary),
                false,
            )))
            .await
            .unwrap();
        socket
            .send(Message::Frame(Frame::message(
                vec![2, 255],
                OpCode::Data(Data::Continue),
                true,
            )))
            .await
            .unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Binary(vec![0, 1, 2, 255].into())
        );
        socket.close(None).await.unwrap();
        handle.stop(true).await;
    }

    #[actix_web::test]
    async fn close_codes_reasons_limits_and_invalid_utf8_are_preserved() {
        use actix_web::HttpServer;
        use futures_util::{SinkExt, StreamExt};
        use tokio::io::AsyncWriteExt;
        use tokio_tungstenite::tungstenite::{
            Message,
            protocol::{
                CloseFrame,
                frame::{
                    Frame,
                    coding::{CloseCode, Data, OpCode},
                },
            },
        };

        let container = Arc::new(build_container::<TestModule>().await.unwrap());
        let gateway = container.resolve::<TestGateway>().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server_container = container.clone();
        let server = HttpServer::new(move || {
            let container = server_container.clone();
            App::new().configure(move |cfg| {
                configure_gateway_routes::<TestModule>(cfg, container.clone(), 128)
            })
        })
        .listen(listener)
        .unwrap()
        .run();
        let handle = server.handle();
        actix_web::rt::spawn(server);
        let url = format!("ws://{address}/socket");

        let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        socket
            .send(Message::Text("local-close".into()))
            .await
            .unwrap();
        let Message::Close(Some(frame)) = socket.next().await.unwrap().unwrap() else {
            panic!("expected local close")
        };
        assert_eq!(u16::from(frame.code), 4001);
        assert_eq!(frame.reason, "local reason");

        let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        socket
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::Restart,
                reason: "deploying".into(),
            })))
            .await
            .unwrap();
        let _ = socket.next().await;

        let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        socket
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::Extension,
                reason: "client extension".into(),
            })))
            .await
            .unwrap();
        let _ = socket.next().await;

        let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        socket
            .send(Message::Close(Some(CloseFrame {
                code: CloseCode::Normal,
                reason: "valid � reason".into(),
            })))
            .await
            .unwrap();
        let _ = socket.next().await;

        let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        let mask = [1u8, 2, 3, 4];
        let raw_payload = [1000u16.to_be_bytes().as_slice(), &[0xff]].concat();
        let mut invalid_reason = vec![0x88, 0x80 | raw_payload.len() as u8];
        invalid_reason.extend(mask);
        invalid_reason.extend(
            raw_payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );
        socket.get_mut().write_all(&invalid_reason).await.unwrap();
        socket.get_mut().flush().await.unwrap();
        let Message::Close(Some(frame)) = socket.next().await.unwrap().unwrap() else {
            panic!("expected invalid UTF-8 close")
        };
        assert_eq!(frame.code, CloseCode::Invalid);

        let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        let one_byte_payload = [0x7fu8];
        let mut malformed_close = vec![0x88, 0x81];
        malformed_close.extend(mask);
        malformed_close.extend(
            one_byte_payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );
        socket.get_mut().write_all(&malformed_close).await.unwrap();
        socket.get_mut().flush().await.unwrap();
        let Message::Close(Some(frame)) = socket.next().await.unwrap().unwrap() else {
            panic!("expected malformed-close protocol error")
        };
        assert_eq!(frame.code, CloseCode::Protocol);

        let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        socket
            .send(Message::Text("server-1010".into()))
            .await
            .unwrap();
        let Message::Close(Some(frame)) = socket.next().await.unwrap().unwrap() else {
            panic!("expected rejected server 1010 close")
        };
        assert_eq!(frame.code, CloseCode::Error);

        let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        socket
            .send(Message::Binary(vec![0; 129].into()))
            .await
            .unwrap();
        let Message::Close(Some(frame)) = socket.next().await.unwrap().unwrap() else {
            panic!("expected size close")
        };
        assert_eq!(frame.code, CloseCode::Size);

        let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        socket
            .send(Message::Frame(Frame::message(
                vec![0xff, 0xfe],
                OpCode::Data(Data::Text),
                true,
            )))
            .await
            .unwrap();
        let Message::Close(Some(frame)) = socket.next().await.unwrap().unwrap() else {
            panic!("expected invalid-data close")
        };
        assert_eq!(frame.code, CloseCode::Invalid);

        let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        socket
            .get_mut()
            .write_all(&[0x81, 0x01, b'x'])
            .await
            .unwrap();
        socket.get_mut().flush().await.unwrap();
        let Message::Close(Some(frame)) = socket.next().await.unwrap().unwrap() else {
            panic!("expected protocol close")
        };
        assert_eq!(frame.code, CloseCode::Protocol);

        let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        let mut interleaved = Vec::new();
        for (header, payload) in [
            (0x01u8, b"first".as_slice()),
            (0x81u8, b"second".as_slice()),
        ] {
            interleaved.extend([header, 0x80 | payload.len() as u8]);
            interleaved.extend(mask);
            interleaved.extend(
                payload
                    .iter()
                    .enumerate()
                    .map(|(index, byte)| byte ^ mask[index % 4]),
            );
        }
        socket.get_mut().write_all(&interleaved).await.unwrap();
        socket.get_mut().flush().await.unwrap();
        let Message::Close(Some(frame)) = socket.next().await.unwrap().unwrap() else {
            panic!("expected interleaving protocol close")
        };
        assert_eq!(frame.code, CloseCode::Protocol);

        let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        let invalid_code = 1005u16.to_be_bytes();
        let mut invalid_close = vec![0x88, 0x82];
        invalid_close.extend(mask);
        invalid_close.extend(
            invalid_code
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );
        socket.get_mut().write_all(&invalid_close).await.unwrap();
        socket.get_mut().flush().await.unwrap();
        let Message::Close(Some(frame)) = socket.next().await.unwrap().unwrap() else {
            panic!("expected invalid-code protocol close")
        };
        assert_eq!(frame.code, CloseCode::Protocol);

        let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
        let mut oversized_ping = vec![0x89, 0xfe, 0, 126, 1, 2, 3, 4];
        oversized_ping.extend(std::iter::repeat_n(0u8, 126));
        socket.get_mut().write_all(&oversized_ping).await.unwrap();
        socket.get_mut().flush().await.unwrap();
        let Message::Close(Some(frame)) = socket.next().await.unwrap().unwrap() else {
            panic!("expected control-frame protocol close")
        };
        assert_eq!(frame.code, CloseCode::Protocol);

        actix_web::rt::time::sleep(std::time::Duration::from_millis(20)).await;
        let frames = gateway.close_frames.lock().unwrap();
        assert!(frames.iter().any(|frame| {
            frame
                .as_ref()
                .is_some_and(|frame| frame.code.as_u16() == 4001 && frame.reason == "local reason")
        }));
        assert!(
            frames.iter().any(|frame| frame
                .as_ref()
                .is_some_and(|frame| frame.code == WebSocketCloseCode::Restart
                    && frame.reason == "deploying"))
        );
        assert!(frames.iter().any(|frame| {
            frame
                .as_ref()
                .is_some_and(|frame| frame.reason == "valid � reason")
        }));
        assert!(
            frames
                .iter()
                .any(|frame| frame.as_ref().is_some_and(|frame| {
                    frame.code == WebSocketCloseCode::MandatoryExtension
                        && frame.reason == "client extension"
                }))
        );
        drop(frames);
        handle.stop(true).await;
    }
}
