pub mod descriptor;
pub mod ident;
pub mod locator;
pub mod loose_descriptor;
pub mod range;
pub mod reference;
pub mod semver_descriptor;
pub mod system;

#[cfg(test)]
pub mod testing;

pub use descriptor::Descriptor;
pub use ident::Ident;
pub use locator::Locator;
pub use loose_descriptor::LooseDescriptor;
pub use range::{PeerRange, Range};
pub use reference::Reference;
pub use semver_descriptor::SemverDescriptor;
pub use system::Requirements;
