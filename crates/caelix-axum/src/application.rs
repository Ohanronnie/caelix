use std::{convert::Infallible, sync::Arc, time::Instant};

use axum::{
    Router,
    body::Body,
    extract::FromRequestParts,
    http::{HeaderMap, Method, Request, Uri, request::Parts},
    response::Response,
};
#[cfg(not(feature = "socketio"))]
use caelix_core::build_container;
#[cfg(feature = "socketio")]
use caelix_core::build_container_with_setup;
#[cfg(feature = "socketio")]
use caelix_core::visit_module_gateways;
use caelix_core::{
    BoxFuture, Container, HttpResponse as CaelixHttpResponse, IntoCaelixResponse, Module,
    NotFoundException, ResponseBody, log_application_started, log_listening, log_module_routes,
    register_module_controllers, shutdown_module,
};
use futures_util::StreamExt;
use tower::{Layer, Service};

pub const DEFAULT_BODY_LIMIT_BYTES: usize = 1024 * 1024;

/// The request data Caelix needs to construct a framework-neutral
/// [`caelix_core::RequestContext`]. It is a parts-only Axum extractor so it
/// can be combined with `Json` and other body-consuming extractors.
pub struct AxumRequestInfo {
    method: Method,
    uri: Uri,
    headers: HeaderMap,
}

impl AxumRequestInfo {
    pub fn method(&self) -> &Method {
        &self.method
    }
    pub fn path(&self) -> &str {
        self.uri.path()
    }
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }
}

impl<S> FromRequestParts<S> for AxumRequestInfo
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        Ok(Self {
            method: parts.method.clone(),
            uri: parts.uri.clone(),
            headers: parts.headers.clone(),
        })
    }
}

/// Mutable route collector used by framework-neutral controller metadata.
///
/// Axum's [`Router`] builder consumes itself when registering a route. This
/// wrapper preserves Caelix's existing `Controller::register_routes(&mut dyn Any)`
/// contract without putting an HTTP framework type in `caelix-core`.
pub struct AxumRouterBuilder {
    router: Router<Arc<Container>>,
}

impl AxumRouterBuilder {
    pub fn new() -> Self {
        Self {
            router: Router::new(),
        }
    }

    pub fn route(
        &mut self,
        path: &str,
        method_router: axum::routing::MethodRouter<Arc<Container>>,
    ) {
        self.router = std::mem::take(&mut self.router).route(path, method_router);
    }

    pub fn into_router(self, container: Arc<Container>) -> Router {
        self.router.with_state(container)
    }
}

impl Default for AxumRouterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Converts a framework-neutral response into an Axum response.
pub fn to_axum_response(response: CaelixHttpResponse) -> Response {
    let mut builder = Response::builder()
        .status(response.status)
        .header("content-type", response.content_type);
    for (name, value) in response.headers {
        builder = builder.header(name, value);
    }

    let body = match response.body {
        ResponseBody::Buffered(bytes) => Body::from(bytes),
        ResponseBody::Streaming(stream) => {
            Body::from_stream(stream.filter_map(|chunk| async move {
                match chunk {
                    Ok(chunk) => Some(Ok::<_, Infallible>(axum::body::Bytes::from(chunk))),
                    Err(err) => {
                        caelix_core::log_http_exception(&err);
                        // HTTP status and headers have already been committed when
                        // a stream fails, so end the response body cleanly.
                        None
                    }
                }
            }))
        }
    };

    builder
        .body(body)
        .unwrap_or_else(|_| Response::new(Body::from("Internal Server Error")))
}

type RouterLayer = Box<dyn FnOnce(Router) -> Router + Send>;

pub struct Application {
    container: Arc<Container>,
    configure_fn: fn(&mut dyn std::any::Any),
    gateway_configure_fn: fn(&mut AxumRouterBuilder, Arc<Container>, usize),
    shutdown_fn: for<'a> fn(&'a Container) -> BoxFuture<'a, caelix_core::Result<()>>,
    body_limit: usize,
    websocket_max_message_size: usize,
    layers: Vec<RouterLayer>,
    #[cfg(feature = "socketio")]
    socket_io_layer: Option<caelix_socketio::SocketIoLayer>,
}

impl Application {
    pub async fn new<M: Module + 'static>() -> caelix_core::Result<Self> {
        let start = Instant::now();
        #[cfg(not(feature = "socketio"))]
        let container = build_container::<M>().await?;
        #[cfg(feature = "socketio")]
        let (container, socket_io_layer) = {
            let (layer, handle) = caelix_socketio::SocketIoHandle::build();
            let container = build_container_with_setup::<M>(|container| {
                container.register_instance(handle);
            })
            .await?;
            (container, layer)
        };
        log_module_routes::<M>();
        log_application_started(start.elapsed());

