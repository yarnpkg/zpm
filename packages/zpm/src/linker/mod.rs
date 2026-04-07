use std::collections::BTreeMap;

use zpm_config::{IslandLinker, NodeLinker};
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
    let mut result = match project.config.settings.node_linker.value {
        NodeLinker::NodeModules
            => nm::link_project_nm(project, install).await?,

        NodeLinker::Pnp
            => pnp::link_project_pnp(project, install).await?,

        NodeLinker::Pnpm
            => pnpm::link_project_pnpm(project, install).await?,
    };

    // Per-island link steps
    for island in &install.resolved_islands {
        match island.linker {
            IslandLinker::NodeModules => {
                let island_result = nm::link_island_nm(project, install, island).await?;
                result.packages_by_location.extend(island_result.packages_by_location);
                result.build_requests.entries.extend(island_result.build_requests.entries);
            }
        }
    }

    Ok(result)
}
