use std::{fmt, hash::Hash};

use rkyv::Archive;
use zpm_macro_enum::zpm_enum;
use zpm_utils::{DataType, FileStringDisplay, Hash64, Path, ToFileString, UrlEncoded};

use super::{Ident, Locator};

fn format_patch(inner: &UrlEncoded<Locator>, path: &str, checksum: &Option<Hash64>) -> String {
    match checksum {
        Some(checksum) => format!("patch:{}#{}&checksum={}", FileStringDisplay(inner), path, FileStringDisplay(checksum)),
        None => format!("patch:{}#{}", FileStringDisplay(inner), path),
    }
}

fn write_patch<W: fmt::Write>(inner: &UrlEncoded<Locator>, path: &str, checksum: &Option<Hash64>, out: &mut W) -> fmt::Result {
    out.write_str("patch:")?;
    inner.write_file_string(out)?;
    out.write_str("#")?;
    out.write_str(path)?;

    if let Some(checksum) = checksum {
        out.write_str("&checksum=")?;
        checksum.write_file_string(out)?;
    }

    Ok(())
}

fn format_registry(ident: &Ident, version: &zpm_semver::Version, url: Option<&String>) -> String {
    match url {
        Some(url) => format!("npm:{}@{}#{}", FileStringDisplay(ident), FileStringDisplay(version), FileStringDisplay(url)),
        None => format!("npm:{}@{}", FileStringDisplay(ident), FileStringDisplay(version)),
    }
}

fn write_registry<W: fmt::Write>(ident: &Ident, version: &zpm_semver::Version, url: Option<&UrlEncoded<String>>, out: &mut W) -> fmt::Result {
    out.write_str("npm:")?;
    ident.write_file_string(out)?;
    out.write_str("@")?;
    version.write_file_string(out)?;

    if let Some(url) = url {
        out.write_str("#")?;
        url.write_file_string(out)?;
    }

    Ok(())
}

fn format_workspace_path(path: &Path) -> String {
    if path.is_empty() {
        "workspace:.".to_string()
    } else {
        format!("workspace:{}", FileStringDisplay(path))
    }
}

fn write_workspace_path<W: fmt::Write>(path: &Path, out: &mut W) -> fmt::Result {
    if path.is_empty() {
        out.write_str("workspace:.")
    } else {
        out.write_str("workspace:")?;
        path.write_file_string(out)
    }
}

#[derive(thiserror::Error, Clone, Debug)]
pub enum ReferenceError {
    #[error("Invalid reference: {0}")]
    SyntaxError(String),
}

#[zpm_enum(error = ReferenceError, or_else = |s| Err(ReferenceError::SyntaxError(s.to_string())))]
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash))]
#[derive_variants(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[variant_struct_attr(rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash)))]
pub enum Reference {
    #[pattern(r"builtin:(?<version>.*)")]
    #[to_file_string(|params| format!("builtin:{}", FileStringDisplay(&params.version)))]
    #[write_file_string(|params, out| { out.write_str("builtin:")?; params.version.write_file_string(out) })]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("builtin:{}", FileStringDisplay(&params.version))))]
    Builtin {
        version: zpm_semver::Version,
    },

    #[pattern(r"npm:(?<version>.*)")]
    #[to_file_string(|params| format!("npm:{}", FileStringDisplay(&params.version)))]
    #[write_file_string(|params, out| { out.write_str("npm:")?; params.version.write_file_string(out) })]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("npm:{}", FileStringDisplay(&params.version))))]
    Shorthand {
        version: zpm_semver::Version,
    },

    #[pattern(r"npm:(?<ident>(?:@[^#@]+/)?[^#@]+)@(?<version>[^#]*)(?:#(?<url>.*))?")]
    #[to_file_string(|params| format_registry(&params.ident, &params.version, params.url.as_deref()))]
    #[write_file_string(|params, out| write_registry(&params.ident, &params.version, params.url.as_ref(), out))]
    #[to_print_string(|params| DataType::Reference.colorize(&format_registry(&params.ident, &params.version, params.url.as_deref())))]
    Registry {
        ident: Ident,
        version: zpm_semver::Version,
        url: Option<UrlEncoded<String>>,
    },

    #[pattern(r"file:(?<path>.*\.(?:tgz|tar\.gz))")]
    #[to_file_string(|params| format!("file:{}", params.path))]
    #[write_file_string(|params, out| { out.write_str("file:")?; out.write_str(&params.path) })]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("file:{}", params.path)))]
    Tarball {
        path: String,
    },

    #[pattern(r"file:(?<path>.*)")]
    #[to_file_string(|params| format!("file:{}", params.path))]
    #[write_file_string(|params, out| { out.write_str("file:")?; out.write_str(&params.path) })]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("file:{}", params.path)))]
    Folder {
        path: String,
    },

    #[pattern(r"link:(?<path>.*)")]
    #[to_file_string(|params| format!("link:{}", params.path))]
    #[write_file_string(|params, out| { out.write_str("link:")?; out.write_str(&params.path) })]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("link:{}", params.path)))]
    Link {
        path: String,
    },

    #[pattern(r"portal:(?<path>.*)")]
    #[to_file_string(|params| format!("portal:{}", params.path))]
    #[write_file_string(|params, out| { out.write_str("portal:")?; out.write_str(&params.path) })]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("portal:{}", params.path)))]
    Portal {
        path: String,
    },

