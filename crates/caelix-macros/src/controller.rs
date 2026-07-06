use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    FnArg, ImplItem, ItemImpl, LitStr, Pat, Token, Type, parse::Parser, parse_macro_input,
    punctuated::Punctuated,
};

enum Extractor {
    Param,
    Body,
    Query,
    User,
}

fn parse_type_list(attr: &syn::Attribute) -> Vec<Type> {
    Punctuated::<Type, Token![,]>::parse_terminated
        .parse2(
            attr.meta
                .require_list()
                .expect("expected guard list")
                .tokens
                .clone(),
        )
        .expect("expected one or more guard types")
        .into_iter()
        .collect()
}

pub(crate) fn expand(args: TokenStream, input: TokenStream) -> TokenStream {
    let base_path = parse_macro_input!(args as LitStr).value();
    let mut impl_block = parse_macro_input!(input as ItemImpl);
    let struct_type = impl_block.self_ty.clone();
    let mut controller_guards = Vec::new();
    let mut controller_interceptors = Vec::new();

    impl_block.attrs.retain(|attr| {
        if attr.path().is_ident("use_guard") {
            controller_guards.extend(parse_type_list(attr));
            false
        } else if attr.path().is_ident("use_interceptor") {
            controller_interceptors.extend(parse_type_list(attr));
            false
        } else {
            true
        }
    });

    let mut wrappers = Vec::new();
    let mut registrations = Vec::new();
    let mut routes = Vec::new();

    for item in &mut impl_block.items {
        if let ImplItem::Fn(method) = item {
            let mut route: Option<(&str, String)> = None;
            let mut method_guards = Vec::new();
            let mut method_interceptors = Vec::new();

            // Strip route attributes off before re-emitting, or rustc
            // complains about an unrecognized attribute in the final output
            method.attrs.retain(|attr| {
                for verb in ["get", "post", "patch", "put", "delete"] {
                    if attr.path().is_ident(verb) {
                        let path: LitStr = attr.parse_args().expect("expected a path string");
                        route = Some((verb, path.value()));
                        return false;
                    }
                }

                if attr.path().is_ident("use_guard") {
                    method_guards.extend(parse_type_list(attr));
                    return false;
                }

                if attr.path().is_ident("use_interceptor") {
                    method_interceptors.extend(parse_type_list(attr));
                    return false;
                }

                true
            });

            let mut extractor_args = Vec::new();

            for input in method.sig.inputs.iter_mut() {
                if let FnArg::Typed(pat_type) = input {
                    let mut found: Option<Extractor> = None;
                    let mut needs_validation = false;

                    pat_type.attrs.retain(|attr| {
                        if attr.path().is_ident("param") {
                            found = Some(Extractor::Param);
                            false
                        } else if attr.path().is_ident("body") {
                            found = Some(Extractor::Body);
                            false
                        } else if attr.path().is_ident("query") {
                            found = Some(Extractor::Query);
                            false
                        } else if attr.path().is_ident("user") {
                            found = Some(Extractor::User);
                            false
                        } else if attr.path().is_ident("validate") {
                            needs_validation = true;
                            false
                        } else {
                            true
                        }
                    });

                    if let Some(extractor) = found {
                        let arg_name = match &*pat_type.pat {
                            Pat::Ident(ident) => ident.ident.clone(),
                            _ => panic!("expected a simple identifier for extractor argument"),
                        };
                        let arg_type = pat_type.ty.clone();

                        extractor_args.push((extractor, arg_name, arg_type, needs_validation));
                    }
                }
            }

            if let Some((verb, path)) = route {
                let method_name = &method.sig.ident;
                let wrapper_name = format_ident!("__{}_handler", method_name);
                let actix_verb = format_ident!("{}", verb);
                let guard_types = controller_guards
                    .iter()
                    .chain(method_guards.iter())
                    .collect::<Vec<_>>();
                let interceptor_types = controller_interceptors
                    .iter()
                    .chain(method_interceptors.iter())
                    .collect::<Vec<_>>();

                let wrapper_params = extractor_args
                    .iter()
                    .filter_map(|(extractor, name, ty, _needs_validation)| match extractor {
                        Extractor::Param => Some(quote! { #name: actix_web::web::Path<#ty> }),
                        Extractor::Body => Some(quote! { #name: actix_web::web::Json<#ty> }),
                        Extractor::Query => Some(quote! { #name: actix_web::web::Query<#ty> }),
                        Extractor::User => None,
                    })
                    .collect::<Vec<_>>();

                let call_args = extractor_args
                    .iter()
                    .map(|(extractor, name, ty, needs_validation)| {
                        let base = match extractor {
                            Extractor::Param | Extractor::Body | Extractor::Query => {
                            quote! { #name.into_inner() }
                            }
                            Extractor::User => quote! {
                            request_context.get::<#ty>()
                                .map(|value| (*value).clone())
                                .ok_or_else(|| caelix::UnauthorizedException::new("Not authenticated"))?
                            },
                        };

                        if *needs_validation {
                            quote! {
                                {
                                    let value = #base;
                                    caelix::validator::Validate::validate(&value)?;
                                    value
                                }
                            }
                        } else {
                            base
                        }
                    })
                    .collect::<Vec<_>>();
                let interceptor_chain = interceptor_types
                    .iter()
                    .rev()
                    .enumerate()
                    .map(|(index, interceptor_type)| {
                        let interceptor_name = format_ident!("__caelix_interceptor_{index}");
                        let interceptor_ref_name = format_ident!("__caelix_interceptor_ref_{index}");

                        quote! {
                            let #interceptor_name = container.resolve::<#interceptor_type>();
                            let #interceptor_ref_name = &#interceptor_name;
                            let next = caelix::Next::new(move || {
                                caelix::Interceptor::intercept(&**#interceptor_ref_name, request_context, next)
                            });
                        }
                    })
                    .collect::<Vec<_>>();

                wrappers.push(quote! {
                    async fn #wrapper_name(
                        container: actix_web::web::Data<std::sync::Arc<caelix::Container>>,
                        req: actix_web::HttpRequest,
                        #(#wrapper_params),*
                    ) -> actix_web::HttpResponse {
                        let mut headers = std::collections::HashMap::new();
                        for (name, value) in req.headers().iter() {
                            let value = match value.to_str() {
                                Ok(value) => value,
                                Err(_) => {
                                    return caelix::to_actix_response(
                                        caelix::IntoCaelixResponse::into_response(
                                            caelix::BadRequestException::new("invalid request header value"),
                                        ),
                                    );
                                }
                            };

                            headers.insert(name.to_string(), value.to_string());
                        }
                        let ctx = caelix::RequestContext::new(
                            req.method().to_string(),
                            req.path().to_string(),
                            headers,
                        );

                        #(
                            let guard = container.resolve::<#guard_types>();
                            match caelix::Guard::can_activate(&*guard, &ctx).await {
                                Ok(true) => {}
                                Ok(false) => {
                                    return caelix::to_actix_response(
                                        caelix::IntoCaelixResponse::into_response(
                                            caelix::ForbiddenException::new("Access denied"),
                                        ),
                                    );
                                }
                                Err(err) => {
                                    return caelix::to_actix_response(
                                        caelix::IntoCaelixResponse::into_response(err),
                                    );
                                }
                            }
                        )*

                        let request_context = &ctx;
                        let controller = container.resolve::<#struct_type>();
                        let next = caelix::Next::new(move || {
                            Box::pin(async move {
                                let value = controller.#method_name(#(#call_args),*).await?;

                                Ok(caelix::IntoCaelixResponse::into_response(value))
                            })
                        });
                        #(#interceptor_chain)*
                        let result = next.run().await;

                        match result {
                            Ok(value) => caelix::to_actix_response(value),
                            Err(err) => caelix::to_actix_response(
                                caelix::IntoCaelixResponse::into_response(err),
                            ),
                        }
                    }
                });

                let full_path = format!("{}{}", base_path, path);
                let display_path = full_path.replace("{", ":").replace("}", "");
                let handler_name = method_name.to_string();

                registrations.push(quote! {
                    cfg.route(#full_path, actix_web::web::#actix_verb().to(#struct_type::#wrapper_name));
                });
                routes.push(quote! {
                    caelix::RouteDef {
                        method: #verb,
                        path: #display_path,
                        handler: #handler_name,
                    }
                });
            }
        }
    }

    let expanded = quote! {
        #impl_block

        impl caelix::Controller for #struct_type {
            fn base_path() -> &'static str { #base_path }

            fn routes() -> &'static [caelix::RouteDef] {
                &[
                    #(#routes),*
                ]
            }

            fn register_routes(cfg_any: &mut dyn std::any::Any) {
                let cfg = cfg_any
                    .downcast_mut::<actix_web::web::ServiceConfig>()
                    .expect("expected actix ServiceConfig");
                #(#registrations)*
            }
        }

        impl #struct_type {
            #(#wrappers)*
        }
    };
    expanded.into()
}
