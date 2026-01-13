use zpm_macro_enum::zpm_enum;
use zpm_utils::ToFileString;

use crate::errors::Error;

fn format_channel_selector(release_line: &Option<ReleaseLine>, channel: &Option<Channel>) -> String {
    match (release_line, channel) {
        (None, None) => "stable".to_string(),
        (Some(release_line), None) => release_line.to_file_string(),
        (None, Some(channel)) => channel.to_file_string(),
        (Some(release_line), Some(channel)) => format!("{}-{}", release_line.to_file_string(), channel.to_file_string()),
    }
}

#[zpm_enum(or_else = |s| Err(Error::InvalidVersionSelector(s.to_string())))]
#[derive(Debug)]
#[derive_variants(Debug)]
pub enum Channel {
    #[pattern("stable")]
    #[to_file_string(|| "stable".to_string())]
    #[to_print_string(|| "stable".to_string())]
    Stable,

    #[pattern("canary")]
    #[to_file_string(|| "canary".to_string())]
    #[to_print_string(|| "canary".to_string())]
    Canary,
}


#[zpm_enum(or_else = |s| Err(Error::InvalidVersionSelector(s.to_string())))]
#[derive(Debug, Copy, Clone)]
#[derive_variants(Debug, Copy, Clone)]
pub enum ReleaseLine {
    #[pattern("classic")]
    #[to_file_string(|| "classic".to_string())]
    #[to_print_string(|| "classic".to_string())]
    Classic,

    #[pattern("berry")]
    #[to_file_string(|| "berry".to_string())]
    #[to_print_string(|| "berry".to_string())]
    Berry,

    #[pattern("zpm")]
    #[to_file_string(|| "zpm".to_string())]
    #[to_print_string(|| "zpm".to_string())]
    Zpm,

    #[pattern("default")]
    #[to_file_string(|| "default".to_string())]
    #[to_print_string(|| "default".to_string())]
    Default,
}

impl ReleaseLine {
    pub fn stable(&self) -> ChannelSelector {
        ChannelSelector {
            release_line: Some(*self),
            channel: Some(Channel::Stable),
        }
    }

    pub fn canary(&self) -> ChannelSelector {
        ChannelSelector {
            release_line: Some(*self),
            channel: Some(Channel::Canary),
        }
    }
}


#[zpm_enum(or_else = |s| Err(Error::InvalidVersionSelector(s.to_string())))]
#[derive(Debug)]
#[derive_variants(Debug)]
pub enum Selector {
    #[pattern("(?<release_line>.*)")]
    #[pattern("(?<channel>.*)")]
    #[pattern("(?<release_line>.*)-(?<channel>.*)")]
    #[to_file_string(|params| format_channel_selector(&params.release_line, &params.channel))]
    #[to_print_string(|params| format_channel_selector(&params.release_line, &params.channel))]
    Channel {
        release_line: Option<ReleaseLine>,
        channel: Option<Channel>,
    },

    #[pattern("(?<version>.*)")]
    #[to_file_string(|params| params.version.to_file_string())]
    #[to_print_string(|params| params.version.to_file_string())]
    Version {
        version: zpm_semver::Version,
    },

    #[pattern("(?<range>.*)")]
    #[to_file_string(|params| params.range.to_file_string())]
    #[to_print_string(|params| params.range.to_file_string())]
    Range {
        range: zpm_semver::Range,
    },
}
