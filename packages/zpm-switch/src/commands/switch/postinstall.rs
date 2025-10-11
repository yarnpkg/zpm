use std::str::FromStr;
use std::process::Command;

use clipanion::cli;
use zpm_parsers::{document::Document, JsonDocument};
use zpm_utils::{DataType, FromFileString, Note, IoResultExt, Path, ToFileString, ToHumanString};

use crate::errors::Error;

struct ShellProfile {
    name: String,
    force: bool,

    rc_file: Path,
    rc_line: String,

    env_file: Path,
    env_line: String,
}

#[cli::command]
#[cli::path("switch", "postinstall")]
#[derive(Debug)]
pub struct PostinstallCommand {
    #[cli::option("-H,--home-dir")]
    home_dir: Option<Path>,
}

impl PostinstallCommand {
    pub async fn execute(&self) {
        let bin_dir = Path::current_exe()
            .ok()
            .and_then(|p| p.dirname());

        let Some(bin_dir) = bin_dir else {
            return;
        };

        println!(
            "Yarn Switch {} was successfully installed into {}",
            DataType::Code.colorize(self.cli_environment.info.version.as_str()),
            DataType::Path.colorize(bin_dir.as_str())
        );

        let Some(home) = self.home_dir.clone().or_else(|| Path::home_dir().unwrap_or_default()) else {
            return;
        };

        self.update_shell_profiles(&home, &bin_dir);
        self.install_github_path(&bin_dir);
        self.check_volta_interference(&bin_dir);
    }

    fn check_has_binary(&self, binary: &str) -> bool {
        Command::new(binary)
            .arg("--version")
            .output()
            .map_or(false, |output| output.status.success())
    }

    fn update_shell_profiles(&self, home: &Path, bin_dir: &Path) {
        let profiles = vec![
            ShellProfile {
                name: "Bash".to_string(),
                force: self.check_has_binary("bash"),
                rc_file: home.with_join_str(".bashrc"),
                rc_line: format!("source \"{}\"\n", home.with_join_str(".yarn/switch/env").to_file_string()),
                env_file: home.with_join_str(".yarn/switch/env"),
                env_line: format!("export PATH=\"{}:$PATH\"\n", bin_dir.to_file_string()),
            },
            ShellProfile {
                name: "Zsh".to_string(),
                force: self.check_has_binary("zsh"),
                rc_file: home.with_join_str(".zshrc"),
                rc_line: format!("source \"{}\"\n", home.with_join_str(".yarn/switch/env").to_file_string()),
                env_file: home.with_join_str(".yarn/switch/env"),
                env_line: format!("export PATH=\"{}:$PATH\"\n", bin_dir.to_file_string()),
            },
            ShellProfile {
                name: "Fish".to_string(),
                force: self.check_has_binary("fish"),
                rc_file: home.with_join_str(".config/fish/config.fish"),
                rc_line: format!("source \"{}\"\n", home.with_join_str(".yarn/switch/env.fish").to_file_string()),
                env_file: home.with_join_str(".yarn/switch/env.fish"),
                env_line: format!("set -x PATH \"{}:$PATH\"", bin_dir.to_file_string()),
            },
        ];

        for profile in profiles {
            self.write_profile(&bin_dir, &profile);
        }
    }

