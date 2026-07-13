use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, GenericArgument, PathArguments, Type, parse_macro_input};

pub(crate) fn expand(_args: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = &input.ident;
    let mut errors = Vec::new();

    let create_body = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => {
                let field_resolutions = fields.named.iter().map(|field| {
                    let Some(field_name) = field.ident.as_ref() else {
                        errors.push(
                            syn::Error::new_spanned(field, "#[injectable] fields must be named")
                                .to_compile_error(),
                        );
                        return quote! {};
                    };
                    let field_type = &field.ty;
                    let Some(resolved_type) = arc_inner_type(field_type) else {
                        errors.push(
                            syn::Error::new_spanned(
                                field_type,
                                "#[injectable] fields must be std::sync::Arc<T>",
                            )
                            .to_compile_error(),
                        );
                        return quote! {
                            #field_name: unreachable!()
                        };
                    };

                    if is_logger_type(resolved_type) {
                        return quote! {
                            #field_name: container.resolve_logger(stringify!(#struct_name))
                        };
                    }

                    quote! {
                        #field_name: container.resolve::<#resolved_type>()?
                    }
                });

                quote! {
                    Ok(Self {
                        #(#field_resolutions),*
                    })
                }
            }
            Fields::Unit => quote! { Ok(Self) },
            Fields::Unnamed(_) => {
                errors.push(
                    syn::Error::new_spanned(
                        &input,
                        "#[injectable] only supports named-field or unit structs",
                    )
                    .to_compile_error(),
                );
                quote! { Ok(Self) }
            }
        },
        _ => {
            errors.push(
                syn::Error::new_spanned(&input, "#[injectable] can only be applied to structs")
                    .to_compile_error(),
            );
            quote! { Ok(Self) }
        }
    };

    let dependencies = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => fields
                .named
                .iter()
                .filter_map(|field| {
                    let resolved = arc_inner_type(&field.ty)?;
                    (!is_logger_type(resolved))
                        .then(|| quote! { caelix::ProviderDependency::of::<#resolved>() })
                })
                .collect::<Vec<_>>(),
            _ => vec![],
        },
        _ => vec![],
    };

    let expanded = quote! {
        #(#errors)*

        #input

        impl caelix::Injectable for #struct_name {
            fn create(container: &caelix::Container) -> caelix::BoxFuture<'_, caelix::Result<Self>> {
                Box::pin(async move {
                    #create_body
                })
            }

            fn dependencies() -> Vec<caelix::ProviderDependency> {
                vec![#(#dependencies),*]
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
