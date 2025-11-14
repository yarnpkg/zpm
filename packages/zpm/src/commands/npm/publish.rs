use std::{collections::BTreeMap, env::VarError};

use clipanion::cli;
use serde::Serialize;
use zpm_macro_enum::zpm_enum;
use zpm_parsers::{JsonDocument, json_provider::json};
use zpm_utils::{IoResultExt, Provider, Sha1, Sha512, ToFileString, is_ci};

use crate::{
    error::Error, http::HttpClient, http_npm::{self, AuthorizationMode, NpmHttpParams}, pack::{PackOptions, pack_workspace}, project::Project, provenance::attest, script::ScriptEnvironment
};

#[zpm_enum(or_else = |s| Err(Error::InvalidNpmPublishAccess(s.to_string())))]
#[derive(Debug)]
#[derive_variants(Debug)]
enum NpmPublishAccess {
    #[literal("public")]
    Public,

    #[literal("restricted")]
    Restricted,
}

/// Print the username associated with the current authentication settings to the standard output.
///
/// When using `-s,--scope`, the username printed will be the one that matches the authentication settings of the registry associated with the given scope (those settings can be overriden using the `npmRegistries` map, and the registry associated with the scope is configured via the `npmScopes` map).
///
/// When using `--publish`, the registry we'll select will by default be the one used when publishing packages (`publishConfig.registry` or `npmPublishRegistry` if available, otherwise we'll fallback to the regular `npmRegistryServer`).
///
#[cli::command]
#[cli::path("npm", "publish")]
#[cli::category("Npm-related commands")]
pub struct Publish {
    /// The access for the published package (public or restricted)
    #[cli::option("-a,--access", default = NpmPublishAccess::Public)]
    access: NpmPublishAccess,

    /// The tag on the registry that the package should be attached to
    #[cli::option("--tag", default = "latest".to_string())]
    tag: String,

    /// Warn and exit when republishing an already existing version of a package
    #[cli::option("--tolerate-republish", default = false)]
    tolerate_republish: bool,

    /// The OTP token to use with the command
    #[cli::option("--otp")]
    otp: Option<String>,

    /// Generate provenance for the package
    #[cli::option("--provenance", default = false)]
    provenance: bool,

    /// Show what would be published without actually publishing
    #[cli::option("--dry-run", default = false)]
    dry_run: bool,

    /// Output the result in JSON format
    #[cli::option("--json", default = false)]
    json: bool,
}

