use std::any::Any;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RouteDef {
    pub method: &'static str,
    pub path: &'static str,
    pub handler: &'static str,
}

pub trait Controller {
    fn base_path() -> &'static str;
    fn routes() -> &'static [RouteDef] {
        &[]
    }
    fn register_routes(cfg: &mut dyn Any);
}
