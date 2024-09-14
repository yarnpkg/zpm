extern crate proc_macro;

use quote::{quote, ToTokens, TokenStreamExt};
use syn::{meta::ParseNestedMeta, parse_macro_input, Data, DeriveInput, Expr, ImplItem, ImplItemFn, Meta};

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

fn extract_literal(meta: &ParseNestedMeta) -> syn::Result<String> {
    let expr: syn::Expr = meta.value()?.parse()?;
    let value = &expr;

    if let syn::Expr::Lit(syn::ExprLit {
        lit: syn::Lit::Str(lit),
        attrs: _,
    }) = value {
        return Ok(lit.value());
    }

    panic!("Invalid syntax")
}

fn extract_bool(meta: &ParseNestedMeta) -> syn::Result<bool> {
    let expr: syn::Expr = meta.value()?.parse()?;
    let value = &expr;

    if let syn::Expr::Lit(syn::ExprLit {
        lit: syn::Lit::Bool(lit),
        attrs: _,
    }) = value {
        return Ok(lit.value);
    }

    panic!("Invalid syntax")
}

#[proc_macro_derive(Parsed, attributes(parse_error, parse_fallback, try_from_str, try_pattern))]
pub fn parsed_derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);

    let fallback = ast.attrs.iter().find_map(|attr| {
        if let Meta::List(list) = &attr.meta {
            if list.path.is_ident("parse_fallback") {
                return Some(list.tokens.clone());
            }
        }
        None
    });

    let user_error = ast.attrs.iter().find_map(|attr| {
        if let Meta::List(list) = &attr.meta {
            if list.path.is_ident("parse_error") {
                return Some(list.tokens.clone());
            }
        }
        None
    });

    let error_value = user_error.unwrap_or(quote!{
    });

    let name = &ast.ident;
    let data = match &ast.data {
        Data::Enum(data) => data,
        _ => panic!("Parsed can only be derived for enums"),
    };

    #[derive(Debug)]
    struct Variant {
        ident: syn::Ident,
        prefix: Option<String>,
        pattern: Option<String>,
        optional_prefix: bool,
        field_count: usize,
    }

    let mut variants: Vec<Variant> = Vec::new();
    let mut arms = Vec::new();

    for variant in &data.variants {
        variants.extend(variant.attrs.iter().filter(|attr| attr.path().is_ident("try_pattern")).map(|attr| {
            let mut variant_info = Variant {
                ident: variant.ident.clone(),
                prefix: None,
                pattern: None,
                optional_prefix: false,
                field_count: variant.fields.len(),
            };
    
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("prefix") {
                    variant_info.prefix = extract_literal(&meta).ok();
                }

                if meta.path.is_ident("pattern") {
                    variant_info.pattern = extract_literal(&meta).ok();
                }

                if meta.path.is_ident("optional_prefix") {
                    variant_info.optional_prefix = extract_bool(&meta).unwrap_or_default();
                }

                Ok(())
            });

            // For some reason, performing the starts_with check is twice slower than doing
            // the regex match during resolution. So we just bake the prefix into the pattern.
            if let Some(prefix) = variant_info.prefix.take() {
                let prefix_part = if variant_info.optional_prefix {
                    format!("(?:{})?", regex::escape(&prefix))
                } else {
                    regex::escape(&prefix)
                };

                let pattern_part = if let Some(pattern) = variant_info.pattern.take() {
                    pattern
                } else {
                    "(.*)".to_string()
                };

                variant_info.pattern = Some(format!("{}{}", prefix_part, pattern_part));
            }

            if let Some(pattern) = variant_info.pattern.take() {
                variant_info.pattern = Some(format!("^{}$", pattern));
            }

            variant_info
        }));
    }

    for variant in &variants {
        let variant_name = &variant.ident;

        let enum_args = if let Some(pattern) = &variant.pattern {
            let captures_len = regex::Regex::new(pattern)
                .unwrap()
                .captures_len();

            (1..captures_len).map(|index| quote! {
                captures.get(#index).unwrap().as_str().try_into().map_err(|_| ())?
            }).collect::<Vec<_>>()
        } else if variant.field_count > 0 {
            vec![quote!{ src.try_into().map_err(|_| ())? }]
        } else {
            vec![]
        };

        let mut arm = quote! {
            if let Ok(val) = (|| -> Result<Self, ()> { Ok(Self::#variant_name(#(#enum_args),*)) })() {
                return Ok(val);
            }
        };

        if let Some(pattern) = &variant.pattern {
            arm = quote! {
                static RE: std::sync::LazyLock<regex::Regex>
                    = std::sync::LazyLock::new(|| regex::Regex::new(#pattern).unwrap());

                if let Some(captures) = RE.captures(src) {
                    #arm
                }
            };
        }

        if let Some(prefix) = &variant.prefix {
            if variant.optional_prefix {
                arm = quote! {
                    if src.starts_with(#prefix) {
                        let src = &src[#prefix.len()..];
                        #arm
                    } else {
                        #arm
                    }
                };
            } else {
                arm = quote! {
                    if src.starts_with(#prefix) {
                        let src = &src[#prefix.len()..];
                        #arm
                    }
                };
            }
        }

        arms.push(quote! { {
            #arm
        } });
    }

    if let Some(fallback) = fallback {
        arms.push(quote! {
            return Ok(#fallback);
        });
    }

    let expanded = quote! {
        crate::yarn_serialization_protocol!(#name, {
            deserialize(src) {
                #(#arms)*
                Err(#error_value(src.to_string()))
            }
        });
    };

    //panic!("{:?}", expanded.to_string());

    expanded.into()
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
