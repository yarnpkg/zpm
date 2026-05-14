use std::process::ExitStatus;

use clipanion::cli;
use zpm_utils::ToFileString;

use crate::{
    commands::switch::explicit::ExplicitCommand,
    cwd::{get_fake_cwd, get_final_cwd},
    errors::Error,
    manifest::{find_closest_package_manager, VersionPackageManagerReference},
    yarn::resolve_selector,
    yarn_enums::{ChannelSelector, ReleaseLine, Selector},
};

/// Set the version of Yarn to use with the local project to the latest stable release
///
/// When called without arguments, downloads the latest stable Yarn version from the `zpm` release
/// line and runs `yarn set version zpm` with it. An optional version selector can be passed to
/// pick a specific release line, channel, version, or range, using the same syntax as the
/// explicit `yarn switch <selector>` form.
///
#[cli::command]
#[cli::path("switch", "up")]
#[cli::category("General commands")]
#[derive(Debug)]
pub struct UpCommand {
    selector: Option<Selector>,
}

impl UpCommand {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let lookup_path
            = get_final_cwd()?;

        let find_result
            = find_closest_package_manager(&lookup_path)?;

        if let Some(detected_root_path) = find_result.detected_root_path {
            std::env::set_var("YARNSW_DETECTED_ROOT", detected_root_path.to_file_string());
        }

        let default_selector = Selector::Channel(ChannelSelector {
            release_line: Some(ReleaseLine::Zpm),
            channel: None,
        });

        let selector = self.selector
            .as_ref()
            .unwrap_or(&default_selector);

        let version
            = resolve_selector(selector).await?;

        let reference
            = VersionPackageManagerReference {version};

        let mut args = vec![
            "set".to_string(),
            "version".to_string(),
            selector.to_file_string(),
        ];

        if let Some(cwd) = get_fake_cwd() {
            args.insert(0, cwd.to_file_string());
        }

        ExplicitCommand::run(&reference.into(), &args).await
    }
}
