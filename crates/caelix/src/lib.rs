//! Public Caelix framework crate.

pub use caelix_core::*;
pub use caelix_macros::*;

#[cfg(feature = "actix")]
pub use caelix_actix::{Application, main, to_actix_response};

pub mod prelude {
    pub use caelix_core::*;
    pub use caelix_macros::*;
}
