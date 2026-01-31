use core::panic;
use std::collections::{BTreeMap, BTreeSet};

use quote::quote;
use syn::{Attribute, Data, DeriveInput, Expr, Fields, Ident, Meta, Type, Variant, parse_quote, parse_quote_spanned, spanned::Spanned};
use zpm_macro_helpers::{AttributeBag, extract_type_from_option};

struct FieldInfo {
    ty: Type,
    attrs: Vec<Attribute>,
}

enum VariantType {
    Struct(Ident, BTreeMap<Ident, FieldInfo>),
    Empty,
}

fn extract_variant_type(enum_name: &Ident, variant: &Variant) -> Result<VariantType, syn::Error> {
    match &variant.fields {
        Fields::Named(enum_fields) => {
            let struct_name
                = syn::Ident::new(&format!("{}{}", variant.ident, enum_name), variant.ident.span());

            let fields
                = enum_fields.named.iter()
                    .map(|field| (
                        field.ident.as_ref().unwrap().clone(),
                        FieldInfo {
                            ty: field.ty.clone(),
                            attrs: field.attrs.clone(),
                        }
                    ))
                    .collect::<BTreeMap<_, _>>();

            Ok(VariantType::Struct(struct_name, fields))
        },

        Fields::Unnamed(_) => {
            return Err(syn::Error::new(variant.span(), "Tuple variants are not supported"));
        },

        Fields::Unit => {
            Ok(VariantType::Empty)
        },
    }
}

