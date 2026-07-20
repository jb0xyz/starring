use std::fmt::{Debug, Formatter};

use authoring_application::AuthorizedPromotionAccessV1;
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use authoring_promotion::{
    AuthoringSessionId, PreparedPromotionPlanV1, PromotionRecordV1, PromotionStageV1,
    SessionGeneration,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

use crate::product_action_digest::{
    keyed_digest, product_action_keyring_coverage_identity_v1, ProductActionDigestKeyringV1,
};

use super::authorization::ProductPromotionAccessArgsV1;
use super::digest::{encode_lower_hex, ProductPromotionDigestsV1, ADMISSION_DOMAIN};

const ADMISSION_FORMAT_VERSION: u16 = 1;
const MAX_ADMISSION_PAYLOAD_BYTES: usize = 32_768;
const ENDPOINT_DOMAIN: &str = "product_promote_v1";
const KEY_MATERIAL_FINGERPRINT_DOMAIN: &[u8] =
    b"starring.product.promotion.digest-key-fingerprint.v1";

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub(super) enum ProductPromotionAdmissionErrorV1 {
    #[error("product promotion admission projection does not match the authorized candidate")]
    ProjectionMismatch,
    #[error("product promotion admission scalar exceeds the database domain")]
    ScalarOverflow,
    #[error("product promotion admission payload serialization failed")]
    Serialization,
    #[error("product promotion admission payload exceeds its size limit")]
    PayloadTooLarge,
    #[error("product promotion admission evidence format is invalid")]
    InvalidFormat,
    #[error("product promotion admission digest key is unavailable")]
    KeyUnavailable,
    #[error("product promotion admission digest does not match")]
    DigestMismatch,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductPromotionAdmissionPayloadV1 {
    pub endpoint_domain: String,
    pub product_request_id: String,
    pub tenant_id: String,
    pub installation_id: String,
    pub principal_id: String,
    pub authoring_session_id: String,
    pub generation: String,
    pub candidate_revision: String,
    pub candidate_hash: String,
    pub promotion_id: String,
    pub promotion_request_digest: String,
    pub session_subject_digest: String,
    pub idempotency_key_digest: String,
    pub idempotency_digest_key_id: String,
    pub idempotency_digest_key_fingerprint: String,
    pub semantic_request_digest: String,
    pub receipt_id: String,
    pub audit_event_id: String,
    pub discord_application_id: String,
    pub guild_id: String,
    pub acting_user_id: String,
    pub capability: String,
    pub authority_revision: String,
    pub authority_payload_digest: String,
    pub authority_observation_digest: String,
    pub authority_observed_at: String,
    pub authority_expires_at: String,
    pub effective_permission_bits: String,
    pub guild_owner: bool,
    pub binding_fingerprint: String,
    pub policy_revision: String,
}

impl Debug for ProductPromotionAdmissionPayloadV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductPromotionAdmissionPayloadV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductPromotionAdmissionEvidenceV1 {
    pub format_version: u16,
    pub payload: ProductPromotionAdmissionPayloadV1,
    pub admitted_at: DateTime<Utc>,
}

impl Debug for ProductPromotionAdmissionEvidenceV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductPromotionAdmissionEvidenceV1(<redacted>)")
    }
}

pub(super) struct PreparedProductPromotionAdmissionV1 {
    pub payload: ProductPromotionAdmissionPayloadV1,
    pub digest: String,
}

impl Debug for PreparedProductPromotionAdmissionV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PreparedProductPromotionAdmissionV1(<redacted>)")
    }
}

pub(super) struct ProductPromotionAdmissionContextV1 {
    pub product_request_id: String,
    pub authoring_session_id: AuthoringSessionId,
    pub generation: SessionGeneration,
}

impl Debug for ProductPromotionAdmissionContextV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductPromotionAdmissionContextV1(<redacted>)")
    }
}

pub(super) fn product_promotion_admission_context_v1(
    access: &AuthorizedPromotionAccessV1<'_, FreshDiscordAuthorityEvidenceV1>,
) -> ProductPromotionAdmissionContextV1 {
    ProductPromotionAdmissionContextV1 {
        product_request_id: access.request_id().as_str().to_string(),
        authoring_session_id: access.session_id().clone(),
        generation: access.expected_generation(),
    }
}

