use crate::{BoxFuture, HttpResponse, RequestContext, Result};

/// Public Caelix type `Next`.
pub struct Next<'a> {
    inner: Box<dyn FnOnce() -> BoxFuture<'a, Result<HttpResponse>> + Send + 'a>,
}

impl<'a> Next<'a> {
    /// Runs the `new` public API operation.
    pub fn new(f: impl FnOnce() -> BoxFuture<'a, Result<HttpResponse>> + Send + 'a) -> Self {
        Self { inner: Box::new(f) }
    }

    /// Runs the `run` public API operation.
    pub fn run(self) -> BoxFuture<'a, Result<HttpResponse>> {
        (self.inner)()
    }
}

/// Public Caelix extension trait `Interceptor`.
pub trait Interceptor: Send + Sync + 'static {
    /// Public Caelix API.
    fn intercept<'a>(
        &'a self,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Result<HttpResponse>>;
}
