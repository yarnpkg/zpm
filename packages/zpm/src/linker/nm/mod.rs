mod hoist_test;
pub mod hoist;

pub use hoist::{hoist, HoistOptions, HoisterNode, HoisterTree, HoisterResult, HoisterDependencyKind};

use std::collections::BTreeMap;

use crate::{
    build::BuildRequests,
    install::Install,
    project::Project,
    error::Error,
};

pub async fn link_project_nm(_project: &mut Project, _install: &mut Install) -> Result<BuildRequests, Error> {
    // TODO: Implement the actual linking logic using the hoisting algorithm
    Ok(BuildRequests {
        entries: vec![],
        dependencies: BTreeMap::new(),
    })
}
