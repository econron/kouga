use heck::ToSnakeCase;
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Data, DeriveInput, Fields, GenericArgument, Ident, LitStr, PathArguments, Type, Visibility,
    parse_macro_input,
};

#[proc_macro_derive(Model, attributes(model))]
pub fn derive_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

struct ModelField {
    ident: Ident,
    ty: Type,
    column: LitStr,
    default: bool,
    system: bool,
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && !value.as_bytes()[0].is_ascii_digit()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn last_type_name(ty: &Type) -> Option<&Ident> {
    let Type::Path(path) = ty else { return None };
    Some(&path.path.segments.last()?.ident)
}

fn utc_datetime(ty: &Type) -> bool {
    let Type::Path(path) = ty else { return false };
    let Some(segment) = path.path.segments.last() else {
        return false;
    };
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return false;
    };
    segment.ident == "DateTime"
        && matches!(args.args.first(), Some(GenericArgument::Type(inner)) if last_type_name(inner).is_some_and(|name| name == "Utc"))
}

fn parse_field(field: &syn::Field) -> syn::Result<ModelField> {
    let ident = field
        .ident
        .clone()
        .ok_or_else(|| syn::Error::new_spanned(field, "named fields required"))?;
    let rust_name = ident.to_string();
    let rust_name = rust_name.strip_prefix("r#").unwrap_or(&rust_name);
    let mut column = None;
    let mut default = false;
    for attr in field.attrs.iter().filter(|a| a.path().is_ident("model")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("default") {
                if default {
                    return Err(meta.error("duplicate default"));
                }
                default = true;
            } else if meta.path.is_ident("column") {
                let value: LitStr = meta.value()?.parse()?;
                if column.replace(value).is_some() {
                    return Err(meta.error("duplicate column"));
                }
            } else {
                return Err(meta.error("expected default or column"));
            }
            Ok(())
        })?;
    }
    let column = column.unwrap_or_else(|| LitStr::new(rust_name, ident.span()));
    if !valid_identifier(&column.value()) {
        return Err(syn::Error::new_spanned(column, "invalid SQL column name"));
    }
    let system = matches!(rust_name, "id" | "created_at" | "updated_at");
    if system && (column.value() != rust_name || default) {
        return Err(syn::Error::new_spanned(
            &ident,
            "id and timestamp columns cannot be renamed or default-marked",
        ));
    }
    if !system && matches!(column.value().as_str(), "id" | "created_at" | "updated_at") {
        return Err(syn::Error::new_spanned(
            &column,
            "system columns cannot be writable attributes",
        ));
    }
    if rust_name == "id" && last_type_name(&field.ty).is_none_or(|name| name != "Uuid") {
        return Err(syn::Error::new_spanned(&field.ty, "id must have UUID type"));
    }
    if matches!(rust_name, "created_at" | "updated_at") && !utc_datetime(&field.ty) {
        return Err(syn::Error::new_spanned(
            &field.ty,
            "timestamps must use DateTime<Utc>",
        ));
    }
    Ok(ModelField {
        ident,
        ty: field.ty.clone(),
        column,
        default,
        system,
    })
}

