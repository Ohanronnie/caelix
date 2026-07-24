use proc_macro::TokenStream;
use quote::quote;
use syn::{
    FnArg, ImplItem, ImplItemFn, ItemImpl, LitStr, Pat, PathArguments, ReturnType, Type,
    parse_macro_input, spanned::Spanned,
};

enum HandlerKind {
    Command,
    Event,
}

pub(crate) fn expand(args: TokenStream, input: TokenStream) -> TokenStream {
    let mut implementation = parse_macro_input!(input as ItemImpl);
    let mut errors = Vec::new();
    if !args.is_empty() {
        errors.push(
            syn::Error::new(
                proc_macro2::Span::call_site(),
                "#[microservice] does not accept arguments",
            )
            .to_compile_error(),
        );
    }
    let Some((_, trait_path, _)) = &implementation.trait_ else {
        let service = implementation.self_ty.clone();
        let mut handlers = Vec::new();

        for item in &mut implementation.items {
            let ImplItem::Fn(method) = item else { continue };
            match parse_handler(method) {
                Ok(Some(handler)) => handlers.push(handler),
                Ok(None) => {}
                Err(error) => errors.push(error.to_compile_error()),
            }
        }

        let expanded = quote! {
            #(#errors)*
            #implementation
            impl caelix::Microservice for #service {
                fn definition() -> caelix::MicroserviceDef {
                    caelix::MicroserviceDef::of::<Self>(|| vec![#(#handlers),*])
                }
            }
        };
        return expanded.into();
    };
    errors.push(
        syn::Error::new_spanned(
            trait_path,
            "#[microservice] only supports inherent impl blocks",
        )
        .to_compile_error(),
    );
    quote!(#(#errors)* #implementation).into()
}

fn parse_handler(method: &mut ImplItemFn) -> syn::Result<Option<proc_macro2::TokenStream>> {
    let mut kind = None;
    let mut subject = None;
    method.attrs.retain(|attribute| {
        if attribute_is(attribute, "message_pattern") || attribute_is(attribute, "event_pattern") {
            let parsed = attribute.parse_args::<LitStr>();
            match parsed {
                Ok(value) => {
                    let next = if attribute_is(attribute, "message_pattern") {
                        HandlerKind::Command
                    } else {
                        HandlerKind::Event
                    };
                    if kind.is_some() {
                        // Keep a marker that is converted into a regular syn error below.
                        kind = Some(HandlerKind::Event);
                        subject = Some(LitStr::new("\0", attribute.span()));
                    } else {
                        kind = Some(next);
                        subject = Some(value);
                    }
                }
                Err(_) => {
                    kind = Some(HandlerKind::Event);
                    subject = Some(LitStr::new("\0", attribute.span()));
                }
            }
            false
        } else {
            true
        }
    });
    let Some(kind) = kind else { return Ok(None) };
    let subject = subject.expect("handler subject set with handler kind");
    if subject.value() == "\0" {
        return Err(syn::Error::new_spanned(
            method,
            "a microservice method must have exactly one #[message_pattern(\"...\")] or #[event_pattern(\"...\")]",
        ));
    }
    validate_subject(&subject, matches!(kind, HandlerKind::Command))?;
    if method.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            method.sig.fn_token,
            "microservice handlers must be async",
        ));
    }
    if !method.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &method.sig.generics,
            "microservice handlers cannot be generic",
        ));
    }
    let Some(FnArg::Receiver(receiver)) = method.sig.inputs.first() else {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "microservice handlers require an &self receiver",
        ));
    };
    if receiver.reference.is_none() || receiver.mutability.is_some() {
        return Err(syn::Error::new_spanned(
            receiver,
            "microservice handlers require an &self receiver",
        ));
    }

    let mut payload = None;
    let mut context = None;
    let mut context_after_payload = false;
    for argument in method.sig.inputs.iter_mut().skip(1) {
        let FnArg::Typed(argument) = argument else {
            return Err(syn::Error::new_spanned(
                argument,
                "microservice handlers support only &self as a receiver",
            ));
        };
        let mut extractor = None;
        let mut invalid_marker = false;
        argument.attrs.retain(|attribute| {
            if attribute_is(attribute, "payload") {
                if !matches!(attribute.meta, syn::Meta::Path(_)) || extractor.is_some() {
                    invalid_marker = true;
                } else {
                    extractor = Some("payload");
                }
                false
            } else if attribute_is(attribute, "context") {
                if !matches!(attribute.meta, syn::Meta::Path(_)) || extractor.is_some() {
                    invalid_marker = true;
                } else {
                    extractor = Some("context");
                }
                false
            } else {
                true
            }
        });
        if invalid_marker {
            return Err(syn::Error::new_spanned(
                argument,
                "microservice parameters require exactly one bare #[payload] or #[context] marker",
            ));
        }
        let Some(extractor) = extractor else {
            return Err(syn::Error::new_spanned(
                argument,
                "microservice parameters require #[payload] or #[context]",
            ));
        };
        let Pat::Ident(name) = argument.pat.as_ref() else {
            return Err(syn::Error::new_spanned(
                &argument.pat,
                "microservice parameters must be named",
            ));
        };
        if extractor == "payload" {
            if payload.is_some() {
                return Err(syn::Error::new_spanned(
                    argument,
                    "a microservice handler can have only one #[payload] parameter",
                ));
            }
            payload = Some((name.ident.clone(), (*argument.ty).clone()));
        } else {
            if context.is_some() || !is_message_context(argument.ty.as_ref()) {
                return Err(syn::Error::new_spanned(
                    argument,
                    "#[context] must be a single MessageContext parameter",
                ));
            }
            context_after_payload = payload.is_some();
            context = Some(name.ident.clone());
        }
    }
    let Some((payload_name, payload_type)) = payload else {
        return Err(syn::Error::new_spanned(
            &method.sig,
            "microservice handlers require exactly one #[payload] parameter",
        ));
    };
    let response_type = result_type(&method.sig.output)?;
    let is_event = matches!(kind, HandlerKind::Event);
    if is_event && !is_unit(response_type) {
        return Err(syn::Error::new_spanned(
            &method.sig.output,
            "event handlers must return Result<()>",
        ));
    }
    let method_name = &method.sig.ident;
    let invocation_arguments = match (context.is_some(), context_after_payload) {
        (false, _) => quote!(#payload_name),
        (true, false) => quote!(context, #payload_name),
        (true, true) => quote!(#payload_name, context),
    };
    let invoke = if is_event {
        quote! {
            caelix::MessageHandlerDef::new(caelix::MessageHandlerKind::Event, #subject, |container, context, payload| {
                Box::pin(async move {
                    fn __caelix_assert_payload<T: caelix::__microservice_serde::de::DeserializeOwned + Send>() {}
                    __caelix_assert_payload::<#payload_type>();
                    let #payload_name: #payload_type = caelix::__microservice_serde_json::from_value(payload).map_err(|error| {
                        caelix::BadRequestException::new(format!("invalid message payload: {error}"))
                    })?;
                    let service = container.resolve::<Self>()?;
                    service.#method_name(#invocation_arguments).await?;
                    Ok(None)
                })
            })
        }
    } else {
        quote! {
            caelix::MessageHandlerDef::new(caelix::MessageHandlerKind::Command, #subject, |container, context, payload| {
                Box::pin(async move {
                    fn __caelix_assert_payload<T: caelix::__microservice_serde::de::DeserializeOwned + Send>() {}
                    fn __caelix_assert_response<T: caelix::__microservice_serde::Serialize + Send>() {}
                    __caelix_assert_payload::<#payload_type>();
                    __caelix_assert_response::<#response_type>();
                    let #payload_name: #payload_type = caelix::__microservice_serde_json::from_value(payload).map_err(|error| {
                        caelix::BadRequestException::new(format!("invalid message payload: {error}"))
                    })?;
                    let service = container.resolve::<Self>()?;
                    let result = service.#method_name(#invocation_arguments).await?;
                    let response = caelix::__microservice_serde_json::to_value(result).map_err(|error| {
                        caelix::HttpException::new(caelix::StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error", error.to_string())
                    })?;
                    Ok(Some(response))
                })
            })
        }
    };
    Ok(Some(invoke))
}

