use clipanion::cli;
use zpm_utils::DataType;

use crate::{
    error::Error,
    http_npm::{self, NpmHttpParams},
    project::Project,
    report::{with_report_result, StreamReport, StreamReportConfig},
};

use super::{registry_auth, StageId};

/// Reject a staged package version.
///
/// This command permanently removes a package version from the configured npm publish registry's staging area.
/// If the registry requires two-factor authentication, use `--otp` or enter the code when prompted.
///
#[cli::command]
#[cli::path("npm", "stage", "reject")]
#[cli::category("Npm-related commands")]
pub struct Reject {
    /// The UUID of the staged package version
    stage_id: StageId,

    /// One-time password to use when the registry requires two-factor authentication
    #[cli::option("--otp")]
    otp: Option<String>,
}

impl Reject {
    pub async fn execute(&self) -> Result<(), Error> {
        let project
            = Project::new(None).await?;

        let report
            = StreamReport::new(StreamReportConfig::from_config(&project.config));

        with_report_result(report, async {
            let (registry, authorization)
                = registry_auth(&project).await?;

            let pretty_stage_id
                = DataType::Code.colorize(&self.stage_id.0);

            println!("Rejecting staged package {}...", pretty_stage_id);

            http_npm::delete(&NpmHttpParams {
                http_client: &project.http_client,
                registry: &registry,
                path: &format!("/-/stage/{}", self.stage_id.0),
                authorization: authorization.as_deref(),
                otp: self.otp.as_deref(),
            }).await?;

            println!("Staged package {} has been rejected.", pretty_stage_id);

            Ok(())
        }).await
    }
}
