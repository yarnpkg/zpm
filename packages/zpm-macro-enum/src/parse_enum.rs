use core::panic;
use std::collections::{BTreeMap, BTreeSet};

use quote::quote;
use syn::{spanned::Spanned, Data, DeriveInput, Expr, Fields, Meta, Type};
use zpm_macro_helpers::{extract_literal, extract_type_from_option};

#[derive(Default)]
pub struct ParseEnumArgs {
    pub error: Option<Type>,
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
        let mut derive_list
            = derive_variant_attr.meta.require_list()?.clone();

        derive_list.path = syn::Path::from(syn::Ident::new("derive", derive_list.path.span()));
        derive_variant_attr.meta = Meta::List(derive_list);
    }

    let name
        = &ast.ident;

    let error
        = args.error.unwrap_or(syn::Type::Path(syn::TypePath {
            qself: None,
            path: syn::Path::from(syn::Ident::new("Error", proc_macro2::Span::call_site())),
        }));

    let Data::Enum(data) = &ast.data else {
        panic!("Parsed can only be derived for enums");
    };

    let mut arms
        = Vec::new();

    let mut generated_structs
        = Vec::new();
    let mut generated_variants
        = Vec::new();
    let mut deserialization_arms
        = Vec::new();
    let mut deserialization_literal_arms
        = Vec::new();
    let mut serialization_literal_arms
        = Some(Vec::new());

    // Track fallback variant information (variant_ident, struct_ident, is_tuple, field_name_opt)
    let mut fallback_variant: Option<(syn::Ident, Option<syn::Ident>, bool, Option<String>)> = None;

    for variant in &data.variants {
        let variant_ident
            = &variant.ident;

        // 1. Extracting the fields from the enum variant. We only support named fields (ie `Foo { a: i32, b: i32 }`, not `Foo(i32, i32)`)

        let fields = match &variant.fields {
            Fields::Named(enum_fields) => {
                let mut fields
                    = BTreeMap::new();

                for field in enum_fields.named.iter() {
                    let field_name
                        = field.ident.as_ref().unwrap().to_string();
                    let field_type
                        = field.ty.clone();

                    fields.insert(
                        field_name,
                        field_type,
                    );
                }

                Some(fields)
            },

            _ => {
                None
            },
        };

        // Check for fallback attribute early to determine struct generation
        let has_fallback = variant.attrs.iter()
            .any(|attr| attr.path().is_ident("fallback"));

        // 2. Generate a struct with the specified fields (skip for fallback tuple variants)

        let struct_ident
            = syn::Ident::new(&format!("{}{}", variant.ident, name), proc_macro2::Span::call_site());

        let skip_struct_generation = has_fallback && matches!(&variant.fields, Fields::Unnamed(_));

        if let Some(fields) = &fields {
            if !skip_struct_generation {
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
        }

        // 3. Replace the variant with the new struct as only parameter (ie we turn `Foo { a: i32, b: i32 }` into `Foo(FooEnum)`)
        // But keep tuple variants as-is if they have #[fallback]

        if skip_struct_generation {
            // Keep the original tuple variant for fallback
            generated_variants.push(quote!{
                #variant_ident(String)
            });
        } else if fields.is_some() {
            generated_variants.push(quote!{
                #variant_ident(#struct_ident)
            });
        } else {
            generated_variants.push(quote!{
                #variant_ident
            });
        }

        // 4. Parse fallback attribute

        if has_fallback {
            // Check that fallback variant doesn't have pattern or literal attributes
            if variant.attrs.iter().any(|attr| attr.path().is_ident("pattern") || attr.path().is_ident("literal")) {
                return Err(syn::Error::new(variant.span(), "#[fallback] variant cannot have #[pattern] or #[literal] attributes"));
            }

            // Check if we already have a fallback variant
            if fallback_variant.is_some() {
                return Err(syn::Error::new(variant.span(), "Only one variant can have the #[fallback] attribute"));
            }

            // Handle both tuple and struct variants
            match &variant.fields {
                Fields::Unnamed(unnamed_fields) => {
                    // Tuple variant like `Other(String)`
                    if unnamed_fields.unnamed.len() != 1 {
                        return Err(syn::Error::new(variant.span(), "#[fallback] tuple variant must have exactly one field"));
                    }

                    let field_type = &unnamed_fields.unnamed.first().unwrap().ty;

                    // Check if the field type is String
                    let is_string = match field_type {
                        Type::Path(type_path) => {
                            type_path.path.segments.last()
                                .map(|seg| seg.ident == "String")
                                .unwrap_or(false)
                        }
                        _ => false
                    };

                    if !is_string {
                        return Err(syn::Error::new(variant.span(), "#[fallback] variant field must be of type String"));
                    }

                    fallback_variant = Some((variant_ident.clone(), None, true, None));
                }
                Fields::Named(_) => {
                    // Struct variant like `Other { value: String }`
                    let fields = fields.as_ref()
                        .ok_or_else(|| syn::Error::new(variant.span(), "#[fallback] struct variant must have fields"))?;

                    if fields.len() != 1 {
                        return Err(syn::Error::new(variant.span(), "#[fallback] struct variant must have exactly one field"));
                    }

                    let (field_name, field_type) = fields.iter().next().unwrap();

                    // Check if the field type is String
                    let is_string = match field_type {
                        Type::Path(type_path) => {
                            type_path.path.segments.last()
                                .map(|seg| seg.ident == "String")
                                .unwrap_or(false)
                        }
                        _ => false
                    };

                    if !is_string {
                        return Err(syn::Error::new(variant.span(), "#[fallback] variant field must be of type String"));
                    }

                    let struct_ident = syn::Ident::new(&format!("{}{}", variant.ident, name), proc_macro2::Span::call_site());
                    fallback_variant = Some((variant_ident.clone(), Some(struct_ident), false, Some(field_name.clone())));
                }
                Fields::Unit => {
                    return Err(syn::Error::new(variant.span(), "#[fallback] variant must have a String field"));
                }
            }
        }

        // 5. Parse literal attributes

        let literal_attrs = variant.attrs.iter()
            .filter(|attr| attr.path().is_ident("literal"))
            .collect::<Vec<_>>();

        let mut is_first_literal
            = true;

        for attr in &literal_attrs {
            // parse #[literal("...")] into a string
            let meta_list = attr.meta.require_list()?;
            let tokens = &meta_list.tokens;
            let lit_str: syn::LitStr = syn::parse2(tokens.clone())
                .map_err(|_| syn::Error::new(attr.span(), "Expected a string literal in #[literal(\"...\")]"))?;
            let literal_value = lit_str.value();

            if is_first_literal {
                if let Some(serialization_literal_arms) = serialization_literal_arms.as_mut() {
                    serialization_literal_arms.push(quote! {
                        Self::#variant_ident => #literal_value.to_string(),
                    });
                }
            }

            if fields.is_some() {
                deserialization_literal_arms.push(quote! {
                    #literal_value => return Ok(Self::#variant_ident(Default::default())),
                });
            } else {
                deserialization_literal_arms.push(quote! {
                    #literal_value => return Ok(Self::#variant_ident),
                });
            }

            is_first_literal = false;
        }

        // Only disable ToFileString generation if there's no literal AND no fallback
        if is_first_literal && !has_fallback {
            serialization_literal_arms = None;
        }

        // 6. Generate the deserialization code for the variant

        let pattern_attrs = variant.attrs.iter()
            .filter(|attr| attr.path().is_ident("pattern"))
            .collect::<Vec<_>>();

        if pattern_attrs.is_empty() && !variant.attrs.iter().any(|attr| attr.path().is_ident("no_pattern") || attr.path().is_ident("literal") || attr.path().is_ident("fallback")) {
            panic!("All variants are expected to have either #[pattern] / #[literal] / #[no_pattern] / #[fallback] attributes");
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
                    pattern_info.pattern = extract_literal(&meta).ok();
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
                let fields
                    = fields.as_ref().unwrap();

                let (captured_fields, missing_fields): (Vec<_>, Vec<_>) = fields.iter()
                    .partition(|(name, _)| capture_names.contains(name.as_str()));

                let field_creators = captured_fields.iter().map(|(name, ty)| {
                    let field_ident
                        = syn::Ident::new(name, proc_macro2::Span::call_site());
                    let is_option_type
                        = extract_type_from_option(ty);

                    match is_option_type {
                        Some(_) => quote!{#field_ident: captures.name(#name).map(|x| zpm_utils::FromFileString::from_file_string(x.as_str()).map_err(|_| ())).transpose()?},
                        None => quote!{#field_ident: zpm_utils::FromFileString::from_file_string(captures.name(#name).unwrap().as_str()).map_err(|_| ())?},
                    }
                });

                let missing_field_creators = missing_fields.iter().map(|(name, _)| {
                    let field_ident
                        = syn::Ident::new(name, proc_macro2::Span::call_site());

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

    if let Some((variant_ident, struct_ident_opt, is_tuple, field_name_opt)) = &fallback_variant {
        // If we have a fallback variant, use it for unrecognized strings
        let variant_constructor = if *is_tuple {
            // Tuple variant: Other(src.to_string())
            quote!{
                Self::#variant_ident(src.to_string())
            }
        } else {
            // Struct variant: Other(OtherEnum { field: src.to_string() })
            let struct_ident = struct_ident_opt.as_ref().unwrap();
            let field_name = field_name_opt.as_ref().unwrap();
            let field_ident = syn::Ident::new(field_name, proc_macro2::Span::call_site());
            quote!{
                Self::#variant_ident(#struct_ident {
                    #field_ident: src.to_string(),
                })
            }
        };

        arms.push(quote!{
            return Ok(#variant_constructor);
        });
    } else if let Some(or_else) = &args.or_else {
        arms.push(quote!{
            return Some(src).map(#or_else).unwrap();
        });
    } else {
        arms.push(quote!{
            panic!("Invalid value: {}", src);
        });
    }

    // Generate ToFileString implementation if all variants have literals or there's a fallback

    let to_file_string_impl = if let Some(mut serialization_literal_arms) = serialization_literal_arms {
        // Add fallback arm for ToFileString if present
        if let Some((variant_ident, struct_ident_opt, is_tuple, field_name_opt)) = &fallback_variant {
            let fallback_arm = if *is_tuple {
                // Tuple variant: extract the string directly
                quote! {
                    Self::#variant_ident(value) => value.clone(),
                }
            } else {
                // Struct variant: extract the string from the field
                let struct_ident = struct_ident_opt.as_ref().unwrap();
                let field_name = field_name_opt.as_ref().unwrap();
                let field_ident = syn::Ident::new(field_name, proc_macro2::Span::call_site());
                quote! {
                    Self::#variant_ident(#struct_ident { #field_ident, .. }) => #field_ident.clone(),
                }
            };
            serialization_literal_arms.push(fallback_arm);
        }

        quote! {
            impl zpm_utils::ToFileString for #name {
                fn to_file_string(&self) -> String {
                    match self {
                        #(#serialization_literal_arms)*
                    }
                }
            }

            impl zpm_utils::ToHumanString for #name {
                fn to_print_string(&self) -> String {
                    use zpm_utils::ToFileString;

                    zpm_utils::DataType::Code.colorize(&self.to_file_string())
                }
            }

            zpm_utils::impl_file_string_from_str!(#name);
            zpm_utils::impl_file_string_serialization!(#name);
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        #(#generated_structs)*

        #(#all_attrs)*
        pub enum #name {
            #(#generated_variants),*
        }

        impl zpm_utils::FromFileString for #name {
            type Error = #error;

            fn from_file_string(src: &str) -> Result<Self, Self::Error> {
                // First try literal matching for quick shortcuts
                match src {
                    #(#deserialization_literal_arms)*
                    _ => {}
                }

                // Then try pattern matching
                #(#deserialization_arms)*
                #(#arms)*
            }
        }

        #to_file_string_impl
    };

    //panic!("{:?}", expanded.to_string());

    Ok(expanded.into())
}
