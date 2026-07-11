//! Internal WebSocket transport derived from `actix-ws` 0.4.0.
//!
//! Kept inside the published adapter so Caelix's RFC 6455 validation is not lost when Cargo
//! resolves dependencies from crates.io. The upstream implementation is MIT OR Apache-2.0.

use actix_http::{
    body::{BodyStream, MessageBody},
    ws::handshake,
};
use actix_web::{HttpRequest, HttpResponse, web};
use tokio::sync::{mpsc::channel, oneshot};

mod session;
mod stream;

pub(crate) use self::{session::Session, stream::MessageStream};

pub(crate) fn handle(
    req: &HttpRequest,
    body: web::Payload,
) -> Result<(HttpResponse, Session, MessageStream), actix_web::Error> {
    let mut response = handshake(req.head())?;
    let (tx, rx) = channel(32);
    let (connection_closed_tx, connection_closed_rx) = oneshot::channel();

    Ok((
        response
            .message_body(
                BodyStream::new(
                    stream::StreamingBody::new(rx)
                        .with_connection_close_signal(connection_closed_tx),
                )
                .boxed(),
            )?
            .into(),
        Session::new(tx),
        MessageStream::new(body.into_inner()).with_connection_close_signal(connection_closed_rx),
    ))
}