    #[pattern(r"patch:(?<inner>.*)#(?<path>.*)(?:&checksum=(?<checksum>[a-f0-9]*))?$")]
    #[to_file_string(|params| format_patch(&params.inner, &params.path, &params.checksum))]
    #[write_file_string(|params, out| write_patch(&params.inner, &params.path, &params.checksum, out))]
    #[to_print_string(|params| DataType::Reference.colorize(&format_patch(&params.inner, &params.path, &params.checksum)))]
    #[struct_attr(rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator + rkyv::ser::Sharing, <__S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
    #[struct_attr(rkyv(deserialize_bounds(__D: rkyv::de::Pooling, <__D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
    #[struct_attr(rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext + rkyv::validation::SharedContext, <__C as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))))]
    Patch {
        #[rkyv(omit_bounds)]
        inner: Box<UrlEncoded<Locator>>,
        path: String,
        checksum: Option<Hash64>,
    },

    #[pattern(r"virtual:(?<hash>[a-f0-9]*)#(?<inner>.*)$")]
    #[to_file_string(|params| format!("virtual:{}#{}", FileStringDisplay(&params.hash), FileStringDisplay(&params.inner)))]
    #[write_file_string(|params, out| {
        out.write_str("virtual:")?;
        params.hash.write_file_string(out)?;
        out.write_str("#")?;
        params.inner.write_file_string(out)
    })]
    #[to_print_string(|params| format!("{} {}", params.inner.to_print_string(), DataType::Reference.colorize(&format!("[{}]", params.hash.mini()))))]
    #[struct_attr(rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator + rkyv::ser::Sharing, <__S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
    #[struct_attr(rkyv(deserialize_bounds(__D: rkyv::de::Pooling, <__D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
    #[struct_attr(rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext + rkyv::validation::SharedContext, <__C as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))))]
    Virtual {
        #[rkyv(omit_bounds)]
        inner: Box<Reference>,
        hash: Hash64,
    },

    #[pattern(r"workspace:(?<ident>.*)")]
    #[to_file_string(|params| format!("workspace:{}", FileStringDisplay(&params.ident)))]
    #[write_file_string(|params, out| { out.write_str("workspace:")?; params.ident.write_file_string(out) })]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("workspace:{}", FileStringDisplay(&params.ident))))]
    WorkspaceIdent {
        ident: Ident,
    },

    #[pattern(r"workspace:(?<path>.*)")]
    #[to_file_string(|params| format_workspace_path(&params.path))]
    #[write_file_string(|params, out| write_workspace_path(&params.path, out))]
    #[to_print_string(|params| DataType::Reference.colorize(&format_workspace_path(&params.path)))]
    WorkspacePath {
        path: Path,
    },

    #[pattern(r"git:(?<git>.*)")]
    #[pattern(r"(?<git>https?://.*\.git#.*)")]
    #[to_file_string(|params| format!("git:{}", FileStringDisplay(&params.git)))]
    #[write_file_string(|params, out| { out.write_str("git:")?; params.git.write_file_string(out) })]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("git:{}", FileStringDisplay(&params.git))))]
    Git {
        git: zpm_git::GitReference,
    },

    #[pattern(r"(?<url>https?://.*(?:/.*|\.tgz|\.tar\.gz))")]
    #[to_file_string(|params| params.url.clone())]
    #[write_file_string(|params, out| out.write_str(&params.url))]
    #[to_print_string(|params| DataType::Reference.colorize(&params.url))]
    Url {
        url: String,
    },
}

impl Reference {
    pub fn must_bind(&self) -> bool {
        // Keep this implementation in sync w/ Range::must_bind

        if let Reference::Patch(params) = self {
            return params.inner.0.reference.must_bind() || (params.path.as_str() != "<builtin>" && !params.path.as_str().starts_with("~/"));
        }

        matches!(&self, Reference::Link(_) | Reference::Portal(_) | Reference::Tarball(_) | Reference::Folder(_))
    }

    pub fn is_workspace_reference(&self) -> bool {
        matches!(&self, Reference::WorkspaceIdent(_) | Reference::WorkspacePath(_))
    }

    pub fn is_disk_reference(&self) -> bool {
        matches!(&self, Reference::WorkspaceIdent(_) | Reference::WorkspacePath(_) | Reference::Portal(_) | Reference::Link(_))
    }

    pub fn is_virtual_reference(&self) -> bool {
        matches!(&self, Reference::Virtual(_))
    }

    pub fn inner_locator(&self) -> Option<&Locator> {
        // Keep this implementation in sync w/ Range::inner_descriptor

        match self {
            Reference::Patch(params) => {
                Some(&params.inner.0)
            },

            _ => {
                None
            },
        }
    }

    pub fn physical_reference(&self) -> &Reference {
        if let Reference::Virtual(params) = self {
            params.inner.physical_reference()
        } else {
            self
        }
    }

    pub fn slug(&self) -> String {
        match self {
            Reference::Builtin(params) => {
                format!("builtin-{}", FileStringDisplay(&params.version))
            },

            Reference::Shorthand(params) => {
                format!("npm-{}", FileStringDisplay(&params.version))
            },

            Reference::Git(_) => {
                "git".to_string()
            },

            Reference::Registry(params) => {
                format!("npm-{}", FileStringDisplay(&params.version))
            },

            Reference::Tarball(_) => {
                "file".to_string()
            },

            Reference::Folder(_) => {
                "file".to_string()
            },

            Reference::Link(_) => {
                "link".to_string()
            },

            Reference::Patch(_) => {
                "patch".to_string()
            },

            Reference::Portal(_) => {
                "portal".to_string()
            },

            Reference::Url(_) => {
                "url".to_string()
            },

            Reference::Virtual(_) => {
                "virtual".to_string()
            },

            Reference::WorkspaceIdent(_) => {
                "workspace".to_string()
            },

            Reference::WorkspacePath(_) => {
                "workspace".to_string()
            },
        }
    }
}
