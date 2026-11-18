use std::process::ExitStatus;

use clipanion::cli;
use zpm_utils::PATH_LIST_SEPARATOR;

use crate::{cargo::toolchain, error::Error, project, script::ScriptEnvironment};

/// Run Cargo with the toolchain of the current workspace
///
/// This command runs `cargo` from the Rust toolchain the workspace asks for through its `rust-toolchain.toml` file (or the `rustToolchain`
/// setting), which Yarn downloads if the machine doesn't have it. The dependencies are installed first if needed, so that Cargo builds from
/// the crates Yarn manages, without accessing the network.
///
/// The other tools of the toolchain (`rustc`, `clippy`, `rustfmt` when the workspace lists them as components) are in the `PATH` of the
/// command, and of the workspace's scripts.
///
/// ## Examples
///
/// Build the crates of the current workspace
///
/// `yarn cargo build --release`
///
/// Run the tests of a crate
///
/// `yarn cargo test -p my-crate`
///
#[cli::command(proxy)]
#[cli::path("cargo")]
#[cli::category("Scripting commands")]
pub struct Cargo {
    /// Arguments to pass to Cargo
    args: Vec<String>,
}

impl Cargo {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let mut project
            = project::Project::new(None).await?;

        project
            .lazy_install().await?;

        let cwd
            = project.project_cwd.with_join(&project.shell_cwd);

        let toolchain
            = toolchain::ensure_toolchain(&project, &cwd).await?
                .ok_or_else(|| Error::CargoError("No Rust toolchain matching this workspace was found on the system, and rustDownloads is disabled".to_string()))?;

        let path = match std::env::var("PATH").ok().filter(|path| !path.is_empty()) {
            Some(path) => format!("{}{}{}", toolchain.bin_dir.to_native_string(), PATH_LIST_SEPARATOR, path),
            None => toolchain.bin_dir.to_native_string(),
        };

        // Cargo picks the compiler from the PATH (or from rustup, which
        // must not get to choose another toolchain than ours)
        let mut env = ScriptEnvironment::new()?
            .with_project(&project)
            .with_package(&project, &project.active_package()?)?
            .enable_shell_forwarding()
            .enable_signal_delegation()
            .with_cwd(cwd)
            .with_env_variable("PATH", &path);

        if toolchain.is_managed {
            env = env.with_env_variable("RUSTC", &toolchain.binary("rustc").to_native_string());
        }

        let result
            = env.run_exec(&toolchain.binary("cargo").to_native_string(), &self.args).await?;

        Ok(result.into())
    }
}
