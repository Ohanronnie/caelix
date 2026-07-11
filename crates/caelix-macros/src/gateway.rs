use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, ImplItem, ItemImpl, LitStr, Pat, Type, parse_macro_input};

pub(crate) fn expand(args: TokenStream, input: TokenStream) -> TokenStream {
    let path = parse_macro_input!(args as LitStr);
    let implementation = parse_macro_input!(input as ItemImpl);
    let self_ty = implementation.self_ty.clone();

    if is_websocket_impl(&implementation) {
        return quote! {
            #implementation

            impl caelix::Gateway for #self_ty {
                fn definition() -> caelix::GatewayDef {
                    caelix::GatewayDef::websocket::<Self>(#path)
                }
            }
        }
        .into();
    }

    expand_socket_io(path, implementation, self_ty).into()
}

fn is_websocket_impl(implementation: &ItemImpl) -> bool {
    implementation.trait_.as_ref().is_some_and(|(_, path, _)| {
        path.segments
            .last()
            .is_some_and(|segment| segment.ident == "WebSocketGateway")
    })
}

fn expand_socket_io(
    path: LitStr,
    mut implementation: ItemImpl,
    self_ty: Box<Type>,
) -> proc_macro2::TokenStream {
    let mut errors = Vec::new();
    let mut handlers = Vec::new();

    for item in &mut implementation.items {
        let ImplItem::Fn(method) = item else { continue };
        let mut event = None;
        method.attrs.retain(|attribute| {
            if attribute.path().is_ident("on_message") {
                match attribute.parse_args::<LitStr>() {
                    Ok(name) => event = Some(name),
                    Err(error) => errors.push(error.to_compile_error()),
                }
                false
            } else {
                true
            }
        });

        let Some(event) = event else { continue };
        if method.sig.asyncness.is_none() {
            errors.push(
                syn::Error::new_spanned(&method.sig, "Socket.IO message handlers must be async")
                    .to_compile_error(),
            );
            continue;
        }

        let typed = method
            .sig
            .inputs
            .iter()
            .filter_map(|input| match input {
                FnArg::Typed(value) => Some(value),
                FnArg::Receiver(_) => None,
            })
            .collect::<Vec<_>>();
        let (socket_argument, payload) = match typed.as_slice() {
            [payload] => (None, payload),
            [socket, payload] => (Some(socket), payload),
            _ => {
                errors.push(
                    syn::Error::new_spanned(
                        &method.sig,
                        "Socket.IO handlers must accept `payload: T` or `socket: SocketRef, payload: T`",
                    )
                    .to_compile_error(),
                );
                continue;
            }
        };
        let Some(payload_name) = ident_pattern(&payload.pat) else {
            errors.push(
                syn::Error::new_spanned(
                    &payload.pat,
                    "expected a simple identifier for the Socket.IO payload",
                )
                .to_compile_error(),
            );
            continue;
        };
        if socket_argument.is_some_and(|socket| ident_pattern(&socket.pat).is_none()) {
            errors.push(
                syn::Error::new_spanned(
                    &socket_argument.unwrap().pat,
                    "expected a simple identifier for the Socket.IO socket",
                )
                .to_compile_error(),
            );
            continue;
        }

        handlers.push((
            event,
            method.sig.ident.clone(),
            payload_name,
            (*payload.ty).clone(),
            socket_argument.is_some(),
        ));
    }

    if handlers.is_empty() && errors.is_empty() {
        errors.push(
            syn::Error::new_spanned(
                &implementation.self_ty,
                "a Socket.IO gateway must declare at least one #[on_message(\"event\")] handler",
            )
            .to_compile_error(),
        );
    }

    let registrations = handlers.into_iter().map(
        |(event, method, payload_name, payload_ty, passes_socket)| {
            let invocation = if passes_socket {
                quote! { gateway.#method(socket.clone(), #payload_name).await }
            } else {
                quote! { gateway.#method(#payload_name).await }
            };
            quote! {
                {
                    let container = container.clone();
                    socket.on(#event, move |socket: caelix::socket_io::SocketRef,
                        caelix::socket_io::Data(#payload_name): caelix::socket_io::Data<#payload_ty>,
                        ack: caelix::socket_io::AckSender| {
                        let container = container.clone();
                        async move {
                            let result = match container.resolve::<#self_ty>() {
                                Ok(gateway) => #invocation,
                                Err(error) => Err(error),
                            };
                            match result {
                                Ok(value) => {
                                    let _ = ack.send(&value);
                                }
                                Err(error) => {
                                    let error = caelix::socket_io::SocketIoError::from(error);
                                    let _ = ack.send(&error);
                                    let _ = socket.emit("error", &error);
                                }
                            }
                        }
                    });
                }
            }
        },
    );

    quote! {
        #implementation
        #(#errors)*

        impl caelix::socket_io::SocketIoGateway for #self_ty {
            fn register_socket_io(
                io: &caelix::socket_io::SocketIo,
                container: &caelix::Container,
            ) -> caelix::Result<()> {
                let container = ::std::sync::Arc::new(container.clone());
                io.ns(#path, move |socket: caelix::socket_io::SocketRef| {
                    let container = container.clone();
                    async move {
                        #(#registrations)*
                    }
                });
                Ok(())
            }
        }

        impl caelix::Gateway for #self_ty {
            fn definition() -> caelix::GatewayDef {
                fn register(
                    container: &caelix::Container,
                    handle: &dyn ::std::any::Any,
                ) -> caelix::Result<()> {
                    let handle = handle
                        .downcast_ref::<caelix::socket_io::SocketIoHandle>()
                        .ok_or_else(|| caelix::HttpException::new(
                            caelix::StatusCode::INTERNAL_SERVER_ERROR,
                            "Internal Server Error",
                            "Socket.IO gateway was registered without a Socket.IO handle",
                        ))?;
                    <#self_ty as caelix::socket_io::SocketIoGateway>::register_socket_io(
                        handle.io(), container,
                    )
                }
                caelix::GatewayDef::socket_io::<Self>(#path, register)
            }
        }
    }
}

fn ident_pattern(pattern: &Pat) -> Option<syn::Ident> {
    match pattern {
        Pat::Ident(ident) => Some(ident.ident.clone()),
        _ => None,
    }
}