fn expand(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "generic models are unsupported",
        ));
    }
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "Model requires a struct",
        ));
    };
    let Fields::Named(named) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &data.fields,
            "Model requires named fields",
        ));
    };
    let mut table = None;
    let mut module = None;
    let mut crud_visibility = None;
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("model")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("table") {
                let value: LitStr = meta.value()?.parse()?;
                if table.replace(value).is_some() {
                    return Err(meta.error("duplicate table"));
                }
            } else if meta.path.is_ident("module") {
                let value: Ident = meta.value()?.parse()?;
                if module.replace(value).is_some() {
                    return Err(meta.error("duplicate module"));
                }
            } else if meta.path.is_ident("crud_visibility") {
                let value: LitStr = meta.value()?.parse()?;
                let parsed = if value.value() == "private" {
                    Visibility::Inherited
                } else {
                    syn::parse_str::<Visibility>(&value.value())?
                };
                if crud_visibility.replace(parsed).is_some() {
                    return Err(meta.error("duplicate crud_visibility"));
                }
            } else {
                return Err(meta.error("expected table, module, or crud_visibility"));
            }
            Ok(())
        })?;
    }
    let table = table.ok_or_else(|| {
        syn::Error::new_spanned(&input.ident, "Model requires #[model(table = \"...\")]")
    })?;
    if !valid_identifier(&table.value()) {
        return Err(syn::Error::new_spanned(&table, "invalid SQL table name"));
    }
    let fields = named
        .named
        .iter()
        .map(parse_field)
        .collect::<syn::Result<Vec<_>>>()?;
    if fields.iter().filter(|f| f.ident == "id").count() != 1 {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "Model requires one id: Uuid field",
        ));
    }
    for (i, field) in fields.iter().enumerate() {
        if fields[..i]
            .iter()
            .any(|other| other.column.value() == field.column.value())
        {
            return Err(syn::Error::new_spanned(
                &field.column,
                "duplicate SQL column",
            ));
        }
    }
    let name = &input.ident;
    let model_visibility = &input.vis;
    let crud_visibility = crud_visibility.unwrap_or_else(|| input.vis.clone());
    let module = module.unwrap_or_else(|| format_ident!("{}", name.to_string().to_snake_case()));
    let new_name = format_ident!("New{name}");
    let update_name = format_ident!("Update{name}");
    let columns = fields.iter().map(|field| &field.column).collect::<Vec<_>>();
    let column_names = fields.iter().map(|field| &field.ident).collect::<Vec<_>>();
    let column_types = fields.iter().map(|field| &field.ty).collect::<Vec<_>>();
    let row_reads = fields.iter().map(|field| {
        let ident = &field.ident;
        let column = &field.column;
        quote!(#ident: ::kouga_model::sqlx::Row::try_get(row, #column)? )
    });
    let writable = fields.iter().filter(|f| !f.system).collect::<Vec<_>>();
    let new_fields = writable.iter().map(|field| {
        let ident = &field.ident;
        let ty = &field.ty;
        if field.default {
            quote!(pub #ident: ::std::option::Option<#ty>)
        } else {
            quote!(pub #ident: #ty)
        }
    });
    let update_fields = writable.iter().map(|field| {
        let ident = &field.ident;
        let ty = &field.ty;
        quote!(pub #ident: ::kouga_model::core::Patch<#ty>)
    });
    let new_push = writable.iter().map(|field| {
        let ident = &field.ident;
        if field.default {
            quote!(if let ::std::option::Option::Some(value) = attrs.#ident { fields.push(::kouga_model::Field::new(#module::columns::#ident, value)); })
        } else {
            quote!(fields.push(::kouga_model::Field::new(#module::columns::#ident, attrs.#ident));)
        }
    });
    let update_push = writable.iter().map(|field| {
        let ident = &field.ident;
        quote!(if let ::kouga_model::core::Patch::Value(value) = attrs.#ident { fields.push(::kouga_model::Field::new(#module::columns::#ident, value)); })
    });
    Ok(quote! {
        impl<'r> ::kouga_model::sqlx::FromRow<'r, ::kouga_model::sqlx::postgres::PgRow> for #name {
            fn from_row(row: &'r ::kouga_model::sqlx::postgres::PgRow) -> Result<Self, ::kouga_model::sqlx::Error> {
                Ok(Self { #(#row_reads),* })
            }
        }
        impl ::kouga_model::Model for #name {
            const TABLE: &'static str = #table;
            const COLUMNS: &'static [&'static str] = &[#(#columns),*];
        }
        #[allow(dead_code)]
        #model_visibility mod #module {
            #[allow(non_upper_case_globals)]
            pub mod columns {
                use super::super::*;
                #(pub const #column_names: ::kouga_model::Column<#name, #column_types> = ::kouga_model::Column::new(#columns);)*
            }
            pub mod relations {}
        }
        #[allow(dead_code)]
        #crud_visibility struct #new_name { #(#new_fields,)* }
        #[derive(Default)]
        #[allow(dead_code)]
        #crud_visibility struct #update_name { #(#update_fields,)* }
        #[allow(dead_code)]
        impl #name {
            #crud_visibility fn query() -> ::kouga_model::Query<Self> { ::kouga_model::Query::new() }
            #crud_visibility async fn find<'c, A>(db: A, id: ::kouga_model::Uuid) -> Result<Option<Self>, ::kouga_model::db::DbError>
            where A: ::kouga_model::db::Acquire<'c, Database = ::kouga_model::db::Postgres> + Send {
                ::kouga_model::find::<Self, A>(db, id).await
            }
            #crud_visibility async fn create<'c, A>(db: A, attrs: #new_name) -> Result<Self, ::kouga_model::db::DbError>
            where A: ::kouga_model::db::Acquire<'c, Database = ::kouga_model::db::Postgres> + Send {
                Self::create_with_id(db, ::kouga_model::Uuid::new_v4(), attrs).await
            }
            #crud_visibility async fn create_with_id<'c, A>(db: A, id: ::kouga_model::Uuid, attrs: #new_name) -> Result<Self, ::kouga_model::db::DbError>
            where A: ::kouga_model::db::Acquire<'c, Database = ::kouga_model::db::Postgres> + Send {
                let mut fields = ::std::vec::Vec::new();
                #(#new_push)*
                ::kouga_model::create::<Self, A>(db, id, fields).await
            }
            #crud_visibility async fn update<'c, A>(db: A, id: ::kouga_model::Uuid, attrs: #update_name) -> Result<Option<Self>, ::kouga_model::db::DbError>
            where A: ::kouga_model::db::Acquire<'c, Database = ::kouga_model::db::Postgres> + Send {
                let mut fields = ::std::vec::Vec::new();
                #(#update_push)*
                ::kouga_model::update::<Self, A>(db, id, fields).await
            }
            #crud_visibility async fn delete<'c, A>(db: A, id: ::kouga_model::Uuid) -> Result<bool, ::kouga_model::db::DbError>
            where A: ::kouga_model::db::Acquire<'c, Database = ::kouga_model::db::Postgres> + Send {
                ::kouga_model::delete::<Self, A>(db, id).await
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn rejects_bad_model_declarations() {
        assert!(
            expand(parse_quote!(
                struct Missing {
                    id: u32,
                }
            ))
            .is_err()
        );
        assert!(
            expand(parse_quote!(
                #[model(table = "bad;table")]
                struct Bad {
                    id: Uuid,
                }
            ))
            .is_err()
        );
        assert!(
            expand(parse_quote!(
                #[model(table = "tasks")]
                enum Bad {
                    A,
                }
            ))
            .is_err()
        );
        assert!(
            expand(parse_quote!(
                #[model(table = "tasks")]
                struct Bad {
                    id: i32,
                }
            ))
            .is_err()
        );
        assert!(
            expand(parse_quote!(
                #[model(table = "tasks")]
                struct Bad {
                    id: Uuid,
                    created_at: DateTime<Local>,
                }
            ))
            .is_err()
        );
        assert!(
            expand(parse_quote!(
                #[model(table = "tasks")]
                struct Bad {
                    id: Uuid,
                    #[model(column = "id")]
                    title: String,
                }
            ))
            .is_err()
        );
        assert!(
            expand(parse_quote!(
                #[model(table = "tasks")]
                struct Bad {
                    id: Uuid,
                    #[model(column = "updated_at")]
                    title: String,
                }
            ))
            .is_err()
        );
    }
}
