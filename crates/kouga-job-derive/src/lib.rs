use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, LitInt, LitStr, parse_macro_input};

#[proc_macro_derive(Job, attributes(job))]
pub fn derive_job(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// One attribute declares both the contract and serde support for the payload.
#[proc_macro_attribute]
pub fn job(args: TokenStream, input: TokenStream) -> TokenStream {
    let item = parse_macro_input!(input as DeriveInput);
    let args = proc_macro2::TokenStream::from(args);
    let annotated: DeriveInput = match syn::parse2(quote!(#[job(#args)] #item)) {
        Ok(value) => value,
        Err(error) => return error.into_compile_error().into(),
    };
    match expand(annotated) {
        Ok(contract) => quote! {
            #[derive(::kouga_job::serde::Serialize, ::kouga_job::serde::Deserialize)]
            #[serde(crate = "kouga_job::serde")]
            #item
            #contract
        }
        .into(),
        Err(error) => error.into_compile_error().into(),
    }
}

fn expand(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let mut name = None;
    let mut version = None;
    let mut queue = None;
    for attr in input
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("job"))
    {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                let value: LitStr = meta.value()?.parse()?;
                if value.value().is_empty() || name.replace(value).is_some() {
                    return Err(meta.error("job name must be nonempty and unique"));
                }
            } else if meta.path.is_ident("version") {
                let value: LitInt = meta.value()?.parse()?;
                if value.base10_parse::<u32>()? == 0 || version.replace(value).is_some() {
                    return Err(meta.error("job version must be positive and unique"));
                }
            } else if meta.path.is_ident("queue") {
                let value: LitStr = meta.value()?.parse()?;
                if value.value().is_empty() || queue.replace(value).is_some() {
                    return Err(meta.error("job queue must be nonempty and unique"));
                }
            } else {
                return Err(meta.error("expected name, version, or queue"));
            }
            Ok(())
        })?;
    }
    let missing = || {
        syn::Error::new_spanned(
            &input.ident,
            "expected #[job(name = \"...\", version = 1, queue = \"...\")]",
        )
    };
    let (name, version, queue) = (
        name.ok_or_else(missing)?,
        version.ok_or_else(missing)?,
        queue.ok_or_else(missing)?,
    );
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::kouga_job::Job for #ident #ty_generics #where_clause {
            const NAME: &'static str = #name;
            const VERSION: u32 = #version;
            const QUEUE: &'static str = #queue;
        }
    })
}