fn validate_subject(subject: &LitStr, command: bool) -> syn::Result<()> {
    let value = subject.value();
    if value.is_empty()
        || value.starts_with('.')
        || value.ends_with('.')
        || value.split('.').any(str::is_empty)
    {
        return Err(syn::Error::new_spanned(
            subject,
            "microservice subjects must contain non-empty dot-separated tokens",
        ));
    }
    if value.chars().any(char::is_whitespace) || value.chars().any(|character| character == '\0') {
        return Err(syn::Error::new_spanned(
            subject,
            "microservice subjects cannot contain whitespace or control characters",
        ));
    }
    let tokens: Vec<_> = value.split('.').collect();
    if tokens.iter().enumerate().any(|(index, token)| {
        (token.contains('*') && (command || *token != "*"))
            || (token.contains('>') && (command || *token != ">" || index + 1 != tokens.len()))
    }) {
        return Err(syn::Error::new_spanned(
            subject,
            "invalid microservice wildcard pattern; `*` matches one token and terminal `>` matches one or more tokens",
        ));
    }
    Ok(())
}

fn result_type(output: &ReturnType) -> syn::Result<&Type> {
    let ReturnType::Type(_, output) = output else {
        return Err(syn::Error::new_spanned(
            output,
            "microservice handlers must return Result<T>",
        ));
    };
    let Type::Path(path) = output.as_ref() else {
        return Err(syn::Error::new_spanned(
            output,
            "microservice handlers must return Result<T>",
        ));
    };
    let Some(segment) = path.path.segments.last() else {
        return Err(syn::Error::new_spanned(
            output,
            "microservice handlers must return Result<T>",
        ));
    };
    if segment.ident != "Result" {
        return Err(syn::Error::new_spanned(
            output,
            "microservice handlers must return Result<T>",
        ));
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            output,
            "microservice handlers must return Result<T>",
        ));
    };
    let Some(syn::GenericArgument::Type(value)) = arguments.args.first() else {
        return Err(syn::Error::new_spanned(
            output,
            "microservice handlers must return Result<T>",
        ));
    };
    Ok(value)
}

fn is_unit(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty())
}

fn is_message_context(ty: &Type) -> bool {
    matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|segment| segment.ident == "MessageContext"))
}

fn attribute_is(attribute: &syn::Attribute, expected: &str) -> bool {
    attribute
        .path()
        .segments
        .last()
        .is_some_and(|segment| segment.ident == expected)
}
