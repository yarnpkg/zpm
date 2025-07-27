use crate::{build::BuildRequests, error::Error, install::Install, project::Project, settings::NodeLinker};

pub mod helpers;
pub mod nm;
pub mod pnpm;
pub mod pnp;

pub async fn link_project<'a>(project: &'a mut Project, install: &'a mut Install) -> Result<BuildRequests, Error> {
    match project.config.project.node_linker.value {
        NodeLinker::Nm => nm::link_project_nm(project, install).await,
        NodeLinker::Pnp => pnp::link_project_pnp(project, install).await,
        NodeLinker::Pnpm => pnpm::link_project_pnpm(project, install).await,
    }
}
