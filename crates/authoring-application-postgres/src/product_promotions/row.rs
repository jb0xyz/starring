use std::fmt::{Debug, Formatter};

use authoring_application::AuthorizedPromotionSubmissionErrorV1;
use authoring_promotion::{PreparedPromotionPlanV1, PromotionRecordV1, PromotionStageV1};
use automation_ruleset_activation::ExpectedActiveBaselineV1;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use discord_model::Permissions;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;
use sqlx::types::Json;
use subtle::ConstantTimeEq;

use crate::product_action_digest::ProductActionDigestKeyringV1;

use super::admission::{
    validate_product_promotion_admission_v1, PreparedProductPromotionAdmissionV1,
    ProductPromotionAdmissionContextV1, ProductPromotionAdmissionEvidenceV1,
};
use super::authorization::ProductPromotionAccessArgsV1;
use super::digest::{promotion_action_ids_v1, ProductPromotionDigestsV1};

const MAX_PROMOTION_RECORD_BYTES: usize = 8_388_608;
const MAX_ADMISSION_BYTES: usize = 32_768;
const MAX_RECEIPT_BYTES: usize = 65_536;
const MAX_AUDIT_EVIDENCE_BYTES: usize = 65_536;
const PROMOTION_ENDPOINT: &str = "product_promote_v1";
const PROMOTION_TARGET: &str = "authoring_promotion";
const PROMOTION_ACTION: &str = "promotion.promote";
const REPLAY_RETENTION: Duration = Duration::hours(168);
const MAX_AUTHORITY_LIFETIME: Duration = Duration::seconds(5);

#[derive(sqlx::FromRow)]
pub(super) struct ProductPromotionReplayRowV1 {
    pub outcome_code: String,
    pub promotion_record: Option<Json<Value>>,
    pub admission_evidence: Option<Json<Value>>,
    pub admission_digest: Option<String>,
    pub receipt_projection: Option<Json<Value>>,
    pub audit_evidence_projection: Option<Json<Value>>,
    pub database_now: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub(super) struct ProductPromotionPrepareRowV1 {
    pub outcome_code: String,
    pub promotion_record: Option<Json<Value>>,
    pub admission_evidence: Option<Json<Value>>,
    pub admission_digest: Option<String>,
    pub database_now: DateTime<Utc>,
}

pub(crate) struct ProductPromotionAdmittedStageV1 {
    pub(crate) record: PromotionRecordV1,
    pub(crate) admission: ProductPromotionAdmissionEvidenceV1,
    pub(crate) admission_digest: String,
    pub(crate) database_now: DateTime<Utc>,
}

impl Debug for ProductPromotionAdmittedStageV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProductPromotionAdmittedStageV1")
            .field("record", &self.record)
            .field("admission", &"<redacted>")
            .field("admission_digest", &"<redacted>")
            .field("database_now", &self.database_now)
            .finish()
    }
}

pub(crate) struct ProductPromotionFinalReplayV1 {
    pub(crate) admitted: ProductPromotionAdmittedStageV1,
    pub(crate) receipt: ProductPromotionReceiptProjectionV1,
    pub(crate) audit_evidence: ProductPromotionAuditEvidenceProjectionV1,
}

pub(crate) struct ProductPromotionLegacyRepairV1 {
    pub(crate) record: PromotionRecordV1,
    pub(crate) database_now: DateTime<Utc>,
}

impl Debug for ProductPromotionLegacyRepairV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProductPromotionLegacyRepairV1")
            .field("record", &self.record)
            .field("database_now", &self.database_now)
            .finish()
    }
}

impl Debug for ProductPromotionFinalReplayV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProductPromotionFinalReplayV1")
            .field("admitted", &self.admitted)
            .field("receipt", &"<redacted>")
            .field("audit_evidence", &"<redacted>")
            .finish()
    }
}

pub(crate) enum ProductPromotionReplayStageV1 {
    Missing,
    PartialExact(Box<ProductPromotionAdmittedStageV1>),
    FinalExact(Box<ProductPromotionFinalReplayV1>),
    LegacyRepairRequired(Box<ProductPromotionLegacyRepairV1>),
}

impl Debug for ProductPromotionReplayStageV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => formatter.write_str("ProductPromotionReplayStageV1::Missing"),
            Self::PartialExact(value) => formatter
                .debug_tuple("ProductPromotionReplayStageV1::PartialExact")
                .field(value)
                .finish(),
            Self::FinalExact(value) => formatter
                .debug_tuple("ProductPromotionReplayStageV1::FinalExact")
                .field(value)
                .finish(),
            Self::LegacyRepairRequired(value) => formatter
                .debug_tuple("ProductPromotionReplayStageV1::LegacyRepairRequired")
                .field(value)
                .finish(),
        }
    }
}

