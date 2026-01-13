use std::collections::HashMap;

use syn::{parse::{Parse, ParseStream}, Expr, ExprLit, Ident, Lit, LitBool, Token};

use crate::attribute_bag::AttributeBag;

#[derive(Clone, Default)]
pub struct Field {
    pub path: Vec<String>,
    pub attributes: AttributeBag,
}

impl OptionBag {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let path: LitStr = input.parse()?;

        let mut path = path.value()
            .split(',')
            .map(|s| s.trim().to_string())
            .collect::<Vec<_>>();

        path.sort_by(|a, b| {
            a.len().cmp(&b.len())
        });

        let mut attributes = AttributeBag::default();
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            attributes = input.parse()?;
        }

        Ok(Self {
            path,
            attributes,
        })
    }

    fn parse_without_path(input: ParseStream) -> syn::Result<Self> {
        Ok(Self {
            path: vec![],
            attributes: input.parse()?,
        })
    }
}

impl Parse for OptionBag {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.peek(syn::Ident) {
            Self::parse_without_path(input)
        } else {
            Self::parse_with_path(input)
        }
    }
}
