use proc_macro::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::{
    Expr, FnArg, ImplItem, ItemImpl, LitStr, Meta, Pat, Token, Type, parse::Parser,
    parse_macro_input, punctuated::Punctuated, spanned::Spanned,
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

#[cfg(feature = "openapi")]
struct HeaderSpec {
    name: LitStr,
    schema: Type,
    required: bool,
    description: Option<LitStr>,
}

#[cfg(feature = "openapi")]
struct ResponseHeaderSpec {
    name: LitStr,
    schema: Type,
    description: Option<LitStr>,
}

#[cfg(feature = "openapi")]
struct ResponseSpec {
    status: Option<LitStr>,
    body: Option<Type>,
    content_type: Option<LitStr>,
    headers: Vec<ResponseHeaderSpec>,
}

#[cfg(feature = "openapi")]
fn string_value(expr: &Expr) -> syn::Result<LitStr> {
    match expr {
        Expr::Lit(value) => match &value.lit {
            syn::Lit::Str(value) => Ok(value.clone()),
            _ => Err(syn::Error::new_spanned(expr, "expected a string literal")),
        },
        _ => Err(syn::Error::new_spanned(expr, "expected a string literal")),
    }
}

#[cfg(feature = "openapi")]
fn type_value(expr: &Expr) -> syn::Result<Type> {
    syn::parse2(expr.to_token_stream())
}

#[cfg(feature = "openapi")]
fn parse_request_header(attr: &syn::Attribute) -> syn::Result<HeaderSpec> {
    let list = attr.meta.require_list()?;
    let values = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(list.tokens.clone())?;
    let mut name = None;
    let mut schema = None;
    let mut required = false;
    let mut description = None;
    for value in values {
        match value {
            Meta::Path(path) if path.is_ident("required") => required = true,
            Meta::NameValue(value) if value.path.is_ident("name") => {
                name = Some(string_value(&value.value)?)
            }
            Meta::NameValue(value) if value.path.is_ident("schema") => {
                schema = Some(type_value(&value.value)?)
            }
            Meta::NameValue(value) if value.path.is_ident("description") => {
                description = Some(string_value(&value.value)?)
            }
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "unsupported request_header argument",
                ));
            }
        }
    }
    Ok(HeaderSpec {
        name: name
            .ok_or_else(|| syn::Error::new_spanned(attr, "request_header requires `name`"))?,
        schema: schema
            .ok_or_else(|| syn::Error::new_spanned(attr, "request_header requires `schema`"))?,
        required,
        description,
    })
}

#[cfg(feature = "openapi")]
fn parse_response_headers(
    tokens: proc_macro2::TokenStream,
) -> syn::Result<Vec<ResponseHeaderSpec>> {
    let values = Punctuated::<Expr, Token![,]>::parse_terminated.parse2(tokens)?;
    values
        .into_iter()
        .map(|value| {
            let Expr::Tuple(tuple) = value else {
                return Err(syn::Error::new_spanned(
                    value,
                    "response headers must be tuples",
                ));
            };
            let tuple_span = tuple.span();
            let mut values = tuple.elems.into_iter();
            let name = values
                .next()
                .ok_or_else(|| syn::Error::new(tuple_span, "response header requires a name"))?;
            let schema = values
                .next()
                .ok_or_else(|| syn::Error::new(tuple_span, "response header requires a schema"))?;
            let description = values
                .next()
                .map(|value| string_value(&value))
                .transpose()?;
            if let Some(value) = values.next() {
                return Err(syn::Error::new_spanned(
                    value,
                    "response header accepts at most three values",
                ));
            }
            Ok(ResponseHeaderSpec {
                name: string_value(&name)?,
                schema: type_value(&schema)?,
                description,
            })
        })
        .collect()
}

#[cfg(feature = "openapi")]
fn parse_response(attr: &syn::Attribute) -> syn::Result<ResponseSpec> {
    let list = attr.meta.require_list()?;
    let values = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(list.tokens.clone())?;
    let mut spec = ResponseSpec {
        status: None,
        body: None,
        content_type: None,
        headers: Vec::new(),
    };
    for value in values {
        match value {
            Meta::Path(path) => spec.body = Some(syn::parse2(path.to_token_stream())?),
            Meta::NameValue(value) if value.path.is_ident("status") => {
                let Expr::Lit(lit) = value.value else {
                    return Err(syn::Error::new_spanned(
                        value,
                        "response status must be an integer",
                    ));
                };
                let syn::Lit::Int(status) = lit.lit else {
                    return Err(syn::Error::new_spanned(
                        lit,
                        "response status must be an integer",
                    ));
                };
                spec.status = Some(LitStr::new(&status.base10_digits(), status.span()));
            }
            Meta::NameValue(value) if value.path.is_ident("body") => {
                spec.body = Some(type_value(&value.value)?)
            }
            Meta::NameValue(value) if value.path.is_ident("content_type") => {
                spec.content_type = Some(string_value(&value.value)?)
            }
            Meta::List(value) if value.path.is_ident("headers") => {
                spec.headers = parse_response_headers(value.tokens)?
            }
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "unsupported response argument",
                ));
            }
        }
    }
    Ok(spec)
}

