use std::{str::FromStr, sync::LazyLock};

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use zpm_config::SupportedArchitectures;
use zpm_utils::Path;

const LDD_PATH: &str = "/usr/bin/ldd";

#[cfg(target_arch = "x86_64")]
const ARCH: zpm_config::Cpu = zpm_config::Cpu::X86_64;

#[cfg(target_arch = "aarch64")]
const ARCH: zpm_config::Cpu = zpm_config::Cpu::Aarch64;

#[cfg(target_os = "linux")]
const OS: zpm_config::Os = zpm_config::Os::Linux;

#[cfg(target_os = "macos")]
const OS: zpm_config::Os = zpm_config::Os::MacOS;

#[cfg(target_os = "windows")]
const OS: zpm_config::Os = zpm_config::Os::Windows;

#[cfg(target_env = "gnu")]
const LIBC: Option<zpm_config::Libc> = Some(zpm_config::Libc::Glibc);

#[cfg(target_env = "musl")]
const LIBC: Option<zpm_config::Libc> = Some(zpm_config::Libc::Musl);

#[cfg(target_env = "")]
const LIBC: Option<zpm_config::Libc> = None;

fn detect_libc() -> Option<zpm_config::Libc> {
    let ldd_contents
        = Path::from_str(LDD_PATH).unwrap()
            .fs_read_text_prealloc()
            .ok();

    if let Some(ldd_contents) = ldd_contents {
        if ldd_contents.contains("GLIBC") || ldd_contents.contains("GNU libc") || ldd_contents.contains("GNU C Library") {
            return Some(zpm_config::Libc::Glibc);
        }

        if ldd_contents.contains("musl") {
            return Some(zpm_config::Libc::Musl);
        }
    }

    LIBC
}

#[derive(Debug)]
pub struct System {
    arch: Option<zpm_config::Cpu>,
    os: Option<zpm_config::Os>,
    libc: Option<zpm_config::Libc>,
}

static CURRENT_DESCRIPTION: LazyLock<System> = LazyLock::new(|| {
    System::from_current()
});

impl System {
    pub fn current() -> &'static Self {
        &*CURRENT_DESCRIPTION
    }

    pub fn from_current() -> Self {
        Self {
            arch: Some(ARCH),
            os: Some(OS),
            libc: detect_libc().map(|s| s),
        }
    }

    pub fn from_supported_architectures(supported_architectures: &SupportedArchitectures) -> Vec<Self> {
        let mut systems
            = Vec::new();

        for cpu in supported_architectures.cpu.iter() {
            for os in supported_architectures.os.iter() {
                for libc in supported_architectures.libc.iter() {
                    systems.push(Self {
                        arch: Some(if cpu.value == zpm_config::Cpu::Current {ARCH} else {cpu.value.clone()}),
                        os: Some(if os.value == zpm_config::Os::Current {OS} else {os.value.clone()}),
                        libc: if libc.value == zpm_config::Libc::Current {LIBC} else {Some(libc.value.clone())},
                    });
                }
            }
        }

        systems
    }
}

#[derive(Clone, Debug, Default, Deserialize, Decode, Encode, Serialize, PartialEq, Eq)]
pub struct Requirements {
    #[serde(default)]
    #[serde(rename = "cpu")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    arch: Vec<zpm_config::Cpu>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    os: Vec<zpm_config::Os>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    libc: Vec<zpm_config::Libc>,
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
