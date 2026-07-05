//! Public Caelix framework crate.

pub use caelix_core as core;
pub use caelix_core::*;
pub use caelix_macros::*;

pub mod prelude {
    pub use caelix_core::*;
    pub use caelix_macros::*;
}
