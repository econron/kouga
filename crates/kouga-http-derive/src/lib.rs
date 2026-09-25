use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Expr, FnArg, GenericArgument, ItemFn, Lit, Meta, PathArguments, ReturnType, Type,
    parse::Parser, parse_macro_input,
};

#[proc_macro_attribute]
pub fn endpoint(args: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    expand(args.into(), input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn inner_type<'a>(ty: &'a Type, name: &str) -> Option<&'a Type> {
    let Type::Path(path) = ty else { return None };
    let segment = path.path.segments.last()?;
    if segment.ident != name {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    match args.args.first()? {
        GenericArgument::Type(ty) => Some(ty),
        _ => None,
    }
}

fn expand(args: proc_macro2::TokenStream, input: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let attrs =
        syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated.parse2(args)?;
    let mut operation_id = None;
    let mut state = None;
    let mut summary = None;
    let mut description = None;
    for attr in attrs {
        let Meta::NameValue(value) = attr else {
            return Err(syn::Error::new_spanned(attr, "expected name = value"));
        };
        if value.path.is_ident("operation_id")
            || value.path.is_ident("summary")
            || value.path.is_ident("description")
        {
            let key = value.path.clone();
            let Expr::Lit(expr) = value.value else {
                return Err(syn::Error::new_spanned(
                    value.path,
                    "expected string literal",
                ));
            };
            let Lit::Str(value) = expr.lit else {
                return Err(syn::Error::new_spanned(expr, "expected string literal"));
            };
            if value.value().is_empty() {
                return Err(syn::Error::new_spanned(value, "value must not be empty"));
            }
            if key.is_ident("operation_id") {
                if operation_id.replace(value).is_some() {
                    return Err(syn::Error::new_spanned(key, "duplicate operation_id"));
                }
            } else if key.is_ident("summary") {
                if summary.replace(value).is_some() {
                    return Err(syn::Error::new_spanned(key, "duplicate summary"));
                }
            } else {
                if description.replace(value).is_some() {
                    return Err(syn::Error::new_spanned(key, "duplicate description"));
                }
            }
        } else if value.path.is_ident("state") {
            let expr = value.value;
            if state.replace(syn::parse2::<Type>(quote!(#expr))?).is_some() {
                return Err(syn::Error::new_spanned(value.path, "duplicate state"));
            }
        } else {
            return Err(syn::Error::new_spanned(
                value.path,
                "unknown endpoint attribute",
            ));
        }
    }
    let operation_id = operation_id.ok_or_else(|| {
        syn::Error::new_spanned(&input.sig.ident, "endpoint requires operation_id")
    })?;
    if input.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            &input.sig.ident,
            "endpoint must be async",
        ));
    }
    if !input.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.sig.generics,
            "generic endpoints are not supported",
        ));
    }
    let name = &input.sig.ident;
    let endpoint_name = format_ident!("{}_endpoint", name);
    let vis = &input.vis;
    let mut inputs = Vec::new();
    for arg in &input.sig.inputs {
        if let FnArg::Typed(arg) = arg {
            if let Some(value) = inner_type(&arg.ty, "State") {
                state = Some(value.clone());
            }
            if inner_type(&arg.ty, "State").is_none() {
                inputs.push(&arg.ty);
            }
        }
    }
    let state = state.unwrap_or_else(|| syn::parse_quote!(()));
    let response = match &input.sig.output {
        ReturnType::Type(_, ty) => inner_type(ty, "Result").unwrap_or(ty),
        ReturnType::Default => {
            return Err(syn::Error::new_spanned(
                &input.sig.ident,
                "endpoint requires an explicit response type",
            ));
        }
    };
    let input_meta = inputs.iter().map(|ty| quote!(.input::<#ty>()));
    let summary_meta = summary.map(|value| quote!(operation.summary = Some(#value);));
    let description_meta = description.map(|value| quote!(operation.description = Some(#value);));
    Ok(quote! {
        #input
        #vis fn #endpoint_name() -> ::kouga_http::Endpoint<#state> {
            let mut operation = ::kouga_http::Operation::new(#operation_id)
                #(#input_meta)*
                .response::<#response>();
            #summary_meta
            #description_meta
            ::kouga_http::Endpoint::handler(#name, operation)
        }
    })
}
