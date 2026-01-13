use std::hash::Hash;

use bincode::{Decode, Encode};
use zpm_macro_enum::zpm_enum;
use zpm_utils::{DataType, Hash64, Path, ToFileString, UrlEncoded};

use super::{Ident, Locator};

fn format_patch(inner: &UrlEncoded<Locator>, path: &str, checksum: &Option<Hash64>) -> String {
    match checksum {
        Some(checksum) => format!("patch:{}#{}&checksum={}", inner.to_file_string(), path, checksum.to_file_string()),
        None => format!("patch:{}#{}", inner.to_file_string(), path),
    }
}

fn format_workspace_path(path: &Path) -> String {
    if path.is_empty() {
        "workspace:.".to_string()
    } else {
        format!("workspace:{}", path.to_file_string())
    }
}

#[derive(thiserror::Error, Clone, Debug)]
pub enum ReferenceError {
    #[error("Invalid reference: {0}")]
    SyntaxError(String),
}

#[zpm_enum(error = ReferenceError, or_else = |s| Err(ReferenceError::SyntaxError(s.to_string())))]
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive_variants(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reference {
    #[pattern(r"builtin:(?<version>.*)")]
    #[to_file_string(|params| format!("builtin:{}", params.version.to_file_string()))]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("builtin:{}", params.version.to_file_string())))]
    Builtin {
        version: zpm_semver::Version,
    },

    #[pattern(r"npm:(?<version>.*)")]
    #[to_file_string(|params| format!("npm:{}", params.version.to_file_string()))]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("npm:{}", params.version.to_file_string())))]
    Shorthand {
        version: zpm_semver::Version,
    },

    #[pattern(r"npm:(?<ident>.*)@(?<version>.*)")]
    #[to_file_string(|params| format!("npm:{}@{}", params.ident.to_file_string(), params.version.to_file_string()))]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("npm:{}@{}", params.ident.to_file_string(), params.version.to_file_string())))]
    Registry {
        ident: Ident,
        version: zpm_semver::Version,
    },

    #[pattern(r"file:(?<path>.*\.(?:tgz|tar\.gz))")]
    #[to_file_string(|params| format!("file:{}", params.path))]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("file:{}", params.path)))]
    Tarball {
        path: String,
    },

    #[pattern(r"file:(?<path>.*)")]
    #[to_file_string(|params| format!("file:{}", params.path))]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("file:{}", params.path)))]
    Folder {
        path: String,
    },

    #[pattern(r"link:(?<path>.*)")]
    #[to_file_string(|params| format!("link:{}", params.path))]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("link:{}", params.path)))]
    Link {
        path: String,
    },

    #[pattern(r"portal:(?<path>.*)")]
    #[to_file_string(|params| format!("portal:{}", params.path))]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("portal:{}", params.path)))]
    Portal {
        path: String,
    },

    #[pattern(r"patch:(?<inner>.*)#(?<path>.*)(?:&checksum=(?<checksum>[a-f0-9]*))?$")]
    #[to_file_string(|params| format_patch(&params.inner, &params.path, &params.checksum))]
    #[to_print_string(|params| DataType::Reference.colorize(&format_patch(&params.inner, &params.path, &params.checksum)))]
    Patch {
        inner: Box<UrlEncoded<Locator>>,
        path: String,
        checksum: Option<Hash64>,
    },

    #[pattern(r"virtual:(?<hash>[a-f0-9]*)#(?<inner>.*)$")]
    #[to_file_string(|params| format!("virtual:{}#{}", params.hash.to_file_string(), params.inner.to_file_string()))]
    #[to_print_string(|params| format!("{} {}", params.inner.to_print_string(), DataType::Reference.colorize(&format!("[{}]", params.hash.mini()))))]
    Virtual {
        inner: Box<Reference>,
        hash: Hash64,
    },

    #[pattern(r"workspace:(?<ident>.*)")]
    #[to_file_string(|params| format!("workspace:{}", params.ident.to_file_string()))]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("workspace:{}", params.ident.to_file_string())))]
    WorkspaceIdent {
        ident: Ident,
    },

    #[pattern(r"workspace:(?<path>.*)")]
    #[to_file_string(|params| format_workspace_path(&params.path))]
    #[to_print_string(|params| DataType::Reference.colorize(&format_workspace_path(&params.path)))]
    WorkspacePath {
        path: Path,
    },

    #[pattern(r"git:(?<git>.*)")]
    #[pattern(r"(?<git>https?://.*\.git#.*)")]
    #[to_file_string(|params| format!("git:{}", params.git.to_file_string()))]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("git:{}", params.git.to_file_string())))]
    Git {
        git: zpm_git::GitReference,
    },

    #[pattern(r"(?<url>https?://.*(?:/.*|\.tgz|\.tar\.gz))")]
    #[to_file_string(|params| params.url.clone())]
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
                format!("builtin-{}", params.version.to_file_string())
            },

            Reference::Shorthand(params) => {
                format!("npm-{}", params.version.to_file_string())
            },

            Reference::Git(_) => {
                "git".to_string()
            },

            Reference::Registry(params) => {
                format!("npm-{}", params.version.to_file_string())
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
