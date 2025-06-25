use std::{collections::BTreeMap, fmt, marker::PhantomData, str::FromStr, sync::{Arc, LazyLock}};

use regex::Regex;
use serde::{de::{self, DeserializeOwned, DeserializeSeed, IgnoredAny, Visitor}, Deserialize, Deserializer};
use zpm_utils::ToFileString;

use crate::{error::Error, http::http_get, install::{InstallContext, IntoResolutionResult, ResolutionResult}, manifest::RemoteManifest, npm, primitives::{range, reference, Descriptor, Ident, Locator, Reference}, resolvers::{workspace::{self, resolve_locator_ident}, Resolution}};

static NODE_GYP_IDENT: LazyLock<Ident> = LazyLock::new(|| Ident::from_str("node-gyp").unwrap());
static NODE_GYP_MATCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(node-gyp|prebuild-install)\b").unwrap());

/**
 * Deserializer that only deserializes the requested field and skips all others.
 */
pub struct FindFieldNested<'a, T> {
    field: &'a str,
    nested: T,
}

impl<'de, T> Visitor<'de> for FindFieldNested<'_, T> where T: DeserializeSeed<'de> + Clone {
    type Value = T::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "a map with a matching field")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error> where A: de::MapAccess<'de> {
        let mut selected = None;

        while let Some(key) = map.next_key::<String>()? {
            if key == self.field {
                selected = Some(map.next_value_seed(self.nested.clone())?);
            } else {
                let _ = map.next_value::<IgnoredAny>();
            }
        }

        selected
            .ok_or(de::Error::missing_field(""))
    }
}

/**
 * Deserializer that only deserializes the value for the highest key matching the provided semver range.
 */
#[derive(Clone)]
pub struct FindHighestCompatibleVersion<T> {
    range: zpm_semver::Range,
    phantom: PhantomData<T>,
}

impl<'de, T> DeserializeSeed<'de> for FindHighestCompatibleVersion<T> where T: DeserializeOwned {
    type Value = Option<(zpm_semver::Version, T)>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error> where D: Deserializer<'de> {
        deserializer.deserialize_map(self)
    }
}

impl<'de, T> Visitor<'de> for FindHighestCompatibleVersion<T> where T: DeserializeOwned {
    type Value = Option<(zpm_semver::Version, T)>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "a map with a matching version")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error> where A: de::MapAccess<'de> {
        let mut selected = None;

        while let Some(key) = map.next_key::<String>()? {
            let version
                = zpm_semver::Version::from_str(key.as_str()).unwrap();

            if self.range.check(&version) && selected.as_ref().map(|(current_version, _)| *current_version < version).unwrap_or(true) {
                selected = Some((version, map.next_value::<sonic_rs::Value>()?));
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }

        let Some((version, version_payload)) = selected else {
            return Ok(None);
        };

        let deserialized_payload
            = T::deserialize(&version_payload).unwrap();

        Ok(Some((version, deserialized_payload)))
    }
}

/**
 * Deserializer that only deserializes the value for the highest key matching the provided semver range.
 */
#[derive(Clone)]
pub struct FindField<'a, TVal> {
    value: &'a str,
    phantom: PhantomData<TVal>,
}

impl<'de, TVal> DeserializeSeed<'de> for FindField<'de, TVal> where TVal: Deserialize<'de> + std::fmt::Debug {
    type Value = Option<TVal>;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error> where D: Deserializer<'de> {
        deserializer.deserialize_map(self)
    }
}

impl<'de, TVal> Visitor<'de> for FindField<'de, TVal> where TVal: Deserialize<'de> + std::fmt::Debug {
    type Value = Option<TVal>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "a map with a matching version")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error> where A: de::MapAccess<'de> {
        let mut res = None;

        while let Some(key) = map.next_key::<String>()? {
            if self.value == key {
                res = Some(map.next_value::<TVal>()?);
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }

        Ok(res)
    }
}

/**
 * We need to read the scripts to figure out whether the package has an implicit node-gyp dependency.
 */
#[derive(Clone, Deserialize, Debug)]
struct RemoteManifestWithScripts {
    #[serde(flatten)]
    remote: RemoteManifest,

    #[serde(default)]
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    scripts: BTreeMap<String, String>,
}

fn fix_manifest(manifest: &mut RemoteManifestWithScripts) {
    // Manually add node-gyp dependency if there is a script using it and not already set
    // This is because the npm registry will automatically add a `node-gyp rebuild` install script
    // in the metadata if there is not already an install script and a binding.gyp file exists.
    // Also, node-gyp is not always set as a dependency in packages, so it will also be added if used in scripts.
    //
    if !manifest.remote.dependencies.contains_key(&NODE_GYP_IDENT) && !manifest.remote.peer_dependencies.contains_key(&NODE_GYP_IDENT) {
        for script in manifest.scripts.values() {
            if NODE_GYP_MATCH.is_match(script.as_str()) {
                manifest.remote.dependencies.insert(NODE_GYP_IDENT.clone(), Descriptor::new_semver(NODE_GYP_IDENT.clone(), "*").unwrap());
                break;
            }
        }
    }
}

