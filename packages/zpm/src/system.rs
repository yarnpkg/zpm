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

pub struct Description {
    arch: Option<String>,
    os: Option<String>,
    libc: Option<String>,
}

impl Description {
    pub fn from_current() -> Self {
        Self {
            arch: Some(ARCH.to_string()),
            os: Some(OS.to_string()),
            libc: LIBC.map(|s| s.to_string()),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Decode, Encode, Serialize, PartialEq, Eq)]
pub struct Requirements {
    #[serde(default)]
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
    pub fn validate(&self, info: &Description) -> bool {
        if let Some(requirement) = &info.arch {
            if self.arch.len() > 0 && !self.arch.contains(&requirement) {
                return false;
            }
        }

        if let Some(requirement) = &info.os {
            if self.os.len() > 0 && !self.os.contains(&requirement) {
                return false;
            }
        }

        if let Some(requirement) = &info.libc {
            if self.libc.len() > 0 && !self.libc.contains(&requirement) {
                return false;
            }
        }

        true
    }
}
