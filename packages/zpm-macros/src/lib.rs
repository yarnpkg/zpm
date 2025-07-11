extern crate proc_macro;

use std::{collections::HashMap, sync::LazyLock};

use convert_case::{Case, Casing};
use parse_enum::ParseEnumArgs;
use proc_macro2::Span;
use quote::{quote, ToTokens, TokenStreamExt};
use regex::Regex;
use syn::{parse_macro_input, DeriveInput, Expr, Ident, ImplItem, ImplItemFn, Type};

mod helpers;
mod parse_enum;

#[proc_macro_attribute]
pub fn parse_enum(args_tokens: proc_macro::TokenStream, input_tokens: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let mut args = ParseEnumArgs::default();

    let args_parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("or_else") {
            args.or_else = Some(meta.value()?.parse()?);
            Ok(())
        } else {
            Err(meta.error("unsupported tea property"))
        }
    });

    parse_macro_input!(args_tokens with args_parser);

    let ast = parse_macro_input!(input_tokens as DeriveInput);

    parse_enum::parse_enum(args, ast)
        .unwrap_or_else(|err| err.to_compile_error().into())
}

// Turn X<Y> into X::<Y>
fn get_expr_path_from_type(input: &syn::Type) -> proc_macro2::TokenStream {
    let input = quote!{#input};
    let mut iter = input.into_iter().peekable();

    let mut result = proc_macro2::TokenStream::new();

    if let Some(first) = iter.next() {
        result.append(first);

        if let Some(proc_macro2::TokenTree::Punct(punct)) = iter.peek() {
            if punct.as_char() == '<' {
                result.append_all(quote! {::});
            }
        }
    }

    for token in iter {
        result.append(token);
    }

    result
}

#[proc_macro_attribute]
pub fn track_time(_attr: proc_macro::TokenStream, item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let mut item = parse_macro_input!(item);

    let input_fn = match &mut item {
        ImplItem::Fn(item_fn) => item_fn,
        _ => panic!("Invalid item type"),
    };

    let ImplItemFn {sig, block, ..} = &input_fn;
    let fn_name = &sig.ident;

    input_fn.block = syn::parse_quote! { {
        if !crate::config::ENV_CONFIG.enable_timings.value {
            return #block;
        }

        let start = std::time::Instant::now();
        let result = #block;

        let duration = start.elapsed();
        println!("{} took {:?}", stringify!(#fn_name), duration);

        result
    } };

    input_fn.to_token_stream().into()
}

static SLUG_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[^a-zA-Z_]+").unwrap()
});

fn get_ident_from_type(ty: &Type) -> Ident {
    let type_string = ty.to_token_stream().to_string();

    let slug = SLUG_REGEX
        .replace_all(&type_string, "_");

    let slug
        = slug.trim_matches('_');

    // Build a new Ident from the slug
    Ident::new(&slug, Span::call_site())
}

#[proc_macro_attribute]
pub fn yarn_config(_attr: proc_macro::TokenStream, item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(item as DeriveInput);

    let struct_sym = &input.ident;
    let enum_sym = Ident::new(&format!("{}Type", struct_sym), Span::call_site());

    let fields = if let syn::Data::Struct(data_struct) = &input.data {
        &data_struct.fields
    } else {
        return proc_macro::TokenStream::from(quote! {
            compile_error!("env_default can only be used with structs");
        });
    };

    let mut default_functions = vec![];
    let mut new_fields = vec![];
    let mut enum_variants = HashMap::new();

    // map.insert("enableAutoType".to_string(), ProjectSettingsType::BoolField(self.enable_auto_type.clone()))
    let mut extract_stmts = vec![];

    // "enable_auto_type" => Ok(ProjectSettingsType::BoolField(self.enable_auto_type.clone()))
    let mut read_setting_stmts = vec![];

    // "enable_auto_type" => Ok(ProjectSettingsType::BoolField(... parses `value` as a bool ...))
    let mut enum_variants_from_file_string = vec![];

    for field in fields.iter() {
        let primary_name_sym = field.ident.as_ref().unwrap();

        let primary_name_str_cc = primary_name_sym
            .to_string()
            .to_case(Case::Camel);

        // Foo<T>
        let field_ty = &field.ty;
        // Foo::<T>
        let field_ty_path = get_expr_path_from_type(field_ty);
        // Foo_T
        let field_ty_slug_sym = get_ident_from_type(field_ty);

        if !enum_variants.contains_key(&field_ty_slug_sym.to_string()) {
            enum_variants.insert(field_ty_slug_sym.to_string(), (field_ty_slug_sym.clone(), field_ty));
        }

        let mut default_value = None;
        let mut all_names_sym = vec![primary_name_sym.clone()];

        for attr in &field.attrs {
            if attr.path().is_ident("default") {
                if let Ok(value) = attr.parse_args::<Expr>() {
                    default_value = Some(value);
                }
            }

            if attr.path().is_ident("alias") {
                if let Ok(ident) = attr.parse_args::<Ident>() {
                    all_names_sym.push(ident);
                }
            }
        }

        if let Some(default) = default_value {
            let default_func_name_sym = syn::Ident::new(&format!("{}_default_from_env", primary_name_sym), primary_name_sym.span());
            let default_func_name_str = default_func_name_sym.to_string();

            let env_get = all_names_sym.iter()
                .map(|setting_name| {
                    let env_name
                        = format!("YARN_{}", setting_name)
                            .to_uppercase();

                    quote!{std::env::var(#env_name)}
                })
                .reduce(|a, b| {
                    quote!{#a.or_else(|_| #b)}
                })
                .unwrap();

            let default_expr = match &default {
                Expr::Closure(_) => quote! {(#default)(crate::config::CONFIG_PATH.lock().unwrap().as_ref().unwrap())},
                _ => quote! {#default},
            };

            default_functions.push(quote! {
                fn #default_func_name_sym() -> #field_ty {
                    match #env_get {
                        Ok(value) => #field_ty_path::from_file_string(&value).unwrap(),
                        Err(_) => #field_ty_path::new(#default_expr),
                    }
                }
            });

            let aliases = &all_names_sym.iter()
                .skip(1)
                .map(|alias_sym| {
                    let alias_str_cc = alias_sym.to_string()
                        .to_case(Case::Camel);

                    quote! {#[serde(alias = #alias_str_cc)]}
                })
                .collect::<Vec<_>>();

            new_fields.push(quote! {
                #[serde(default = #default_func_name_str)]
                #(#aliases)*
                pub #primary_name_sym: #field_ty,
            });
        } else {
            new_fields.push(quote! {
                #[serde(default)]
                pub #primary_name_sym: #field_ty,
            });
        }

        for name_sym in &all_names_sym {
            let name_str = name_sym.to_string();

            enum_variants_from_file_string.push(quote! {
                #name_str => {
                    use zpm_utils::ToFileString;

                    let parsed = #field_ty_path::from_file_string(&value)
                        .map_err(|_| crate::error::Error::InvalidConfigValue(key.to_string(), value.to_file_string()))?;

                    Ok(#enum_sym::#field_ty_slug_sym(parsed))
                },
            });

            read_setting_stmts.push(quote! {
                #name_str => Ok(#enum_sym::#field_ty_slug_sym(self.#primary_name_sym.clone())),
            });
        }

        extract_stmts.push(quote! {
            map.insert(#primary_name_str_cc.to_string(), #enum_sym::#field_ty_slug_sym(self.#primary_name_sym.clone()));
        });
    }

    let enum_variants_vec = enum_variants.into_values()
        .collect::<Vec<_>>();

    let enum_variants_fields = enum_variants_vec.iter()
        .map(|(ident, ty)| quote! {
            #ident(#ty),
        });

    let enum_variants_to_file_string = enum_variants_vec.iter()
        .map(|(ident, _ty)| quote! {
            #enum_sym::#ident(inner) => inner.to_file_string(),
        });

    let enum_variants_to_human_string = enum_variants_vec.iter()
        .map(|(ident, _ty)| quote! {
            #enum_sym::#ident(inner) => inner.to_print_string(),
        });

    let expanded = quote! {
        #(#default_functions)*

        #[derive(Clone, Debug, serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct #struct_sym {
            pub path: Option<zpm_utils::Path>,

            #(#new_fields)*
        }

        impl #struct_sym {
            pub fn to_btree_map(&self) -> std::collections::BTreeMap<String, #enum_sym> {
                let mut map = std::collections::BTreeMap::new();
                #(#extract_stmts)*
                map
            }
        }

        #[derive(Clone, Debug)]
        pub enum #enum_sym {
            #(#enum_variants_fields)*
        }

        impl #enum_sym {
            pub fn from_file_string(key: &str, value: &str) -> Result<Self, crate::error::Error> {
                match key {
                    #(#enum_variants_from_file_string)*

                    _ => Err(crate::error::Error::ConfigKeyNotFound(key.to_string())),
                }
            }
        }

        impl #struct_sym {
            pub fn get(&self, name: &str) -> Result<#enum_sym, crate::error::Error> {
                match name {
                    #(#read_setting_stmts)*

                    _ => Err(crate::error::Error::ConfigKeyNotFound(name.to_string())),
                }
            }

            pub fn set(&self, name: &str, value: #enum_sym) -> Result<(), crate::error::Error> {
                use zpm_utils::IoResultExt;
                use convert_case::{Casing, Case};

                let config_path = self.path.as_ref()
                    .expect("config path not set");
                let config_text = config_path
                    .fs_read_text()
                    .ok_missing()?
                    .unwrap_or_default();

                let updated_config
                    = zpm_parsers::yaml::update_document_field(
                        &config_text,
                        &name.to_case(Case::Camel),
                        &zpm_parsers::yaml::escape_string(&value.to_file_string())
                    )?;

                config_path
                    .fs_write_text(&updated_config)?;

                Ok(())
            }
        }

        impl zpm_utils::ToFileString for #enum_sym {
            fn to_file_string(&self) -> String {
                match self {
                    #(#enum_variants_to_file_string)*

                    // Needed to workaround a warning when enum_variants_to_human_string is empty
                    _ => unreachable!(),
                }
            }
        }

        impl zpm_utils::ToHumanString for #enum_sym {
            fn to_print_string(&self) -> String {
                match self {
                    #(#enum_variants_to_human_string)*

                    // Needed to workaround a warning when enum_variants_to_human_string is empty
                    _ => unreachable!(),
                }
            }
        }
    };

    // panic!("{:?}", expanded.to_string());
    expanded.into()
}
