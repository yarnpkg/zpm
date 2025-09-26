use std::{str::FromStr, sync::LazyLock};

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use zpm_config::SupportedArchitectures;
use zpm_parsers::JsonDocument;
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
        let arch = std::env::var("YARN_CPU_OVERRIDE").ok()
            .map_or(Some(ARCH), |s| Some(zpm_config::Cpu::from_str(&s).unwrap()));

        let os = std::env::var("YARN_OS_OVERRIDE").ok()
            .map_or(Some(OS), |s| Some(zpm_config::Os::from_str(&s).unwrap()));

        let libc = std::env::var("YARN_LIBC_OVERRIDE").ok()
            .map_or(detect_libc(), |s| Some(zpm_config::Libc::from_str(&s).unwrap()));

        Self {
            arch,
            os,
            libc,
        }
    }

    pub fn from_supported_architectures(supported_architectures: &SupportedArchitectures) -> Vec<Self> {
        let mut systems
            = Vec::new();

        let current
            = Self::from_current();

        let cpus = if supported_architectures.cpu.is_empty() {
            vec![&zpm_config::Cpu::Current]
        } else {
            supported_architectures.cpu.iter().map(|c| &c.value).collect()
        };

        let os = if supported_architectures.os.is_empty() {
            vec![&zpm_config::Os::Current]
        } else {
            supported_architectures.os.iter().map(|o| &o.value).collect()
        };

        let libc = if supported_architectures.libc.is_empty() {
            vec![&zpm_config::Libc::Current]
        } else {
            supported_architectures.libc.iter().map(|l| &l.value).collect()
        };

        for &cpu in &cpus {
            for &os in &os {
                for &libc in &libc {
                    let arch = if cpu == &zpm_config::Cpu::Current {
                        current.arch.clone()
                    } else {
                        Some(cpu.clone())
                    };

                    let os = if os == &zpm_config::Os::Current {
                        current.os.clone()
                    } else {
                        Some(os.clone())
                    };

                    let libc = if libc == &zpm_config::Libc::Current {
                        current.libc.clone()
                    } else {
                        Some(libc.clone())
                    };

                    systems.push(Self {
                        arch,
                        os,
                        libc,
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

impl FromStr for Requirements {
    type Err = zpm_parsers::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(JsonDocument::hydrate_from_str(s)?)
    }
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
