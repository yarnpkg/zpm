use std::{collections::{BTreeMap, HashMap}, ffi::OsStr, fs::Permissions, hash::{DefaultHasher, Hash, Hasher}, io::Read, os::unix::{fs::PermissionsExt, process::ExitStatusExt}, process::{ExitStatus, Output}, sync::LazyLock};

use arca::{Path, ToArcaPath};
use itertools::Itertools;
use regex::Regex;
use tokio::process::Command;
use zpm_macros::track_time;

use crate::{error::Error, primitives::Locator, project::Project};

static CJS_LOADER_MATCHER: LazyLock<Regex> = LazyLock::new(|| regex::Regex::new(r"\s*--require\s+\S*\.pnp\.c?js\s*").unwrap());
static ESM_LOADER_MATCHER: LazyLock<Regex> = LazyLock::new(|| regex::Regex::new(r"\s*--experimental-loader\s+\S*\.pnp\.loader\.mjs\s*").unwrap());
static JS_EXTENSION: LazyLock<Regex> = LazyLock::new(|| regex::Regex::new(r"\.[cm]?[jt]sx?$").unwrap());

fn make_path_wrapper(bin_dir: &Path, name: &str, argv0: &str, args: Vec<&str>) -> Result<(), Error> {
    if cfg!(windows) {
        let cmd_script = format!(
            r#"@goto #_undefined_# 2>NUL || @title %COMSPEC% & @setlocal & @"{}" {} %*"#,
            argv0,
            args.iter().map(|arg| format!(r#""{}""#, arg.replace(r#"""#, r#""""#))).collect::<Vec<String>>().join(" "),
        );

        bin_dir
            .with_join_str(format!("{}.cmd", name))
            .fs_write_text(&cmd_script)?;
    } else {
        let sh_script = format!(
            "#!/bin/sh\nexec \"{}\" {} \"$@\"\n",
            argv0,
            args.iter().map(|arg| format!("'{}'", arg.replace("'", "'\"'\"'"))).collect_vec().join(" "),
        );

        bin_dir
            .with_join_str(name)
            .fs_write_text(&sh_script)?
            .fs_set_permissions(Permissions::from_mode(0o755))?;
    }

    Ok(())
}

fn is_node_script(p: Path) -> bool {
    let ext = p.extname().unwrap_or_default();

    if JS_EXTENSION.is_match(ext) {
        return true;
    }

    if ext == ".exe" || ext == ".bin" {
        return false;
    }

    let mut buf = [0u8; 4];

    let magic = std::fs::File::open(p.to_path_buf())
        .and_then(|mut fd| fd.read_exact(&mut buf))
        .map(|_| u32::from_be_bytes(buf));

    match magic {
        Err(_) => true,

        // OSX Universal Binary
        // Mach-O
        // ELF
        Ok(0xcafebabe | 0xcffaedfe | 0x7f454c46) => false,

        // DOS MZ Executable
        Ok(n) if (n & 0xffff0000) == 0x4d5a0000 => false,

        _ => true,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BinaryKind {
    Default,
    Node,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Binary {
    pub path: Path,
    pub kind: BinaryKind,
}

impl Binary {
    pub fn new(project: &Project, path_rel: Path) -> Self {
        let path_abs = project.project_cwd
            .with_join(&path_rel);

        let kind = match is_node_script(path_abs.clone()) {
            true => BinaryKind::Node,
            false => BinaryKind::Default,
        };

        Self {
            path: path_abs,
            kind,
        }
    }
}

pub enum ScriptResult {
    Success(Output),
    Failure(Output, String),
}

impl ScriptResult {
    pub fn new_success() -> Self {
        Self::Success(Output {
            status: ExitStatus::from_raw(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }

    pub fn new(output: Output, program: &str) -> Self {
        if output.status.success() {
            Self::Success(output)
        } else {
            Self::Failure(output, program.to_string())
        }
    }

    pub fn success(&self) -> bool {
        matches!(self, Self::Success(_))
    }

    pub fn ok(&self) -> Result<(), Error> {
        if !self.success() {
            println!("{}", String::from_utf8_lossy(&self.output().stderr));
        }
        match self {
            Self::Success(_) => Ok(()),
            Self::Failure(_, program) => Err(Error::ChildProcessFailed(program.clone())),
        }
    }

    pub fn output(&self) -> &Output {
        match self {
            Self::Success(output) => output,
            Self::Failure(output, _) => output,
        }
    }
}

impl Into<ExitStatus> for ScriptResult {
    fn into(self) -> ExitStatus {
        match self {
            ScriptResult::Success(output) => output.status,
            ScriptResult::Failure(output, _) => output.status,
        }
    }
}

pub struct ScriptEnvironment {
    cwd: Path,
    env: HashMap<String, String>,
}

impl Default for ScriptEnvironment {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptEnvironment {
    pub fn new() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap().to_arca(),
            env: HashMap::new(),
        }
    }

    fn prepend_env(&mut self, key: &str, separator: char, value: &str) {
        let current = self.env.entry(key.to_string())
            .or_insert(std::env::var(key).unwrap_or_default());

        if !current.is_empty() {
            current.insert(0, separator);
        }

        current.insert_str(0, value);
    }

    fn append_env(&mut self, key: &str, separator: char, value: &str) {
        let current = self.env.entry(key.to_string())
            .or_insert(std::env::var(key).unwrap_or_default());

        if !current.is_empty() {
            current.push(separator)
        }

        current.push_str(value);
    }

    pub fn with_project(mut self, project: &Project) -> Self {
        if let Some(pnp_path) = project.pnp_path().if_exists() {
            self.append_env("NODE_OPTIONS", ' ', &format!("--require {}", pnp_path.to_string()));
        }

        if let Some(pnp_loader_path) = project.pnp_loader_path().if_exists() {
            self.append_env("NODE_OPTIONS", ' ', &format!("--experimental-loader {}", pnp_loader_path.to_string()));
        }

        self.env.insert("PROJECT_CWD".to_string(), project.project_cwd.to_string());
        self.env.insert("INIT_CWD".to_string(), project.project_cwd.with_join(&project.shell_cwd).to_string());

        self
    }

    fn attach_package_variables(&mut self, project: &Project, locator: &Locator) -> Result<(), Error> {
        let install_state = project.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let resolution = install_state.resolution_tree.locator_resolutions.get(locator)
            .expect("Expected active package to have a resolution tree");

        // TODO (but do we really need to do this?): We may return the wrong
        // location when a same package is hoisted in multiple places, since
        // we only return the first one we find.
        //
        let package_location_rel = install_state.locations_by_package.get(locator)
            .expect("Expected the package to be installed");

        let manifest_location_abs = project.project_cwd
            .with_join(package_location_rel)
            .with_join_str("package.json");
        
        self.env.insert("npm_package_name".to_string(), locator.ident.to_string());
        self.env.insert("npm_package_version".to_string(), resolution.version.to_string());
        self.env.insert("npm_package_json".to_string(), manifest_location_abs.to_string());

        Ok(())
    }

    #[track_time]
    fn attach_binaries(&mut self, locator: &Locator, binaries: &BTreeMap<String, Binary>, relative_to: &Path) -> Result<(), Error> {
        let mut hash = DefaultHasher::new();
        binaries.hash(&mut hash);
        let hash = hash.finish();

        let dir = std::env::temp_dir()
            .to_arca()
            .with_join_str(format!("zpm-{}-{}", locator.slug(), hash));

        // We try to reuse directories rather than generate the binaries at
        // every command; I noticed that on OSX the content of these directories
        // is sometimes purged (perhaps because we write in /tmp?), so to avoid
        // that we check whether a known file is still there before blindly
        // using the directory.
        //
        let ready_path = dir
            .with_join_str(".ready");

        if !ready_path.fs_exists() && dir.fs_exists() {
            dir.fs_rm()?;
        }

        if !dir.fs_exists() {
            let nonce = format!("{:08x}", rand::random::<u64>());

            let temp_dir = std::env::temp_dir()
                .to_arca()
                .with_join_str(format!("zpm-temp-{}", nonce));

            temp_dir.fs_create_dir()?;

            temp_dir
                .with_join_str(".ready")
                .fs_write_text("")?;

            let self_path_str = std::env::current_exe()?
                .to_arca()
                .to_string();

            make_path_wrapper(&temp_dir, "run", &self_path_str, vec!["run"])?;
            make_path_wrapper(&temp_dir, "yarn", &self_path_str, vec![])?;
            make_path_wrapper(&temp_dir, "yarnpkg", &self_path_str, vec![])?;
            make_path_wrapper(&temp_dir, "node-gyp", &self_path_str, vec!["run", "--top-level", "node-gyp"])?;

            for (name, binary) in binaries {
                let binary_path_abs = relative_to
                    .with_join(&binary.path)
                    .to_string();

                if binary.kind == BinaryKind::Node {
                    make_path_wrapper(&temp_dir, name, "node", vec![&binary_path_abs])?;
                } else {
                    make_path_wrapper(&temp_dir, name, &binary_path_abs, vec![])?;
                }
            }

            temp_dir
                .fs_rename(&dir)?;
        }

        let bin_dir_str = dir.to_string();
    
        self.prepend_env("PATH", ':', &bin_dir_str);
        self.env.insert("BERRY_BIN_FOLDER".to_string(), bin_dir_str);

        Ok(())
    }

    pub fn with_package(mut self, project: &Project, locator: &Locator) -> Result<Self, Error> {
        let install_state = project.install_state
            .as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let package_cwd_rel = install_state.locations_by_package.get(locator)
            .expect("Expected the package to be installed");

        self.cwd = project.project_cwd
            .with_join(package_cwd_rel);
    
        self.attach_package_variables(project, locator)?;

        let binaries = project.package_visible_binaries(locator)?;
        self.attach_binaries(locator, &binaries, &project.project_cwd)?;

        Ok(self)
    }

    pub fn with_cwd(mut self, cwd: Path) -> Self {
        self.cwd = cwd;
        self
    }

    #[track_time]
    pub async fn run_exec<I, S>(&mut self, program: &str, args: I) -> ScriptResult where I: IntoIterator<Item = S>, S: AsRef<OsStr> {
        let mut cmd = Command::new(program);

        cmd.current_dir(self.cwd.to_path_buf());
        cmd.envs(self.env.iter());
        cmd.args(args);

        let output
            = cmd.output().await.unwrap();

        ScriptResult::new(output, program)
    }

    pub async fn run_binary<I, S>(&mut self, binary: &Binary, args: I) -> ScriptResult where I: IntoIterator<Item = S>, S: AsRef<OsStr> + ToString {
        match binary.kind {
            BinaryKind::Node => {
                let mut node_args = vec![];

                node_args.push(binary.path.to_string());
                node_args.extend(args.into_iter().map(|arg| arg.to_string()));

                self.run_exec("node", node_args).await
            },

            BinaryKind::Default => {
                self.run_exec(&binary.path.to_string(), args).await
            },
        }
    }

    pub async fn run_script<I, S>(&mut self, script: &str, args: I) -> ScriptResult where I: IntoIterator<Item = S>, S: AsRef<OsStr> + ToString {
        let mut bash_args = vec![];

        bash_args.push("-c".to_string());
        bash_args.push(format!("{} \"$@\"", script));
        bash_args.push("yarn-script".to_string());
        bash_args.extend(args.into_iter().map(|arg| arg.to_string()));

        self.run_exec("bash", bash_args).await
    }
}
