extern crate proc_macro;

use parse_enum::ParseEnumArgs;
use quote::{quote, ToTokens, TokenStreamExt};
use syn::{parse_macro_input, DeriveInput, Expr, ImplItem, ImplItemFn};

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

#[proc_macro_attribute]
pub fn yarn_config(_attr: proc_macro::TokenStream, item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let struct_name = &input.ident;

    let fields = if let syn::Data::Struct(data_struct) = &input.data {
        &data_struct.fields
    } else {
        return proc_macro::TokenStream::from(quote! {
            compile_error!("env_default can only be used with structs");
        });
    };

    let mut default_functions = vec![];
    let mut new_fields = vec![];

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
    }

    let expanded = quote! {
        #(#default_functions)*

        #[derive(Clone, Debug, serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct #struct_name {
            #(#new_fields)*
        }
    };

    // panic!("{:?}", expanded.to_string());
    expanded.into()
}
