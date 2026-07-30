use proc_macro::TokenStream;
#[cfg(feature = "openapi")]
use quote::ToTokens;
use quote::{format_ident, quote};
#[cfg(feature = "uploads")]
use std::collections::HashSet;
use syn::Meta;
#[cfg(feature = "openapi")]
use syn::spanned::Spanned;
use syn::{
    Expr, FnArg, ImplItem, ItemImpl, LitStr, Pat, Token, Type, parse::Parser, parse_macro_input,
    punctuated::Punctuated,
};

enum Extractor {
    Param,
    Body,
    Query,
    User,
    Cookie(LitStr),
    Multipart,
    File,
    Files,
}

#[derive(Clone, Copy)]
enum ThrottleAnnotation {
    Policy(u64, u64),
    Skip,
}

fn stable_identifier_hash(value: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    value
        .as_bytes()
        .iter()
        .fold(FNV_OFFSET_BASIS, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
        })
}

fn parse_throttle(attr: &syn::Attribute) -> syn::Result<ThrottleAnnotation> {
    let list = attr.meta.require_list()?;
    let values = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(list.tokens.clone())?;
    let mut limit = None;
    let mut window = None;
    for value in values {
        let Meta::NameValue(value) = value else {
            return Err(syn::Error::new_spanned(
                value,
                "throttle arguments must use `limit` and `window_seconds`",
            ));
        };
        let target = if value.path.is_ident("limit") {
            &mut limit
        } else if value.path.is_ident("window_seconds") {
            &mut window
        } else {
            return Err(syn::Error::new_spanned(
                value,
                "throttle arguments must use `limit` and `window_seconds`",
            ));
        };
        if target.is_some() {
            return Err(syn::Error::new_spanned(
                value,
                "duplicate throttle argument",
            ));
        }
        let Expr::Lit(expression) = &value.value else {
            return Err(syn::Error::new_spanned(
                &value.value,
                "throttle arguments must be positive integer literals",
            ));
        };
        let syn::Lit::Int(integer) = &expression.lit else {
            return Err(syn::Error::new_spanned(
                expression,
                "throttle arguments must be positive integer literals",
            ));
        };
        let parsed = integer.base10_parse::<u64>()?;
        if parsed == 0 {
            return Err(syn::Error::new_spanned(
                integer,
                "throttle arguments must be greater than zero",
            ));
        }
        *target = Some(parsed);
    }
    Ok(ThrottleAnnotation::Policy(
        limit.ok_or_else(|| syn::Error::new_spanned(attr, "missing throttle `limit`"))?,
        window.ok_or_else(|| syn::Error::new_spanned(attr, "missing throttle `window_seconds`"))?,
    ))
}

