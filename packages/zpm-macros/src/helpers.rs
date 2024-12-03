use syn::meta::ParseNestedMeta;

pub fn extract_literal(meta: &ParseNestedMeta) -> syn::Result<String> {
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

// pub fn extract_bool(meta: &ParseNestedMeta) -> syn::Result<bool> {
//     let expr: syn::Expr = meta.value()?.parse()?;
//     let value = &expr;

//     if let syn::Expr::Lit(syn::ExprLit {
//         lit: syn::Lit::Bool(lit),
//         attrs: _,
//     }) = value {
//         return Ok(lit.value);
//     }

//     panic!("Invalid syntax")
// }

// https://stackoverflow.com/a/56264023/880703
pub fn extract_type_from_option(ty: &syn::Type) -> Option<&syn::Type> {
    use syn::{GenericArgument, Path, PathArguments, PathSegment};

    fn extract_type_path(ty: &syn::Type) -> Option<&Path> {
        match *ty {
            syn::Type::Path(ref typepath) if typepath.qself.is_none() => Some(&typepath.path),
            _ => None,
        }
    }

    fn extract_option_segment(path: &Path) -> Option<&PathSegment> {
        let idents_of_path = path
            .segments
            .iter()
            .fold(String::new(), |mut acc, v| {
                acc.push_str(&v.ident.to_string());
                acc.push('|');
                acc
            });

        vec!["Option|", "std|option|Option|", "core|option|Option|"]
            .into_iter()
            .find(|s| &idents_of_path == s)
            .and_then(|_| path.segments.last())
    }

    extract_type_path(ty)
        .and_then(|path| extract_option_segment(path))
        .and_then(|path_seg| {
            let type_params = &path_seg.arguments;
            // It should have only on angle-bracketed param ("<String>"):
            match *type_params {
                PathArguments::AngleBracketed(ref params) => params.args.first(),
                _ => None,
            }
        })
        .and_then(|generic_arg| match *generic_arg {
            GenericArgument::Type(ref ty) => Some(ty),
            _ => None,
        })
}
