use thiserror::Error;
use zpm_macro_enum::zpm_enum;
use zpm_primitives::IdentGlob;
use zpm_utils::impl_file_string_from_str;

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
    #[pattern(spec = r"^(?<path>(?:\.{0,2}|[^@{}*/]+)/.*)$")]
    Path {
        path: zpm_utils::Glob,
    },

    #[pattern(spec = r"^(?<ident>.*)$")]
    Ident {
        ident: IdentGlob,
    },
}

impl WorkspaceGlob {
    pub fn check(&self, workspace: &Workspace) -> bool {
        match self {
            WorkspaceGlob::Ident(params)
                => params.ident.check(&workspace.name),

            WorkspaceGlob::Path(params)
                => params.path.is_match(&workspace.rel_path.as_str()),
        }
    }
}

impl_file_string_from_str!(WorkspaceGlob);
