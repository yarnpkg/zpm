use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};
use zpm_utils::ToFileString;

use crate::{error::Error, http::HttpClient, primitives::ident::Ident};

const ALGOLIA_URL: &str = "https://OFCNCOG2CU.algolia.net/1/indexes/*/objects";

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

pub async fn query_algolia(idents: &[Ident], http_client: &Arc<HttpClient>) -> Result<HashMap<Ident, Ident>, Error> {
    let input_payload = AlgoliaInputPayload {
        requests: idents.iter().map(|ident| AlgoliaRequest {
            index_name: "npm-search".to_string(),
            object_id: ident.to_file_string(),
            attributes_to_retrieve: vec!["types".to_string()],
        }).collect(),
    };

    let response = http_client.post(ALGOLIA_URL, sonic_rs::to_string(&input_payload).unwrap())?
        .header("x-algolia-application-id", "OFCNCOG2CU")
        .header("x-algolia-api-key", "e8e1bd300d860104bb8c58453ffa1eb4")
        .send()
        .await?;

    if response.status().as_u16() != 200 {
        return Ok(HashMap::new());
    }

    let body = response.text().await
        .map_err(|err| Error::AlgoliaRegistryError(Arc::new(err)))?;

    let Ok(output_payload) = sonic_rs::from_str::<AlgoliaOutputPayload>(body.as_str()) else {
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
