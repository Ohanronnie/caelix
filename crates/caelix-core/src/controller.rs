use std::any::Any;

use crate::ProviderDependency;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Public Caelix type `RouteDef`.
pub struct RouteDef {
    /// The `method` value.
    pub method: &'static str,
    /// The `path` value.
    pub path: &'static str,
    /// The `handler` value.
    pub handler: &'static str,
}

/// Public Caelix extension trait `Controller`.
pub trait Controller {
    /// Public Caelix API.
    fn base_path() -> &'static str;

    /// Providers resolved by generated route wrappers, such as guards and
    /// interceptors. These participate in module visibility validation and
    /// lifecycle ordering just like constructor-injected dependencies.
    fn route_dependencies() -> Vec<ProviderDependency> {
        vec![]
    }

    /// Public Caelix API.
    fn routes() -> &'static [RouteDef] {
        &[]
    }
    /// Public Caelix API.
    fn register_routes(cfg: &mut dyn Any);

    #[cfg(feature = "openapi")]
    #[doc(hidden)]
    fn openapi_routes() -> &'static [crate::openapi::OpenApiRouteDef] {
        &[]
    }
}
