use zpm_parsers::json_provider::json;
use zpm_utils::Sha256;

use crate::{error::Error, http::HttpClient};

const PAE_PREFIX: &str = "DSSEv1";

/// DSSE Pre-Auth Encoding
///
/// https://github.com/secure-systems-lab/dsse/blob/master/protocol.md#signature-definition
fn pre_auth_encoding(payload_type: &str, payload: &str) -> Vec<u8> {
    format!(
        "{} {} {} {} {}",
        PAE_PREFIX,
        payload_type.len(),
        payload_type,
        payload.len(),
        payload,
    ).into_bytes()
}

pub async fn attest(
    http_client: &HttpClient,
    data: &str,
    type_: &str,
) -> Result<ProvenanceBundle, AnyError> {
    // DSSE Pre-Auth Encoding (PAE) payload
    let pae = pre_auth_encoding(type_, data);

    let signer = FulcioSigner::new(http_client)?;
    let (signature, key_material) = signer.sign(&pae).await?;

    let content = json!({
        "case": "dsseSignature",
        "dsseEnvelope": {
            "payloadType": type_.to_string(),
            "payload": BASE64_STANDARD.encode(data),
            "signatures": [{
                "keyid": "",
                "sig": BASE64_STANDARD.encode(signature.as_ref()),
            }],
        },
    });

    let transparency_logs =
    testify(http_client, &content, &key_material.certificate).await?;

    // First log entry is the one we're interested in
    let (_, log_entry) = transparency_logs.iter().next().unwrap();

    let bundle = json!({
        "mediaType": "application/vnd.in-toto+json",
        "content": content,
        "verificationMaterial": {
            "content": {
                "case": "x509CertificateChain",
                "x509CertificateChain": {
                    "certificates": [{
                        "raw_bytes": key_material.certificate,
                    }],
                },
            },
            "tlog_entries": [{
                "log_index": log_entry.log_index,
            }],
        },
    });

    Ok(bundle)
}

// Rekor witness
async fn testify(
    http_client: &HttpClient,
    content: &SignatureBundle,
    public_key: &str,
) -> Result<RekorEntry, Error> {
    // Rekor "intoto" entry for the given DSSE envelope and signature.
    //
    // Calculate the value for the payloadHash field into the Rekor entry
    let payload_hash = Sha256::new(
        content.dsse_envelope.payload.as_bytes(),
    ).to_hex();

    let dsse = json!({
        "payload": content.dsse_envelope.payload.clone(),
        "payloadType": content.dsse_envelope.payload_type.clone(),
        "signatures": [{
            "sig": content.dsse_envelope.signatures[0].sig.clone(),
            "publicKey": public_key.to_string(),
        }],
    }).to_string();

    // Calculate the value for the hash field into the Rekor entry
    let envelope_hash
        = Sha256::new(dsse.as_bytes()).to_hex();

    // Re-create the DSSE envelop. `publicKey` is not the standard part of
    // DSSE, but it's required by Rekor.
    //
    // Double-encode payload and signature cause that's what Rekor expects
    let dsse = json!({
        "payloadType": content.dsse_envelope.payload_type.clone(),
        "payload": BASE64_STANDARD.encode(content.dsse_envelope.payload.clone()),
        "signatures": [{
            "sig": BASE64_STANDARD
            .encode(content.dsse_envelope.signatures[0].sig.clone()),
            "publicKey": BASE64_STANDARD.encode(public_key),
        }],
    });

    let proposed_intoto_entry = json!({
        "apiVersion": "0.0.2",
        "kind": "intoto",
        "spec": {
            "content": {
                "envelope": dsse,
                "hash": {
                    "algorithm": "sha256",
                    "value": envelope_hash,
                },
                "payload_hash": {
                    "algorithm": "sha256",
                    "value": payload_hash,
                },
            },
        },
    });

    let url
        = format!("{}/api/v1/log/entries", *DEFAULT_REKOR_URL);

    let response = http_client
        .post(url)?
        .body(proposed_intoto_entry.to_string())
        .send()
        .await?;

    let response_body
        = response.text().await?;

    let body: RekorEntry
        = http_util::body_to_json(res).await?;

    Ok(body)
}
