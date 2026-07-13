use std::any::Any;

use crate::ProviderDependency;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RouteDef {
    pub method: &'static str,
    pub path: &'static str,
    pub handler: &'static str,
}

pub trait Controller {
    fn base_path() -> &'static str;

    /// Providers resolved by generated route wrappers, such as guards and
    /// interceptors. These participate in module visibility validation and
    /// lifecycle ordering just like constructor-injected dependencies.
    fn route_dependencies() -> Vec<ProviderDependency> {
        vec![]
    }

    fn routes() -> &'static [RouteDef] {
        &[]
    }
    fn register_routes(cfg: &mut dyn Any);

    #[cfg(feature = "openapi")]
    #[doc(hidden)]
    fn openapi_routes() -> &'static [crate::openapi::OpenApiRouteDef] {
        &[]
    }
}
