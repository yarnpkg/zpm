use std::{collections::HashMap, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use zpm_config::Configuration;
use zpm_parsers::JsonDocument;
use zpm_primitives::Ident;
use zpm_utils::ToFileString;

use crate::{error::Error, http::HttpClient, report::{if_active_async, with_report, StreamReport, StreamReportConfig}};

const ALGOLIA_URL: &str = "https://OFCNCOG2CU.algolia.net/1/indexes/*/objects";

/// Maximum amount of time we're willing to wait for Algolia to tell us whether
/// the packages we're adding ship their types through DefinitelyTyped. The
/// lookup is a nicety, so we cap it way below the global `httpTimeout` setting;
/// otherwise a network that silently drops the connection (a corporate proxy,
/// for instance) would stall `yarn add` for as long as the global timeout.
///
/// See https://github.com/yarnpkg/berry/issues/7111
const ALGOLIA_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AlgoliaInputPayload {
    requests: Vec<AlgoliaRequest>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AlgoliaRequest {
    index_name: String,

    #[serde(rename = "objectID")]
    object_id: String,

    attributes_to_retrieve: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlgoliaOutputPayload {
    results: Vec<AlgoliaResult>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlgoliaResult {
    types: AlgoliaTypes,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlgoliaTypes {
    definitely_typed: Option<Ident>,
}

/// Returns the packages from `idents` that have a matching DefinitelyTyped
/// package, along with the name of said package.
///
/// The Algolia index is only used to make `yarn add` more convenient, so being
/// unable to reach it must never turn into a hard failure: we warn the user and
/// let the install proceed without the `@types` packages.
pub async fn query_algolia(idents: &[Ident], config: &Configuration, http_client: &Arc<HttpClient>) -> HashMap<Ident, Ident> {
    if idents.is_empty() {
        return HashMap::new();
    }

    match query_algolia_impl(idents, http_client).await {
        Ok(type_idents) => type_idents,

        Err(err) => {
            let warnings = [
                format!("Couldn't query Algolia's npm-search index to detect which packages need a matching @types package ({}); they will be added without it.", err),
                "You can disable this lookup by setting enableAutoTypes to false in your .yarnrc.yml (or by setting the YARN_ENABLE_AUTO_TYPES=0 environment variable).".to_string(),
            ];

            // The lookup happens before `yarn add` opens the install report, so
            // we usually have to open a report of our own to be heard.
            if !emit_warnings(&warnings).await {
                let report
                    = StreamReport::new(StreamReportConfig::from_config(config));

                with_report(report, emit_warnings(&warnings)).await;
            }

            HashMap::new()
        },
    }
}

/// Sends the given warnings to the active report, if any. Returns whether a
/// report was active.
async fn emit_warnings(warnings: &[String]) -> bool {
    if_active_async(|report| {
        for warning in warnings {
            report.warn(warning.clone());
        }
    }).await
}

async fn query_algolia_impl(idents: &[Ident], http_client: &Arc<HttpClient>) -> Result<HashMap<Ident, Ident>, Error> {
    let input_payload = AlgoliaInputPayload {
        requests: idents.iter().map(|ident| AlgoliaRequest {
            index_name: "npm-search".to_string(),
            object_id: ident.to_file_string(),
            attributes_to_retrieve: vec!["types".to_string()],
        }).collect(),
    };

    let response = http_client.post(ALGOLIA_URL)?
        .body(JsonDocument::to_string(&input_payload).unwrap())
        .header("x-algolia-application-id", Some("OFCNCOG2CU"))
        .header("x-algolia-api-key", Some("e8e1bd300d860104bb8c58453ffa1eb4"))
        .timeout(ALGOLIA_TIMEOUT)
        .send()
        .await?;

    if response.status().as_u16() != 200 {
        return Ok(HashMap::new());
    }

    let body = response.text().await
        .map_err(|err| Error::AlgoliaRegistryError(Arc::new(err)))?;

    let Ok(output_payload) = JsonDocument::hydrate_from_str::<AlgoliaOutputPayload>(body.as_str()) else {
        return Ok(HashMap::new());
    };

    let type_idents_to_idents = idents.iter()
        .map(|ident| (ident.type_ident(), ident.clone()))
        .collect::<HashMap<_, _>>();

    let idents_to_type_idents = output_payload.results.into_iter()
        .filter_map(|result| result.types.definitely_typed)
        .map(|type_ident| (type_idents_to_idents.get(&type_ident).unwrap().clone(), type_ident))
        .collect::<HashMap<_, _>>();

    Ok(idents_to_type_idents)
}
