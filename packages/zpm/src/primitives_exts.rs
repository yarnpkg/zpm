use zpm_primitives::Range;

pub struct RangeDetails {
    /**
     * The descriptor requires a binding to work. This is usually because they
     * rely on paths (such as file: or link:), as those paths are relative to
     * the parent package.
     */
    pub require_binding: bool,

    /**
     * The descriptor must be fetched before being resolved. This is for
     * packages that don't have metadata endpoints such as git: or file:.
     */
    pub fetch_before_resolve: bool,

    /**
     * The resolution for this descriptor should be discarded at every new
     * install. This is for packages whose metadata are extracted from the
     * disk and whose information could be stale.
     */
    pub transient_resolution: bool,
}

pub trait RangeExt {
    fn details(&self) -> RangeDetails;
}

impl RangeExt for Range {
    #[inline]
    fn details(&self) -> RangeDetails {
        match self {
            Range::AnonymousSemver(_) |
            Range::AnonymousTag(_) |
            Range::RegistrySemver(_) |
            Range::RegistryTag(_) => {
                RangeDetails {
                    require_binding: false,
                    fetch_before_resolve: false,
                    transient_resolution: false,
                }
            },

            Range::Folder(_) |
            Range::Tarball(_) => {
                RangeDetails {
                    require_binding: true,
                    fetch_before_resolve: true,
                    // TODO: This shouldn't be transient unless the parent is a workspace
                    transient_resolution: true,
                }
            },

            Range::Git(_) |
            Range::Url(_) => {
                RangeDetails {
                    require_binding: false,
                    fetch_before_resolve: true,
                    transient_resolution: false,
                }
            },

            Range::Link(_) |
            Range::Portal(_) => {
                RangeDetails {
                    require_binding: true,
                    fetch_before_resolve: false,
                    transient_resolution: true,
                }
            },

            Range::MissingPeerDependency => {
                RangeDetails {
                    require_binding: false,
                    fetch_before_resolve: false,
                    transient_resolution: false,
                }
            },

            Range::Patch(params) => {
                RangeDetails {
                    require_binding: params.inner.0.range.details().require_binding || (params.path.as_str() != "<builtin>" && !params.path.as_str().starts_with("~/")),
                    fetch_before_resolve: false,
                    // TODO: This shouldn't be transient unless the parent is a workspace
                    transient_resolution: true,
                }
            },

            Range::Virtual(_) => {
                // Virtual ranges only appear after the install has completed,
                // so by this point none of these fields should matter anymore.
                RangeDetails {
                    require_binding: false,
                    fetch_before_resolve: false,
                    transient_resolution: false,
                }
            },

            Range::WorkspaceIdent(_) |
            Range::WorkspaceMagic(_) |
            Range::WorkspacePath(_) |
            Range::WorkspaceSemver(_) => {
                RangeDetails {
                    require_binding: false,
                    fetch_before_resolve: false,
                    transient_resolution: true,
                }
            },
        }
    }
}
