use std::collections::BTreeMap;

use crate::{
    build::BuildRequests,
    error::Error,
    install::{Install, InstallState},
    linker::{nm::hoist::{Hoister, InputTree, WorkTree}, LinkResult},
    project::Project,
};

pub mod hoist;

pub fn hoist_install(project: &Project, install_state: &InstallState) -> Result<WorkTree, Error> {
    let input_tree
        = InputTree::from_install_state(project, install_state);

    let mut work_tree
        = WorkTree::from_input_tree(&input_tree);

    let mut hoister
        = Hoister::new(&mut work_tree);

    hoister.hoist();

    Ok(work_tree)
}

pub async fn link_project_nm(_project: &Project, _install: &Install) -> Result<LinkResult, Error> {
    Ok(LinkResult {
        packages_by_location: BTreeMap::new(),
        build_requests: BuildRequests {
            entries: vec![],
            dependencies: BTreeMap::new(),
        },
    })
}
