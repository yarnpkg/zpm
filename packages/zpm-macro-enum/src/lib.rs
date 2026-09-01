mod parse_enum;

use parse_enum::ParseEnumArgs;
use syn::{parse_macro_input, DeriveInput};

/// Derives parsing/serialization for enums whose variants are described by
/// `#[pattern(...)]` regexes (plus `#[literal(...)]` shortcuts).
///
/// Parsing contract — variant order is load-bearing in two ways:
///
/// - **First match wins, in declaration order.** Literal arms are tried
///   before patterns; for patterns, an arm is selected when its anchored
///   regex matches AND every typed capture parses, otherwise parsing falls
///   through to the next arm (this is how several variants can share one
///   regex, disambiguated purely by capture type). A catch-all pattern
///   therefore shadows every variant declared after it; scope catch-alls
///   accordingly (e.g. `Range::AnonymousTag` excludes `:` so that
///   protocol-prefixed variants declared later remain reachable).
/// - **Declaration order feeds the derived `Ord`**, so variants are also
///   positioned for sorting (e.g. virtual ranges are declared last to sort
///   last). Reordering variants changes both parsing precedence and sort
///   order at once.
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
