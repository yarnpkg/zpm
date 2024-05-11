use tokio;
use zpm::{linker, project};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    project::top_level_dependencies()?;
    project::lockfile()?;

    println!("---");

    project::resolutions().await?;
    project::resolution_checksums().await?;
    project::persist_lockfile().await?;

    println!("---");

    linker::link_project().await?;

    Ok(())
}
