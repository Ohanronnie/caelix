use crate::{BoxFuture, HttpResponse, RequestContext, Result};

pub struct Next<'a> {
    inner: Box<dyn FnOnce() -> BoxFuture<'a, Result<HttpResponse>> + Send + 'a>,
}

impl<'a> Next<'a> {
    pub fn new(f: impl FnOnce() -> BoxFuture<'a, Result<HttpResponse>> + Send + 'a) -> Self {
        Self { inner: Box::new(f) }
    }

    pub fn run(self) -> BoxFuture<'a, Result<HttpResponse>> {
        (self.inner)()
    }
}

pub trait Interceptor: Send + Sync + 'static {
    fn intercept<'a>(
        &'a self,
        ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Result<HttpResponse>>;
}
