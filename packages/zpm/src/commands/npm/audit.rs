use std::collections::{BTreeMap, BTreeSet, HashMap};

use clipanion::cli;
use globset::GlobBuilder;
use indexmap::IndexMap;
use serde::Deserialize;
use zpm_parsers::JsonDocument;
use zpm_primitives::{Ident, Locator, Reference};
use zpm_semver::Range;
use zpm_utils::{AbstractValue, FromFileString, RawString, ToFileString, tree};

use crate::{
    error::Error,
    http_npm::{self, AuthorizationMode, GetAuthorizationOptions, NpmHttpParams, get_authorization, get_registry},
    project::{Project, Workspace},
};

/// Perform a vulnerability audit against the installed packages.
///
/// This command checks for known security reports on the packages you use. The reports are by default extracted from
/// the npm registry, and may or may not be relevant to your actual program (not all vulnerabilities affect all code paths).
///
/// For consistency with our other commands the default is to only check the direct dependencies for the active workspace.
/// To extend this search to all workspaces, use `-A,--all`. To extend this search to both direct and transitive dependencies,
/// use `-R,--recursive`.
///
/// Applying the `--severity` flag will limit the audit table to vulnerabilities of the corresponding severity and above.
/// Valid values are `info`, `low`, `moderate`, `high`, and `critical`.
///
/// If the `--json` flag is set, Yarn will print the output exactly as received from the registry. Regardless of this flag,
/// the process will exit with a non-zero exit code if a report is found for the selected packages.
///
/// If certain packages produce false positives for a particular environment, the `--exclude` flag can be used to exclude
/// any number of packages from the audit. This can also be set in the configuration file with the `npmAuditExcludePackages` option.
///
/// If particular advisories are needed to be ignored, the `--ignore` flag can be used with Advisory ID's to ignore any number
/// of advisories in the audit report. This can also be set in the configuration file with the `npmAuditIgnoreAdvisories` option.
///
/// To understand the dependency tree requiring vulnerable packages, check the raw report with the `--json` flag or use
/// `yarn why <package>` to get more information as to who depends on them.
///
#[cli::command]
#[cli::path("npm", "audit")]
#[cli::category("Npm-related commands")]
pub struct Audit {
    /// Audit dependencies from all workspaces
    #[cli::option("-A,--all", default = false)]
    all: bool,

    /// Audit transitive dependencies as well
    #[cli::option("-R,--recursive", default = false)]
    recursive: bool,

    /// Which environments to cover (all, production, development)
    #[cli::option("--environment", default = "all".to_string())]
    environment: String,

    /// Format the output as an NDJSON stream
    #[cli::option("--json", default = false)]
    json: bool,

    /// Don't warn about deprecated packages
    #[cli::option("--no-deprecations", default = false)]
    no_deprecations: bool,

    /// Minimal severity requested for packages to be displayed (info, low, moderate, high, critical)
    #[cli::option("--severity", default = "info".to_string())]
    severity: String,

    /// Array of glob patterns of packages to exclude from audit
    #[cli::option("--exclude", default = Vec::new())]
    excludes: Vec<String>,

    /// Array of glob patterns of advisory ID's to ignore in the audit report
    #[cli::option("--ignore", default = Vec::new())]
    ignores: Vec<String>,
}

const ALL_SEVERITIES: &[&str] = &["info", "low", "moderate", "high", "critical"];

fn get_severity_inclusions(severity: &str) -> BTreeSet<String> {
    let index = ALL_SEVERITIES.iter().position(|&s| s == severity).unwrap_or(0);
    ALL_SEVERITIES[index..].iter().map(|s| s.to_string()).collect()
}

fn is_npm_package(locator: &Locator) -> bool {
    matches!(&locator.reference, Reference::Shorthand { .. } | Reference::Registry { .. })
}

fn get_npm_version(locator: &Locator) -> Option<zpm_semver::Version> {
    match &locator.reference {
        Reference::Shorthand(params) => Some(params.version.clone()),
        Reference::Registry(params) => Some(params.version.clone()),
        _ => None,
    }
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .ok()
        .and_then(|glob| glob.compile_matcher().is_match(value).then_some(()))
        .is_some()
}

#[derive(Debug, Deserialize)]
struct AuditAdvisory {
    id: serde_json::Value,
    url: Option<String>,
    title: String,
    severity: String,
    vulnerable_versions: String,
}

struct AuditExtendedAdvisory {
    id: String,
    url: Option<String>,
    title: String,
    severity: String,
    vulnerable_versions: String,
    versions: Vec<zpm_semver::Version>,
    dependents: Vec<Locator>,
}

