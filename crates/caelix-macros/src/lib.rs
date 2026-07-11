mod controller;
mod gateway;
mod injectable;

#[cfg(all(feature = "actix", feature = "axum"))]
compile_error!("caelix-macros backend features `actix` and `axum` are mutually exclusive");

use proc_macro::TokenStream;
use quote::quote;
#[cfg(feature = "axum")]
use syn::{ItemFn, parse_macro_input, parse_quote};

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

/// Registers either an RFC 6455 `impl WebSocketGateway` or an Axum Socket.IO
/// gateway implementation at the supplied path.
#[proc_macro_attribute]
pub fn gateway(args: TokenStream, input: TokenStream) -> TokenStream {
    gateway::expand(args, input)
}

/// Marks an async method on a Socket.IO `#[gateway]` implementation as an
/// event handler.
#[proc_macro_attribute]
pub fn on_message(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Marks an async `main` that runs on the selected backend runtime.
///
/// Expands through `caelix::__actix_web` so consumers only need a `caelix` dependency
/// (with the `actix` feature), not a direct `actix-web` dependency.
#[proc_macro_attribute]
pub fn main(_args: TokenStream, item: TokenStream) -> TokenStream {
    #[cfg(feature = "axum")]
    {
        expand_tokio_runtime(item, false)
    }
    #[cfg(not(feature = "axum"))]
    {
        let item = proc_macro2::TokenStream::from(item);
        quote! { #[caelix::__actix_web::rt::main(system = "caelix::__actix_web::rt::System")] #item }.into()
    }
}

/// Marks an async test that runs on the selected backend runtime.
///
/// Prefer this for integration tests that use `TestApplication`. Expands through
/// `caelix::__actix_web` so consumers only need a `caelix` dependency.
#[proc_macro_attribute]
pub fn test(_args: TokenStream, item: TokenStream) -> TokenStream {
    #[cfg(feature = "axum")]
    {
        expand_tokio_runtime(item, true)
    }
    #[cfg(not(feature = "axum"))]
    {
        let item = proc_macro2::TokenStream::from(item);
        quote! { #[caelix::__actix_web::rt::test(system = "caelix::__actix_web::rt::System")] #item }.into()
    }
}

#[cfg(feature = "axum")]
fn expand_tokio_runtime(item: TokenStream, is_test: bool) -> TokenStream {
    let mut function = parse_macro_input!(item as ItemFn);
    let body = function.block;
    function.sig.asyncness = None;
    function.block = Box::new(parse_quote!({
        caelix::__tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build the Caelix runtime")
            .block_on(async #body)
    }));
    if is_test {
        function
            .attrs
            .push(parse_quote!(#[::core::prelude::v1::test]));
    }
    quote!(#function).into()
}
