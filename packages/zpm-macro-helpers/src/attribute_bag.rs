use std::collections::HashMap;

use syn::{Expr, ExprLit, Ident, Lit, LitBool, Token, parse::{Parse, ParseStream, discouraged::Speculative}, spanned::Spanned};

fn maybe_expr(input: &ParseStream) -> Option<Expr> {
    let fork
        = input.fork();

    let Ok(expr) = fork.parse::<Expr>() else {
        return None;
    };

    input.advance_to(&fork);
    Some(expr)
}

#[derive(Clone)]
pub struct AttributeRecord {
    pub ident: Ident,
    pub value: Expr,
}

#[derive(Clone, Default)]
pub struct AttributeBag {
    attributes: HashMap<String, AttributeRecord>,
}

impl AttributeBag {
    pub fn errors(&self) -> Vec<syn::Error> {
        self.attributes.iter()
            .map(|(key, value)| syn::Error::new_spanned(value.ident.clone(), format!("Unsupported extra attribute: {}", key)))
            .collect::<Vec<_>>()
    }

    pub fn take(&mut self, key: &str) -> Option<Expr> {
        self.attributes.remove(key).map(|record| record.value)
    }
}

impl Parse for AttributeBag {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut attributes
            = HashMap::new();

        if let Some(expr) = maybe_expr(&input) {
            attributes.insert("main".to_string(), AttributeRecord {
                ident: Ident::new("main", expr.span()),
                value: expr,
            });

            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }

        while !input.is_empty() {
            let identifier: Ident
                = input.parse()?;

            let value: Expr = if input.peek(Token![=]) {
                input.parse::<Token![=]>()?;
                input.parse()?
            } else {
                Expr::Lit(ExprLit {
                    attrs: vec![],
                    lit: Lit::Bool(LitBool {
                        value: true,
                        span: identifier.span(),
                    }),
                })
            };

            attributes.insert(identifier.to_string(), AttributeRecord {
                ident: identifier,
                value: value,
            });

            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }

        if !input.is_empty() {
            return Err(input.error("Unexpected token"));
        }

        Ok(Self {
            attributes,
        })
    }
}
