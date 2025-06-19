use std::{fs::Permissions, os::unix::fs::PermissionsExt, str::FromStr};

use clipanion::cli;
use zpm_utils::{DataType, Path};

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

        let Ok(shell) = std::env::var("SHELL") else {
            return;
        };

        let Ok(shell_path) = Path::from_str(&shell) else {
            return;
        };

        let Some(shell_name) = shell_path.basename() else {
            return;
        };

        let Some(home) = self.home_dir.clone().or_else(|| Path::home_dir().unwrap_or_default()) else {
            return;
        };

        match shell_name {
            "bash" => {
                let insert_line
                    = format!("export PATH=\"{}:$PATH\"\n", bin_dir.to_string());

                let bashrc_path = home
                    .with_join_str(".bashrc");

                insert_rc_line(bashrc_path, insert_line);
            },

            "zsh" => {
                let insert_line
                    = format!("export PATH=\"{}:$PATH\"\n", bin_dir.to_string());

                let zshrc_path = home
                    .with_join_str(".zshrc");

                insert_rc_line(zshrc_path, insert_line);
            },

            _ => {
                println!("We couldn't find a supported shell to update ({}). Please manually add the following line to your shell configuration file:", DataType::Code.colorize(&format!("SHELL={}", shell)));
                println!("{}", DataType::Code.colorize(&format!("export PATH=\"{}:$PATH\"", bin_dir)));
            },
        }
    }
}

fn insert_rc_line(rc_path: Path, line: String) {
    let rc_content = rc_path
        .fs_read_text_prealloc();

    let initial_rc_content = match rc_content {
        Ok(content) => {
            content
        },

        Err(e) if e.io_kind() == Some(std::io::ErrorKind::NotFound) => {
            String::new()
        },

        Err(_) => {
            println!("Failed to read rc file");
            return;
        },
    };

    if initial_rc_content.contains(&line) {
        return;
    }

    let mut rc_content
        = initial_rc_content.clone();

    let header
        = "# BEGIN YARN SWITCH MANAGED BLOCK\n";
    let footer
        = "# END YARN SWITCH MANAGED BLOCK\n";

    let header_position = rc_content
        .find(header);
    let footer_position = rc_content
        .find(footer);

    let final_string
        = header.to_string() + &line + &footer;

    match (header_position, footer_position) {
        (Some(header_position), Some(footer_position)) => {
            rc_content.replace_range(header_position..footer_position + footer.len(), &final_string);
        },

        (Some(header_position), None) => {
            rc_content.replace_range(header_position..header_position + header.len(), &final_string);
        },

        (None, Some(footer_position)) => {
            rc_content.replace_range(footer_position..footer_position + footer.len(), &final_string);
        },

        (None, None) => {
            if rc_content.is_empty() || rc_content.ends_with("\n\n") {
                // All good, we can insert the line right away!
            } else if rc_content.ends_with("\n") {
                rc_content.push('\n');
            } else {
                rc_content.push_str("\n\n");
            }

            rc_content.push_str(&final_string);
        },
    }

    let _ = rc_path
        .fs_change(rc_content, false);

    println!("We updated the {} file for you; please restart your shell to apply the changes.", DataType::Path.colorize(rc_path.as_str()));
}
