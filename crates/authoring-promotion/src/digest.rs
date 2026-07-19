use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use automation_ruleset::{RuleSetContentHash, RuleSetSchemaVersion, RuleSetVersionId};
use automation_ruleset_activation::ActivationDigest;

use crate::id::{
    AuthoringHash, IdempotencyScopeDigest, PrincipalId, PromotionRequestDigest, TenantId,
};
use crate::model::{ProductApprovalPayloadV1, PromotionIntentV1, PublicationRecordV1};
use crate::{IdempotencyKey, PromotionId};

const IDEMPOTENCY_SCOPE_DOMAIN_V1: &[u8] = b"starring.authoring_promotion.scope.v1\0";
const PROMOTION_REQUEST_DOMAIN_V1: &[u8] = b"starring.authoring_promotion.request.v1\0";
const ACTIVATION_REQUEST_DOMAIN_V1: &[u8] = b"starring.authoring_promotion.activation_request.v1\0";
const APPROVAL_PAYLOAD_DOMAIN_V1: &[u8] = b"starring.authoring_promotion.approval_payload.v1\0";

pub(crate) fn idempotency_scope_digest_v1(
    tenant_id: &TenantId,
    principal_id: &PrincipalId,
    key: &IdempotencyKey,
) -> Result<IdempotencyScopeDigest, DigestError> {
    idempotency_scope_digest_from_secret_v1(tenant_id, principal_id, key.as_str())
}

pub(crate) fn idempotency_scope_digest_from_secret_v1(
    tenant_id: &TenantId,
    principal_id: &PrincipalId,
    secret: &str,
) -> Result<IdempotencyScopeDigest, DigestError> {
    IdempotencyKey::validate_secret(secret).map_err(DigestError::IdempotencyIdentity)?;
    let fields = [
        b"{\"idempotency_key\":\"".as_slice(),
        secret.as_bytes(),
        b"\",\"principal_id\":\"".as_slice(),
        principal_id.as_str().as_bytes(),
        b"\",\"tenant_id\":\"".as_slice(),
        tenant_id.as_str().as_bytes(),
        b"\"}".as_slice(),
    ];
    let encoded_length = fields
        .iter()
        .try_fold(0_usize, |total, field| total.checked_add(field.len()));
    let encoded_length = encoded_length.ok_or_else(|| {
        DigestError::Serialize("promotion digest input length overflow".to_string())
    })?;
    let mut hasher = Sha256::new();
    update_length_framed(&mut hasher, IDEMPOTENCY_SCOPE_DOMAIN_V1);
    hasher.update(
        u64::try_from(encoded_length)
            .map_err(|_| DigestError::Serialize("promotion digest input is too long".to_string()))?
            .to_be_bytes(),
    );
    for field in fields {
        hasher.update(field);
    }
    IdempotencyScopeDigest::parse(&to_lower_hex(&hasher.finalize())).map_err(DigestError::Identity)
}

pub(crate) fn promotion_request_digest_v1(
    intent: &PromotionIntentV1,
) -> Result<PromotionRequestDigest, DigestError> {
    canonical_digest(PROMOTION_REQUEST_DOMAIN_V1, intent)
        .and_then(|value| PromotionRequestDigest::parse(&value).map_err(DigestError::Identity))
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ActivationRequestProjectionV1<'a> {
    promotion_id: &'a PromotionId,
    promotion_request_digest: &'a PromotionRequestDigest,
    version: RuleSetVersionId,
    schema_version: RuleSetSchemaVersion,
    content_hash: RuleSetContentHash,
}

pub(crate) fn activation_request_hash_v1(
    promotion_id: &PromotionId,
    promotion_request_digest: &PromotionRequestDigest,
    publication: &PublicationRecordV1,
) -> Result<AuthoringHash, DigestError> {
    canonical_digest(
        ACTIVATION_REQUEST_DOMAIN_V1,
        &ActivationRequestProjectionV1 {
            promotion_id,
            promotion_request_digest,
            version: publication.version,
            schema_version: publication.schema_version,
            content_hash: publication.content_hash,
        },
    )
    .and_then(|value| AuthoringHash::parse(&value).map_err(DigestError::Identity))
}

pub fn approval_payload_digest_v1(
    payload: &ProductApprovalPayloadV1,
) -> Result<ActivationDigest, DigestError> {
    canonical_digest(APPROVAL_PAYLOAD_DOMAIN_V1, payload).and_then(|value| {
        ActivationDigest::parse(&value)
            .map_err(|error| DigestError::ActivationIdentity(error.to_string()))
    })
}

fn canonical_digest(domain: &[u8], value: &impl Serialize) -> Result<String, DigestError> {
    let value =
        serde_json::to_value(value).map_err(|error| DigestError::Serialize(error.to_string()))?;
    let bytes = serde_json::to_vec(&canonicalize(value))
        .map_err(|error| DigestError::Serialize(error.to_string()))?;
    let mut hasher = Sha256::new();
    update_length_framed(&mut hasher, domain);
    update_length_framed(&mut hasher, &bytes);
    Ok(to_lower_hex(&hasher.finalize()))
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted = map
                .into_iter()
                .map(|(key, value)| (key, canonicalize(value)))
                .collect();
            Value::Object(sorted)
        }
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize).collect()),
        value => value,
    }
}

fn update_length_framed(hasher: &mut Sha256, value: &[u8]) {
    let length = u64::try_from(value.len()).expect("promotion digest field exceeds u64::MAX");
    hasher.update(length.to_be_bytes());
    hasher.update(value);
}

fn to_lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DigestError {
    #[error("promotion digest serialization failed: {0}")]
    Serialize(String),
    #[error("promotion digest identity is invalid: {0}")]
    Identity(crate::PromotionIdError),
    #[error("promotion idempotency identity is invalid: {0}")]
    IdempotencyIdentity(crate::OpaqueIdError),
    #[error("activation digest identity is invalid: {0}")]
    ActivationIdentity(String),
}