pub(crate) enum ProductPromotionPrepareStageV1 {
    Created(Box<ProductPromotionAdmittedStageV1>),
    PartialExact(Box<ProductPromotionAdmittedStageV1>),
    FinalReplayRequired(Box<ProductPromotionAdmittedStageV1>),
    FinalExact(Box<ProductPromotionFinalReplayV1>),
}

impl Debug for ProductPromotionPrepareStageV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Created(value) => formatter
                .debug_tuple("ProductPromotionPrepareStageV1::Created")
                .field(value)
                .finish(),
            Self::PartialExact(value) => formatter
                .debug_tuple("ProductPromotionPrepareStageV1::PartialExact")
                .field(value)
                .finish(),
            Self::FinalReplayRequired(value) => formatter
                .debug_tuple("ProductPromotionPrepareStageV1::FinalReplayRequired")
                .field(value)
                .finish(),
            Self::FinalExact(value) => formatter
                .debug_tuple("ProductPromotionPrepareStageV1::FinalExact")
                .field(value)
                .finish(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductPromotionReceiptProjectionV1 {
    format_version: u16,
    receipt_id: String,
    tenant_id: String,
    installation_id: String,
    principal_id: String,
    endpoint_domain: String,
    idempotency_key_digest: String,
    idempotency_digest_key_id: String,
    idempotency_digest_key_fingerprint: String,
    request_digest: String,
    target_resource_type: String,
    target_resource_id: String,
    resulting_revision: Option<i64>,
    resulting_state: String,
    result_code: String,
    http_disposition_class: i16,
    completed_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductPromotionAuditEvidenceProjectionV1 {
    format_version: u16,
    event_id: String,
    receipt_id: String,
    tenant_id: String,
    installation_id: String,
    principal_id: String,
    session_subject_digest: String,
    action: String,
    target_resource_type: String,
    target_resource_id: String,
    request_id: String,
    authority_observation_digest: String,
    effective_permission_bits: String,
    authority_observed_at: DateTime<Utc>,
    installation_authority_revision: i64,
    expected_generation: Option<i64>,
    actual_generation: Option<i64>,
    payload_digest: Option<String>,
    binding_fingerprint: Option<String>,
    policy_revision: Option<i64>,
    active_baseline_version: Option<i64>,
    active_baseline_hash: Option<String>,
    resulting_state: String,
    result_code: String,
    dependency_latency_classes: ProductPromotionDependencyLatencyClassesV1,
    occurred_at: DateTime<Utc>,
    endpoint_domain: String,
    request_digest: String,
    resulting_revision: Option<i64>,
    http_disposition_class: i16,
    completed_at: DateTime<Utc>,
    evidence_version: i16,
    replay_policy_version: i16,
    replay_guaranteed_until: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductPromotionDependencyLatencyClassesV1 {}

pub(super) fn decode_product_promotion_replay_v1(
    row: ProductPromotionReplayRowV1,
    keyring: &ProductActionDigestKeyringV1,
    context: &ProductPromotionAdmissionContextV1,
    access: &ProductPromotionAccessArgsV1,
    digests: &ProductPromotionDigestsV1,
) -> Result<ProductPromotionReplayStageV1, AuthorizedPromotionSubmissionErrorV1> {
    let validation = ProductPromotionValidationContextV1 {
        keyring,
        context,
        access,
        digests,
    };
    match row.outcome_code.as_str() {
        "missing" => {
            require_all_absent(&row)?;
            Ok(ProductPromotionReplayStageV1::Missing)
        }
        "partial_exact" => {
            if row.receipt_projection.is_some() || row.audit_evidence_projection.is_some() {
                return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
            }
            let admitted = decode_admitted_v1(
                row.promotion_record,
                row.admission_evidence,
                row.admission_digest,
                row.database_now,
                &validation,
            )?;
            if !matches!(
                admitted.record.stage,
                PromotionStageV1::Prepared | PromotionStageV1::Published { .. }
            ) || admitted.admission.admitted_at != admitted.record.created_at
            {
                return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
            }
            Ok(ProductPromotionReplayStageV1::PartialExact(Box::new(
                admitted,
            )))
        }
        "final_exact" => {
            let receipt = decode_bounded_v1(row.receipt_projection, MAX_RECEIPT_BYTES)?;
            let audit_evidence =
                decode_bounded_v1(row.audit_evidence_projection, MAX_AUDIT_EVIDENCE_BYTES)?;
            let admitted = decode_admitted_v1(
                row.promotion_record,
                row.admission_evidence,
                row.admission_digest,
                row.database_now,
                &validation,
            )?;
            validate_final_projection_v1(&admitted, &receipt, &audit_evidence)?;
            Ok(ProductPromotionReplayStageV1::FinalExact(Box::new(
                ProductPromotionFinalReplayV1 {
                    admitted,
                    receipt,
                    audit_evidence,
                },
            )))
        }
        "legacy_repair_required" => {
            if row.admission_evidence.is_some()
                || row.admission_digest.is_some()
                || row.receipt_projection.is_some()
                || row.audit_evidence_projection.is_some()
            {
                return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
            }
            let record = decode_bounded_v1(row.promotion_record, MAX_PROMOTION_RECORD_BYTES)?;
            validate_legacy_repair_record_v1(&record, row.database_now, context, access, digests)?;
            Ok(ProductPromotionReplayStageV1::LegacyRepairRequired(
                Box::new(ProductPromotionLegacyRepairV1 {
                    record,
                    database_now: row.database_now,
                }),
            ))
        }
        "idempotency_conflict" => Err(AuthorizedPromotionSubmissionErrorV1::IdempotencyConflict),
        "access_denied" => Err(AuthorizedPromotionSubmissionErrorV1::Forbidden),
        "scope_mismatch" => Err(AuthorizedPromotionSubmissionErrorV1::ScopeMismatch),
        "persistence_corrupt" => Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt),
        _ => Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt),
    }
}

pub(super) fn decode_product_promotion_prepare_v1(
    row: ProductPromotionPrepareRowV1,
    keyring: &ProductActionDigestKeyringV1,
    context: &ProductPromotionAdmissionContextV1,
    access: &ProductPromotionAccessArgsV1,
    digests: &ProductPromotionDigestsV1,
    plan: &PreparedPromotionPlanV1,
    prepared_admission: &PreparedProductPromotionAdmissionV1,
) -> Result<ProductPromotionPrepareStageV1, AuthorizedPromotionSubmissionErrorV1> {
    let validation = ProductPromotionValidationContextV1 {
        keyring,
        context,
        access,
        digests,
    };
    match row.outcome_code.as_str() {
        "created" | "partial_exact" | "final_exact" => {
            let admitted = decode_admitted_v1(
                row.promotion_record,
                row.admission_evidence,
                row.admission_digest,
                row.database_now,
                &validation,
            )?;
            plan.validate_admitted_record(&admitted.record)
                .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
            match row.outcome_code.as_str() {
                "created" => {
                    if admitted.admission.payload != prepared_admission.payload
                        || !constant_time_text_eq(
                            &admitted.admission_digest,
                            &prepared_admission.digest,
                        )
                    {
                        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
                    }
                    plan.validate_prepared_record(&admitted.record)
                        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
                    if admitted.admission.admitted_at != admitted.record.created_at
                        || admitted.database_now != admitted.record.created_at
                    {
                        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
                    }
                    Ok(ProductPromotionPrepareStageV1::Created(Box::new(admitted)))
                }
                "partial_exact" => {
                    if !matches!(
                        admitted.record.stage,
                        PromotionStageV1::Prepared | PromotionStageV1::Published { .. }
                    ) || admitted.admission.admitted_at != admitted.record.created_at
                    {
                        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
                    }
                    Ok(ProductPromotionPrepareStageV1::PartialExact(Box::new(
                        admitted,
                    )))
                }
                "final_exact" => {
                    if !is_final_stage(&admitted.record.stage) {
                        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
                    }
                    Ok(ProductPromotionPrepareStageV1::FinalReplayRequired(
                        Box::new(admitted),
                    ))
                }
                _ => Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt),
            }
        }
        "idempotency_conflict" => Err(AuthorizedPromotionSubmissionErrorV1::IdempotencyConflict),
        "generation_mismatch" => Err(AuthorizedPromotionSubmissionErrorV1::GenerationMismatch),
        "access_denied" => Err(AuthorizedPromotionSubmissionErrorV1::Forbidden),
        "scope_mismatch" => Err(AuthorizedPromotionSubmissionErrorV1::ScopeMismatch),
        "invalid_candidate" => Err(AuthorizedPromotionSubmissionErrorV1::InvalidCandidate),
        "persistence_corrupt" => Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt),
        _ => Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt),
    }
}

