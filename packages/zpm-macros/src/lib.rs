extern crate proc_macro;

use quote::{format_ident, quote};
use syn::{meta::ParseNestedMeta, parse_macro_input, Data, DeriveInput, ItemFn, Meta};

#[proc_macro_attribute]
pub fn track_time(_attr: proc_macro::TokenStream, item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);

    // Decompose the input function to inspect its components
    let ItemFn {
        attrs,
        vis,
        sig,
        block,
    } = input_fn;

    let fn_name = &sig.ident; // Get the function name
    let is_async = sig.asyncness.is_some();
    let result_var = format_ident!("result");

    let exec_time_log = quote! {
        let duration = start.elapsed();
        println!("{} took {:?}", stringify!(#fn_name), duration);
    };

    // Apply different logic based on the async attribute
    let output = if is_async {
        quote! {
            #(#attrs)* #vis #sig {
                let start = std::time::Instant::now();
                let #result_var = (|| async #block)().await;
                #exec_time_log
                #result_var
            }
        }
    } else {
        quote! {
            #(#attrs)* #vis #sig {
                let start = std::time::Instant::now();
                let #result_var = (|| #block)();
                #exec_time_log
                #result_var
            }
        }
    };

    output.into()
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

#[proc_macro_derive(Parsed, attributes(parse_error, try_from_str, try_pattern))]
pub fn parsed_derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);

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
        variants.extend(variant.attrs.iter().filter_map(|attr| {
            attr.path().is_ident("try_pattern").then(|| {
                let mut variant_info = Variant {
                    ident: variant.ident.clone(),
                    prefix: None,
                    pattern: None,
                    optional_prefix: false,
                    field_count: variant.fields.len(),
                };
        
                if let Ok(_) = attr.parse_nested_meta(|meta| {
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
                }) {}

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
            })
        }));
    }

    for variant in &variants {
        let variant_name = &variant.ident;

        let enum_args = if let Some(pattern) = &variant.pattern {
            let captures_len = regex::Regex::new(&pattern)
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
                static RE: once_cell::sync::Lazy<regex::Regex>
                    = once_cell::sync::Lazy::new(|| regex::Regex::new(#pattern).unwrap());

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
