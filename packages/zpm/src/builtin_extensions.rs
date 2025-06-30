use std::{collections::BTreeMap, sync::LazyLock};
use serde::{Deserialize, Serialize};

use crate::{
    primitives::{Descriptor, Ident, PeerRange, SemverDescriptor},
    settings::PackageExtension,
};

// Include the builtin extensions JSON at compile time
const BUILTIN_EXTENSIONS_JSON: &str = include_str!("../data/builtin-extensions.json");

// This is a wrapper type to help with deserialization
#[derive(Debug, Clone, Deserialize, Serialize)]
struct BuiltinExtensions(BTreeMap<String, PackageExtension>);

static BUILTIN_EXTENSIONS: LazyLock<BTreeMap<SemverDescriptor, PackageExtension>> = LazyLock::new(|| {
    let extensions: Vec<(SemverDescriptor, PackageExtension)>
        = serde_json::from_str(BUILTIN_EXTENSIONS_JSON)
            .expect("Failed to parse builtin extensions JSON");

    let extension_map = extensions
        .into_iter()
        .collect::<BTreeMap<_, _>>();

    extension_map
});

pub fn iter_builtin_extensions() -> impl Iterator<Item = (&'static SemverDescriptor, &'static PackageExtension)> {
    BUILTIN_EXTENSIONS.iter()
}
