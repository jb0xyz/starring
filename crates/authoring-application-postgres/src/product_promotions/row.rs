use std::fmt::{Debug, Formatter};

use authoring_application::AuthorizedPromotionSubmissionErrorV1;
use authoring_promotion::{
    plan_activation_link_v1, plan_approval_environment_v1, plan_ruleset_publication_v1,
    validate_exact_planned_record_v1, LinkedActivationTransitionV1, PendingActivationDispositionV1,
    PendingActivationProposalV1, PreparedPromotionPlanV1, PromotionRecordV1, PromotionRevision,
    PromotionStageV1, PublicationDispositionV1, PublicationPortOutcomeV1, PublicationRecordV1,
    PublishedAuthoringRuleSetV1, ResolvedProductApprovalContextV1,
};
use automation_ruleset::{content_hash, RuleSetVersion, RuleSetVersionId};
use automation_ruleset_activation::{
    validate_product_target_v1, ActivationApprovalContextV1, ActivationLinkStateV1,
    ActivationRequest, ActivationRequestState, ApprovalBindingContextV1, ExpectedActiveBaselineV1,
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use desired_state::ResourceKey;
use discord_model::Permissions;
use resource_resolution::{
    approval_binding_fingerprint_v1, resource_binding_fingerprint_v2, ResolvedApprovalBinding,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;
use sqlx::types::Json;
use subtle::ConstantTimeEq;

use crate::bindings::decode_resource_bindings;
use crate::product_action_digest::ProductActionDigestKeyringV1;

use super::admission::{
    validate_product_promotion_admission_v1, PreparedProductPromotionAdmissionV1,
    ProductPromotionAdmissionContextV1, ProductPromotionAdmissionEvidenceV1,
};
use super::authorization::ProductPromotionAccessArgsV1;
use super::digest::{promotion_action_ids_v1, ProductPromotionDigestsV1};

const MAX_PROMOTION_RECORD_BYTES: usize = 8_388_608;
const MAX_ADMISSION_BYTES: usize = 32_768;
const MAX_RESOURCE_BINDINGS_BYTES: usize = 262_144;
const MAX_RULESET_DEFINITION_BYTES: usize = 524_288;
const MAX_TARGET_ARTIFACT_BYTES: usize = 1_048_576;
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

#[derive(sqlx::FromRow)]
pub(super) struct ProductPromotionPublicationRowV1 {
    pub outcome_code: String,
    pub publication_projection: Option<Json<Value>>,
    pub promotion_record: Option<Json<Value>>,
    pub database_now: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub(super) struct ProductPromotionApprovalEnvironmentRowV1 {
    pub outcome_code: String,
    pub promotion_record: Option<Json<Value>>,
    pub historical_binding_revision: Option<i64>,
    pub historical_resource_bindings: Option<Json<Value>>,
    pub historical_binding_fingerprint: Option<String>,
    pub active_version: Option<i64>,
    pub active_content_hash: Option<String>,
    pub target_artifact_projection: Option<Json<Value>>,
    pub database_now: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub(super) struct ProductPromotionActivationLinkRowV1 {
    pub outcome_code: String,
    pub promotion_record: Option<Json<Value>>,
    pub admission_evidence: Option<Json<Value>>,
    pub admission_digest: Option<String>,
    pub activation_projection: Option<Json<Value>>,
    pub receipt_projection: Option<Json<Value>>,
    pub audit_evidence_projection: Option<Json<Value>>,
    pub database_now: DateTime<Utc>,
}

pub(crate) struct ProductPromotionAdmittedStageV1 {
    pub(crate) record: PromotionRecordV1,
    pub(crate) admission: ProductPromotionAdmissionEvidenceV1,
    pub(crate) admission_digest: String,
    pub(crate) database_now: DateTime<Utc>,
}

pub(crate) enum ProductPromotionPublishStageV1 {
    Published(Box<ProductPromotionAdmittedStageV1>),
    FinalReplayRequired(Box<ProductPromotionAdmittedStageV1>),
}

pub(crate) enum ProductPromotionActivationStageV1 {
    Finalized(Box<ProductPromotionFinalReplayV1>),
    FinalReplayRequired(Box<ProductPromotionAdmittedStageV1>),
}

pub(crate) enum ProductPromotionLegacyRepairStageV1 {
    Finalized(Box<ProductPromotionFinalReplayV1>),
    FinalReplayRequired(Box<ProductPromotionAdmittedStageV1>),
}

impl Debug for ProductPromotionActivationStageV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Finalized(value) => formatter
                .debug_tuple("ProductPromotionActivationStageV1::Finalized")
                .field(value)
                .finish(),
            Self::FinalReplayRequired(value) => formatter
                .debug_tuple("ProductPromotionActivationStageV1::FinalReplayRequired")
                .field(value)
                .finish(),
        }
    }
}

impl Debug for ProductPromotionLegacyRepairStageV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Finalized(value) => formatter
                .debug_tuple("ProductPromotionLegacyRepairStageV1::Finalized")
                .field(value)
                .finish(),
            Self::FinalReplayRequired(value) => formatter
                .debug_tuple("ProductPromotionLegacyRepairStageV1::FinalReplayRequired")
                .field(value)
                .finish(),
        }
    }
}

impl Debug for ProductPromotionPublishStageV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Published(value) => formatter
                .debug_tuple("ProductPromotionPublishStageV1::Published")
                .field(value)
                .finish(),
            Self::FinalReplayRequired(value) => formatter
                .debug_tuple("ProductPromotionPublishStageV1::FinalReplayRequired")
                .field(value)
                .finish(),
        }
    }
}

pub(crate) struct ProductPromotionApprovalEnvironmentStageV1 {
    pub(crate) admitted: ProductPromotionAdmittedStageV1,
    pub(crate) resolved: ResolvedProductApprovalContextV1,
    pub(crate) target_artifact: RuleSetVersion,
}

pub(super) struct ProductPromotionPublicationDecodedV1 {
    pub(super) record: PromotionRecordV1,
    pub(super) database_now: DateTime<Utc>,
    pub(super) final_replay_required: bool,
}

pub(super) struct ProductPromotionApprovalEnvironmentDecodedV1 {
    pub(super) resolved: ResolvedProductApprovalContextV1,
    pub(super) target_artifact: RuleSetVersion,
    pub(super) database_now: DateTime<Utc>,
}

impl Debug for ProductPromotionApprovalEnvironmentStageV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProductPromotionApprovalEnvironmentStageV1")
            .field("admitted", &self.admitted)
            .field("resolved", &"<redacted>")
            .field("target_artifact", &"<redacted>")
            .finish()
    }
}

impl Debug for ProductPromotionAdmittedStageV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProductPromotionAdmittedStageV1")
            .field("record", &"<redacted>")
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
            .field("record", &"<redacted>")
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

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductPromotionPublicationProjectionV1 {
    format_version: u16,
    disposition: PublicationDispositionV1,
    artifact: RuleSetVersion,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductPromotionTargetArtifactProjectionV1 {
    format_version: u16,
    artifact: RuleSetVersion,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductPromotionActivationProjectionV1 {
    format_version: u16,
    disposition: PendingActivationDispositionV1,
    request: ActivationRequest,
}

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
        "idempotency_conflict" => {
            require_all_absent(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::IdempotencyConflict)
        }
        "access_denied" => {
            require_all_absent(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::Forbidden)
        }
        "scope_mismatch" => {
            require_all_absent(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::ScopeMismatch)
        }
        "persistence_corrupt" => {
            require_all_absent(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        }
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
        "idempotency_conflict" => {
            require_prepare_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::IdempotencyConflict)
        }
        "generation_mismatch" => {
            require_prepare_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::GenerationMismatch)
        }
        "access_denied" => {
            require_prepare_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::Forbidden)
        }
        "scope_mismatch" => {
            require_prepare_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::ScopeMismatch)
        }
        "invalid_candidate" => {
            require_prepare_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::InvalidCandidate)
        }
        "persistence_corrupt" => {
            require_prepare_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        }
        _ => Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt),
    }
}

pub(super) fn validate_product_promotion_admitted_for_access_v1(
    admitted: &ProductPromotionAdmittedStageV1,
    keyring: &ProductActionDigestKeyringV1,
    context: &ProductPromotionAdmissionContextV1,
    access: &ProductPromotionAccessArgsV1,
    digests: &ProductPromotionDigestsV1,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    validate_product_promotion_admission_v1(
        keyring,
        &admitted.admission,
        &admitted.admission_digest,
    )
    .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    validate_admitted_projection_v1(
        &admitted.record,
        &admitted.admission,
        admitted.database_now,
        keyring,
        context,
        access,
        digests,
    )
}

pub(super) fn validate_product_promotion_legacy_for_access_v1(
    legacy: &ProductPromotionLegacyRepairV1,
    context: &ProductPromotionAdmissionContextV1,
    access: &ProductPromotionAccessArgsV1,
    digests: &ProductPromotionDigestsV1,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    validate_legacy_repair_record_v1(
        &legacy.record,
        legacy.database_now,
        context,
        access,
        digests,
    )
}

pub(super) fn decode_product_promotion_publication_v1(
    row: ProductPromotionPublicationRowV1,
    admitted: &ProductPromotionAdmittedStageV1,
) -> Result<ProductPromotionPublicationDecodedV1, AuthorizedPromotionSubmissionErrorV1> {
    match row.outcome_code.as_str() {
        "created" | "reused" | "published_exact" | "final_exact" => {
            let projection =
                decode_bounded_v1(row.publication_projection, MAX_TARGET_ARTIFACT_BYTES)?;
            let persisted = decode_bounded_v1(row.promotion_record, MAX_PROMOTION_RECORD_BYTES)?;
            validate_publication_success_v1(
                row.outcome_code.as_str(),
                &projection,
                &persisted,
                row.database_now,
                &admitted.record,
            )?;
            Ok(ProductPromotionPublicationDecodedV1 {
                final_replay_required: row.outcome_code == "final_exact",
                record: persisted,
                database_now: row.database_now,
            })
        }
        "access_denied" => {
            require_publication_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::Forbidden)
        }
        "scope_mismatch" => {
            require_publication_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::ScopeMismatch)
        }
        "persistence_corrupt" => {
            require_publication_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        }
        _ => Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt),
    }
}