pub(crate) fn expand_controller_throttle_marker(
    args: TokenStream,
    input: TokenStream,
    skip: bool,
) -> TokenStream {
    let mut item = parse_macro_input!(input as ItemImpl);
    let args = proc_macro2::TokenStream::from(args);
    let annotation = if skip {
        if !args.is_empty() {
            return syn::Error::new_spanned(args, "skip_throttle does not accept arguments")
                .to_compile_error()
                .into();
        }
        ThrottleAnnotation::Skip
    } else {
        let attr: syn::Attribute = syn::parse_quote!(#[throttle(#args)]);
        match parse_throttle(&attr) {
            Ok(annotation) => annotation,
            Err(error) => return error.to_compile_error().into(),
        }
    };
    let marker = match annotation {
        ThrottleAnnotation::Policy(limit, window) => {
            format!("__caelix_throttle:{limit}:{window}")
        }
        ThrottleAnnotation::Skip => "__caelix_skip_throttle".to_string(),
    };
    item.attrs.push(syn::parse_quote!(#[doc = #marker]));
    quote!(#item).into()
}

fn throttle_doc_marker(attr: &syn::Attribute) -> Option<ThrottleAnnotation> {
    if !attr.path().is_ident("doc") {
        return None;
    }
    let Meta::NameValue(value) = &attr.meta else {
        return None;
    };
    let Expr::Lit(expression) = &value.value else {
        return None;
    };
    let syn::Lit::Str(value) = &expression.lit else {
        return None;
    };
    if value.value() == "__caelix_skip_throttle" {
        return Some(ThrottleAnnotation::Skip);
    }
    let raw = value.value();
    let mut parts = raw.strip_prefix("__caelix_throttle:")?.split(':');
    let limit = parts.next()?.parse().ok()?;
    let window = parts.next()?.parse().ok()?;
    parts
        .next()
        .is_none()
        .then_some(ThrottleAnnotation::Policy(limit, window))
}

fn attr_named(attr: &syn::Attribute, name: &str) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|segment| segment.ident == name)
}

#[cfg(feature = "uploads")]
struct UploadFieldOptions {
    name: Option<LitStr>,
    max_size: Option<u64>,
    content_types: Vec<LitStr>,
    trust_content_type_header: bool,
    validator: Option<syn::Ident>,
}

#[allow(dead_code)]
struct UploadField {
    name: LitStr,
    max_size: Option<u64>,
    content_types: Vec<LitStr>,
    trust_content_type_header: bool,
    validator: Option<syn::Ident>,
}

#[cfg(feature = "uploads")]
fn parse_upload_field_options(attr: &syn::Attribute) -> syn::Result<UploadFieldOptions> {
    if matches!(attr.meta, Meta::Path(_)) {
        return Ok(UploadFieldOptions {
            name: None,
            max_size: None,
            content_types: Vec::new(),
            trust_content_type_header: false,
            validator: None,
        });
    }
    let list = attr.meta.require_list()?;
    let values = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(list.tokens.clone())?;
    let mut options = UploadFieldOptions {
        name: None,
        max_size: None,
        content_types: Vec::new(),
        trust_content_type_header: false,
        validator: None,
    };

    for value in values {
        let Meta::NameValue(value) = value else {
            return Err(syn::Error::new_spanned(
                value,
                "file extractor arguments must use `name`, `max_size`, `content_type`, `trust_content_type_header`, or `validate`",
            ));
        };
        if value.path.is_ident("name") {
            if options.name.is_some() {
                return Err(syn::Error::new_spanned(
                    value,
                    "duplicate file extractor `name`",
                ));
            }
            options.name = Some(string_lit(&value.value)?);
        } else if value.path.is_ident("max_size") {
            if options.max_size.is_some() {
                return Err(syn::Error::new_spanned(
                    value,
                    "duplicate file extractor `max_size`",
                ));
            }
            options.max_size = Some(parse_upload_size(&string_lit(&value.value)?)?);
        } else if value.path.is_ident("content_type") {
            if !options.content_types.is_empty() {
                return Err(syn::Error::new_spanned(
                    value,
                    "duplicate file extractor `content_type`",
                ));
            }
            options.content_types = parse_content_types(&string_lit(&value.value)?)?;
        } else if value.path.is_ident("trust_content_type_header") {
            if options.trust_content_type_header {
                return Err(syn::Error::new_spanned(
                    value,
                    "duplicate file extractor `trust_content_type_header`",
                ));
            }
            let Expr::Lit(literal) = &value.value else {
                return Err(syn::Error::new_spanned(
                    &value.value,
                    "trust_content_type_header must be `true`",
                ));
            };
            let syn::Lit::Bool(value) = &literal.lit else {
                return Err(syn::Error::new_spanned(
                    literal,
                    "trust_content_type_header must be `true`",
                ));
            };
            if !value.value {
                return Err(syn::Error::new_spanned(
                    value,
                    "trust_content_type_header must be `true`",
                ));
            }
            options.trust_content_type_header = true;
        } else if value.path.is_ident("validate") {
            if options.validator.is_some() {
                return Err(syn::Error::new_spanned(
                    value,
                    "duplicate file extractor `validate`",
                ));
            }
            let Expr::Path(path) = &value.value else {
                return Err(syn::Error::new_spanned(
                    &value.value,
                    "file extractor `validate` must name a controller method",
                ));
            };
            let Some(validator) = path.path.get_ident() else {
                return Err(syn::Error::new_spanned(
                    path,
                    "file extractor `validate` must name a controller method",
                ));
            };
            options.validator = Some(validator.clone());
        } else {
            return Err(syn::Error::new_spanned(
                value,
                "file extractor arguments must use `name`, `max_size`, `content_type`, `trust_content_type_header`, or `validate`",
            ));
        }
    }

    if options.trust_content_type_header && options.content_types.is_empty() {
        return Err(syn::Error::new_spanned(
            attr,
            "trust_content_type_header requires `content_type = \"...\"`",
        ));
    }

    Ok(options)
}

#[cfg(feature = "uploads")]
fn parse_upload_size(value: &LitStr) -> syn::Result<u64> {
    const UNITS: [(&str, u64); 7] = [
        ("KiB", 1024),
        ("MiB", 1024 * 1024),
        ("GiB", 1024 * 1024 * 1024),
        ("KB", 1000),
        ("MB", 1000 * 1000),
        ("GB", 1000 * 1000 * 1000),
        ("B", 1),
    ];
    let raw = value.value();
    let Some((digits, multiplier)) = UNITS.iter().find_map(|(suffix, multiplier)| {
        raw.strip_suffix(suffix).map(|digits| (digits, multiplier))
    }) else {
        return Err(syn::Error::new_spanned(
            value,
            "max_size must be a whole-number value ending in B, KB, MB, GB, KiB, MiB, or GiB",
        ));
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(syn::Error::new_spanned(
            value,
            "max_size must be a whole-number value ending in B, KB, MB, GB, KiB, MiB, or GiB",
        ));
    }
    let bytes = digits
        .parse::<u64>()
        .ok()
        .and_then(|bytes| bytes.checked_mul(*multiplier));
    bytes.ok_or_else(|| {
        syn::Error::new_spanned(value, "max_size exceeds the maximum supported byte limit")
    })
}

#[cfg(feature = "uploads")]
fn parse_content_types(value: &LitStr) -> syn::Result<Vec<LitStr>> {
    let mut content_types = Vec::new();
    for content_type in value.value().split(',') {
        let normalized = content_type.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err(syn::Error::new_spanned(
                value,
                "content_type must not contain empty MIME type entries",
            ));
        }
        if !content_types
            .iter()
            .any(|existing: &LitStr| existing.value() == normalized)
        {
            content_types.push(LitStr::new(&normalized, value.span()));
        }
    }
    Ok(content_types)
}

#[cfg(feature = "uploads")]
fn upload_validator_error(validator: &syn::Ident) -> syn::Error {
    syn::Error::new_spanned(
        validator,
        format!(
            "upload validator `{validator}` must be declared as `async fn {validator}(&self, file: &UploadedFile) -> Result<()>`"
        ),
    )
}

#[cfg(feature = "uploads")]
fn is_upload_validator_signature(method: &syn::ImplItemFn) -> bool {
    if method.sig.asyncness.is_none() || method.sig.inputs.len() != 2 {
        return false;
    }
    let Some(FnArg::Receiver(receiver)) = method.sig.inputs.first() else {
        return false;
    };
    if !matches!(receiver.kind, syn::ReceiverKind::Reference(_, _, None)) {
        return false;
    }
    let Some(FnArg::Typed(file)) = method.sig.inputs.iter().nth(1) else {
        return false;
    };
    let Type::Reference(file) = file.ty.as_ref() else {
        return false;
    };
    if file.mutability.is_some() || !is_named_type(file.elem.as_ref(), "UploadedFile") {
        return false;
    }
    let syn::ReturnType::Type(_, output) = &method.sig.output else {
        return false;
    };
    let Type::Path(result) = output.as_ref() else {
        return false;
    };
    let Some(segment) = result.path.segments.last() else {
        return false;
    };
    if segment.ident != "Result" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return false;
    };
    arguments.args.len() == 1
        && matches!(
            arguments.args.first(),
            Some(syn::GenericArgument::Type(Type::Tuple(unit))) if unit.elems.is_empty()
        )
}

#[cfg(feature = "uploads")]
fn validate_upload_validator(impl_block: &ItemImpl, validator: &syn::Ident) -> syn::Result<()> {
    let Some(method) = impl_block.items.iter().find_map(|item| match item {
        ImplItem::Fn(method) if method.sig.ident == *validator => Some(method),
        _ => None,
    }) else {
        return Err(syn::Error::new_spanned(
            validator,
            format!("upload validator `{validator}` was not found on this controller"),
        ));
    };
    if !is_upload_validator_signature(method) {
        return Err(upload_validator_error(validator));
    }
    Ok(())
}

#[cfg(feature = "uploads")]
fn upload_validation(upload: &UploadField, file: &syn::Ident) -> proc_macro2::TokenStream {
    let max_size = upload.max_size.map(|max_size| {
        quote! { #file.validate_max_size(#max_size)?; }
    });
    let content_type = (!upload.content_types.is_empty()).then(|| {
        let content_types = &upload.content_types;
        let trust_content_type_header = upload.trust_content_type_header;
        quote! {
            #file.validate_content_type(&[#(#content_types),*], #trust_content_type_header).await?;
        }
    });
    let validator = upload.validator.as_ref().map(|validator| {
        quote! { controller.#validator(&#file).await?; }
    });
    quote! {
        #max_size
        #content_type
        #validator
    }
}

#[cfg(feature = "uploads")]
fn string_lit(expr: &Expr) -> syn::Result<LitStr> {
    match expr {
        Expr::Lit(value) => match &value.lit {
            syn::Lit::Str(value) => Ok(value.clone()),
            _ => Err(syn::Error::new_spanned(expr, "expected a string literal")),
        },
        _ => Err(syn::Error::new_spanned(expr, "expected a string literal")),
    }
}

fn is_named_type(ty: &Type, name: &str) -> bool {
    matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|segment| segment.ident == name))
}

fn is_option_of(ty: &Type, name: &str) -> bool {
    let Type::Path(path) = ty else { return false };
    let Some(segment) = path.path.segments.last() else {
        return false;
    };
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return false;
    };
    segment.ident == "Option"
        && arguments.args.iter().any(
            |argument| matches!(argument, syn::GenericArgument::Type(inner) if is_named_type(inner, name)),
        )
}

fn is_option_uploaded_file(ty: &Type) -> bool {
    let Type::Path(path) = ty else { return false };
    let Some(segment) = path.path.segments.last() else {
        return false;
    };
    if segment.ident != "Option" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return false;
    };
    arguments.args.iter().any(|argument| matches!(argument, syn::GenericArgument::Type(inner) if is_named_type(inner, "UploadedFile")))
}

