use std::{collections::BTreeMap, sync::LazyLock};

use tokio::{process::Command, sync::Mutex};
use zpm_utils::{DataType, FromFileString, Path, ToFileString};

use crate::{
    error::Error,
    report::{current_report, PromptType, StreamReport, StreamReportConfig},
};

type TrustPromptResultCache = BTreeMap<Path, bool>;

static TRUST_PROMPT_RESULTS: LazyLock<Mutex<TrustPromptResultCache>> = LazyLock::new(|| Mutex::new(BTreeMap::new()));

#[derive(Clone, Copy, Debug)]
pub enum ProjectTrustReason {
    ConfigurationInterpolation,
    InstallScripts,
}

impl ProjectTrustReason {
    fn required_error(self, project_cwd: Path) -> Error {
        match self {
            Self::ConfigurationInterpolation => Error::ProjectTrustRequiredForConfigurationInterpolation(project_cwd),
            Self::InstallScripts => Error::ProjectTrustRequired(project_cwd),
        }
    }

    fn not_trusted_error(self, project_cwd: Path) -> Error {
        match self {
            Self::ConfigurationInterpolation => Error::ProjectNotTrustedForConfigurationInterpolation(project_cwd),
            Self::InstallScripts => Error::ProjectNotTrusted(project_cwd),
        }
    }

    fn prompt_message(self, project_cwd: &Path) -> String {
        match self {
            Self::ConfigurationInterpolation => format!(
                "Yarn needs to interpolate values in the configuration file.\nAttackers could use this mechanism to leak environment variables.\n\nDo you trust the project in {}?",
                DataType::Path.colorize(&project_cwd.to_home_string()),
            ),

            Self::InstallScripts => format!(
                "Yarn needs to run potentially dangerous commands to complete the installation.\nAttackers could use this mechanism to run arbitrary code.\n\nDo you trust the project in {}?",
                DataType::Path.colorize(&project_cwd.to_home_string()),
            ),
        }
    }
}

fn get_cached_trust_prompt_result(cache: &TrustPromptResultCache, project_cwd: &Path) -> Option<bool> {
    cache.get(project_cwd).copied()
}

fn set_cached_trust_prompt_result(cache: &mut TrustPromptResultCache, project_cwd: &Path, trusted: bool) {
    cache.insert(project_cwd.clone(), trusted);
}

fn get_switch_path() -> Option<Path> {
    std::env::var(zpm_switch::YARNSW_PATH_ENV)
        .ok()
        .and_then(|path| Path::from_file_string(&path).ok())
}

async fn check_project_trust(switch_path: &Path, project_cwd: &Path) -> Result<Option<bool>, Error> {
    let status
        = Command::new(switch_path.to_file_string())
            .args(["switch", "trust", "--check", project_cwd.as_str()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await?;

    match status.code() {
        Some(0) => Ok(Some(true)),
        Some(2) => Ok(Some(false)),
        Some(3) => Ok(None),
        _ => Err(Error::ChildProcessFailed("yarn switch trust --check".to_string())),
    }
}

async fn trust_project(switch_path: &Path, project_cwd: &Path) -> Result<(), Error> {
    let status
        = Command::new(switch_path.to_file_string())
            .args(["switch", "trust", "--set", "true", project_cwd.as_str()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await?;

    if status.success() {
        Ok(())
    } else {
        Err(Error::ChildProcessFailed("yarn switch trust --set".to_string()))
    }
}

async fn prompt_project_trust(project_cwd: &Path, reason: ProjectTrustReason) -> Result<bool, Error> {
    if !zpm_utils::is_terminal() {
        return Err(reason.required_error(project_cwd.clone()));
    }

    let prompt
        = PromptType::Confirm(reason.prompt_message(project_cwd));

    let report_guard
        = current_report().await;

    if let Some(report) = report_guard.as_ref() {
        let answer
            = report.prompt(prompt).await;

        return Ok(answer == "true");
    }

    drop(report_guard);

    let report
        = StreamReport::new(StreamReportConfig::default());

    let answer
        = report.prompt(prompt).await;

    report.close();

    Ok(answer == "true")
}

pub async fn ensure_project_trusted(project_cwd: &Path, reason: ProjectTrustReason) -> Result<(), Error> {
    let Some(switch_path) = get_switch_path() else {
        return Ok(());
    };

    match check_project_trust(&switch_path, project_cwd).await? {
        Some(true) => return Ok(()),
        Some(false) => return Err(reason.not_trusted_error(project_cwd.clone())),
        None => (),
    }

    let mut prompt_results
        = TRUST_PROMPT_RESULTS.lock().await;

    let trusted = match get_cached_trust_prompt_result(&prompt_results, project_cwd) {
        Some(trusted) => trusted,
        None => {
            let trusted
                = prompt_project_trust(project_cwd, reason).await?;

            if trusted {
                trust_project(&switch_path, project_cwd).await?;
            }

            set_cached_trust_prompt_result(&mut prompt_results, project_cwd, trusted);

            trusted
        },
    };

    match trusted {
        true => Ok(()),
        false => Err(reason.not_trusted_error(project_cwd.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_prompt_result_cache_is_scoped_by_project_cwd() {
        let first_project
            = Path::from_file_string("/tmp/first-project").unwrap();
        let second_project
            = Path::from_file_string("/tmp/second-project").unwrap();
        let third_project
            = Path::from_file_string("/tmp/third-project").unwrap();
        let mut cache
            = TrustPromptResultCache::new();

        set_cached_trust_prompt_result(&mut cache, &first_project, true);
        set_cached_trust_prompt_result(&mut cache, &second_project, false);

        assert_eq!(get_cached_trust_prompt_result(&cache, &first_project), Some(true));
        assert_eq!(get_cached_trust_prompt_result(&cache, &second_project), Some(false));
        assert_eq!(get_cached_trust_prompt_result(&cache, &third_project), None);
    }
}
