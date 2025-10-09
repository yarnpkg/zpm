use std::collections::BTreeMap;

use zpm_config::NodeLinker;
use zpm_primitives::Locator;
use zpm_utils::Path;

use crate::{
    build::BuildRequests,
    error::Error,
    install::Install,
    project::Project,
};

pub mod helpers;
pub mod nm;
pub mod pnpm;
pub mod pnp;

#[derive(Debug)]
pub struct LinkResult {
    pub packages_by_location: BTreeMap<Path, Locator>,
    pub build_requests: BuildRequests,
}

pub async fn link_project<'a>(project: &'a Project, install: &'a Install) -> Result<LinkResult, Error> {
    match project.config.settings.node_linker.value {
        NodeLinker::NodeModules
            => nm::link_project_nm(project, install).await,

        NodeLinker::Pnp
            => pnp::link_project_pnp(project, install).await,

        NodeLinker::Pnpm
            => pnpm::link_project_pnpm(project, install).await,
    }
}
