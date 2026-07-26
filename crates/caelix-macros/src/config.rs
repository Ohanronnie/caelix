use proc_macro::TokenStream;
use quote::quote;
use std::collections::HashSet;
use syn::{
    Data, DeriveInput, Expr, Fields, Lit, LitStr, Meta, Path, Token, parse_macro_input,
    punctuated::Punctuated, spanned::Spanned,
};

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let mut errors = Vec::new();

    if !input.generics.params.is_empty() {
        errors.push(syn::Error::new_spanned(
            &input.generics,
            "#[derive(Config)] does not support generic structs",
        ));
    }

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                errors.push(syn::Error::new_spanned(
                    &data.fields,
                    "#[derive(Config)] requires a named-field struct",
                ));
                return compile_errors(errors);
            }
        },
        _ => {
            errors.push(syn::Error::new_spanned(
                &input,
                "#[derive(Config)] can only be used on a struct",
            ));
            return compile_errors(errors);
        }
    };

    let mut environment_names = HashSet::new();
    let mut metadata = Vec::new();
    let mut initializers = Vec::new();

    for (index, field) in fields.iter().enumerate() {
        let field_name = field.ident.as_ref().expect("named fields have identifiers");
        let field_type = &field.ty;
        let mut env = None;
        let mut default = None;
        let config_attributes: Vec<_> = field
            .attrs
            .iter()
            .filter(|attribute| attribute.path().is_ident("config"))
            .collect();

        if config_attributes.is_empty() {
            errors.push(syn::Error::new_spanned(
                field,
                "missing #[config(env = \"NAME\")] mapping",
            ));
        }
        if config_attributes.len() > 1 {
            errors.push(syn::Error::new_spanned(
                config_attributes[1],
                "duplicate #[config(...)] attribute",
            ));
        }

        for attribute in config_attributes {
            if let Err(error) = attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("env") {
                    if env.is_some() {
                        return Err(meta.error("duplicate `env` argument"));
                    }
                    env = Some(meta.value()?.parse::<LitStr>()?);
                    return Ok(());
                }
                if meta.path.is_ident("default") {
                    if default.is_some() {
                        return Err(meta.error("duplicate `default` argument"));
                    }
                    default = Some(meta.value()?.parse::<LitStr>()?);
                    return Ok(());
                }
                Err(meta.error("unknown config argument; expected `env` or `default`"))
            }) {
                errors.push(error);
            }
        }

        let Some(env) = env else {
            continue;
        };
        if !environment_names.insert(env.value()) {
            errors.push(syn::Error::new_spanned(
                &env,
                format!("duplicate environment variable mapping `{}`", env.value()),
            ));
        }

        let default_tokens = default
            .as_ref()
            .map(|value| quote! { Some(#value) })
            .unwrap_or_else(|| quote! { None });
        let is_optional = is_option(field_type);
        metadata.push(quote! {
            caelix::ConfigField {
                field: stringify!(#field_name),
                env: #env,
                default: #default_tokens,
                optional: #is_optional,
            }
        });
        let value_index = syn::Index::from(index);
        let custom_deserializer = custom_deserializer(field);
        let conversion = if let Some(path) = custom_deserializer {
            quote! {{
                let __caelix_value = &__caelix_values[#value_index];
                let __caelix_deserializer =
                    caelix::__ConfigStrDeserializer::<caelix::__ConfigValueError>::new(
                        __caelix_value.as_str(),
                    );
                #path(__caelix_deserializer).map_err(|_| {
                    caelix::__config_conversion_error(stringify!(#field_name), #env)
                })?
            }}
        } else {
            quote! {
                caelix::__deserialize_config_field::<#field_type>(
                    stringify!(#field_name),
                    #env,
                    __caelix_values[#value_index].clone(),
                )?
            }
        };
        initializers.push(quote! { #field_name: #conversion });
    }

    if !errors.is_empty() {
        return compile_errors(errors);
    }

    let expanded = quote! {
        impl caelix::Config for #name {
            fn load() -> caelix::Result<Self> {
                Self::__caelix_load_config(None)
            }

            fn load_from(
                path: impl AsRef<std::path::Path>,
            ) -> caelix::Result<Self> {
                Self::__caelix_load_config(Some(path.as_ref()))
            }
        }

        impl #name {
            fn __caelix_load_config(
                path: Option<&std::path::Path>,
            ) -> caelix::Result<Self> {
                fn __caelix_assert_deserialize<T: caelix::__DeserializeOwned>() {}
                __caelix_assert_deserialize::<Self>();
                const __CAELIX_FIELDS: &[caelix::ConfigField] = &[#(#metadata),*];
                let __caelix_values = caelix::__load_config_values(__CAELIX_FIELDS, path)?;
                let __caelix_config = Self { #(#initializers),* };
                caelix::__Validate::validate(&__caelix_config)
                    .map_err(|errors| caelix::__config_validation_error(&errors))?;
                Ok(__caelix_config)
            }
        }

        impl caelix::Injectable for #name {
            fn create(
                _container: &caelix::Container,
            ) -> caelix::BoxFuture<'_, caelix::Result<Self>> {
                Box::pin(async { <Self as caelix::Config>::load() })
            }

            fn dependencies() -> Vec<caelix::ProviderDependency> {
                Vec::new()
            }
        }
    };
    expanded.into()
}

fn custom_deserializer(field: &syn::Field) -> Option<Path> {
    for attribute in field
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("serde"))
    {
        let Meta::List(list) = &attribute.meta else {
            continue;
        };
        let Ok(items) = list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
        else {
            continue;
        };
        for item in items {
            let Meta::NameValue(name_value) = item else {
                continue;
            };
            let Expr::Lit(expression) = name_value.value else {
                continue;
            };
            let Lit::Str(path) = expression.lit else {
                continue;
            };
            if name_value.path.is_ident("deserialize_with")
                && let Ok(path) = path.parse()
            {
                return Some(path);
            }
            if name_value.path.is_ident("with")
                && let Ok(mut path) = path.parse::<Path>()
            {
                path.segments.push(syn::PathSegment::from(syn::Ident::new(
                    "deserialize",
                    path.span(),
                )));
                return Some(path);
            }
        }
    }
    None
}

fn is_option(ty: &syn::Type) -> bool {
    let syn::Type::Path(path) = ty else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "Option")
}

fn compile_errors(errors: Vec<syn::Error>) -> TokenStream {
    let errors = errors.into_iter().map(|error| error.to_compile_error());
    quote! { #(#errors)* }.into()
}
