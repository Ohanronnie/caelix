//! Public Caelix framework crate.

pub use caelix_core::*;
// Explicit re-exports so `test` / `main` are not pulled into `prelude` (which
// would shadow Rust's `#[test]`).
pub use caelix_macros::{controller, guard, injectable};

#[cfg(feature = "actix")]
pub use caelix_macros::{main, test};

/// Hidden Actix re-export for macro-generated code. Prefer `caelix` public APIs
/// (`Application`, `TestApplication`, `#[caelix::main]`, `#[caelix::test]`).
#[cfg(feature = "actix")]
#[doc(hidden)]
pub use caelix_actix::__actix_web;

#[cfg(feature = "actix")]
pub use caelix_actix::{
    Application, Logging, TestApplication, TestApplicationBuilder, TestRequestBuilder,
    TestResponse, to_actix_response,
};

pub mod prelude {
    pub use caelix_core::*;
    pub use caelix_macros::{controller, guard, injectable};
}
