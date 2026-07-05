use std::any::Any;

pub trait Controller {
    fn base_path() -> &'static str;
    fn register_routes(cfg: &mut dyn Any);
}