fn decode_admitted_v1(
    promotion_record: Option<Json<Value>>,
    admission_evidence: Option<Json<Value>>,
    admission_digest: Option<String>,
    database_now: DateTime<Utc>,
    validation: &ProductPromotionValidationContextV1<'_>,
) -> Result<ProductPromotionAdmittedStageV1, AuthorizedPromotionSubmissionErrorV1> {
    let record = decode_bounded_v1(promotion_record, MAX_PROMOTION_RECORD_BYTES)?;
    let admission = decode_bounded_v1(admission_evidence, MAX_ADMISSION_BYTES)?;
    let admission_digest =
        admission_digest.ok_or(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    validate_product_promotion_admission_v1(validation.keyring, &admission, &admission_digest)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    validate_admitted_projection_v1(
        &record,
        &admission,
        database_now,
        validation.keyring,
        validation.context,
        validation.access,
        validation.digests,
    )?;
    Ok(ProductPromotionAdmittedStageV1 {
        record,
        admission,
        admission_digest,
        database_now,
    })
}

struct ProductPromotionValidationContextV1<'a> {
    keyring: &'a ProductActionDigestKeyringV1,
    context: &'a ProductPromotionAdmissionContextV1,
    access: &'a ProductPromotionAccessArgsV1,
    digests: &'a ProductPromotionDigestsV1,
}

