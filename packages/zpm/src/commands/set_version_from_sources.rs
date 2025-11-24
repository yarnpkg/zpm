use std::sync::LazyLock;

use regex::Regex;
use zpm_utils::{Hash64, Path, ToFileString};
use clipanion::cli;

use crate::{error::Error, git_utils::get_commit_hash, script::ScriptEnvironment};

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

    #[cli::option("--repository", default = "https://github.com/yarnpkg/zpm.git".to_string())]
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

        let commit_hash
            = get_commit_hash(&target, "HEAD").await?;

        let bundle_path
            = target.with_join_str(format!("target/release/yarn-{}", commit_hash));

        if !bundle_path.fs_exists() {
            run_build(&target).await?;

            println!();
        }

        if self.dry_run {
            return Ok(());
        }

        let mut env
            = ScriptEnvironment::new()?
                .enable_shell_forwarding();

        env.run_exec("yarn", ["switch", "link", bundle_path.as_str()]).await?.ok()?;

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

async fn run_clone(spec: &SetVersionFromSources, target: &Path) -> Result<(), Error> {
    let mut env
        = ScriptEnvironment::new()?
            .with_cwd(target.clone())
            .enable_shell_forwarding();

    env.run_exec("git", vec!["init", target.as_str()]).await?.ok()?;
    env.run_exec("git", vec!["remote", "add", "origin", spec.repository.as_str()]).await?.ok()?;
    env.run_exec("git", vec!["fetch", "origin", "--depth=1", get_branch_ref(&spec.branch).as_str()]).await?.ok()?;
    env.run_exec("git", vec!["reset", "--hard", "FETCH_HEAD"]).await?.ok()?;

    Ok(())
}

async fn run_update(spec: &SetVersionFromSources, target: &Path) -> Result<(), Error> {
    let mut env
        = ScriptEnvironment::new()?
            .with_cwd(target.clone())
            .enable_shell_forwarding();

    env.run_exec("git", vec!["fetch", "origin", "--depth=1", get_branch_ref(&spec.branch).as_str(), "--force"]).await?.ok()?;
    env.run_exec("git", vec!["reset", "--hard", "FETCH_HEAD"]).await?.ok()?;
    env.run_exec("git", vec!["clean", "-dfx", "-e", "target"]).await?.ok()?;

    Ok(())
}

async fn run_build(target: &Path) -> Result<(), Error> {
    let mut env
        = ScriptEnvironment::new()?
            .with_cwd(target.clone())
            .enable_shell_forwarding();

    env.run_exec("cargo", vec!["build", "--release"]).await?.ok()?;

    Ok(())
}
