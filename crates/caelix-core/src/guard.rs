use crate::{BoxFuture, RequestContext, Result};

pub trait Guard: Send + Sync + 'static {
    fn can_activate<'a>(&'a self, ctx: &'a RequestContext) -> BoxFuture<'a, Result<bool>>;
}
