pub trait RangeExt {
    fn must_bind(&self) -> bool;
    fn must_fetch_before_resolve(&self) -> bool;
    fn is_transient_resolution(&self) -> bool;
}

impl RangeExt for Range {
    fn must_bind(&self) -> bool {
        // Keep this implementation in sync w/ Reference::must_bind

        if let Range::Patch(params) = self {
            return params.inner.0.range.must_bind() || (params.path.as_str() != "<builtin>" && !params.path.as_str().starts_with("~/"));
        }

        matches!(&self, Range::Link(_) | Range::Portal(_) | Range::Tarball(_) | Range::Folder(_))
    }

    fn must_fetch_before_resolve(&self) -> bool {
        matches!(&self, Range::Git(_) | Range::Folder(_) | Range::Tarball(_) | Range::Url(_))
    }

    fn is_transient_resolution(&self) -> bool {
        matches!(&self, Range::Link(_) | Range::Portal(_) | Range::Tarball(_) | Range::Folder(_) | Range::Patch(_) | Range::WorkspaceIdent(_) | Range::WorkspaceMagic(_) | Range::WorkspacePath(_) | Range::WorkspaceSemver(_))
    }
}
