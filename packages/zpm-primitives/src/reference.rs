use std::hash::Hash;

use rkyv::Archive;
use zpm_macro_enum::zpm_enum;
#[cfg(test)]
use zpm_utils::FromFileString;
use zpm_utils::{DataType, Hash64, Path, ToFileString, UrlEncoded};

use super::{Ident, Locator};

fn format_patch(inner: &UrlEncoded<Locator>, path: &str, checksum: &Option<Hash64>) -> String {
    match checksum {
        Some(checksum) => format!("patch:{}#{}&checksum={}", inner.to_file_string(), path, checksum.to_file_string()),
        None => format!("patch:{}#{}", inner.to_file_string(), path),
    }
}

fn format_registry(ident: &Ident, version: &zpm_semver::Version, url: Option<&String>) -> String {
    match url {
        Some(url) => format!("npm:{}@{}#{}", ident.to_file_string(), version.to_file_string(), url.to_file_string()),
        None => format!("npm:{}@{}", ident.to_file_string(), version.to_file_string()),
    }
}

fn format_pypi(version: &crate::PypiVersion, url: Option<&String>) -> String {
    match url {
        Some(url) => format!("pypi:{}#{}", version.to_file_string(), url.to_file_string()),
        None => format!("pypi:{}", version.to_file_string()),
    }
}

fn format_pypi_registry(ident: &Ident, version: &crate::PypiVersion, url: Option<&String>) -> String {
    match url {
        Some(url) => format!("pypi:{}@{}#{}", ident.to_file_string(), version.to_file_string(), url.to_file_string()),
        None => format!("pypi:{}@{}", ident.to_file_string(), version.to_file_string()),
    }
}