pub(super) fn prepare_product_promotion_admission_v1(
    keyring: &ProductActionDigestKeyringV1,
    context: &ProductPromotionAdmissionContextV1,
    access_args: &ProductPromotionAccessArgsV1,
    plan: &PreparedPromotionPlanV1,
    digests: &ProductPromotionDigestsV1,
) -> Result<PreparedProductPromotionAdmissionV1, ProductPromotionAdmissionErrorV1> {
    validate_plan_projection_v1(context, access_args, plan, digests)?;
    let candidate_revision = bounded_positive_i64_string(plan.intent.evidence.candidate_revision)?;
    let policy_revision = bounded_positive_i64_string(plan.intent.authority.policy.revision.get())?;
    let payload = ProductPromotionAdmissionPayloadV1 {
        endpoint_domain: ENDPOINT_DOMAIN.to_string(),
        product_request_id: context.product_request_id.clone(),
        tenant_id: access_args.expected_tenant_id.clone(),
        installation_id: access_args.expected_installation_id.clone(),
        principal_id: access_args.expected_principal_id.clone(),
        authoring_session_id: context.authoring_session_id.as_str().to_string(),
        generation: context.generation.get().to_string(),
        candidate_revision,
        candidate_hash: plan
            .intent
            .evidence
            .candidate_ruleset_hash
            .as_str()
            .to_string(),
        promotion_id: plan.promotion_id.as_str().to_string(),
        promotion_request_digest: plan.request_digest.as_str().to_string(),
        session_subject_digest: encode_lower_hex(&digests.session_subject),
        idempotency_key_digest: digests.active_idempotency.clone(),
        idempotency_digest_key_id: digests.active_key_id.clone(),
        idempotency_digest_key_fingerprint: digests.active_key_fingerprint.clone(),
        semantic_request_digest: digests.semantic_request.clone(),
        receipt_id: digests.receipt_id.clone(),
        audit_event_id: digests.audit_event_id.clone(),
        discord_application_id: access_args.expected_discord_application_id.clone(),
        guild_id: access_args.expected_guild_id.clone(),
        acting_user_id: access_args.expected_acting_user_id.clone(),
        capability: access_args.expected_capability.clone(),
        authority_revision: access_args.observed_current_authority_revision.to_string(),
        authority_payload_digest: access_args
            .observed_current_authority_payload_digest
            .clone(),
        authority_observation_digest: access_args.authority_observation_digest.clone(),
        authority_observed_at: canonical_timestamp(access_args.authority_observed_at),
        authority_expires_at: canonical_timestamp(access_args.authority_expires_at),
        effective_permission_bits: access_args.effective_permission_bits.clone(),
        guild_owner: access_args.guild_owner,
        binding_fingerprint: plan
            .intent
            .evidence
            .context_fingerprint
            .as_str()
            .to_string(),
        policy_revision,
    };
    let digest = sign_payload_v1(keyring, &payload)?;
    Ok(PreparedProductPromotionAdmissionV1 { payload, digest })
}

