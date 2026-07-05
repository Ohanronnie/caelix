use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, GenericArgument, PathArguments, Type, parse_macro_input};

pub(crate) fn expand(_args: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = &input.ident;

    let create_body = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => {
                let field_resolutions = fields.named.iter().map(|field| {
                    let field_name = field.ident.as_ref().unwrap();
                    let field_type = &field.ty;
                    let resolved_type = arc_inner_type(field_type).unwrap_or_else(|| {
                        panic!("#[injectable] fields must be std::sync::Arc<T>")
                    });

                    if is_logger_type(resolved_type) {
                        return quote! {
                            #field_name: container.resolve_logger(stringify!(#struct_name))
                        };
                    }

                    quote! {
                        #field_name: container.resolve::<#resolved_type>()
                    }
                });

                quote! {
                    Self {
                        #(#field_resolutions),*
                    }
                }
            }
            Fields::Unit => quote! { Self },
            Fields::Unnamed(_) => {
                panic!("#[injectable] only supports named-field or unit structs")
            }
        },
        _ => panic!("#[injectable] can only be applied to structs"),
    };

    let expanded = quote! {
        #input

        impl caelix_core::Injectable for #struct_name {
            fn create(container: &caelix_core::Container) -> caelix_core::BoxFuture<'_, Self> {
                Box::pin(async move {
                    #create_body
                })
            }
        }
    };

    expanded.into()
}

fn arc_inner_type(ty: &Type) -> Option<&Type> {
    let Type::Path(type_path) = ty else {
        return None;
    };

    let segment = type_path.path.segments.last()?;
    if segment.ident != "Arc" {
        return None;
    }

    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };

    match arguments.args.first()? {
        GenericArgument::Type(inner_type) => Some(inner_type),
        _ => None,
    }
}

fn is_logger_type(ty: &Type) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };

    let Some(segment) = type_path.path.segments.last() else {
        return false;
    };

    segment.ident == "Logger"
}