fn format_local(protocol: &str, path: &str, hash: &Option<Hash64>) -> String {
    match hash {
        Some(hash) => format!("{}:{}#{}", protocol, path, hash.to_file_string()),
        None => format!("{}:{}", protocol, path),
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
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash))]
#[derive_variants(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Archive, rkyv::Serialize, rkyv::Deserialize)]
#[variant_struct_attr(rkyv(derive(PartialEq, Eq, PartialOrd, Ord, Hash)))]
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

    #[pattern(r"npm:(?<ident>(?:@[^#@]+/)?[^#@]+)@(?<version>[^#]*)(?:#(?<url>.*))?")]
    #[to_file_string(|params| format_registry(&params.ident, &params.version, params.url.as_deref()))]
    #[to_print_string(|params| DataType::Reference.colorize(&format_registry(&params.ident, &params.version, params.url.as_deref())))]
    Registry {
        ident: Ident,
        version: zpm_semver::Version,
        url: Option<UrlEncoded<String>>,
    },

    #[pattern(r"pypi:(?<version>[^#]*)(?:#(?<url>.*))?")]
    #[to_file_string(|params| format_pypi(&params.version, params.url.as_deref()))]
    #[to_print_string(|params| DataType::Reference.colorize(&format_pypi(&params.version, params.url.as_deref())))]
    PypiShorthand {
        version: crate::PypiVersion,
        url: Option<UrlEncoded<String>>,
    },

    #[pattern(r"pypi:(?<ident>(?:@[^#@]+/)?[^#@]+)@(?<version>[^#]*)(?:#(?<url>.*))?")]
    #[to_file_string(|params| format_pypi_registry(&params.ident, &params.version, params.url.as_deref()))]
    #[to_print_string(|params| DataType::Reference.colorize(&format_pypi_registry(&params.ident, &params.version, params.url.as_deref())))]
    PypiRegistry {
        ident: Ident,
        version: crate::PypiVersion,
        url: Option<UrlEncoded<String>>,
    },

    #[pattern(r"file:(?<path>.*\.(?:tgz|tar\.gz))(?:#(?<hash>[a-f0-9]*))?")]
    #[to_file_string(|params| format_local("file", &params.path, &params.hash))]
    #[to_print_string(|params| DataType::Reference.colorize(&format_local("file", &params.path, &params.hash)))]
    Tarball {
        path: String,
        hash: Option<Hash64>,
    },

    #[pattern(r"file:(?<path>.*?)(?:#(?<hash>[a-f0-9]*))?")]
    #[to_file_string(|params| format_local("file", &params.path, &params.hash))]
    #[to_print_string(|params| DataType::Reference.colorize(&format_local("file", &params.path, &params.hash)))]
    Folder {
        path: String,
        hash: Option<Hash64>,
    },

    #[pattern(r"link:(?<path>.*)")]
    #[to_file_string(|params| format!("link:{}", params.path))]
    #[to_print_string(|params| DataType::Reference.colorize(&format!("link:{}", params.path)))]
    Link {
        path: String,
    },

    #[pattern(r"portal:(?<path>.*?)(?:#(?<hash>[a-f0-9]*))?")]
    #[to_file_string(|params| format_local("portal", &params.path, &params.hash))]
    #[to_print_string(|params| DataType::Reference.colorize(&format_local("portal", &params.path, &params.hash)))]
    Portal {
        path: String,

        /// Hash of the portal target's manifest. Portals aren't
        /// copied, but their manifest feeds the resolution, so the
        /// locator must change when it does.
        hash: Option<Hash64>,
    },

    #[pattern(r"exec:(?<path>.*?)(?:#(?<hash>[a-f0-9]*))?")]
    #[to_file_string(|params| format_local("exec", &params.path, &params.hash))]
    #[to_print_string(|params| DataType::Reference.colorize(&format_local("exec", &params.path, &params.hash)))]
    Exec {
        path: String,
        hash: Option<Hash64>,
    },

    #[pattern(r"patch:(?<inner>.*)#(?<path>.*)(?:&checksum=(?<checksum>[a-f0-9]*))?$")]
    #[to_file_string(|params| format_patch(&params.inner, &params.path, &params.checksum))]
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
    #[to_file_string(|params| format!("virtual:{}#{}", params.hash.to_file_string(), params.inner.to_file_string()))]
    #[to_print_string(|params| format!("{} {}", params.inner.to_print_string(), DataType::Reference.colorize(&format!("[{}]", params.hash.mini()))))]
    #[struct_attr(rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator + rkyv::ser::Sharing, <__S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
    #[struct_attr(rkyv(deserialize_bounds(__D: rkyv::de::Pooling, <__D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
    #[struct_attr(rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext + rkyv::validation::SharedContext, <__C as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))))]
    Virtual {
        #[rkyv(omit_bounds)]
        inner: Box<Reference>,
        hash: Hash64,
    },

    #[pattern(r"env:(?<hash>[a-f0-9]*)#(?<inner>.*)$")]
    #[to_file_string(|params| format!("env:{}#{}", params.hash.to_file_string(), params.inner.to_file_string()))]
    #[to_print_string(|params| format!("{} {}", params.inner.to_print_string(), DataType::Reference.colorize(&format!("[env:{}]", params.hash.mini()))))]
    #[struct_attr(rkyv(serialize_bounds(__S: rkyv::ser::Writer + rkyv::ser::Allocator + rkyv::ser::Sharing, <__S as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
    #[struct_attr(rkyv(deserialize_bounds(__D: rkyv::de::Pooling, <__D as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source)))]
    #[struct_attr(rkyv(bytecheck(bounds(__C: rkyv::validation::ArchiveContext + rkyv::validation::SharedContext, <__C as rkyv::rancor::Fallible>::Error: rkyv::rancor::Source))))]
    Env {
        #[rkyv(omit_bounds)]
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

        if let Reference::Virtual(params) = self {
            return params.inner.must_bind();
        }

        if let Reference::Env(params) = self {
            return params.inner.must_bind();
        }

        if let Reference::PypiRegistry(params) = self {
            return params.url.as_ref().is_some_and(|url| url.0.starts_with("file:"));
        }

        matches!(&self, Reference::Link(_) | Reference::Portal(_) | Reference::Tarball(_) | Reference::Folder(_) | Reference::Exec(_))
    }

    pub fn is_workspace_reference(&self) -> bool {
        matches!(self.physical_reference(), Reference::WorkspaceIdent(_) | Reference::WorkspacePath(_))
    }

    pub fn is_disk_reference(&self) -> bool {
        matches!(self.physical_reference(), Reference::WorkspaceIdent(_) | Reference::WorkspacePath(_) | Reference::Portal(_) | Reference::Link(_))
    }

    pub fn is_virtual_reference(&self) -> bool {
        matches!(&self, Reference::Virtual(_))
    }

    pub fn is_portal(&self) -> bool {
        matches!(self.physical_reference(), Reference::Portal(_))
    }

    pub fn is_link(&self) -> bool {
        matches!(self.physical_reference(), Reference::Link(_))
    }

    pub fn inner_locator(&self) -> Option<&Locator> {
        // Keep this implementation in sync w/ Range::inner_descriptor

        match self {
            Reference::Patch(params) => {
                Some(&params.inner.0)
            },

            Reference::Virtual(params) => {
                params.inner.inner_locator()
            },

            Reference::Env(params) => {
                params.inner.inner_locator()
            },

            _ => {
                None
            },
        }
    }

    pub fn physical_reference(&self) -> &Reference {
        match self {
            Reference::Virtual(params) => {
                params.inner.physical_reference()
            },

            Reference::Env(params) => {
                params.inner.physical_reference()
            },

            _ => {
                self
            },
        }
    }

    pub fn env_qualified_with_hash(&self, hash: Hash64) -> Reference {
        match self {
            Reference::Virtual(params) => {
                Reference::Virtual(VirtualReference {
                    inner: Box::new(params.inner.env_qualified_with_hash(hash)),
                    hash: params.hash.clone(),
                })
            },

            Reference::Env(params) if params.hash == hash => {
                self.clone()
            },

            _ => {
                Reference::Env(EnvReference {
                    inner: Box::new(self.clone()),
                    hash,
                })
            },
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

            Reference::PypiShorthand(params) => {
                format!("pypi-{}", params.version.to_file_string())
            },

            Reference::PypiRegistry(params) => {
                format!("pypi-{}", params.version.to_file_string())
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

            Reference::Exec(_) => {
                "exec".to_string()
            },

            Reference::Url(_) => {
                "url".to_string()
            },

            Reference::Virtual(_) => {
                "virtual".to_string()
            },

            Reference::Env(params) => {
                params.inner.slug()
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

#[test]
fn test_env_reference_serialization() {
    let hash
        = Hash64::from_data("fork").to_file_string();
    let reference
        = format!("env:{hash}#pypi:1.0.0");

    assert_eq!(reference, Reference::from_file_string(&reference).unwrap().to_file_string());
}

#[test]
fn test_env_reference_physical_reference() {
    let hash
        = Hash64::from_data("fork");
    let reference
        = Reference::from_file_string(&format!("env:{}#pypi:1.0.0", hash.to_file_string())).unwrap();

    assert_eq!("pypi:1.0.0", reference.physical_reference().to_file_string());
    assert_eq!("pypi-1.0.0", reference.slug());
}

#[test]
fn test_env_reference_preserves_virtual_outer_wrapper() {
    let fork_hash
        = Hash64::from_data("fork");
    let peer_hash
        = Hash64::from_data("peer");
    let reference
        = Reference::from_file_string(&format!("virtual:{}#pypi:1.0.0", peer_hash.to_file_string())).unwrap();

    assert_eq!(
        format!("virtual:{}#env:{}#pypi:1.0.0", peer_hash.to_file_string(), fork_hash.to_file_string()),
        reference.env_qualified_with_hash(fork_hash).to_file_string(),
    );
}
