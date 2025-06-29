use std::io::Read;

use zpm_formats::zip::ZipSupport;
use zpm_utils::ToHumanString;

use crate::{error::Error, hash::Sha256, install::{FetchResult, InstallContext, InstallOpResult}, manifest::Manifest, patch, primitives::{reference, Ident, Locator}, resolvers::Resolution};

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

pub async fn fetch_locator<'a>(context: &InstallContext<'a>, locator: &Locator, params: &reference::PatchReference, dependencies: Vec<InstallOpResult>) -> Result<FetchResult, Error> {
    let project = context.project
        .expect("The project is required to fetch a patch package");

    let mut dependencies_it
        = dependencies.iter();

    let patch_content = match params.path.as_str() {
        "<builtin>" => {
            let compressed_patch = BUILTIN_PATCHES.iter()
                .find(|(name, _)| name == &locator.ident.as_str())
                .unwrap()
                .1;

            let mut decompressor
                = brotli::Decompressor::new(compressed_patch, 4096);

            let mut decompressed_bytes = Vec::new();
            decompressor.read_to_end(&mut decompressed_bytes).unwrap();

            let decompressed_string
                = String::from_utf8(decompressed_bytes)?;

            decompressed_string
        },

        path if path.starts_with("~/") => {
            project.project_cwd
                .with_join_str(&path[2..])
                .fs_read_text_with_zip()?
        },

        path => {
            let parent_data
                = dependencies_it.next().unwrap().as_fetched();

            parent_data.package_data.context_directory()
                .with_join_str(path)
                .fs_read_text_with_zip()?
        },
    };

    let patch_checksum
        = Sha256::from_string(&patch_content);

    let reference = reference::PatchReference {
        inner: params.inner.clone(),
        path: params.path.clone(),
        checksum: Some(patch_checksum),
    }.into();

    let locator
        = Locator::new_bound(locator.ident.clone(), reference, locator.parent.clone());

    let original_data
        = dependencies_it.next().unwrap().as_fetched();

    let cached_blob = context.package_cache.unwrap().upsert_blob(locator.clone(), ".zip", || async {
        let original_bytes = match &original_data.package_data {
            PackageData::Zip {archive_path, ..} => Some(archive_path.fs_read()?),
            _ => None,
        };

        let original_entries = match &original_data.package_data {
            PackageData::Local {package_directory, ..} => {
                println!("package_directory: {}", package_directory.to_print_string());
                zpm_formats::entries_from_folder(package_directory)?
            },

            PackageData::Zip {..} => {
                let entries
                    = zpm_formats::zip::entries_from_zip(original_bytes.as_ref().unwrap())?;

                let package_subpath
                    = original_data.package_data.package_subpath();

                zpm_formats::strip_prefix(entries, package_subpath.as_str())
            },

            PackageData::MissingZip {..} => {
                return Err(Error::Unsupported);
            },
        };

        // I have to locate the package.json and extract its version to pass it as
        // parameter to patch::apply::apply_patch

        let package_json_entry
            = original_entries.iter()
                .find(|entry| entry.name == "package.json")
                .ok_or(Error::MissingPackageManifest)?;

        let package_json_content
            = sonic_rs::from_slice::<Manifest>(&package_json_entry.data)?;

        let package_version
            = package_json_content.remote.version.clone();

        if package_version.is_none() {
            println!("Package version is missing for {}", locator.ident.as_str());
        }

        let package_version
            = package_version.unwrap_or_default();

        let patched_entries
            = patch::apply::apply_patch(original_entries, &patch_content, &package_version)?;

        Ok(zpm_formats::convert::convert_entries_to_zip(&locator.ident.nm_subdir(), patched_entries)?)
    }).await?;

    let first_entry
        = zpm_formats::zip::first_entry_from_zip(&cached_blob.data)?;

    let manifest
        = sonic_rs::from_slice::<Manifest>(&first_entry.data)?;

    let resolution
        = Resolution::from_remote_manifest(locator.clone(), manifest.remote);

    let package_directory = cached_blob.info.path
        .with_join_str(locator.ident.nm_subdir());

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
