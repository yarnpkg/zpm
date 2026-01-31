use std::{fmt, hash::Hash, str::FromStr, sync::LazyLock};

use regex::Regex;
use rkyv::Archive;
use zpm_macro_enum::zpm_enum;
use zpm_utils::{DataType, FileStringDisplay, Hash64, Path, ToFileString, UrlEncoded};

use crate::{PeerRange, SemverPeerRange};

use super::{Descriptor, Ident};

pub static EXPLICIT_PATH_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^.{0,2}/").unwrap()
});

fn format_registry_semver(ident: &Option<Ident>, range: &zpm_semver::Range) -> String {
    let mut buffer = String::from("npm:");

    if let Some(ident) = ident {
        let _ = ident.write_file_string(&mut buffer);
        buffer.push('@');
    }

    let _ = range.write_file_string(&mut buffer);
    buffer
}

fn write_registry_semver<W: fmt::Write>(ident: &Option<Ident>, range: &zpm_semver::Range, out: &mut W) -> fmt::Result {
    out.write_str("npm:")?;

    if let Some(ident) = ident {
        ident.write_file_string(out)?;
        out.write_str("@")?;
    }

    range.write_file_string(out)
}

fn format_registry_tag(ident: &Option<Ident>, tag: &str) -> String {
    let mut buffer = String::from("npm:");

    if let Some(ident) = ident {
        let _ = ident.write_file_string(&mut buffer);
        buffer.push('@');
    }

    buffer.push_str(tag);
    buffer
}

fn format_prefixed<T: ToFileString>(prefix: &str, value: &T) -> String {
    format!("{}{}", prefix, FileStringDisplay(value))
}

fn colorize_prefixed<T: ToFileString>(prefix: &str, value: &T, kind: DataType) -> String {
    kind.colorize(&format_prefixed(prefix, value))
}

fn write_registry_tag<W: fmt::Write>(ident: &Option<Ident>, tag: &str, out: &mut W) -> fmt::Result {
    out.write_str("npm:")?;

    if let Some(ident) = ident {
        ident.write_file_string(out)?;
        out.write_str("@")?;
    }

    out.write_str(tag)
}

fn format_path_range(path: &str) -> String {
    if EXPLICIT_PATH_REGEX.is_match(path) {
        path.to_string()
    } else {
        format!("file:{}", path)
    }
}

fn write_path_range<W: fmt::Write>(path: &str, out: &mut W) -> fmt::Result {
    if EXPLICIT_PATH_REGEX.is_match(path) {
        out.write_str(path)
    } else {
        out.write_str("file:")?;
        out.write_str(path)
    }
}


#[derive(thiserror::Error, Clone, Debug)]
pub enum RangeError {
    #[error("Invalid range: {0}")]
    SyntaxError(String),

    #[error("Parsing error: {0}")]
    SemverError(#[from] zpm_semver::Error),

    #[error("Cannot convert range to peer range: {0}")]
    PeerRangeError(String),

    #[error(transparent)]
    FromUtf8Error(#[from] std::string::FromUtf8Error),
}

#[zpm_enum(error = RangeError, or_else = |s| Err(RangeError::SyntaxError(s.to_string())))]
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash))]
#[derive_variants(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[variant_struct_attr(rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash)))]
pub enum Range {
    #[no_pattern]
    #[to_file_string(|| "missing!".to_string())]
    #[write_file_string(|out| out.write_str("missing!"))]
    #[to_print_string(|| DataType::Range.colorize("missing!"))]
    MissingPeerDependency,

    #[pattern(r"builtin:(?<range>.*)")]
    #[to_file_string(|params| format_prefixed("builtin:", &params.range))]
    #[write_file_string(|params, out| { out.write_str("builtin:")?; params.range.write_file_string(out) })]
    #[to_print_string(|params| colorize_prefixed("builtin:", &params.range, DataType::Range))]
    Builtin {
        range: zpm_semver::Range,
    },

    #[pattern(r"(?<range>.*)")]
    #[to_file_string(|params| params.range.to_file_string())]
    #[write_file_string(|params, out| params.range.write_file_string(out))]
    #[to_print_string(|params| DataType::Range.colorize(&params.range.to_file_string()))]
    AnonymousSemver {
        range: zpm_semver::Range,
    },

    #[pattern(r"npm:(?:(?<ident>.*)@)?(?<range>.*)")]
    #[to_file_string(|params| format_registry_semver(&params.ident, &params.range))]
    #[write_file_string(|params, out| write_registry_semver(&params.ident, &params.range, out))]
    #[to_print_string(|params| DataType::Range.colorize(&format_registry_semver(&params.ident, &params.range)))]
    RegistrySemver {
        ident: Option<Ident>,
        range: zpm_semver::Range,
    },

