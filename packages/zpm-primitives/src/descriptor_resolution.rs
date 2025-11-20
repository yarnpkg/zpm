use serde::Serialize;
use zpm_utils::ToHumanString;

use crate::{Descriptor, Locator};

#[derive(Debug, Serialize)]
pub struct DescriptorResolution {
    pub descriptor: Descriptor,
    pub locator: Locator,
}

impl DescriptorResolution {
    pub fn new(descriptor: Descriptor, locator: Locator) -> DescriptorResolution {
        DescriptorResolution {
            descriptor,
            locator,
        }
    }
}

impl ToHumanString for DescriptorResolution {
    fn to_print_string(&self) -> String {
        format!("{} (via {})", self.locator.to_print_string(), self.descriptor.range.to_print_string())
    }
}
