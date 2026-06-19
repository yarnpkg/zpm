use std::collections::BTreeMap;

use zpm_config::{IslandLinker, NodeLinker};
use zpm_primitives::Locator;
use zpm_utils::{IoResultExt, Path};

use crate::{
    build::BuildRequests,
    error::Error,
    install::Install,
    project::Project,
};

pub mod helpers;
pub mod nm;
pub mod package_map;
pub mod pnpm;
pub mod pnp;
pub mod venv;

#[derive(Debug)]
pub struct LinkResult {
    pub packages_by_location: BTreeMap<Path, Locator>,
    pub build_requests: BuildRequests,
}

pub async fn link_project<'a>(project: &'a Project, install: &'a Install) -> Result<LinkResult, Error> {
    cleanup_inactive_linker_artifacts(project)?;

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
            },

            IslandLinker::Venv => {
                let island_result = venv::link_island_venv(project, install, island).await?;
                result.packages_by_location.extend(island_result.packages_by_location);
                result.build_requests.entries.extend(island_result.build_requests.entries);
            },
        }
    }

    Ok(result)
}

fn cleanup_inactive_linker_artifacts(project: &Project) -> Result<(), Error> {
    let active = project.config.settings.node_linker.value;

    if active != NodeLinker::Pnp {
        for path in [
            project.pnp_path(),
            project.pnp_data_path(),
            project.pnp_loader_path(),
            project.unplugged_path(),
        ] {
            if path.fs_exists() {
                path.fs_rm()?;
            }
        }
    }

    if active == NodeLinker::Pnp {
        project.package_map_path(None)
            .fs_rm()
            .ok_missing()?;
    }

    Ok(())
}