        Ok(Self {
            container: Arc::new(container),
            configure_fn: |router| register_module_controllers::<M>(router),
            gateway_configure_fn: |routes, container, max_message_size| {
                crate::websocket::configure_gateway_routes::<M>(routes, container, max_message_size)
            },
            shutdown_fn: |container| Box::pin(async move { shutdown_module::<M>(container).await }),
            body_limit: DEFAULT_BODY_LIMIT_BYTES,
            websocket_max_message_size: crate::websocket::DEFAULT_WEBSOCKET_MAX_MESSAGE_SIZE,
            layers: Vec::new(),
            #[cfg(feature = "socketio")]
            socket_io_layer: Some(socket_io_layer),
        })
    }

    pub fn body_limit(mut self, bytes: usize) -> Self {
        self.body_limit = bytes;
        self
    }

    /// Sets the maximum assembled RFC 6455 message size accepted by decorated
    /// WebSocket gateways.
    pub fn websocket_max_message_size(mut self, bytes: usize) -> Self {
        self.websocket_max_message_size = bytes.max(1);
        self
    }

    /// Adds a native Tower layer to the underlying Axum router.
    pub fn layer<L>(mut self, layer: L) -> Self
    where
        L: Layer<axum::routing::Route> + Clone + Send + Sync + 'static,
        L::Service: Service<Request<Body>> + Clone + Send + Sync + 'static,
        <L::Service as Service<Request<Body>>>::Response: axum::response::IntoResponse + 'static,
        <L::Service as Service<Request<Body>>>::Error: Into<Infallible> + 'static,
        <L::Service as Service<Request<Body>>>::Future: Send + 'static,
    {
        self.layers
            .push(Box::new(move |router| router.layer(layer)));
        self
    }

    /// Attaches Socket.IO to this Axum application and registers its handle as
    /// an injectable provider. This method only exists when the `socketio`
    /// feature is enabled, which itself requires the Axum backend.
    #[cfg(feature = "socketio")]
    pub fn with_socket_io<M: Module + 'static>(mut self) -> Self {
        let handle = self
            .container
            .resolve::<caelix_socketio::SocketIoHandle>()
            .expect("Socket.IO handle must be registered before module providers are built");
        visit_module_gateways::<M>(&mut |gateway| {
            gateway
                .register_socket_io(&self.container, handle.as_ref())
                .expect("Socket.IO gateway metadata must be valid");
        });
        let layer = self
            .socket_io_layer
            .take()
            .expect("Socket.IO can only be attached once per application");
        self.layer(layer)
    }

    pub fn into_router(self) -> Router {
        let mut routes = AxumRouterBuilder::new();
        (self.configure_fn)(&mut routes);
        (self.gateway_configure_fn)(
            &mut routes,
            self.container.clone(),
            self.websocket_max_message_size,
        );
        let mut router = routes.into_router(self.container);
        router = router.layer(axum::extract::DefaultBodyLimit::max(self.body_limit));
        router = router.fallback(|request: Request<Body>| async move {
            to_axum_response(
                NotFoundException::new(format!(
                    "Cannot {} {}",
                    request.method(),
                    request.uri().path()
                ))
                .into_response(),
            )
        });
        for layer in self.layers {
            router = layer(router);
        }
        router
    }

    async fn shutdown(&self) -> caelix_core::Result<()> {
        (self.shutdown_fn)(&self.container).await
    }

    pub async fn listen(self, addr: &str) -> std::io::Result<()> {
        log_listening(addr);
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(error) => {
                let _ = self.shutdown().await;
                return Err(error);
            }
        };
        let shutdown_fn = self.shutdown_fn;
        let container = self.container.clone();
        let router = self.into_router();
        let result = axum::serve(listener, router).await;
        shutdown_fn(&container).await.map_err(to_io_error)?;
        result
    }
}

fn to_io_error(err: caelix_core::HttpException) -> std::io::Error {
    std::io::Error::other(err.message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use caelix_core::{HttpResponse, ModuleMetadata, Response as CaelixResponse};
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn response_adapter_preserves_status_body_and_content_type() {
        let response = to_axum_response(
            HttpResponse::text(StatusCode::CREATED, "created").with_header("X-Request-Id", "abc"),
        );
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()["content-type"], "text/plain");
        assert_eq!(response.headers()["x-request-id"], "abc");
        assert_eq!(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .as_ref(),
            b"created"
        );
    }

    #[tokio::test]
    async fn response_adapter_streams_chunks() {
        let response = to_axum_response(CaelixResponse::stream(
            "text/plain",
            futures_util::stream::iter(vec![
                Ok(bytes::Bytes::from_static(b"one")),
                Ok(bytes::Bytes::from_static(b"two")),
            ]),
        ));
        assert_eq!(response.headers()["content-type"], "text/plain");
        assert_eq!(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .as_ref(),
            b"onetwo"
        );
    }

    struct EmptyModule;
    impl Module for EmptyModule {
        fn register() -> ModuleMetadata {
            ModuleMetadata::new()
        }
    }

    #[tokio::test]
    async fn application_accepts_native_tower_layers() {
        let _router = Application::new::<EmptyModule>()
            .await
            .unwrap()
            .layer(tower_http::trace::TraceLayer::new_for_http())
            .layer(tower_http::compression::CompressionLayer::new())
            .into_router();
    }
}
