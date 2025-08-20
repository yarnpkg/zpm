use std::sync::{LazyLock, OnceLock};

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

#[cfg(target_arch = "x86_64")]
const ARCH: &str = "x64";

#[cfg(target_arch = "aarch64")]
const ARCH: &str = "arm64";

#[cfg(target_os = "linux")]
const OS: &str = "linux";

#[cfg(target_os = "macos")]
const OS: &str = "darwin";

#[cfg(target_os = "windows")]
const OS: &str = "win32";

#[cfg(target_env = "gnu")]
const LIBC: Option<&str> = Some("glibc");

#[cfg(target_env = "musl")]
const LIBC: Option<&str> = Some("musl");

#[cfg(target_env = "")]
const LIBC: Option<&str> = None;

#[derive(Debug)]
pub struct Description {
    arch: Option<(String, String)>,
    os: Option<(String, String)>,
    libc: Option<(String, String)>,
}

static CURRENT_DESCRIPTION: LazyLock<Description> = LazyLock::new(|| {
    Description::from_current()
});

impl Description {
    pub fn current() -> &'static Self {
        &*CURRENT_DESCRIPTION
    }

    pub fn from_current() -> Self {
        Self {
            arch: Some((ARCH.to_string(), format!("!{}", ARCH))),
            os: Some((OS.to_string(), format!("!{}", OS))),
            libc: LIBC.map(|s| (s.to_string(), format!("!{}", s))),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Decode, Encode, Serialize, PartialEq, Eq)]
pub struct Requirements {
    #[serde(default)]
    #[serde(rename = "cpu")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    arch: Vec<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    os: Vec<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    libc: Vec<String>,
}

impl Requirements {
    pub fn is_conditional(&self) -> bool {
        !self.arch.is_empty() || !self.os.is_empty() || !self.libc.is_empty()
    }

    pub fn validate(&self, info: &Description) -> bool {
        if let Some((requirement, inversed)) = &info.arch {
            if !self.arch.is_empty() && (!self.arch.contains(requirement) || self.arch.contains(inversed)) {
                return false;
            }
        }

        if let Some((requirement, inversed)) = &info.os {
            if !self.os.is_empty() && (!self.os.contains(requirement) || self.os.contains(inversed)) {
                return false;
            }
        }

        if let Some((requirement, inversed)) = &info.libc {
            if !self.libc.is_empty() && (!self.libc.contains(requirement) || self.libc.contains(inversed)) {
                return false;
            }
        }

        true
    }
}