fn validate_admitted_projection_v1(
    record: &PromotionRecordV1,
    admission: &ProductPromotionAdmissionEvidenceV1,
    database_now: DateTime<Utc>,
    keyring: &ProductActionDigestKeyringV1,
    context: &ProductPromotionAdmissionContextV1,
    access: &ProductPromotionAccessArgsV1,
    digests: &ProductPromotionDigestsV1,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    record
        .validate()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let payload = &admission.payload;
    let authority = &record.intent.authority;
    if record.id != digests.promotion_id
        || payload.endpoint_domain != PROMOTION_ENDPOINT
        || !valid_opaque_id(&payload.product_request_id, 128)
        || payload.tenant_id != access.expected_tenant_id
        || payload.installation_id != access.expected_installation_id
        || payload.principal_id != access.expected_principal_id
        || payload.authoring_session_id != context.authoring_session_id.as_str()
        || payload.generation != context.generation.get().to_string()
        || payload.candidate_revision != record.intent.evidence.candidate_revision.to_string()
        || payload.candidate_hash != record.intent.evidence.candidate_ruleset_hash.as_str()
        || payload.promotion_id != record.id.as_str()
        || payload.promotion_request_digest != record.request_digest.as_str()
        || !is_lower_hex_digest(&payload.session_subject_digest)
        || payload.semantic_request_digest != digests.semantic_request
        || payload.discord_application_id != access.expected_discord_application_id
        || payload.guild_id != access.expected_guild_id
        || payload.acting_user_id != access.expected_acting_user_id
        || payload.capability != "promote"
        || payload.binding_fingerprint != record.intent.evidence.context_fingerprint.as_str()
        || payload.policy_revision != authority.policy.revision.get().to_string()
        || authority.tenant_id.as_str() != access.expected_tenant_id
        || authority.installation_id.as_str() != access.expected_installation_id
        || authority.principal_id.as_str() != access.expected_principal_id
        || authority.session_owner_id != authority.principal_id
        || authority.session_id != context.authoring_session_id
        || authority.session_generation != context.generation
        || authority.guild_id.to_string() != access.expected_guild_id
        || authority.requester.to_string() != access.expected_acting_user_id
        || record.updated_at > database_now
        || !is_lower_hex_digest(&payload.authority_payload_digest)
        || !is_lower_hex_digest(&payload.authority_observation_digest)
        || !is_lower_hex_digest(&payload.receipt_id)
        || !is_lower_hex_digest(&payload.audit_event_id)
        || !is_lower_hex_digest(&payload.idempotency_key_digest)
        || !is_lower_hex_digest(&payload.idempotency_digest_key_fingerprint)
        || !valid_key_id(&payload.idempotency_digest_key_id)
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    let candidate_index = digests
        .idempotency_candidate_key_ids
        .iter()
        .zip(&digests.idempotency_candidate_key_fingerprints)
        .position(|(key_id, fingerprint)| {
            key_id == &payload.idempotency_digest_key_id
                && fingerprint == &payload.idempotency_digest_key_fingerprint
        })
        .ok_or(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    if !constant_time_text_eq(
        &payload.idempotency_key_digest,
        &digests.idempotency_candidates[candidate_index],
    ) {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    let action_ids = promotion_action_ids_v1(
        &keyring.keys()[candidate_index],
        &payload.tenant_id,
        &payload.installation_id,
        &payload.principal_id,
        &payload.promotion_id,
        &payload.idempotency_key_digest,
        &payload.semantic_request_digest,
    );
    if !constant_time_text_eq(&payload.receipt_id, &action_ids.receipt_id)
        || !constant_time_text_eq(&payload.audit_event_id, &action_ids.audit_event_id)
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    let authority_revision = parse_positive_i64(&payload.authority_revision)?;
    let permission_bits = payload
        .effective_permission_bits
        .parse::<u64>()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let permissions = Permissions::from_bits_retain(permission_bits);
    if authority_revision == 0
        || (!payload.guild_owner
            && !permissions.intersects(Permissions::ADMINISTRATOR | Permissions::MANAGE_GUILD))
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    let observed_at = parse_canonical_timestamp(&payload.authority_observed_at)?;
    let expires_at = parse_canonical_timestamp(&payload.authority_expires_at)?;
    let latest_expiry = observed_at
        .checked_add_signed(MAX_AUTHORITY_LIFETIME)
        .ok_or(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    if postgres_timestamp_micros(observed_at) > postgres_timestamp_micros(admission.admitted_at)
        || postgres_timestamp_micros(admission.admitted_at) >= postgres_timestamp_micros(expires_at)
        || expires_at > latest_expiry
        || admission.admitted_at < record.created_at
        || admission.admitted_at > database_now
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn validate_legacy_repair_record_v1(
    record: &PromotionRecordV1,
    database_now: DateTime<Utc>,
    context: &ProductPromotionAdmissionContextV1,
    access: &ProductPromotionAccessArgsV1,
    digests: &ProductPromotionDigestsV1,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    record
        .validate()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let authority = &record.intent.authority;
    if record.id != digests.promotion_id
        || record.revision.get() != 3
        || !matches!(&record.stage, PromotionStageV1::ActivationPending { .. })
        || record.updated_at > database_now
        || authority.tenant_id.as_str() != access.expected_tenant_id
        || authority.installation_id.as_str() != access.expected_installation_id
        || authority.principal_id.as_str() != access.expected_principal_id
        || authority.session_owner_id != authority.principal_id
        || authority.session_id != context.authoring_session_id
        || authority.session_generation != context.generation
        || authority.guild_id.to_string() != access.expected_guild_id
        || authority.requester.to_string() != access.expected_acting_user_id
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn validate_final_projection_v1(
    admitted: &ProductPromotionAdmittedStageV1,
    receipt: &ProductPromotionReceiptProjectionV1,
    audit: &ProductPromotionAuditEvidenceProjectionV1,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    if !is_final_stage(&admitted.record.stage) {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    let payload = &admitted.admission.payload;
    let revision = i64::try_from(admitted.record.revision.get())
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let state = stage_name(&admitted.record.stage);
    let generation = i64::try_from(admitted.record.intent.authority.session_generation.get())
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let authority_revision = parse_positive_i64(&payload.authority_revision)?;
    let policy_revision = i64::try_from(admitted.record.intent.authority.policy.revision.get())
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let authority_observed_at = parse_canonical_timestamp(&payload.authority_observed_at)?;
    let (active_baseline_version, active_baseline_hash) = active_baseline(&admitted.record)?;
    let valid_result_code = matches!(
        receipt.result_code.as_str(),
        "promotion_created" | "promotion_recovered"
    );
    if receipt.format_version != 1
        || receipt.receipt_id != payload.receipt_id
        || receipt.tenant_id != payload.tenant_id
        || receipt.installation_id != payload.installation_id
        || receipt.principal_id != payload.principal_id
        || receipt.endpoint_domain != PROMOTION_ENDPOINT
        || receipt.idempotency_key_digest != payload.idempotency_key_digest
        || receipt.idempotency_digest_key_id != payload.idempotency_digest_key_id
        || receipt.idempotency_digest_key_fingerprint != payload.idempotency_digest_key_fingerprint
        || receipt.request_digest != payload.semantic_request_digest
        || receipt.target_resource_type != PROMOTION_TARGET
        || receipt.target_resource_id != payload.promotion_id
        || !receipt_matches_journal_identity(
            state,
            revision,
            receipt.resulting_state.as_str(),
            receipt.resulting_revision,
            receipt.completed_at,
            admitted.record.updated_at,
        )
        || !valid_result_code
        || receipt.http_disposition_class != 2
        || receipt.completed_at < admitted.admission.admitted_at
        || receipt.completed_at > admitted.database_now
        || audit.format_version != 1
        || audit.receipt_id != receipt.receipt_id
        || audit.event_id != payload.audit_event_id
        || audit.tenant_id != receipt.tenant_id
        || audit.installation_id != receipt.installation_id
        || audit.principal_id != receipt.principal_id
        || audit.session_subject_digest != payload.session_subject_digest
        || audit.request_id != payload.product_request_id
        || audit.authority_observation_digest != payload.authority_observation_digest
        || audit.effective_permission_bits != payload.effective_permission_bits
        || !same_postgres_timestamp(audit.authority_observed_at, authority_observed_at)
        || audit.installation_authority_revision != authority_revision
        || audit.expected_generation != Some(generation)
        || audit.actual_generation != Some(generation)
        || audit.payload_digest.as_deref() != Some(payload.promotion_request_digest.as_str())
        || audit.binding_fingerprint.as_deref() != Some(payload.binding_fingerprint.as_str())
        || audit.policy_revision != Some(policy_revision)
        || audit.active_baseline_version != active_baseline_version
        || audit.active_baseline_hash != active_baseline_hash
        || audit.endpoint_domain != receipt.endpoint_domain
        || audit.action != PROMOTION_ACTION
        || audit.request_digest != receipt.request_digest
        || audit.target_resource_type != receipt.target_resource_type
        || audit.target_resource_id != receipt.target_resource_id
        || audit.resulting_revision != receipt.resulting_revision
        || audit.resulting_state != receipt.resulting_state
        || audit.result_code != receipt.result_code
        || audit.http_disposition_class != receipt.http_disposition_class
        || audit.completed_at != receipt.completed_at
        || audit.occurred_at != receipt.completed_at
        || audit.evidence_version != 1
        || audit.replay_policy_version != 1
        || audit.replay_guaranteed_until
            != receipt
                .completed_at
                .checked_add_signed(REPLAY_RETENTION)
                .ok_or(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    if !admission_time_matches_result(
        &receipt.result_code,
        admitted.admission.admitted_at,
        admitted.record.created_at,
    ) {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn admission_time_matches_result(
    result_code: &str,
    admitted_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
) -> bool {
    match result_code {
        "promotion_created" => admitted_at == created_at,
        "promotion_recovered" => admitted_at >= created_at,
        _ => false,
    }
}

fn receipt_matches_journal_identity(
    current_state: &str,
    current_revision: i64,
    receipt_state: &str,
    receipt_revision: Option<i64>,
    receipt_completed_at: DateTime<Utc>,
    current_updated_at: DateTime<Utc>,
) -> bool {
    match (
        current_state,
        current_revision,
        receipt_state,
        receipt_revision,
    ) {
        ("activation_pending", 3, "activation_pending", Some(3))
        | ("expired", 3, "expired", Some(3)) => receipt_completed_at == current_updated_at,
        ("expired", 4, "activation_pending", Some(3)) => receipt_completed_at <= current_updated_at,
        _ => false,
    }
}

fn active_baseline(
    record: &PromotionRecordV1,
) -> Result<(Option<i64>, Option<String>), AuthorizedPromotionSubmissionErrorV1> {
    let activation = match &record.stage {
        PromotionStageV1::ActivationPending { activation, .. }
        | PromotionStageV1::Expired { activation, .. } => activation,
        PromotionStageV1::Prepared | PromotionStageV1::Published { .. } => {
            return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
        }
    };
    match &activation.approval_context.baseline {
        ExpectedActiveBaselineV1::Absent => Ok((None, None)),
        ExpectedActiveBaselineV1::Exact {
            version,
            content_hash,
        } => Ok((Some(i64::from(version.get())), Some(content_hash.to_hex()))),
    }
}

fn require_all_absent(
    row: &ProductPromotionReplayRowV1,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    if row.promotion_record.is_some()
        || row.admission_evidence.is_some()
        || row.admission_digest.is_some()
        || row.receipt_projection.is_some()
        || row.audit_evidence_projection.is_some()
    {
        Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
    } else {
        Ok(())
    }
}

fn decode_bounded_v1<T: DeserializeOwned>(
    value: Option<Json<Value>>,
    maximum: usize,
) -> Result<T, AuthorizedPromotionSubmissionErrorV1> {
    let value = value.ok_or(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let encoded = serde_json::to_vec(&value.0)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    if encoded.len() > maximum {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    serde_json::from_slice(&encoded)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
}

fn parse_positive_i64(value: &str) -> Result<i64, AuthorizedPromotionSubmissionErrorV1> {
    let value = value
        .parse::<i64>()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    if value <= 0 {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    Ok(value)
}

fn parse_canonical_timestamp(
    value: &str,
) -> Result<DateTime<Utc>, AuthorizedPromotionSubmissionErrorV1> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?
        .with_timezone(&Utc);
    if parsed.to_rfc3339_opts(SecondsFormat::Nanos, true) != value {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    Ok(parsed)
}

fn is_final_stage(stage: &PromotionStageV1) -> bool {
    matches!(
        stage,
        PromotionStageV1::ActivationPending { .. } | PromotionStageV1::Expired { .. }
    )
}

fn stage_name(stage: &PromotionStageV1) -> &'static str {
    match stage {
        PromotionStageV1::Prepared => "prepared",
        PromotionStageV1::Published { .. } => "published",
        PromotionStageV1::ActivationPending { .. } => "activation_pending",
        PromotionStageV1::Expired { .. } => "expired",
    }
}

fn valid_opaque_id(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

fn valid_key_id(value: &str) -> bool {
    valid_opaque_id(value, 64)
}

fn is_lower_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn constant_time_text_eq(left: &str, right: &str) -> bool {
    left.len() == right.len() && bool::from(left.as_bytes().ct_eq(right.as_bytes()))
}

fn postgres_timestamp_micros(value: DateTime<Utc>) -> i64 {
    value.timestamp_micros()
}

fn same_postgres_timestamp(left: DateTime<Utc>, right: DateTime<Utc>) -> bool {
    postgres_timestamp_micros(left) == postgres_timestamp_micros(right)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn database_now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-07-20T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn missing_row() -> ProductPromotionReplayRowV1 {
        ProductPromotionReplayRowV1 {
            outcome_code: "missing".to_string(),
            promotion_record: None,
            admission_evidence: None,
            admission_digest: None,
            receipt_projection: None,
            audit_evidence_projection: None,
            database_now: database_now(),
        }
    }

    #[test]
    fn missing_requires_every_optional_projection_to_be_absent() {
        assert_eq!(require_all_absent(&missing_row()), Ok(()));
        let mut corrupt = missing_row();
        corrupt.admission_digest = Some("ab".repeat(32));
        assert_eq!(
            require_all_absent(&corrupt),
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        );
    }

    #[test]
    fn bounded_decoder_rejects_unknown_fields_and_oversized_values() {
        let unknown = Json(json!({
            "format_version": 1,
            "receipt_id": "ab",
            "unexpected": true
        }));
        assert!(matches!(
            decode_bounded_v1::<ProductPromotionReceiptProjectionV1>(Some(unknown), 65_536),
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        ));
        let oversized = Json(Value::String("x".repeat(33)));
        assert!(matches!(
            decode_bounded_v1::<Value>(Some(oversized), 32),
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        ));
    }

    #[test]
    fn final_projection_envelopes_match_receipt_and_audit_evidence_columns() {
        let completed_at = "2026-07-20T00:00:00Z";
        let replay_until = "2026-07-27T00:00:00Z";
        let receipt = json!({
            "format_version": 1,
            "receipt_id": "ab".repeat(32),
            "tenant_id": "tenant-one",
            "installation_id": "installation-one",
            "principal_id": "principal-one",
            "endpoint_domain": "product_promote_v1",
            "idempotency_key_digest": "bc".repeat(32),
            "idempotency_digest_key_id": "active-v1",
            "idempotency_digest_key_fingerprint": "cd".repeat(32),
            "request_digest": "de".repeat(32),
            "target_resource_type": "authoring_promotion",
            "target_resource_id": "ef".repeat(32),
            "resulting_revision": 3,
            "resulting_state": "activation_pending",
            "result_code": "promotion_created",
            "http_disposition_class": 2,
            "completed_at": completed_at
        });
        let receipt = decode_bounded_v1::<ProductPromotionReceiptProjectionV1>(
            Some(Json(receipt)),
            MAX_RECEIPT_BYTES,
        )
        .unwrap();
        let audit = json!({
            "format_version": 1,
            "event_id": "fa".repeat(32),
            "receipt_id": receipt.receipt_id.clone(),
            "tenant_id": receipt.tenant_id.clone(),
            "installation_id": receipt.installation_id.clone(),
            "principal_id": receipt.principal_id.clone(),
            "session_subject_digest": "01".repeat(32),
            "action": "promotion.promote",
            "target_resource_type": receipt.target_resource_type.clone(),
            "target_resource_id": receipt.target_resource_id.clone(),
            "request_id": "request-one",
            "authority_observation_digest": "02".repeat(32),
            "effective_permission_bits": "32",
            "authority_observed_at": completed_at,
            "installation_authority_revision": 4,
            "expected_generation": 3,
            "actual_generation": 3,
            "payload_digest": "03".repeat(32),
            "binding_fingerprint": "04".repeat(32),
            "policy_revision": 2,
            "active_baseline_version": null,
            "active_baseline_hash": null,
            "resulting_state": receipt.resulting_state.clone(),
            "result_code": receipt.result_code.clone(),
            "dependency_latency_classes": {},
            "occurred_at": receipt.completed_at,
            "endpoint_domain": receipt.endpoint_domain.clone(),
            "request_digest": receipt.request_digest.clone(),
            "resulting_revision": receipt.resulting_revision,
            "http_disposition_class": receipt.http_disposition_class,
            "completed_at": receipt.completed_at,
            "evidence_version": 1,
            "replay_policy_version": 1,
            "replay_guaranteed_until": replay_until
        });
        let audit = decode_bounded_v1::<ProductPromotionAuditEvidenceProjectionV1>(
            Some(Json(audit)),
            MAX_AUDIT_EVIDENCE_BYTES,
        )
        .unwrap();
        assert_eq!(audit.evidence_version, 1);
        assert_eq!(audit.replay_policy_version, 1);
        assert_eq!(
            audit.replay_guaranteed_until,
            receipt.completed_at + REPLAY_RETENTION
        );
        assert!(
            serde_json::from_value::<ProductPromotionDependencyLatencyClassesV1>(
                json!({"database": "fast"})
            )
            .is_err()
        );
    }

    #[test]
    fn only_canonical_database_timestamps_are_accepted() {
        assert!(parse_canonical_timestamp("2026-07-20T00:00:00.000000000Z").is_ok());
        assert!(parse_canonical_timestamp("2026-07-20T00:00:00Z").is_err());
        assert!(parse_canonical_timestamp("2026-07-20T09:00:00+09:00").is_err());
    }

    #[test]
    fn stable_identifier_and_digest_domains_are_strict() {
        assert!(valid_opaque_id("request.one:2", 128));
        assert!(!valid_opaque_id("request one", 128));
        assert!(is_lower_hex_digest(&"ab".repeat(32)));
        assert!(!is_lower_hex_digest(&"AB".repeat(32)));
        assert!(constant_time_text_eq("same", "same"));
        assert!(!constant_time_text_eq("same", "different"));
    }

    #[test]
    fn final_receipt_preserves_the_historical_promote_result() {
        let activation_time = database_now();
        let expired_time = activation_time + Duration::hours(1);
        assert!(receipt_matches_journal_identity(
            "activation_pending",
            3,
            "activation_pending",
            Some(3),
            activation_time,
            activation_time,
        ));
        assert!(receipt_matches_journal_identity(
            "expired",
            3,
            "expired",
            Some(3),
            expired_time,
            expired_time,
        ));
        assert!(receipt_matches_journal_identity(
            "expired",
            4,
            "activation_pending",
            Some(3),
            activation_time,
            expired_time,
        ));
        assert!(!receipt_matches_journal_identity(
            "expired",
            4,
            "expired",
            Some(4),
            expired_time,
            expired_time,
        ));
        assert!(!receipt_matches_journal_identity(
            "expired",
            4,
            "activation_pending",
            Some(3),
            expired_time,
            activation_time,
        ));
    }

    #[test]
    fn normal_and_recovery_admission_times_have_distinct_contracts() {
        let created_at = database_now();
        let recovered_at = created_at + Duration::minutes(10);
        assert!(admission_time_matches_result(
            "promotion_created",
            created_at,
            created_at,
        ));
        assert!(!admission_time_matches_result(
            "promotion_created",
            recovered_at,
            created_at,
        ));
        assert!(admission_time_matches_result(
            "promotion_recovered",
            recovered_at,
            created_at,
        ));
    }

    #[test]
    fn historical_authority_time_matches_postgres_precision() {
        let base = database_now();
        assert!(same_postgres_timestamp(
            base + Duration::nanoseconds(101),
            base + Duration::nanoseconds(999),
        ));
        assert!(!same_postgres_timestamp(
            base + Duration::nanoseconds(999),
            base + Duration::microseconds(1),
        ));
    }
}
