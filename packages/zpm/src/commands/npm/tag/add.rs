use clipanion::cli;
use zpm_primitives::IdentVersion;
use zpm_utils::{DataType, ToFileString, ToHumanString};

use crate::{
    error::Error,
    http_npm::{self, NpmHttpParams},
    project::Project,
    report::{with_report_result, StreamReport, StreamReportConfig},
};

use super::{dist_tags_path, get_dist_tags, publish_registry_auth};

/// Add a tag for a specific version of a package
///
/// This command adds a tag to the npm registry for a specific version of a package. If the tag already exists, it will be overwritten.
///
/// The request is sent to the registry the package would be published to (the workspace's `publishConfig.registry` if the package is one of
/// the project's workspaces, otherwise `npmPublishRegistry` as configured for its scope or matching package rules) and always requires
/// authentication. If the registry requires two-factor authentication, use `--otp` or enter the code when prompted.
///
/// For example, to add a `beta` tag for version `2.3.4-beta.4` of package `my-pkg`:
///
///   - `yarn npm tag add my-pkg@2.3.4-beta.4 beta`
///
#[cli::command]
#[cli::path("npm", "tag", "add")]
#[cli::category("Npm-related commands")]
pub struct Add {
    /// One-time password to use when the registry requires two-factor authentication
    #[cli::option("--otp")]
    otp: Option<String>,

    /// The package and version to tag, in the form `<name>@<version>`
    package: IdentVersion,

    /// The tag to add
    tag: String,
}

impl Add {
    pub async fn execute(&self) -> Result<(), Error> {
        let ident
            = &self.package.ident;

        // Build metadata isn't kept, which matches what npm does when it
        // publishes a package: registries never key versions with it.
        let version
            = self.package.version.to_file_string();

        let project
            = Project::new(None).await?;

        let report
            = StreamReport::new(StreamReportConfig::from_config(&project.config));

        with_report_result(report, async {
            let (registry, authorization)
                = publish_registry_auth(&project, ident).await?;

            let pretty_ident
                = ident.to_print_string();
            let pretty_version
                = DataType::Range.colorize(&version);
            let pretty_tag
                = DataType::Code.colorize(&self.tag);

            let dist_tags
                = get_dist_tags(&project, &registry, ident, authorization.as_deref()).await?;

            if dist_tags.get(&self.tag) == Some(&version) {
                crate::report::if_active_async(|report| {
                    report.warn(format!("Tag {} is already set to version {}", pretty_tag, pretty_version));
                }).await;
            }

            http_npm::put(&NpmHttpParams {
                http_client: &project.http_client,
                registry: &registry,
                path: &dist_tags_path(ident, Some(&self.tag)),
                authorization: authorization.as_deref(),
                otp: self.otp.as_deref(),
            }, serde_json::to_string(&version).unwrap()).await?;

            crate::report::if_active_async(|report| {
                report.info(format!("Tag {} added to version {} of package {}", pretty_tag, pretty_version, pretty_ident));
            }).await;

            Ok(())
        }).await
    }
}
