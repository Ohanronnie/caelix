//! Public Caelix framework crate.

pub use caelix_core as core;
pub use caelix_core::exception;
pub use caelix_core::response;

pub mod prelude {
    #[allow(unused_imports)]
    pub use caelix_core::StatusCode;
}
