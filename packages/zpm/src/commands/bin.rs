use clipanion::cli;
use zpm_parsers::JsonDocument;
use zpm_utils::{ToFileString, ToHumanString};

use crate::{
    error::Error,
    project,
    script::Binary,
};

/// List the binaries available to the current workspace
///
/// This command prints the names of all the binaries the current workspace can run: those exposed by its direct dependencies, plus the ones
/// listed in its own `bin` field. Binaries exposed by transitive dependencies aren't listed, since they aren't accessible from the workspace.
///
/// Adding the `-v,--verbose` flag will cause the output to contain both the binary name and the locator of the package that provides it. The
/// `--json` flag prints one JSON object per binary, with its name, the name of the package that provides it, and its path (the same one as
/// printed by `yarn bin <name>`).
///
/// ## Examples
///
/// List all the available binaries
///
/// `yarn bin`
///
/// List all the available binaries along with the packages that provide them
///
/// `yarn bin -v`
///
#[cli::command]
#[cli::path("bin")]
#[cli::category("Scripting commands")]
pub struct BinList {
    /// Print both the binary name and the locator of the package that provides the binary
    #[cli::option("-v,--verbose", default = false)]
    verbose: bool,

    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,
}

impl BinList {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        project
            .lazy_install().await?;

        let active_package
            = project.active_package()?;

        let binaries
            = project.package_visible_binaries_with_providers(&active_package)?;

        if self.json {
            #[derive(serde::Serialize)]
            struct Payload<'a> {
                name: &'a str,
                source: String,
                path: Option<String>,
            }

            for (name, (provider, binary)) in &binaries {
                let path = match binary {
                    Binary::Path {path, ..} => Some(path.to_file_string()),
                    Binary::PythonEntryPoint {..} => None,
                };

                let payload = Payload {
                    name,
                    source: provider.ident.to_file_string(),
                    path,
                };

                println!("{}", JsonDocument::to_string(&payload)?);
            }
        } else if self.verbose {
            let max_name_length
                = binaries.keys()
                    .map(|name| name.len())
                    .max()
                    .unwrap_or(0);

            for (name, (provider, _)) in &binaries {
                println!("{:<width$}   {}", name, provider.to_print_string(), width = max_name_length);
            }
        } else {
            for name in binaries.keys() {
                println!("{}", name);
            }
        }

        Ok(())
    }
}

/// Print the path of an accessible binary
///
/// This command prints the path to a binary exposed by one of the current workspace's direct dependencies, or by the workspace's own `bin`
/// field, then exits. The reported path may point inside a zip archive.
///
#[cli::command]
#[cli::path("bin")]
#[cli::category("Scripting commands")]
pub struct Bin {
    /// Binary name to resolve
    name: String,
}

impl Bin {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = project::Project::new(None).await?;

        project
            .lazy_install().await?;

        let active_package
            = project.active_package()?;

        let binary
            = project.package_visible_binaries(&active_package)?
                .remove(&self.name)
                .ok_or_else(|| Error::BinaryNotFoundForPackage(self.name.clone(), active_package.clone()))?;

        let path = match binary {
            Binary::Path {path, ..} => path,
            Binary::PythonEntryPoint {..} => {
                return Err(Error::Unsupported);
            },
        };

        println!("{}", path.to_file_string());

        Ok(())
    }
}
