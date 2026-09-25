//! `Request` deriveは名前付きフィールドのstruct、単位/名前付きvariantのenumを対象にする。

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Data, DeriveInput, Expr, Field, Fields, GenericArgument, LitStr, Path, PathArguments, Type,
    parse_macro_input,
};

#[proc_macro_derive(Request, attributes(request, validate, schema))]
pub fn derive_request(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[derive(Default)]
struct Rules {
    length: Option<(Option<usize>, Option<usize>)>,
    range: Option<(Option<Expr>, Option<Expr>)>,
    email: bool,
    nested: bool,
    custom: Option<Path>,
    custom_async: Option<Path>,
}

fn parse_rules(attrs: &[syn::Attribute]) -> syn::Result<Rules> {
    let mut rules = Rules::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("validate")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("length") {
                if rules.length.is_some() {
                    return Err(meta.error("duplicate length rule"));
                }
                let mut min = None;
                let mut max = None;
                meta.parse_nested_meta(|arg| {
                    let value: syn::LitInt = arg.value()?.parse()?;
                    if arg.path.is_ident("min") {
                        if min.replace(value.base10_parse()?).is_none() {
                            Ok(())
                        } else {
                            Err(arg.error("duplicate min"))
                        }
                    } else if arg.path.is_ident("max") {
                        if max.replace(value.base10_parse()?).is_none() {
                            Ok(())
                        } else {
                            Err(arg.error("duplicate max"))
                        }
                    } else {
                        Err(arg.error("expected unique min or max"))
                    }
                })?;
                if min.is_none() && max.is_none() {
                    return Err(meta.error("length requires min or max"));
                }
                if min.zip(max).is_some_and(|(a, b)| a > b) {
                    return Err(meta.error("length min exceeds max"));
                }
                rules.length = Some((min, max));
            } else if meta.path.is_ident("range") {
                if rules.range.is_some() {
                    return Err(meta.error("duplicate range rule"));
                }
                let mut min = None;
                let mut max = None;
                meta.parse_nested_meta(|arg| {
                    let value: Expr = arg.value()?.parse()?;
                    if arg.path.is_ident("min") {
                        if min.replace(value).is_none() {
                            Ok(())
                        } else {
                            Err(arg.error("duplicate min"))
                        }
                    } else if arg.path.is_ident("max") {
                        if max.replace(value).is_none() {
                            Ok(())
                        } else {
                            Err(arg.error("duplicate max"))
                        }
                    } else {
                        Err(arg.error("expected unique min or max"))
                    }
                })?;
                if min.is_none() && max.is_none() {
                    return Err(meta.error("range requires min or max"));
                }
                rules.range = Some((min, max));
            } else if meta.path.is_ident("email") {
                if rules.email {
                    return Err(meta.error("duplicate email rule"));
                }
                rules.email = true;
            } else if meta.path.is_ident("nested") {
                if rules.nested {
                    return Err(meta.error("duplicate nested rule"));
                }
                rules.nested = true;
            } else if meta.path.is_ident("custom") {
                if rules.custom.is_some() {
                    return Err(meta.error("duplicate custom rule"));
                }
                rules.custom = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("custom_async") {
                if rules.custom_async.is_some() {
                    return Err(meta.error("duplicate custom_async rule"));
                }
                rules.custom_async = Some(meta.value()?.parse()?);
            } else {
                return Err(meta.error("unknown validation rule"));
            }
            Ok(())
        })?;
    }
    Ok(rules)
}

fn inner<'a>(ty: &'a Type, name: &str) -> Option<&'a Type> {
    let Type::Path(path) = ty else { return None };
    let segment = path.path.segments.last()?;
    if segment.ident != name {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    let Some(GenericArgument::Type(ty)) = args.args.first() else {
        return None;
    };
    Some(ty)
}

fn base_type(mut ty: &Type) -> &Type {
    while let Some(next) = inner(ty, "Option")
        .or_else(|| inner(ty, "Patch"))
        .or_else(|| inner(ty, "Box"))
    {
        ty = next;
    }
    ty
}

fn has_interior_mutability(ty: &Type) -> bool {
    let Type::Path(path) = ty else { return false };
    path.path.segments.iter().any(|segment| {
        matches!(segment.ident.to_string().as_str(), "Cell" | "RefCell" | "Mutex" | "RwLock" | "AtomicBool" | "AtomicUsize")
            || matches!(&segment.arguments, PathArguments::AngleBracketed(args) if args.args.iter().any(|arg| matches!(arg, GenericArgument::Type(ty) if has_interior_mutability(ty))))
    })
}

