use std::{process::{Command, ExitStatus, Stdio}, sync::Arc};

use clipanion::cli;
use zpm_utils::ToFileString;

use crate::{config::validate_yarn_version, cwd::{get_fake_cwd, get_final_cwd}, errors::Error, install::install_package_manager, ipc::YARNSW_PATH_ENV, manifest::{find_closest_package_manager, PackageManagerReference, VersionPackageManagerReference}, yarn::resolve_selector, yarn_enums::Selector};

/// Run a command with an explicit Yarn selector
///
/// This command resolves the selector, installs the matching Yarn binary if needed, and forwards the remaining arguments to it.
///
#[cli::command(proxy)]
#[cli::path("switch")]
#[cli::category("General commands")]
#[derive(Debug)]
pub struct ExplicitCommand {
    /// Yarn selector to execute
    selector: Selector,

    /// Yarn command and arguments to forward
    args: Vec<String>,
}

impl ExplicitCommand {
    pub async fn run(reference: &PackageManagerReference, args: &[String]) -> Result<ExitStatus, Error> {
        if let PackageManagerReference::Version(params) = reference {
            validate_yarn_version(&params.version)?;
        }

        let mut binary = match reference {
            PackageManagerReference::Version(params)
                => install_package_manager(params).await?,

            PackageManagerReference::Local(params)
                => Command::new(params.path.to_file_string()),
        };

        binary.stdout(Stdio::inherit());
        binary.args(args);

        if let Ok(switch_path) = std::env::current_exe() {
            binary.env(YARNSW_PATH_ENV, switch_path);
        }

        let mut child
            = binary.spawn()
                .map_err(|err| Error::FailedToExecuteBinary(binary.get_program().to_string_lossy().to_string(), Arc::new(err)))?;

        // Ignore SIGINT/SIGTERM while waiting for the child process.
        // This ensures the child's exit code is properly propagated
        // instead of the parent being killed by a signal.
        // Note: We must spawn BEFORE setting SIG_IGN, otherwise the child
        // inherits the ignored signal disposition and won't receive signals.
        #[cfg(unix)]
        let _guard = zpm_utils::IgnoreSignals::new();

        let exit_code
            = child.wait()
                .map_err(|err| Error::FailedToExecuteBinary(binary.get_program().to_string_lossy().to_string(), Arc::new(err)))?;

        Ok(exit_code)
    }

    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let lookup_path
            = get_final_cwd()?;

        let find_result
            = find_closest_package_manager(&lookup_path)?;

        if let Some(detected_root_path) = find_result.detected_root_path {
            std::env::set_var("YARNSW_DETECTED_ROOT", detected_root_path.to_file_string());
        }

        let mut args
            = self.args.clone();

        // Don't forget to add back the cwd parameter that was removed earlier on!
        if let Some(cwd) = get_fake_cwd() {
            args.insert(0, cwd.to_file_string());
        }

        let version
            = resolve_selector(&self.selector).await?;

        let reference
            = VersionPackageManagerReference {version};

        ExplicitCommand::run(&reference.into(), &args).await
    }
}