    #[pattern(r"npm:(?:(?<ident>.*)@)?(?<tag>[-a-z0-9._^v][-a-z0-9._]*)")]
    #[to_file_string(|params| format_registry_tag(&params.ident, &params.tag))]
    #[write_file_string(|params, out| write_registry_tag(&params.ident, &params.tag, out))]
    #[to_print_string(|params| DataType::Range.colorize(&format_registry_tag(&params.ident, &params.tag)))]
    RegistryTag {
        ident: Option<Ident>,
        tag: String,
    },

    #[pattern(r"link:(?<path>.*)")]
    #[to_file_string(|params| format!("link:{}", params.path))]
    #[write_file_string(|params, out| { out.write_str("link:")?; out.write_str(&params.path) })]
    #[to_print_string(|params| DataType::Range.colorize(&format!("link:{}", params.path)))]
    Link {
        path: String,
    },

    #[pattern(r"portal:(?<path>.*)")]
    #[to_file_string(|params| format!("portal:{}", params.path))]
    #[write_file_string(|params, out| { out.write_str("portal:")?; out.write_str(&params.path) })]
    #[to_print_string(|params| DataType::Range.colorize(&format!("portal:{}", params.path)))]
    Portal {
        path: String,
    },

    #[pattern(r"file:(?<path>.*\.(?:tgz|tar\.gz))")]
    #[pattern(r"(?<path>\.{0,2}/.*\.(?:tgz|tar\.gz))")]
    #[to_file_string(|params| format_path_range(&params.path))]
    #[write_file_string(|params, out| write_path_range(&params.path, out))]
    #[to_print_string(|params| DataType::Range.colorize(&format_path_range(&params.path)))]
    Tarball {
        path: String,
    },

    #[pattern(r"file:(?<path>.*)")]
    #[pattern(r"(?<path>\.{0,2}/.*)")]
    #[to_file_string(|params| format_path_range(&params.path))]
    #[write_file_string(|params, out| write_path_range(&params.path, out))]
    #[to_print_string(|params| DataType::Range.colorize(&format_path_range(&params.path)))]
    Folder {
        path: String,
    },

    #[pattern(r"patch:(?<inner>.*)#(?<path>.*)$")]
    #[to_file_string(|params| format!("patch:{}#{}", FileStringDisplay(&params.inner), params.path))]
    #[write_file_string(|params, out| {
        out.write_str("patch:")?;
        params.inner.write_file_string(out)?;
        out.write_str("#")?;
        out.write_str(&params.path)
    })]
    #[to_print_string(|params| DataType::Range.colorize(&format!("patch:{}#{}", FileStringDisplay(&params.inner), params.path)))]
    #[struct_attr(rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator + rkyv::ser::Sharing, <__S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
    #[struct_attr(rkyv(deserialize_bounds(__D: rkyv::de::Pooling, <__D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
    #[struct_attr(rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext + rkyv::validation::SharedContext, <__C as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))))]
    Patch {
        #[rkyv(omit_bounds)]
        inner: Box<UrlEncoded<Descriptor>>,
        path: String,
    },

    #[pattern(r"catalog:(?<catalog>.+)?")]
    #[to_file_string(|params| format!("catalog:{}", params.catalog.as_deref().unwrap_or("")))]
    #[write_file_string(|params, out| {
        out.write_str("catalog:")?;
        if let Some(catalog) = &params.catalog {
            out.write_str(catalog)?;
        }
        Ok(())
    })]
    #[to_print_string(|params| DataType::Range.colorize(&format!("catalog:{}", params.catalog.as_deref().unwrap_or(""))))]
    Catalog {
        catalog: Option<String>,
    },

    #[pattern(r"workspace:(?<magic>.*)")]
    #[to_file_string(|params| format!("workspace:{}", serde_plain::to_string(&params.magic).unwrap()))]
    #[write_file_string(|params, out| {
        out.write_str("workspace:")?;
        out.write_str(&serde_plain::to_string(&params.magic).unwrap())
    })]
    #[to_print_string(|params| DataType::Range.colorize(&format!("workspace:{}", serde_plain::to_string(&params.magic).unwrap())))]
    WorkspaceMagic {
        magic: zpm_semver::RangeKind,
    },

    #[pattern(r"workspace:(?<range>.*)")]
    #[to_file_string(|params| format_prefixed("workspace:", &params.range))]
    #[write_file_string(|params, out| { out.write_str("workspace:")?; params.range.write_file_string(out) })]
    #[to_print_string(|params| colorize_prefixed("workspace:", &params.range, DataType::Range))]
    WorkspaceSemver {
        range: zpm_semver::Range,
    },

    #[pattern(r"workspace:(?<ident>.*)")]
    #[to_file_string(|params| format_prefixed("workspace:", &params.ident))]
    #[write_file_string(|params, out| { out.write_str("workspace:")?; params.ident.write_file_string(out) })]
    #[to_print_string(|params| colorize_prefixed("workspace:", &params.ident, DataType::Range))]
    WorkspaceIdent {
        ident: Ident,
    },

    #[pattern(r"workspace:(?<path>.*)")]
    #[to_file_string(|params| format_prefixed("workspace:", &params.path))]
    #[write_file_string(|params, out| { out.write_str("workspace:")?; params.path.write_file_string(out) })]
    #[to_print_string(|params| colorize_prefixed("workspace:", &params.path, DataType::Range))]
    WorkspacePath {
        path: Path,
    },

    #[pattern("(?<git>.*)")]
    #[to_file_string(|params| params.git.to_file_string())]
    #[write_file_string(|params, out| params.git.write_file_string(out))]
    #[to_print_string(|params| DataType::Range.colorize(&params.git.to_file_string()))]
    Git {
        git: zpm_git::GitRange,
    },

    #[pattern(r"(?<url>https?://.*(?:/.*|\.tgz|\.tar\.gz))")]
    #[to_file_string(|params| params.url.clone())]
    #[write_file_string(|params, out| out.write_str(&params.url))]
    #[to_print_string(|params| DataType::Range.colorize(&params.url))]
    Url {
        url: String,
    },

    #[pattern(r"(?<tag>.*)")]
    #[to_file_string(|params| params.tag.clone())]
    #[write_file_string(|params, out| out.write_str(&params.tag))]
    #[to_print_string(|params| DataType::Range.colorize(&params.tag))]
    AnonymousTag {
        tag: String,
    },

    // We keep this at the end so virtual ranges are listed last when sorted
    #[pattern(r"virtual:(?<inner>.*)#(?<hash>[a-f0-9]*)$")]
    #[to_file_string(|params| format!("virtual:{}#{}", FileStringDisplay(&params.inner), FileStringDisplay(&params.hash)))]
    #[write_file_string(|params, out| {
        out.write_str("virtual:")?;
        params.inner.write_file_string(out)?;
        out.write_str("#")?;
        params.hash.write_file_string(out)
    })]
    #[to_print_string(|params| format!("{} {}", params.inner.to_print_string(), DataType::Range.colorize(&format!("[{}]", params.hash.mini()))))]
    #[struct_attr(rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator + rkyv::ser::Sharing, <__S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
    #[struct_attr(rkyv(deserialize_bounds(__D: rkyv::de::Pooling, <__D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
    #[struct_attr(rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext + rkyv::validation::SharedContext, <__C as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))))]
    Virtual {
        #[rkyv(omit_bounds)]
        inner: Box<Range>,
        hash: Hash64,
    },
}