pub(super) fn decode_product_promotion_approval_environment_v1(
    row: ProductPromotionApprovalEnvironmentRowV1,
    admitted: &ProductPromotionAdmittedStageV1,
) -> Result<ProductPromotionApprovalEnvironmentDecodedV1, AuthorizedPromotionSubmissionErrorV1> {
    match row.outcome_code.as_str() {
        "resolved" => validate_approval_environment_success_v1(row, &admitted.record),
        "access_denied" => {
            require_approval_environment_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::Forbidden)
        }
        "scope_mismatch" => {
            require_approval_environment_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::ScopeMismatch)
        }
        "persistence_corrupt" => {
            require_approval_environment_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        }
        _ => Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt),
    }
}

pub(super) fn decode_product_promotion_activation_link_v1(
    row: ProductPromotionActivationLinkRowV1,
    keyring: &ProductActionDigestKeyringV1,
    context: &ProductPromotionAdmissionContextV1,
    access: &ProductPromotionAccessArgsV1,
    digests: &ProductPromotionDigestsV1,
    environment: &ProductPromotionApprovalEnvironmentStageV1,
    proposal: &PendingActivationProposalV1,
) -> Result<ProductPromotionActivationStageV1, AuthorizedPromotionSubmissionErrorV1> {
    match row.outcome_code.as_str() {
        "created" | "reused" => validate_activation_link_success_v1(
            row,
            keyring,
            context,
            access,
            digests,
            environment,
            proposal,
        )
        .map(|value| ProductPromotionActivationStageV1::Finalized(Box::new(value))),
        "final_replay_required" => validate_activation_final_replay_signal_v1(
            row,
            keyring,
            context,
            access,
            digests,
            environment,
        )
        .map(|value| ProductPromotionActivationStageV1::FinalReplayRequired(Box::new(value))),
        "idempotency_conflict" => {
            require_activation_link_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::IdempotencyConflict)
        }
        "access_denied" => {
            require_activation_link_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::Forbidden)
        }
        "scope_mismatch" => {
            require_activation_link_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::ScopeMismatch)
        }
        "persistence_corrupt" => {
            require_activation_link_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        }
        _ => Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt),
    }
}

pub(super) fn decode_product_promotion_repair_link_v1(
    row: ProductPromotionActivationLinkRowV1,
    keyring: &ProductActionDigestKeyringV1,
    context: &ProductPromotionAdmissionContextV1,
    access: &ProductPromotionAccessArgsV1,
    digests: &ProductPromotionDigestsV1,
    legacy: &ProductPromotionLegacyRepairV1,
    prepared_admission: &PreparedProductPromotionAdmissionV1,
) -> Result<ProductPromotionLegacyRepairStageV1, AuthorizedPromotionSubmissionErrorV1> {
    match row.outcome_code.as_str() {
        "recovered" => validate_legacy_repair_success_v1(
            row,
            keyring,
            context,
            access,
            digests,
            legacy,
            prepared_admission,
        )
        .map(|value| ProductPromotionLegacyRepairStageV1::Finalized(Box::new(value))),
        "final_replay_required" => validate_legacy_repair_final_replay_signal_v1(
            row, keyring, context, access, digests, legacy,
        )
        .map(|value| ProductPromotionLegacyRepairStageV1::FinalReplayRequired(Box::new(value))),
        "not_found" => {
            require_activation_link_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::NotFound)
        }
        "idempotency_conflict" => {
            require_activation_link_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::IdempotencyConflict)
        }
        "access_denied" => {
            require_activation_link_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::Forbidden)
        }
        "scope_mismatch" => {
            require_activation_link_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::ScopeMismatch)
        }
        "persistence_corrupt" => {
            require_activation_link_absent_v1(&row)?;
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        }
        _ => Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt),
    }
}

fn validate_legacy_repair_success_v1(
    row: ProductPromotionActivationLinkRowV1,
    keyring: &ProductActionDigestKeyringV1,
    context: &ProductPromotionAdmissionContextV1,
    access: &ProductPromotionAccessArgsV1,
    digests: &ProductPromotionDigestsV1,
    legacy: &ProductPromotionLegacyRepairV1,
    prepared_admission: &PreparedProductPromotionAdmissionV1,
) -> Result<ProductPromotionFinalReplayV1, AuthorizedPromotionSubmissionErrorV1> {
    let activation = decode_bounded_v1::<ProductPromotionActivationProjectionV1>(
        row.activation_projection,
        MAX_TARGET_ARTIFACT_BYTES,
    )?;
    let receipt = decode_bounded_v1(row.receipt_projection, MAX_RECEIPT_BYTES)?;
    let audit_evidence =
        decode_bounded_v1(row.audit_evidence_projection, MAX_AUDIT_EVIDENCE_BYTES)?;
    let validation = ProductPromotionValidationContextV1 {
        keyring,
        context,
        access,
        digests,
    };
    let admitted = decode_admitted_v1(
        row.promotion_record,
        row.admission_evidence,
        row.admission_digest,
        row.database_now,
        &validation,
    )?;
    if admitted.admission.payload != prepared_admission.payload
        || !constant_time_text_eq(&admitted.admission_digest, &prepared_admission.digest)
        || admitted.admission.admitted_at != admitted.database_now
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    validate_legacy_repair_activation_projection_v1(
        legacy,
        &activation,
        &admitted.record,
        admitted.database_now,
    )?;
    validate_new_recovery_receipt_v1(&admitted.record, &receipt, admitted.database_now)?;
    validate_final_projection_v1(&admitted, &receipt, &audit_evidence)?;
    require_recovery_result_v1(&receipt)?;
    Ok(ProductPromotionFinalReplayV1 {
        admitted,
        receipt,
        audit_evidence,
    })
}

fn validate_new_recovery_receipt_v1(
    record: &PromotionRecordV1,
    receipt: &ProductPromotionReceiptProjectionV1,
    database_now: DateTime<Utc>,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    let exact = match &record.stage {
        PromotionStageV1::ActivationPending { .. } => {
            record.revision.get() == 3
                && receipt.resulting_state == "activation_pending"
                && receipt.resulting_revision == Some(3)
                && receipt.completed_at == database_now
        }
        PromotionStageV1::Expired { .. } => {
            record.revision.get() == 4
                && record.updated_at == database_now
                && receipt.resulting_state == "expired"
                && receipt.resulting_revision == Some(4)
                && receipt.completed_at == database_now
        }
        PromotionStageV1::Prepared | PromotionStageV1::Published { .. } => false,
    };
    if exact {
        Ok(())
    } else {
        Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
    }
}

fn validate_legacy_repair_final_replay_signal_v1(
    row: ProductPromotionActivationLinkRowV1,
    keyring: &ProductActionDigestKeyringV1,
    context: &ProductPromotionAdmissionContextV1,
    access: &ProductPromotionAccessArgsV1,
    digests: &ProductPromotionDigestsV1,
    legacy: &ProductPromotionLegacyRepairV1,
) -> Result<ProductPromotionAdmittedStageV1, AuthorizedPromotionSubmissionErrorV1> {
    if row.activation_projection.is_some()
        || row.receipt_projection.is_some()
        || row.audit_evidence_projection.is_some()
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    let validation = ProductPromotionValidationContextV1 {
        keyring,
        context,
        access,
        digests,
    };
    let admitted = decode_admitted_v1(
        row.promotion_record,
        row.admission_evidence,
        row.admission_digest,
        row.database_now,
        &validation,
    )?;
    validate_legacy_final_replay_record_v1(&legacy.record, &admitted.record, row.database_now)?;
    Ok(admitted)
}

