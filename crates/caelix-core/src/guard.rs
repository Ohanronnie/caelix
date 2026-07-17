use crate::{BoxFuture, RequestContext, Result};

/// Public Caelix extension trait `Guard`.
pub trait Guard: Send + Sync + 'static {
    /// Public Caelix API.
    fn can_activate<'a>(&'a self, ctx: &'a RequestContext) -> BoxFuture<'a, Result<bool>>;
}
