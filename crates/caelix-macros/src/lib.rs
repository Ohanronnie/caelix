mod controller;
mod injectable;

use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn injectable(args: TokenStream, input: TokenStream) -> TokenStream {
    injectable::expand(args, input)
}

#[proc_macro_attribute]
pub fn controller(args: TokenStream, input: TokenStream) -> TokenStream {
    controller::expand(args, input)
}
