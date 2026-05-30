use zpm_parsers::JsonDocument;
use zpm_primitives::{ExecReference, Locator};
use zpm_utils::{FromFileString, Path, ToFileString};

use crate::{
    error::Error,
    install::{FetchResult, InstallContext, InstallOpResult},
    manifest::RemoteManifest,
    npm::NpmEntryExt,
    resolvers::Resolution,
    script::ScriptEnvironment,
};

use super::PackageData;

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &ExecReference, is_mock_request: bool, dependencies: Vec<InstallOpResult>) -> Result<FetchResult, Error> {
    let package_cache
        = context.package_cache
            .expect("The package cache is required for fetching exec packages");

    let cache_packer
        = package_cache.packer();

    if is_mock_request {
        let archive_path = package_cache
            .key_path(locator, ".zip");

        let package_directory = archive_path
            .with_join(&locator.ident.nm_subdir());

        return Ok(FetchResult::new_mock(archive_path, package_directory));
    }

    let script_relative_path
        = Path::from_file_string(&params.path)?;

    let parent_context_directory = dependencies.first()
        .ok_or(Error::Unsupported)?
        .as_fetched()
        .package_data
        .context_directory()
        .clone();

    let script_path = if script_relative_path.is_absolute() {
        script_relative_path
    } else {
        parent_context_directory.with_join_str(&params.path)
    };

    let package_subdir
        = locator.ident.nm_subdir();
    let package_subdir_for_entries
        = package_subdir.clone();
    let locator_str
        = locator.to_file_string();

    let pkg_blob = package_cache.refetch_blob_data(locator.clone(), ".zip", || async {
        let temp_dir
            = Path::temp_dir_pattern("exec-<>")?;
        let build_dir
            = temp_dir.with_join_str("build");
        build_dir.fs_create_dir_all()?;

        let wrapper_path
            = temp_dir.with_join_str("wrapper.cjs");
        wrapper_path.fs_change(make_wrapper(&temp_dir, &build_dir, &locator_str)?.as_bytes(), false)?;

        ScriptEnvironment::new()?
            .with_cwd(parent_context_directory.clone())
            .run_exec("node", [wrapper_path.to_file_string(), script_path.to_file_string()])
            .await?
            .ok()?;

        let archive = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, Error> {
            let entries
                = zpm_formats::entries_from_folder(&build_dir)?
                    .into_iter()
                    .prepare_npm_entries(&package_subdir_for_entries)?;

            cache_packer.pack(entries)
        }).await??;

        Ok(archive)
    }).await?;

    let first_entry
        = zpm_formats::zip::first_entry_from_zip(&pkg_blob.data)?;

    let remote_manifest: RemoteManifest
        = JsonDocument::hydrate_from_slice(&first_entry.data)?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), remote_manifest);

    let package_directory = pkg_blob.info.path
        .with_join(&package_subdir);

    Ok(FetchResult {
        resolution: Some(resolution),
        package_data: PackageData::Zip {
            archive_path: pkg_blob.info.path,
            checksum: pkg_blob.info.checksum,
            context_directory: package_directory.clone(),
            package_directory,
        },
    })
}

fn make_wrapper(temp_dir: &Path, build_dir: &Path, locator: &str) -> Result<String, Error> {
    let exec_env = serde_json::json!({
        "tempDir": temp_dir.to_file_string(),
        "buildDir": build_dir.to_file_string(),
        "locator": locator,
    });

    Ok(format!(r#"
const Module = require('module');

for (const name of Module.builtinModules) {{
  if (name === 'module' || name.startsWith('_'))
    continue;

  try {{
    globalThis[name] = require(name);
  }} catch {{
    globalThis[name] = undefined;
  }}
}}

globalThis.Module = Module;
globalThis.execEnv = {};

require(process.argv[2]);
"#, serde_json::to_string(&exec_env).unwrap()))
}
