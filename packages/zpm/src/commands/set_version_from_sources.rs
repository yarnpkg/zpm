use std::sync::LazyLock;

use regex::Regex;
use zpm_utils::{DataType, Hash64, Path, ToFileString};
use clipanion::cli;

use crate::{error::Error, script::ScriptEnvironment};

static PR_REGEXP: LazyLock<Regex>
    = LazyLock::new(|| Regex::new(r"^[0-9]+$").unwrap());

fn get_branch_ref(branch: &str) -> String {
    if PR_REGEXP.is_match(branch) {
        format!("pull/{}/head", branch)
    } else {
        branch.to_string()
    }
}

/// Set the version of Yarn to use with the local project from the sources of the ZPM repository
///
/// This command will clone the ZPM repository, build the bundle and switch to it.
///
#[cli::command]
#[cli::path("set", "version", "from", "sources")]
#[cli::category("Configuration commands")]
pub struct SetVersionFromSources {
    #[cli::option("--path")]
    install_path: Option<String>,

    #[cli::option("--repository", default = "git@github.com/yarnpkg/zpm.git".to_string())]
    repository: String,

    #[cli::option("--branch", default = "main".to_string())]
    branch: String,

    #[cli::option("-n,--dry-run", default = false)]
    dry_run: bool,

    #[cli::option("-f,--force", default = false)]
    force: bool,
}

impl SetVersionFromSources {
    pub async fn execute(&self) -> Result<(), Error> {
        check_rustup().await?;

        let target = if let Some(install_path) = &self.install_path {
            Path::try_from(install_path.as_str())?
        } else {
            let hash
                = Hash64::from_string(&self.repository);

            Path::temp_root_dir()?
                .with_join_str(format!("zpm-sources/{}", hash.to_file_string()))
        };

        prepare_repo(self, &target).await?;

        println!();
        println!("Building a fresh bundle");
        println!();

        let bundle_path
            = target.with_join_str("target/release/yarn-bin");

        if !bundle_path.fs_exists() {
            run_build(&target).await?;

            println!();
        }

        if self.dry_run {
            return Ok(());
        }

        let mut env
            = ScriptEnvironment::new()?;

        run_command(&mut env, "yarn", &["switch", "link", bundle_path.as_str()]).await?;

        Ok(())
    }
}

async fn prepare_repo(spec: &SetVersionFromSources, target: &Path) -> Result<(), Error> {
    let mut ready
        = false;

    if !spec.force && target.with_join_str(".git").fs_exists() {
        println!("Fetching the latest commits");
        println!();

        match run_update(spec, target).await {
            Ok(_) => {
                ready = true;
            },

            Err(_) => {
                println!();
                println!("Repository update failed; we'll try to regenerate it");
            }
        }
    }

    if !ready {
        println!("Cloning the remote repository");
        println!();

        if target.fs_exists() {
            target.fs_rm()?;
        }

        target
            .fs_create_dir_all()?;

        run_clone(spec, target).await?;
    }

    Ok(())
}

async fn run_command(env: &mut ScriptEnvironment, command: &str, args: &[&str]) -> Result<(), Error> {
    println!("{}", DataType::Code.colorize(&format!("  $ {} {}", command, args.join(" "))));
    env.run_exec(command, args).await?.ok()?;

    Ok(())
}

async fn run_clone(spec: &SetVersionFromSources, target: &Path) -> Result<(), Error> {
    let mut env
        = ScriptEnvironment::new()?
            .with_cwd(target.clone());

    run_command(&mut env, "git", &["init", target.as_str()]).await?;
    run_command(&mut env, "git", &["remote", "add", "origin", spec.repository.as_str()]).await?;
    run_command(&mut env, "git", &["fetch", "origin", "--depth=1", get_branch_ref(&spec.branch).as_str()]).await?;
    run_command(&mut env, "git", &["reset", "--hard", "FETCH_HEAD"]).await?;

    Ok(())
}

async fn run_update(spec: &SetVersionFromSources, target: &Path) -> Result<(), Error> {
    let mut env
        = ScriptEnvironment::new()?
            .with_cwd(target.clone());

    run_command(&mut env, "git", &["fetch", "origin", "--depth=1", get_branch_ref(&spec.branch).as_str(), "--force"]).await?;
    run_command(&mut env, "git", &["reset", "--hard", "FETCH_HEAD"]).await?;
    run_command(&mut env, "git", &["clean", "-dfx", "-e", "target"]).await?;

    Ok(())
}

async fn run_build(target: &Path) -> Result<(), Error> {
    let mut env
        = ScriptEnvironment::new()?
            .with_cwd(target.clone());

    run_command(&mut env, "cargo", &["build", "--release"]).await?;

    Ok(())
}

async fn check_rustup() -> Result<(), Error> {
    let mut env
        = ScriptEnvironment::new()?;

    let result = env
        .run_exec("rustup", ["--version"]).await;

    let success
        = result.map_or(false, |result| result.success());

    if !success {
        return Err(Error::MissingRustup);
    }

    Ok(())
}