impl Audit {
    pub async fn execute(&self) -> Result<(), Error> {
        if !ALL_SEVERITIES.contains(&self.severity.as_str()) {
            return Err(Error::ConflictingOptions(format!(
                "Invalid severity '{}'; expected one of: {}",
                self.severity,
                ALL_SEVERITIES.join(", ")
            )));
        }

        if !["all", "production", "development"].contains(&self.environment.as_str()) {
            return Err(Error::ConflictingOptions(format!(
                "Invalid environment '{}'; expected one of: all, production, development",
                self.environment,
            )));
        }

        let mut project
            = Project::new(None).await?;

        project.lazy_install().await?;

        let packages
            = self.collect_packages(&project)?;

        let excluded_packages = {
            let mut excluded: Vec<String>
                = project.config.settings.npm_audit_exclude_packages.iter()
                    .map(|s| s.value.clone())
                    .collect();
            excluded.extend(self.excludes.clone());
            excluded
        };

        let mut payload: BTreeMap<String, Vec<String>>
            = BTreeMap::new();

        for (package_name, versions) in &packages {
            if excluded_packages.iter().any(|pattern| glob_matches(pattern, package_name)) {
                continue;
            }

            payload.insert(
                package_name.clone(),
                versions.keys().map(|v| v.to_file_string()).collect(),
            );
        }

        if payload.is_empty() {
            if !self.json {
                println!("No audit suggestions");
            }
            return Ok(());
        }

        let registry = project.config.settings.npm_audit_registry.value.as_deref()
            .map(|s| s.strip_suffix('/').unwrap_or(s))
            .unwrap_or_else(|| get_registry(&project.config, None, false).unwrap());

        let authorization
            = get_authorization(&GetAuthorizationOptions {
                configuration: &project.config,
                http_client: &project.http_client,
                registry,
                ident: None,
                auth_mode: AuthorizationMode::BestEffort,
                allow_oidc: false,
            }).await?;

        let payload_json
            = serde_json::to_string(&payload)
                .map_err(|e| Error::SerializationError(e.to_string()))?;

        let audit_response
            = http_npm::post(&NpmHttpParams {
                http_client: &project.http_client,
                registry,
                path: "/-/npm/v1/security/advisories/bulk",
                authorization: authorization.as_deref(),
                otp: None,
            }, payload_json).await?;

        let audit_body
            = audit_response.text().await
                .map_err(|e| Error::HttpError { inner: std::sync::Arc::new(e), extra: None })?;

        let mut result: HashMap<String, Vec<AuditAdvisory>>
            = JsonDocument::hydrate_from_str(&audit_body)?;

        if !self.no_deprecations {
            self.check_deprecations(&project, &payload, &mut result).await?;
        }

        let severities
            = get_severity_inclusions(&self.severity);

        let ignored_advisories = {
            let mut ignored: Vec<String>
                = project.config.settings.npm_audit_ignore_advisories.iter()
                    .map(|s| s.value.clone())
                    .collect();
            ignored.extend(self.ignores.clone());
            ignored
        };

        let mut expanded_result: BTreeMap<String, Vec<AuditExtendedAdvisory>>
            = BTreeMap::new();

        for (package_name, advisories) in &result {
            let filtered: Vec<&AuditAdvisory> = advisories.iter()
                .filter(|advisory| {
                    let id_str = match &advisory.id {
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::String(s) => s.clone(),
                        _ => format!("{}", advisory.id),
                    };

                    let is_ignored = ignored_advisories.iter()
                        .any(|pattern| glob_matches(pattern, &id_str));

                    !is_ignored && severities.contains(&advisory.severity)
                })
                .collect();

            if filtered.is_empty() {
                continue;
            }

            let package_versions = match packages.get(package_name) {
                Some(versions) => versions,
                None => continue,
            };

            let extended: Vec<AuditExtendedAdvisory> = filtered.iter().map(|advisory| {
                let vulnerable_range
                    = Range::from_file_string(&advisory.vulnerable_versions).ok();

                let affected_versions: Vec<zpm_semver::Version> = package_versions.keys()
                    .filter(|version| {
                        vulnerable_range.as_ref()
                            .map(|range| range.check(version))
                            .unwrap_or(false)
                    })
                    .cloned()
                    .collect();

                let mut dependents_map: BTreeMap<String, Locator>
                    = BTreeMap::new();

                for version in &affected_versions {
                    if let Some(locators) = package_versions.get(version) {
                        for locator in locators {
                            dependents_map.insert(locator.to_file_string(), locator.clone());
                        }
                    }
                }

                let id_str = match &advisory.id {
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::String(s) => s.clone(),
                    _ => format!("{}", advisory.id),
                };

                AuditExtendedAdvisory {
                    id: id_str,
                    url: advisory.url.clone(),
                    title: advisory.title.clone(),
                    severity: advisory.severity.clone(),
                    vulnerable_versions: advisory.vulnerable_versions.clone(),
                    versions: affected_versions,
                    dependents: dependents_map.into_values().collect(),
                }
            }).collect();

            if !extended.is_empty() {
                expanded_result.insert(package_name.clone(), extended);
            }
        }

        let has_vulnerabilities
            = !expanded_result.is_empty();

        if has_vulnerabilities {
            let root_node
                = Self::build_report_tree(&expanded_result);

            let rendering
                = tree::TreeRenderer::new()
                    .render(&root_node, self.json);

            print!("{}", rendering);

            return Err(Error::SilentError);
        }

        if !self.json {
            println!("No audit suggestions");
        }

        Ok(())
    }

