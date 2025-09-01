use zpm_primitives::Ident;

use crate::Configuration;

pub trait ConfigExt {
    fn registry_base_for(&self, ident: &Ident) -> String;
}

impl ConfigExt for Configuration {
    fn registry_base_for(&self, ident: &Ident) -> String {
        // TODO: We should read from npmScopes here
        self.settings.npm_registry_server.value.clone()
    }
}
