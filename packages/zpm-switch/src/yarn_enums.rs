use std::fmt;

use zpm_macro_enum::zpm_enum;
use zpm_utils::ToFileString;

use crate::errors::Error;

fn format_channel_selector(release_line: &Option<ReleaseLine>, channel: &Option<Channel>) -> String {
    let mut buffer = String::new();
    let _ = write_channel_selector(release_line, channel, &mut buffer);
    buffer
}

fn write_channel_selector<W: fmt::Write>(release_line: &Option<ReleaseLine>, channel: &Option<Channel>, out: &mut W) -> fmt::Result {
    match (release_line, channel) {
        (None, None) => out.write_str("stable"),
        (Some(release_line), None) => release_line.write_file_string(out),
        (None, Some(channel)) => channel.write_file_string(out),
        (Some(release_line), Some(channel)) => {
            release_line.write_file_string(out)?;
            out.write_str("-")?;
            channel.write_file_string(out)
        },
    }
}
#[zpm_enum(or_else = |s| Err(Error::InvalidVersionSelector(s.to_string())))]
#[derive(Debug)]
#[derive_variants(Debug)]
pub enum Channel {
    #[pattern("stable")]
    #[to_file_string(|| "stable".to_string())]
    #[write_file_string(|out| out.write_str("stable"))]
    #[to_print_string(|| "stable".to_string())]
    Stable,

    #[pattern("canary")]
    #[to_file_string(|| "canary".to_string())]
    #[write_file_string(|out| out.write_str("canary"))]
    #[to_print_string(|| "canary".to_string())]
    Canary,
}


#[zpm_enum(or_else = |s| Err(Error::InvalidVersionSelector(s.to_string())))]
#[derive(Debug, Copy, Clone)]
#[derive_variants(Debug, Copy, Clone)]
pub enum ReleaseLine {
    #[pattern("classic")]
    #[to_file_string(|| "classic".to_string())]
    #[write_file_string(|out| out.write_str("classic"))]
    #[to_print_string(|| "classic".to_string())]
    Classic,

    #[pattern("berry")]
    #[to_file_string(|| "berry".to_string())]
    #[write_file_string(|out| out.write_str("berry"))]
    #[to_print_string(|| "berry".to_string())]
    Berry,

    #[pattern("zpm")]
    #[to_file_string(|| "zpm".to_string())]
    #[write_file_string(|out| out.write_str("zpm"))]
    #[to_print_string(|| "zpm".to_string())]
    Zpm,

    #[pattern("default")]
    #[to_file_string(|| "default".to_string())]
    #[write_file_string(|out| out.write_str("default"))]
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
    #[write_file_string(|params, out| write_channel_selector(&params.release_line, &params.channel, out))]
    #[to_print_string(|params| format_channel_selector(&params.release_line, &params.channel))]
    Channel {
        release_line: Option<ReleaseLine>,
        channel: Option<Channel>,
    },

    #[pattern("(?<version>.*)")]
    #[to_file_string(|params| {
        let mut buffer = String::new();
        let _ = params.version.write_file_string(&mut buffer);
        buffer
    })]
    #[write_file_string(|params, out| params.version.write_file_string(out))]
    #[to_print_string(|params| {
        let mut buffer = String::new();
        let _ = params.version.write_file_string(&mut buffer);
        buffer
    })]
    Version {
        version: zpm_semver::Version,
    },

    #[pattern("(?<range>.*)")]
    #[to_file_string(|params| {
        let mut buffer = String::new();
        let _ = params.range.write_file_string(&mut buffer);
        buffer
    })]
    #[write_file_string(|params, out| params.range.write_file_string(out))]
    #[to_print_string(|params| {
        let mut buffer = String::new();
        let _ = params.range.write_file_string(&mut buffer);
        buffer
    })]
    Range {
        range: zpm_semver::Range,
    },
}