fn is_vec_uploaded_file(ty: &Type) -> bool {
    let Type::Path(path) = ty else { return false };
    let Some(segment) = path.path.segments.last() else {
        return false;
    };
    if segment.ident != "Vec" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return false;
    };
    arguments.args.iter().any(|argument| matches!(argument, syn::GenericArgument::Type(inner) if is_named_type(inner, "UploadedFile")))
}

fn payload_slot_ident(name: &syn::Ident) -> syn::Ident {
    let name = name.to_string();
    format_ident!("__caelix_payload_{}", name.trim_start_matches('_'))
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
    let controller_ident = match struct_type.as_ref() {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.clone())
            .unwrap_or_else(|| format_ident!("Controller")),
        _ => format_ident!("Controller"),
    };
    let mut controller_guards = Vec::new();
    let mut controller_interceptors = Vec::new();
    let mut controller_throttle = None;
    let mut errors = Vec::new();
    #[cfg(feature = "uploads")]
    let mut invalid_upload_validators = HashSet::new();

    impl_block.attrs.retain(|attr| {
        if let Some(value) = throttle_doc_marker(attr) {
            if controller_throttle.is_some() {
                errors.push(
                    syn::Error::new_spanned(attr, "conflicting controller throttle settings")
                        .to_compile_error(),
                );
            } else {
                controller_throttle = Some(value);
            }
            false
        } else if attr.path().is_ident("use_guard") {
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
        } else if attr_named(attr, "throttle") {
            if controller_throttle.is_some() {
                errors.push(
                    syn::Error::new_spanned(attr, "conflicting controller throttle settings")
                        .to_compile_error(),
                );
            } else {
                match parse_throttle(attr) {
                    Ok(value) => controller_throttle = Some(value),
                    Err(err) => errors.push(err.to_compile_error()),
                }
            }
            false
        } else if attr_named(attr, "skip_throttle") {
            if !matches!(attr.meta, Meta::Path(_)) {
                errors.push(
                    syn::Error::new_spanned(attr, "skip_throttle does not accept arguments")
                        .to_compile_error(),
                );
            }
            if controller_throttle.is_some() {
                errors.push(
                    syn::Error::new_spanned(attr, "conflicting controller throttle settings")
                        .to_compile_error(),
                );
            } else {
                controller_throttle = Some(ThrottleAnnotation::Skip);
            }
            false
        } else {
            true
        }
    });

    #[cfg(feature = "uploads")]
    for item in &impl_block.items {
        let ImplItem::Fn(method) = item else { continue };
        for input in &method.sig.inputs {
            let FnArg::Typed(argument) = input else {
                continue;
            };
            for attribute in &argument.attrs {
                if !(attribute.path().is_ident("file") || attribute.path().is_ident("files")) {
                    continue;
                }
                if let Ok(options) = parse_upload_field_options(attribute)
                    && let Some(validator) = options.validator
                    && let Err(error) = validate_upload_validator(&impl_block, &validator)
                {
                    invalid_upload_validators.insert(validator.to_string());
                    errors.push(error.to_compile_error());
                }
            }
        }
    }

    let response_adapter = match backend {
        Backend::Actix => quote! { caelix::to_actix_response },
        Backend::Axum => quote! { caelix::to_axum_response },
    };
    let mut wrappers = Vec::new();
    let mut registrations = Vec::new();
    let mut container_registrations = Vec::new();
    let mut route_states = Vec::new();
    let mut routes = Vec::new();
    let mut route_dependencies = Vec::new();
    let mut route_throttle_policies = Vec::new();
    #[cfg(feature = "openapi")]
    let mut openapi_routes = Vec::new();
    #[cfg(feature = "openapi")]
    let mut openapi_document_functions = Vec::new();

    for item in &mut impl_block.items {
        let ImplItem::Fn(method) = item else { continue };
        let mut route: Option<(&str, String)> = None;
        let mut method_guards = Vec::new();
        let mut method_interceptors = Vec::new();
        let mut upload_limit: Option<Expr> = None;
        let mut method_throttle = None;
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
            } else if attr.path().is_ident("upload") {
                #[cfg(not(feature = "uploads"))]
                {
                    errors.push(
                        syn::Error::new_spanned(
                            attr,
                            "multipart upload support requires the `uploads` feature",
                        )
                        .to_compile_error(),
                    );
                }
                let parsed = attr.meta.require_list().and_then(|list| {
                    Punctuated::<Meta, Token![,]>::parse_terminated.parse2(list.tokens.clone())
                });
                match parsed {
                    Ok(values) if values.len() == 1 => match values.first() {
                        Some(Meta::NameValue(value)) if value.path.is_ident("limit") => {
                            upload_limit = Some(value.value.clone());
                        }
                        _ => errors.push(
                            syn::Error::new_spanned(attr, "upload requires `limit = ...`")
                                .to_compile_error(),
                        ),
                    },
                    _ => errors.push(
                        syn::Error::new_spanned(attr, "upload requires `limit = ...`")
                            .to_compile_error(),
                    ),
                }
                false
            } else if attr_named(attr, "throttle") {
                if method_throttle.is_some() {
                    errors.push(
                        syn::Error::new_spanned(attr, "conflicting method throttle settings")
                            .to_compile_error(),
                    );
                } else {
                    match parse_throttle(attr) {
                        Ok(value) => method_throttle = Some(value),
                        Err(err) => errors.push(err.to_compile_error()),
                    }
                }
                false
            } else if attr_named(attr, "skip_throttle") {
                if !matches!(attr.meta, Meta::Path(_)) {
                    errors.push(
                        syn::Error::new_spanned(attr, "skip_throttle does not accept arguments")
                            .to_compile_error(),
                    );
                }
                if method_throttle.is_some() {
                    errors.push(
                        syn::Error::new_spanned(attr, "conflicting method throttle settings")
                            .to_compile_error(),
                    );
                } else {
                    method_throttle = Some(ThrottleAnnotation::Skip);
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
            } else {
                if attr.path().is_ident("security") {
                    #[cfg(feature = "openapi")]
                    match attr.parse_args::<Expr>() {
                        Ok(security) => security_expressions.push(security),
                        Err(err) => errors.push(err.to_compile_error()),
                    }
                }
                true
            }
        });

        let mut extractor_args = Vec::new();
        for input in method.sig.inputs.iter_mut() {
            if let FnArg::Typed(pat_type) = input {
                let mut found: Option<Extractor> = None;
                let mut needs_validation = false;
                #[cfg(feature = "uploads")]
                let mut upload_options = None;
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
                    } else if attr.path().is_ident("cookie") {
                        match attr.parse_args::<LitStr>() {
                            Ok(name) if !name.value().is_empty() => {
                                found = Some(Extractor::Cookie(name));
                            }
                            Ok(name) => errors.push(
                                syn::Error::new_spanned(name, "cookie name must not be empty")
                                    .to_compile_error(),
                            ),
                            Err(_) => errors.push(
                                syn::Error::new_spanned(
                                    attr,
                                    "#[cookie] requires a non-empty string literal, for example #[cookie(\"session\")]",
                                )
                                .to_compile_error(),
                            ),
                        }
                        false
                    } else if attr.path().is_ident("multipart") {
                        found = Some(Extractor::Multipart);
                        #[cfg(not(feature = "uploads"))]
                        {
                            errors.push(
                                syn::Error::new_spanned(
                                    attr,
                                    "multipart upload support requires the `uploads` feature",
                                )
                                .to_compile_error(),
                            );
                        }
                        false
                    } else if attr.path().is_ident("file") {
                        found = Some(Extractor::File);
                        #[cfg(feature = "uploads")]
                        match parse_upload_field_options(attr) {
                            Ok(options) => upload_options = Some(options),
                            Err(err) => errors.push(err.to_compile_error()),
                        }
                        #[cfg(not(feature = "uploads"))]
                        {
                            errors.push(
                                syn::Error::new_spanned(
                                    attr,
                                    "multipart upload support requires the `uploads` feature",
                                )
                                .to_compile_error(),
                            );
                        }
                        false
                    } else if attr.path().is_ident("files") {
                        found = Some(Extractor::Files);
                        #[cfg(feature = "uploads")]
                        match parse_upload_field_options(attr) {
                            Ok(options) => upload_options = Some(options),
                            Err(err) => errors.push(err.to_compile_error()),
                        }
                        #[cfg(not(feature = "uploads"))]
                        {
                            errors.push(
                                syn::Error::new_spanned(
                                    attr,
                                    "multipart upload support requires the `uploads` feature",
                                )
                                .to_compile_error(),
                            );
                        }
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
                    if matches!(extractor, Extractor::Cookie(_))
                        && !(is_named_type(&pat_type.ty, "String")
                            || is_option_of(&pat_type.ty, "String"))
                    {
                        errors.push(
                            syn::Error::new_spanned(
                                &pat_type.ty,
                                "#[cookie] requires String or Option<String>",
                            )
                            .to_compile_error(),
                        );
                    }
                    if matches!(extractor, Extractor::File)
                        && !(is_named_type(&pat_type.ty, "UploadedFile")
                            || is_option_uploaded_file(&pat_type.ty))
                    {
                        errors.push(
                            syn::Error::new_spanned(
                                &pat_type.ty,
                                "#[file] requires UploadedFile or Option<UploadedFile>",
                            )
                            .to_compile_error(),
                        );
                    }
                    if matches!(extractor, Extractor::Files) && !is_vec_uploaded_file(&pat_type.ty)
                    {
                        errors.push(
                            syn::Error::new_spanned(
                                &pat_type.ty,
                                "#[files] requires Vec<UploadedFile>",
                            )
                            .to_compile_error(),
                        );
                    }
                    #[cfg(feature = "uploads")]
                    let upload = if matches!(extractor, Extractor::File | Extractor::Files) {
                        let options = upload_options.unwrap_or(UploadFieldOptions {
                            name: None,
                            max_size: None,
                            content_types: Vec::new(),
                            trust_content_type_header: false,
                            validator: None,
                        });
                        let mut upload = UploadField {
                            name: options.name.unwrap_or_else(|| {
                                LitStr::new(&arg_name.to_string(), arg_name.span())
                            }),
                            max_size: options.max_size,
                            content_types: options.content_types,
                            trust_content_type_header: options.trust_content_type_header,
                            validator: options.validator,
                        };
                        if let Some(validator) = &upload.validator
                            && invalid_upload_validators.contains(&validator.to_string())
                        {
                            upload.validator = None;
                        }
                        Some(upload)
                    } else {
                        None
                    };
                    #[cfg(not(feature = "uploads"))]
                    let upload: Option<UploadField> = None;
                    extractor_args.push((
                        extractor,
                        arg_name,
                        pat_type.ty.clone(),
                        needs_validation,
                        upload,
                    ));
                }
            }
        }

        let Some((verb, path)) = route else { continue };
        let method_name = &method.sig.ident;
        let effective_throttle = method_throttle.or(controller_throttle);
        let explicit_throttle =
            matches!(effective_throttle, Some(ThrottleAnnotation::Policy(_, _)));
        if let Some(ThrottleAnnotation::Policy(limit, window)) = effective_throttle {
            route_throttle_policies.push((limit, window));
        }
        if explicit_throttle {
            route_dependencies.push(syn::parse_quote!(caelix::Throttle));
        }
        let throttle_policy = match effective_throttle {
            Some(ThrottleAnnotation::Policy(limit, window)) => quote! {
                Some(caelix::ThrottlePolicy::new(#limit, #window))
            },
            Some(ThrottleAnnotation::Skip) => quote! { None },
            None => quote! { state.throttle.as_ref().map(|service| service.policy()) },
        };
        let throttle_enabled = !matches!(effective_throttle, Some(ThrottleAnnotation::Skip));
        #[cfg(feature = "openapi")]
        let documented_throttle = match effective_throttle {
            Some(ThrottleAnnotation::Policy(_, _)) => quote! { true },
            Some(ThrottleAnnotation::Skip) => quote! { false },
            None => quote! { __caelix_global_throttle },
        };
        let full_path = format!("{}{}", base_path, path);
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
        route_dependencies.extend(guard_types.iter().map(|guard| (*guard).clone()));
        route_dependencies.extend(
            interceptor_types
                .iter()
                .map(|interceptor| (*interceptor).clone()),
        );

        let mut ordered_extractors = extractor_args.iter().collect::<Vec<_>>();
        if matches!(backend, Backend::Axum) {
            ordered_extractors.sort_by_key(|(extractor, _, _, _, _)| match extractor {
                Extractor::Param | Extractor::Query | Extractor::User | Extractor::Cookie(_) => 0,
                Extractor::Body | Extractor::Multipart | Extractor::File | Extractor::Files => 1,
            });
        }
        let has_payload = extractor_args.iter().any(|(extractor, _, _, _, _)| {
            matches!(
                extractor,
                Extractor::Body | Extractor::Multipart | Extractor::File | Extractor::Files
            )
        });
        let multipart_count = extractor_args
            .iter()
            .filter(|(extractor, _, _, _, _)| matches!(extractor, Extractor::Multipart))
            .count();
        if multipart_count > 0
            && extractor_args.iter().any(|(extractor, _, _, _, _)| {
                matches!(
                    extractor,
                    Extractor::Body | Extractor::File | Extractor::Files
                )
            })
        {
            errors.push(
                syn::Error::new_spanned(
                    &method.sig,
                    "#[multipart] cannot be combined with #[body], #[file], or #[files]",
                )
                .to_compile_error(),
            );
        }
        let wrapper_params = ordered_extractors
            .iter()
            .filter_map(|(extractor, name, ty, _, _)| match (backend, extractor) {
                (_, Extractor::User | Extractor::Cookie(_)) => None,
                (Backend::Actix, Extractor::Param) => {
                    Some(quote! { #name: caelix::__actix_web::web::Path<#ty> })
                }
                (
                    Backend::Actix,
                    Extractor::Body | Extractor::Multipart | Extractor::File | Extractor::Files,
                ) => None,
                (Backend::Actix, Extractor::Query) => {
                    Some(quote! { #name: caelix::__actix_web::web::Query<#ty> })
                }
                (Backend::Axum, Extractor::Param) => {
                    Some(quote! { #name: caelix::__axum::extract::Path<#ty> })
                }
                (
                    Backend::Axum,
                    Extractor::Body | Extractor::Multipart | Extractor::File | Extractor::Files,
                ) => None,
                (Backend::Axum, Extractor::Query) => {
                    Some(quote! { #name: caelix::__axum::extract::Query<#ty> })
                }
            })
            .collect::<Vec<_>>();
        let mut wrapper_params = wrapper_params;
        let mut wrapper_arg_names = ordered_extractors
            .iter()
            .filter_map(|(extractor, name, _, _, _)| match (backend, extractor) {
                (_, Extractor::User | Extractor::Cookie(_)) => None,
                (Backend::Actix, Extractor::Param | Extractor::Query) => Some((*name).clone()),
                (
                    Backend::Actix,
                    Extractor::Body | Extractor::Multipart | Extractor::File | Extractor::Files,
                ) => None,
                (Backend::Axum, Extractor::Param | Extractor::Query) => Some((*name).clone()),
                (
                    Backend::Axum,
                    Extractor::Body | Extractor::Multipart | Extractor::File | Extractor::Files,
                ) => None,
            })
            .collect::<Vec<_>>();
        if has_payload && matches!(backend, Backend::Actix) {
            wrapper_params.push(quote! { __caelix_payload: caelix::RawRequestPayload });
            wrapper_arg_names.push(format_ident!("__caelix_payload"));
        }

        let body_extractors = extractor_args
            .iter()
            .filter(|(extractor, _, _, _, _)| matches!(extractor, Extractor::Body))
            .collect::<Vec<_>>();
        if body_extractors.len() > 1 {
            errors.push(
                syn::Error::new_spanned(&method.sig, "a route may have only one #[body] argument")
                    .to_compile_error(),
            );
        }
        #[cfg(feature = "uploads")]
        let force_multipart = extractor_args.iter().any(|(extractor, _, ty, _, _)| {
            matches!(extractor, Extractor::Multipart | Extractor::Files)
                || (matches!(extractor, Extractor::File) && !is_option_uploaded_file(ty))
        });
        #[cfg(feature = "uploads")]
        let route_limit = upload_limit
            .as_ref()
            .map(|limit| quote! { Some((#limit) as usize) })
            .unwrap_or_else(|| quote! { None });
        let payload_slots = extractor_args
            .iter()
            .filter(|(extractor, _, _, _, _)| {
                matches!(
                    extractor,
                    Extractor::Body | Extractor::Multipart | Extractor::File | Extractor::Files
                )
            })
            .map(|(_, name, ty, _, _)| {
                let slot = payload_slot_ident(name);
                quote! { let mut #slot: Option<#ty> = None; }
            })
            .collect::<Vec<_>>();
        #[cfg(feature = "uploads")]
        let multipart_assignments = extractor_args.iter().filter_map(|(extractor, name, ty, _, upload)| {
            let slot = payload_slot_ident(name);
            match extractor {
            Extractor::Body => Some(quote! { #slot = Some(__caelix_form.deserialize::<#ty>()?); }),
            Extractor::File => {
                let upload = upload.as_ref().expect("file fields have options");
                let field_name = &upload.name;
                let file = format_ident!("__caelix_extracted_{}", name);
                let validation = upload_validation(upload, &file);
                if is_option_uploaded_file(ty) {
                    Some(quote! {
                        let #file = __caelix_form.take_file(#field_name)?;
                        if let Some(#file) = #file.as_ref() {
                            #validation
                        }
                        #slot = Some(#file);
                    })
                } else {
                    Some(quote! {
                        let #file = __caelix_form.take_file(#field_name)?
                            .ok_or_else(|| caelix::BadRequestException::new(format!("missing required file field `{}`", #field_name)))?;
                        #validation
                        #slot = Some(#file);
                    })
                }
            }
            Extractor::Files => {
                let upload = upload.as_ref().expect("file fields have options");
                let field_name = &upload.name;
                let files = format_ident!("__caelix_extracted_{}", name);
                let file = format_ident!("__caelix_file_{}", name);
                let validation = upload_validation(upload, &file);
                Some(quote! {
                    let #files = __caelix_form.take_files(#field_name);
                    for #file in &#files {
                        #validation
                    }
                    #slot = Some(#files);
                })
            }
            _ => None,
        }}).collect::<Vec<_>>();
        let json_assignments = extractor_args
            .iter()
            .filter_map(|(extractor, name, ty, _, _)| {
                let slot = payload_slot_ident(name);
                match extractor {
                    Extractor::Body => {
                        Some(quote! { #slot = Some(__caelix_payload.json::<#ty>()?); })
                    }
                    Extractor::File if is_option_uploaded_file(ty) => {
                        Some(quote! { #slot = Some(None); })
                    }
                    _ => None,
                }
            })
            .collect::<Vec<_>>();
        #[cfg(feature = "uploads")]
        let multipart_direct_assignment =
            extractor_args
                .iter()
                .find_map(|(extractor, name, _ty, _, _)| {
                    let slot = payload_slot_ident(name);
                    matches!(extractor, Extractor::Multipart).then(
                    || quote! { #slot = Some(__caelix_payload.multipart(#route_limit).await?); },
                )
                });
        let payload_values = extractor_args.iter().filter(|(extractor, _, _, _, _)| {
            matches!(extractor, Extractor::Body | Extractor::Multipart | Extractor::File | Extractor::Files)
        }).map(|(_, name, _, _, _)| {
                let slot = payload_slot_ident(name);
                quote! { let #name = #slot.expect("multipart payload extractor must be initialized"); }
        }).collect::<Vec<_>>();
        let payload_setup = if has_payload {
            #[cfg(feature = "uploads")]
            {
                if let Some(assignment) = multipart_direct_assignment {
                    quote! {
                        #(#payload_slots)*
                        if !__caelix_payload.is_multipart() {
                            return Err(caelix::UnsupportedMediaTypeException::new("multipart/form-data is required"));
                        }
                        #assignment
                        #(#payload_values)*
                    }
                } else {
                    quote! {
                        #(#payload_slots)*
                        if __caelix_payload.is_multipart() {
                            let mut __caelix_form = __caelix_payload.multipart(#route_limit).await?;
                            #(#multipart_assignments)*
                        } else {
                            if #force_multipart || !__caelix_payload.is_json_or_missing_content_type() {
                                return Err(caelix::UnsupportedMediaTypeException::new("unsupported request content type"));
                            }
                            #(#json_assignments)*
                        }
                        #(#payload_values)*
                    }
                }
            }
            #[cfg(not(feature = "uploads"))]
            {
                quote! {
                    #(#payload_slots)*
                    if !__caelix_payload.is_json_or_missing_content_type() {
                        return Err(caelix::UnsupportedMediaTypeException::new("unsupported request content type"));
                    }
                    #(#json_assignments)*
                    #(#payload_values)*
                }
            }
        } else {
            quote! {}
        };

        let call_args = extractor_args.iter().map(|(extractor, name, ty, needs_validation, _)| {
            let base = match extractor {
                Extractor::Param | Extractor::Query => match backend {
                    Backend::Actix => quote! { #name.into_inner() },
                    Backend::Axum => quote! { #name.0 },
                },
                Extractor::Body | Extractor::Multipart | Extractor::File | Extractor::Files => quote! { #name },
                Extractor::User => quote! {
                    request_context.get::<#ty>()?
                        .map(|value| (*value).clone())
                        .ok_or_else(|| caelix::UnauthorizedException::new("Not authenticated"))?
                },
                Extractor::Cookie(cookie_name) => {
                    if is_option_of(ty, "String") {
                        quote! { request_context.cookie(#cookie_name).map(str::to_owned) }
                    } else {
                        quote! {
                            request_context.cookie(#cookie_name)
                                .map(str::to_owned)
                                .ok_or_else(|| caelix::BadRequestException::new(
                                    format!("missing required cookie '{}'", #cookie_name)
                                ))?
                        }
                    }
                }
            };
            if *needs_validation {
                quote! {{ let value = #base; caelix::validator::Validate::validate(&value)?; value }}
            } else { base }
        }).collect::<Vec<_>>();

        let route_state_identity = format!(
            "{}::{method_name}::{verb}::{full_path}",
            quote!(#struct_type)
        );
        let route_state_hash = stable_identifier_hash(&route_state_identity);
        let route_state_name = format_ident!(
            "__CaelixRouteState_{}_{}_{route_state_hash:016x}",
            controller_ident,
            method_name
        );
        let guard_state_fields = guard_types
            .iter()
            .enumerate()
            .map(|(index, _)| format_ident!("guard_{index}"))
            .collect::<Vec<_>>();
        let interceptor_state_fields = interceptor_types
            .iter()
            .enumerate()
            .map(|(index, _)| format_ident!("interceptor_{index}"))
            .collect::<Vec<_>>();
        let guard_state_declarations = guard_state_fields
            .iter()
            .zip(guard_types.iter())
            .map(|(field, ty)| quote! { #field: std::sync::Arc<#ty> })
            .collect::<Vec<_>>();
        let interceptor_state_declarations = interceptor_state_fields
            .iter()
            .zip(interceptor_types.iter())
            .map(|(field, ty)| quote! { #field: std::sync::Arc<#ty> })
            .collect::<Vec<_>>();
        let guard_state_initializers = guard_state_fields
            .iter()
            .zip(guard_types.iter())
            .map(|(field, ty)| {
                quote! {
                    #field: container.resolve::<#ty>()
                        .expect("validated route guard must be registered")
                }
            })
            .collect::<Vec<_>>();
        let interceptor_state_initializers = interceptor_state_fields
            .iter()
            .zip(interceptor_types.iter())
            .map(|(field, ty)| {
                quote! {
                    #field: container.resolve::<#ty>()
                        .expect("validated route interceptor must be registered")
                }
            })
            .collect::<Vec<_>>();
        let throttle_state_initializer = if throttle_enabled {
            quote! { container.resolve::<caelix::Throttle>().ok() }
        } else {
            quote! { None }
        };
        route_states.push(quote! {
            #[allow(non_camel_case_types)]
            struct #route_state_name {
                controller: std::sync::Arc<#struct_type>,
                throttle: Option<std::sync::Arc<caelix::Throttle>>,
                #(#guard_state_declarations,)*
                #(#interceptor_state_declarations,)*
            }

            impl #route_state_name {
                fn new(container: &caelix::Container) -> Self {
                    Self {
                        controller: container.resolve::<#struct_type>()
                            .expect("validated route controller must be registered"),
                        throttle: #throttle_state_initializer,
                        #(#guard_state_initializers,)*
                        #(#interceptor_state_initializers,)*
                    }
                }
            }
        });

        let interceptor_chain = interceptor_state_fields
            .iter()
            .rev()
            .enumerate()
            .map(|(index, field)| {
                let interceptor_ref_name = format_ident!("__caelix_interceptor_ref_{index}");
                quote! {
                    let #interceptor_ref_name = &state.#field;
                    let next = caelix::Next::new(move || {
                        caelix::Interceptor::intercept(
                            &**#interceptor_ref_name,
                            request_context,
                            next,
                        )
                    });
                }
            })
            .collect::<Vec<_>>();

        let needs_request_context = !guard_types.is_empty()
            || !interceptor_types.is_empty()
            || extractor_args.iter().any(|(extractor, _, _, _, _)| {
                matches!(extractor, Extractor::User | Extractor::Cookie(_))
            });
        let request_context_binding = needs_request_context.then(
            || quote! { let request_context = ctx.as_ref().expect("request context required"); },
        );
        let (request_headers, request_method, request_path, request_peer) = match backend {
            Backend::Actix => (
                quote! { req.headers() },
                quote! { req.method().as_str() },
                quote! { req.path() },
                quote! { req.peer_addr() },
            ),
            Backend::Axum => (
                quote! { request.headers() },
                quote! { request.method().as_str() },
                quote! { request.path() },
                quote! { request.peer_addr() },
            ),
        };
        let throttle_check = if throttle_enabled {
            quote! {
                if let Some(__caelix_policy) = #throttle_policy {
                    let Some(__caelix_throttle) = state.throttle.as_ref() else {
                        let err = caelix::InternalServerErrorException::new(std::io::Error::other(
                            "throttled route requires ThrottleModule",
                        ));
                        return #response_adapter(correlation.attach_headers(
                            caelix::IntoCaelixResponse::into_response(err)
                        ));
                    };
                    let __caelix_context = ctx.as_ref().expect("throttled route requires context");
                    match __caelix_throttle.check(
                        __caelix_context,
                        #verb,
                        #full_path,
                        __caelix_policy,
                    ).await {
                        Ok(Some(response)) => return #response_adapter(
                            correlation.attach_headers(response)
                        ),
                        Ok(None) => {}
                        Err(err) => return #response_adapter(correlation.attach_headers(
                            caelix::IntoCaelixResponse::into_response(err)
                        )),
                    }
                }
            }
        } else {
            quote! {}
        };
        let payload_buffering = if has_payload {
            let raw_payload = match backend {
                Backend::Actix => quote! { __caelix_payload },
                Backend::Axum => quote! { request.take_payload() },
            };
            quote! {
                let __caelix_payload = match #raw_payload.buffer().await {
                    Ok(payload) => payload,
                    Err(err) => return #response_adapter(correlation.attach_headers(
                        caelix::IntoCaelixResponse::into_response(err)
                    )),
                };
            }
        } else {
            quote! {}
        };
        let guard_checks = guard_state_fields
            .iter()
            .map(|field| {
                quote! {
                    match caelix::Guard::can_activate(
                        &*state.#field,
                        ctx.as_ref().expect("guarded route requires context"),
                    ).await {
                        Ok(true) => {}
                        Ok(false) => return #response_adapter(correlation.attach_headers(
                            caelix::IntoCaelixResponse::into_response(
                                caelix::ForbiddenException::new("Access denied")
                            )
                        )),
                        Err(err) => {
                            caelix::log_http_exception_with_correlation(
                                &err,
                                #request_method,
                                #request_path,
                                &correlation,
                            );
                            return #response_adapter(correlation.attach_headers(
                                caelix::IntoCaelixResponse::into_response(err)
                            ));
                        }
                    }
                }
            })
            .collect::<Vec<_>>();
        let handler_execution = if interceptor_types.is_empty() {
            quote! {
                let controller = &state.controller;
                let __caelix_result: caelix::Result<caelix::HttpResponse> = async {
                    #payload_setup
                    let value = controller.#method_name(#(#call_args),*).await?;
                    Ok(caelix::IntoCaelixResponse::into_response(value))
                }.await;
            }
        } else {
            quote! {
                let controller = &state.controller;
                let next = caelix::Next::new(move || Box::pin(async move {
                    #payload_setup
                    let value = controller.#method_name(#(#call_args),*).await?;
                    Ok(caelix::IntoCaelixResponse::into_response(value))
                }));
                #(#interceptor_chain)*
                let __caelix_result = next.run().await;
            }
        };
        let request_context_body = quote! {
            let mut __caelix_request_id_seen = false;
            let mut __caelix_traceparent_seen = false;
            let mut __caelix_trace_id_seen = false;
            for (__caelix_header_name, __caelix_header_value) in #request_headers.iter() {
                if __caelix_header_value.to_str().is_err() {
                    return #response_adapter(caelix::IntoCaelixResponse::into_response(
                        caelix::BadRequestException::new("invalid request header value"),
                    ));
                }

                let __caelix_seen = match __caelix_header_name.as_str() {
                    "x-request-id" => &mut __caelix_request_id_seen,
                    "traceparent" => &mut __caelix_traceparent_seen,
                    "x-trace-id" => &mut __caelix_trace_id_seen,
                    _ => continue,
                };
                if std::mem::replace(__caelix_seen, true) {
                    return #response_adapter(caelix::IntoCaelixResponse::into_response(
                        caelix::BadRequestException::new(format!(
                            "duplicate correlation header '{}'",
                            __caelix_header_name,
                        )),
                    ));
                }
            }
            let correlation = caelix::CorrelationContext::from_header_values(
                #request_headers.get("x-request-id").and_then(|value| value.to_str().ok()),
                #request_headers.get("traceparent").and_then(|value| value.to_str().ok()),
                #request_headers.get("x-trace-id").and_then(|value| value.to_str().ok()),
            );
            let mut ctx = if #needs_request_context || state.throttle.is_some() {
                let mut headers: std::collections::HashMap<String, String> =
                    std::collections::HashMap::with_capacity(#request_headers.len());
                for (name, value) in #request_headers.iter() {
                    let value = match value.to_str() {
                        Ok(value) => value,
                        Err(_) => unreachable!("request headers were validated"),
                    };
                    let name = name.as_str().to_string();
                    if name == "cookie" {
                        headers.entry(name)
                            .and_modify(|existing| {
                                existing.push_str("; ");
                                existing.push_str(value);
                            })
                            .or_insert_with(|| value.to_string());
                    } else if name == "x-forwarded-for" {
                        headers.entry(name)
                            .and_modify(|existing| {
                                existing.push_str(", ");
                                existing.push_str(value);
                            })
                            .or_insert_with(|| value.to_string());
                    } else {
                        headers.insert(name, value.to_string());
                    }
                }
                let mut context = caelix::RequestContext::from_normalized_headers_with_correlation(
                    #request_method,
                    #request_path,
                    headers,
                    correlation.clone(),
                );
                if let Some(peer_addr) = #request_peer {
                    context = context.with_peer_addr(peer_addr);
                }
                Some(context)
            } else {
                None
            };
            #throttle_check
            #request_context_binding
            #(#guard_checks)*
            #payload_buffering
            #handler_execution
            match __caelix_result {
                Ok(value) => #response_adapter(correlation.attach_headers(value)),
                Err(err) => {
                    caelix::log_http_exception_with_correlation(
                        &err,
                        #request_method,
                        #request_path,
                        &correlation,
                    );
                    #response_adapter(correlation.attach_headers(
                        caelix::IntoCaelixResponse::into_response(err)
                    ))
                }
            }
        };
        let legacy_wrapper_name = format_ident!("{}_legacy", wrapper_name);
        let wrapper = match backend {
            Backend::Actix => quote! {
                async fn #wrapper_name(
                    state: caelix::__actix_web::web::Data<#route_state_name>,
                    req: caelix::__actix_web::HttpRequest,
                    #(#wrapper_params),*
                ) -> caelix::__actix_web::HttpResponse { #request_context_body }

                async fn #legacy_wrapper_name(
                    container: caelix::__actix_web::web::Data<caelix::Container>,
                    req: caelix::__actix_web::HttpRequest,
                    #(#wrapper_params),*
                ) -> caelix::__actix_web::HttpResponse {
                    let state = caelix::__actix_web::web::Data::new(
                        #route_state_name::new(container.get_ref())
                    );
                    Self::#wrapper_name(state, req, #(#wrapper_arg_names),*).await
                }
            },
            Backend::Axum => quote! {
                async fn #wrapper_name(
                    state: std::sync::Arc<#route_state_name>,
                    mut request: caelix::CaelixRequest,
                    #(#wrapper_params,)*
                ) -> caelix::__axum::response::Response { #request_context_body }

                async fn #legacy_wrapper_name(
                    caelix::__axum::extract::State(container):
                        caelix::__axum::extract::State<std::sync::Arc<caelix::Container>>,
                    #(#wrapper_params,)*
                    request: caelix::CaelixRequest,
                ) -> caelix::__axum::response::Response {
                    let state = std::sync::Arc::new(#route_state_name::new(&container));
                    Self::#wrapper_name(state, request, #(#wrapper_arg_names),*).await
                }
            },
        };
        wrappers.push(wrapper);

        let display_path = full_path.replace("{", ":").replace("}", "");
        let handler_name = method_name.to_string();
        registrations.push(match backend {
            Backend::Actix => quote! {
                cfg.route(
                    #full_path,
                    caelix::__actix_web::web::#backend_verb()
                        .to(#struct_type::#legacy_wrapper_name),
                );
            },
            Backend::Axum => quote! {
                cfg.route(
                    #full_path,
                    caelix::__axum::routing::#backend_verb(#struct_type::#legacy_wrapper_name),
                );
            },
        });
        container_registrations.push(match backend {
            Backend::Actix => quote! {
                cfg.app_data(caelix::__actix_web::web::Data::new(
                    #route_state_name::new(&container)
                ));
                cfg.route(
                    #full_path,
                    caelix::__actix_web::web::#backend_verb()
                        .to(#struct_type::#wrapper_name),
                );
            },
            Backend::Axum => quote! {
                let state = std::sync::Arc::new(#route_state_name::new(&container));
                cfg.route(
                    #full_path,
                    caelix::__axum::routing::#backend_verb({
                        move |
                            #(#wrapper_params,)*
                            request: caelix::CaelixRequest
                        | {
                            let state = state.clone();
                            async move {
                                #struct_type::#wrapper_name(
                                    state,
                                    request,
                                    #(#wrapper_arg_names),*
                                ).await
                            }
                        }
                    }),
                );
            },
        });
        routes.push(quote! { caelix::RouteDef { method: #verb, path: #display_path, handler: #handler_name } });

        #[cfg(feature = "openapi")]
        {
            let openapi_name = format_ident!("__{}_openapi", method_name);
            openapi_document_functions.push(openapi_name.clone());
            let typed_openapi = response_spec.is_some()
                || !documented_headers.is_empty()
                || !documented_errors.is_empty()
                || !security_expressions.is_empty();
            let summary = method_summary(&method.attrs)
                .map(|summary| quote! { operation.summary = Some(#summary.to_string()); });
            let body = extractor_args.iter().find_map(|(extractor, _, ty, _, _)| {
                matches!(extractor, Extractor::Body).then(|| (ty.clone(), !typed_openapi))
            });
            let extractor_parameters = extractor_args.iter().filter_map(|(extractor, name, ty, _, _)| {
                let parameter_in = match extractor {
                    Extractor::Param => quote! { caelix::openapi::utoipa::openapi::path::ParameterIn::Path },
                    Extractor::Query => quote! { caelix::openapi::utoipa::openapi::path::ParameterIn::Query },
                    Extractor::Cookie(_) => quote! { caelix::openapi::utoipa::openapi::path::ParameterIn::Cookie },
                    _ => return None,
                };
                let required = matches!(extractor, Extractor::Param)
                    || matches!(extractor, Extractor::Cookie(_)) && !is_option_of(ty, "String");
                let parameter_name = match extractor {
                    Extractor::Cookie(name) => quote! { #name },
                    _ => quote! { stringify!(#name) },
                };
                    let schema = if matches!(extractor, Extractor::Cookie(_)) {
                        quote! { caelix::openapi::inline_schema::<String>() }
                    } else if !typed_openapi {
                        quote! { caelix::openapi::untyped_schema() }
                    } else {
                    quote! { caelix::openapi::inline_schema::<#ty>() }
                };
                Some(quote! {
                    operation.parameters.get_or_insert_with(Vec::new).push(caelix::openapi::parameter(
                        #parameter_name, #parameter_in, #required, None, #schema,
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
            let multipart_files = extractor_args
                .iter()
                .filter_map(|(extractor, _name, ty, _, upload)| match extractor {
                    Extractor::File => {
                        let upload = upload.as_ref().expect("file fields have options");
                        let field_name = &upload.name;
                        let required = !is_option_uploaded_file(ty);
                        let max_size = upload
                            .max_size
                            .map(|max_size| quote! { Some(#max_size) })
                            .unwrap_or_else(|| quote! { None });
                        let content_types = &upload.content_types;
                        Some(quote! { (#field_name, false, #required, #max_size, &[#(#content_types),*]) })
                    }
                    Extractor::Files => {
                        let upload = upload.as_ref().expect("file fields have options");
                        let field_name = &upload.name;
                        let max_size = upload
                            .max_size
                            .map(|max_size| quote! { Some(#max_size) })
                            .unwrap_or_else(|| quote! { None });
                        let content_types = &upload.content_types;
                        Some(quote! { (#field_name, true, true, #max_size, &[#(#content_types),*]) })
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            let has_direct_multipart = extractor_args
                .iter()
                .any(|(extractor, _, _, _, _)| matches!(extractor, Extractor::Multipart));
            let all_files_optional = extractor_args
                .iter()
                .filter(|(extractor, _, _, _, _)| {
                    matches!(extractor, Extractor::File | Extractor::Files)
                })
                .all(|(extractor, _, ty, _, _)| {
                    matches!(extractor, Extractor::File) && is_option_uploaded_file(ty)
                });
            let request_body = if has_direct_multipart {
                quote! {
                    operation.request_body = Some(caelix::openapi::multipart_request_body(None, &[]));
                }
            } else if !multipart_files.is_empty() {
                match body {
                    Some((body, false)) if all_files_optional => quote! {
                        let mut request = caelix::openapi::request_body(caelix::openapi::schema_ref::<#body>(openapi));
                        let multipart = caelix::openapi::multipart_request_body(Some(caelix::openapi::schema_ref::<#body>(openapi)), &[#(#multipart_files),*]);
                        request.content.extend(multipart.content);
                        operation.request_body = Some(request);
                    },
                    Some((body, false)) => quote! {
                        operation.request_body = Some(caelix::openapi::multipart_request_body(Some(caelix::openapi::schema_ref::<#body>(openapi)), &[#(#multipart_files),*]));
                    },
                    Some((_body, true)) => quote! {
                        operation.request_body = Some(caelix::openapi::multipart_request_body(Some(caelix::openapi::untyped_schema()), &[#(#multipart_files),*]));
                    },
                    None => quote! {
                        operation.request_body = Some(caelix::openapi::multipart_request_body(None, &[#(#multipart_files),*]));
                    },
                }
            } else if let Some((body, untyped)) = body {
                if untyped {
                    quote! {
                        let mut request = caelix::openapi::request_body(caelix::openapi::untyped_schema());
                        let multipart = caelix::openapi::multipart_request_body(Some(caelix::openapi::untyped_schema()), &[]);
                        request.content.extend(multipart.content);
                        operation.request_body = Some(request);
                    }
                } else {
                    quote! {
                        let mut request = caelix::openapi::request_body(caelix::openapi::schema_ref::<#body>(openapi));
                        let multipart = caelix::openapi::multipart_request_body(Some(caelix::openapi::schema_ref::<#body>(openapi)), &[]);
                        request.content.extend(multipart.content);
                        operation.request_body = Some(request);
                    }
                }
            } else {
                quote! {}
            };
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
                .map(|body| {
                    if typed_openapi {
                        quote! { Some(caelix::openapi::schema_ref::<#body>(openapi)) }
                    } else {
                        quote! { Some(caelix::openapi::untyped_schema()) }
                    }
                })
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
            let throttle_response = quote! {
                if #documented_throttle {
                    let (status, response) =
                        caelix::openapi::error_response::<caelix::TooManyRequestsException>(openapi);
                    operation.responses.responses.insert(status, response.into());
                }
            };
            let route_security = quote! {
                caelix::openapi::apply_security(&mut operation, &[#(#security_expressions),*]);
            };
            let full_path = full_path.clone();
            openapi_routes.push(quote! {
                fn #openapi_name(
                    openapi: &mut caelix::openapi::utoipa::openapi::OpenApi,
                    __caelix_global_throttle: bool,
                ) {
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
                    #throttle_response
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
    let register_routes_with_container = match backend {
        Backend::Actix => quote! {
            let Some(cfg) = cfg_any.downcast_mut::<caelix::__actix_web::web::ServiceConfig>() else { return; };
            #(#container_registrations)*
        },
        Backend::Axum => quote! {
            let Some(cfg) = cfg_any.downcast_mut::<caelix::AxumRouterBuilder>() else { return; };
            #(#container_registrations)*
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
    #[cfg(feature = "openapi")]
    let openapi_route_functions = quote! { #(#openapi_routes)* };
    #[cfg(not(feature = "openapi"))]
    let openapi_route_functions = quote! {};
    let route_dependencies = route_dependencies.iter();
    let route_throttle_policies = route_throttle_policies.iter().map(|(limit, window)| {
        quote! { caelix::ThrottlePolicy::new(#limit, #window).validate()?; }
    });
    quote! {
        #(#errors)*
        #impl_block
        #(#route_states)*
        impl caelix::Controller for #struct_type {
            fn base_path() -> &'static str { #base_path }
            fn route_dependencies() -> Vec<caelix::ProviderDependency> {
                vec![#(caelix::ProviderDependency::of::<#route_dependencies>()),*]
            }
            fn validate_routes() -> caelix::Result<()> {
                #(#route_throttle_policies)*
                Ok(())
            }
            fn routes() -> &'static [caelix::RouteDef] { &[#(#routes),*] }
            fn register_routes(cfg_any: &mut dyn std::any::Any) { #register_routes }
            fn register_routes_with_container(
                cfg_any: &mut dyn std::any::Any,
                container: std::sync::Arc<caelix::Container>,
            ) {
                #register_routes_with_container
            }
            #openapi_controller_methods
        }
        impl #struct_type { #(#wrappers)* #openapi_route_functions }
    }
    .into()
}