    fn collect_packages(&self, project: &Project) -> Result<BTreeMap<String, BTreeMap<zpm_semver::Version, Vec<Locator>>>, Error> {
        let install_state = project.install_state.as_ref()
            .ok_or(Error::InstallStateNotFound)?;

        let workspaces: Vec<&Workspace> = if self.all {
            project.workspaces.iter().collect()
        } else {
            let active = project.active_workspace()?;
            vec![active]
        };

        let include_dependencies
            = self.environment == "all" || self.environment == "production";
        let include_dev_dependencies
            = self.environment == "all" || self.environment == "development";

        let mut top_level_descriptors
            = Vec::new();

        for workspace in &workspaces {
            let workspace_locator
                = workspace.locator();

            let resolution = install_state.resolution_tree.locator_resolutions
                .get(&workspace_locator);

            let Some(resolution) = resolution else {
                continue;
            };

            for (ident, descriptor) in &resolution.dependencies {
                let is_dev_dependency
                    = workspace.manifest.dev_dependencies.contains_key(ident);

                if is_dev_dependency && !include_dev_dependencies {
                    continue;
                }

                if !is_dev_dependency && !include_dependencies {
                    continue;
                }

                if let Some(locator) = install_state.resolution_tree.descriptor_to_locator.get(descriptor) {
                    top_level_descriptors.push((workspace_locator.clone(), locator.clone()));
                }
            }
        }

        let mut packages: BTreeMap<String, BTreeMap<zpm_semver::Version, Vec<Locator>>>
            = BTreeMap::new();
        let mut visited
            = BTreeSet::new();
        let mut queue: Vec<(Locator, Locator)>
            = top_level_descriptors;

        while let Some((parent, locator)) = queue.pop() {
            let physical_locator
                = locator.physical_locator();

            if !visited.insert(physical_locator.to_file_string()) {
                continue;
            }

            if is_npm_package(&physical_locator) {
                if let Some(version) = get_npm_version(&physical_locator) {
                    let package_name
                        = physical_locator.ident.to_file_string();

                    packages.entry(package_name)
                        .or_default()
                        .entry(version)
                        .or_default()
                        .push(parent.clone());
                }
            }

            if self.recursive {
                if let Some(resolution) = install_state.resolution_tree.locator_resolutions.get(&locator) {
                    for descriptor in resolution.dependencies.values() {
                        if let Some(dep_locator) = install_state.resolution_tree.descriptor_to_locator.get(descriptor) {
                            queue.push((locator.clone(), dep_locator.clone()));
                        }
                    }
                }
            }
        }

        Ok(packages)
    }

