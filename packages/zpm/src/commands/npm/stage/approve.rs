use clipanion::cli;
use zpm_utils::DataType;

use crate::{
    error::Error,
    http_npm::{self, NpmHttpParams},
    project::Project,
    report::{with_report_result, StreamReport, StreamReportConfig},
};

use super::{registry_auth, StageId};

/// Approve a staged package version for publishing.
///
/// This command approves a staged package version, publishing it to the configured npm publish registry.
/// If the registry requires two-factor authentication, use `--otp` or enter the code when prompted.
///
#[cli::command]
#[cli::path("npm", "stage", "approve")]
#[cli::category("Npm-related commands")]
pub struct Approve {
    /// The UUID of the staged package version
    stage_id: StageId,

    /// One-time password to use when the registry requires two-factor authentication
    #[cli::option("--otp")]
    otp: Option<String>,
}

impl Approve {
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

            println!("Approving staged package {}...", pretty_stage_id);

            http_npm::post(&NpmHttpParams {
                http_client: &project.http_client,
                registry: &registry,
                path: &format!("/-/stage/{}/approve", self.stage_id.0),
                authorization: authorization.as_deref(),
                otp: self.otp.as_deref(),
            }, "null".to_string()).await?;

            println!("Staged package {} approved and published successfully.", pretty_stage_id);

            Ok(())
        }).await
    }
}
