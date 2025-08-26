use core::panic;
use std::collections::{BTreeMap, BTreeSet};

use quote::quote;
use syn::{spanned::Spanned, Data, DeriveInput, Expr, Fields, Meta};

use crate::helpers;

#[derive(Default)]
pub struct ParseEnumArgs {
    pub or_else: Option<Expr>,
}

pub fn parse_enum(args: ParseEnumArgs, ast: DeriveInput) -> Result<proc_macro::TokenStream, syn::Error> {
    let all_attrs = ast.attrs.iter()
        .filter(|attr| !attr.path().is_ident("derive_variants"))
        .cloned()
        .collect::<Vec<_>>();

    let mut derive_variants_attrs = ast.attrs.iter()
        .filter(|attr| attr.path().is_ident("derive_variants"))
        .cloned()
        .collect::<Vec<_>>();

    for derive_variant_attr in derive_variants_attrs.iter_mut() {
        let mut derive_list = derive_variant_attr.meta.require_list()?.clone();
        derive_list.path = syn::Path::from(syn::Ident::new("derive", derive_list.path.span()));
        derive_variant_attr.meta = Meta::List(derive_list);
    }

    let name = &ast.ident;
    let data = match &ast.data {
        Data::Enum(data) => data,
        _ => panic!("Parsed can only be derived for enums"),
    };

    let mut arms = Vec::new();

    let mut generated_structs = Vec::new();
    let mut generated_variants = Vec::new();
    let mut deserialization_arms = Vec::new();

    for variant in &data.variants {
        let variant_ident = &variant.ident;

        // 1. Extracting the fields from the enum variant. We only support named fields (ie `Foo { a: i32, b: i32 }`, not `Foo(i32, i32)`)

        let fields = match &variant.fields {
            Fields::Named(enum_fields) => {
                let mut fields = BTreeMap::new();

                for field in enum_fields.named.iter() {
                    let field_name = field.ident.as_ref().unwrap().to_string();
                    let field_type = field.ty.clone();

                    fields.insert(field_name, field_type);
                }

                Some(fields)
            },

            _ => {
                None
            },
        };

        // 2. Generate a struct with the specified fields

        let struct_ident = syn::Ident::new(&format!("{}{}", variant.ident, name), proc_macro2::Span::call_site());

        if let Some(fields) = &fields {
            let struct_members = fields.iter().map(|(name, ty)| {
                let field_ident = syn::Ident::new(name, proc_macro2::Span::call_site());
                quote! {pub #field_ident: #ty}
            });

            generated_structs.push(quote!{
                #(#derive_variants_attrs)*
                pub struct #struct_ident {
                    #(#struct_members),*
                }

                impl Into<#name> for #struct_ident {
                    fn into(self) -> #name {
                        #name::#variant_ident(self)
                    }
                }
            });
        }

        // 3. Replace the variant with the new struct as only parameter (ie we turn `Foo { a: i32, b: i32 }` into `Foo(FooEnum)`)

        if fields.is_some() {
            generated_variants.push(quote!{
                #variant_ident(#struct_ident)
            });
        } else {
            generated_variants.push(quote!{
                #variant_ident
            });
        }

        // 4. Generate the deserialization code for the variant

        let pattern_attrs = variant.attrs.iter()
            .filter(|attr| attr.path().is_ident("pattern"))
            .collect::<Vec<_>>();

        if pattern_attrs.is_empty() && !variant.attrs.iter().any(|attr| attr.path().is_ident("no_pattern")) {
            panic!("All variants are expected to have either a #[pattern] attribute or #[no_pattern] if it's intended to be manually created");
        }

        for attr in pattern_attrs {
            struct Pattern {
                pattern: Option<String>,
            }

            let mut pattern_info = Pattern {
                pattern: None,
            };

            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("spec") {
                    pattern_info.pattern = helpers::extract_literal(&meta).ok();
                }
                Ok(())
            });

            let pattern = pattern_info.pattern
                .ok_or_else(|| syn::Error::new(attr.span(), "Each pattern must have an attached spec"))?;

            let pattern = format!("^{}$", pattern);

            let regex = regex::Regex::new(&pattern)
                .map_err(|_| syn::Error::new(attr.span(), "Invalid regex pattern"))?;

            let capture_names = regex.capture_names()
                .skip(1)
                .map(|name| name.ok_or(()))
                .collect::<Result<BTreeSet<_>, ()>>()
                .map_err(|_| syn::Error::new(attr.span(), "Named capture groups are required"))?;

            let variant_factory = if capture_names.len() > 0 {
                let fields = fields.as_ref().unwrap();

                let (captured_fields, missing_fields): (Vec<_>, Vec<_>) = fields.iter()
                    .partition(|(name, _)| capture_names.contains(name.as_str()));

                let field_creators = captured_fields.iter().map(|(name, ty)| {
                    let field_ident = syn::Ident::new(name, proc_macro2::Span::call_site());
                    let is_option_type = helpers::extract_type_from_option(ty);

                    match is_option_type {
                        Some(_) => quote!{#field_ident: captures.name(#name).map(|x| x.as_str().try_into().map_err(|_| ())).transpose()?},
                        None => quote!{#field_ident: captures.name(#name).unwrap().as_str().try_into().map_err(|_| ())?},
                    }
                });

                let missing_field_creators = missing_fields.iter().map(|(name, _)| {
                    let field_ident = syn::Ident::new(name, proc_macro2::Span::call_site());
                    quote!{#field_ident: Default::default()}
                });

                quote!{
                    Self::#variant_ident(#struct_ident {
                        #(#field_creators,)*
                        #(#missing_field_creators,)*
                    })
                }
            } else {
                quote!{
                    Self::#variant_ident
                }
            };

            deserialization_arms.push(quote! {{
                static RE: std::sync::LazyLock<regex::Regex>
                    = std::sync::LazyLock::new(|| regex::Regex::new(#pattern).unwrap());

                if let Some(captures) = RE.captures(src) {
                    if let Ok(val) = (|| -> Result<Self, ()> {Ok(#variant_factory)})() {
                        return Ok(val);
                    }
                }
            }});
        }
    }

    if let Some(or_else) = &args.or_else {
        arms.push(quote!{
            return Some(src).map(#or_else).unwrap();
        });
    } else {
        arms.push(quote!{
            panic!("Invalid value: {}", src);
        });
    }

    let expanded = quote! {
        #(#generated_structs)*

        #(#all_attrs)*
        pub enum #name {
            #(#generated_variants),*
        }

        impl zpm_utils::FromFileString for #name {
            type Error = Error;

            fn from_file_string(src: &str) -> Result<Self, Self::Error> {
                #(#deserialization_arms)*
                #(#arms)*
            }
        }
    };

    //panic!("{:?}", expanded.to_string());

    Ok(expanded.into())
}
