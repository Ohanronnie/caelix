#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Procedural macros for defining Caelix applications.

mod controller;
mod gateway;
mod injectable;

#[cfg(all(feature = "actix", feature = "axum"))]
compile_error!("caelix-macros backend features `actix` and `axum` are mutually exclusive");

use proc_macro::TokenStream;
use quote::quote;
#[cfg(feature = "axum")]
use syn::{ItemFn, parse_macro_input, parse_quote};

/// Implements `caelix::Injectable` for a named or unit struct.
///
/// Every named field must be `Arc<T>` and is resolved from the module container;
/// `Arc<Logger>` receives a logger scoped to the struct name. Tuple structs are
/// rejected. The macro also records the resolved dependencies for visibility
/// validation and preserves the optional lifecycle hooks on `Injectable`.
#[proc_macro_attribute]
pub fn injectable(args: TokenStream, input: TokenStream) -> TokenStream {
    injectable::expand(args, input)
}

/// Marks a dependency-injected struct as a route guard.
///
/// It accepts the same struct forms and field rules as [`injectable`], and
/// generates `Injectable`; implement `caelix::Guard` separately to decide
/// whether a request may continue.
#[proc_macro_attribute]
pub fn guard(args: TokenStream, input: TokenStream) -> TokenStream {
    injectable::expand(args, input)
}

/// Generates controller metadata and backend routes for an `impl` block.
///
/// The required argument is a base path such as `#[controller("/users")]`.
/// Methods may use `#[get]`, `#[post]`, `#[put]`, `#[patch]`, or `#[delete]`;
/// extractor and guard/interceptor attributes are interpreted by the selected
/// Actix or Axum backend. Generated code resolves controller dependencies from
/// the `caelix` facade, so applications need no direct runtime dependency.
#[proc_macro_attribute]
pub fn controller(args: TokenStream, input: TokenStream) -> TokenStream {
    controller::expand(args, input)
}

/// Declares an OpenAPI response for a controller handler.
///
/// This marker is consumed by [`controller`] when the `openapi` feature is
/// enabled and otherwise leaves the annotated item unchanged.
#[proc_macro_attribute]
pub fn response(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Declares OpenAPI error responses for a controller handler.
///
/// Use exception marker types as arguments; the attribute is documentation
/// metadata only and leaves the item unchanged outside controller expansion.
#[proc_macro_attribute]
pub fn errors(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Declares a request-header parameter in generated OpenAPI documentation.
///
/// This is consumed by [`controller`] with the `openapi` feature and has no
/// runtime extraction effect by itself.
#[proc_macro_attribute]
pub fn request_header(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Declares OpenAPI security requirements for a controller or route handler.
///
/// Use `caelix::openapi::Security` values accepted by the controller macro;
/// this marker affects generated documentation, not request authentication.
#[proc_macro_attribute]
pub fn security(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Registers either an RFC 6455 `impl WebSocketGateway` or an Axum Socket.IO
/// gateway implementation at the supplied path.
///
/// Apply it to an `impl` block with a string path argument. Regular WebSocket
/// gateways implement `WebSocketGateway`; Socket.IO gateway expansion is only
/// available with the Axum-selected `socketio` feature and works with
/// [`on_message`] methods.
#[proc_macro_attribute]
pub fn gateway(args: TokenStream, input: TokenStream) -> TokenStream {
    gateway::expand(args, input)
}

/// Marks an async method on a Socket.IO `#[gateway]` implementation as an
/// event handler.
///
/// The event name is derived from the method name unless configured by the
/// gateway macro. This attribute is meaningful only for Socket.IO gateways;
/// it otherwise leaves the method unchanged until gateway expansion consumes it.
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
