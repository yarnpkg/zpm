extern crate proc_macro;

use std::{collections::HashMap, sync::LazyLock};

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
    let struct_name = &input.ident;
    let enum_name = Ident::new(&format!("{}Type", struct_name), Span::call_site());

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
    let mut extract_stmts = vec![];

    for field in fields.iter() {
        let field_name = field.ident.as_ref().unwrap();
        let field_type = &field.ty;
        let field_type_path = get_expr_path_from_type(field_type);

        let mut default_value = None;

        for attr in &field.attrs {
            if attr.path().is_ident("default") {
                if let Ok(value) = attr.parse_args::<Expr>() {
                    default_value = Some(value);
                }
            }
        }

        if let Some(default) = default_value {
            let func_name = syn::Ident::new(&format!("{}_default_from_env", field_name), field_name.span());
            let func_name_str = func_name.to_string();

            default_functions.push(quote! {
                fn #func_name() -> #field_type {
                    use crate::config::FromEnv;

                    match std::env::var(concat!("YARN_", stringify!(#field_name)).to_uppercase()) {
                        Ok(value) => #field_type_path::from_env(&value).unwrap(),
                        Err(_) => #field_type_path::new(#default),
                    }
                }
            });

            new_fields.push(quote! {
                #[serde(default = #func_name_str)]
                pub #field_name: #field_type,
            });
        } else {
            new_fields.push(quote! {
                #[serde(default)]
                pub #field_name: #field_type,
            });
        }

        let enum_variant_ident
            = get_ident_from_type(field_type);

        extract_stmts.push(quote! {
            map.insert(stringify!(#field_name).to_string(), #enum_name::#enum_variant_ident(self.#field_name.clone()));
        });
    
        if !enum_variants.contains_key(&enum_variant_ident.to_string()) {
            enum_variants.insert(enum_variant_ident.to_string(), (enum_variant_ident, field_type));
        }
    }

    let enum_variants_vec = enum_variants.into_values()
        .collect::<Vec<_>>();

    let enum_variants_fields = enum_variants_vec.iter()
        .map(|(ident, ty)| quote! {
            #ident(#ty),
        });

    let enum_variants_to_file_string = enum_variants_vec.iter()
        .map(|(ident, _ty)| quote! {
            #enum_name::#ident(inner) => inner.to_file_string(),
        });

    let enum_variants_to_human_string = enum_variants_vec.iter()
        .map(|(ident, _ty)| quote! {
            #enum_name::#ident(inner) => inner.to_print_string(),
        });

    let expanded = quote! {
        #(#default_functions)*

        #[derive(Clone, Debug, serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct #struct_name {
            #(#new_fields)*
        }

        impl #struct_name {
            pub fn to_btree_map(&self) -> std::collections::BTreeMap<String, #enum_name> {
                let mut map = std::collections::BTreeMap::new();
                #(#extract_stmts)*
                map
            }
        }

        #[derive(Clone, Debug)]
        pub enum #enum_name {
            #(#enum_variants_fields)*
        }

        impl zpm_utils::ToFileString for #enum_name {
            fn to_file_string(&self) -> String {
                match self {
                    #(#enum_variants_to_file_string)*

                    // Needed to workaround a warning when enum_variants_to_human_string is empty
                    _ => unreachable!(),
                }
            }
        }

        impl zpm_utils::ToHumanString for #enum_name {
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
