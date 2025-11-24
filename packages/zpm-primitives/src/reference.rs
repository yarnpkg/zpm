use std::hash::Hash;

use bincode::{Decode, Encode};
use zpm_macro_enum::zpm_enum;
use zpm_utils::{impl_file_string_from_str, impl_file_string_serialization, DataType, Hash64, Path, ToFileString, ToHumanString, UrlEncoded};

use super::{Ident, Locator};

#[derive(thiserror::Error, Clone, Debug)]
pub enum ReferenceError {
    #[error("Invalid reference: {0}")]
    SyntaxError(String),
}

#[zpm_enum(error = ReferenceError, or_else = |s| Err(ReferenceError::SyntaxError(s.to_string())))]
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive_variants(Clone, Debug, Decode, Encode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reference {
    #[pattern(spec = r"npm:(?<version>.*)")]
    Shorthand {
        version: zpm_semver::Version,
    },

    #[pattern(spec = r"npm:(?<ident>.*)@(?<version>.*)")]
    Registry {
        ident: Ident,
        version: zpm_semver::Version,
    },

    #[pattern(spec = r"file:(?<path>.*\.(?:tgz|tar\.gz))")]
    Tarball {
        path: String,
    },

    #[pattern(spec = r"file:(?<path>.*)")]
    Folder {
        path: String,
    },

    #[pattern(spec = r"link:(?<path>.*)")]
    Link {
        path: String,
    },

    #[pattern(spec = r"portal:(?<path>.*)")]
    Portal {
        path: String,
    },

    #[pattern(spec = r"patch:(?<inner>.*)#(?<path>.*)(?:&checksum=(?<checksum>[a-f0-9]*))?$")]
    Patch {
        inner: Box<UrlEncoded<Locator>>,
        path: String,
        checksum: Option<Hash64>,
    },

    #[pattern(spec = r"virtual:(?<hash>[a-f0-9]*)#(?<inner>.*)$")]
    Virtual {
        inner: Box<Reference>,
        hash: Hash64,
    },

    #[pattern(spec = r"workspace:(?<ident>.*)")]
    WorkspaceIdent {
        ident: Ident,
    },

    #[pattern(spec = r"workspace:(?<path>.*)")]
    WorkspacePath {
        path: Path,
    },

    #[pattern(spec = r"git:(?<git>.*)")]
    #[pattern(spec = r"(?<git>https?://.*\.git#.*)")]
    Git {
        git: zpm_git::GitReference,
    },

    #[pattern(spec = r"(?<url>https?://.*(?:/.*|\.tgz|\.tar\.gz))")]
    Url {
        url: String,
    },

    #[no_pattern]
    Synthetic {
        nonce: usize,
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

            Reference::Synthetic(_) => {
                "synthetic".to_string()
            },
        }
    }
}

impl ToFileString for Reference {
    fn to_file_string(&self) -> String {
        match self {
            Reference::Shorthand(params) => {
                format!("npm:{}", params.version.to_file_string())
            },

            Reference::Git(params) => {
                format!("git:{}", params.git.to_file_string())
            },

            Reference::Url(params) => {
                params.url.to_file_string()
            },

            Reference::Registry(params) => {
                format!("npm:{}@{}", params.ident.to_file_string(), params.version.to_file_string())
            },

            Reference::Tarball(params) => {
                format!("file:{}", params.path.to_file_string())
            },

            Reference::Folder(params) => {
                format!("file:{}", params.path.to_file_string())
            },

            Reference::Link(params) => {
                format!("link:{}", params.path.to_file_string())
            },

            Reference::Patch(params) => {
                if let Some(checksum) = &params.checksum {
                    format!("patch:{}#{}&checksum={}", params.inner.to_file_string(), params.path.to_file_string(), checksum.to_file_string())
                } else {
                    format!("patch:{}#{}", params.inner.to_file_string(), params.path.to_file_string())
                }
            },

            Reference::Portal(params) => {
                format!("portal:{}", params.path.to_file_string())
            },

            Reference::Virtual(params) => {
                format!("virtual:{}#{}", params.hash.to_file_string(), params.inner.to_file_string())
            },

            Reference::WorkspaceIdent(params) => {
                format!("workspace:{}", params.ident.to_file_string())
            },

            Reference::WorkspacePath(params) => {
                format!("workspace:{}", match params.path.is_empty() {
                    true => ".".to_string(),
                    false => params.path.to_file_string(),
                })
            },

            Reference::Synthetic(params) => {
                format!("synthetic:{}", params.nonce)
            },
        }
    }
}

impl ToHumanString for Reference {
    fn to_print_string(&self) -> String {
        if let Reference::Virtual(params) = self {
            return format!("{} {}", params.inner.to_print_string(), DataType::Reference.colorize(&format!("[{}]", params.hash.mini())));
        } else {
            DataType::Reference.colorize(&self.to_file_string())
        }
    }
}

impl_file_string_from_str!(Reference);
impl_file_string_serialization!(Reference);
