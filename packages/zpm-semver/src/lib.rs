mod error;
mod extract;
mod range;
mod version;

pub use error::Error;
pub use range::Range;
pub use range::RangeKind;
pub use version::Version;
pub use version::VersionRc;

/// JS-compatible semver limits:
/// https://github.com/npm/node-semver/blob/120968b76760cb0db85a72bde2adedd0e9628793/internal/constants.js
pub const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
/// Maximum length of a semver string.
pub const MAX_LENGTH: usize = 256;
/// Maximum digits allowed for a numeric component (major/minor/patch).
pub const MAX_SAFE_COMPONENT_LENGTH: usize = 16;