    async fn check_deprecations(
        &self,
        project: &Project,
        payload: &BTreeMap<String, Vec<String>>,
        result: &mut HashMap<String, Vec<AuditAdvisory>>,
    ) -> Result<(), Error> {
        let registry
            = get_registry(&project.config, None, false)?;

        let authorization
            = get_authorization(&GetAuthorizationOptions {
                configuration: &project.config,
                http_client: &project.http_client,
                registry,
                ident: None,
                auth_mode: AuthorizationMode::BestEffort,
                allow_oidc: false,
            }).await?;

        for (package_name, versions) in payload {
            let ident = match Ident::from_file_string(package_name) {
                Ok(ident) => ident,
                Err(_) => continue,
            };

            let (scope, name) = ident.split();
            let encoded_name = if let Some(scope) = scope {
                format!("{}%2f{}", scope, name)
            } else {
                name.to_string()
            };

            let metadata_response = http_npm::get(&NpmHttpParams {
                http_client: &project.http_client,
                registry,
                path: &format!("/{}", encoded_name),
                authorization: authorization.as_deref(),
                otp: None,
            }).await;

            let metadata_bytes = match metadata_response {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };

            #[derive(Deserialize)]
            struct VersionMetadata {
                deprecated: Option<serde_json::Value>,
            }

            #[derive(Deserialize)]
            struct PackageMetadata {
                #[serde(default)]
                versions: HashMap<String, VersionMetadata>,
            }

            let metadata: PackageMetadata = match JsonDocument::hydrate_from_slice(&metadata_bytes) {
                Ok(m) => m,
                Err(_) => continue,
            };

            for version_str in versions {
                let version_meta = match metadata.versions.get(version_str) {
                    Some(meta) => meta,
                    None => continue,
                };

                let deprecated_message = match &version_meta.deprecated {
                    Some(serde_json::Value::String(s)) if !s.is_empty() => s.clone(),
                    Some(serde_json::Value::Bool(true)) => "This package has been deprecated.".to_string(),
                    _ => continue,
                };

                let already_vulnerable = result.get(package_name)
                    .map(|advisories| {
                        advisories.iter().any(|advisory| {
                            Range::from_file_string(&advisory.vulnerable_versions).ok()
                                .and_then(|range| {
                                    zpm_semver::Version::from_file_string(version_str).ok()
                                        .map(|v| range.check(&v))
                                })
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false);

                if already_vulnerable {
                    continue;
                }

                result.entry(package_name.clone())
                    .or_default()
                    .push(AuditAdvisory {
                        id: serde_json::Value::String(format!("{} (deprecation)", package_name)),
                        title: deprecated_message,
                        severity: "moderate".to_string(),
                        vulnerable_versions: version_str.clone(),
                        url: None,
                    });
            }
        }

        Ok(())
    }

    fn build_report_tree(result: &BTreeMap<String, Vec<AuditExtendedAdvisory>>) -> tree::Node<'_> {
        let mut root_children
            = vec![];

        for (package_name, advisories) in result {
            for advisory in advisories {
                let mut children
                    = IndexMap::new();

                children.insert("ID".to_string(), tree::Node {
                    label: Some("ID".to_string()),
                    value: Some(AbstractValue::new(RawString::new(advisory.id.clone()))),
                    children: None,
                });

                children.insert("Issue".to_string(), tree::Node {
                    label: Some("Issue".to_string()),
                    value: Some(AbstractValue::new(RawString::new(advisory.title.clone()))),
                    children: None,
                });

                if let Some(url) = &advisory.url {
                    children.insert("URL".to_string(), tree::Node {
                        label: Some("URL".to_string()),
                        value: Some(AbstractValue::new(RawString::new(url.clone()))),
                        children: None,
                    });
                }

                children.insert("Severity".to_string(), tree::Node {
                    label: Some("Severity".to_string()),
                    value: Some(AbstractValue::new(RawString::new(advisory.severity.clone()))),
                    children: None,
                });

                children.insert("Vulnerable Versions".to_string(), tree::Node {
                    label: Some("Vulnerable Versions".to_string()),
                    value: Some(AbstractValue::new(RawString::new(advisory.vulnerable_versions.clone()))),
                    children: None,
                });

                if !advisory.versions.is_empty() {
                    let version_nodes: Vec<tree::Node<'_>> = advisory.versions.iter()
                        .map(|v| tree::Node::new_value(v.clone()))
                        .collect();

                    children.insert("Tree Versions".to_string(), tree::Node {
                        label: Some("Tree Versions".to_string()),
                        value: None,
                        children: Some(tree::TreeNodeChildren::Vec(version_nodes)),
                    });
                }

                if !advisory.dependents.is_empty() {
                    let dependent_nodes: Vec<tree::Node<'_>> = advisory.dependents.iter()
                        .map(|l| tree::Node::new_value(l.clone()))
                        .collect();

                    children.insert("Dependents".to_string(), tree::Node {
                        label: Some("Dependents".to_string()),
                        value: None,
                        children: Some(tree::TreeNodeChildren::Vec(dependent_nodes)),
                    });
                }

                let ident = Ident::from_file_string(package_name).ok();

                root_children.push(tree::Node {
                    label: None,
                    value: Some(match ident {
                        Some(ident) => AbstractValue::new(ident),
                        None => AbstractValue::new(RawString::new(package_name.clone())),
                    }),
                    children: Some(tree::TreeNodeChildren::Map(children)),
                });
            }
        }

        tree::Node {
            label: None,
            value: None,
            children: Some(tree::TreeNodeChildren::Vec(root_children)),
        }
    }
}
