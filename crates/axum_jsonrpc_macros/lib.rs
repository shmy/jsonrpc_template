use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{
    Expr, FnArg, ItemFn, LitBool, LitStr, Meta, PatType, ReturnType, Token, Type, TypePath,
    parse_macro_input,
};

/// Annotate an RPC handler to generate unit struct `{fn}_rpc` for [`RpcRouter::register`].
///
/// ```ignore
/// /// Create an account.
/// #[rpc("identity.account.create")]
/// pub async fn create_account(
///     Params(req): Params<CreateAccountRequest>,
/// ) -> Result<CreateAccountResponse> { ... }
///
/// // generates unit struct `create_account_rpc`
/// // register with: .register(methods::create_account::create_account_rpc)?
/// ```
#[proc_macro_attribute]
pub fn rpc(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as RpcArgs);
    let input = parse_macro_input!(item as ItemFn);

    match expand(args, input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

struct RpcArgs {
    name: LitStr,
    summary: Option<LitStr>,
    description: Option<LitStr>,
    tag: Option<LitStr>,
    deprecated: Option<bool>,
}

impl Parse for RpcArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name: LitStr = input.parse()?;
        let mut summary = None;
        let mut description = None;
        let mut tag = None;
        let mut deprecated = None;

        while input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let key: syn::Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            match key.to_string().as_str() {
                "summary" => summary = Some(input.parse()?),
                "description" => description = Some(input.parse()?),
                "tag" => tag = Some(input.parse()?),
                "deprecated" => {
                    let value: LitBool = input.parse()?;
                    deprecated = Some(value.value());
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown rpc option `{other}`"),
                    ));
                }
            }
        }

        Ok(Self {
            name,
            summary,
            description,
            tag,
            deprecated,
        })
    }
}

fn expand(args: RpcArgs, input: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let fn_name = &input.sig.ident;
    let rpc_fn_name = syn::Ident::new(&format!("{fn_name}_rpc"), fn_name.span());
    let params_ty = find_params_type(&input)?;
    let result_ty = find_result_type(&input)?;
    let extractor_tys = extractor_types(&input);

    let (doc_summary, doc_description) = docs_from_attrs(&input);
    let summary = args.summary.map(|s| s.value()).or(doc_summary);
    let description = args.description.map(|s| s.value()).or(doc_description);

    let name_lit = args.name;
    let mut build = quote! {
        ::axum_jsonrpc::MethodSpec::new(#name_lit, #fn_name)
    };

    if let Some(summary) = summary {
        build = quote! { #build.summary(#summary) };
    }
    if let Some(description) = description {
        build = quote! { #build.description(#description) };
    }
    if let Some(tag) = args.tag {
        let tag = tag.value();
        build = quote! { #build.tag(#tag) };
    }
    if let Some(deprecated) = args.deprecated {
        build = quote! { #build.deprecated(#deprecated) };
    }
    if let Some(params_ty) = params_ty {
        build = quote! { #build.params::<#params_ty>() };
    }
    if let Some(result_ty) = result_ty {
        build = quote! { #build.result::<#result_ty>() };
    }

    Ok(quote! {
        #input

        /// OpenRPC + router registration handle for [`#fn_name`].
        #[allow(non_camel_case_types)]
        #[derive(Clone, Copy, Debug, Default)]
        pub struct #rpc_fn_name;

        impl<S> ::axum_jsonrpc::Registrable<S> for #rpc_fn_name
        where
            S: ::core::clone::Clone + ::core::marker::Send + ::core::marker::Sync + 'static,
            #(
                #extractor_tys: ::axum_jsonrpc::FromRequestParts<S> + ::core::marker::Send,
                <#extractor_tys as ::axum_jsonrpc::FromRequestParts<S>>::Rejection:
                    ::axum_jsonrpc::IntoExtractError + ::core::marker::Send,
            )*
        {
            fn register(
                self,
                router: ::axum_jsonrpc::RpcRouter<S>,
            ) -> ::core::result::Result<::axum_jsonrpc::RpcRouter<S>, ::axum_jsonrpc::RegisterError>
            {
                router.register_spec(#build)
            }
        }
    })
}

fn extractor_types(input: &ItemFn) -> Vec<Type> {
    input
        .sig
        .inputs
        .iter()
        .filter_map(|arg| match arg {
            FnArg::Typed(PatType { ty, .. }) => Some(ty.as_ref().clone()),
            FnArg::Receiver(_) => None,
        })
        .collect()
}

fn find_params_type(input: &ItemFn) -> syn::Result<Option<Type>> {
    let mut found = None;
    for arg in &input.sig.inputs {
        let FnArg::Typed(PatType { ty, .. }) = arg else {
            continue;
        };
        if let Some(inner) = extract_params_inner(ty) {
            if found.is_some() {
                return Err(syn::Error::new(
                    ty.span(),
                    "multiple `Params<_>` / `ValidatedParams<_>` extractors are not supported by #[rpc]",
                ));
            }
            found = Some(inner);
        }
    }
    Ok(found)
}

fn extract_params_inner(ty: &Type) -> Option<Type> {
    let Type::Path(TypePath { path, .. }) = ty else {
        return None;
    };
    let last = path.segments.last()?;
    if last.ident != "Params" && last.ident != "ValidatedParams" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(inner) => Some(inner.clone()),
        _ => None,
    })
}

fn find_result_type(input: &ItemFn) -> syn::Result<Option<Type>> {
    let ReturnType::Type(_, ty) = &input.sig.output else {
        return Ok(None);
    };
    Ok(extract_result_ok(ty))
}

fn extract_result_ok(ty: &Type) -> Option<Type> {
    let Type::Path(TypePath { path, .. }) = ty else {
        return None;
    };
    let last = path.segments.last()?;
    if last.ident != "Result" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(inner) => Some(inner.clone()),
        _ => None,
    })
}

fn docs_from_attrs(input: &ItemFn) -> (Option<String>, Option<String>) {
    let mut lines = Vec::new();
    for attr in &input.attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        let Meta::NameValue(meta) = &attr.meta else {
            continue;
        };
        let Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(lit),
            ..
        }) = &meta.value
        else {
            continue;
        };
        lines.push(trim_doc_line(&lit.value()));
    }

    if lines.is_empty() {
        return (None, None);
    }

    let split_at = lines
        .iter()
        .position(String::is_empty)
        .unwrap_or(lines.len());
    let summary = {
        let text = lines
            .get(..split_at)
            .unwrap_or(&[])
            .iter()
            .filter(|l| !l.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        (!text.is_empty()).then_some(text)
    };
    let description = lines
        .get(split_at + 1..)
        .map(|rest| rest.join("\n"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    (summary, description)
}

fn trim_doc_line(line: &str) -> String {
    line.strip_prefix(' ').unwrap_or(line).to_string()
}