impl Publish {
    pub async fn execute(&self) -> Result<(), Error> {
        let mut project
            = Project::new(None).await?;

        let published_workspace
            = project.active_workspace()?;

        if published_workspace.manifest.private == Some(true) {
            return Err(Error::CannotPublishPrivatePackage);
        }

        let published_workspace_locator
            = published_workspace.locator();

        let pack_result
            = pack_workspace(&mut project, &published_workspace_locator, &PackOptions {
                preserve_workspaces: false,
            }).await?;

        let published_workspace
            = project.workspace_by_locator(&published_workspace_locator)?;

        let (Some(name), Some(version)) = (pack_result.pack_manifest.name.as_ref(), pack_result.pack_manifest.remote.version.as_ref()) else {
            return Err(Error::CannotPublishMissingNameOrVersion);
        };

        let registry
            = http_npm::get_registry(&project.config, name.scope(), true)?;

        let authorization
            = http_npm::get_authorization(&http_npm::GetAuthorizationOptions {
                configuration: &project.config,
                http_client: &project.http_client,
                registry: &registry,
                ident: Some(name),
                auth_mode: AuthorizationMode::AlwaysAuthenticate,
                allow_oidc: true,
            }).await?;

        let sha1_digest
            = Sha1::new(&pack_result.pack_file).to_hex();
        let sha512_digest
            = Sha512::new(&pack_result.pack_file).to_hex();

        let tarball_name
            = format!("{}-{}.tgz", name.to_file_string(), version.to_file_string());

        let mut attachments = BTreeMap::from_iter([
            (tarball_name.clone(), AttachmentInfo::from_raw("application/octet-stream".to_string(), &pack_result.pack_file)),
        ]);

        let provenance
            = pack_result.pack_manifest.publish_config.provenance
                .unwrap_or(self.provenance);

        if provenance {
            let provenance_digest = ProvenanceDigest {
                sha512: sha512_digest.clone(),
            };

            let provenance_file = ProvenanceSubject {
                // Adapted from https://github.com/npm/npm-package-arg/blob/fbbf22ef99ece449428fee761ae8950c08bc2cbf/lib/npa.js#L118
                name: format!("pkg:npm/{}@{}", name.to_file_string().replace("@", "%40"), version.to_file_string()),
                digest: provenance_digest,
            };

            let oidc_token
                = authorization.as_deref()
                    .ok_or(Error::ProvenanceRequiresAuthentication)?
                    .strip_prefix("Bearer ")
                    .ok_or(Error::ProvenanceRequiresAuthentication)?;

            let provenance_payload
                = create_provenance_payload(&project.http_client, &provenance_file, &oidc_token).await?;

            if let Some(provenance_payload) = provenance_payload {
                attachments.insert(
                    format!("{}-{}.sigstore", name.to_file_string(), version.to_file_string()),
                    AttachmentInfo::from_str("application/json".to_string(), &provenance_payload),
                );
            }
        }

        let git_head
            = ScriptEnvironment::new()?
                .with_cwd(published_workspace.path.clone())
                .run_exec("git", &["rev-parse", "HEAD"])
                .await
                .ok()
                .map(|r| r.stdout_text())
                .transpose()?;

        let readme
            = published_workspace.path.with_join_str("README.md")
                .fs_read_text()
                .ok_missing()?
                .unwrap_or_else(|| format!("# {}\n", name.to_file_string()));

        // While the npm registry ignores the provided tarball URL, it's used by
        // other registries such as verdaccio.
        let tarball_url
            = format!("{}/{}/-/{}", project.config.settings.npm_registry_server.value, name.to_file_string(), tarball_name);

        let version_string
            = version.to_file_string();

        let publish_body = json!({
            "_id": name,
            "name": name,
            "access": self.access,
            "attachments": attachments,
            "dist-tags": {
                self.tag: version,
            },
            "versions": {
                version_string: {
                    "_id": format!("{}@{}", name.to_file_string(), version_string),
                    "name": name,
                    "version": version,
                    "git_head": git_head,
                    "dist": {
                        "shasum": sha1_digest,
                        "integrity": sha512_digest,

                        // the npm registry requires a tarball path, but it seems useless 🤷
                        "tarball": tarball_url,
                    },
                },
            },
            "readme": readme,
        }).to_string();

        http_npm::put(&NpmHttpParams {
            http_client: &project.http_client,
            registry: &registry,
            path: &format!("/{}/-/{}/{}", name.to_file_string(), version_string, tarball_name),
            authorization: authorization.as_deref(),
        }, publish_body).await?;

        Ok(())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProvenanceDigest {
    sha512: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProvenanceSubject {
    name: String,
    digest: ProvenanceDigest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentInfo {
    #[serde(rename = "content_type")]
    content_type: String,
    data: String,
    length: u64,
}

impl AttachmentInfo {
    pub fn from_str(content_type: String, data: &str) -> Self {
        Self {
            content_type,
            data: data.to_string(),
            length: data.len() as u64,
        }
    }

    pub fn from_raw(content_type: String, data: &[u8]) -> Self {
        let encoded
            = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, data);

        Self {
            content_type,
            data: encoded,
            length: data.len() as u64,
        }
    }
}

const INTOTO_PAYLOAD_TYPE: &str = "application/vnd.in-toto+json";
const INTOTO_STATEMENT_V01_TYPE: &str = "https://in-toto.io/Statement/v0.1";
const INTOTO_STATEMENT_V1_TYPE: &str = "https://in-toto.io/Statement/v1";

const SLSA_PREDICATE_V02_TYPE: &str = "https://slsa.dev/provenance/v0.2";
const SLSA_PREDICATE_V1_TYPE: &str = "https://slsa.dev/provenance/v1";

async fn create_provenance_payload(http_client: &HttpClient, subject: &ProvenanceSubject, oidc_token: &str) -> Result<Option<String>, Error> {
    let Some(provider) = is_ci() else {
        return Ok(None);
    };

    let payload = match provider {
        Provider::GitHubActions
            => Some(create_github_provenance_payload(subject)),

        Provider::GitLab
            => Some(create_gitlab_provenance_payload(subject)),

        Provider::Unknown
            => None,
    };

    let Some(payload) = payload else {
        return Ok(None);
    };

    let payload = payload
        .map_err(|e| Error::MissingEnvironmentVariableForProvenancePayload(e.to_string()))?;

    Ok(Some(JsonDocument::to_string(&attest(http_client, &payload, INTOTO_PAYLOAD_TYPE, oidc_token).await?)?))
}

const GITHUB_BUILDER_ID_PREFIX: &str = "https://github.com/actions/runner";
const GITHUB_BUILD_TYPE: &str = "https://slsa-framework.github.io/github-actions-buildtypes/workflow/v1";
const GITLAB_BUILD_TYPE_PREFIX: &str = "https://github.com/npm/cli/gitlab";
const GITLAB_BUILD_TYPE_VERSION: &str = "v0alpha1";

fn create_github_provenance_payload(subject: &ProvenanceSubject) -> Result<String, VarError> {
    let github_repository
        = std::env::var("GITHUB_REPOSITORY")?;

    let github_workflow_ref
        = std::env::var("GITHUB_WORKFLOW_REF")?
            .replace(format!("{}/", github_repository).as_str(), "");

    let github_server_url
        = std::env::var("GITHUB_SERVER_URL")?;

    let (workflow_path, workflow_ref)
        = github_workflow_ref.split_once('/')
            .unwrap_or_else(|| panic!("Expected workflow path and ref to both exist (got '{}' instead)", github_workflow_ref));

    let workflow_repository
        = format!("{}/{}", github_server_url, github_repository);

    let value = json!({
        "_type": INTOTO_STATEMENT_V1_TYPE,
        "subject": [subject],
        "predicate_type": SLSA_PREDICATE_V1_TYPE,
        "predicate": {
            "buildDefinition": {
                "buildType": GITHUB_BUILD_TYPE,
                "externalParameters": {
                    "workflow": {
                        "ref": workflow_ref,
                        "repository": workflow_repository,
                        "path": workflow_path,
                    },
                },
                "internalParameters": {
                    "github": {
                        "event_name": std::env::var("GITHUB_EVENT_NAME")?,
                        "repository_id": std::env::var("GITHUB_REPOSITORY_ID")?,
                        "repository_owner": std::env::var("GITHUB_REPOSITORY_OWNER")?,
                    },
                },
                "resolvedDependencies": [{
                    "uri": format!("git+{}/{}@{}", std::env::var("GITHUB_SERVER_URL")?, std::env::var("GITHUB_REPOSITORY")?, std::env::var("GITHUB_REF")?),
                    "digest": {
                        "gitCommit": std::env::var("GITHUB_SHA")?,
                    },
                }],
            },
            "runDetails": {
                "builder": {
                    "id": format!("{}{}", GITHUB_BUILDER_ID_PREFIX, std::env::var("RUNNER_ENVIRONMENT")?),
                },
                "metadata": {
                    "invocationId": format!("{}/{}/actions/runs/{}/attempts/{}", std::env::var("GITHUB_SERVER_URL")?, std::env::var("GITHUB_REPOSITORY")?, std::env::var("GITHUB_RUN_ID")?, std::env::var("GITHUB_RUN_ATTEMPT")?),
                },
            },
        },
    });

    Ok(value.to_string())
}

const GITLAB_CI_PARAMETERS: &[&str] = &[
    "CI",
    "CI_API_GRAPHQL_URL",
    "CI_API_V4_URL",
    "CI_BUILD_BEFORE_SHA",
    "CI_BUILD_ID",
    "CI_BUILD_NAME",
    "CI_BUILD_REF",
    "CI_BUILD_REF_NAME",
    "CI_BUILD_REF_SLUG",
    "CI_BUILD_STAGE",
    "CI_COMMIT_BEFORE_SHA",
    "CI_COMMIT_BRANCH",
    "CI_COMMIT_REF_NAME",
    "CI_COMMIT_REF_PROTECTED",
    "CI_COMMIT_REF_SLUG",
    "CI_COMMIT_SHA",
    "CI_COMMIT_SHORT_SHA",
    "CI_COMMIT_TIMESTAMP",
    "CI_COMMIT_TITLE",
    "CI_CONFIG_PATH",
    "CI_DEFAULT_BRANCH",
    "CI_DEPENDENCY_PROXY_DIRECT_GROUP_IMAGE_PREFIX",
    "CI_DEPENDENCY_PROXY_GROUP_IMAGE_PREFIX",
    "CI_DEPENDENCY_PROXY_SERVER",
    "CI_DEPENDENCY_PROXY_USER",
    "CI_JOB_ID",
    "CI_JOB_NAME",
    "CI_JOB_NAME_SLUG",
    "CI_JOB_STAGE",
    "CI_JOB_STARTED_AT",
    "CI_JOB_URL",
    "CI_NODE_TOTAL",
    "CI_PAGES_DOMAIN",
    "CI_PAGES_URL",
    "CI_PIPELINE_CREATED_AT",
    "CI_PIPELINE_ID",
    "CI_PIPELINE_IID",
    "CI_PIPELINE_SOURCE",
    "CI_PIPELINE_URL",
    "CI_PROJECT_CLASSIFICATION_LABEL",
    "CI_PROJECT_DESCRIPTION",
    "CI_PROJECT_ID",
    "CI_PROJECT_NAME",
    "CI_PROJECT_NAMESPACE",
    "CI_PROJECT_NAMESPACE_ID",
    "CI_PROJECT_PATH",
    "CI_PROJECT_PATH_SLUG",
    "CI_PROJECT_REPOSITORY_LANGUAGES",
    "CI_PROJECT_ROOT_NAMESPACE",
    "CI_PROJECT_TITLE",
    "CI_PROJECT_URL",
    "CI_PROJECT_VISIBILITY",
    "CI_REGISTRY",
    "CI_REGISTRY_IMAGE",
    "CI_REGISTRY_USER",
    "CI_RUNNER_DESCRIPTION",
    "CI_RUNNER_ID",
    "CI_RUNNER_TAGS",
    "CI_SERVER_HOST",
    "CI_SERVER_NAME",
    "CI_SERVER_PORT",
    "CI_SERVER_PROTOCOL",
    "CI_SERVER_REVISION",
    "CI_SERVER_SHELL_SSH_HOST",
    "CI_SERVER_SHELL_SSH_PORT",
    "CI_SERVER_URL",
    "CI_SERVER_VERSION",
    "CI_SERVER_VERSION_MAJOR",
    "CI_SERVER_VERSION_MINOR",
    "CI_SERVER_VERSION_PATCH",
    "CI_TEMPLATE_REGISTRY_HOST",
    "GITLAB_CI",
    "GITLAB_FEATURES",
    "GITLAB_USER_ID",
    "GITLAB_USER_LOGIN",
    "RUNNER_GENERATE_ARTIFACTS_METADATA",
];

fn create_gitlab_provenance_payload(subject: &ProvenanceSubject) -> Result<String, VarError> {
    let parameters = GITLAB_CI_PARAMETERS.iter()
        .filter_map(|&key| std::env::var(key).ok().map(|value| (key, value)))
        .collect::<BTreeMap<_, _>>();

    let value = json!({
        "_type": INTOTO_STATEMENT_V01_TYPE,
        "subject": [subject],
        "predicate_type": SLSA_PREDICATE_V02_TYPE,
        "predicate": {
            "buildType": format!("{}/{}", GITLAB_BUILD_TYPE_PREFIX, GITLAB_BUILD_TYPE_VERSION),
            "builder": {
                "id": format!("{}/-/runners/{}", std::env::var("CI_PROJECT_URL")?, std::env::var("CI_RUNNER_ID")?),
            },
            "invocation": {
                "configSource": {
                    "uri": format!("git+{}", std::env::var("CI_PROJECT_URL")?),
                    "digest": {
                        "sha1": std::env::var("CI_COMMIT_SHA")?,
                    },
                    "entryPoint": std::env::var("CI_JOB_NAME")?,
                },
                "parameters": parameters,
            },
            "environment": {
                "name": std::env::var("CI_RUNNER_DESCRIPTION")?,
                "architecture": std::env::var("CI_RUNNER_EXECUTABLE_ARCH")?,
                "server": std::env::var("CI_SERVER_URL")?,
                "project": std::env::var("CI_PROJECT_PATH")?,
                "job": {
                    "id": std::env::var("CI_JOB_ID")?,
                },
                "pipeline": {
                    "id": std::env::var("CI_PIPELINE_ID")?,
                    "ref": std::env::var("CI_CONFIG_PATH")?,
                },
            },
        },
        "metadata": {
            "buildInvocationId": std::env::var("CI_PIPELINE_ID")?,
            "completeness": {
                "parameters": true,
                "environment": true,
                "materials": false,
            },
            "reproducible": false,
        },
        "materials": [{
            "uri": format!("git+{}", std::env::var("CI_PROJECT_URL")?),
            "digest": {
                "sha1": std::env::var("CI_COMMIT_SHA")?,
            },
        }],
    });

    Ok(value.to_string())
}
