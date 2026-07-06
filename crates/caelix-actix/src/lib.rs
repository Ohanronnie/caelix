mod application;

pub use actix_web::main;
pub use application::{Application, to_actix_response};
