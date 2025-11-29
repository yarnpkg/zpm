use std::{collections::BTreeMap, ffi::OsStr, fs::Permissions, io::Read, os::unix::{fs::PermissionsExt, process::ExitStatusExt}, process::{ExitStatus, Output}, sync::{Arc, LazyLock}};

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use zpm_parsers::JsonDocument;
use zpm_primitives::Locator;
use zpm_utils::{shell_escape, to_shell_line, FromFileString, Hash64, Path, ToFileString};
use itertools::Itertools;
use regex::Regex;
use tokio::process::Command;

use crate::{
    error::Error,
    project::Project,
};

static CJS_LOADER_MATCHER: LazyLock<Regex> = LazyLock::new(|| regex::Regex::new(r"\s*--require\s+\S*\.pnp\.c?js\s*").unwrap());
static ESM_LOADER_MATCHER: LazyLock<Regex> = LazyLock::new(|| regex::Regex::new(r"\s*--experimental-loader\s+\S*\.pnp\.loader\.mjs\s*").unwrap());
static JS_EXTENSION: LazyLock<Regex> = LazyLock::new(|| regex::Regex::new(r"\.[cm]?[jt]sx?$").unwrap());

fn make_path_wrapper(bin_dir: &Path, name: &str, argv0: &str, args: &Vec<String>) -> Result<(), Error> {
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

fn get_self_path() -> Result<Path, Error> {
    let self_path = std::env::var("YARNSW_EXEC_PATH")
        .ok()
        .map(|path| Path::from_file_string(&path))
        .unwrap_or_else(|| Path::current_exe())?;

    Ok(self_path)
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum BinaryKind {
    Default,
    Node,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
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

#[derive(Serialize)]
pub struct ScriptBinary {
    pub name: String,
    pub argv0: String,
    pub args: Vec<String>,
}

#[derive(Default, Serialize)]
pub struct ScriptBinaries {
    pub binaries: Vec<ScriptBinary>,
}

impl ScriptBinaries {
    pub fn new() -> Self {
        Self {
            binaries: Vec::new(),
        }
    }

    pub fn with_standard(mut self) -> Result<Self, Error> {
        let self_path = get_self_path()?
            .to_file_string();

        self.binaries.push(ScriptBinary {
            name: "run".to_string(),
            argv0: self_path.clone(),
            args: vec!["run".to_string()],
        });

        self.binaries.push(ScriptBinary {
            name: "yarn".to_string(),
            argv0: self_path.clone(),
            args: vec![],
        });

        self.binaries.push(ScriptBinary {
            name: "yarnpkg".to_string(),
            argv0: self_path.clone(),
            args: vec![],
        });

        self.binaries.push(ScriptBinary {
            name: "node-gyp".to_string(),
            argv0: self_path.clone(),
            args: vec!["run".to_string(), "--top-level".to_string(), "node-gyp".to_string()],
        });

        Ok(self)
    }

    pub fn with_package(mut self, binaries: &BTreeMap<String, Binary>, relative_to: &Path) -> Result<Self, Error> {
        for (name, binary) in binaries {
            let binary_path_abs = relative_to
                .with_join(&binary.path);

            if binary.kind == BinaryKind::Node {
                self.binaries.push(ScriptBinary {
                    name: name.clone(),
                    argv0: "node".to_string(),
                    args: vec![binary_path_abs.to_file_string()],
                });
            } else {
                self.binaries.push(ScriptBinary {
                    name: name.clone(),
                    argv0: binary_path_abs.to_file_string(),
                    args: vec![],
                });
            }
        }

        Ok(self)
    }
}

#[derive(Debug)]
pub enum ScriptResult {
    Success(Output),
    Failure(Output, String, String),
}

impl ScriptResult {
    pub fn new_success() -> Self {
        Self::Success(Output {
            status: ExitStatus::from_raw(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
        })
    }

    pub fn new(output: Output, cmd: &std::process::Command) -> Self {
        if output.status.success() {
            Self::Success(output)
        } else {
            Self::Failure(output, cmd.get_program().to_str().unwrap().to_string(), to_shell_line(cmd).unwrap())
        }
    }

    pub fn success(&self) -> bool {
        matches!(self, Self::Success(_))
    }

    pub fn ok(self) -> Result<Self, Error> {
        match self {
            Self::Success(_) => {
                Ok(self)
            },

            Self::Failure(output, program, shell_line) => {
                if let Ok(temp_dir) = Path::temp_dir() {
                    let log_path = temp_dir
                        .with_join_str("error.log");

                    let stdout
                        = String::from_utf8_lossy(&output.stdout);
                    let stderr
                        = String::from_utf8_lossy(&output.stderr);

                    // open a fd and write stdout/err into it
                    let log_write = log_path
                        .fs_write_text(format!("=== COMMAND ===\n\n{}\n\n=== STDOUT ===\n\n{}\n=== STDERR ===\n\n{}", shell_line, stdout, stderr));

                    if log_write.is_ok() {
                        Err(Error::ChildProcessFailedWithLog(program, log_path))
                    } else {
                        Err(Error::ChildProcessFailed(program))
                    }
                } else {
                    Err(Error::ChildProcessFailed(program))
                }
            },
        }
    }

    pub fn output(self) -> Output {
        match self {
            Self::Success(output) => output,
            Self::Failure(output, _, _) => output,
        }
    }

    pub fn stdout_text(self) -> Result<String, Error> {
        let output
            = self.output();

        let text
            = String::from_utf8(output.stdout)?
                .trim()
                .to_string();

        Ok(text)
    }
}

impl From<ScriptResult> for ExitStatus {
    fn from(val: ScriptResult) -> Self {
        match val {
            ScriptResult::Success(output) => output.status,
            ScriptResult::Failure(output, _, _) => output.status,
        }
    }
}

pub struct ScriptEnvironment {
    cwd: Path,
    binaries: ScriptBinaries,
    env: BTreeMap<String, Option<String>>,
    node_args: Vec<String>,
    shell_forwarding: bool,
    stdin: Option<String>,
    enable_sandbox: bool,
    project_cwd: Option<Path>,
    global_folder: Option<Path>,
}

impl ScriptEnvironment {
    pub fn new() -> Result<Self, Error> {
        let mut value = Self {
            cwd: Path::current_dir().unwrap(),
            binaries: ScriptBinaries::new().with_standard()?,
            env: BTreeMap::new(),
            node_args: Vec::new(),
            shell_forwarding: false,
            stdin: None,
            enable_sandbox: false,
            project_cwd: None,
            global_folder: None,
        };

        if let Ok(val) = std::env::var("YARNSW_DETECTED_ROOT") {
            value.env.insert("YARNSW_DETECTED_ROOT".to_string(), Some(val));
        }

        let self_path
            = get_self_path()?;

        value.env.insert("npm_execpath".to_string(), Some(self_path.to_file_string()));
        value.env.insert("npm_config_user_agent".to_string(), Some(format!("yarn/{}", zpm_switch::get_bin_version())));

        Ok(value)
    }

    // fn prepend_env(&mut self, key: &str, separator: char, value: &str) {
    //     let current = self.env.entry(key.to_string())
    //         .or_insert(std::env::var(key).unwrap_or_default());

    //     if !current.is_empty() {
    //         current.insert(0, separator);
    //     }

    //     current.insert_str(0, value);
    // }

    fn append_env(&mut self, key: &str, separator: char, value: &str) {
        let current = self.env.entry(key.to_string())
            .or_insert_with(|| std::env::var(key).ok());

        match current {
            Some(existing) => {
                if !existing.is_empty() {
                    existing.push(separator);
                }
                existing.push_str(value);
            },

            None => {
                *current = Some(value.to_string());
            },
        }
    }

    pub fn with_env(mut self, env: BTreeMap<String, String>) -> Self {
        for (key, value) in env {
            self.env.insert(key, Some(value));
        }
        self
    }

    pub fn with_env_variable(mut self, key: &str, value: &str) -> Self {
        self.env.insert(key.to_string(), Some(value.to_string()));
        self
    }

    pub fn delete_env_variable(mut self, key: &str) -> Self {
        self.env.insert(key.to_string(), None);
        self
    }

    pub fn with_node_args(mut self, args: Vec<String>) -> Self {
        self.node_args = args;
        self
    }

    pub fn enable_shell_forwarding(mut self) -> Self {
        self.shell_forwarding = true;
        self
    }

    pub fn with_stdin(mut self, stdin: Option<String>) -> Self {
        self.stdin = stdin;
        self
    }

    pub fn with_project(mut self, project: &Project) -> Self {
        self.remove_pnp_loader();

        if let Some(pnp_path) = project.pnp_path().if_exists() {
            self.append_env("NODE_OPTIONS", ' ', &format!("--require {}", pnp_path.to_file_string()));
        }

        if let Some(pnp_loader_path) = project.pnp_loader_path().if_exists() {
            self.append_env("NODE_OPTIONS", ' ', &format!("--experimental-loader {}", pnp_loader_path.to_file_string()));
        }

        self.env.insert("PROJECT_CWD".to_string(), Some(project.project_cwd.to_file_string()));
        self.env.insert("INIT_CWD".to_string(), Some(project.project_cwd.with_join(&project.shell_cwd).to_file_string()));
        self.env.insert("CACHE_CWD".to_string(), Some(project.preferred_cache_path().to_file_string()));

        self.enable_sandbox = project.config.settings.enable_sandbox.value;
        self.project_cwd = Some(project.project_cwd.clone());
        self.global_folder = Some(project.config.settings.global_folder.value.clone());

        self
    }

    fn remove_pnp_loader(&mut self) {
        let current = self.env.get("NODE_OPTIONS")
            .and_then(|opt| opt.clone())
            .or_else(|| std::env::var("NODE_OPTIONS").ok());

        let Some(current) = current else {
            return;
        };

        let updated = CJS_LOADER_MATCHER.replace_all(&current, " ");
        let updated = ESM_LOADER_MATCHER.replace_all(&updated, " ");
        let updated = updated.trim();

        if current != updated {
            // When set to an empty string, some tools consider it as explicitly
            // set to the empty value, and do not set their own value.
            if updated.is_empty() {
                self.env.insert("NODE_OPTIONS".to_string(), None);
            } else {
                self.env.insert("NODE_OPTIONS".to_string(), Some(updated.to_string()));
            }
        }
    }

    pub fn with_standard_binaries(mut self) -> Self {
        self.binaries = ScriptBinaries::new().with_standard().unwrap();
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

        self.env.insert("npm_package_name".to_string(), Some(locator.ident.to_file_string()));
        self.env.insert("npm_package_version".to_string(), Some(resolution.version.to_file_string()));
        self.env.insert("npm_package_json".to_string(), Some(manifest_location_abs.to_file_string()));

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

        let binaries
            = project.package_visible_binaries(locator)?;

        self.binaries = self.binaries
            .with_package(&binaries, &project.project_cwd)?;

        Ok(self)
    }

    pub fn with_cwd(mut self, cwd: Path) -> Self {
        self.cwd = cwd;
        self
    }

    fn install_binaries(&mut self) -> Result<Path, Error> {
        let hash
            = Hash64::from_string(&JsonDocument::to_string(&self.binaries)?);
        let dir_name
            = format!(".yarn/zpm/binaries/zpm-{}", hash.to_file_string());

        let dir = Path::home_dir()?
            .expect("Expected home directory")
            .with_join_str(&dir_name);

        if !dir.fs_exists() {
            let temp_dir
                = Path::temp_dir()?;

            temp_dir
                .fs_create_dir_all()?;

            for binary in &self.binaries.binaries {
                make_path_wrapper(&temp_dir, &binary.name, &binary.argv0, &binary.args)?;
            }

            dir
                .fs_create_parent()?;

            temp_dir
                .fs_concurrent_move(&dir)?;
        }

        Ok(dir)
    }

    /// Generates a sandbox profile for macOS seatbelt.
    /// The profile is restrictive by default:
    /// - Project folder (project_cwd) is allowed read-write
    /// - Yarn global folder is allowed read-only
    /// - All other file operations are denied by default
    #[cfg(target_os = "macos")]
    fn generate_sandbox_profile(&self) -> String {
        // Get project_cwd for read-write access
        let project_cwd = self.project_cwd
            .as_ref()
            .map(|p| p.to_file_string())
            .unwrap_or_else(|| self.cwd.to_file_string());

        // Get global_folder for read-only access
        let global_folder = self.global_folder
            .as_ref()
            .map(|p| p.to_file_string());

        let mut profile = String::from(r#"(version 1)
(deny default)
(allow process-fork)
(allow process-exec)
(allow sysctl-read)
(allow mach-lookup)
(allow signal)
(allow ipc-posix*)
"#);

        // Allow read-write access to project folder
        profile.push_str(&format!(r#"
; Allow read-write access to project folder
(allow file-read* (subpath "{}"))
(allow file-write* (subpath "{}"))
"#, project_cwd, project_cwd));

        // Allow read-only access to Yarn global folder
        if let Some(ref global) = global_folder {
            profile.push_str(&format!(r#"
; Allow read-only access to Yarn global folder
(allow file-read* (subpath "{}"))
"#, global));
        }

        profile
    }

    pub async fn run_exec<I, S>(&mut self, program: &str, args: I) -> Result<ScriptResult, Error> where I: IntoIterator<Item = S>, S: AsRef<str> {
        let args = args.into_iter()
            .map(|arg| arg.as_ref().to_string())
            .collect::<Vec<_>>();

        let bin_dir
            = self.install_binaries()?;

        // On macOS, wrap with sandbox-exec if sandbox is enabled
        #[cfg(target_os = "macos")]
        let (actual_program, actual_args) = if self.enable_sandbox {
            let profile = self.generate_sandbox_profile();
            let mut sandbox_args = vec![
                "-p".to_string(),
                profile,
                program.to_string(),
            ];
            sandbox_args.extend(args);
            ("sandbox-exec".to_string(), sandbox_args)
        } else {
            (program.to_string(), args)
        };

        #[cfg(not(target_os = "macos"))]
        let (actual_program, actual_args) = (program.to_string(), args);

        let mut cmd
            = Command::new(&actual_program);

        cmd.current_dir(self.cwd.to_path_buf());

        for (key, value) in &self.env {
            match value {
                Some(val) => {
                    cmd.env(key, val);
                },

                None => {
                    cmd.env_remove(key);
                },
            };
        }

        let env_path = self.env.get("PATH")
            .cloned()
            .unwrap_or_else(|| std::env::var("PATH").ok())
            .unwrap_or_default();

        let next_env_path = match env_path.is_empty() {
            true => {
                bin_dir.to_file_string()
            },

            false => {
                format!("{}:{}", bin_dir.to_file_string(), env_path)
            },
        };

        cmd.env("PATH", next_env_path);
        cmd.env("BERRY_BIN_FOLDER", bin_dir.to_file_string());

        cmd.args(&actual_args);

        if self.stdin.is_some() {
            cmd.stdin(std::process::Stdio::piped());
        }

        if !self.shell_forwarding {
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());
        }

        let mut child
            = cmd.spawn()
                .map_err(|e| Error::SpawnFailed(actual_program.clone(), self.cwd.clone(), Arc::new(Box::new(e))))?;

        if let Some(stdin) = &self.stdin {
            if let Some(mut child_stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                child_stdin.write_all(stdin.as_bytes()).await.unwrap();
            }
        }

        let output = match self.shell_forwarding {
            false => {
                child.wait_with_output().await.unwrap()
            },

            true => {
                Output {
                    status: child.wait().await.unwrap(),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                }
            },
        };

        Ok(ScriptResult::new(output, cmd.as_std()))
    }

    pub async fn run_binary<I, S>(&mut self, binary: &Binary, args: I) -> Result<ScriptResult, Error> where I: IntoIterator<Item = S>, S: AsRef<str> {
        match binary.kind {
            BinaryKind::Node => {
                let mut node_args = self.node_args.clone();

                node_args.push(binary.path.to_file_string());
                node_args.extend(args.into_iter().map(|arg| arg.as_ref().to_string()));

                self.run_exec("node", node_args).await
            },

            BinaryKind::Default => {
                self.run_exec(&binary.path.to_file_string(), args).await
            },
        }
    }

    pub async fn run_script<I, S>(&mut self, script: &str, args: I) -> Result<ScriptResult, Error> where I: IntoIterator<Item = S>, S: AsRef<OsStr> + ToString {
        let mut final_script
            = script.to_string();

        for arg in args {
            final_script.push(' ');
            final_script.push_str(&shell_escape(arg.to_string().as_str()));
        }

        let mut bash_args = vec![];

        bash_args.push("-c".to_string());
        bash_args.push(final_script);
        bash_args.push("yarn-script".to_string());

        self.run_exec("bash", bash_args).await
    }
}
