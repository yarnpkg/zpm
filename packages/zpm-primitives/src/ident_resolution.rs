use serde::Serialize;
use zpm_utils::{DataType, ToHumanString};

use crate::{Ident, Locator};

#[derive(Debug, Serialize)]
pub struct IdentResolution {
    pub ident: Ident,
    pub locator: Option<Locator>,
}

impl IdentResolution {
    pub fn new(ident: Ident, locator: Option<Locator>) -> IdentResolution {
        IdentResolution {
            ident,
            locator,
        }
    }
}

impl ToHumanString for IdentResolution {
    fn to_print_string(&self) -> String {
        if let Some(locator) = &self.locator {
            format!("{} → {}", self.ident.to_print_string(), locator.to_print_string())
        } else {
            format!("{} → {}", self.ident.to_print_string(), DataType::Error.colorize("✘"))
        }
    }
}