impl Range {
    pub fn new_semver(range_str: &str) -> Result<Range, RangeError> {
        let range
            = zpm_semver::Range::from_str(range_str)?;

        Ok(Range::AnonymousSemver(AnonymousSemverRange {range}.into()))
    }

    pub fn inner_descriptor(&self) -> Option<Descriptor> {
        match self {
            Range::RegistrySemver(params) if params.ident.is_some()
                => Some(Descriptor::new(params.ident.clone().unwrap(), RegistrySemverRange {ident: None, range: params.range.clone()}.into())),

            Range::RegistryTag(params) if params.ident.is_some()
                => Some(Descriptor::new(params.ident.clone().unwrap(), RegistryTagRange {ident: None, tag: params.tag.clone()}.into())),

            Range::Patch(params)
                => Some(params.inner.0.clone()),

            _ => None,
        }
    }

    pub fn physical_range(&self) -> &Range {
        if let Range::Virtual(params) = self {
            params.inner.physical_range()
        } else {
            self
        }
    }

    pub fn is_workspace(&self) -> bool {
        matches!(self, Range::WorkspaceMagic(_) | Range::WorkspaceSemver(_) | Range::WorkspaceIdent(_) | Range::WorkspacePath(_))
    }

    pub fn to_anonymous_range(&self) -> Range {
        match self {
            Range::RegistrySemver(params) => {
                Range::AnonymousSemver(AnonymousSemverRange {range: params.range.clone()})
            },

            Range::RegistryTag(params) => {
                Range::AnonymousTag(AnonymousTagRange {tag: params.tag.clone()})
            },

            _ => self.clone(),
        }
    }

    pub fn to_semver_range(&self) -> Option<zpm_semver::Range> {
        match self {
            Range::AnonymousSemver(params) => {
                Some(params.range.clone())
            },

            Range::RegistrySemver(params) => {
                Some(params.range.clone())
            },

            _ => None,
        }
    }

    pub fn to_peer_range(&self) -> Result<PeerRange, RangeError> {
        match self {
            Range::AnonymousSemver(params) => {
                Ok(PeerRange::Semver(SemverPeerRange {range: params.range.clone()}))
            },

            Range::RegistrySemver(params) => {
                Ok(PeerRange::Semver(SemverPeerRange {range: params.range.clone()}))
            },

            _ => {
                Err(RangeError::PeerRangeError(self.to_file_string()))
            },
        }
    }
}
