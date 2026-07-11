#![cfg(feature = "actix")]

use caelix::{
    BoxFuture, Module, ModuleMetadata, Result, WebSocketGateway, WebSocketSession, gateway,
    injectable,
};
use std::sync::Arc;

#[injectable]
struct EchoGateway;

#[gateway("/echo")]
impl WebSocketGateway for EchoGateway {
    fn on_text(&self, session: Arc<WebSocketSession>, text: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { session.send_text(text).await })
    }
}

struct GatewayModule;

impl Module for GatewayModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().gateway::<EchoGateway>()
    }
}

#[caelix::test]
async fn gateway_attribute_registers_rfc6455_metadata() {
    caelix::build_container::<GatewayModule>().await.unwrap();
}