    fn write_profile(&self, bin_dir: &Path, profile: &ShellProfile) {
        let profile_content = profile.rc_file
            .fs_read_text_prealloc()
            .ok_missing();

        if let Ok(maybe_profile_content) = profile_content {
            if maybe_profile_content.is_none() && !profile.force {
                return;
            }

            let profile_content
                = maybe_profile_content.unwrap_or_default();

            if self.write_env(&profile.env_file, &profile.env_line).is_err() {
                Note::Warning(format!("
                    We failed to update the environment file referencing the Yarn Switch binary.
                    You may need to manually add the following folder to your PATH:
                    {}
                ", bin_dir.to_print_string())).print();

                return;
            }

            if self.write_rc(&profile.rc_file, profile_content, &profile.rc_line).is_ok() {
                return;
            }
        }

        Note::Warning(format!("
            We failed to update the profile file to load the Yarn Switch environment.
            You may need to manually add the following line to your {} profile:
            {}
        ", profile.name, DataType::Code.colorize(&profile.rc_line))).print();
    }

    fn write_env(&self, env_file: &Path, env_line: &str) -> Result<(), Error> {
        env_file
            .fs_create_parent()?
            .fs_write_text(env_line)?;

        Ok(())
    }

    fn write_rc(&self, rc_file: &Path, mut rc_content: String, rc_line: &str) -> Result<(), Error> {
        if rc_content.contains(&rc_line) {
            return Ok(());
        }

        if !rc_content.is_empty() && !rc_content.ends_with('\n') {
            rc_content.push('\n');
        }

        if !rc_content.is_empty() {
            rc_content.push('\n');
        }

        rc_content
            .push_str("# Added by Yarn Switch\n");
        rc_content
            .push_str(&rc_line);

        rc_file
            .fs_create_parent()?
            .fs_write_text(&rc_content)?;

        Ok(())
    }

    fn install_github_path(&self, bin_dir: &Path) -> bool {
        let Ok(github_path) = std::env::var("GITHUB_PATH") else {
            return false;
        };

        let github_path_file
            = Path::from_str(&github_path).unwrap();

        let github_path_file_write_result = github_path_file
            .fs_append_text(format!("{}\n", bin_dir.to_file_string()));

        if github_path_file_write_result.is_err() {
            Note::Warning(format!("
                We failed to add the bin directory into your GITHUB_PATH.
                You will need to manually add a similar command to your workflow:
                {}
            ", DataType::Code.colorize(&format!("echo \"{}\" >> $GITHUB_PATH", bin_dir.to_home_string())))).print();

            // Even if we failed to write the bin directory into the GITHUB_PATH file,
            // we still return true since we detected that the user is running this
            // command from within a GitHub Action and it wouldn't make sense to try
            // and add the bin directory into the shell profiles.
            return true;
        }

        Note::Info(format!("
            You seem to be running this command from within a GitHub Action.
            We automatically added the bin directory to your GITHUB_PATH file.
        ")).print();

        return true;
    }

    fn check_volta_interference(&self, bin_dir: &Path) {
        let path
            = format!("{}:{}", bin_dir.to_file_string(), std::env::var("PATH").unwrap_or_default());

        let output = Command::new("node")
            .env("PATH", path)
            .arg("-p")
            .arg("process.env.PATH")
            .output();

        let Ok(output) = output else {
            return;
        };

        if !output.status.success() {
            return;
        }

        let Ok(path_output) = String::from_utf8(output.stdout) else {
            return;
        };

        let volta_yarn_path = path_output
            .split(':')
            .find(|entry| entry.contains("/tools/image/yarn/"));

        if let Some(volta_yarn_path) = volta_yarn_path {
            Note::Warning(format!("
                Volta appears to be injecting paths that shadow our own shims in Node.js subprocesses.
                We're going to remove the yarn field from Volta's platform.json file to workaround this issue.
                See {url} for more information.
            ", url = DataType::Url.colorize("https://github.com/volta-cli/volta/issues/2053"))).print();

            if let Err(err) = self.apply_volta_workaround(volta_yarn_path) {
                println!("          Failed to apply workaround: {err}");
            }
        }
    }

    fn apply_volta_workaround(&self, volta_yarn_path: &str) -> Result<(), Error> {
        let volta_yarn_path
            = Path::from_file_string(volta_yarn_path)?;

        let volta_platform_path = volta_yarn_path
            .with_join_str("../../../../user/platform.json");

        let mut volta_platform_content = volta_platform_path
            .fs_read_prealloc()?;

        if volta_platform_content.is_empty() {
            volta_platform_content = "{}".as_bytes().to_vec();
        }

        let mut document
            = JsonDocument::new(volta_platform_content)?;

        document.set_path(
            &vec!["yarn".to_string()].into(),
            zpm_parsers::Value::Undefined,
        )?;

        volta_platform_path
            .fs_write(&document.input)?;

        Ok(())
    }
}
