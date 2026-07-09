mod controller;
mod injectable;

use proc_macro::TokenStream;
use quote::quote;

#[proc_macro_attribute]
pub fn injectable(args: TokenStream, input: TokenStream) -> TokenStream {
    injectable::expand(args, input)
}

#[proc_macro_attribute]
pub fn guard(args: TokenStream, input: TokenStream) -> TokenStream {
    injectable::expand(args, input)
}

#[proc_macro_attribute]
pub fn controller(args: TokenStream, input: TokenStream) -> TokenStream {
    controller::expand(args, input)
}

/// Marks an async `main` that runs on the Actix runtime.
///
/// Expands through `caelix::__actix_web` so consumers only need a `caelix` dependency
/// (with the `actix` feature), not a direct `actix-web` dependency.
#[proc_macro_attribute]
pub fn main(_args: TokenStream, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    quote! {
        // Use `caelix::` (not `::caelix::`) so paths resolve to the facade crate
        // for apps and to a local `mod caelix` shim in macro unit tests.
        #[caelix::__actix_web::rt::main(system = "caelix::__actix_web::rt::System")]
        #item
    }
    .into()
}

/// Marks an async test that runs on the Actix runtime.
///
/// Prefer this for integration tests that use `TestApplication`. Expands through
/// `caelix::__actix_web` so consumers only need a `caelix` dependency.
#[proc_macro_attribute]
pub fn test(_args: TokenStream, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    quote! {
        #[caelix::__actix_web::rt::test(system = "caelix::__actix_web::rt::System")]
        #item
    }
    .into()
}
