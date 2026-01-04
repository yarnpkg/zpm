use std::process::ExitCode;

use clipanion::cli;
use itertools::Itertools;
use zpm_macro_enum::zpm_enum;
use zpm_utils::{IoResultExt, Path, ToFileString};

use crate::{error::Error, script::ScriptEnvironment};

fn remove_list(list: &[&str]) -> Result<(), Error> {
    let current_dir
        = Path::current_dir()?;

    for path in list {
        current_dir
            .with_join_str(*path)
            .fs_rm()
            .ok_missing()?;
    }

    Ok(())
}

async fn run_cli(args: &[&str]) -> Result<(), Error> {
    let args = args.iter()
        .map(|s| s.to_string())
        .collect_vec();

    if Box::pin(crate::commands::run_default(Some(args))).await == ExitCode::SUCCESS {
        Ok(())
    } else {
        Err(Error::ReplaceMe)
    }
}

/// Benchmark names corresponding to available test fixtures
#[zpm_enum(or_else = |s| Err(Error::InvalidBenchName(s.to_string())))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchName {
    #[literal("gatsby")]
    Gatsby,

    #[literal("next")]
    Next,
}

impl BenchName {
    pub fn get_manifest(&self) -> String {
        match self {
            BenchName::Gatsby => {
                include_str!("../../../../../scripts/benchmarks/gatsby.json").to_string()
            },
            BenchName::Next => {
                include_str!("../../../../../scripts/benchmarks/next.json").to_string()
            },
        }
    }
}

/// Benchmark modes corresponding to different installation scenarios
#[zpm_enum(or_else = |s| Err(Error::InvalidBenchMode(s.to_string())))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchMode {
    /// Full cold install: no cache, no lockfile
    #[literal("install-full-cold")]
    InstallFullCold,

    /// Install with cache but no lockfile
    #[literal("install-cache-only")]
    InstallCacheOnly,

    /// Install with cache and lockfile
    #[literal("install-cache-and-lock")]
    InstallCacheAndLock,

    /// Install when already ready (add a dummy package)
    #[literal("install-ready")]
    InstallReady,
}

impl BenchMode {
    pub async fn prepare_folder(&self) -> Result<(), Error> {
        let global_folder
            = Path::current_dir()?
                .with_join_str(".yarn-global");

        let yarnrc_yml
            = Path::current_dir()?
                .with_join_str(".yarnrc.yml");

        yarnrc_yml
            .fs_append_text(format!("globalFolder: '{}'\n", global_folder.to_file_string()))?
            .fs_append_text("enableImmutableInstalls: false\n")?
            .fs_append_text("enableScripts: false\n")?;

        if self == &BenchMode::InstallReady {
            let dummy_pkg_json
                = Path::current_dir()?
                    .with_join_str("dummy-pkg/package.json");

            dummy_pkg_json
                .fs_create_parent()?
                .fs_write("{\"name\": \"dummy-pkg\"}")?;
        }

        run_cli(&["install"]).await?;

        self.cleanup_folder().await?;

        Ok(())
    }

    pub async fn cleanup_folder(&self) -> Result<(), Error> {
        match self {
            BenchMode::InstallFullCold => {
                remove_list(&[".yarn", ".pnp.cjs", ".pnp.loader.mjs", "yarn.lock", ".yarn-global"])
            },
            BenchMode::InstallCacheOnly => {
                remove_list(&[".yarn", ".pnp.cjs", ".pnp.loader.mjs", "yarn.lock"])
            },
            BenchMode::InstallCacheAndLock => {
                remove_list(&[".yarn", ".pnp.cjs", ".pnp.loader.mjs"])
            },
            BenchMode::InstallReady => {
                run_cli(&["remove", "dummy-pkg"]).await
            },
        }
    }

    pub async fn run_iteration(&self) -> Result<(), Error> {
        match self {
            BenchMode::InstallReady => {
                run_cli(&["add", "dummy-pkg@link:./dummy-pkg", "--silent"]).await
            },
            _ => {
                run_cli(&["install", "--silent"]).await
            },
        }
    }
}

#[cli::command]
#[cli::path("debug", "bench")]
pub struct BenchRun {
    name: BenchName,
    mode: BenchMode,
}

impl BenchRun {
    pub async fn execute(&self) -> Result<(), Error> {
        let current_exec_string
            = Path::current_exe()?
                .to_file_string();

        let mode_string
            = self.mode.to_file_string();
        let name_string
            = self.name.to_file_string();

        let bench_json_string
            = Path::current_dir()?
                .with_join_str(format!("bench-{name_string}-{mode_string}.json"))
                .to_file_string();

        let temp_directory
            = Path::temp_dir()?;

        ScriptEnvironment::new()?
            .with_cwd(temp_directory.clone())
            .run_exec(&current_exec_string, ["debug", "bench", &mode_string, "--prepare", &name_string])
            .await?
            .ok()?;

        let hyperfine_args = vec![
            "--min-runs=30".to_string(),
            "--warmup=4".to_string(),
            format!("--export-json={bench_json_string}"),
            format!("--prepare={current_exec_string} debug bench {mode_string} --cleanup"),
            format!("{current_exec_string} debug bench {mode_string} --iteration"),
        ];

        ScriptEnvironment::new()?
            .enable_shell_forwarding()
            .with_cwd(temp_directory)
            .run_exec("hyperfine", hyperfine_args)
            .await?
            .ok()?;

        Ok(())
    }
}

#[cli::command]
#[cli::path("debug", "bench")]
pub struct BenchPrepare {
    #[cli::option("--prepare")]
    name: BenchName,

    mode: BenchMode,
}

impl BenchPrepare {
    pub async fn execute(&self) -> Result<(), Error> {
        Path::current_dir()?
            .with_join_str("package.json")
            .fs_write(&self.name.get_manifest())?;

        self.mode.prepare_folder().await?;

        Ok(())
    }
}

#[cli::command]
#[cli::path("debug", "bench")]
pub struct BenchIter {
    #[cli::option("--iteration")]
    _run: bool,

    mode: BenchMode,
}

impl BenchIter {
    pub async fn execute(&self) -> Result<(), Error> {
        self.mode.run_iteration().await?;

        Ok(())
    }
}

#[cli::command]
#[cli::path("debug", "bench")]
pub struct BenchCleanup {
    #[cli::option("--cleanup")]
    _cleanup: bool,

    mode: BenchMode,
}

impl BenchCleanup {
    pub async fn execute(&self) -> Result<(), Error> {
        self.mode.cleanup_folder().await?;

        Ok(())
    }
}
