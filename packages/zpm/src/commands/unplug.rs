use clipanion::cli;
use zpm_parsers::{Document, JsonDocument, Value};
use zpm_primitives::Ident;
use zpm_utils::ToFileString;

use crate::{
    error::Error,
    project,
};

/// Requests a package to be materialized on disk
///
/// This command will add the selectors matching the specified patterns to the list of packages that must be unplugged when installed.
///
/// A package being unplugged means that instead of being referenced directly through its archive, it will be unpacked at install time in the
/// directory configured via `pnpUnpluggedFolder`. Note that unpacking packages this way is generally not recommended because it'll make it harder to
/// store your packages within the repository. However, it's a good approach to quickly and safely debug some packages, and can even sometimes be
/// required depending on the context (for example when the package contains shellscripts).
///
/// Running the command will set a persistent flag inside your top-level `package.json`, in the `dependenciesMeta` field. As such, to undo its effects,
/// you'll need to revert the changes made to the manifest and run `yarn install` to apply the modification.
///
/// By default, only direct dependencies from the current workspace are affected. If `-A,--all` is set, direct dependencies from the entire project are
/// affected. Using the `-R,--recursive` flag will affect transitive dependencies as well as direct ones.
///
/// This command accepts glob patterns inside the scope and name components (not the range). Make sure to escape the patterns to prevent your own
/// shell from trying to expand them.
///
#[cli::command]
#[cli::path("unplug")]
#[cli::category("Dependency management")]
pub struct Unplug {
    /// Revert the changes made to the manifest
    #[cli::option("--revert", default = false)]
    revert: bool,

    /// The identifiers to unplug
    identifiers: Vec<Ident>,
}

impl Unplug {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = project::Project::new(None).await?;

        let manifest_path = project.project_cwd
            .with_join_str(project::MANIFEST_NAME);

        let manifest_content = manifest_path
            .fs_read_prealloc()?;

        let mut document
            = JsonDocument::new(manifest_content)?;

        for identifier in &self.identifiers {
            document.set_path(
                &zpm_parsers::Path::from_segments(vec!["dependenciesMeta".to_string(), identifier.to_file_string(), "unplugged".to_string()]),
                if self.revert {Value::Undefined} else {Value::Bool(true)},
            )?;
        }

        manifest_path
            .fs_change(&document.input, false)?;

        let mut project
            = project::Project::new(None).await?;

        project.run_install(project::RunInstallOptions {
            ..Default::default()
        }).await?;

        Ok(())
    }
}
