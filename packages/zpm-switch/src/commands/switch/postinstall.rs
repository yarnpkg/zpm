use std::str::FromStr;
use std::process::Command;

use clipanion::cli;
use sonic_rs::JsonValueMutTrait;
use zpm_utils::{DataType, FromFileString, Note, IoResultExt, Path, ToFileString, ToHumanString};

use crate::errors::Error;

pub enum ShellProfile {
    Posix(Path),
    Fish(Path),
}

impl ShellProfile {
    pub fn as_path(&self) -> &Path {
        match self {
            ShellProfile::Posix(path) => path,
            ShellProfile::Fish(path) => path,
        }
    }

    pub fn to_env_path_lines(&self, bin_dir: &Path) -> String {
        match self {
            ShellProfile::Posix(_) => format!("export PATH=\"{}:$PATH\"\n", bin_dir.to_file_string()),
            ShellProfile::Fish(_) => format!("set -x PATH \"{}:$PATH\"", bin_dir.to_file_string()),
        }
    }

    pub fn to_source_line(&self, env_path: &Path) -> String {
        match self {
            ShellProfile::Posix(_) => format!("source \"{}\"", env_path.to_file_string()),
            ShellProfile::Fish(_) => format!("source \"{}\"", env_path.to_file_string()),
        }
    }
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

        let env_path = home
            .with_join_str(".yarn/switch/env");

        let fallback_profile
            = ShellProfile::Posix(home.with_join_str(".bashrc"));

        if !self.check_github_path(&bin_dir) {
            if let Some(profile) = self.get_profile(&home) {
                self.write_env(&env_path, &bin_dir, &profile);
                self.write_profile(&profile, &env_path);
            } else {
                self.write_env(&env_path, &bin_dir, &fallback_profile);
                self.report_profile_write_error(&fallback_profile, &env_path);
            }
        } else {
            self.write_env(&env_path, &bin_dir, &ShellProfile::Posix(home.with_join_str(".bashrc")));
        }

        self.check_volta_interference();
    }

    fn write_profile(&self, profile: &ShellProfile, env_path: &Path) {
        let profile_path = profile
            .as_path();

        let profile_content = profile_path
            .fs_read_text_prealloc()
            .ok_missing();

        let Ok(profile_content) = profile_content else {
            return;
        };

        let mut profile_content
            = profile_content.unwrap_or_default();

        let profile_line
            = format!("{}\n", profile.to_source_line(env_path));

        if profile_content.contains(&profile_line) {
            return;
        }

        if !profile_content.is_empty() && !profile_content.ends_with('\n') {
            profile_content.push('\n');
        }

        if !profile_content.is_empty() {
            profile_content.push('\n');
        }

        profile_content
            .push_str("# Added by Yarn Switch\n");
        profile_content
            .push_str(&profile_line);

        let profile_write_result = profile_path
            .fs_create_parent()
            .and_then(|_| profile_path.fs_write_text(&profile_content));

        if profile_write_result.is_ok() {
            Note::Info(format!("
                We updated your shell configuration file for you.
                Please restart your shell or run {} to apply the changes.
            ", DataType::Code.colorize(&profile.to_source_line(env_path)))).print();
        } else {
            self.report_profile_write_error(profile, env_path);
        }
    }

    fn report_profile_write_error(&self, profile: &ShellProfile, env_path: &Path) {
        Note::Warning(format!("
            We failed to update your shell configuration file.
            You will need to manually append the following line to your shell configuration file (perhaps {}?):
            {}
        ", profile.as_path().to_print_string(), DataType::Code.colorize(&profile.to_source_line(env_path)))).print();
    }

    fn write_env(&self, env_path: &Path, bin_dir: &Path, profile: &ShellProfile) {
        let env_path_lines
            = profile.to_env_path_lines(bin_dir);

        let env_write_result = env_path
            .fs_create_parent()
            .and_then(|_| env_path.fs_write_text(&env_path_lines));

        if env_write_result.is_err() {
            Note::Warning(format!("
                We failed to update the Yarn Switch environment file.
                You will need to manually append the following line to your shell configuration file:
                {}
            ", DataType::Code.colorize(&env_path_lines))).print();

            return;
        }
    }

    fn get_profile(&self, home: &Path) -> Option<ShellProfile> {
        let Ok(shell) = std::env::var("SHELL") else {
            return None;
        };

        let shell_name = shell
            .split('/')
            .last();

        let Some(shell_name) = shell_name else {
            return None;
        };

        match shell_name {
            "bash" => Some(ShellProfile::Posix(home.with_join_str(".bashrc"))),
            "zsh" => Some(ShellProfile::Posix(home.with_join_str(".zshrc"))),
            "fish" => Some(ShellProfile::Fish(home.with_join_str(".config/fish/config.fish"))),
            _ => None,
        }
    }

    fn check_github_path(&self, bin_dir: &Path) -> bool {
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

    fn check_volta_interference(&self) {
        let output = Command::new("node")
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

        let volta_platform_content = volta_platform_path
            .fs_read_prealloc()?;

        let mut volta_platform
            = sonic_rs::from_slice::<sonic_rs::Value>(&volta_platform_content)?;

        volta_platform
            .as_object_mut()
            .ok_or(Error::VoltaPlatformJsonInvalid)?
            .remove(&"yarn");

        let volta_platform_json
            = sonic_rs::to_string_pretty(&volta_platform)?;

        volta_platform_path
            .fs_write_text(&volta_platform_json)?;

        Ok(())
    }
}