pub(super) fn prepare_legacy_product_promotion_admission_v1(
    keyring: &ProductActionDigestKeyringV1,
    context: &ProductPromotionAdmissionContextV1,
    access_args: &ProductPromotionAccessArgsV1,
    record: &PromotionRecordV1,
    digests: &ProductPromotionDigestsV1,
) -> Result<PreparedProductPromotionAdmissionV1, ProductPromotionAdmissionErrorV1> {
    validate_legacy_projection_v1(context, access_args, record, digests)?;
    let candidate_revision =
        bounded_positive_i64_string(record.intent.evidence.candidate_revision)?;
    let policy_revision =
        bounded_positive_i64_string(record.intent.authority.policy.revision.get())?;
    let payload = ProductPromotionAdmissionPayloadV1 {
        endpoint_domain: ENDPOINT_DOMAIN.to_string(),
        product_request_id: context.product_request_id.clone(),
        tenant_id: access_args.expected_tenant_id.clone(),
        installation_id: access_args.expected_installation_id.clone(),
        principal_id: access_args.expected_principal_id.clone(),
        authoring_session_id: context.authoring_session_id.as_str().to_string(),
        generation: context.generation.get().to_string(),
        candidate_revision,
        candidate_hash: record
            .intent
            .evidence
            .candidate_ruleset_hash
            .as_str()
            .to_string(),
        promotion_id: record.id.as_str().to_string(),
        promotion_request_digest: record.request_digest.as_str().to_string(),
        session_subject_digest: encode_lower_hex(&digests.session_subject),
        idempotency_key_digest: digests.active_idempotency.clone(),
        idempotency_digest_key_id: digests.active_key_id.clone(),
        idempotency_digest_key_fingerprint: digests.active_key_fingerprint.clone(),
        semantic_request_digest: digests.semantic_request.clone(),
        receipt_id: digests.receipt_id.clone(),
        audit_event_id: digests.audit_event_id.clone(),
        discord_application_id: access_args.expected_discord_application_id.clone(),
        guild_id: access_args.expected_guild_id.clone(),
        acting_user_id: access_args.expected_acting_user_id.clone(),
        capability: access_args.expected_capability.clone(),
        authority_revision: access_args.observed_current_authority_revision.to_string(),
        authority_payload_digest: access_args
            .observed_current_authority_payload_digest
            .clone(),
        authority_observation_digest: access_args.authority_observation_digest.clone(),
        authority_observed_at: canonical_timestamp(access_args.authority_observed_at),
        authority_expires_at: canonical_timestamp(access_args.authority_expires_at),
        effective_permission_bits: access_args.effective_permission_bits.clone(),
        guild_owner: access_args.guild_owner,
        binding_fingerprint: record
            .intent
            .evidence
            .context_fingerprint
            .as_str()
            .to_string(),
        policy_revision,
    };
    let digest = sign_payload_v1(keyring, &payload)?;
    Ok(PreparedProductPromotionAdmissionV1 { payload, digest })
}

pub(super) fn validate_product_promotion_admission_v1(
    keyring: &ProductActionDigestKeyringV1,
    evidence: &ProductPromotionAdmissionEvidenceV1,
    persisted_digest: &str,
) -> Result<(), ProductPromotionAdmissionErrorV1> {
    if evidence.format_version != ADMISSION_FORMAT_VERSION
        || evidence.payload.endpoint_domain != ENDPOINT_DOMAIN
        || !is_lower_hex_digest(persisted_digest)
    {
        return Err(ProductPromotionAdmissionErrorV1::InvalidFormat);
    }
    let canonical = canonical_payload_v1(&evidence.payload)?;
    let identity =
        product_action_keyring_coverage_identity_v1(keyring, KEY_MATERIAL_FINGERPRINT_DOMAIN);
    let key_index = identity
        .key_ids
        .iter()
        .zip(&identity.key_fingerprints)
        .position(|(key_id, fingerprint)| {
            key_id == &evidence.payload.idempotency_digest_key_id
                && fingerprint == &evidence.payload.idempotency_digest_key_fingerprint
        })
        .ok_or(ProductPromotionAdmissionErrorV1::KeyUnavailable)?;
    let expected = keyed_digest(
        &keyring.keys()[key_index],
        ADMISSION_DOMAIN,
        &[canonical.as_slice()],
    );
    if persisted_digest.len() != expected.len()
        || !bool::from(persisted_digest.as_bytes().ct_eq(expected.as_bytes()))
    {
        return Err(ProductPromotionAdmissionErrorV1::DigestMismatch);
    }
    Ok(())
}

fn validate_plan_projection_v1(
    context: &ProductPromotionAdmissionContextV1,
    access_args: &ProductPromotionAccessArgsV1,
    plan: &PreparedPromotionPlanV1,
    digests: &ProductPromotionDigestsV1,
) -> Result<(), ProductPromotionAdmissionErrorV1> {
    let authority = &plan.intent.authority;
    if plan.promotion_id != digests.promotion_id
        || authority.tenant_id.as_str() != access_args.expected_tenant_id
        || authority.installation_id.as_str() != access_args.expected_installation_id
        || authority.principal_id.as_str() != access_args.expected_principal_id
        || authority.session_owner_id != authority.principal_id
        || authority.session_id != context.authoring_session_id
        || authority.session_generation != context.generation
        || authority.guild_id.to_string() != access_args.expected_guild_id
        || authority.requester.to_string() != access_args.expected_acting_user_id
        || access_args.expected_capability != "promote"
        || access_args.expected_product_session_digest.len() != 32
        || digests.session_subject.len() != 32
        || digests.idempotency_candidates.is_empty()
        || digests.idempotency_candidates.len() > 8
        || digests.idempotency_candidates.len() != digests.idempotency_candidate_key_ids.len()
        || digests.idempotency_candidates.len()
            != digests.idempotency_candidate_key_fingerprints.len()
    {
        return Err(ProductPromotionAdmissionErrorV1::ProjectionMismatch);
    }
    Ok(())
}

