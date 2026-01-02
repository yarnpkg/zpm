// Copyright 2018-2025 the Deno authors. MIT license.

use std::collections::HashMap;
use std::env;
use std::sync::LazyLock;

use ring::rand::SystemRandom;
use ring::signature::EcdsaKeyPair;
use ring::signature::KeyPair;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::prelude::BASE64_STANDARD;
use p256::elliptic_curve;
use p256::pkcs8::AssociatedOid;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use spki::der::EncodePem;
use spki::der::asn1;
use spki::der::pem::LineEnding;
use zpm_parsers::JsonDocument;

use crate::error::Error;
use crate::http::HttpClient;

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
    )
    .into_bytes()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Signature {
    keyid: &'static str,
    sig: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    payload_type: String,
    payload: String,
    signatures: Vec<Signature>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureBundle {
    #[serde(rename = "$case")]
    case: &'static str,
    dsse_envelope: Envelope,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct X509Certificate {
    pub raw_bytes: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct X509CertificateChain {
    pub certificates: [X509Certificate; 1],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationMaterialContent {
    #[serde(rename = "$case")]
    pub case: &'static str,
    pub x509_certificate_chain: X509CertificateChain,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TlogEntry {
    pub log_index: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationMaterial {
    pub content: VerificationMaterialContent,
    pub tlog_entries: [TlogEntry; 1],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvenanceBundle {
    pub media_type: &'static str,
    pub content: SignatureBundle,
    pub verification_material: VerificationMaterial,
}

pub async fn attest(
    http_client: &HttpClient,
    data: &str,
    type_: &str,
    oidc_token: &str,
) -> Result<ProvenanceBundle, Error> {
    // DSSE Pre-Auth Encoding (PAE) payload
    let pae = pre_auth_encoding(type_, data);

    let signer = FulcioSigner::new(http_client)?;
    let (signature, key_material) = signer.sign(&pae, oidc_token).await?;

    let content = SignatureBundle {
        case: "dsseSignature",
        dsse_envelope: Envelope {
            payload_type: type_.to_string(),
            payload: BASE64_STANDARD.encode(data),
            signatures: vec![Signature {
                keyid: "",
                sig: BASE64_STANDARD.encode(signature.as_ref()),
            }],
        },
    };
    let transparency_logs =
        testify(http_client, &content, &key_material.certificate).await?;

    // First log entry is the one we're interested in
    let (_, log_entry) = transparency_logs.iter().next().unwrap();

    let bundle = ProvenanceBundle {
        media_type: "application/vnd.in-toto+json",
        content,
        verification_material: VerificationMaterial {
            content: VerificationMaterialContent {
                case: "x509CertificateChain",
                x509_certificate_chain: X509CertificateChain {
                    certificates: [X509Certificate {
                        raw_bytes: key_material.certificate,
                    }],
                },
            },
            tlog_entries: [TlogEntry {
                log_index: log_entry.log_index,
            }],
        },
    };

    Ok(bundle)
}

static DEFAULT_FULCIO_URL: LazyLock<String> = LazyLock::new(|| {
    env::var("FULCIO_URL")
        .unwrap_or_else(|_| "https://fulcio.sigstore.dev".to_string())
});

static ALGORITHM: &ring::signature::EcdsaSigningAlgorithm =
    &ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING;

struct KeyMaterial {
    pub _case: &'static str,
    pub certificate: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicKey {
    algorithm: &'static str,
    content: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicKeyRequest {
    public_key: PublicKey,
    proof_of_possession: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Credentials {
    oidc_identity_token: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateSigningCertificateRequest {
    credentials: Credentials,
    public_key_request: PublicKeyRequest,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CertificateChain {
    certificates: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignedCertificate {
    chain: CertificateChain,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SigningCertificateResponse {
    signed_certificate_embedded_sct: Option<SignedCertificate>,
    signed_certificate_detached_sct: Option<SignedCertificate>,
}

struct FulcioSigner<'a> {
    // The ephemeral key pair used to sign.
    ephemeral_signer: EcdsaKeyPair,
    rng: SystemRandom,
    http_client: &'a HttpClient,
}

impl<'a> FulcioSigner<'a> {
    pub fn new(http_client: &'a HttpClient) -> Result<Self, Error> {
        let rng
            = SystemRandom::new();

        let document
            = EcdsaKeyPair::generate_pkcs8(ALGORITHM, &rng)
                .map_err(|e| Error::ProvenanceError(format!("Failed to generate key pair: {}", e)))?;

        let ephemeral_signer =
            EcdsaKeyPair::from_pkcs8(ALGORITHM, document.as_ref(), &rng)
                .map_err(|e| Error::ProvenanceError(format!("Failed to create key pair from PKCS8: {}", e)))?;

        Ok(Self {
            ephemeral_signer,
            rng,
            http_client,
        })
    }

    pub async fn sign(
        self,
        data: &[u8],
        oidc_token: &str,
    ) -> Result<(ring::signature::Signature, KeyMaterial), Error> {
        // Extract the subject from the token
        let subject
            = extract_jwt_subject(oidc_token)?;

        // Sign the subject to create a challenge
        let challenge =
            self.ephemeral_signer.sign(&self.rng, subject.as_bytes())
                .map_err(|e| Error::ProvenanceError(format!("Failed to sign challenge: {}", e)))?;

        let subject_public_key
            = self.ephemeral_signer.public_key().as_ref();

        let algorithm = spki::AlgorithmIdentifier {
            oid: elliptic_curve::ALGORITHM_OID,
            parameters: Some((&p256::NistP256::OID).into()),
        };

        let spki = spki::SubjectPublicKeyInfoRef {
            algorithm,
            subject_public_key: asn1::BitStringRef::from_bytes(subject_public_key)
                .map_err(|e| Error::ProvenanceError(format!("Failed to create BitStringRef: {}", e)))?,
        };

        let pem = spki.to_pem(LineEnding::LF)
            .map_err(|e| Error::ProvenanceError(format!("Failed to encode PEM: {}", e)))?;

        // Create signing certificate
        let certificates = self
            .create_signing_certificate(oidc_token, pem, challenge)
            .await?;

        let signature = self.ephemeral_signer.sign(&self.rng, data)
            .map_err(|e| Error::ProvenanceError(format!("Failed to sign data: {}", e)))?;

        Ok((
            signature,
            KeyMaterial {
                _case: "x509Certificate",
                certificate: certificates[0].clone(),
            },
        ))
    }

    async fn create_signing_certificate(
        &self,
        token: &str,
        public_key: String,
        challenge: ring::signature::Signature,
    ) -> Result<Vec<String>, Error> {
        let url
            = format!("{}/api/v2/signingCert", *DEFAULT_FULCIO_URL);

        let request_body = CreateSigningCertificateRequest {
            credentials: Credentials {
                oidc_identity_token: token.to_string(),
            },
            public_key_request: PublicKeyRequest {
                public_key: PublicKey {
                    algorithm: "ECDSA",
                    content: public_key,
                },
                proof_of_possession: BASE64_STANDARD.encode(challenge.as_ref()),
            },
        };

        let body_str
            = JsonDocument::to_string(&request_body)?;

        let response = self
            .http_client
            .post(&url)?
            .header("content-type", Some("application/json"))
            .body(body_str)
            .send()
            .await?;

        let body_text
            = response.text().await?;

        let body: SigningCertificateResponse
            = JsonDocument::hydrate_from_str(&body_text)?;

        let key = body
            .signed_certificate_embedded_sct
            .or(body.signed_certificate_detached_sct)
            .ok_or_else(|| Error::ProvenanceError("No certificate chain returned".to_string()))?;

        Ok(key.chain.certificates)
    }

}

#[derive(Deserialize)]
struct JwtSubject<'a> {
    email: Option<String>,
    sub: String,
    iss: &'a str,
}

fn extract_jwt_subject(token: &str) -> Result<String, Error> {
    let parts: Vec<&str> = token.split('.').collect();

    let payload = parts.get(1)
        .ok_or_else(|| Error::ProvenanceError("Invalid JWT token format".to_string()))?;
    let payload = STANDARD_NO_PAD.decode(payload)
        .map_err(|e| Error::ProvenanceError(format!("Failed to decode JWT payload: {}", e)))?;

    let subject: JwtSubject
        = JsonDocument::hydrate_from_slice(&payload)?;

    match subject.iss {
        "https://accounts.google.com" | "https://oauth2.sigstore.dev/auth"
            => Ok(subject.email.unwrap_or(subject.sub)),
        _ => Ok(subject.sub),
    }
}

static DEFAULT_REKOR_URL: LazyLock<String> = LazyLock::new(|| {
    env::var("REKOR_URL")
        .unwrap_or_else(|_| "https://rekor.sigstore.dev".to_string())
});

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    #[allow(dead_code)]
    #[serde(rename = "logID")]
    pub log_id: String,
    pub log_index: u64,
}

type RekorEntry = HashMap<String, LogEntry>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RekorSignature {
    sig: String,
    // `publicKey` is not the standard part of
    // DSSE, but it's required by Rekor.
    public_key: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DsseEnvelope {
    payload: String,
    payload_type: String,
    signatures: [RekorSignature; 1],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProposedIntotoEntry {
    api_version: &'static str,
    kind: &'static str,
    spec: ProposedIntotoEntrySpec,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProposedIntotoEntrySpec {
    content: ProposedIntotoEntryContent,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProposedIntotoEntryContent {
    envelope: DsseEnvelope,
    hash: ProposedIntotoEntryHash,
    payload_hash: ProposedIntotoEntryHash,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProposedIntotoEntryHash {
    algorithm: &'static str,
    value: String,
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
    let payload_hash = hex::encode(sha2::Sha256::digest(
        content.dsse_envelope.payload.as_bytes(),
    ));

    // Calculate the value for the hash field into the Rekor entry
    let envelope_hash = hex::encode({
        let dsse = DsseEnvelope {
            payload: content.dsse_envelope.payload.clone(),
            payload_type: content.dsse_envelope.payload_type.clone(),
            signatures: [RekorSignature {
                sig: content.dsse_envelope.signatures[0].sig.clone(),
                public_key: public_key.to_string(),
            }],
        };

        let dsse_str
            = JsonDocument::to_string(&dsse)?;

        sha2::Sha256::digest(dsse_str.as_bytes())
    });

    // Re-create the DSSE envelop. `publicKey` is not the standard part of
    // DSSE, but it's required by Rekor.
    //
    // Double-encode payload and signature cause that's what Rekor expects
    let dsse = DsseEnvelope {
        payload_type: content.dsse_envelope.payload_type.clone(),
        payload: BASE64_STANDARD.encode(content.dsse_envelope.payload.clone()),
        signatures: [RekorSignature {
            sig: BASE64_STANDARD
                .encode(content.dsse_envelope.signatures[0].sig.clone()),
            public_key: BASE64_STANDARD.encode(public_key),
        }],
    };

    let proposed_intoto_entry = ProposedIntotoEntry {
        api_version: "0.0.2",
        kind: "intoto",
        spec: ProposedIntotoEntrySpec {
            content: ProposedIntotoEntryContent {
                envelope: dsse,
                hash: ProposedIntotoEntryHash {
                    algorithm: "sha256",
                    value: envelope_hash,
                },
                payload_hash: ProposedIntotoEntryHash {
                    algorithm: "sha256",
                    value: payload_hash,
                },
            },
        },
    };

    let url
        = format!("{}/api/v1/log/entries", *DEFAULT_REKOR_URL);

    let body_str
        = JsonDocument::to_string(&proposed_intoto_entry)?;

    let res = http_client
        .post(&url)?
        .header("content-type", Some("application/json"))
        .body(body_str)
        .send()
        .await?;

    let body_text
        = res.text().await?;

    let body: RekorEntry
        = JsonDocument::hydrate_from_str(&body_text)?;

    Ok(body)
}
