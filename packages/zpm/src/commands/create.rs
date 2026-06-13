use std::process::ExitStatus;

use clipanion::cli;
use zpm_primitives::{split_ident_and_selector, AnonymousSemverRange, Descriptor, Ident, Range, RegistryTagRange};
use zpm_utils::{FromFileString, ToFileString};

use crate::{
    commands::dlx::{install_and_run_single, InstallAndRunOptions},
    descriptor_loose::{DescriptorLooseDescriptor, IdentLooseDescriptor, LooseDescriptor},
    error::Error,
};

/// Create a new package from a starter kit
///
/// This command uses `dlx` to fetch a package named `create-<name>` (or `@<scope>/create-<name>` for scoped packages), then runs its binary.
///
/// For example:
///
///   - `yarn create react-app`  →  `yarn dlx create-react-app`
///   - `yarn create @scope/app` →  `yarn dlx @scope/create-app`
///
#[cli::command(proxy)]
#[cli::path("create")]
#[cli::category("Scripting commands")]
pub struct Create {
    /// Suppress the install unless it errors
    #[cli::option("-q,--quiet", default = false)]
    quiet: bool,

    /// Starter kit name, such as `react-app` or `@scope/app`
    starter: String,

    /// Arguments to pass to the starter kit's binary
    args: Vec<String>,
}

impl Create {
    pub async fn execute(&self) -> Result<ExitStatus, Error> {
        let (initializer_ident, range_part) = rewrite_starter(&self.starter)?;

        let parsed_range = range_part.as_deref().map(parse_range);

        let descriptor_print
            = format!(
                "{}@{}",
                initializer_ident.to_file_string(),
                parsed_range
                    .as_ref()
                    .map(|range| range.to_file_string())
                    .unwrap_or_else(|| "unknown".to_string()),
            );

        let loose_descriptor = match parsed_range {
            Some(range) => LooseDescriptor::Descriptor(DescriptorLooseDescriptor {
                descriptor: Descriptor::new(initializer_ident.clone(), range),
            }),
            None => LooseDescriptor::Ident(IdentLooseDescriptor {
                ident: initializer_ident.clone(),
            }),
        };

        install_and_run_single(loose_descriptor, InstallAndRunOptions {
            args: self.args.clone(),
            quiet: self.quiet,
            banner: Some(format!("Installing {}...", descriptor_print)),
            fallback_binary: true,
            ..Default::default()
        }).await
    }
}

fn rewrite_starter(input: &str) -> Result<(Ident, Option<String>), Error> {
    let (selector, range_part) = split_ident_and_selector(input);
    let range_part = range_part.map(str::to_string);

    let new_ident_str = if let Some(stripped) = selector.strip_prefix('@') {
        if let Some((scope, rest)) = stripped.split_once('/') {
            format!("@{}/create-{}", scope, rest)
        } else {
            format!("@{}/create", stripped)
        }
    } else {
        format!("create-{}", selector)
    };

    let ident = Ident::from_file_string(&new_ident_str)
        .map_err(|_| Error::InvalidIdent(new_ident_str.clone()))?;

    Ok((ident, range_part))
}

fn parse_range(range_str: &str) -> Range {
    if let Ok(semver_range) = zpm_semver::Range::from_file_string(range_str) {
        return Range::AnonymousSemver(AnonymousSemverRange { range: semver_range });
    }

    Range::RegistryTag(RegistryTagRange { ident: None, tag: range_str.into() })
}
