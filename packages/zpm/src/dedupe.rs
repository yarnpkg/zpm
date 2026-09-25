use std::collections::BTreeMap;

use itertools::Itertools;
use zpm_primitives::{Descriptor, Ident, IdentGlob, Locator, Range, Reference, RegistryReference, RegistrySemverRange, ShorthandReference};

use crate::install::InstallState;

fn extract_semver_version(locator: &Locator) -> Option<(&Ident, &zpm_semver::Version)> {
    match &locator.reference {
        Reference::Shorthand(params)
            => Some((&locator.ident, &params.version)),

        Reference::Registry(params)
            => Some((&params.ident, &params.version)),

        _ => None,
    }
}

pub fn prepare_highest_dedupe(install_state: &InstallState, patterns: &[IdentGlob]) -> BTreeMap<Descriptor, Locator> {

    let locators_by_ident
        = install_state.normalized_resolutions.keys()
            .filter_map(extract_semver_version)
            .into_group_map_by(|(ident, _)| *ident);

    let best_version_for = |ident: &Ident, range: &zpm_semver::Range| {
        locators_by_ident.get(&ident).into_iter().flatten()
            .map(|(_, version)| *version)
            .filter(|version| range.check(version))
            .max()
    };

    let attach_highest_version = |descriptor: &Descriptor| {
        let suggested_locator = match &descriptor.range {
            Range::AnonymousSemver(params) => {
                let best_version
                    = best_version_for(&descriptor.ident, &params.range);

                best_version.map(|version| {
                    Locator::new(descriptor.ident.clone(), ShorthandReference {
                        version: version.clone(),
                    }.into())
                })
            },

            Range::RegistrySemver(RegistrySemverRange {ident: None, range}) => {
                let best_version
                    = best_version_for(&descriptor.ident, range);

                best_version.map(|version| {
                    Locator::new(descriptor.ident.clone(), ShorthandReference {
                        version: version.clone(),
                    }.into())
                })
            },

            Range::RegistrySemver(RegistrySemverRange {ident: Some(ident), range}) => {
                let best_version
                    = best_version_for(ident, range);

                best_version.map(|version| {
                    Locator::new(descriptor.ident.clone(), RegistryReference {
                        ident: ident.clone(),
                        version: version.clone(),
                        url: None,
                    }.into())
                })
            },

            _ => return None,
        };

        suggested_locator.and_then(|locator| {
            let current_resolution
                = install_state.descriptor_to_locator
                    .get(descriptor);

            if current_resolution != Some(&locator) {
                Some((descriptor.clone(), locator))
            } else {
                None
            }
        })
    };

    let upgradable_candidates
        = install_state.descriptor_to_locator.keys()
            .filter(|descriptor| patterns.is_empty() || patterns.iter().any(|matcher| matcher.check(&descriptor.ident)))
            .filter_map(attach_highest_version)
            .collect::<BTreeMap<_, _>>();

    upgradable_candidates
}
