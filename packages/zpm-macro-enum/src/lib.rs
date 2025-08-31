mod parse_enum;

use parse_enum::ParseEnumArgs;
use syn::{parse_macro_input, DeriveInput};

#[proc_macro_attribute]
pub fn zpm_enum(args_tokens: proc_macro::TokenStream, input_tokens: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let mut args
        = ParseEnumArgs::default();

    let args_parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("error") {
            args.error = Some(meta.value()?.parse()?);
            Ok(())
        } else if meta.path.is_ident("or_else") {
            args.or_else = Some(meta.value()?.parse()?);
            Ok(())
        } else {
            Err(meta.error(format!("unsupported zpm_enum property ({:?})", meta.path)))
        }
    });

    parse_macro_input!(args_tokens with args_parser);

    let ast
        = parse_macro_input!(input_tokens as DeriveInput);

    parse_enum::parse_enum(args, ast)
        .unwrap_or_else(|err| err.to_compile_error().into())
}