fn build_resolution_result(context: &InstallContext, descriptor: &Descriptor, package_ident: &Ident, version: zpm_semver::Version, mut manifest: RemoteManifestWithScripts) -> ResolutionResult {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    fix_manifest(&mut manifest);

    let dist_manifest = manifest.remote.dist
        .as_ref()
        .expect("Expected the registry to return a 'dist' field amongst the manifest data");

    let registry_reference = reference::RegistryReference {
        ident: package_ident.clone(),
        version,
    };

    let expected_registry_url
        = npm::registry_url_for_package_data(&project.config.registry_base_for(&registry_reference.ident), &registry_reference.ident, &registry_reference.version);

    let locator = descriptor.resolve_with(match expected_registry_url == dist_manifest.tarball {
        true => registry_reference.into(),
        false => reference::UrlReference {url: dist_manifest.tarball.clone()}.into(),
    });

    Resolution::from_remote_manifest(locator, manifest.remote)
        .into_resolution_result(context)
}

pub async fn resolve_semver_or_workspace_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &range::RegistrySemverRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    if project.config.project.enable_transparent_workspaces.value {
        if let Some(resolved) = workspace::resolve_ident(context, &descriptor.ident) {
            if params.range.check(&resolved.resolution.version) {
                return Ok(resolved);
            }
        }
    }

    resolve_semver_descriptor(context, descriptor, params).await
}

pub async fn resolve_semver_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &range::RegistrySemverRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let package_ident = params.ident.as_ref()
        .unwrap_or(&descriptor.ident);

    let registry_url
        = npm::registry_url_for_all_versions(&project.config.registry_base_for(package_ident), package_ident);

    let response
        = http_get(&registry_url).await?;

    let registry_text = response.text().await
        .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

    let mut deserializer
        = sonic_rs::Deserializer::from_str(registry_text.as_str());

    let (version, manifest) = deserializer.deserialize_map(FindFieldNested {
        field: "versions",
        nested: FindHighestCompatibleVersion {
            range: params.range.clone(),
            phantom: PhantomData::<RemoteManifestWithScripts>,
        },
    })?.ok_or_else(|| {
        Error::NoCandidatesFound(descriptor.range.clone())
    })?;

    Ok(build_resolution_result(context, descriptor, package_ident, version, manifest))
}

pub async fn resolve_tag_or_workspace_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &range::RegistryTagRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    if project.config.project.enable_transparent_workspaces.value {
        if let Some(resolved) = workspace::resolve_ident(context, &descriptor.ident) {
            return Ok(resolved);
        }
    }

    resolve_tag_descriptor(context, descriptor, params).await
}

pub async fn resolve_tag_descriptor(context: &InstallContext<'_>, descriptor: &Descriptor, params: &range::RegistryTagRange) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let package_ident = params.ident.as_ref()
        .unwrap_or(&descriptor.ident);

    let registry_url
        = npm::registry_url_for_all_versions(&project.config.registry_base_for(package_ident), package_ident);

    let response
        = http_get(&registry_url).await?;

    let registry_text = response.text().await
        .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

    #[derive(Deserialize)]
    struct RegistryMetadata {
        #[serde(rename(deserialize = "dist-tags"))]
        dist_tags: sonic_rs::Value,
        versions: sonic_rs::Value,
    }

    let registry_data: RegistryMetadata = sonic_rs::from_str(registry_text.as_str())
        .map_err(Arc::new)?;

    let version = registry_data.dist_tags.deserialize_map(FindField {
        value: params.tag.as_str(),
        phantom: PhantomData::<zpm_semver::Version>,
    })?.ok_or_else(|| {
        Error::TagNotFound(params.tag.clone())
    })?;

    let manifest = registry_data.versions.deserialize_map(FindField {
        value: &version.to_file_string(),
        phantom: PhantomData::<RemoteManifestWithScripts>,
    })?.ok_or_else(|| {
        Error::NoCandidatesFound(descriptor.range.clone())
    })?;

    Ok(build_resolution_result(context, descriptor, package_ident, version, manifest))
}

pub async fn resolve_locator(context: &InstallContext<'_>, locator: &Locator, params: &reference::RegistryReference) -> Result<ResolutionResult, Error> {
    let project = context.project
        .expect("The project is required for resolving a workspace package");

    let registry_url
        = npm::registry_url_for_one_version(&project.config.registry_base_for(&params.ident), &params.ident, &params.version);

    let response
        = http_get(&registry_url).await?;

    let registry_text = response.text().await
        .map_err(|err| Error::RemoteRegistryError(Arc::new(err)))?;

    let mut manifest: RemoteManifestWithScripts
        = sonic_rs::from_str(registry_text.as_str())?;

    fix_manifest(&mut manifest);

    let resolution = Resolution::from_remote_manifest(locator.clone(), manifest.remote);

    Ok(resolution.into_resolution_result(context))
}
