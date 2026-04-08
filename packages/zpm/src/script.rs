use std::{collections::BTreeMap, ffi::OsStr, fs::Permissions, io::Read, os::unix::{fs::PermissionsExt, process::ExitStatusExt}, process::{ExitStatus, Output}, sync::{Arc, LazyLock}};

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

fn make_python_entry_point_snippet(binary_name: &str, package_path: &Path, module: &str, object: &str) -> String {
    let binary_name
        = serde_json::to_string(binary_name).expect("expected valid binary name");
    let package_path
        = serde_json::to_string(&package_path.to_file_string()).expect("expected valid package path");
    let module
        = serde_json::to_string(module).expect("expected valid python module");
    let object
        = serde_json::to_string(object).expect("expected valid python object");

    format!(
        "import importlib, sys\nsys.path.insert(0, {package_path})\nmodule = importlib.import_module({module})\nentry = module\nfor part in {object}.split('.'):\n    entry = getattr(entry, part)\nsys.argv[0] = {binary_name}\nsys.exit(entry())"
    )
}

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
    if let Some(ext) = p.extname() {
        return JS_EXTENSION.is_match(ext);
    }

    let Ok(file) = std::fs::File::open(p.to_path_buf()) else {
        return true;
    };

    let mut buf
        = Vec::with_capacity(64);

    let read_result
        = file.take(buf.capacity() as u64)
            .read_to_end(&mut buf);

    if read_result.is_err() {
        return true;
    }

    let magic_bytes = buf.get(0..4)
        .and_then(|data| data.try_into().ok())
        .map(|bytes| u32::from_be_bytes(bytes));

    if let Some(magic) = magic_bytes {
        match magic {
            // OSX Universal Binary
            // Mach-O
            // ELF
            0xcafebabe | 0xcffaedfe | 0x7f454c46
                => return false,

            // DOS MZ Executable
            n if (n & 0xffff0000) == 0x4d5a0000
                => return false,

            _ => (),
        }
    }

    if buf.starts_with(b"#!") {
        let shebang_line
            = buf.iter()
                .position(|&b| b == b'\n')
                .map(|pos| &buf[..pos]);

        if let Some(line) = shebang_line {
            return line.ends_with(b"node");
        }
    }

    true
}