#[cfg(feature = "openapi")]
fn inferred_response_type(output: &syn::ReturnType) -> Option<Type> {
    let syn::ReturnType::Type(_, ty) = output else {
        return None;
    };
    let ty = ty.as_ref();
    let Type::Path(result) = ty else { return None };
    let segment = result.path.segments.last()?;
    if segment.ident != "Result" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    let inner = arguments.args.iter().find_map(|argument| match argument {
        syn::GenericArgument::Type(ty) => Some(ty.clone()),
        _ => None,
    })?;
    let Type::Path(response) = &inner else {
        return Some(inner);
    };
    let segment = response.path.segments.last()?;
    if segment.ident != "Response" {
        return Some(inner);
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Some(inner);
    };
    arguments.args.iter().find_map(|argument| match argument {
        syn::GenericArgument::Type(ty) => Some(ty.clone()),
        _ => None,
    })
}

#[cfg(feature = "openapi")]
fn method_summary(attrs: &[syn::Attribute]) -> Option<LitStr> {
    attrs.iter().find_map(|attr| {
        if !attr.path().is_ident("doc") {
            return None;
        }
        let Meta::NameValue(value) = &attr.meta else {
            return None;
        };
        let Ok(value) = string_value(&value.value) else {
            return None;
        };
        let summary = value.value().trim().to_owned();
        (!summary.is_empty()).then(|| LitStr::new(&summary, value.span()))
    })
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
    #[cfg(feature = "openapi")]
    let mut openapi_routes = Vec::new();
    #[cfg(feature = "openapi")]
    let mut openapi_document_functions = Vec::new();

    for item in &mut impl_block.items {
        let ImplItem::Fn(method) = item else { continue };
        let mut route: Option<(&str, String)> = None;
        let mut method_guards = Vec::new();
        let mut method_interceptors = Vec::new();
        #[cfg(feature = "openapi")]
        let mut documented_headers = Vec::new();
        #[cfg(feature = "openapi")]
        let mut response_spec = None;
        #[cfg(feature = "openapi")]
        let mut documented_errors = Vec::new();
        #[cfg(feature = "openapi")]
        let mut security_expressions = Vec::new();

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
            } else if attr.path().is_ident("request_header") {
                #[cfg(feature = "openapi")]
                match parse_request_header(attr) {
                    Ok(header) => documented_headers.push(header),
                    Err(err) => errors.push(err.to_compile_error()),
                }
                true
            } else if attr.path().is_ident("response") {
                #[cfg(feature = "openapi")]
                match parse_response(attr) {
                    Ok(response) => response_spec = Some(response),
                    Err(err) => errors.push(err.to_compile_error()),
                }
                true
            } else if attr.path().is_ident("errors") {
                #[cfg(feature = "openapi")]
                match parse_type_list(attr) {
                    Ok(types) => documented_errors.extend(types),
                    Err(err) => errors.push(err.to_compile_error()),
                }
                true
            } else if attr.path().is_ident("security") {
                #[cfg(feature = "openapi")]
                match attr.parse_args::<Expr>() {
                    Ok(security) => security_expressions.push(security),
                    Err(err) => errors.push(err.to_compile_error()),
                }
                true
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

        #[cfg(feature = "openapi")]
        {
            let openapi_name = format_ident!("__{}_openapi", method_name);
            openapi_document_functions.push(openapi_name.clone());
            let summary = method_summary(&method.attrs)
                .map(|summary| quote! { operation.summary = Some(#summary.to_string()); });
            let body = extractor_args.iter().find_map(|(extractor, _, ty, _)| {
                matches!(extractor, Extractor::Body).then(|| ty.clone())
            });
            let extractor_parameters = extractor_args.iter().filter_map(|(extractor, name, ty, _)| {
                let parameter_in = match extractor {
                    Extractor::Param => quote! { caelix::openapi::utoipa::openapi::path::ParameterIn::Path },
                    Extractor::Query => quote! { caelix::openapi::utoipa::openapi::path::ParameterIn::Query },
                    _ => return None,
                };
                let required = matches!(extractor, Extractor::Param);
                Some(quote! {
                    operation.parameters.get_or_insert_with(Vec::new).push(caelix::openapi::parameter(
                        stringify!(#name), #parameter_in, #required, None,
                        caelix::openapi::inline_schema::<#ty>(),
                    ));
                })
            });
            let header_parameters = documented_headers.iter().map(|header| {
                let name = &header.name;
                let schema = &header.schema;
                let required = header.required;
                let description = header.description.as_ref().map(|description| quote! { Some(#description) }).unwrap_or_else(|| quote! { None });
                quote! {
                    operation.parameters.get_or_insert_with(Vec::new).push(caelix::openapi::parameter(
                        #name, caelix::openapi::utoipa::openapi::path::ParameterIn::Header,
                        #required, #description, caelix::openapi::inline_schema::<#schema>(),
                    ));
                }
            });
            let request_body = body.map(|body| quote! {
                operation.request_body = Some(caelix::openapi::request_body(caelix::openapi::schema_ref::<#body>(openapi)));
            });
            let inferred = inferred_response_type(&method.sig.output);
            let (status, content_type, response_body, response_headers) =
                if let Some(spec) = response_spec {
                    let status = spec
                        .status
                        .unwrap_or_else(|| LitStr::new("200", method_name.span()));
                    let content_type = spec
                        .content_type
                        .unwrap_or_else(|| LitStr::new("application/json", method_name.span()));
                    (status, content_type, spec.body, spec.headers)
                } else {
                    (
                        LitStr::new("200", method_name.span()),
                        LitStr::new("application/json", method_name.span()),
                        inferred,
                        Vec::new(),
                    )
                };
            let response_schema = response_body
                .map(|body| quote! { Some(caelix::openapi::schema_ref::<#body>(openapi)) })
                .unwrap_or_else(|| quote! { None });
            let response_headers = response_headers.iter().map(|header| {
                let name = &header.name;
                let schema = &header.schema;
                let description = header
                    .description
                    .as_ref()
                    .map(|description| quote! { Some(#description.to_string()) })
                    .unwrap_or_else(|| quote! { None });
                quote! {
                    let mut header = caelix::openapi::utoipa::openapi::header::Header::new(
                        caelix::openapi::inline_schema::<#schema>(),
                    );
                    header.description = #description;
                    response.headers.insert(#name.to_string(), header);
                }
            });
            let error_responses = documented_errors.iter().map(|error| {
                quote! {
                    let (status, response) = caelix::openapi::error_response::<#error>(openapi);
                    operation.responses.responses.insert(status, response.into());
                }
            });
            let route_security = quote! {
                caelix::openapi::apply_security(&mut operation, &[#(#security_expressions),*]);
            };
            let full_path = full_path.clone();
            openapi_routes.push(quote! {
                fn #openapi_name(openapi: &mut caelix::openapi::utoipa::openapi::OpenApi) {
                    let mut operation = caelix::openapi::utoipa::openapi::path::Operation::new();
                    operation.operation_id = Some(#handler_name.to_string());
                    #summary
                    #route_security
                    #(#extractor_parameters)*
                    #(#header_parameters)*
                    #request_body
                    let mut response = caelix::openapi::response(Some(#content_type), #response_schema);
                    #(#response_headers)*
                    operation.responses.responses.insert(#status.to_string(), response.into());
                    #(#error_responses)*
                    caelix::openapi::operation(#verb, #full_path, operation, openapi);
                }
            });
        }
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
    #[cfg(feature = "openapi")]
    let openapi_controller_methods = quote! {
        fn openapi_routes() -> &'static [caelix::openapi::OpenApiRouteDef] {
            &[#(caelix::openapi::OpenApiRouteDef { document: #struct_type::#openapi_document_functions }),*]
        }
    };
    #[cfg(not(feature = "openapi"))]
    let openapi_controller_methods = quote! {};
    quote! {
        #(#errors)*
        #impl_block
        impl caelix::Controller for #struct_type {
            fn base_path() -> &'static str { #base_path }
            fn routes() -> &'static [caelix::RouteDef] { &[#(#routes),*] }
            fn register_routes(cfg_any: &mut dyn std::any::Any) { #register_routes }
            #openapi_controller_methods
        }
        impl #struct_type { #(#wrappers)* #(#openapi_routes)* }
    }
    .into()
}
