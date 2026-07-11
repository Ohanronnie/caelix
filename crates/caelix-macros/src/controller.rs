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

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum Backend {
    Actix,
    Axum,
}

fn selected_backend() -> Backend {
    #[cfg(feature = "axum")]
    {
        Backend::Axum
    }
    #[cfg(not(feature = "axum"))]
    {
        Backend::Actix
    }
}

fn parse_type_list(attr: &syn::Attribute) -> syn::Result<Vec<Type>> {
    let list = attr.meta.require_list()?;
    Punctuated::<Type, Token![,]>::parse_terminated
        .parse2(list.tokens.clone())
        .map(|types| types.into_iter().collect())
}

pub(crate) fn expand(args: TokenStream, input: TokenStream) -> TokenStream {
    let backend = selected_backend();
    let base_path = parse_macro_input!(args as LitStr).value();
    let mut impl_block = parse_macro_input!(input as ItemImpl);
    let struct_type = impl_block.self_ty.clone();
    let mut controller_guards = Vec::new();
    let mut controller_interceptors = Vec::new();
    let mut errors = Vec::new();

    impl_block.attrs.retain(|attr| {
        if attr.path().is_ident("use_guard") {
            match parse_type_list(attr) {
                Ok(types) => controller_guards.extend(types),
                Err(err) => errors.push(err.to_compile_error()),
            }
            false
        } else if attr.path().is_ident("use_interceptor") {
            match parse_type_list(attr) {
                Ok(types) => controller_interceptors.extend(types),
                Err(err) => errors.push(err.to_compile_error()),
            }
            false
        } else {
            true
        }
    });

    let response_adapter = match backend {
        Backend::Actix => quote! { caelix::to_actix_response },
        Backend::Axum => quote! { caelix::to_axum_response },
    };
    let mut wrappers = Vec::new();
    let mut registrations = Vec::new();
    let mut routes = Vec::new();

    for item in &mut impl_block.items {
        let ImplItem::Fn(method) = item else { continue };
        let mut route: Option<(&str, String)> = None;
        let mut method_guards = Vec::new();
        let mut method_interceptors = Vec::new();

        method.attrs.retain(|attr| {
            for verb in ["get", "post", "patch", "put", "delete"] {
                if attr.path().is_ident(verb) {
                    match attr.parse_args::<LitStr>() {
                        Ok(path) => route = Some((verb, path.value())),
                        Err(err) => errors.push(err.to_compile_error()),
                    }
                    return false;
                }
            }
            if attr.path().is_ident("use_guard") {
                match parse_type_list(attr) {
                    Ok(types) => method_guards.extend(types),
                    Err(err) => errors.push(err.to_compile_error()),
                }
                false
            } else if attr.path().is_ident("use_interceptor") {
                match parse_type_list(attr) {
                    Ok(types) => method_interceptors.extend(types),
                    Err(err) => errors.push(err.to_compile_error()),
                }
                false
            } else {
                true
            }
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
                        _ => {
                            errors.push(
                                syn::Error::new_spanned(
                                    &pat_type.pat,
                                    "expected a simple identifier for extractor argument",
                                )
                                .to_compile_error(),
                            );
                            continue;
                        }
                    };
                    extractor_args.push((
                        extractor,
                        arg_name,
                        pat_type.ty.clone(),
                        needs_validation,
                    ));
                }
            }
        }

        let Some((verb, path)) = route else { continue };
        let method_name = &method.sig.ident;
        let wrapper_name = format_ident!("__{}_handler", method_name);
        let backend_verb = format_ident!("{}", verb);
        let guard_types = controller_guards
            .iter()
            .chain(method_guards.iter())
            .collect::<Vec<_>>();
        let interceptor_types = controller_interceptors
            .iter()
            .chain(method_interceptors.iter())
            .collect::<Vec<_>>();

        let mut ordered_extractors = extractor_args.iter().collect::<Vec<_>>();
        if matches!(backend, Backend::Axum) {
            ordered_extractors.sort_by_key(|(extractor, _, _, _)| match extractor {
                Extractor::Param | Extractor::Query | Extractor::User => 0,
                Extractor::Body => 1,
            });
        }
        let wrapper_params = ordered_extractors
            .iter()
            .filter_map(|(extractor, name, ty, _)| match (backend, extractor) {
                (_, Extractor::User) => None,
                (Backend::Actix, Extractor::Param) => {
                    Some(quote! { #name: caelix::__actix_web::web::Path<#ty> })
                }
                (Backend::Actix, Extractor::Body) => {
                    Some(quote! { #name: caelix::__actix_web::web::Json<#ty> })
                }
                (Backend::Actix, Extractor::Query) => {
                    Some(quote! { #name: caelix::__actix_web::web::Query<#ty> })
                }
                (Backend::Axum, Extractor::Param) => {
                    Some(quote! { #name: caelix::__axum::extract::Path<#ty> })
                }
                (Backend::Axum, Extractor::Body) => {
                    Some(quote! { #name: caelix::__axum::extract::Json<#ty> })
                }
                (Backend::Axum, Extractor::Query) => {
                    Some(quote! { #name: caelix::__axum::extract::Query<#ty> })
                }
            })
            .collect::<Vec<_>>();

        let call_args = extractor_args.iter().map(|(extractor, name, ty, needs_validation)| {
            let base = match extractor {
                Extractor::Param | Extractor::Body | Extractor::Query => match backend {
                    Backend::Actix => quote! { #name.into_inner() },
                    Backend::Axum => quote! { #name.0 },
                },
                Extractor::User => quote! {
                    request_context.get::<#ty>()?
                        .map(|value| (*value).clone())
                        .ok_or_else(|| caelix::UnauthorizedException::new("Not authenticated"))?
                },
            };
            if *needs_validation {
                quote! {{ let value = #base; caelix::validator::Validate::validate(&value)?; value }}
            } else { base }
        }).collect::<Vec<_>>();

        let interceptor_chain = interceptor_types.iter().rev().enumerate().map(|(index, interceptor_type)| {
            let interceptor_name = format_ident!("__caelix_interceptor_{index}");
            let interceptor_ref_name = format_ident!("__caelix_interceptor_ref_{index}");
            quote! {
                let #interceptor_name = match container.resolve::<#interceptor_type>() {
                    Ok(value) => value,
                    Err(err) => { caelix::log_http_exception(&err); return #response_adapter(caelix::IntoCaelixResponse::into_response(err)); }
                };
                let #interceptor_ref_name = &#interceptor_name;
                let next = caelix::Next::new(move || {
                    caelix::Interceptor::intercept(&**#interceptor_ref_name, request_context, next)
                });
            }
        }).collect::<Vec<_>>();

        let needs_request_context = !guard_types.is_empty()
            || !interceptor_types.is_empty()
            || extractor_args
                .iter()
                .any(|(extractor, _, _, _)| matches!(extractor, Extractor::User));
        let (request_headers, request_method, request_path) = match backend {
            Backend::Actix => (
                quote! { req.headers() },
                quote! { req.method().as_str() },
                quote! { req.path() },
            ),
            Backend::Axum => (
                quote! { request_info.headers() },
                quote! { request_info.method().as_str() },
                quote! { request_info.path() },
            ),
        };
        let request_context_body = quote! {
            let mut headers = std::collections::HashMap::with_capacity(#request_headers.len());
            for (name, value) in #request_headers.iter() {
                let value = match value.to_str() {
                    Ok(value) => value,
                    Err(_) => return #response_adapter(caelix::IntoCaelixResponse::into_response(
                        caelix::BadRequestException::new("invalid request header value"),
                    )),
                };
                headers.insert(name.as_str().to_string(), value.to_string());
            }
            let ctx = caelix::RequestContext::new(#request_method, #request_path, headers);
            #(
                let guard = match container.resolve::<#guard_types>() {
                    Ok(value) => value,
                    Err(err) => { caelix::log_http_exception(&err); return #response_adapter(caelix::IntoCaelixResponse::into_response(err)); }
                };
                match caelix::Guard::can_activate(&*guard, &ctx).await {
                    Ok(true) => {}
                    Ok(false) => return #response_adapter(caelix::IntoCaelixResponse::into_response(caelix::ForbiddenException::new("Access denied"))),
                    Err(err) => { caelix::log_http_exception(&err); return #response_adapter(caelix::IntoCaelixResponse::into_response(err)); }
                }
            )*
            let request_context = &ctx;
            let controller = match container.resolve::<#struct_type>() {
                Ok(value) => value,
                Err(err) => { caelix::log_http_exception(&err); return #response_adapter(caelix::IntoCaelixResponse::into_response(err)); }
            };
            let next = caelix::Next::new(move || Box::pin(async move {
                let value = controller.#method_name(#(#call_args),*).await?;
                Ok(caelix::IntoCaelixResponse::into_response(value))
            }));
            #(#interceptor_chain)*
            match next.run().await {
                Ok(value) => #response_adapter(value),
                Err(err) => { caelix::log_http_exception(&err); #response_adapter(caelix::IntoCaelixResponse::into_response(err)) }
            }
        };
        let direct_body = quote! {
            let controller = match container.resolve::<#struct_type>() {
                Ok(value) => value,
                Err(err) => { caelix::log_http_exception(&err); return #response_adapter(caelix::IntoCaelixResponse::into_response(err)); }
            };
            let result = async move {
                let value = controller.#method_name(#(#call_args),*).await?;
                Ok(caelix::IntoCaelixResponse::into_response(value))
            }.await;
            match result {
                Ok(value) => #response_adapter(value),
                Err(err) => { caelix::log_http_exception(&err); #response_adapter(caelix::IntoCaelixResponse::into_response(err)) }
            }
        };
        let wrapper_body = if needs_request_context {
            request_context_body
        } else {
            direct_body
        };

        let wrapper = match (backend, needs_request_context) {
            (Backend::Actix, true) => quote! {
                async fn #wrapper_name(
                    container: caelix::__actix_web::web::Data<caelix::Container>,
                    req: caelix::__actix_web::HttpRequest,
                    #(#wrapper_params),*
                ) -> caelix::__actix_web::HttpResponse { #wrapper_body }
            },
            (Backend::Actix, false) => quote! {
                async fn #wrapper_name(
                    container: caelix::__actix_web::web::Data<caelix::Container>,
                    #(#wrapper_params),*
                ) -> caelix::__actix_web::HttpResponse { #wrapper_body }
            },
            (Backend::Axum, true) => quote! {
                async fn #wrapper_name(
                    caelix::__axum::extract::State(container): caelix::__axum::extract::State<std::sync::Arc<caelix::Container>>,
                    request_info: caelix::AxumRequestInfo,
                    #(#wrapper_params,)*
                ) -> caelix::__axum::response::Response { #wrapper_body }
            },
            (Backend::Axum, false) => quote! {
                async fn #wrapper_name(
                    caelix::__axum::extract::State(container): caelix::__axum::extract::State<std::sync::Arc<caelix::Container>>,
                    #(#wrapper_params),*
                ) -> caelix::__axum::response::Response { #wrapper_body }
            },
        };
        wrappers.push(wrapper);

        let full_path = format!("{}{}", base_path, path);
        let display_path = full_path.replace("{", ":").replace("}", "");
        let handler_name = method_name.to_string();
        registrations.push(match backend {
            Backend::Actix => quote! { cfg.route(#full_path, caelix::__actix_web::web::#backend_verb().to(#struct_type::#wrapper_name)); },
            Backend::Axum => quote! { cfg.route(#full_path, caelix::__axum::routing::#backend_verb(#struct_type::#wrapper_name)); },
        });
        routes.push(quote! { caelix::RouteDef { method: #verb, path: #display_path, handler: #handler_name } });
    }

    let register_routes = match backend {
        Backend::Actix => quote! {
            let Some(cfg) = cfg_any.downcast_mut::<caelix::__actix_web::web::ServiceConfig>() else { return; };
            #(#registrations)*
        },
        Backend::Axum => quote! {
            let Some(cfg) = cfg_any.downcast_mut::<caelix::AxumRouterBuilder>() else { return; };
            #(#registrations)*
        },
    };
    quote! {
        #(#errors)*
        #impl_block
        impl caelix::Controller for #struct_type {
            fn base_path() -> &'static str { #base_path }
            fn routes() -> &'static [caelix::RouteDef] { &[#(#routes),*] }
            fn register_routes(cfg_any: &mut dyn std::any::Any) { #register_routes }
        }
        impl #struct_type { #(#wrappers)* }
    }
    .into()
}