fn get_self_path() -> Result<Path, Error> {
    let self_path = std::env::var("YARNSW_EXEC_PATH")
        .ok()
        .map(|path| Path::from_file_string(&path))
        .unwrap_or_else(|| Path::current_exe())?;

    Ok(self_path)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryKind {
    Default,
    Node,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Binary {
    Path {
        path: Path,
        kind: BinaryKind,
    },
    PythonEntryPoint {
        name: String,
        package_path: Path,
        module: String,
        object: String,
    },
}

impl Binary {
    pub fn new_path(project: &Project, path_rel: Path) -> Self {
        let path_abs = project.project_cwd
            .with_join(&path_rel);

        let kind = match is_node_script(path_abs.clone()) {
            true => BinaryKind::Node,
            false => BinaryKind::Default,
        };

        Self::Path {
            path: path_abs,
            kind,
        }
    }

    pub fn new_python(name: String, package_path: Path, module: String, object: String) -> Self {
        Self::PythonEntryPoint {
            name,
            package_path,
            module,
            object,
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
            match binary {
                Binary::Path {path, kind} => {
                    let binary_path_abs = relative_to
                        .with_join(path);

                    if *kind == BinaryKind::Node {
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
                },

                Binary::PythonEntryPoint {name, package_path, module, object} => {
                    self.binaries.push(ScriptBinary {
                        name: name.clone(),
                        argv0: "python".to_string(),
                        args: vec![
                            "-c".to_string(),
                            make_python_entry_point_snippet(name, package_path, module, object),
                        ],
                    });
                },
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

/// Target output mode for script execution.
#[derive(Clone, Debug, Default)]
pub enum TargetOutput {
    /// Inherit stdout/stderr from parent process (direct to terminal).
    Inherit,
    /// Buffer all stdout/stderr and return in ScriptResult (default).
    #[default]
    Buffers,
}

/// A running script process with piped output.
pub struct RunningScript {
    pub child: tokio::process::Child,
    program: String,
}

impl RunningScript {
    /// Wait for the script to complete and return the result.
    pub async fn wait(mut self) -> Result<ScriptResult, Error> {
        let status = self.child.wait().await?;
        let output = Output {
            status,
            stdout: Vec::new(),
            stderr: Vec::new(),
        };
        Ok(if status.success() {
            ScriptResult::Success(output)
        } else {
            ScriptResult::Failure(output, self.program.clone(), self.program)
        })
    }
}

pub struct ScriptEnvironment {
    cwd: Path,
    binaries: ScriptBinaries,
    env: BTreeMap<String, Option<String>>,
    node_args: Vec<String>,
    target_output: TargetOutput,
    stdin: Option<String>,
    signal_delegation: bool,
}

impl ScriptEnvironment {
    pub fn new() -> Result<Self, Error> {
        let mut value = Self {
            cwd: Path::current_dir().unwrap(),
            binaries: ScriptBinaries::new().with_standard()?,
            env: BTreeMap::new(),
            node_args: Vec::new(),
            target_output: TargetOutput::default(),
            stdin: None,
            signal_delegation: false,
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
        self.target_output = TargetOutput::Inherit;
        self
    }

    pub fn with_stdin(mut self, stdin: Option<String>) -> Self {
        self.stdin = stdin;
        self
    }

    /// Enables signal delegation mode.
    ///
    /// When enabled, SIGINT is ignored in the parent process while waiting
    /// for child processes to complete. This allows the child to handle
    /// the signal and exit gracefully, with its exit code properly propagated.
    ///
    /// This is useful when the parent is a wrapper (like yarn-switch) that
    /// should delegate signal handling to the actual command being run.
    pub fn enable_signal_delegation(mut self) -> Self {
        self.signal_delegation = true;
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

    /// Prepares a command with the current environment settings.
    fn prepare_command(&mut self, program: &str, args: &[String]) -> Result<(Command, Path), Error> {
        let mut cmd = Command::new(program);

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

        let bin_dir = self.install_binaries()?;

        let env_path = self.env.get("PATH")
            .cloned()
            .unwrap_or_else(|| std::env::var("PATH").ok())
            .unwrap_or_default();

        let next_env_path = match env_path.is_empty() {
            true => bin_dir.to_file_string(),
            false => format!("{}:{}", bin_dir.to_file_string(), env_path),
        };

        cmd.env("PATH", next_env_path);
        cmd.env("BERRY_BIN_FOLDER", bin_dir.to_file_string());
        cmd.args(args);

        if self.stdin.is_some() {
            cmd.stdin(std::process::Stdio::piped());
        }

        Ok((cmd, bin_dir))
    }

    pub async fn run_exec<I, S>(&mut self, program: &str, args: I) -> Result<ScriptResult, Error> where I: IntoIterator<Item = S>, S: AsRef<str> {
        let args = args.into_iter()
            .map(|arg| arg.as_ref().to_string())
            .collect::<Vec<_>>();

        let (mut cmd, _) = self.prepare_command(program, &args)?;

        // Configure stdout/stderr based on target_output
        match &self.target_output {
            TargetOutput::Inherit => {
                // Output goes directly to terminal
            },
            TargetOutput::Buffers => {
                cmd.stdout(std::process::Stdio::piped());
                cmd.stderr(std::process::Stdio::piped());
            },
        }

        let mut child = cmd.spawn()
            .map_err(|e| Error::SpawnFailed { name: program.to_string(), path: self.cwd.clone(), error: Arc::new(Box::new(e)) })?;

        if let Some(stdin) = &self.stdin {
            if let Some(mut child_stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                child_stdin.write_all(stdin.as_bytes()).await.unwrap();
            }
        }

        // If signal delegation is enabled, ignore SIGINT while waiting for the child.
        // This allows the child to handle the signal and exit gracefully.
        #[cfg(unix)]
        let _guard = if self.signal_delegation {
            Some(zpm_utils::IgnoreSigint::new())
        } else {
            None
        };

        let output = match &self.target_output {
            TargetOutput::Inherit => {
                Output {
                    status: child.wait().await.unwrap(),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                }
            },
            TargetOutput::Buffers => {
                child.wait_with_output().await.unwrap()
            },
        };

        Ok(ScriptResult::new(output, cmd.as_std()))
    }

    /// Spawns a command and returns the running process with piped stdout/stderr.
    /// Use this when you need to read output incrementally (e.g., for interlaced task output).
    pub async fn spawn_exec<I, S>(&mut self, program: &str, args: I) -> Result<RunningScript, Error> where I: IntoIterator<Item = S>, S: AsRef<str> {
        let args = args.into_iter()
            .map(|arg| arg.as_ref().to_string())
            .collect::<Vec<_>>();

        let (mut cmd, _) = self.prepare_command(program, &args)?;

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        // Put the child in its own process group so we can kill the entire group if needed
        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = cmd.spawn()
            .map_err(|e| Error::SpawnFailed { name: program.to_string(), path: self.cwd.clone(), error: Arc::new(Box::new(e)) })?;

        if let Some(stdin) = &self.stdin {
            if let Some(mut child_stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                child_stdin.write_all(stdin.as_bytes()).await.unwrap();
            }
        }

        Ok(RunningScript {
            child,
            program: program.to_string(),
        })
    }

    pub async fn run_binary<I, S>(&mut self, binary: &Binary, args: I) -> Result<ScriptResult, Error> where I: IntoIterator<Item = S>, S: AsRef<str> {
        match binary {
            Binary::Path {path, kind: BinaryKind::Node} => {
                let mut node_args = self.node_args.clone();

                node_args.push(path.to_file_string());
                node_args.extend(args.into_iter().map(|arg| arg.as_ref().to_string()));

                self.run_exec("node", node_args).await
            },

            Binary::Path {path, kind: BinaryKind::Default} => {
                self.run_exec(&path.to_file_string(), args).await
            },

            Binary::PythonEntryPoint {name, package_path, module, object} => {
                let mut python_args = vec![
                    "-c".to_string(),
                    make_python_entry_point_snippet(name, package_path, module, object),
                ];

                python_args.extend(args.into_iter().map(|arg| arg.as_ref().to_string()));

                match self.run_exec("python", &python_args).await {
                    Ok(result) => Ok(result),
                    Err(Error::SpawnFailed {name, ..}) if name == "python" => self.run_exec("python3", &python_args).await,
                    Err(err) => Err(err),
                }
            },
        }
    }

    pub async fn run_script<I, S>(&mut self, script: &str, args: I) -> Result<ScriptResult, Error> where I: IntoIterator<Item = S>, S: AsRef<OsStr> + ToString {
        let mut final_script = script.to_string();

        for arg in args {
            final_script.push(' ');
            final_script.push_str(&shell_escape(arg.to_string().as_str()));
        }

        self.run_exec("bash", ["-c", &final_script, "yarn-script"]).await
    }

    /// Spawns a script and returns the running process with piped stdout/stderr.
    /// Use this when you need to read output incrementally (e.g., for interlaced task output).
    pub async fn spawn_script<I, S>(&mut self, script: &str, args: I) -> Result<RunningScript, Error> where I: IntoIterator<Item = S>, S: AsRef<OsStr> + ToString {
        // Pass args as bash positional parameters ($1, $2, etc.)
        // Format: bash -c "script" yarn-script arg1 arg2 ...
        let mut bash_args = vec!["-c".to_string(), script.to_string(), "yarn-script".to_string()];
        for arg in args {
            bash_args.push(arg.to_string());
        }

        self.spawn_exec("bash", bash_args.iter().map(|s| s.as_str())).await
    }

    /// Runs a script with inherited stdio (output goes directly to terminal).
    /// Use this when you want the script's output to go directly to the terminal without capturing.
    pub async fn run_script_inherited<I, S>(&mut self, script: &str, args: I) -> Result<ExitStatus, Error> where I: IntoIterator<Item = S>, S: AsRef<OsStr> + ToString {
        let mut final_script = script.to_string();

        for arg in args {
            final_script.push(' ');
            final_script.push_str(&shell_escape(arg.to_string().as_str()));
        }

        let args = ["-c", &final_script, "yarn-script"];
        let (mut cmd, _) = self.prepare_command("bash", &args.iter().map(|s| s.to_string()).collect::<Vec<_>>())?;

        cmd.stdout(std::process::Stdio::inherit());
        cmd.stderr(std::process::Stdio::inherit());
        cmd.stdin(std::process::Stdio::inherit());

        // If signal delegation is enabled, ignore SIGINT while waiting for the child.
        // This allows the child to handle the signal and exit gracefully.
        #[cfg(unix)]
        let _guard = if self.signal_delegation {
            Some(zpm_utils::IgnoreSigint::new())
        } else {
            None
        };

        let status = cmd.status().await
            .map_err(|e| Error::SpawnFailed { name: "bash".to_string(), path: self.cwd.clone(), error: Arc::new(Box::new(e)) })?;

        Ok(status)
    }
}
