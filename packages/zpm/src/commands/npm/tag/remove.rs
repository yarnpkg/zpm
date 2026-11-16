use clipanion::cli;
use zpm_primitives::Ident;
use zpm_utils::{DataType, FromFileString, ToHumanString};

use crate::{
    error::Error,
    http_npm::{self, NpmHttpParams},
    project::Project,
    report::{with_report_result, StreamReport, StreamReportConfig},
};

use super::{dist_tags_path, get_dist_tags, publish_registry_auth};

/// Remove a tag from a package
///
/// This command removes a tag from a package from the npm registry. The `latest` tag cannot be removed.
///
/// The request is sent to the registry the package would be published to (the workspace's `publishConfig.registry` if the package is one of
/// the project's workspaces, otherwise `npmPublishRegistry` as configured for its scope or matching package rules) and always requires
/// authentication. If the registry requires two-factor authentication, use `--otp` or enter the code when prompted.
///
/// ## Examples
///
/// Remove the `beta` tag from package `my-pkg`
///
/// `yarn npm tag remove my-pkg beta`
///
#[cli::command]
#[cli::path("npm", "tag", "remove")]
#[cli::category("Npm-related commands")]
pub struct Remove {
    /// One-time password to use when the registry requires two-factor authentication
    #[cli::option("--otp")]
    otp: Option<String>,

    /// The package to remove the tag from
    package: String,

    /// The tag to remove
    tag: String,
}

impl Remove {
    pub async fn execute(&self) -> Result<(), Error> {
        if self.tag == "latest" {
            return Err(Error::CannotRemoveLatestNpmTag);
        }

        let ident
            = Ident::from_file_string(&self.package)
                .map_err(|_| Error::InvalidIdent(self.package.clone()))?;

        let project
            = Project::new(None).await?;

        let (registry, authorization)
            = publish_registry_auth(&project, &ident).await?;

        let dist_tags
            = get_dist_tags(&project, &registry, &ident, authorization.as_deref()).await?;

        if !dist_tags.contains_key(&self.tag) {
            return Err(Error::NpmTagNotFound {tag: self.tag.clone(), ident});
        }

        let report
            = StreamReport::new(StreamReportConfig::from_config(&project.config));

        with_report_result(report, async {
            http_npm::delete(&NpmHttpParams {
                http_client: &project.http_client,
                registry: &registry,
                path: &dist_tags_path(&ident, Some(&self.tag)),
                authorization: authorization.as_deref(),
                otp: self.otp.as_deref(),
            }).await?;

            crate::report::if_active_async(|report| {
                report.info(format!("Tag {} removed from package {}", DataType::Code.colorize(&self.tag), ident.to_print_string()));
            }).await;

            Ok(())
        }).await
    }
}
