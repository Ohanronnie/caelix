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

fn parse_guard_types(attr: &syn::Attribute) -> Vec<Type> {
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

    impl_block.attrs.retain(|attr| {
        if attr.path().is_ident("use_guard") {
            controller_guards.extend(parse_guard_types(attr));
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
                    method_guards.extend(parse_guard_types(attr));
                    return false;
                }

                true
            });

            let mut extractor_args = Vec::new();

            for input in method.sig.inputs.iter_mut() {
                if let FnArg::Typed(pat_type) = input {
                    let mut found: Option<Extractor> = None;

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

                        extractor_args.push((extractor, arg_name, arg_type));
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

                let wrapper_params = extractor_args
                    .iter()
                    .filter_map(|(extractor, name, ty)| match extractor {
                        Extractor::Param => Some(quote! { #name: actix_web::web::Path<#ty> }),
                        Extractor::Body => Some(quote! { #name: actix_web::web::Json<#ty> }),
                        Extractor::Query => Some(quote! { #name: actix_web::web::Query<#ty> }),
                        Extractor::User => None,
                    })
                    .collect::<Vec<_>>();

                let call_args = extractor_args
                    .iter()
                    .map(|(extractor, name, ty)| match extractor {
                        Extractor::Param | Extractor::Body | Extractor::Query => {
                            quote! { #name.into_inner() }
                        }
                        Extractor::User => quote! {
                            ctx.get::<#ty>()
                                .map(|value| (*value).clone())
                                .ok_or_else(|| caelix_core::UnauthorizedException::new("Not authenticated"))?
                        },
                    })
                    .collect::<Vec<_>>();

                wrappers.push(quote! {
                    async fn #wrapper_name(
                        container: actix_web::web::Data<std::sync::Arc<caelix_core::Container>>,
                        req: actix_web::HttpRequest,
                        #(#wrapper_params),*
                    ) -> actix_web::HttpResponse {
                        let headers = req
                            .headers()
                            .iter()
                            .filter_map(|(name, value)| {
                                Some((name.to_string(), value.to_str().ok()?.to_string()))
                            })
                            .collect();
                        let ctx = caelix_core::RequestContext::new(
                            req.method().to_string(),
                            req.path().to_string(),
                            headers,
                        );

                        #(
                            let guard = container.resolve::<#guard_types>();
                            match caelix_core::Guard::can_activate(&*guard, &ctx).await {
                                Ok(true) => {}
                                Ok(false) => {
                                    return caelix_actix::to_actix_response(
                                        caelix_core::IntoCaelixResponse::into_response(
                                            caelix_core::ForbiddenException::new("Access denied"),
                                        ),
                                    );
                                }
                                Err(err) => {
                                    return caelix_actix::to_actix_response(
                                        caelix_core::IntoCaelixResponse::into_response(err),
                                    );
                                }
                            }
                        )*

                        let controller = container.resolve::<#struct_type>();
                        let result: caelix_core::Result<_> = async {
                            Ok(controller.#method_name(#(#call_args),*).await?)
                        }.await;

                        match result {
                            Ok(value) => caelix_actix::to_actix_response(
                                caelix_core::IntoCaelixResponse::into_response(value),
                            ),
                            Err(err) => caelix_actix::to_actix_response(
                                caelix_core::IntoCaelixResponse::into_response(err),
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
                    caelix_core::RouteDef {
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

        impl caelix_core::Controller for #struct_type {
            fn base_path() -> &'static str { #base_path }

            fn routes() -> &'static [caelix_core::RouteDef] {
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
