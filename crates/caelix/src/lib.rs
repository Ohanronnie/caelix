//! Public Caelix framework crate.

pub use caelix_core as core;
pub use caelix_core::*;
pub use caelix_macros::*;

#[cfg(feature = "actix")]
pub use caelix_actix::main;

pub mod prelude {
    pub use caelix_core::*;
    pub use caelix_macros::*;
}