fn validate_legacy_projection_v1(
    context: &ProductPromotionAdmissionContextV1,
    access_args: &ProductPromotionAccessArgsV1,
    record: &PromotionRecordV1,
    digests: &ProductPromotionDigestsV1,
) -> Result<(), ProductPromotionAdmissionErrorV1> {
    record
        .validate()
        .map_err(|_| ProductPromotionAdmissionErrorV1::ProjectionMismatch)?;
    let authority = &record.intent.authority;
    if record.id != digests.promotion_id
        || record.revision.get() != 3
        || !matches!(record.stage, PromotionStageV1::ActivationPending { .. })
        || authority.tenant_id.as_str() != access_args.expected_tenant_id
        || authority.installation_id.as_str() != access_args.expected_installation_id
        || authority.principal_id.as_str() != access_args.expected_principal_id
        || authority.session_owner_id != authority.principal_id
        || authority.session_id != context.authoring_session_id
        || authority.session_generation != context.generation
        || authority.guild_id.to_string() != access_args.expected_guild_id
        || authority.requester.to_string() != access_args.expected_acting_user_id
        || access_args.expected_capability != "promote"
        || access_args.expected_product_session_digest.len() != 32
        || digests.session_subject.len() != 32
        || digests.idempotency_candidates.is_empty()
        || digests.idempotency_candidates.len() > 8
        || digests.idempotency_candidates.len() != digests.idempotency_candidate_key_ids.len()
        || digests.idempotency_candidates.len()
            != digests.idempotency_candidate_key_fingerprints.len()
    {
        return Err(ProductPromotionAdmissionErrorV1::ProjectionMismatch);
    }
    Ok(())
}

fn sign_payload_v1(
    keyring: &ProductActionDigestKeyringV1,
    payload: &ProductPromotionAdmissionPayloadV1,
) -> Result<String, ProductPromotionAdmissionErrorV1> {
    let canonical = canonical_payload_v1(payload)?;
    Ok(keyed_digest(
        keyring.active(),
        ADMISSION_DOMAIN,
        &[canonical.as_slice()],
    ))
}

fn canonical_payload_v1(
    payload: &ProductPromotionAdmissionPayloadV1,
) -> Result<Vec<u8>, ProductPromotionAdmissionErrorV1> {
    let canonical =
        serde_json::to_vec(payload).map_err(|_| ProductPromotionAdmissionErrorV1::Serialization)?;
    if canonical.len() > MAX_ADMISSION_PAYLOAD_BYTES {
        return Err(ProductPromotionAdmissionErrorV1::PayloadTooLarge);
    }
    Ok(canonical)
}

fn bounded_positive_i64_string(value: u64) -> Result<String, ProductPromotionAdmissionErrorV1> {
    if value == 0 || value > i64::MAX as u64 {
        return Err(ProductPromotionAdmissionErrorV1::ScalarOverflow);
    }
    Ok(value.to_string())
}

