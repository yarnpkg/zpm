use clipanion::cli;
use zpm_utils::{DataType, OkMissing, Path, ToFileString};

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

        let profile_path = home
            .with_join_str(".profile");

        let profile_content = profile_path
            .fs_read_text_prealloc()
            .ok_missing();

        let Ok(profile_content) = profile_content else {
            return;
        };

        let path_line
            = format!("export PATH=\"{}:$PATH\"\n", bin_dir.to_file_string());

        let env_path = home
            .with_join_str(".yarn/switch/env");

        let env_write_result = env_path
            .fs_create_parent()
            .and_then(|_| env_path.fs_write_text(&path_line));

        if env_write_result.is_err() {
            println!(
                "Failed to write {}; manually append the following line to your shell configuration file:\n{}",
                env_path,
                DataType::Code.colorize(&path_line)
            );

            return;
        }

        let mut profile_content
            = profile_content.unwrap_or_default();

        let profile_line
            = format!(". \"{}\"\n", env_path.to_file_string());

        if profile_content.contains(&profile_line) {
            return;
        }

        if !profile_content.is_empty() && !profile_content.ends_with('\n') {
            profile_content.push('\n');
        }

        profile_content
            .push_str(&profile_line);

        let profile_write_result = profile_path
            .fs_create_parent()
            .and_then(|_| profile_path.fs_write_text(&profile_content));

        if profile_write_result.is_ok() {
            println!(
                "We updated {} for you; please restart your shell or run {} to apply the changes.",
                profile_path,
                DataType::Code.colorize("source ~/.profile")
            );
        } else {
            println!(
                "Failed to write {}; manually append the following line:\n{}",
                profile_path,
                DataType::Code.colorize(&profile_line)
            );
        }
    }
}