fn validate_legacy_repair_activation_projection_v1(
    legacy: &ProductPromotionLegacyRepairV1,
    projection: &ProductPromotionActivationProjectionV1,
    persisted: &PromotionRecordV1,
    database_now: DateTime<Utc>,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    if projection.format_version != 1
        || legacy.database_now > database_now
        || legacy.record.updated_at > database_now
        || persisted.updated_at > database_now
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    projection
        .request
        .validate()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let transition = plan_activation_link_v1(&legacy.record)
        .and_then(|plan| plan.complete(&legacy.record, &projection.request, database_now))
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let expected = match transition {
        LinkedActivationTransitionV1::Linked { expected_record } => expected_record,
        LinkedActivationTransitionV1::Expired {
            expected_record, ..
        } => expected_record,
    };
    validate_exact_planned_record_v1(&expected, persisted)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let activation = match &persisted.stage {
        PromotionStageV1::ActivationPending { activation, .. }
        | PromotionStageV1::Expired { activation, .. } => activation,
        PromotionStageV1::Prepared | PromotionStageV1::Published { .. } => {
            return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        }
    };
    if projection.disposition != activation.disposition {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn validate_legacy_final_replay_record_v1(
    legacy: &PromotionRecordV1,
    final_record: &PromotionRecordV1,
    database_now: DateTime<Utc>,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    legacy
        .validate()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    final_record
        .validate()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let PromotionStageV1::ActivationPending {
        publication: legacy_publication,
        activation: legacy_activation,
    } = &legacy.stage
    else {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    };
    let exact_stage = match &final_record.stage {
        PromotionStageV1::ActivationPending { .. } => final_record == legacy,
        PromotionStageV1::Expired {
            publication,
            activation,
        } => {
            final_record.revision.get() == 4
                && publication == legacy_publication
                && activation.request_id == legacy_activation.request_id
                && activation.target == legacy_activation.target
                && activation.requester == legacy_activation.requester
                && activation.required_approvals == legacy_activation.required_approvals
                && activation.observed_active == legacy_activation.observed_active
                && activation.created_at == legacy_activation.created_at
                && activation.expires_at == legacy_activation.expires_at
                && activation.disposition == PendingActivationDispositionV1::Reused
                && activation.request_state_at_journal == ActivationRequestState::Expired
                && activation.approval_context == legacy_activation.approval_context
        }
        PromotionStageV1::Prepared | PromotionStageV1::Published { .. } => false,
    };
    if !exact_stage
        || legacy.id != final_record.id
        || legacy.request_digest != final_record.request_digest
        || legacy.intent != final_record.intent
        || legacy.created_at != final_record.created_at
        || final_record.updated_at < legacy.updated_at
        || final_record.updated_at > database_now
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn validate_activation_final_replay_signal_v1(
    row: ProductPromotionActivationLinkRowV1,
    keyring: &ProductActionDigestKeyringV1,
    context: &ProductPromotionAdmissionContextV1,
    access: &ProductPromotionAccessArgsV1,
    digests: &ProductPromotionDigestsV1,
    environment: &ProductPromotionApprovalEnvironmentStageV1,
) -> Result<ProductPromotionAdmittedStageV1, AuthorizedPromotionSubmissionErrorV1> {
    if row.activation_projection.is_some()
        || row.receipt_projection.is_some()
        || row.audit_evidence_projection.is_some()
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    let validation = ProductPromotionValidationContextV1 {
        keyring,
        context,
        access,
        digests,
    };
    let admitted = decode_admitted_v1(
        row.promotion_record,
        row.admission_evidence,
        row.admission_digest,
        row.database_now,
        &validation,
    )?;
    if environment.admitted.admission != admitted.admission
        || !constant_time_text_eq(
            &environment.admitted.admission_digest,
            &admitted.admission_digest,
        )
        || admitted.admission.admitted_at != admitted.record.created_at
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    validate_final_replay_signal_record_v1(
        &environment.admitted.record,
        &admitted.record,
        row.database_now,
    )?;
    Ok(admitted)
}

fn validate_final_replay_signal_record_v1(
    published: &PromotionRecordV1,
    final_record: &PromotionRecordV1,
    database_now: DateTime<Utc>,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    published
        .validate()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    final_record
        .validate()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let PromotionStageV1::Published {
        publication: published_artifact,
    } = &published.stage
    else {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    };
    let (final_artifact, valid_revision) = match &final_record.stage {
        PromotionStageV1::ActivationPending { publication, .. } => {
            (publication, final_record.revision.get() == 3)
        }
        PromotionStageV1::Expired { publication, .. } => {
            (publication, matches!(final_record.revision.get(), 3 | 4))
        }
        PromotionStageV1::Prepared | PromotionStageV1::Published { .. } => {
            return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        }
    };
    if !valid_revision
        || published.id != final_record.id
        || published.request_digest != final_record.request_digest
        || published.intent != final_record.intent
        || published.created_at != final_record.created_at
        || published_artifact != final_artifact
        || final_record.updated_at < published.updated_at
        || final_record.updated_at > database_now
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn validate_activation_link_success_v1(
    row: ProductPromotionActivationLinkRowV1,
    keyring: &ProductActionDigestKeyringV1,
    context: &ProductPromotionAdmissionContextV1,
    access: &ProductPromotionAccessArgsV1,
    digests: &ProductPromotionDigestsV1,
    environment: &ProductPromotionApprovalEnvironmentStageV1,
    proposal: &PendingActivationProposalV1,
) -> Result<ProductPromotionFinalReplayV1, AuthorizedPromotionSubmissionErrorV1> {
    let outcome_code = row.outcome_code.clone();
    let activation = decode_bounded_v1::<ProductPromotionActivationProjectionV1>(
        row.activation_projection,
        MAX_TARGET_ARTIFACT_BYTES,
    )?;
    let receipt = decode_bounded_v1(row.receipt_projection, MAX_RECEIPT_BYTES)?;
    let audit_evidence =
        decode_bounded_v1(row.audit_evidence_projection, MAX_AUDIT_EVIDENCE_BYTES)?;
    let validation = ProductPromotionValidationContextV1 {
        keyring,
        context,
        access,
        digests,
    };
    let admitted = decode_admitted_v1(
        row.promotion_record,
        row.admission_evidence,
        row.admission_digest,
        row.database_now,
        &validation,
    )?;
    if environment.admitted.admission != admitted.admission
        || !constant_time_text_eq(
            &environment.admitted.admission_digest,
            &admitted.admission_digest,
        )
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    validate_activation_projection_v1(
        &outcome_code,
        &environment.admitted.record,
        proposal,
        &activation,
        &admitted.record,
        row.database_now,
    )?;
    validate_final_projection_v1(&admitted, &receipt, &audit_evidence)?;
    require_normal_activation_result_v1(&receipt)?;
    Ok(ProductPromotionFinalReplayV1 {
        admitted,
        receipt,
        audit_evidence,
    })
}

fn require_normal_activation_result_v1(
    receipt: &ProductPromotionReceiptProjectionV1,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    if receipt.result_code == "promotion_created" {
        Ok(())
    } else {
        Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
    }
}

fn require_recovery_result_v1(
    receipt: &ProductPromotionReceiptProjectionV1,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    if receipt.result_code == "promotion_recovered" {
        Ok(())
    } else {
        Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
    }
}

fn validate_activation_projection_v1(
    outcome_code: &str,
    published: &PromotionRecordV1,
    proposal: &PendingActivationProposalV1,
    projection: &ProductPromotionActivationProjectionV1,
    persisted: &PromotionRecordV1,
    database_now: DateTime<Utc>,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    published
        .validate()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    persisted
        .validate()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let PromotionStageV1::Published { .. } = &published.stage else {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    };
    if projection.format_version != 1
        || published.updated_at > database_now
        || persisted.updated_at > database_now
        || persisted.updated_at < published.updated_at
        || (matches!(outcome_code, "created" | "reused") && persisted.updated_at != database_now)
        || (outcome_code == "created"
            && projection.disposition != PendingActivationDispositionV1::Created)
        || (outcome_code == "reused"
            && projection.disposition != PendingActivationDispositionV1::Reused)
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    projection
        .request
        .validate()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let expected = proposal.request().create;
    let request = &projection.request;
    if request.id != expected.id
        || request.target != expected.target
        || request.requester != expected.requester
        || request.approval_context
            != (ActivationApprovalContextV1::ProductAuthoring {
                context: Box::new(expected.context.clone()),
            })
        || request.required_approvals != expected.context.policy.required_approvals.get()
        || request.observed_active != expected.context.baseline.as_observed()
        || request.created_at < published.updated_at
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    let journal_activation = match &persisted.stage {
        PromotionStageV1::ActivationPending { activation, .. } => {
            if request.state == ActivationRequestState::Expired
                || !matches!(request.link_state, ActivationLinkStateV1::Linked { .. })
            {
                return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
            }
            if matches!(outcome_code, "created" | "reused")
                && (request.state != ActivationRequestState::Pending
                    || !activation_request_is_pristine_v1(request)
                    || !matches!(
                        request.link_state,
                        ActivationLinkStateV1::Linked { linked_at }
                            if linked_at == persisted.updated_at
                    ))
            {
                return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
            }
            let transition = plan_activation_link_v1(persisted)
                .and_then(|plan| plan.complete(persisted, request, database_now))
                .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
            match transition {
                LinkedActivationTransitionV1::Linked { expected_record }
                    if expected_record.as_ref() == persisted => {}
                LinkedActivationTransitionV1::Linked { .. }
                | LinkedActivationTransitionV1::Expired { .. } => {
                    return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
                }
            }
            activation
        }
        PromotionStageV1::Expired { activation, .. } => {
            if request.state != ActivationRequestState::Expired {
                return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
            }
            if matches!(outcome_code, "created" | "reused")
                && (!activation_request_is_pristine_v1(request)
                    || request.link_state != ActivationLinkStateV1::Unlinked
                    || outcome_code == "created")
            {
                return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
            }
            activation
        }
        PromotionStageV1::Prepared | PromotionStageV1::Published { .. } => {
            return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        }
    };
    if journal_activation.request_id != request.id
        || journal_activation.target != request.target
        || journal_activation.requester != request.requester
        || journal_activation.required_approvals != expected.context.policy.required_approvals
        || journal_activation.observed_active != request.observed_active
        || journal_activation.created_at != request.created_at
        || journal_activation.expires_at != request.expires_at
        || journal_activation.disposition != projection.disposition
        || journal_activation.approval_context != expected.context
        || journal_activation.request_state_at_journal
            != match &persisted.stage {
                PromotionStageV1::ActivationPending { .. } => ActivationRequestState::Pending,
                PromotionStageV1::Expired { .. } => ActivationRequestState::Expired,
                PromotionStageV1::Prepared | PromotionStageV1::Published { .. } => {
                    return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
                }
            }
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    let expected_record = match &persisted.stage {
        PromotionStageV1::ActivationPending { .. } => published
            .transition_to_activation_pending(
                published.revision,
                journal_activation.clone(),
                persisted.updated_at,
            )
            .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?,
        PromotionStageV1::Expired { .. } => published
            .transition_to_expired(
                published.revision,
                journal_activation.clone(),
                persisted.updated_at,
            )
            .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?,
        PromotionStageV1::Prepared | PromotionStageV1::Published { .. } => {
            return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        }
    };
    validate_exact_planned_record_v1(&expected_record, persisted)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
}

fn activation_request_is_pristine_v1(request: &ActivationRequest) -> bool {
    request.approvals.is_empty()
        && request.rejection.is_none()
        && request.apply_attempt_id.is_none()
        && request.apply_attempt_no == 0
        && request.apply_lease_until.is_none()
        && request.last_apply_error.is_none()
        && request.completion.is_none()
        && request.termination.is_none()
}

fn validate_publication_success_v1(
    outcome_code: &str,
    projection: &ProductPromotionPublicationProjectionV1,
    persisted: &PromotionRecordV1,
    database_now: DateTime<Utc>,
    admitted: &PromotionRecordV1,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    admitted
        .validate()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    persisted
        .validate()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    if projection.format_version != 1
        || admitted.updated_at > database_now
        || persisted.updated_at > database_now
        || persisted.updated_at < admitted.updated_at
        || (matches!(outcome_code, "created" | "reused")
            && admitted.stage != PromotionStageV1::Prepared)
        || (outcome_code == "created"
            && projection.disposition != PublicationDispositionV1::Created)
        || (outcome_code == "reused" && projection.disposition != PublicationDispositionV1::Reused)
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    let prepared = publication_planning_basis_v1(admitted)?;
    let proposal = plan_ruleset_publication_v1(&prepared)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    validate_ruleset_artifact_v1(&proposal.request(), &projection.artifact)?;
    let artifact = PublishedAuthoringRuleSetV1 {
        guild_id: projection.artifact.guild_id,
        ruleset_key: projection.artifact.ruleset_key.clone(),
        version: projection.artifact.version,
        schema_version: projection.artifact.schema_version,
        definition: projection.artifact.definition.clone(),
        content_hash: projection.artifact.content_hash,
        created_by: projection.artifact.created_by,
    };
    let port_outcome = match projection.disposition {
        PublicationDispositionV1::Created => PublicationPortOutcomeV1::Created(artifact),
        PublicationDispositionV1::Reused => PublicationPortOutcomeV1::Reused(artifact),
    };
    let transition = proposal
        .complete(&prepared, port_outcome, persisted.updated_at)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    match outcome_code {
        "created" | "reused" | "published_exact" => {
            validate_exact_planned_record_v1(&transition.expected_record, persisted)
                .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
            if matches!(&admitted.stage, PromotionStageV1::Published { .. })
                && admitted != persisted
            {
                return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
            }
            if matches!(outcome_code, "created" | "reused") && persisted.updated_at != database_now
            {
                return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
            }
        }
        "final_exact" => {
            let publication = final_publication_v1(persisted)?;
            if publication != &transition.publication
                || persisted.id != prepared.id
                || persisted.request_digest != prepared.request_digest
                || persisted.intent != prepared.intent
                || persisted.created_at != prepared.created_at
            {
                return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
            }
            if is_final_stage(&admitted.stage) && admitted != persisted {
                return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
            }
        }
        _ => return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt),
    }
    Ok(())
}

fn validate_approval_environment_success_v1(
    row: ProductPromotionApprovalEnvironmentRowV1,
    admitted: &PromotionRecordV1,
) -> Result<ProductPromotionApprovalEnvironmentDecodedV1, AuthorizedPromotionSubmissionErrorV1> {
    admitted
        .validate()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    if admitted.updated_at > row.database_now {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    let persisted =
        decode_bounded_v1::<PromotionRecordV1>(row.promotion_record, MAX_PROMOTION_RECORD_BYTES)?;
    if &persisted != admitted {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    let published = approval_environment_planning_basis_v1(admitted)?;
    let plan = plan_approval_environment_v1(&published)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let request = plan.request();
    let historical_binding_revision = row
        .historical_binding_revision
        .and_then(|value| u64::try_from(value).ok())
        .and_then(std::num::NonZeroU64::new)
        .ok_or(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    if historical_binding_revision.get() != request.binding_revision.get() {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    let historical_bindings_value = bounded_json_value_v1(
        row.historical_resource_bindings,
        MAX_RESOURCE_BINDINGS_BYTES,
    )?;
    let historical_bindings = decode_resource_bindings(historical_bindings_value)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let historical_binding_fingerprint = row
        .historical_binding_fingerprint
        .ok_or(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    if resource_binding_fingerprint_v2(&historical_bindings).as_str()
        != historical_binding_fingerprint
        || request.context_fingerprint.as_str() != historical_binding_fingerprint
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    let required_bindings = request
        .required_channel_bindings
        .iter()
        .map(|key| {
            let key = ResourceKey(key.clone());
            historical_bindings
                .channel_bindings
                .get(&key)
                .copied()
                .map(|id| ResolvedApprovalBinding::Channel { key, id })
                .ok_or(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let approval_fingerprint = approval_binding_fingerprint_v1(
        request.target.guild_id,
        historical_binding_revision,
        &required_bindings,
    )
    .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let binding = ApprovalBindingContextV1 {
        revision: historical_binding_revision,
        required_bindings,
        fingerprint: approval_fingerprint,
    };
    if !binding.validate(request.target.guild_id) {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    let baseline = decode_active_baseline_v1(row.active_version, row.active_content_hash)?;
    let target_projection = decode_bounded_v1::<ProductPromotionTargetArtifactProjectionV1>(
        row.target_artifact_projection,
        MAX_TARGET_ARTIFACT_BYTES,
    )?;
    if target_projection.format_version != 1 {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    validate_target_artifact_v1(&published, &request.target, &target_projection.artifact)?;
    Ok(ProductPromotionApprovalEnvironmentDecodedV1 {
        resolved: ResolvedProductApprovalContextV1 { binding, baseline },
        target_artifact: target_projection.artifact,
        database_now: row.database_now,
    })
}

fn publication_planning_basis_v1(
    record: &PromotionRecordV1,
) -> Result<PromotionRecordV1, AuthorizedPromotionSubmissionErrorV1> {
    match &record.stage {
        PromotionStageV1::Prepared => Ok(record.clone()),
        PromotionStageV1::Published { .. }
        | PromotionStageV1::ActivationPending { .. }
        | PromotionStageV1::Expired { .. } => {
            let prepared = PromotionRecordV1 {
                id: record.id.clone(),
                revision: PromotionRevision::FIRST,
                request_digest: record.request_digest.clone(),
                intent: record.intent.clone(),
                stage: PromotionStageV1::Prepared,
                created_at: record.created_at,
                updated_at: record.created_at,
            };
            prepared
                .validate()
                .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
            Ok(prepared)
        }
    }
}

fn approval_environment_planning_basis_v1(
    record: &PromotionRecordV1,
) -> Result<PromotionRecordV1, AuthorizedPromotionSubmissionErrorV1> {
    let publication = match &record.stage {
        PromotionStageV1::Published { publication }
        | PromotionStageV1::ActivationPending { publication, .. }
        | PromotionStageV1::Expired { publication, .. } => publication.clone(),
        PromotionStageV1::Prepared => {
            return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        }
    };
    let published = PromotionRecordV1 {
        id: record.id.clone(),
        revision: PromotionRevision::new(2)
            .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?,
        request_digest: record.request_digest.clone(),
        intent: record.intent.clone(),
        stage: PromotionStageV1::Published { publication },
        created_at: record.created_at,
        updated_at: record.updated_at,
    };
    published
        .validate()
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    Ok(published)
}

fn final_publication_v1(
    record: &PromotionRecordV1,
) -> Result<&PublicationRecordV1, AuthorizedPromotionSubmissionErrorV1> {
    match &record.stage {
        PromotionStageV1::ActivationPending { publication, .. }
        | PromotionStageV1::Expired { publication, .. } => Ok(publication),
        PromotionStageV1::Prepared | PromotionStageV1::Published { .. } => {
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        }
    }
}

fn validate_ruleset_artifact_v1(
    expected: &authoring_promotion::PublishAuthoringRuleSetV1,
    artifact: &RuleSetVersion,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    let encoded_definition = serde_json::to_vec(&artifact.definition)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let computed = content_hash(artifact.schema_version, &artifact.definition)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    if encoded_definition.len() > MAX_RULESET_DEFINITION_BYTES
        || artifact.guild_id != expected.guild_id
        || artifact.created_by.0 == 0
        || artifact.ruleset_key != expected.ruleset_key
        || artifact.definition != expected.definition
        || artifact.content_hash != computed
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn validate_target_artifact_v1(
    published: &PromotionRecordV1,
    target: &automation_ruleset_activation::ActivationTarget,
    artifact: &RuleSetVersion,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    validate_product_target_v1(target, artifact)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let publication = match &published.stage {
        PromotionStageV1::Published { publication } => publication,
        PromotionStageV1::Prepared
        | PromotionStageV1::ActivationPending { .. }
        | PromotionStageV1::Expired { .. } => {
            return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        }
    };
    let encoded_definition = serde_json::to_vec(&artifact.definition)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let computed = content_hash(artifact.schema_version, &artifact.definition)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    if encoded_definition.len() > MAX_RULESET_DEFINITION_BYTES
        || artifact.created_by.0 == 0
        || artifact.schema_version != publication.schema_version
        || artifact.content_hash != publication.content_hash
        || artifact.created_by != publication.registry_created_by
        || artifact.definition != published.intent.definition
        || computed != artifact.content_hash
    {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn decode_active_baseline_v1(
    active_version: Option<i64>,
    active_content_hash: Option<String>,
) -> Result<ExpectedActiveBaselineV1, AuthorizedPromotionSubmissionErrorV1> {
    match (active_version, active_content_hash) {
        (None, None) => Ok(ExpectedActiveBaselineV1::Absent),
        (Some(version), Some(content_hash)) => {
            let version = u32::try_from(version)
                .ok()
                .and_then(|value| RuleSetVersionId::new(value).ok())
                .ok_or(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
            let content_hash = automation_ruleset::RuleSetContentHash::parse_hex(&content_hash)
                .ok_or(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
            Ok(ExpectedActiveBaselineV1::Exact {
                version,
                content_hash,
            })
        }
        _ => Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt),
    }
}

fn require_publication_absent_v1(
    row: &ProductPromotionPublicationRowV1,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    if row.publication_projection.is_none() && row.promotion_record.is_none() {
        Ok(())
    } else {
        Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
    }
}

fn require_approval_environment_absent_v1(
    row: &ProductPromotionApprovalEnvironmentRowV1,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    if row.promotion_record.is_none()
        && row.historical_binding_revision.is_none()
        && row.historical_resource_bindings.is_none()
        && row.historical_binding_fingerprint.is_none()
        && row.active_version.is_none()
        && row.active_content_hash.is_none()
        && row.target_artifact_projection.is_none()
    {
        Ok(())
    } else {
        Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
    }
}

fn require_activation_link_absent_v1(
    row: &ProductPromotionActivationLinkRowV1,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    if row.promotion_record.is_none()
        && row.admission_evidence.is_none()
        && row.admission_digest.is_none()
        && row.activation_projection.is_none()
        && row.receipt_projection.is_none()
        && row.audit_evidence_projection.is_none()
    {
        Ok(())
    } else {
        Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
    }
}

fn bounded_json_value_v1(
    value: Option<Json<Value>>,
    maximum: usize,
) -> Result<Value, AuthorizedPromotionSubmissionErrorV1> {
    let value = value.ok_or(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    let encoded = serde_json::to_vec(&value.0)
        .map_err(|_| AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)?;
    if encoded.len() > maximum {
        return Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt);
    }
    Ok(value.0)
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
            receipt.result_code.as_str(),
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
    result_code: &str,
) -> bool {
    match result_code {
        "promotion_created" => match (
            current_state,
            current_revision,
            receipt_state,
            receipt_revision,
        ) {
            ("activation_pending", 3, "activation_pending", Some(3))
            | ("expired", 3, "expired", Some(3)) => receipt_completed_at == current_updated_at,
            ("expired", 4, "activation_pending", Some(3)) => {
                receipt_completed_at <= current_updated_at
            }
            _ => false,
        },
        "promotion_recovered" => match (
            current_state,
            current_revision,
            receipt_state,
            receipt_revision,
        ) {
            ("activation_pending", 3, "activation_pending", Some(3)) => {
                receipt_completed_at >= current_updated_at
            }
            ("expired", 4, "expired", Some(4)) => receipt_completed_at == current_updated_at,
            ("expired", 4, "activation_pending", Some(3)) => {
                receipt_completed_at <= current_updated_at
            }
            _ => false,
        },
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

fn require_prepare_absent_v1(
    row: &ProductPromotionPrepareRowV1,
) -> Result<(), AuthorizedPromotionSubmissionErrorV1> {
    if row.promotion_record.is_some()
        || row.admission_evidence.is_some()
        || row.admission_digest.is_some()
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
    use authoring_promotion::{
        plan_pending_activation_v1, PendingActivationDispositionV1, PendingActivationReceiptV1,
        PendingActivationTransitionV1,
    };
    use automation_ruleset_activation::ActivationRequest;
    use discord_model::ChannelId;
    use serde_json::json;

    use super::super::admission::prepare_legacy_product_promotion_admission_v1;
    use super::super::prepare::postgres_tests::prepared_decoder_stage;
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

    fn publication_row(
        outcome_code: &str,
        publication_projection: Option<Json<Value>>,
        promotion_record: Option<Json<Value>>,
        database_now: DateTime<Utc>,
    ) -> ProductPromotionPublicationRowV1 {
        ProductPromotionPublicationRowV1 {
            outcome_code: outcome_code.to_string(),
            publication_projection,
            promotion_record,
            database_now,
        }
    }

    fn approval_environment_row(outcome_code: &str) -> ProductPromotionApprovalEnvironmentRowV1 {
        ProductPromotionApprovalEnvironmentRowV1 {
            outcome_code: outcome_code.to_string(),
            promotion_record: None,
            historical_binding_revision: None,
            historical_resource_bindings: None,
            historical_binding_fingerprint: None,
            active_version: None,
            active_content_hash: None,
            target_artifact_projection: None,
            database_now: database_now(),
        }
    }

    fn activation_link_row(outcome_code: &str) -> ProductPromotionActivationLinkRowV1 {
        ProductPromotionActivationLinkRowV1 {
            outcome_code: outcome_code.to_string(),
            promotion_record: None,
            admission_evidence: None,
            admission_digest: None,
            activation_projection: None,
            receipt_projection: None,
            audit_evidence_projection: None,
            database_now: database_now(),
        }
    }

    fn decoder_keyring() -> ProductActionDigestKeyringV1 {
        let active = crate::product_action_digest::ProductActionDigestKeyV1::from_bytes(
            "active-v2",
            std::array::from_fn(|index| 7_u8.wrapping_add(index as u8)),
        )
        .unwrap();
        let retired = crate::product_action_digest::ProductActionDigestKeyV1::from_bytes(
            "retired-v1",
            std::array::from_fn(|index| 113_u8.wrapping_add(index as u8)),
        )
        .unwrap();
        ProductActionDigestKeyringV1::new(active, [retired]).unwrap()
    }

    fn decoder_access(admitted: &ProductPromotionAdmittedStageV1) -> ProductPromotionAccessArgsV1 {
        let payload = &admitted.admission.payload;
        ProductPromotionAccessArgsV1 {
            expected_tenant_id: payload.tenant_id.clone(),
            expected_installation_id: payload.installation_id.clone(),
            expected_principal_id: payload.principal_id.clone(),
            expected_product_session_digest: vec![17; 32],
            expected_acting_user_id: payload.acting_user_id.clone(),
            expected_discord_application_id: payload.discord_application_id.clone(),
            expected_guild_id: payload.guild_id.clone(),
            expected_capability: payload.capability.clone(),
            observed_current_authority_revision: 1,
            observed_current_authority_payload_digest: "5".repeat(64),
            authority_observation_digest: "7".repeat(64),
            authority_observed_at: admitted.database_now - Duration::milliseconds(100),
            authority_expires_at: admitted.database_now + Duration::seconds(4),
            effective_permission_bits: Permissions::MANAGE_GUILD.bits().to_string(),
            guild_owner: false,
        }
    }

    fn decoder_context(
        admitted: &ProductPromotionAdmittedStageV1,
    ) -> ProductPromotionAdmissionContextV1 {
        ProductPromotionAdmissionContextV1 {
            product_request_id: admitted.admission.payload.product_request_id.clone(),
            authoring_session_id: admitted.record.intent.authority.session_id.clone(),
            generation: admitted.record.intent.authority.session_generation,
        }
    }

    fn decoder_digests(admitted: &ProductPromotionAdmittedStageV1) -> ProductPromotionDigestsV1 {
        let payload = &admitted.admission.payload;
        ProductPromotionDigestsV1 {
            promotion_id: admitted.record.id.clone(),
            active_idempotency: payload.idempotency_key_digest.clone(),
            idempotency_candidates: vec![payload.idempotency_key_digest.clone()],
            idempotency_candidate_key_ids: vec![payload.idempotency_digest_key_id.clone()],
            idempotency_candidate_key_fingerprints: vec![payload
                .idempotency_digest_key_fingerprint
                .clone()],
            active_key_id: payload.idempotency_digest_key_id.clone(),
            active_key_fingerprint: payload.idempotency_digest_key_fingerprint.clone(),
            semantic_request: payload.semantic_request_digest.clone(),
            receipt_id: payload.receipt_id.clone(),
            audit_event_id: payload.audit_event_id.clone(),
            session_subject: vec![17; 32],
        }
    }

    struct RepairDecoderCase {
        keyring: ProductActionDigestKeyringV1,
        access: ProductPromotionAccessArgsV1,
        context: ProductPromotionAdmissionContextV1,
        digests: ProductPromotionDigestsV1,
        legacy: ProductPromotionLegacyRepairV1,
        prepared: PreparedProductPromotionAdmissionV1,
        promotion_record: Value,
        admission_evidence: Value,
        activation_projection: Value,
        receipt_projection: Value,
        audit_evidence_projection: Value,
        database_now: DateTime<Utc>,
    }

    impl RepairDecoderCase {
        fn row(&self, outcome_code: &str) -> ProductPromotionActivationLinkRowV1 {
            ProductPromotionActivationLinkRowV1 {
                outcome_code: outcome_code.to_string(),
                promotion_record: Some(Json(self.promotion_record.clone())),
                admission_evidence: Some(Json(self.admission_evidence.clone())),
                admission_digest: Some(self.prepared.digest.clone()),
                activation_projection: Some(Json(self.activation_projection.clone())),
                receipt_projection: Some(Json(self.receipt_projection.clone())),
                audit_evidence_projection: Some(Json(self.audit_evidence_projection.clone())),
                database_now: self.database_now,
            }
        }

        fn final_replay_row(&self) -> ProductPromotionActivationLinkRowV1 {
            let mut row = self.row("final_replay_required");
            row.activation_projection = None;
            row.receipt_projection = None;
            row.audit_evidence_projection = None;
            row
        }

        fn decode(
            &self,
            row: ProductPromotionActivationLinkRowV1,
        ) -> Result<ProductPromotionLegacyRepairStageV1, AuthorizedPromotionSubmissionErrorV1>
        {
            decode_product_promotion_repair_link_v1(
                row,
                &self.keyring,
                &self.context,
                &self.access,
                &self.digests,
                &self.legacy,
                &self.prepared,
            )
        }
    }

    async fn repair_decoder_case(expire_directly: bool) -> RepairDecoderCase {
        let prepared_at = database_now();
        let admitted = prepared_decoder_stage(prepared_at).await;
        let publication_plan = plan_ruleset_publication_v1(&admitted.record).unwrap();
        let publication_request = publication_plan.request();
        let artifact = PublishedAuthoringRuleSetV1 {
            guild_id: publication_request.guild_id,
            ruleset_key: publication_request.ruleset_key,
            version: RuleSetVersionId::FIRST,
            schema_version: admitted.record.intent.registry_schema_version,
            definition: publication_request.definition,
            content_hash: admitted.record.intent.expected_registry_content_hash,
            created_by: publication_request.created_by,
        };
        let published_at = prepared_at + Duration::seconds(1);
        let published = publication_plan
            .complete(
                &admitted.record,
                PublicationPortOutcomeV1::Created(artifact),
                published_at,
            )
            .unwrap()
            .expected_record;
        let environment_request = plan_approval_environment_v1(&published).unwrap().request();
        let binding_revision =
            std::num::NonZeroU64::new(environment_request.binding_revision.get()).unwrap();
        let required_bindings = environment_request
            .required_channel_bindings
            .iter()
            .map(|key| ResolvedApprovalBinding::Channel {
                key: ResourceKey(key.clone()),
                id: ChannelId(700),
            })
            .collect::<Vec<_>>();
        let binding = ApprovalBindingContextV1 {
            revision: binding_revision,
            fingerprint: approval_binding_fingerprint_v1(
                environment_request.target.guild_id,
                binding_revision,
                &required_bindings,
            )
            .unwrap(),
            required_bindings,
        };
        let proposal = plan_pending_activation_v1(
            &published,
            ResolvedProductApprovalContextV1 {
                binding,
                baseline: ExpectedActiveBaselineV1::Absent,
            },
        )
        .unwrap();
        let activation_created_at = prepared_at + Duration::seconds(2);
        let mut request =
            ActivationRequest::create_product(proposal.request().create, activation_created_at)
                .unwrap();
        let pending = match proposal
            .complete(
                &published,
                &PendingActivationReceiptV1 {
                    request: request.clone(),
                    disposition: PendingActivationDispositionV1::Created,
                },
                activation_created_at,
            )
            .unwrap()
        {
            PendingActivationTransitionV1::ActivationPending {
                expected_record, ..
            } => expected_record,
            PendingActivationTransitionV1::Expired { .. }
            | PendingActivationTransitionV1::RefreshJournal => {
                panic!("expected activation-pending transition")
            }
        };
        let legacy = ProductPromotionLegacyRepairV1 {
            record: pending.clone(),
            database_now: activation_created_at,
        };
        let recovery_at = if expire_directly {
            request.expires_at + Duration::seconds(1)
        } else {
            activation_created_at + Duration::seconds(1)
        };
        let link_plan = plan_activation_link_v1(&pending).unwrap();
        if expire_directly {
            assert!(request.expire_if_due(recovery_at));
        } else {
            let link = link_plan.request();
            request
                .link_product_at(
                    &link.link.promotion_id,
                    &link.link.promotion_request_digest,
                    &link.link.approval_context_digest,
                    recovery_at,
                )
                .unwrap();
        }
        let final_record = match link_plan.complete(&pending, &request, recovery_at).unwrap() {
            LinkedActivationTransitionV1::Linked { expected_record }
            | LinkedActivationTransitionV1::Expired {
                expected_record, ..
            } => *expected_record,
        };
        let final_disposition = match &final_record.stage {
            PromotionStageV1::ActivationPending { activation, .. }
            | PromotionStageV1::Expired { activation, .. } => activation.disposition,
            PromotionStageV1::Prepared | PromotionStageV1::Published { .. } => {
                panic!("expected final promotion")
            }
        };
        let keyring = decoder_keyring();
        let mut access = decoder_access(&admitted);
        access.authority_observed_at = recovery_at - Duration::milliseconds(100);
        access.authority_expires_at = recovery_at + Duration::seconds(4);
        let mut context = decoder_context(&admitted);
        context.product_request_id = "repair-request".to_string();
        let digests = decoder_digests(&admitted);
        let prepared = prepare_legacy_product_promotion_admission_v1(
            &keyring,
            &context,
            &access,
            &legacy.record,
            &digests,
        )
        .unwrap();
        let evidence = ProductPromotionAdmissionEvidenceV1 {
            format_version: 1,
            payload: prepared.payload.clone(),
            admitted_at: recovery_at,
        };
        let state = stage_name(&final_record.stage);
        let revision = i64::try_from(final_record.revision.get()).unwrap();
        let payload = &prepared.payload;
        let receipt_projection = json!({
            "format_version": 1,
            "receipt_id": payload.receipt_id,
            "tenant_id": payload.tenant_id,
            "installation_id": payload.installation_id,
            "principal_id": payload.principal_id,
            "endpoint_domain": PROMOTION_ENDPOINT,
            "idempotency_key_digest": payload.idempotency_key_digest,
            "idempotency_digest_key_id": payload.idempotency_digest_key_id,
            "idempotency_digest_key_fingerprint": payload.idempotency_digest_key_fingerprint,
            "request_digest": payload.semantic_request_digest,
            "target_resource_type": PROMOTION_TARGET,
            "target_resource_id": payload.promotion_id,
            "resulting_revision": revision,
            "resulting_state": state,
            "result_code": "promotion_recovered",
            "http_disposition_class": 2,
            "completed_at": recovery_at,
        });
        let (active_baseline_version, active_baseline_hash) =
            active_baseline(&final_record).unwrap();
        let audit_evidence_projection = json!({
            "format_version": 1,
            "event_id": payload.audit_event_id,
            "receipt_id": payload.receipt_id,
            "tenant_id": payload.tenant_id,
            "installation_id": payload.installation_id,
            "principal_id": payload.principal_id,
            "session_subject_digest": payload.session_subject_digest,
            "action": PROMOTION_ACTION,
            "target_resource_type": PROMOTION_TARGET,
            "target_resource_id": payload.promotion_id,
            "request_id": payload.product_request_id,
            "authority_observation_digest": payload.authority_observation_digest,
            "effective_permission_bits": payload.effective_permission_bits,
            "authority_observed_at": payload.authority_observed_at,
            "installation_authority_revision": payload.authority_revision.parse::<i64>().unwrap(),
            "expected_generation": payload.generation.parse::<i64>().unwrap(),
            "actual_generation": payload.generation.parse::<i64>().unwrap(),
            "payload_digest": payload.promotion_request_digest,
            "binding_fingerprint": payload.binding_fingerprint,
            "policy_revision": payload.policy_revision.parse::<i64>().unwrap(),
            "active_baseline_version": active_baseline_version,
            "active_baseline_hash": active_baseline_hash,
            "resulting_state": state,
            "result_code": "promotion_recovered",
            "dependency_latency_classes": {},
            "occurred_at": recovery_at,
            "endpoint_domain": PROMOTION_ENDPOINT,
            "request_digest": payload.semantic_request_digest,
            "resulting_revision": revision,
            "http_disposition_class": 2,
            "completed_at": recovery_at,
            "evidence_version": 1,
            "replay_policy_version": 1,
            "replay_guaranteed_until": recovery_at + REPLAY_RETENTION,
        });
        RepairDecoderCase {
            keyring,
            access,
            context,
            digests,
            legacy,
            prepared,
            promotion_record: serde_json::to_value(final_record).unwrap(),
            admission_evidence: serde_json::to_value(evidence).unwrap(),
            activation_projection: json!({
                "format_version": 1,
                "disposition": final_disposition,
                "request": request,
            }),
            receipt_projection,
            audit_evidence_projection,
            database_now: recovery_at,
        }
    }

    fn admitted_with_record(
        admitted: &ProductPromotionAdmittedStageV1,
        record: PromotionRecordV1,
        database_now: DateTime<Utc>,
    ) -> ProductPromotionAdmittedStageV1 {
        ProductPromotionAdmittedStageV1 {
            record,
            admission: admitted.admission.clone(),
            admission_digest: admitted.admission_digest.clone(),
            database_now,
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
    fn prepare_error_requires_every_optional_projection_to_be_absent() {
        let mut row = ProductPromotionPrepareRowV1 {
            outcome_code: "access_denied".to_string(),
            promotion_record: None,
            admission_evidence: None,
            admission_digest: None,
            database_now: database_now(),
        };
        assert_eq!(require_prepare_absent_v1(&row), Ok(()));
        row.promotion_record = Some(Json(json!({})));
        assert_eq!(
            require_prepare_absent_v1(&row),
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        );
    }

    #[tokio::test]
    async fn publication_and_approval_decoders_reject_adversarial_outcomes_and_bounds() {
        let admitted = prepared_decoder_stage(database_now()).await;
        assert!(matches!(
            decode_product_promotion_publication_v1(
                publication_row("access_denied", None, None, database_now()),
                &admitted,
            ),
            Err(AuthorizedPromotionSubmissionErrorV1::Forbidden)
        ));
        assert!(matches!(
            decode_product_promotion_publication_v1(
                publication_row(
                    "access_denied",
                    Some(Json(json!({"format_version": 1}))),
                    None,
                    database_now(),
                ),
                &admitted,
            ),
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        ));

        assert!(matches!(
            decode_product_promotion_approval_environment_v1(
                approval_environment_row("scope_mismatch"),
                &admitted,
            ),
            Err(AuthorizedPromotionSubmissionErrorV1::ScopeMismatch)
        ));
        let mut approval_with_projection = approval_environment_row("scope_mismatch");
        approval_with_projection.historical_resource_bindings = Some(Json(json!({})));
        assert!(matches!(
            decode_product_promotion_approval_environment_v1(approval_with_projection, &admitted,),
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        ));

        assert!(matches!(
            decode_product_promotion_publication_v1(
                publication_row("unexpected", None, None, database_now()),
                &admitted,
            ),
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        ));
        assert!(matches!(
            decode_product_promotion_approval_environment_v1(
                approval_environment_row("unexpected"),
                &admitted,
            ),
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        ));

        let oversized = Json(json!({"padding": "x".repeat(MAX_TARGET_ARTIFACT_BYTES + 1)}));
        assert!(matches!(
            decode_product_promotion_publication_v1(
                publication_row("created", Some(oversized), None, database_now()),
                &admitted,
            ),
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        ));
    }

    #[tokio::test]
    async fn final_exact_rejects_a_timestamp_regression_from_the_admitted_record() {
        let admitted = prepared_decoder_stage(database_now()).await;
        let publication_plan = plan_ruleset_publication_v1(&admitted.record).unwrap();
        let request = publication_plan.request();
        let artifact = RuleSetVersion {
            guild_id: request.guild_id,
            ruleset_key: request.ruleset_key,
            version: RuleSetVersionId::FIRST,
            schema_version: admitted.record.intent.registry_schema_version,
            definition: request.definition,
            content_hash: admitted.record.intent.expected_registry_content_hash,
            created_by: request.created_by,
        };
        let early_publication_time = database_now() + Duration::seconds(1);
        let activation_time = database_now() + Duration::seconds(2);
        let late_publication_time = database_now() + Duration::seconds(3);
        let result_time = database_now() + Duration::seconds(4);
        let published_early = publication_plan
            .complete(
                &admitted.record,
                PublicationPortOutcomeV1::Created(PublishedAuthoringRuleSetV1::from(
                    artifact.clone(),
                )),
                early_publication_time,
            )
            .unwrap()
            .expected_record;
        let published_late = publication_plan
            .complete(
                &admitted.record,
                PublicationPortOutcomeV1::Created(PublishedAuthoringRuleSetV1::from(
                    artifact.clone(),
                )),
                late_publication_time,
            )
            .unwrap()
            .expected_record;
        let environment_request = plan_approval_environment_v1(&published_early)
            .unwrap()
            .request();
        let binding_revision =
            std::num::NonZeroU64::new(environment_request.binding_revision.get()).unwrap();
        let required_bindings = environment_request
            .required_channel_bindings
            .iter()
            .map(|key| ResolvedApprovalBinding::Channel {
                key: ResourceKey(key.clone()),
                id: ChannelId(700),
            })
            .collect::<Vec<_>>();
        let binding = ApprovalBindingContextV1 {
            revision: binding_revision,
            fingerprint: approval_binding_fingerprint_v1(
                environment_request.target.guild_id,
                binding_revision,
                &required_bindings,
            )
            .unwrap(),
            required_bindings,
        };
        let pending_plan = plan_pending_activation_v1(
            &published_early,
            ResolvedProductApprovalContextV1 {
                binding,
                baseline: ExpectedActiveBaselineV1::Absent,
            },
        )
        .unwrap();
        let activation =
            ActivationRequest::create_product(pending_plan.request().create, activation_time)
                .unwrap();
        let pending = match pending_plan
            .complete(
                &published_early,
                &PendingActivationReceiptV1 {
                    request: activation,
                    disposition: PendingActivationDispositionV1::Created,
                },
                activation_time,
            )
            .unwrap()
        {
            PendingActivationTransitionV1::ActivationPending {
                expected_record, ..
            } => expected_record,
            PendingActivationTransitionV1::Expired { .. }
            | PendingActivationTransitionV1::RefreshJournal => {
                panic!("expected activation-pending transition")
            }
        };
        let projection = || {
            Some(Json(json!({
                "format_version": 1,
                "disposition": "created",
                "artifact": artifact.clone(),
            })))
        };
        let persisted = || Some(Json(serde_json::to_value(&pending).unwrap()));
        let early_admitted = admitted_with_record(&admitted, published_early, result_time);
        assert!(decode_product_promotion_publication_v1(
            publication_row("final_exact", projection(), persisted(), result_time),
            &early_admitted,
        )
        .is_ok());
        let late_admitted = admitted_with_record(&admitted, published_late, result_time);
        assert!(matches!(
            decode_product_promotion_publication_v1(
                publication_row("final_exact", projection(), persisted(), result_time),
                &late_admitted,
            ),
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        ));
    }

    #[tokio::test]
    async fn activation_projection_requires_the_exact_domain_transition_and_link() {
        let admitted = prepared_decoder_stage(database_now()).await;
        let publication_plan = plan_ruleset_publication_v1(&admitted.record).unwrap();
        let publication_request = publication_plan.request();
        let artifact = PublishedAuthoringRuleSetV1 {
            guild_id: publication_request.guild_id,
            ruleset_key: publication_request.ruleset_key,
            version: RuleSetVersionId::FIRST,
            schema_version: admitted.record.intent.registry_schema_version,
            definition: publication_request.definition,
            content_hash: admitted.record.intent.expected_registry_content_hash,
            created_by: publication_request.created_by,
        };
        let published_at = database_now() + Duration::seconds(1);
        let published = publication_plan
            .complete(
                &admitted.record,
                PublicationPortOutcomeV1::Created(artifact),
                published_at,
            )
            .unwrap()
            .expected_record;
        let environment_request = plan_approval_environment_v1(&published).unwrap().request();
        let binding_revision =
            std::num::NonZeroU64::new(environment_request.binding_revision.get()).unwrap();
        let required_bindings = environment_request
            .required_channel_bindings
            .iter()
            .map(|key| ResolvedApprovalBinding::Channel {
                key: ResourceKey(key.clone()),
                id: ChannelId(700),
            })
            .collect::<Vec<_>>();
        let binding = ApprovalBindingContextV1 {
            revision: binding_revision,
            fingerprint: approval_binding_fingerprint_v1(
                environment_request.target.guild_id,
                binding_revision,
                &required_bindings,
            )
            .unwrap(),
            required_bindings,
        };
        let proposal = plan_pending_activation_v1(
            &published,
            ResolvedProductApprovalContextV1 {
                binding,
                baseline: ExpectedActiveBaselineV1::Absent,
            },
        )
        .unwrap();
        let proposal_document = serde_json::to_value(&proposal).unwrap();
        let proposal_fields = proposal_document.as_object().unwrap();
        assert_eq!(proposal_fields.len(), 7);
        for field in [
            "promotion_id",
            "promotion_request_digest",
            "expected_revision",
            "request_id",
            "target",
            "requester",
            "approval_context",
        ] {
            assert!(proposal_fields.contains_key(field));
        }
        let activated_at = database_now() + Duration::seconds(2);
        let mut request =
            ActivationRequest::create_product(proposal.request().create, activated_at).unwrap();
        let pending = match proposal
            .complete(
                &published,
                &PendingActivationReceiptV1 {
                    request: request.clone(),
                    disposition: PendingActivationDispositionV1::Created,
                },
                activated_at,
            )
            .unwrap()
        {
            PendingActivationTransitionV1::ActivationPending {
                expected_record, ..
            } => expected_record,
            PendingActivationTransitionV1::Expired { .. }
            | PendingActivationTransitionV1::RefreshJournal => {
                panic!("expected activation-pending transition")
            }
        };
        let link = plan_activation_link_v1(&pending).unwrap().request();
        request
            .link_product_at(
                &link.link.promotion_id,
                &link.link.promotion_request_digest,
                &link.link.approval_context_digest,
                activated_at,
            )
            .unwrap();
        let projection = ProductPromotionActivationProjectionV1 {
            format_version: 1,
            disposition: PendingActivationDispositionV1::Created,
            request: request.clone(),
        };
        assert!(validate_activation_projection_v1(
            "created",
            &published,
            &proposal,
            &projection,
            &pending,
            activated_at,
        )
        .is_ok());
        let mut wrong_disposition = projection.clone();
        wrong_disposition.disposition = PendingActivationDispositionV1::Reused;
        assert!(matches!(
            validate_activation_projection_v1(
                "created",
                &published,
                &proposal,
                &wrong_disposition,
                &pending,
                activated_at,
            ),
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        ));
        let mut unlinked = projection;
        unlinked.request.link_state = ActivationLinkStateV1::Unlinked;
        assert!(matches!(
            validate_activation_projection_v1(
                "created",
                &published,
                &proposal,
                &unlinked,
                &pending,
                activated_at,
            ),
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        ));
    }

    #[test]
    fn activation_error_outcomes_require_every_projection_to_be_absent() {
        let row = activation_link_row("access_denied");
        assert_eq!(require_activation_link_absent_v1(&row), Ok(()));
        let mut corrupt = activation_link_row("scope_mismatch");
        corrupt.activation_projection = Some(Json(json!({"format_version": 1})));
        assert_eq!(
            require_activation_link_absent_v1(&corrupt),
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        );
    }

    #[tokio::test]
    async fn repair_decoder_maps_only_known_null_error_shapes() {
        let admitted = prepared_decoder_stage(database_now()).await;
        let keyring = decoder_keyring();
        let access = decoder_access(&admitted);
        let context = decoder_context(&admitted);
        let digests = decoder_digests(&admitted);
        let legacy = ProductPromotionLegacyRepairV1 {
            record: admitted.record.clone(),
            database_now: admitted.database_now,
        };
        let prepared = PreparedProductPromotionAdmissionV1 {
            payload: admitted.admission.payload.clone(),
            digest: admitted.admission_digest.clone(),
        };
        for (outcome, expected) in [
            ("not_found", AuthorizedPromotionSubmissionErrorV1::NotFound),
            (
                "idempotency_conflict",
                AuthorizedPromotionSubmissionErrorV1::IdempotencyConflict,
            ),
            (
                "access_denied",
                AuthorizedPromotionSubmissionErrorV1::Forbidden,
            ),
            (
                "scope_mismatch",
                AuthorizedPromotionSubmissionErrorV1::ScopeMismatch,
            ),
            (
                "persistence_corrupt",
                AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt,
            ),
        ] {
            assert_eq!(
                decode_product_promotion_repair_link_v1(
                    activation_link_row(outcome),
                    &keyring,
                    &context,
                    &access,
                    &digests,
                    &legacy,
                    &prepared,
                )
                .unwrap_err(),
                expected
            );
        }
        let mut non_null = activation_link_row("access_denied");
        non_null.admission_digest = Some("ab".repeat(32));
        assert_eq!(
            decode_product_promotion_repair_link_v1(
                non_null, &keyring, &context, &access, &digests, &legacy, &prepared,
            )
            .unwrap_err(),
            AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt
        );
        assert_eq!(
            decode_product_promotion_repair_link_v1(
                activation_link_row("unexpected"),
                &keyring,
                &context,
                &access,
                &digests,
                &legacy,
                &prepared,
            )
            .unwrap_err(),
            AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt
        );
    }

    #[tokio::test]
    async fn repair_decoder_accepts_only_exact_pending_and_direct_expired_results() {
        let pending = repair_decoder_case(false).await;
        let pending_result = pending.decode(pending.row("recovered")).unwrap();
        let ProductPromotionLegacyRepairStageV1::Finalized(pending_final) = pending_result else {
            panic!("expected finalized recovery")
        };
        assert_eq!(pending_final.receipt.result_code, "promotion_recovered");
        assert_eq!(pending_final.receipt.resulting_state, "activation_pending");
        assert_eq!(pending_final.receipt.resulting_revision, Some(3));
        assert_eq!(pending_final.receipt.completed_at, pending.database_now);
        assert!(matches!(
            pending_final.admitted.record.stage,
            PromotionStageV1::ActivationPending { .. }
        ));

        let expired = repair_decoder_case(true).await;
        let expired_result = expired.decode(expired.row("recovered")).unwrap();
        let ProductPromotionLegacyRepairStageV1::Finalized(expired_final) = expired_result else {
            panic!("expected finalized recovery")
        };
        assert_eq!(expired_final.receipt.result_code, "promotion_recovered");
        assert_eq!(expired_final.receipt.resulting_state, "expired");
        assert_eq!(expired_final.receipt.resulting_revision, Some(4));
        assert_eq!(expired_final.receipt.completed_at, expired.database_now);
        assert_eq!(expired_final.admitted.record.revision.get(), 4);
        assert!(matches!(
            expired_final.admitted.record.stage,
            PromotionStageV1::Expired { .. }
        ));

        let mut historical_pending = expired.row("recovered");
        historical_pending.receipt_projection.as_mut().unwrap().0["resulting_state"] =
            json!("activation_pending");
        historical_pending.receipt_projection.as_mut().unwrap().0["resulting_revision"] = json!(3);
        historical_pending
            .audit_evidence_projection
            .as_mut()
            .unwrap()
            .0["resulting_state"] = json!("activation_pending");
        historical_pending
            .audit_evidence_projection
            .as_mut()
            .unwrap()
            .0["resulting_revision"] = json!(3);
        assert_eq!(
            expired.decode(historical_pending).unwrap_err(),
            AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt
        );
    }

    #[tokio::test]
    async fn repair_decoder_rejects_admission_activation_receipt_and_audit_tampering() {
        let case = repair_decoder_case(false).await;
        let assert_corrupt = |row| {
            assert_eq!(
                case.decode(row).unwrap_err(),
                AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt
            );
        };

        let mut digest = case.row("recovered");
        digest.admission_digest = Some("ab".repeat(32));
        assert_corrupt(digest);

        let mut admission = case.row("recovered");
        admission.admission_evidence.as_mut().unwrap().0["payload"]["guild_owner"] = json!(true);
        assert_corrupt(admission);

        let mut disposition = case.row("recovered");
        disposition.activation_projection.as_mut().unwrap().0["disposition"] = json!("reused");
        assert_corrupt(disposition);

        let mut request = case.row("recovered");
        request.activation_projection.as_mut().unwrap().0["request"]["requester"] = json!(9999);
        assert_corrupt(request);

        for (field, value) in [
            ("resulting_state", json!("expired")),
            ("resulting_revision", json!(4)),
            ("result_code", json!("promotion_created")),
        ] {
            let mut receipt = case.row("recovered");
            receipt.receipt_projection.as_mut().unwrap().0[field] = value;
            assert_corrupt(receipt);
        }

        for (field, value) in [
            ("resulting_state", json!("expired")),
            ("resulting_revision", json!(4)),
            ("result_code", json!("promotion_created")),
        ] {
            let mut audit = case.row("recovered");
            audit.audit_evidence_projection.as_mut().unwrap().0[field] = value;
            assert_corrupt(audit);
        }
    }

    #[tokio::test]
    async fn repair_final_replay_signal_is_exact_and_defers_final_truth() {
        let case = repair_decoder_case(false).await;
        let result = case.decode(case.final_replay_row()).unwrap();
        let ProductPromotionLegacyRepairStageV1::FinalReplayRequired(admitted) = result else {
            panic!("expected final replay signal")
        };
        assert_eq!(admitted.record, case.legacy.record);

        let expired = repair_decoder_case(true).await;
        let expired_result = expired.decode(expired.final_replay_row()).unwrap();
        let ProductPromotionLegacyRepairStageV1::FinalReplayRequired(expired_admitted) =
            expired_result
        else {
            panic!("expected expired final replay signal")
        };
        assert_eq!(expired_admitted.record.revision.get(), 4);
        assert!(matches!(
            expired_admitted.record.stage,
            PromotionStageV1::Expired { .. }
        ));

        let mut record_tamper = case.final_replay_row();
        record_tamper.promotion_record.as_mut().unwrap().0["revision"] = json!(4);
        assert_eq!(
            case.decode(record_tamper).unwrap_err(),
            AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt
        );

        let mut admission_tamper = case.final_replay_row();
        admission_tamper.admission_evidence.as_mut().unwrap().0["payload"]["guild_owner"] =
            json!(true);
        assert_eq!(
            case.decode(admission_tamper).unwrap_err(),
            AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt
        );

        let mut leaked_projection = case.final_replay_row();
        leaked_projection.activation_projection = Some(Json(case.activation_projection.clone()));
        assert_eq!(
            case.decode(leaked_projection).unwrap_err(),
            AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt
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
    fn publication_and_target_artifact_envelopes_have_exact_versioned_shapes() {
        let definition = json!({
            "version": 1,
            "panels": [],
            "modals": [],
            "rules": []
        });
        let typed_definition = serde_json::from_value(definition.clone()).unwrap();
        let hash = content_hash(
            automation_ruleset::CURRENT_RULESET_SCHEMA_VERSION,
            &typed_definition,
        )
        .unwrap()
        .to_hex();
        let artifact = json!({
            "guild_id": "7",
            "ruleset_key": "studyrooms",
            "version": 1,
            "schema_version": 1,
            "definition": definition,
            "content_hash": hash,
            "created_by": "9"
        });
        let publication = decode_bounded_v1::<ProductPromotionPublicationProjectionV1>(
            Some(Json(json!({
                "format_version": 1,
                "disposition": "created",
                "artifact": artifact.clone()
            }))),
            MAX_TARGET_ARTIFACT_BYTES,
        )
        .unwrap();
        let target = decode_bounded_v1::<ProductPromotionTargetArtifactProjectionV1>(
            Some(Json(json!({
                "format_version": 1,
                "artifact": artifact.clone()
            }))),
            MAX_TARGET_ARTIFACT_BYTES,
        )
        .unwrap();
        assert_eq!(publication.format_version, 1);
        assert_eq!(publication.artifact, target.artifact);
        assert!(matches!(
            decode_bounded_v1::<ProductPromotionTargetArtifactProjectionV1>(
                Some(Json(json!({
                    "format_version": 1,
                    "artifact": artifact,
                    "active": false
                }))),
                MAX_TARGET_ARTIFACT_BYTES,
            ),
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        ));
    }

    #[test]
    fn active_baseline_requires_a_canonical_nullable_pair() {
        assert_eq!(
            decode_active_baseline_v1(None, None),
            Ok(ExpectedActiveBaselineV1::Absent)
        );
        let exact = decode_active_baseline_v1(Some(3), Some("ab".repeat(32))).unwrap();
        assert_eq!(
            exact,
            ExpectedActiveBaselineV1::Exact {
                version: RuleSetVersionId::new(3).unwrap(),
                content_hash: automation_ruleset::RuleSetContentHash::parse_hex(&"ab".repeat(32))
                    .unwrap()
            }
        );
        assert!(decode_active_baseline_v1(Some(3), None).is_err());
        assert!(decode_active_baseline_v1(None, Some("ab".repeat(32))).is_err());
        assert!(decode_active_baseline_v1(Some(0), Some("ab".repeat(32))).is_err());
        assert!(decode_active_baseline_v1(Some(1), Some("AB".repeat(32))).is_err());
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
        assert_eq!(require_normal_activation_result_v1(&receipt), Ok(()));
        assert_eq!(
            require_recovery_result_v1(&receipt),
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        );
        let mut recovery_receipt = receipt.clone();
        recovery_receipt.result_code = "promotion_recovered".to_string();
        assert_eq!(
            require_normal_activation_result_v1(&recovery_receipt),
            Err(AuthorizedPromotionSubmissionErrorV1::PersistenceCorrupt)
        );
        assert_eq!(require_recovery_result_v1(&recovery_receipt), Ok(()));
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
            "promotion_created",
        ));
        assert!(receipt_matches_journal_identity(
            "expired",
            3,
            "expired",
            Some(3),
            expired_time,
            expired_time,
            "promotion_created",
        ));
        assert!(receipt_matches_journal_identity(
            "expired",
            4,
            "activation_pending",
            Some(3),
            activation_time,
            expired_time,
            "promotion_created",
        ));
        assert!(!receipt_matches_journal_identity(
            "expired",
            4,
            "expired",
            Some(4),
            expired_time,
            expired_time,
            "promotion_created",
        ));
        assert!(!receipt_matches_journal_identity(
            "expired",
            4,
            "activation_pending",
            Some(3),
            expired_time,
            activation_time,
            "promotion_created",
        ));
        assert!(receipt_matches_journal_identity(
            "activation_pending",
            3,
            "activation_pending",
            Some(3),
            expired_time,
            activation_time,
            "promotion_recovered",
        ));
        assert!(receipt_matches_journal_identity(
            "expired",
            4,
            "expired",
            Some(4),
            expired_time,
            expired_time,
            "promotion_recovered",
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

    #[tokio::test]
    async fn promotion_stage_debug_redacts_design_and_approval_payloads() {
        let admitted = prepared_decoder_stage(database_now()).await;
        let admitted_debug = format!("{admitted:?}");
        assert!(admitted_debug.contains("<redacted>"));
        assert!(!admitted_debug.contains("community_hub"));
        assert!(!admitted_debug.contains("ruleset"));

        let definition = serde_json::from_value(json!({
            "version": 1,
            "panels": [],
            "modals": [],
            "rules": []
        }))
        .unwrap();
        let artifact_hash = content_hash(
            automation_ruleset::CURRENT_RULESET_SCHEMA_VERSION,
            &definition,
        )
        .unwrap();
        let guild_id = discord_model::GuildId(711);
        let binding_revision = std::num::NonZeroU64::new(1).unwrap();
        let binding_key = ResourceKey("approval-binding-sentinel".to_string());
        let required_bindings = vec![ResolvedApprovalBinding::Channel {
            key: binding_key,
            id: ChannelId(712),
        }];
        let environment = ProductPromotionApprovalEnvironmentStageV1 {
            admitted,
            resolved: ResolvedProductApprovalContextV1 {
                binding: ApprovalBindingContextV1 {
                    revision: binding_revision,
                    fingerprint: approval_binding_fingerprint_v1(
                        guild_id,
                        binding_revision,
                        &required_bindings,
                    )
                    .unwrap(),
                    required_bindings,
                },
                baseline: ExpectedActiveBaselineV1::Absent,
            },
            target_artifact: RuleSetVersion {
                guild_id,
                ruleset_key: "debug-sentinel".parse().unwrap(),
                version: RuleSetVersionId::FIRST,
                schema_version: automation_ruleset::CURRENT_RULESET_SCHEMA_VERSION,
                definition,
                content_hash: artifact_hash,
                created_by: discord_model::UserId(713),
            },
        };
        let environment_debug = format!("{environment:?}");
        assert!(environment_debug.contains("<redacted>"));
        assert!(!environment_debug.contains("approval-binding-sentinel"));
        assert!(!environment_debug.contains("debug-sentinel"));
        assert!(!environment_debug.contains("community_hub"));
    }
}