fn canonical_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn is_lower_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use discord_model::Permissions;
    use serde_json::Value;

    use super::*;
    use crate::product_action_digest::ProductActionDigestKeyV1;

    fn key(id: &str, seed: u8) -> crate::ProductActionDigestKeyV1 {
        ProductActionDigestKeyV1::from_bytes(
            id,
            std::array::from_fn(|index| seed.wrapping_add(index as u8)),
        )
        .unwrap()
    }

    fn payload() -> ProductPromotionAdmissionPayloadV1 {
        ProductPromotionAdmissionPayloadV1 {
            endpoint_domain: ENDPOINT_DOMAIN.to_string(),
            product_request_id: "request-one".to_string(),
            tenant_id: "tenant-one".to_string(),
            installation_id: "installation-one".to_string(),
            principal_id: "principal-one".to_string(),
            authoring_session_id: "session-one".to_string(),
            generation: "3".to_string(),
            candidate_revision: "7".to_string(),
            candidate_hash: "ab".repeat(32),
            promotion_id: "bc".repeat(32),
            promotion_request_digest: "cd".repeat(32),
            session_subject_digest: "de".repeat(32),
            idempotency_key_digest: "ef".repeat(32),
            idempotency_digest_key_id: "active-v1".to_string(),
            idempotency_digest_key_fingerprint: String::new(),
            semantic_request_digest: "12".repeat(32),
            receipt_id: "23".repeat(32),
            audit_event_id: "34".repeat(32),
            discord_application_id: "100".to_string(),
            guild_id: "200".to_string(),
            acting_user_id: "300".to_string(),
            capability: "promote".to_string(),
            authority_revision: "5".to_string(),
            authority_payload_digest: "45".repeat(32),
            authority_observation_digest: "56".repeat(32),
            authority_observed_at: "2026-07-20T00:00:00.000000000Z".to_string(),
            authority_expires_at: "2026-07-20T00:00:05.000000000Z".to_string(),
            effective_permission_bits: Permissions::MANAGE_GUILD.bits().to_string(),
            guild_owner: false,
            binding_fingerprint: "67".repeat(32),
            policy_revision: "2".to_string(),
        }
    }

    fn evidence(
        keyring: &ProductActionDigestKeyringV1,
    ) -> (ProductPromotionAdmissionEvidenceV1, String) {
        let mut payload = payload();
        payload.idempotency_digest_key_fingerprint =
            product_action_keyring_coverage_identity_v1(keyring, KEY_MATERIAL_FINGERPRINT_DOMAIN)
                .key_fingerprints[0]
                .clone();
        let digest = sign_payload_v1(keyring, &payload).unwrap();
        (
            ProductPromotionAdmissionEvidenceV1 {
                format_version: ADMISSION_FORMAT_VERSION,
                payload,
                admitted_at: DateTime::parse_from_rfc3339("2026-07-20T00:00:10Z")
                    .unwrap()
                    .with_timezone(&Utc),
            },
            digest,
        )
    }

    #[test]
    fn admission_payload_has_the_exact_sql_field_set() {
        let value = serde_json::to_value(payload()).unwrap();
        let Value::Object(fields) = value else {
            panic!("payload must be an object")
        };
        assert_eq!(fields.len(), 31);
        for required in [
            "endpoint_domain",
            "product_request_id",
            "candidate_hash",
            "session_subject_digest",
            "idempotency_digest_key_fingerprint",
            "authority_observation_digest",
            "effective_permission_bits",
            "binding_fingerprint",
            "policy_revision",
        ] {
            assert!(fields.contains_key(required));
        }
    }

    #[test]
    fn admission_hmac_survives_active_key_rotation_and_detects_tampering() {
        let original = ProductActionDigestKeyringV1::new(key("active-v1", 9), []).unwrap();
        let (evidence, digest) = evidence(&original);
        assert_eq!(
            validate_product_promotion_admission_v1(&original, &evidence, &digest),
            Ok(())
        );
        let rotated =
            ProductActionDigestKeyringV1::new(key("active-v2", 90), [key("active-v1", 9)]).unwrap();
        assert_eq!(
            validate_product_promotion_admission_v1(&rotated, &evidence, &digest),
            Ok(())
        );
        let mut tampered = evidence;
        tampered.payload.guild_owner = true;
        assert_eq!(
            validate_product_promotion_admission_v1(&rotated, &tampered, &digest),
            Err(ProductPromotionAdmissionErrorV1::DigestMismatch)
        );
    }

    #[test]
    fn admission_envelope_rejects_unknown_fields_and_redacts_debug() {
        let keyring = ProductActionDigestKeyringV1::new(key("active-v1", 9), []).unwrap();
        let (evidence, _) = evidence(&keyring);
        let mut value = serde_json::to_value(&evidence).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("unknown".to_string(), Value::Bool(true));
        assert!(serde_json::from_value::<ProductPromotionAdmissionEvidenceV1>(value).is_err());
        assert_eq!(
            format!("{evidence:?}"),
            "ProductPromotionAdmissionEvidenceV1(<redacted>)"
        );
    }
}
