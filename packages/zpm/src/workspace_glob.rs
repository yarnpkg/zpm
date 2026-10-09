use thiserror::Error;
use zpm_macro_enum::zpm_enum;
use zpm_primitives::IdentGlob;

use crate::project::Workspace;

#[derive(Debug, Error)]
pub enum WorkspaceGlobError {
    #[error("Invalid workspace glob: {0}")]
    SyntaxError(String),
}

#[zpm_enum(error = WorkspaceGlobError, or_else = |s| Err(WorkspaceGlobError::SyntaxError(s.to_string())))]
#[derive(Debug)]
#[derive_variants(Debug)]
pub enum WorkspaceGlob {
    #[pattern(r"^(?<path>(?:\.{0,2}|[^@{}*/]+)/.*)$")]
    Path {
        path: zpm_utils::Glob,
    },

    #[pattern(r"^(?<ident>.*)$")]
    Ident {
        ident: IdentGlob,
    },
}

impl WorkspaceGlob {
    /// Workspace paths are stored without a `./` prefix (and the root
    /// workspace has an empty path), but path globs are commonly written as
    /// `./packages/*` (as in turbo's `--filter`); match both forms.
    fn check_path(glob: &zpm_utils::Glob, workspace: &Workspace) -> bool {
        let rel_path
            = workspace.rel_path.as_str();

        if glob.is_match(rel_path) {
            return true;
        }

        let dotted_path = match rel_path.is_empty() {
            true => ".".to_string(),
            false => format!("./{}", rel_path),
        };

        glob.is_match(&dotted_path)
    }

    pub fn check(&self, workspace: &Workspace) -> bool {
        match self {
            WorkspaceGlob::Ident(params)
                => params.ident.check(&workspace.name),

            WorkspaceGlob::Path(params)
                => Self::check_path(&params.path, workspace),
        }
    }
}