fn field_name(field: &Field) -> syn::Result<String> {
    let mut rename = None;
    for attr in field.attrs.iter().filter(|a| a.path().is_ident("request")) {
        attr.parse_nested_meta(|meta| {
            if !meta.path.is_ident("rename") {
                return Err(meta.error("expected rename"));
            }
            if rename.is_some() {
                return Err(meta.error("duplicate rename"));
            }
            let lit: LitStr = meta.value()?.parse()?;
            rename = Some(lit.value());
            Ok(())
        })?;
    }
    Ok(rename.unwrap_or_else(|| field.ident.as_ref().unwrap().to_string()))
}

fn description(attrs: &[syn::Attribute]) -> syn::Result<Option<String>> {
    let mut result = None;
    for attr in attrs.iter().filter(|a| a.path().is_ident("schema")) {
        attr.parse_nested_meta(|meta| {
            if !meta.path.is_ident("description") {
                return Err(meta.error("expected description"));
            }
            if result.is_some() {
                return Err(meta.error("duplicate description"));
            }
            let lit: LitStr = meta.value()?.parse()?;
            result = Some(lit.value());
            Ok(())
        })?;
    }
    Ok(result)
}

fn opt_tokens<T: quote::ToTokens>(value: &Option<T>) -> proc_macro2::TokenStream {
    value
        .as_ref()
        .map_or_else(|| quote!(None), |v| quote!(Some(#v)))
}

fn value_ref(
    ty: &Type,
    expr: proc_macro2::TokenStream,
    body: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    if let Some(inside) = inner(ty, "Option") {
        let nested = value_ref(inside, quote!(value), body);
        quote! { if let Some(value) = (#expr).as_ref() { #nested } }
    } else if let Some(inside) = inner(ty, "Patch") {
        let nested = value_ref(inside, quote!(value), body);
        quote! { if let ::kouga_validation::kouga_core::Patch::Value(value) = (#expr) { #nested } }
    } else if let Some(inside) = inner(ty, "Box") {
        let nested = value_ref(inside, quote!(value), body);
        quote! { let value = (#expr).as_ref(); #nested }
    } else {
        quote! { let value = #expr; #body }
    }
}

fn expand(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    if matches!(input.data, Data::Enum(_)) {
        return expand_enum(input);
    }
    let name = input.ident;
    let Data::Struct(data) = input.data else {
        return Err(syn::Error::new_spanned(
            name,
            "Request requires a named-field struct",
        ));
    };
    let Fields::Named(fields) = data.fields else {
        return Err(syn::Error::new_spanned(
            name,
            "Request requires named fields",
        ));
    };
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            "generic Request is not supported",
        ));
    }
    let mut context: Type = syn::parse_quote!(());
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("request")) {
        attr.parse_nested_meta(|meta| {
            if !meta.path.is_ident("context") {
                return Err(meta.error("expected context"));
            }
            context = meta.value()?.parse()?;
            Ok(())
        })?;
    }
    let whole = parse_rules(&input.attrs)?;
    if whole.length.is_some() || whole.range.is_some() || whole.email || whole.nested {
        return Err(syn::Error::new_spanned(
            name,
            "struct validation only supports custom and custom_async",
        ));
    }
    let mut raw_fields = Vec::new();
    let mut construct_fields = Vec::new();
    let mut sync_checks = Vec::new();
    let mut async_checks = Vec::new();
    let mut schemas = Vec::new();
    let mut required = Vec::new();
    let mut patch_fields = Vec::new();
    let mut all_patch = !fields.named.is_empty();
    for field in &fields.named {
        let id = field.ident.as_ref().unwrap();
        let ty = &field.ty;
        if has_interior_mutability(ty) {
            return Err(syn::Error::new_spanned(
                ty,
                "Request fields cannot have interior mutability",
            ));
        }
        let json_name = field_name(field)?;
        let rules = parse_rules(&field.attrs)?;
        let description = description(&field.attrs)?;
        let is_option = inner(ty, "Option").is_some();
        let patch = inner(ty, "Patch");
        if patch.is_none() {
            all_patch = false;
        } else {
            patch_fields.push(quote!(self.#id.is_missing()));
        }
        if !is_option && patch.is_none() {
            required.push(json_name.clone());
        }
        let default = if is_option || patch.is_some() {
            quote!(#[serde(default)])
        } else {
            quote!()
        };
        let rename = quote!(#[serde(rename = #json_name)]);
        raw_fields.push(quote!(#default #rename #id: #ty));
        construct_fields.push(quote!(#id: raw.#id));
        let path = quote!(::kouga_validation::join_path(path, #json_name));
        let base = base_type(ty);
        if rules.length.is_some()
            && inner(base, "Vec").is_none()
            && !matches!(base, Type::Path(p) if p.path.is_ident("String"))
        {
            return Err(syn::Error::new_spanned(ty, "length requires String or Vec"));
        }
        if rules.email && !matches!(base, Type::Path(p) if p.path.is_ident("String")) {
            return Err(syn::Error::new_spanned(ty, "email requires String"));
        }
        if rules.nested && (rules.length.is_some() || rules.range.is_some() || rules.email) {
            return Err(syn::Error::new_spanned(
                ty,
                "nested cannot combine with built-in rules",
            ));
        }
        let mut checks = Vec::new();
        if let Some((min, max)) = &rules.length {
            let min = opt_tokens(min);
            let max = opt_tokens(max);
            let length = if inner(base, "Vec").is_some() {
                quote!(value.len())
            } else {
                quote!(value.chars().count())
            };
            checks.push(quote! { if !(::kouga_validation::Rule::<()>::Length { min: #min, max: #max }).check_length(#length) { errors.push(::kouga_validation::ValidationError::new("length").at(&field_path)); } });
        }
        if let Some((min, max)) = &rules.range {
            let min = opt_tokens(min);
            let max = opt_tokens(max);
            checks.push(quote! { if !(::kouga_validation::Rule::Range { min: #min, max: #max }).check_range(value) { errors.push(::kouga_validation::ValidationError::new("range").at(&field_path)); } });
        }
        if rules.email {
            checks.push(quote! { if !::kouga_validation::Rule::<()>::Email.check_email(value) { errors.push(::kouga_validation::ValidationError::new("email").at(&field_path)); } });
        }
        if let Some(custom) = &rules.custom {
            let arg = if matches!(base, Type::Path(p) if p.path.is_ident("String")) {
                quote!(value.as_str())
            } else if inner(base, "Vec").is_some() {
                quote!(value.as_slice())
            } else {
                quote!(value)
            };
            checks.push(quote! { if let Err(error) = #custom(#arg) { ::kouga_validation::record_error(errors, &field_path, error); } });
        }
        if rules.nested {
            if let Some(item) = inner(base, "Vec") {
                let _ = item;
                checks.push(quote! { for (index, item) in value.iter().enumerate() { if errors.is_full() { break; } ::kouga_validation::DerivedRequest::validate_sync_nested(item, errors, &format!("{field_path}[{index}]"), depth + 1); } });
            } else {
                checks.push(quote! { ::kouga_validation::DerivedRequest::validate_sync_nested(value, errors, &field_path, depth + 1); });
            }
        }
        if !checks.is_empty() {
            let wrapped = value_ref(
                ty,
                quote!(&self.#id),
                quote!(#(if !errors.is_full() { #checks })*),
            );
            sync_checks.push(quote! { if !errors.is_full() { let field_path = #path; #wrapped } });
        }
        let mut async_field_checks = Vec::new();
        if let Some(custom) = &rules.custom_async {
            let arg = if matches!(base, Type::Path(p) if p.path.is_ident("String")) {
                quote!(value.as_str())
            } else if inner(base, "Vec").is_some() {
                quote!(value.as_slice())
            } else {
                quote!(value)
            };
            async_field_checks.push(quote! { if let Err(error) = #custom(#arg, context).await { ::kouga_validation::record_async_error(errors, &field_path, error)?; } });
        }
        if rules.nested {
            if inner(base, "Vec").is_some() {
                async_field_checks.push(quote! { for (index, item) in value.iter().enumerate() { if errors.is_full() { break; } ::kouga_validation::DerivedRequest::validate_async_nested(item, context, errors, &format!("{field_path}[{index}]"), depth + 1).await?; } });
            } else {
                async_field_checks.push(quote! { ::kouga_validation::DerivedRequest::validate_async_nested(value, context, errors, &field_path, depth + 1).await?; });
            }
        }
        if !async_field_checks.is_empty() {
            let wrapped = value_ref(
                ty,
                quote!(&self.#id),
                quote!(#(if !errors.is_full() { #async_field_checks })*),
            );
            async_checks.push(quote! { if !errors.is_full() { let field_path = #path; #wrapped } });
        }
        let schema_ty = patch.unwrap_or(ty);
        let mut schema_rules = Vec::new();
        if let Some((min, max)) = &rules.length {
            let keys = if inner(base, "Vec").is_some() {
                ("minItems", "maxItems")
            } else {
                ("minLength", "maxLength")
            };
            if let Some(min) = min {
                let key = keys.0;
                schema_rules.push(
                    quote!(map.insert(#key.into(), ::kouga_validation::serde_json::json!(#min));),
                );
            }
            if let Some(max) = max {
                let key = keys.1;
                schema_rules.push(
                    quote!(map.insert(#key.into(), ::kouga_validation::serde_json::json!(#max));),
                );
            }
        }
        if let Some((min, max)) = &rules.range {
            if let Some(min) = min {
                schema_rules.push(quote!(map.insert("minimum".into(), ::kouga_validation::serde_json::json!(#min));));
            }
            if let Some(max) = max {
                schema_rules.push(quote!(map.insert("maximum".into(), ::kouga_validation::serde_json::json!(#max));));
            }
        }
        if rules.email {
            schema_rules.push(quote!(map.insert("format".into(), ::kouga_validation::serde_json::json!("email"));));
        }
        if let Some(desc) = description {
            schema_rules.push(quote!(map.insert("description".into(), ::kouga_validation::serde_json::json!(#desc));));
        }
        schemas.push(quote! {
            let mut property = generator.subschema_for::<#schema_ty>();
            let map = property.as_object_mut().expect("object schema");
            #(#schema_rules)*
            properties.insert(#json_name.into(), property.into());
        });
    }
    let whole_sync = whole.custom.map(|custom| quote! { if !errors.is_full() { if let Err(error) = #custom(self) { ::kouga_validation::record_error(errors, path, error); } } });
    let whole_async = whole.custom_async.map(|custom| quote! { if !errors.is_full() { if let Err(error) = #custom(self, context).await { ::kouga_validation::record_async_error(errors, path, error)?; } } });
    let empty_patch = if all_patch {
        quote! { if #(#patch_fields)&&* { errors.push(::kouga_validation::ValidationError::new("empty_patch").at(path)); } }
    } else {
        quote!()
    };
    Ok(quote! {
        impl<'de> ::kouga_validation::serde::Deserialize<'de> for #name {
            fn deserialize<D: ::kouga_validation::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let _depth = ::kouga_validation::DecodeDepth::enter().ok_or_else(||
                    <D::Error as ::kouga_validation::serde::de::Error>::custom("Request nesting exceeds 32")
                )?;
                #[derive(::kouga_validation::serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Raw { #(#raw_fields,)* }
                let raw = <Raw as ::kouga_validation::serde::Deserialize>::deserialize(deserializer)?;
                Ok(Self { #(#construct_fields,)* })
            }
        }
        impl ::kouga_validation::Request for #name {
            type Context = #context;
            fn validate_sync(&self, errors: &mut ::kouga_validation::ValidationErrors) {
                <Self as ::kouga_validation::DerivedRequest>::validate_sync_nested(self, errors, "", 1);
            }
            async fn validate_async<'a>(&'a self, context: &'a Self::Context, errors: &'a mut ::kouga_validation::ValidationErrors) -> Result<(), ::kouga_validation::kouga_core::Error> {
                <Self as ::kouga_validation::DerivedRequest>::validate_async_nested(self, context, errors, "", 1).await
            }
        }
        impl ::kouga_validation::DerivedRequest for #name {
            fn validate_sync_nested(&self, errors: &mut ::kouga_validation::ValidationErrors, path: &str, depth: usize) {
                if ::kouga_validation::depth_error(errors, path, depth) { return; }
                #(#sync_checks)*
                #whole_sync
                #empty_patch
            }
            fn validate_async_nested<'a>(&'a self, context: &'a Self::Context, errors: &'a mut ::kouga_validation::ValidationErrors, path: &'a str, depth: usize) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ::kouga_validation::kouga_core::Error>> + Send + 'a>> {
                Box::pin(async move {
                    if ::kouga_validation::depth_error(errors, path, depth) { return Ok(()); }
                    #(#async_checks)*
                    #whole_async
                    Ok(())
                })
            }
        }
        impl ::kouga_validation::ApiSchema for #name {
            fn schema(generator: &mut ::kouga_validation::schemars::SchemaGenerator, _direction: ::kouga_validation::SchemaDirection) -> ::kouga_validation::schemars::Schema {
                let mut properties = ::kouga_validation::serde_json::Map::new();
                #(#schemas)*
                ::kouga_validation::schemars::Schema::try_from(::kouga_validation::serde_json::json!({
                    "type": "object", "properties": properties,
                    "required": [#(#required),*], "additionalProperties": false
                })).expect("valid Request schema")
            }
        }
        impl ::kouga_validation::schemars::JsonSchema for #name {
            fn schema_name() -> std::borrow::Cow<'static, str> { stringify!(#name).into() }
            fn json_schema(generator: &mut ::kouga_validation::schemars::SchemaGenerator) -> ::kouga_validation::schemars::Schema {
                <Self as ::kouga_validation::ApiSchema>::schema(generator, ::kouga_validation::SchemaDirection::Input)
            }
        }
    })
}

fn enum_field_checks(
    field: &Field,
) -> syn::Result<(proc_macro2::TokenStream, proc_macro2::TokenStream)> {
    let id = field.ident.as_ref().unwrap();
    let ty = &field.ty;
    if has_interior_mutability(ty) {
        return Err(syn::Error::new_spanned(
            ty,
            "Request fields cannot have interior mutability",
        ));
    }
    let name = field_name(field)?;
    let rules = parse_rules(&field.attrs)?;
    let base = base_type(ty);
    if rules.length.is_some()
        && inner(base, "Vec").is_none()
        && !matches!(base, Type::Path(p) if p.path.is_ident("String"))
    {
        return Err(syn::Error::new_spanned(ty, "length requires String or Vec"));
    }
    if rules.email && !matches!(base, Type::Path(p) if p.path.is_ident("String")) {
        return Err(syn::Error::new_spanned(ty, "email requires String"));
    }
    let mut sync = Vec::new();
    let mut asynchronous = Vec::new();
    if let Some((min, max)) = rules.length {
        let min = opt_tokens(&min);
        let max = opt_tokens(&max);
        let len = if inner(base, "Vec").is_some() {
            quote!(value.len())
        } else {
            quote!(value.chars().count())
        };
        sync.push(quote! { if !(::kouga_validation::Rule::<()>::Length { min: #min, max: #max }).check_length(#len) { errors.push(::kouga_validation::ValidationError::new("length").at(&field_path)); } });
    }
    if let Some((min, max)) = rules.range {
        let min = opt_tokens(&min);
        let max = opt_tokens(&max);
        sync.push(quote! { if !(::kouga_validation::Rule::Range { min: #min, max: #max }).check_range(value) { errors.push(::kouga_validation::ValidationError::new("range").at(&field_path)); } });
    }
    if rules.email {
        sync.push(quote! { if !::kouga_validation::Rule::<()>::Email.check_email(value) { errors.push(::kouga_validation::ValidationError::new("email").at(&field_path)); } });
    }
    if let Some(custom) = rules.custom {
        let arg = if matches!(base, Type::Path(p) if p.path.is_ident("String")) {
            quote!(value.as_str())
        } else if inner(base, "Vec").is_some() {
            quote!(value.as_slice())
        } else {
            quote!(value)
        };
        sync.push(quote! { if let Err(error) = #custom(#arg) { ::kouga_validation::record_error(errors, &field_path, error); } });
    }
    if rules.nested {
        if inner(base, "Vec").is_some() {
            sync.push(quote! { for (index, item) in value.iter().enumerate() { if errors.is_full() { break; } ::kouga_validation::DerivedRequest::validate_sync_nested(item, errors, &format!("{field_path}[{index}]"), depth + 1); } });
            asynchronous.push(quote! { for (index, item) in value.iter().enumerate() { if errors.is_full() { break; } ::kouga_validation::DerivedRequest::validate_async_nested(item, context, errors, &format!("{field_path}[{index}]"), depth + 1).await?; } });
        } else {
            sync.push(quote! { ::kouga_validation::DerivedRequest::validate_sync_nested(value, errors, &field_path, depth + 1); });
            asynchronous.push(quote! { ::kouga_validation::DerivedRequest::validate_async_nested(value, context, errors, &field_path, depth + 1).await?; });
        }
    }
    if let Some(custom) = rules.custom_async {
        let arg = if matches!(base, Type::Path(p) if p.path.is_ident("String")) {
            quote!(value.as_str())
        } else if inner(base, "Vec").is_some() {
            quote!(value.as_slice())
        } else {
            quote!(value)
        };
        asynchronous.insert(0, quote! { if let Err(error) = #custom(#arg, context).await { ::kouga_validation::record_async_error(errors, &field_path, error)?; } });
    }
    let sync = value_ref(ty, quote!(#id), quote!(#(if !errors.is_full() { #sync })*));
    let asynchronous = value_ref(
        ty,
        quote!(#id),
        quote!(#(if !errors.is_full() { #asynchronous })*),
    );
    Ok((
        quote! { if !errors.is_full() { let field_path = ::kouga_validation::join_path(path, #name); #sync } },
        quote! { if !errors.is_full() { let field_path = ::kouga_validation::join_path(path, #name); #asynchronous } },
    ))
}

fn expand_enum(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = input.ident;
    let Data::Enum(data) = input.data else {
        unreachable!()
    };
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            "generic Request is not supported",
        ));
    }
    let mut context: Type = syn::parse_quote!(());
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("request")) {
        attr.parse_nested_meta(|meta| {
            if !meta.path.is_ident("context") {
                return Err(meta.error("expected context"));
            }
            context = meta.value()?.parse()?;
            Ok(())
        })?;
    }
    let whole = parse_rules(&input.attrs)?;
    if whole.length.is_some() || whole.range.is_some() || whole.email || whole.nested {
        return Err(syn::Error::new_spanned(
            name,
            "enum validation only supports custom and custom_async",
        ));
    }
    let raw_name = format_ident!("__KougaRaw{name}");
    let mut helpers = Vec::new();
    let mut raw_variants = Vec::new();
    let mut decode_arms = Vec::new();
    let mut sync_arms = Vec::new();
    let mut async_arms = Vec::new();
    let mut schemas = Vec::new();
    for variant in data.variants {
        let variant_id = variant.ident;
        let variant_name = variant_id.to_string();
        match variant.fields {
            Fields::Unit => {
                raw_variants.push(quote!(#variant_id));
                decode_arms.push(quote!(#raw_name::#variant_id => Self::#variant_id));
                sync_arms.push(quote!(Self::#variant_id => {}));
                async_arms.push(quote!(Self::#variant_id => {}));
                schemas.push(quote!(
                    ::kouga_validation::serde_json::json!({"const": #variant_name})
                ));
            }
            Fields::Named(fields) => {
                let helper = format_ident!("__Kouga{name}{variant_id}");
                let fields_raw = fields.named.iter().collect::<Vec<_>>();
                let ids = fields_raw
                    .iter()
                    .map(|f| f.ident.as_ref().unwrap())
                    .collect::<Vec<_>>();
                let mut sync_checks = Vec::new();
                let mut async_checks = Vec::new();
                for field in &fields_raw {
                    let (sync, asynchronous) = enum_field_checks(field)?;
                    sync_checks.push(sync);
                    async_checks.push(asynchronous);
                }
                helpers.push(quote! { #[derive(::kouga_validation::Request)] #[request(context = #context)] struct #helper { #(#fields_raw,)* } });
                raw_variants.push(quote!(#variant_id(#helper)));
                decode_arms.push(quote!(#raw_name::#variant_id(value) => Self::#variant_id { #(#ids: value.#ids,)* }));
                sync_arms.push(quote!(Self::#variant_id { #(#ids,)* } => { let variant_path = ::kouga_validation::join_path(path, #variant_name); let path = variant_path.as_str(); #(#sync_checks)* }));
                async_arms.push(quote!(Self::#variant_id { #(#ids,)* } => { let variant_path = ::kouga_validation::join_path(path, #variant_name); let path = variant_path.as_str(); #(#async_checks)* }));
                schemas.push(quote! { ::kouga_validation::serde_json::json!({
                    "type": "object", "properties": {#variant_name: <#helper as ::kouga_validation::ApiSchema>::schema(generator, ::kouga_validation::SchemaDirection::Input)},
                    "required": [#variant_name], "additionalProperties": false
                }) });
            }
            Fields::Unnamed(fields) => {
                return Err(syn::Error::new_spanned(
                    fields,
                    "tuple Request variants are not supported",
                ));
            }
        }
    }
    let whole_sync = whole.custom.map(|custom| quote! { if !errors.is_full() { if let Err(error) = #custom(self) { ::kouga_validation::record_error(errors, path, error); } } });
    let whole_async = whole.custom_async.map(|custom| quote! { if !errors.is_full() { if let Err(error) = #custom(self, context).await { ::kouga_validation::record_async_error(errors, path, error)?; } } });
    Ok(quote! {
        #(#helpers)*
        #[derive(::kouga_validation::serde::Deserialize)]
        enum #raw_name { #(#raw_variants,)* }
        impl<'de> ::kouga_validation::serde::Deserialize<'de> for #name {
            fn deserialize<D: ::kouga_validation::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let raw = <#raw_name as ::kouga_validation::serde::Deserialize>::deserialize(deserializer)?;
                Ok(match raw { #(#decode_arms,)* })
            }
        }
        impl ::kouga_validation::Request for #name {
            type Context = #context;
            fn validate_sync(&self, errors: &mut ::kouga_validation::ValidationErrors) {
                <Self as ::kouga_validation::DerivedRequest>::validate_sync_nested(self, errors, "", 1);
            }
            async fn validate_async<'a>(&'a self, context: &'a Self::Context, errors: &'a mut ::kouga_validation::ValidationErrors) -> Result<(), ::kouga_validation::kouga_core::Error> {
                <Self as ::kouga_validation::DerivedRequest>::validate_async_nested(self, context, errors, "", 1).await
            }
        }
        impl ::kouga_validation::DerivedRequest for #name {
            fn validate_sync_nested(&self, errors: &mut ::kouga_validation::ValidationErrors, path: &str, depth: usize) {
                if ::kouga_validation::depth_error(errors, path, depth) { return; }
                match self { #(#sync_arms,)* }
                #whole_sync
            }
            fn validate_async_nested<'a>(&'a self, context: &'a Self::Context, errors: &'a mut ::kouga_validation::ValidationErrors, path: &'a str, depth: usize) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ::kouga_validation::kouga_core::Error>> + Send + 'a>> {
                Box::pin(async move {
                    if ::kouga_validation::depth_error(errors, path, depth) { return Ok(()); }
                    match self { #(#async_arms,)* }
                    #whole_async
                    Ok(())
                })
            }
        }
        impl ::kouga_validation::ApiSchema for #name {
            fn schema(generator: &mut ::kouga_validation::schemars::SchemaGenerator, _direction: ::kouga_validation::SchemaDirection) -> ::kouga_validation::schemars::Schema {
                ::kouga_validation::schemars::Schema::try_from(::kouga_validation::serde_json::json!({"oneOf": [#(#schemas),*]})).expect("valid Request enum schema")
            }
        }
        impl ::kouga_validation::schemars::JsonSchema for #name {
            fn schema_name() -> std::borrow::Cow<'static, str> { stringify!(#name).into() }
            fn json_schema(generator: &mut ::kouga_validation::schemars::SchemaGenerator) -> ::kouga_validation::schemars::Schema {
                <Self as ::kouga_validation::ApiSchema>::schema(generator, ::kouga_validation::SchemaDirection::Input)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_attributes_at_expansion() {
        for input in [
            quote!(
                struct Bad {
                    #[validate(lenght(min = 1))]
                    name: String,
                }
            ),
            quote!(
                struct Bad {
                    #[validate(length(min = 3, max = 1))]
                    name: String,
                }
            ),
            quote!(
                struct Bad {
                    #[validate(email)]
                    count: i32,
                }
            ),
            quote!(
                struct Bad {
                    #[request(renamed = "x")]
                    name: String,
                }
            ),
            quote!(
                struct Bad {
                    name: std::cell::Cell<i32>,
                }
            ),
        ] {
            assert!(expand(syn::parse2(input).unwrap()).is_err());
        }
    }
}
