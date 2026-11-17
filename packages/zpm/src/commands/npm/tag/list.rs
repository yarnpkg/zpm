use clipanion::cli;
use zpm_primitives::{Descriptor, DescriptorResolution, Ident, Locator};
use zpm_utils::{tree, AbstractValue, FromFileString};

use crate::{
    error::Error,
    http_npm::{self, AuthorizationMode, GetAuthorizationOptions},
    project::{Project, MANIFEST_NAME},
};

use super::get_dist_tags;

/// List all dist-tags of a package
///
/// This command lists all tags of a package from the npm registry, sorted by tag name, along with the version each of them points to.
///
/// If the package is not specified, Yarn will default to the current workspace.
///
/// The tags are read from the registry packages are fetched from (`npmRegistryServer`, or the `npmScopes` and `packageRules` settings that
/// apply to the package), and credentials are sent according to the usual rules (always for scoped packages, otherwise only when `npmAlwaysAuth`
/// is enabled).
///
/// ## Examples
///
/// List all tags of package `my-pkg`
///
/// `yarn npm tag list my-pkg`
///
/// List all tags of the current workspace
///
/// `yarn npm tag list`
///
#[cli::command]
#[cli::path("npm", "tag", "list")]
#[cli::category("Npm-related commands")]
pub struct List {
    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,

    /// The package to list the tags of (defaults to the current workspace)
    package: Option<String>,
}

impl List {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let ident = match &self.package {
            Some(package) => {
                Ident::from_file_string(package)
                    .map_err(|_| Error::InvalidIdent(package.clone()))?
            },

            None => {
                let workspace
                    = project.active_workspace()?;

                workspace.manifest.name.clone()
                    .ok_or_else(|| Error::MissingWorkspaceName(workspace.path.with_join_str(MANIFEST_NAME)))?
            },
        };

        let registry
            = http_npm::get_registry_for_ident(&project.config, Some(&ident), false)?
                .to_string();

        let authorization
            = http_npm::get_authorization(&GetAuthorizationOptions {
                configuration: &project.config,
                http_client: &project.http_client,
                registry: &registry,
                ident: Some(&ident),
                auth_mode: AuthorizationMode::RespectConfiguration,
                allow_oidc: false,
            }).await?;

        let dist_tags
            = get_dist_tags(&project, &registry, &ident, authorization.as_deref()).await?;

        let mut nodes = Vec::new();

        for (tag, version) in dist_tags {
            let descriptor_string
                = format!("{}@{}", ident.as_str(), tag);
            let descriptor
                = Descriptor::from_file_string(&descriptor_string)
                    .map_err(|_| Error::InvalidDescriptor(descriptor_string))?;
            let locator
                = Locator::from_file_string(&format!("{}@npm:{}", ident.as_str(), version))?;

            nodes.push(tree::Node {
                label: None,
                value: Some(AbstractValue::new(DescriptorResolution::new(descriptor, locator))),
                children: None,
            });
        }

        let root = tree::Node {
            label: None,
            value: None,
            children: Some(tree::TreeNodeChildren::Vec(nodes)),
        };

        print!("{}", tree::TreeRenderer::new().render(&root, self.json));

        Ok(())
    }
}
