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

    // Only complete versions; `Version` would also accept partial ones like
    // `4` or `4.x` (as `4.0.0`), which must be resolved as ranges instead
    #[pattern(r"(?<version>v?[0-9]+\.[0-9]+\.[0-9]+(?:[-+].*)?)")]
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

#[cfg(test)]
mod tests {
    use zpm_utils::FromFileString;

    use super::*;

    #[test]
    fn complete_versions_are_versions() {
        for selector in ["4.1.2", "v4.1.2", "4.1.2-rc.1", "6.0.0-rc.23"] {
            assert!(matches!(Selector::from_file_string(selector), Ok(Selector::Version(_))), "{selector}");
        }
    }

    #[test]
    fn partial_versions_are_ranges() {
        for selector in ["4", "4.x", "4.*", "4.1", "4.1.x", "^4.1", "~4.1.0", ">=4 <5"] {
            assert!(matches!(Selector::from_file_string(selector), Ok(Selector::Range(_))), "{selector}");
        }
    }

    #[test]
    fn channels_are_channels() {
        for selector in ["stable", "canary", "berry", "classic", "berry-canary"] {
            assert!(matches!(Selector::from_file_string(selector), Ok(Selector::Channel(_))), "{selector}");
        }
    }
}
