use std::str::FromStr;
use std::sync::LazyLock;

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use zpm_macro_enum::zpm_enum;

use crate::{EnumError, Path, ToFileString};

use crate as zpm_utils;

pub fn get_system_string() -> &'static str {
    env!("TARGET")
}

const LDD_PATH: &str = "/usr/bin/ldd";

#[cfg(target_arch = "x86_64")]
const ARCH: Cpu = Cpu::X86_64;

#[cfg(target_arch = "aarch64")]
const ARCH: Cpu = Cpu::Aarch64;

#[cfg(target_arch = "x86")]
const ARCH: Cpu = Cpu::I386;

#[cfg(target_os = "linux")]
const OS: Os = Os::Linux;

#[cfg(target_os = "macos")]
const OS: Os = Os::MacOS;

#[cfg(target_os = "windows")]
const OS: Os = Os::Windows;

#[cfg(target_env = "gnu")]
const LIBC: Option<Libc> = Some(Libc::Glibc);

#[cfg(target_env = "musl")]
const LIBC: Option<Libc> = Some(Libc::Musl);

#[cfg(target_env = "")]
const LIBC: Option<Libc> = None;

fn detect_libc() -> Option<Libc> {
    let ldd_contents
        = Path::from_str(LDD_PATH).unwrap()
            .fs_read_text_prealloc()
            .ok();

    if let Some(ldd_contents) = ldd_contents {
        if ldd_contents.contains("GLIBC") || ldd_contents.contains("GNU libc") || ldd_contents.contains("GNU C Library") {
            return Some(Libc::Glibc);
        }

        if ldd_contents.contains("musl") {
            return Some(Libc::Musl);
        }
    }

    LIBC
}

#[derive(Debug)]
pub struct System {
    pub arch: Option<Cpu>,
    pub os: Option<Os>,
    pub libc: Option<Libc>,
}

impl System {
    pub fn without_libc(&self) -> Self {
        Self {
            arch: self.arch.clone(),
            os: self.os.clone(),
            libc: None,
        }
    }

    pub fn to_requirements(&self) -> Requirements {
        Requirements {
            arch: self.arch.clone().into_iter().collect(),
            os: self.os.clone().into_iter().collect(),
            libc: self.libc.clone().into_iter().collect(),
        }
    }
}

impl ToFileString for System {
    fn to_file_string(&self) -> String {
        let mut segments
            = vec![];

        if let Some(os) = &self.os {
            segments.push(os.to_file_string());
        }

        if let Some(arch) = &self.arch {
            segments.push(arch.to_file_string());
        }

        if let Some(libc) = &self.libc {
            segments.push(libc.to_file_string());
        }

        if segments.is_empty() {
            return "unknown".to_string();
        }

        segments.join("-")
    }
}

static CURRENT_DESCRIPTION: LazyLock<System> = LazyLock::new(|| {
    System::from_current()
});

impl System {
    pub fn current() -> &'static Self {
        &*CURRENT_DESCRIPTION
    }

    pub fn from_current() -> Self {
        let arch = std::env::var("YARN_CPU_OVERRIDE").ok()
            .map_or(Some(ARCH), |s| Some(Cpu::from_str(&s).unwrap()));

        let os = std::env::var("YARN_OS_OVERRIDE").ok()
            .map_or(Some(OS), |s| Some(Os::from_str(&s).unwrap()));

        let libc = std::env::var("YARN_LIBC_OVERRIDE").ok()
            .map_or(detect_libc(), |s| Some(Libc::from_str(&s).unwrap()));

        Self {
            arch,
            os,
            libc,
        }
    }
}

#[zpm_enum(error = EnumError, or_else = |s| Err(EnumError::NotFound(s.to_string())))]
#[derive(Debug, Clone, Encode, Decode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Cpu {
    #[literal("current")]
    Current,

    #[literal("ia32")]
    I386,

    #[literal("x64")]
    X86_64,

    #[literal("arm64")]
    Aarch64,

    #[fallback]
    Other(String),
}

#[zpm_enum(error = EnumError, or_else = |s| Err(EnumError::NotFound(s.to_string())))]
#[derive(Debug, Clone, Encode, Decode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Libc {
    #[literal("current")]
    Current,

    #[literal("glibc")]
    Glibc,

    #[literal("musl")]
    Musl,

    #[fallback]
    Other(String),
}

#[zpm_enum(error = EnumError, or_else = |s| Err(EnumError::NotFound(s.to_string())))]
#[derive(Debug, Clone, Encode, Decode, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Os {
    #[literal("current")]
    Current,

    #[literal("darwin")]
    MacOS,

    #[literal("linux")]
    Linux,

    #[literal("win32")]
    Windows,

    #[fallback]
    Other(String),
}

#[derive(Clone, Debug, Default, Deserialize, Decode, Encode, Serialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Requirements {
    #[serde(default)]
    #[serde(rename = "cpu")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    arch: Vec<Cpu>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    os: Vec<Os>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    libc: Vec<Libc>,
}

impl Requirements {
    pub fn is_conditional(&self) -> bool {
        !self.arch.is_empty() || !self.os.is_empty() || !self.libc.is_empty()
    }

    pub fn validate_system(&self, system: &System) -> bool {
        let is_arch_valid
            = self.arch.is_empty() || system.arch.as_ref().map_or(false, |arch| self.arch.contains(&arch));

        if !is_arch_valid {
            return false;
        }

        let is_os_valid
            = self.os.is_empty() || system.os.as_ref().map_or(false, |os| self.os.contains(&os));

        if !is_os_valid {
            return false;
        }

        let is_libc_valid
            = self.libc.is_empty() || system.libc.as_ref().map_or(false, |libc| self.libc.contains(&libc));

        if !is_libc_valid {
            return false;
        }

        true
    }

    pub fn validate_any(&self, info: &Vec<System>) -> bool {
        let is_arch_valid = self.arch.is_empty() || self.arch.iter()
            .any(|requirement| info.iter().any(|system| system.arch.as_ref() == Some(requirement)));

        if !is_arch_valid {
            return false;
        }

        let is_os_valid = self.os.is_empty() || self.os.iter()
            .any(|requirement| info.iter().any(|system| system.os.as_ref() == Some(requirement)));

        if !is_os_valid {
            return false;
        }

        let is_libc_valid = self.libc.is_empty() || self.libc.iter()
            .any(|requirement| info.iter().any(|system| system.libc.as_ref() == Some(requirement)));

        if !is_libc_valid {
            return false;
        }

        true
    }
}
