#![cfg(feature = "socketio")]

use std::sync::Arc;

use caelix::socket_io::SocketRef;
use caelix::{
    Application, Module, ModuleMetadata, Result, gateway, injectable, socket_io::SocketIoHandle,
};

#[injectable]
#[allow(dead_code)]
struct SocketIoConsumer {
    handle: Arc<SocketIoHandle>,
}

#[injectable]
struct ChatGateway;

#[gateway("/chat")]
impl ChatGateway {
    #[on_message("echo")]
    async fn echo(&self, text: String) -> Result<String> {
        Ok(text)
    }

    #[on_message("fail")]
    async fn fail(&self, _input: String) -> Result<String> {
        Err(caelix::BadRequestException::new("bad input"))
    }

    #[on_message("join")]
    async fn join(&self, socket: SocketRef, room: String) -> Result<String> {
        socket.join(room);
        Ok("joined".to_string())
    }

    #[on_message("announce")]
    async fn announce(&self, socket: SocketRef, message: String) -> Result<String> {
        let _ = socket.within("room").emit("room-message", &message).await;
        Ok("sent".to_string())
    }
}

#[injectable]
struct OtherGateway;

#[gateway("/other")]
impl OtherGateway {
    #[on_message("echo")]
    async fn echo(&self, text: String) -> Result<String> {
        Ok(format!("other:{text}"))
    }
}

struct SocketIoModule;

impl Module for SocketIoModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<SocketIoConsumer>()
            .gateway::<ChatGateway>()
            .gateway::<OtherGateway>()
    }
}

#[caelix::test]
async fn socket_io_handle_is_injectable_and_real_client_receives_acks_and_errors() {
    let application = Application::new::<SocketIoModule>()
        .await
        .unwrap()
        .with_socket_io::<SocketIoModule>();
    let listener = caelix::__tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    let server = caelix::__tokio::spawn(async move {
        caelix::__axum::serve(listener, application.into_router())
            .await
            .unwrap();
    });

    let harness = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/socketio_client");
    if !harness.join("node_modules/socket.io-client").exists() {
        let install = std::process::Command::new("npm")
            .args(["ci", "--ignore-scripts"])
            .current_dir(&harness)
            .status()
            .expect("Node.js and npm are required for the Socket.IO compatibility test");
        assert!(
            install.success(),
            "could not install the pinned socket.io-client harness"
        );
    }
    let output = caelix::__tokio::task::spawn_blocking(move || {
        std::process::Command::new("node")
            .arg("ack-and-error.js")
            .arg(port)
            .current_dir(harness)
            .output()
            .expect("Node.js is required for the Socket.IO compatibility test")
    })
    .await
    .unwrap();
    server.abort();
    assert!(
        output.status.success(),
        "Socket.IO client harness failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
