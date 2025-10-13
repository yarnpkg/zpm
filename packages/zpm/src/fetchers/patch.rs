use zpm_formats::{iter_ext::IterExt, zip::ZipSupport};
use zpm_parsers::JsonDocument;
use zpm_primitives::{Ident, Locator, PatchReference};
use zpm_utils::Hash64;

use crate::{
    error::Error, install::{FetchResult, InstallContext, InstallOpResult}, manifest::Manifest, misc::unpack_brotli_data, npm::NpmEntryExt, patch::apply::apply_patch, resolvers::Resolution
};

use super::PackageData;

const BUILTIN_PATCHES: &[(&str, &[u8])] = &[
    ("fsevents", std::include_bytes!("../../patches/fsevents.brotli.dat")),
    ("resolve", std::include_bytes!("../../patches/resolve.brotli.dat")),
    ("typescript", std::include_bytes!("../../patches/typescript.brotli.dat")),
];

pub fn has_builtin_patch(ident: &Ident) -> bool {
    BUILTIN_PATCHES.iter()
        .any(|(name, _)| *name == ident.as_str())
}

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &PatchReference, dependencies: Vec<InstallOpResult>) -> Result<FetchResult, Error> {
    let project = context.project
        .expect("The project is required to fetch a patch package");

    let mut dependencies_it
        = dependencies.iter();

    let parent_data = locator.reference.must_bind()
        .then(|| dependencies_it.next().unwrap().as_fetched());

    let mut is_builtin = false;

    let patch_content = match params.path.as_str() {
        "<builtin>" => {
            let compressed_patch = BUILTIN_PATCHES.iter()
                .find(|(name, _)| name == &locator.ident.as_str())
                .unwrap()
                .1;

            is_builtin = true;

            unpack_brotli_data(compressed_patch)?
        },

        path if path.starts_with("~/") => {
            project.project_cwd
                .with_join_str(&path[2..])
                .fs_read_text_with_zip()?
        },

        path => {
            let parent_data
                = parent_data.expect("Expected parent data to be fetched when the patchfile is relative to the parent package");

            parent_data.package_data.context_directory()
                .with_join_str(path)
                .fs_read_text_with_zip()?
        },
    };

    let patch_checksum
        = Hash64::from_string(&patch_content);

    let reference = PatchReference {
        inner: params.inner.clone(),
        path: params.path.clone(),
        checksum: Some(patch_checksum),
    }.into();

    let locator
        = Locator::new_bound(locator.ident.clone(), reference, locator.parent.clone());

    let original_data
        = dependencies_it.next().unwrap().as_fetched();

    let package_cache = context.package_cache
        .expect("The package cache is required to fetch a patch package");

    let package_subdir
        = locator.ident.nm_subdir();

    let cached_blob = package_cache.upsert_blob(locator.clone(), ".zip", || async {
        let original_bytes = match &original_data.package_data {
            PackageData::Zip {archive_path, ..} => Some(archive_path.fs_read()?),
            _ => None,
        };

        let original_entries = match &original_data.package_data {
            PackageData::Local {package_directory, ..} => {
                zpm_formats::entries_from_folder(package_directory)?
            },

            PackageData::Zip {..} => {
                let package_subpath
                    = original_data.package_data.package_subpath();

                zpm_formats::zip::entries_from_zip(original_bytes.as_ref().unwrap())?
                    .into_iter()
                    .strip_path_prefix(&package_subpath)
                    .collect::<Vec<_>>()
            },

            PackageData::MissingZip {..} => {
                return Err(Error::Unsupported);
            },
        };

        let package_json_entry
            = original_entries
                // The cached files always have the package.json at the beginning of the archive
                .first()
                .ok_or(Error::MissingPackageManifest)?;

        let package_json_content: Manifest
            = JsonDocument::hydrate_from_slice(&package_json_entry.data)?;

        let package_version
            = package_json_content.remote.version
                .unwrap_or_default();

        let patched_entries = match is_builtin {
            true => {
                apply_patch(original_entries.clone(), &patch_content, &package_version)
                    .unwrap_or(original_entries)
            },

            false => {
                apply_patch(original_entries, &patch_content, &package_version)?
            },
        };

        let patched_entries = patched_entries
            .into_iter()
            .prepare_npm_entries(&package_subdir)
            .collect::<Vec<_>>();

        Ok(package_cache.bundle_entries(patched_entries)?)
    }).await?;

    // Find the root package.json (shortest path) from a list of entries.
    // This handles packages with nested package.json files (e.g., gl-matrix).
    let entries
        = zpm_formats::zip::entries_from_zip(&cached_blob.data)?;
    let package_json_entry
        =  entries
            .iter()
            .filter(|entry| entry.name.basename() == Some("package.json"))
            .min_by_key(|entry| entry.name.as_str().len())
            .ok_or(Error::MissingPackageManifest)?;

    let manifest: Manifest
        = JsonDocument::hydrate_from_slice(&package_json_entry.data)?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest.remote);

    let package_directory = cached_blob.info.path
        .with_join(&package_subdir);

    Ok(FetchResult {
        resolution: Some(resolution),
        package_data: PackageData::Zip {
            archive_path: cached_blob.info.path,
            checksum: cached_blob.info.checksum,
            context_directory: package_directory.clone(),
            package_directory,
        },
    })
}
