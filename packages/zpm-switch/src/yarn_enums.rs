use zpm_macros::parse_enum;
use zpm_utils::{impl_serialization_traits, ToFileString, ToHumanString};

use crate::errors::Error;

#[parse_enum(or_else = |s| Err(Error::InvalidVersionSelector(s.to_string())))]
#[derive(Debug)]
#[derive_variants(Debug)]
pub enum Channel {
    #[pattern(spec = "stable")]
    Stable,

    #[pattern(spec = "canary")]
    Canary,
}

impl ToFileString for Channel {
    fn to_file_string(&self) -> String {
        match self {
            Channel::Stable => "stable".to_string(),
            Channel::Canary => "canary".to_string(),
        }
    }
}

impl ToHumanString for Channel {
    fn to_print_string(&self) -> String {
        self.to_file_string()
    }
}

impl_serialization_traits!(Channel);

#[parse_enum(or_else = |s| Err(Error::InvalidVersionSelector(s.to_string())))]
#[derive(Debug, Copy, Clone)]
#[derive_variants(Debug, Copy, Clone)]
pub enum ReleaseLine {
    #[pattern(spec = "classic")]
    Classic,

    #[pattern(spec = "berry")]
    Berry,

    #[pattern(spec = "zpm")]
    Zpm,

    #[pattern(spec = "default")]
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

impl ToFileString for ReleaseLine {
    fn to_file_string(&self) -> String {
        match self {
            ReleaseLine::Classic => "classic".to_string(),
            ReleaseLine::Berry => "berry".to_string(),
            ReleaseLine::Zpm => "zpm".to_string(),
            ReleaseLine::Default => "default".to_string(),
        }
    }
}

impl ToHumanString for ReleaseLine {
    fn to_print_string(&self) -> String {
        self.to_file_string()
    }
}

impl_serialization_traits!(ReleaseLine);

#[parse_enum(or_else = |s| Err(Error::InvalidVersionSelector(s.to_string())))]
#[derive(Debug)]
#[derive_variants(Debug)]
pub enum Selector {
    #[pattern(spec = "(?<release_line>.*)")]
    #[pattern(spec = "(?<channel>.*)")]
    #[pattern(spec = "(?<release_line>.*)-(?<channel>.*)")]
    Channel {
        release_line: Option<ReleaseLine>,
        channel: Option<Channel>,
    },

    #[pattern(spec = "(?<version>.*)")]
    Version {
        version: zpm_semver::Version,
    },

    #[pattern(spec = "(?<range>.*)")]
    Range {
        range: zpm_semver::Range,
    },
}


impl ToFileString for Selector {
    fn to_file_string(&self) -> String {
        match self {
            Selector::Channel(ChannelSelector {release_line: Some(release_line), channel: None}) => {
                release_line.to_file_string()
            },

            Selector::Channel(ChannelSelector {release_line: None, channel: Some(channel)}) => {
                channel.to_file_string()
            },

            Selector::Channel(ChannelSelector {release_line: None, channel: None}) => {
                "stable".to_string()
            },

            Selector::Channel(ChannelSelector {release_line: Some(release_line), channel: Some(channel)}) => {
                format!("{}-{}", release_line.to_file_string(), channel.to_file_string())
            },

            Selector::Version(VersionSelector {version}) => {
                version.to_file_string()
            },

            Selector::Range(RangeSelector {range}) => {
                range.to_file_string()
            },
        }
    }
}

impl ToHumanString for Selector {
    fn to_print_string(&self) -> String {
        self.to_file_string()
    }
}

impl_serialization_traits!(Selector);