fn make_value_hydrater(string_repr: &Expr, ty: &Type) -> proc_macro2::TokenStream {
    let option_type
        = extract_type_from_option(ty);

    match option_type {
        Some(_) => quote!{
            #string_repr
                .map(|x| zpm_utils::FromFileString::from_file_string(x.as_str()).map_err(|_| ()))
                .transpose()?
        },

        None => quote!{
            zpm_utils::FromFileString::from_file_string(#string_repr.unwrap().as_str())
                .map_err(|_| ())?
        },
    }
}

fn make_pattern_factory(capture_expr: &Expr, pattern: &Expr, variant_name: &Ident, variant_type: &VariantType) -> Result<proc_macro2::TokenStream, syn::Error> {
    let Expr::Lit(syn::ExprLit {
        lit: syn::Lit::Str(pattern_lit),
        attrs: _,
    }) = pattern else {
        return Err(syn::Error::new(pattern.span(), "Expected a string literal in #[pattern(\"...\")]"));
    };

    let pattern
        = format!("^{}$", pattern_lit.value());

    let regex = regex::Regex::new(&pattern)
        .map_err(|_| syn::Error::new(pattern.span(), "Invalid regex pattern"))?;

    let capture_names = regex.capture_names()
        .skip(1)
        .map(|name| name.ok_or(syn::Error::new(pattern.span(), "Capture groups must be named")))
        .collect::<Result<BTreeSet<_>, _>>()?;

    match variant_type {
        VariantType::Empty => {
            return Ok(quote!{
                Self::#variant_name
            });
        },

        VariantType::Struct(struct_name, fields) => {
            let (captured_fields, missing_fields): (Vec<_>, Vec<_>)
                = fields.iter()
                    .partition(|(name, _)| capture_names.contains(name.to_string().as_str()));

            let field_creators = captured_fields.iter().copied().map(|(field_name, field_info)| {
                let field_name_str: String
                    = field_name.to_string();

                let field_name_str_expr: Expr
                    = parse_quote_spanned!(pattern.span() => #field_name_str);
                let field_capture: Expr
                    = parse_quote!(#capture_expr.name(#field_name_str_expr));

                let field_hydrater
                    = make_value_hydrater(&field_capture, &field_info.ty);

                quote!{#field_name: #field_hydrater}
            });

            let missing_field_creators
                = missing_fields.iter().map(|(field_name, _)| {
                    quote!{#field_name: Default::default()}
                });

            Ok(quote!{
                Self::#variant_name(#struct_name {
                    #(#field_creators,)*
                    #(#missing_field_creators,)*
                })
            })
        },
    }
}

#[derive(Default)]
pub struct ParseEnumArgs {
    pub error: Option<Type>,
    pub or_else: Option<Expr>,
}

pub fn parse_enum(args: ParseEnumArgs, ast: DeriveInput) -> Result<proc_macro::TokenStream, syn::Error> {
    let all_attrs = ast.attrs.iter()
        .filter(|attr| !attr.path().is_ident("derive_variants") && !attr.path().is_ident("variant_struct_attr"))
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

    // Parse #[variant_struct_attr(...)] from enum attributes
    // These are applied to ALL generated variant structs
    let variant_struct_attrs = ast.attrs.iter()
        .filter(|attr| attr.path().is_ident("variant_struct_attr"))
        .map(|attr| {
            let meta = attr.meta.require_list()?;
            let inner_meta: Meta = syn::parse2(meta.tokens.clone())
                .map_err(|e| syn::Error::new(meta.tokens.span(), format!("Expected attribute in #[variant_struct_attr(...)]: {}", e)))?;
            Ok(syn::Attribute {
                pound_token: attr.pound_token.clone(),
                style: attr.style.clone(),
                bracket_token: attr.bracket_token.clone(),
                meta: inner_meta,
            })
        })
        .collect::<Result<Vec<_>, syn::Error>>()?;

    let enum_name
        = &ast.ident;

    let Data::Enum(data) = &ast.data else {
        panic!("Parsed can only be derived for enums");
    };

    let mut generated_structs
        = Vec::new();
    let mut generated_variants
        = Vec::new();
    let mut deserialization_literal_arms
        = Vec::new();
    let mut deserialization_pattern_arms
        = Vec::new();
    let mut write_file_string_arms
        = Vec::new();
    let mut to_print_string_arms
        = Vec::new();

    let mut errors
        = Vec::new();

    let mut fallback_variant: Option<Expr> = None;

    for variant in &data.variants {
        let variant_ident
            = &variant.ident;

        let has_fallback = variant.attrs.iter()
            .any(|attr| attr.path().is_ident("fallback"));

        if has_fallback {
            fallback_variant = Some(match &variant.fields {
                Fields::Named(fields) => {
                    let field_names
                        = fields.named.iter()
                            .map(|field| field.ident.as_ref().unwrap().clone())
                            .collect::<Vec<_>>();

                    if field_names.len() != 1 {
                        return Err(syn::Error::new(variant.span(), "Expected a single field in the fallback variant"));
                    }

                    let primary_field_name
                        = field_names[0].clone();

                    write_file_string_arms.push(quote! {
                        Self::#variant_ident(#primary_field_name) => out.write_str(#primary_field_name),
                    });

                    to_print_string_arms.push(quote! {
                        Self::#variant_ident(#primary_field_name) => #primary_field_name.clone(),
                    });

                    parse_quote!{#enum_name::#variant_ident(#primary_field_name: src.to_string())}
                },
                Fields::Unnamed(fields) => {
                    if fields.unnamed.len() != 1 {
                        return Err(syn::Error::new(variant.span(), "Expected a single field in the fallback variant"));
                    }

                    write_file_string_arms.push(quote! {
                        Self::#variant_ident(src) => out.write_str(src),
                    });

                    parse_quote!{#enum_name::#variant_ident(src.to_string())}
                },
                Fields::Unit => {
                    parse_quote!{#enum_name::#variant_ident}
                },
            });

            generated_variants.push(quote! {
                #variant_ident(String)
            });

            continue;
        }

        let variant_type
            = extract_variant_type(enum_name, &variant)?;

        // Parse #[struct_attr(...)] from variant attributes
        // Converts #[struct_attr(rkyv(serialize_bounds(...)))] to #[rkyv(serialize_bounds(...))]
        let struct_attrs = variant.attrs.iter()
            .filter(|attr| attr.path().is_ident("struct_attr"))
            .map(|attr| {
                let meta = attr.meta.require_list()?;
                // Parse the inner content as a Meta
                let inner_meta: Meta = syn::parse2(meta.tokens.clone())
                    .map_err(|e| syn::Error::new(meta.tokens.span(), format!("Expected attribute in #[struct_attr(...)]: {}", e)))?;
                Ok(syn::Attribute {
                    pound_token: attr.pound_token.clone(),
                    style: attr.style.clone(),
                    bracket_token: attr.bracket_token.clone(),
                    meta: inner_meta,
                })
            })
            .collect::<Result<Vec<_>, syn::Error>>();

        let struct_attrs = match struct_attrs {
            Ok(attrs) => attrs,
            Err(e) => {
                errors.push(e);
                Vec::new()
            }
        };

        match &variant_type {
            VariantType::Struct(struct_name, fields) => {
                let field_tokens
                    = fields.iter()
                        .map(|(field_name, field_info)| {
                            let field_ty = &field_info.ty;
                            let field_attrs = &field_info.attrs;
                            quote!{
                                #(#field_attrs)*
                                pub #field_name: #field_ty
                            }
                        })
                        .collect::<Vec<_>>();

                generated_structs.push(quote!{
                    #(#derive_variants_attrs)*
                    #(#variant_struct_attrs)*
                    #(#struct_attrs)*
                    pub struct #struct_name {
                        #(#field_tokens),*
                    }

                    impl Into<#enum_name> for #struct_name {
                        fn into(self) -> #enum_name {
                            #enum_name::#variant_ident(self)
                        }
                    }
                });

                generated_variants.push(quote!{
                    #variant_ident(#struct_name)
                });
            },

            VariantType::Empty => {
                generated_variants.push(quote!{
                    #variant_ident
                });
            },
        }

        let literal_attrs
            = variant.attrs.iter()
                .filter(|attr| attr.path().is_ident("literal"))
                .map(|attr| attr.parse_args::<AttributeBag>())
                .collect::<Result<Vec<_>, _>>()?;

        let pattern_attrs
            = variant.attrs.iter()
                .filter(|attr| attr.path().is_ident("pattern"))
                .map(|attr| attr.parse_args::<AttributeBag>())
                .collect::<Result<Vec<_>, _>>()?;

        let mut to_file_string_attrs
            = variant.attrs.iter()
                .filter(|attr| attr.path().is_ident("to_file_string"))
                .map(|attr| attr.parse_args::<AttributeBag>())
                .collect::<Result<Vec<_>, _>>()?;

        let mut write_file_string_attrs
            = variant.attrs.iter()
                .filter(|attr| attr.path().is_ident("write_file_string"))
                .map(|attr| attr.parse_args::<AttributeBag>())
                .collect::<Result<Vec<_>, _>>()?;

        let mut to_print_string_attrs
            = variant.attrs.iter()
                .filter(|attr| attr.path().is_ident("to_print_string"))
                .map(|attr| attr.parse_args::<AttributeBag>())
                .collect::<Result<Vec<_>, _>>()?;

        let mut has_serialization_arm
            = false;
        let mut has_write_arm
            = false;

        if let Some(mut write_file_string_attr) = write_file_string_attrs.pop() {
            let Some(write_file_string_attr) = write_file_string_attr.take("main") else {
                errors.push(syn::Error::new(variant.span(), "Expected a closure in #[write_file_string(|params, out| ...)]"));
                continue;
            };

            has_write_arm = true;

            match &variant_type {
                VariantType::Struct(struct_name, _) => {
                    write_file_string_arms.push(quote! {
                        Self::#variant_ident(params) => {
                            fn call<W: std::fmt::Write, F: Fn(&#struct_name, &mut W) -> std::fmt::Result>(f: F, p: &#struct_name, out: &mut W) -> std::fmt::Result { f(p, out) }
                            call(#write_file_string_attr, params, out)
                        },
                    });
                },

                VariantType::Empty => {
                    write_file_string_arms.push(quote! {
                        Self::#variant_ident => {
                            fn call<W: std::fmt::Write, F: Fn(&mut W) -> std::fmt::Result>(f: F, out: &mut W) -> std::fmt::Result { f(out) }
                            call(#write_file_string_attr, out)
                        },
                    });
                },
            }
        }

        if let Some(mut to_file_string_attr) = to_file_string_attrs.pop() {
            let Some(to_file_string_attr) = to_file_string_attr.take("main") else {
                errors.push(syn::Error::new(variant.span(), "Expected a closure in #[to_file_string(|params| ...)]"));
                continue;
            };

            if !has_write_arm {
                has_write_arm = true;

                match &variant_type {
                    VariantType::Struct(struct_name, _) => {
                        write_file_string_arms.push(quote! {
                            Self::#variant_ident(params) => {
                                fn call<F: Fn(&#struct_name) -> String>(f: F, p: &#struct_name) -> String { f(p) }
                                out.write_str(&call(#to_file_string_attr, params))
                            },
                        });
                    },

                    VariantType::Empty => {
                        write_file_string_arms.push(quote! {
                            Self::#variant_ident => out.write_str(&(#to_file_string_attr)()),
                        });
                    },
                }
            }
        }

        if let Some(mut to_print_string_attr) = to_print_string_attrs.pop() {
            let Some(to_print_string_attr) = to_print_string_attr.take("main") else {
                errors.push(syn::Error::new(variant.span(), "Expected a closure in #[to_print_string(|params| ...)]"));
                continue;
            };

            match &variant_type {
                VariantType::Struct(struct_name, _) => {
                    to_print_string_arms.push(quote! {
                        Self::#variant_ident(params) => {
                            fn call<F: Fn(&#struct_name) -> String>(f: F, p: &#struct_name) -> String { f(p) }
                            call(#to_print_string_attr, params)
                        },
                    });
                },

                VariantType::Empty => {
                    to_print_string_arms.push(quote! {
                        Self::#variant_ident => (#to_print_string_attr)(),
                    });
                },
            }
        }

        for mut literal_attr in literal_attrs {
            let Some(literal) = literal_attr.take("main") else {
                errors.push(syn::Error::new(variant.span(), "Expected a string literal in #[literal(\"...\")]"));
                continue;
            };

            deserialization_literal_arms.push(quote! {
                #literal => return Ok(Self::#variant_ident),
            });

            if !has_write_arm {
                write_file_string_arms.push(quote! {
                    Self::#variant_ident => out.write_str(#literal),
                });

                has_write_arm = true;
            }

            if !has_serialization_arm {
                to_print_string_arms.push(quote! {
                    Self::#variant_ident => #literal.to_string(),
                });

                has_serialization_arm = true;
            }

            errors.extend(literal_attr.errors());
        }

        for mut pattern_attr in pattern_attrs {
            let Some(pattern_expr) = pattern_attr.take("main") else {
                errors.push(syn::Error::new(variant.span(), "Expected a string literal in #[pattern(\"...\")]"));
                continue;
            };

            let capture_expr: Expr
                = parse_quote_spanned!(pattern_expr.span() => captures);

            let variant_factory
                = make_pattern_factory(&capture_expr, &pattern_expr, &variant_ident, &variant_type)?;

            deserialization_pattern_arms.push(quote! {{
                static RE: std::sync::LazyLock<regex::Regex>
                    = std::sync::LazyLock::new(|| regex::Regex::new(&format!("^{}$", #pattern_expr)).unwrap());

                if let Some(captures) = RE.captures(src) {
                    if let Ok(val) = (|| -> Result<Self, ()> {Ok(#variant_factory)})() {
                        return Ok(val);
                    }
                }
            }});

            errors.extend(pattern_attr.errors());
        }
    }

    let error_type
        = args.error.unwrap_or_else(|| {
            if fallback_variant.is_some() && deserialization_pattern_arms.is_empty() {
                parse_quote!{std::convert::Infallible}
            } else {
                syn::Type::Path(syn::TypePath {
                    qself: None,
                    path: syn::Path::from(syn::Ident::new("Error", proc_macro2::Span::call_site())),
                })
            }
        });

    if let Some(fallback_variant) = &fallback_variant {
        deserialization_pattern_arms.push(quote!{
            return Ok(#fallback_variant);
        });
    } else if let Some(or_else) = &args.or_else {
        deserialization_pattern_arms.push(quote!{
            return Some(src).map(#or_else).unwrap();
        });
    } else {
        deserialization_pattern_arms.push(quote!{
            panic!("Invalid value: {}", src);
        });
    }

    let expanded = quote! {
        #(#generated_structs)*

        #(#all_attrs)*
        pub enum #enum_name {
            #(#generated_variants),*
        }

        impl zpm_utils::FromFileString for #enum_name {
            type Error = #error_type;

            fn from_file_string(src: &str) -> Result<Self, Self::Error> {
                // First try literal matching for quick shortcuts
                match src {
                    #(#deserialization_literal_arms)*
                    _ => {}
                }

                // Then try pattern matching
                #(#deserialization_pattern_arms)*
            }
        }

        impl zpm_utils::ToFileString for #enum_name {
            fn write_file_string<W: std::fmt::Write>(&self, out: &mut W) -> std::fmt::Result {
                match self {
                    #(#write_file_string_arms)*
                    _ => panic!("Invalid value"),
                }
            }
        }

        impl zpm_utils::ToHumanString for #enum_name {
            fn to_print_string(&self) -> String {
                match self {
                    #(#to_print_string_arms)*
                    _ => panic!("Invalid value"),
                }
            }
        }

        zpm_utils::impl_file_string_from_str!(#enum_name);
        zpm_utils::impl_file_string_serialization!(#enum_name);
    };

    if !errors.is_empty() {
        let mut error_it
            = errors.into_iter();

        let mut first_error
            = error_it.next().unwrap();

        while let Some(error) = error_it.next() {
            first_error.combine(error);
        }

        return Err(first_error);
    }

    // panic!("{}", expanded.to_string());

    Ok(expanded.into())
}
