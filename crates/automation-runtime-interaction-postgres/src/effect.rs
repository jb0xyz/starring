use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt::{Debug, Formatter};
use std::num::{NonZeroU64, NonZeroUsize};
use std::time::Duration;

use automation_instance::{InstanceId, InstanceKind};
use automation_runtime_convergence::ProcessInstanceId;
use automation_runtime_interaction::{
    build_interaction_effect_compensation_intent_digest_v1,
    build_interaction_effect_compensation_observation_digest_v1,
    build_interaction_effect_compensation_result_digest_v1,
    build_interaction_effect_correlation_v1, build_interaction_effect_identity_digest_v1,
    build_interaction_effect_intent_digest_v1, build_interaction_effect_observation_digest_v1,
    build_interaction_effect_planned_correlation_v1,
    build_interaction_effect_planned_identity_digest_v1,
    build_interaction_effect_planned_preimage_digest_v1,
    build_interaction_effect_preimage_digest_v1,
    build_interaction_effect_recovery_compensation_intent_digest_v1,
    build_interaction_effect_recovery_compensation_observation_digest_v1,
    build_interaction_effect_recovery_compensation_result_digest_v1,
    build_interaction_effect_recovery_correlation_v1,
    build_interaction_effect_recovery_observation_digest_v1,
    build_interaction_effect_resolved_input_digest_v1, build_interaction_effect_result_digest_v1,
    validate_interaction_effect_compensation_observation_v1,
    validate_interaction_effect_observation_v1,
    validate_interaction_effect_recovery_compensation_observation_v1,
    validate_interaction_effect_recovery_observation_v1, InteractionActionPlanDigestV1,
    InteractionEffectActionIndexV1, InteractionEffectAttemptOutcomeV1,
    InteractionEffectChannelIdV1, InteractionEffectCompensationIntentDigestV1,
    InteractionEffectCompensationObservationOutcomeV1, InteractionEffectCompensationOutcomeV1,
    InteractionEffectCompensationResultDigestV1, InteractionEffectCorrelationClassV1,
    InteractionEffectCorrelationDigestV1, InteractionEffectCorrelationV1,
    InteractionEffectDefinitionV1, InteractionEffectExpectedPostimageDigestV1,
    InteractionEffectGuildIdV1, InteractionEffectIdentityDigestV1, InteractionEffectInputDigestV1,
    InteractionEffectInstanceStateV1, InteractionEffectInstanceTargetV1,
    InteractionEffectIntentDigestV1, InteractionEffectKindV1, InteractionEffectMaterializedPlanV1,
    InteractionEffectMessageIdV1, InteractionEffectObservationDigestV1,
    InteractionEffectObservationOutcomeV1, InteractionEffectObservedOutputV1,
    InteractionEffectOutputClassV1, InteractionEffectOverwriteTargetV1,
    InteractionEffectPayloadDigestV1, InteractionEffectPermissionStateV1,
    InteractionEffectPermissionTargetV1, InteractionEffectPermissionValueV1,
    InteractionEffectPlanDefinitionV1, InteractionEffectPlannedChannelReferenceV1,
    InteractionEffectPlannedIdentityDigestV1, InteractionEffectPlannedInstanceTargetV1,
    InteractionEffectPlannedOverwriteTargetV1, InteractionEffectPlannedPermissionTargetV1,
    InteractionEffectPlannedPreimageDigestV1, InteractionEffectPlannedPreimageV1,
    InteractionEffectPlannedRecoveryInputV1, InteractionEffectPlannedRoleMembershipTargetV1,
    InteractionEffectPlannedRoleReferenceV1, InteractionEffectPlannedTargetV1,
    InteractionEffectPreimageDigestV1, InteractionEffectPreimageV1,
    InteractionEffectRecoveryBindingV1, InteractionEffectRecoveryTargetV1,
    InteractionEffectResolvedInputDigestV1, InteractionEffectResultDigestV1,
    InteractionEffectRoleIdV1, InteractionEffectRoleMembershipTargetV1, InteractionEffectStateV1,
    InteractionEffectSuccessBindingV1 as PureInteractionEffectSuccessBindingV1,
    InteractionEffectTargetV1, InteractionEffectUserIdV1, InteractionExpectedRouteV1,
    InteractionInstanceManifestDigestV1, InteractionPreflightCertificateDigestV1,
    InteractionPreflightSnapshotDigestV1, InteractionReceiptClaimRootV1,
    InteractionReceiptIdentityV1, InteractionReceiptStateV1, MAX_INTERACTION_EFFECT_ACTIONS_V1,
};
use chrono::{DateTime, Utc};
use discord_model::{ChannelId, UserId};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::error::validate_millisecond_duration;
use crate::receipt::{
    RuntimeInteractionReceiptClaimLeaseV1, RuntimeInteractionReceiptExclusiveClaimV1,
    RuntimeInteractionReceiptRequestKindV1, RuntimeInteractionReceiptRouteV1,
};
use crate::receipt_row::bytes_to_lower_hex;
use crate::RuntimeInteractionPersistenceErrorV1;

pub const MAX_RUNTIME_INTERACTION_EFFECT_RECOVERY_SCAN_BATCH: usize = 256;
pub const MAX_RUNTIME_INTERACTION_EFFECT_PLAN_DOCUMENT_BYTES: usize = 1_048_576;
pub const MAX_RUNTIME_INTERACTION_EFFECT_RECOVERY_DOCUMENT_BYTES: usize = 4_096;
pub const MIN_RUNTIME_INTERACTION_EFFECT_RETRY_DELAY: Duration = Duration::from_secs(1);
pub const MAX_RUNTIME_INTERACTION_EFFECT_RETRY_DELAY: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectMutationDispositionV1 {
    Applied,
    ExactReplay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectRecoveryPathV1 {
    Observation,
    Compensation,
    ResponseTail,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectOutputIdentityV1 {
    Discord(NonZeroU64),
    Instance(InstanceId),
}

impl RuntimeInteractionEffectOutputIdentityV1 {
    pub fn discord(value: u64) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        NonZeroU64::new(value)
            .map(Self::Discord)
            .ok_or(RuntimeInteractionPersistenceErrorV1::InvalidInput)
    }

    pub fn instance(value: InstanceId) -> Self {
        Self::Instance(value)
    }

    pub fn as_parameter(&self) -> String {
        match self {
            Self::Discord(value) => value.get().to_string(),
            Self::Instance(value) => value.as_str().to_string(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectPlanActionV1 {
    definition: InteractionEffectPlanDefinitionV1,
    planned_identity_digest: InteractionEffectPlannedIdentityDigestV1,
    expected_postimage_digest: InteractionEffectExpectedPostimageDigestV1,
    planned_preimage_digest: InteractionEffectPlannedPreimageDigestV1,
    correlation: InteractionEffectCorrelationV1,
    planned_recovery_input: Value,
    planned_preimage: Value,
}

impl RuntimeInteractionEffectPlanActionV1 {
    pub fn new(
        definition: InteractionEffectPlanDefinitionV1,
        expected_postimage_digest: InteractionEffectExpectedPostimageDigestV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        let planned_recovery_input =
            planned_recovery_input_document_v1(definition.recovery_input())?;
        let planned_preimage =
            planned_preimage_document_v1(definition.recovery_input().preimage())?;
        validate_recovery_document_v1(&planned_recovery_input)?;
        validate_recovery_document_v1(&planned_preimage)?;
        let planned_identity_digest =
            build_interaction_effect_planned_identity_digest_v1(&definition);
        let planned_preimage_digest = build_interaction_effect_planned_preimage_digest_v1(
            definition.recovery_input().preimage(),
        );
        let correlation = build_interaction_effect_planned_correlation_v1(&definition);
        Ok(Self {
            definition,
            planned_identity_digest,
            expected_postimage_digest,
            planned_preimage_digest,
            correlation,
            planned_recovery_input,
            planned_preimage,
        })
    }

    pub fn definition(&self) -> &InteractionEffectPlanDefinitionV1 {
        &self.definition
    }

    pub fn expected_postimage_digest(&self) -> &InteractionEffectExpectedPostimageDigestV1 {
        &self.expected_postimage_digest
    }

    pub fn planned_identity_digest(&self) -> &InteractionEffectPlannedIdentityDigestV1 {
        &self.planned_identity_digest
    }

    pub fn planned_preimage_digest(&self) -> &InteractionEffectPlannedPreimageDigestV1 {
        &self.planned_preimage_digest
    }

    pub fn correlation(&self) -> &InteractionEffectCorrelationV1 {
        &self.correlation
    }

    pub fn planned_recovery_input(&self) -> &Value {
        &self.planned_recovery_input
    }

    pub fn planned_preimage(&self) -> &Value {
        &self.planned_preimage
    }

    pub(crate) fn document(&self) -> Value {
        let definition = &self.definition;
        let correlation_marker = correlation_marker_v1(&self.correlation);
        json!({
            "action_index": definition.action().action_index().get(),
            "action_kind": definition.action().kind().code(),
            "dependency_indices": definition
                .dependencies()
                .iter()
                .map(|dependency| dependency.action_index().get())
                .collect::<Vec<_>>(),
            "planned_identity_digest": self.planned_identity_digest.as_str(),
            "input_digest": definition.action().input_digest().as_str(),
            "expected_postimage_digest": self.expected_postimage_digest.as_str(),
            "planned_recovery_input": self.planned_recovery_input,
            "planned_preimage_digest": self.planned_preimage_digest.as_str(),
            "planned_preimage": self.planned_preimage,
            "output_kind": output_class_code_v1(definition.output_class()),
            "correlation_class": correlation_class_code_v1(self.correlation.class()),
            "correlation_digest": self.correlation.marker_digest().as_str(),
            "correlation_marker": correlation_marker,
        })
    }
}

impl Debug for RuntimeInteractionEffectPlanActionV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeInteractionEffectPlanActionV1")
            .field(
                "action_index",
                &self.definition.action().action_index().get(),
            )
            .field("action_kind", &self.definition.action().kind())
            .field("dependency_count", &self.definition.dependencies().len())
            .field("recovery_input", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectPlanBindRequestV1 {
    identity: InteractionReceiptIdentityV1,
    receipt_head_revision: u64,
    receipt_claim_revision: u64,
    process_instance_id: ProcessInstanceId,
    action_plan_digest: InteractionActionPlanDigestV1,
    preflight_certificate_digest: InteractionPreflightCertificateDigestV1,
    snapshot_digest: InteractionPreflightSnapshotDigestV1,
    actions: Vec<RuntimeInteractionEffectPlanActionV1>,
    action_document: Value,
}

impl RuntimeInteractionEffectPlanBindRequestV1 {
    pub fn new(
        claim: &RuntimeInteractionReceiptExclusiveClaimV1,
        preflight_certificate_digest: InteractionPreflightCertificateDigestV1,
        snapshot_digest: InteractionPreflightSnapshotDigestV1,
        actions: Vec<RuntimeInteractionEffectPlanActionV1>,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        if !matches!(
            claim.state(),
            InteractionReceiptStateV1::Prepared | InteractionReceiptStateV1::Executing
        ) || actions.len() > usize::from(MAX_INTERACTION_EFFECT_ACTIONS_V1)
        {
            return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
        }
        let action_plan_digest = claim
            .action_plan_digest()
            .cloned()
            .ok_or(RuntimeInteractionPersistenceErrorV1::Conflict)?;
        let identity = claim.claim_root().identity();
        let mut correlations = BTreeSet::new();
        for (index, action) in actions.iter().enumerate() {
            let action_identity = action.definition().action();
            if action_identity.receipt_identity() != identity
                || action_identity.action_plan_digest() != &action_plan_digest
                || action_identity.preflight_certificate_digest() != &preflight_certificate_digest
                || usize::from(action_identity.action_index().get()) != index
            {
                return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
            }
            if let Some(marker) = correlation_marker_v1(action.correlation()) {
                let key = (
                    correlation_class_code_v1(action.correlation().class()),
                    marker,
                );
                if !correlations.insert(key) {
                    return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
                }
            }
        }
        let action_document = Value::Array(
            actions
                .iter()
                .map(RuntimeInteractionEffectPlanActionV1::document)
                .collect(),
        );
        if serde_json::to_vec(&action_document)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?
            .len()
            > MAX_RUNTIME_INTERACTION_EFFECT_PLAN_DOCUMENT_BYTES
        {
            return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
        }
        Ok(Self {
            identity,
            receipt_head_revision: claim.head_revision(),
            receipt_claim_revision: claim.claim_revision(),
            process_instance_id: claim.claim_process_instance_id().clone(),
            action_plan_digest,
            preflight_certificate_digest,
            snapshot_digest,
            actions,
            action_document,
        })
    }

    pub fn identity(&self) -> InteractionReceiptIdentityV1 {
        self.identity
    }

    pub fn actions(&self) -> &[RuntimeInteractionEffectPlanActionV1] {
        &self.actions
    }

    pub(crate) fn receipt_head_revision(&self) -> u64 {
        self.receipt_head_revision
    }

    pub(crate) fn receipt_claim_revision(&self) -> u64 {
        self.receipt_claim_revision
    }

    pub(crate) fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub(crate) fn action_plan_digest(&self) -> &InteractionActionPlanDigestV1 {
        &self.action_plan_digest
    }

    pub(crate) fn preflight_certificate_digest(&self) -> &InteractionPreflightCertificateDigestV1 {
        &self.preflight_certificate_digest
    }

    pub(crate) fn snapshot_digest(&self) -> &InteractionPreflightSnapshotDigestV1 {
        &self.snapshot_digest
    }

    pub(crate) fn action_document(&self) -> &Value {
        &self.action_document
    }
}

impl Debug for RuntimeInteractionEffectPlanBindRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeInteractionEffectPlanBindRequestV1")
            .field("action_count", &self.actions.len())
            .field("payload", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeInteractionEffectPlanBindOutcomeV1 {
    disposition: RuntimeInteractionEffectMutationDispositionV1,
    action_count: usize,
    certificate_issued_at: DateTime<Utc>,
    certificate_expires_at: DateTime<Utc>,
    observed_database_now: DateTime<Utc>,
}

impl RuntimeInteractionEffectPlanBindOutcomeV1 {
    pub(crate) fn new(
        disposition: RuntimeInteractionEffectMutationDispositionV1,
        action_count: usize,
        certificate_issued_at: DateTime<Utc>,
        certificate_expires_at: DateTime<Utc>,
        observed_database_now: DateTime<Utc>,
    ) -> Self {
        Self {
            disposition,
            action_count,
            certificate_issued_at,
            certificate_expires_at,
            observed_database_now,
        }
    }

    pub fn disposition(&self) -> RuntimeInteractionEffectMutationDispositionV1 {
        self.disposition
    }

    pub fn action_count(&self) -> usize {
        self.action_count
    }

    pub fn certificate_issued_at(&self) -> DateTime<Utc> {
        self.certificate_issued_at
    }

    pub fn certificate_expires_at(&self) -> DateTime<Utc> {
        self.certificate_expires_at
    }

    pub fn observed_database_now(&self) -> DateTime<Utc> {
        self.observed_database_now
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectIntendRequestV1 {
    identity: InteractionReceiptIdentityV1,
    receipt_head_revision: u64,
    receipt_claim_revision: u64,
    process_instance_id: ProcessInstanceId,
    preflight_certificate_digest: InteractionPreflightCertificateDigestV1,
    action_index: InteractionEffectActionIndexV1,
    effect_head_revision: u64,
    intent_digest: InteractionEffectIntentDigestV1,
    resolved_input_digest: InteractionEffectResolvedInputDigestV1,
    resolved_effect_identity_digest: InteractionEffectIdentityDigestV1,
    resolved_input: Value,
    resolved_instance_manifest_digest: Option<InteractionInstanceManifestDigestV1>,
    resolved_preimage_digest: InteractionEffectPreimageDigestV1,
    resolved_preimage: Value,
    recovery_delay_milliseconds: i64,
}

impl RuntimeInteractionEffectIntendRequestV1 {
    pub fn new(
        claim: &RuntimeInteractionReceiptExclusiveClaimV1,
        materialized: &InteractionEffectMaterializedPlanV1,
        effect_head_revision: u64,
        resolved_instance_manifest_digest: Option<InteractionInstanceManifestDigestV1>,
        recovery_delay: Duration,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        let definition = materialized.definition();
        validate_claim_definition_v1(claim, definition)?;
        if claim.state() != InteractionReceiptStateV1::Executing
            || effect_head_revision == 0
            || effect_head_revision >= i64::MAX as u64
        {
            return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
        }
        let correlation = build_interaction_effect_correlation_v1(definition);
        let intent_digest = build_interaction_effect_intent_digest_v1(definition, &correlation)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
        let resolved_input = resolved_recovery_input_document_v1(
            materialized,
            resolved_instance_manifest_digest.as_ref(),
        )?;
        let resolved_preimage = resolved_preimage_document_v1(materialized)?;
        validate_recovery_document_v1(&resolved_input)?;
        validate_recovery_document_v1(&resolved_preimage)?;
        let recovery_delay_milliseconds = retry_delay_milliseconds_v1(recovery_delay)?;
        Ok(Self {
            identity: definition.action().receipt_identity(),
            receipt_head_revision: claim.head_revision(),
            receipt_claim_revision: claim.claim_revision(),
            process_instance_id: claim.claim_process_instance_id().clone(),
            preflight_certificate_digest: definition
                .action()
                .preflight_certificate_digest()
                .clone(),
            action_index: definition.action().action_index(),
            effect_head_revision,
            intent_digest,
            resolved_input_digest: build_interaction_effect_resolved_input_digest_v1(
                materialized.resolved_input(),
            ),
            resolved_effect_identity_digest: build_interaction_effect_identity_digest_v1(
                materialized.definition(),
            ),
            resolved_input,
            resolved_instance_manifest_digest,
            resolved_preimage_digest: build_interaction_effect_preimage_digest_v1(
                materialized.resolved_input().preimage(),
            ),
            resolved_preimage,
            recovery_delay_milliseconds,
        })
    }

    pub fn identity(&self) -> InteractionReceiptIdentityV1 {
        self.identity
    }

    pub fn action_index(&self) -> InteractionEffectActionIndexV1 {
        self.action_index
    }

    pub fn effect_head_revision(&self) -> u64 {
        self.effect_head_revision
    }

    pub fn intent_digest(&self) -> &InteractionEffectIntentDigestV1 {
        &self.intent_digest
    }

    pub fn resolved_input_digest(&self) -> &InteractionEffectResolvedInputDigestV1 {
        &self.resolved_input_digest
    }

    pub fn resolved_effect_identity_digest(&self) -> &InteractionEffectIdentityDigestV1 {
        &self.resolved_effect_identity_digest
    }

    pub fn resolved_instance_manifest_digest(
        &self,
    ) -> Option<&InteractionInstanceManifestDigestV1> {
        self.resolved_instance_manifest_digest.as_ref()
    }

    pub(crate) fn receipt_head_revision(&self) -> u64 {
        self.receipt_head_revision
    }

    pub(crate) fn receipt_claim_revision(&self) -> u64 {
        self.receipt_claim_revision
    }

    pub(crate) fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub(crate) fn preflight_certificate_digest(&self) -> &InteractionPreflightCertificateDigestV1 {
        &self.preflight_certificate_digest
    }

    pub(crate) fn resolved_input(&self) -> &Value {
        &self.resolved_input
    }

    pub(crate) fn resolved_preimage_digest(&self) -> &InteractionEffectPreimageDigestV1 {
        &self.resolved_preimage_digest
    }

    pub(crate) fn resolved_preimage(&self) -> &Value {
        &self.resolved_preimage
    }

    pub(crate) fn recovery_delay_milliseconds(&self) -> i64 {
        self.recovery_delay_milliseconds
    }
}

impl Debug for RuntimeInteractionEffectIntendRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeInteractionEffectIntendRequestV1")
            .field("action_index", &self.action_index.get())
            .field("effect_head_revision", &self.effect_head_revision)
            .field("payload", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectFinishRequestV1 {
    identity: InteractionReceiptIdentityV1,
    receipt_head_revision: u64,
    receipt_claim_revision: u64,
    process_instance_id: ProcessInstanceId,
    preflight_certificate_digest: InteractionPreflightCertificateDigestV1,
    action_index: InteractionEffectActionIndexV1,
    effect_head_revision: u64,
    result_digest: InteractionEffectResultDigestV1,
    outcome: InteractionEffectAttemptOutcomeV1,
    output_identity: Option<RuntimeInteractionEffectOutputIdentityV1>,
}

impl RuntimeInteractionEffectFinishRequestV1 {
    pub fn new(
        claim: &RuntimeInteractionReceiptExclusiveClaimV1,
        materialized: &InteractionEffectMaterializedPlanV1,
        effect_head_revision: u64,
        outcome: InteractionEffectAttemptOutcomeV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        let definition = materialized.definition();
        validate_claim_definition_v1(claim, definition)?;
        if claim.state() != InteractionReceiptStateV1::Executing
            || effect_head_revision == 0
            || effect_head_revision >= i64::MAX as u64
        {
            return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
        }
        if let InteractionEffectAttemptOutcomeV1::KnownSucceeded(output) = &outcome {
            definition
                .validate_observed_output(output)
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
        }
        let correlation = build_interaction_effect_correlation_v1(definition);
        let intent_digest = build_interaction_effect_intent_digest_v1(definition, &correlation)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
        let result_digest =
            build_interaction_effect_result_digest_v1(definition, &intent_digest, &outcome)
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
        let output_identity = output_identity_v1(materialized, &outcome)?;
        Ok(Self {
            identity: definition.action().receipt_identity(),
            receipt_head_revision: claim.head_revision(),
            receipt_claim_revision: claim.claim_revision(),
            process_instance_id: claim.claim_process_instance_id().clone(),
            preflight_certificate_digest: definition
                .action()
                .preflight_certificate_digest()
                .clone(),
            action_index: definition.action().action_index(),
            effect_head_revision,
            result_digest,
            outcome,
            output_identity,
        })
    }

    pub fn identity(&self) -> InteractionReceiptIdentityV1 {
        self.identity
    }

    pub fn action_index(&self) -> InteractionEffectActionIndexV1 {
        self.action_index
    }

    pub fn effect_head_revision(&self) -> u64 {
        self.effect_head_revision
    }

    pub fn outcome(&self) -> &InteractionEffectAttemptOutcomeV1 {
        &self.outcome
    }

    pub fn result_digest(&self) -> &InteractionEffectResultDigestV1 {
        &self.result_digest
    }

    pub fn output_identity(&self) -> Option<&RuntimeInteractionEffectOutputIdentityV1> {
        self.output_identity.as_ref()
    }

    pub(crate) fn receipt_head_revision(&self) -> u64 {
        self.receipt_head_revision
    }

    pub(crate) fn receipt_claim_revision(&self) -> u64 {
        self.receipt_claim_revision
    }

    pub(crate) fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub(crate) fn preflight_certificate_digest(&self) -> &InteractionPreflightCertificateDigestV1 {
        &self.preflight_certificate_digest
    }

    pub(crate) fn outcome_code(&self) -> &'static str {
        match self.outcome {
            InteractionEffectAttemptOutcomeV1::KnownSucceeded(_) => "succeeded",
            InteractionEffectAttemptOutcomeV1::KnownFailed(_) => "definitive_failure",
            InteractionEffectAttemptOutcomeV1::Indeterminate(_) => "indeterminate",
        }
    }

    pub(crate) fn output_parameter(&self) -> String {
        self.output_identity
            .as_ref()
            .map(RuntimeInteractionEffectOutputIdentityV1::as_parameter)
            .unwrap_or_default()
    }
}

impl Debug for RuntimeInteractionEffectFinishRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeInteractionEffectFinishRequestV1")
            .field("action_index", &self.action_index.get())
            .field("effect_head_revision", &self.effect_head_revision)
            .field("outcome", &self.outcome_code())
            .field("payload", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeInteractionEffectCheckpointV1 {
    disposition: RuntimeInteractionEffectMutationDispositionV1,
    state: InteractionEffectStateV1,
    effect_head_revision: u64,
    recovery_at: Option<DateTime<Utc>>,
    observed_database_now: DateTime<Utc>,
}

impl RuntimeInteractionEffectCheckpointV1 {
    pub(crate) fn new(
        disposition: RuntimeInteractionEffectMutationDispositionV1,
        state: InteractionEffectStateV1,
        effect_head_revision: u64,
        recovery_at: Option<DateTime<Utc>>,
        observed_database_now: DateTime<Utc>,
    ) -> Self {
        Self {
            disposition,
            state,
            effect_head_revision,
            recovery_at,
            observed_database_now,
        }
    }

    pub fn disposition(&self) -> RuntimeInteractionEffectMutationDispositionV1 {
        self.disposition
    }

    pub fn state(&self) -> InteractionEffectStateV1 {
        self.state
    }

    pub fn effect_head_revision(&self) -> u64 {
        self.effect_head_revision
    }

    pub fn recovery_at(&self) -> Option<DateTime<Utc>> {
        self.recovery_at
    }

    pub fn observed_database_now(&self) -> DateTime<Utc> {
        self.observed_database_now
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectSuccessBindingV1 {
    AttemptResult(InteractionEffectResultDigestV1),
    Observation(InteractionEffectObservationDigestV1),
}

impl RuntimeInteractionEffectSuccessBindingV1 {
    fn as_pure(&self) -> PureInteractionEffectSuccessBindingV1<'_> {
        match self {
            Self::AttemptResult(digest) => {
                PureInteractionEffectSuccessBindingV1::AttemptResult(digest)
            }
            Self::Observation(digest) => PureInteractionEffectSuccessBindingV1::Observation(digest),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectOriginV1 {
    claim_root: InteractionReceiptClaimRootV1,
    route: RuntimeInteractionReceiptRouteV1,
    attestation_id: String,
    channel_id: ChannelId,
    actor_user_id: UserId,
    request_kind: RuntimeInteractionReceiptRequestKindV1,
}

impl RuntimeInteractionEffectOriginV1 {
    pub(crate) fn new(
        claim_root: InteractionReceiptClaimRootV1,
        route: RuntimeInteractionReceiptRouteV1,
        attestation_id: String,
        channel_id: ChannelId,
        actor_user_id: UserId,
        request_kind: RuntimeInteractionReceiptRequestKindV1,
    ) -> Self {
        Self {
            claim_root,
            route,
            attestation_id,
            channel_id,
            actor_user_id,
            request_kind,
        }
    }

    pub fn claim_root(&self) -> &InteractionReceiptClaimRootV1 {
        &self.claim_root
    }

    pub fn route(&self) -> &RuntimeInteractionReceiptRouteV1 {
        &self.route
    }

    pub fn attestation_id(&self) -> &str {
        &self.attestation_id
    }

    pub fn channel_id(&self) -> ChannelId {
        self.channel_id
    }

    pub fn actor_user_id(&self) -> UserId {
        self.actor_user_id
    }

    pub fn request_kind(&self) -> RuntimeInteractionReceiptRequestKindV1 {
        self.request_kind
    }
}

impl Debug for RuntimeInteractionEffectOriginV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectOriginV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectRecoveryScanKeyV1 {
    recovery_at: DateTime<Utc>,
    identity: InteractionReceiptIdentityV1,
    action_index: InteractionEffectActionIndexV1,
}

impl RuntimeInteractionEffectRecoveryScanKeyV1 {
    pub fn new(
        recovery_at: DateTime<Utc>,
        identity: InteractionReceiptIdentityV1,
        action_index: InteractionEffectActionIndexV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        validate_effect_database_time_v1(recovery_at, false)?;
        Ok(Self {
            recovery_at,
            identity,
            action_index,
        })
    }

    pub fn recovery_at(&self) -> DateTime<Utc> {
        self.recovery_at
    }

    pub fn identity(&self) -> InteractionReceiptIdentityV1 {
        self.identity
    }

    pub fn action_index(&self) -> InteractionEffectActionIndexV1 {
        self.action_index
    }

    pub(crate) fn cmp_c(&self, other: &Self) -> Ordering {
        self.recovery_at
            .cmp(&other.recovery_at)
            .then_with(|| {
                self.identity
                    .application_id()
                    .get()
                    .to_string()
                    .cmp(&other.identity.application_id().get().to_string())
            })
            .then_with(|| {
                self.identity
                    .interaction_id()
                    .get()
                    .to_string()
                    .cmp(&other.identity.interaction_id().get().to_string())
            })
            .then_with(|| self.action_index.cmp(&other.action_index))
    }
}

impl Debug for RuntimeInteractionEffectRecoveryScanKeyV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectRecoveryScanKeyV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq, Default)]
pub struct RuntimeInteractionEffectRecoveryScanCursorV1 {
    after: Option<RuntimeInteractionEffectRecoveryScanKeyV1>,
    through: Option<RuntimeInteractionEffectRecoveryScanKeyV1>,
}

impl RuntimeInteractionEffectRecoveryScanCursorV1 {
    pub fn new(
        after: Option<RuntimeInteractionEffectRecoveryScanKeyV1>,
        through: Option<RuntimeInteractionEffectRecoveryScanKeyV1>,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        if through.is_none() && after.is_some()
            || after
                .as_ref()
                .zip(through.as_ref())
                .is_some_and(|(after, through)| after.cmp_c(through).is_ge())
        {
            return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
        }
        Ok(Self { after, through })
    }

    pub fn after(&self) -> Option<&RuntimeInteractionEffectRecoveryScanKeyV1> {
        self.after.as_ref()
    }

    pub fn through(&self) -> Option<&RuntimeInteractionEffectRecoveryScanKeyV1> {
        self.through.as_ref()
    }
}

impl Debug for RuntimeInteractionEffectRecoveryScanCursorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectRecoveryScanCursorV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectRecoveryCandidateV1 {
    pub(crate) key: RuntimeInteractionEffectRecoveryScanKeyV1,
    pub(crate) kind: InteractionEffectKindV1,
    pub(crate) state: InteractionEffectStateV1,
    pub(crate) effect_head_revision: u64,
    pub(crate) recovery_claim_revision: u64,
    pub(crate) attempt_count: u16,
    pub(crate) observation_attempt_count: u16,
    pub(crate) compensation_attempt_count: u16,
    pub(crate) compensation_observation_attempt_count: u16,
    pub(crate) dependency_indices: Vec<InteractionEffectActionIndexV1>,
    pub(crate) planned_identity_digest: InteractionEffectPlannedIdentityDigestV1,
    pub(crate) input_digest: InteractionEffectInputDigestV1,
    pub(crate) expected_postimage_digest: InteractionEffectExpectedPostimageDigestV1,
    pub(crate) planned_recovery_input: Value,
    pub(crate) planned_preimage_digest: InteractionEffectPlannedPreimageDigestV1,
    pub(crate) planned_preimage: Value,
    pub(crate) resolved_input: Value,
    pub(crate) resolved_preimage_digest: InteractionEffectPreimageDigestV1,
    pub(crate) resolved_preimage: Value,
    pub(crate) resolved_effect_identity_digest: InteractionEffectIdentityDigestV1,
    pub(crate) resolved_instance_manifest_digest: Option<InteractionInstanceManifestDigestV1>,
    pub(crate) output_class: InteractionEffectOutputClassV1,
    pub(crate) output_identity: Option<RuntimeInteractionEffectOutputIdentityV1>,
    pub(crate) correlation_class: InteractionEffectCorrelationClassV1,
    pub(crate) correlation_digest: InteractionEffectCorrelationDigestV1,
    pub(crate) correlation_marker: Option<String>,
    pub(crate) intent_digest: Option<InteractionEffectIntentDigestV1>,
    pub(crate) result_digest: Option<InteractionEffectResultDigestV1>,
    pub(crate) success_binding: Option<RuntimeInteractionEffectSuccessBindingV1>,
    pub(crate) compensation_intent_digest: Option<InteractionEffectCompensationIntentDigestV1>,
    pub(crate) compensation_result_digest: Option<InteractionEffectCompensationResultDigestV1>,
    pub(crate) action_plan_digest: InteractionActionPlanDigestV1,
    pub(crate) preflight_certificate_digest: InteractionPreflightCertificateDigestV1,
    pub(crate) snapshot_digest: InteractionPreflightSnapshotDigestV1,
    pub(crate) certificate_issued_at: DateTime<Utc>,
    pub(crate) certificate_expires_at: DateTime<Utc>,
    pub(crate) origin: RuntimeInteractionEffectOriginV1,
}

impl RuntimeInteractionEffectRecoveryCandidateV1 {
    pub fn key(&self) -> &RuntimeInteractionEffectRecoveryScanKeyV1 {
        &self.key
    }

    pub fn kind(&self) -> InteractionEffectKindV1 {
        self.kind
    }

    pub fn state(&self) -> InteractionEffectStateV1 {
        self.state
    }

    pub fn recovery_path(&self) -> RuntimeInteractionEffectRecoveryPathV1 {
        if self.kind == InteractionEffectKindV1::EditResponse {
            RuntimeInteractionEffectRecoveryPathV1::ResponseTail
        } else if matches!(
            self.state,
            InteractionEffectStateV1::KnownSucceeded
                | InteractionEffectStateV1::ReconciledSucceeded
                | InteractionEffectStateV1::CompensationIntended
                | InteractionEffectStateV1::CompensationIndeterminate
                | InteractionEffectStateV1::CompensationObserving
                | InteractionEffectStateV1::CompensationObservationPending
        ) {
            RuntimeInteractionEffectRecoveryPathV1::Compensation
        } else {
            RuntimeInteractionEffectRecoveryPathV1::Observation
        }
    }

    pub fn effect_head_revision(&self) -> u64 {
        self.effect_head_revision
    }

    pub fn recovery_claim_revision(&self) -> u64 {
        self.recovery_claim_revision
    }

    pub fn attempt_count(&self) -> u16 {
        self.attempt_count
    }

    pub fn observation_attempt_count(&self) -> u16 {
        self.observation_attempt_count
    }

    pub fn compensation_attempt_count(&self) -> u16 {
        self.compensation_attempt_count
    }

    pub fn compensation_observation_attempt_count(&self) -> u16 {
        self.compensation_observation_attempt_count
    }

    pub fn dependency_indices(&self) -> &[InteractionEffectActionIndexV1] {
        &self.dependency_indices
    }

    pub fn planned_identity_digest(&self) -> &InteractionEffectPlannedIdentityDigestV1 {
        &self.planned_identity_digest
    }

    pub fn input_digest(&self) -> &InteractionEffectInputDigestV1 {
        &self.input_digest
    }

    pub fn expected_postimage_digest(&self) -> &InteractionEffectExpectedPostimageDigestV1 {
        &self.expected_postimage_digest
    }

    pub fn resolved_effect_identity_digest(&self) -> &InteractionEffectIdentityDigestV1 {
        &self.resolved_effect_identity_digest
    }

    pub fn resolved_instance_manifest_digest(
        &self,
    ) -> Option<&InteractionInstanceManifestDigestV1> {
        self.resolved_instance_manifest_digest.as_ref()
    }

    pub fn output_class(&self) -> InteractionEffectOutputClassV1 {
        self.output_class
    }

    pub fn output_identity(&self) -> Option<&RuntimeInteractionEffectOutputIdentityV1> {
        self.output_identity.as_ref()
    }

    pub fn correlation_class(&self) -> InteractionEffectCorrelationClassV1 {
        self.correlation_class
    }

    pub fn correlation_digest(&self) -> &InteractionEffectCorrelationDigestV1 {
        &self.correlation_digest
    }

    pub fn correlation_marker(&self) -> Option<&str> {
        self.correlation_marker.as_deref()
    }

    pub fn intent_digest(&self) -> Option<&InteractionEffectIntentDigestV1> {
        self.intent_digest.as_ref()
    }

    pub fn result_digest(&self) -> Option<&InteractionEffectResultDigestV1> {
        self.result_digest.as_ref()
    }

    pub fn success_binding(&self) -> Option<&RuntimeInteractionEffectSuccessBindingV1> {
        self.success_binding.as_ref()
    }

    pub fn compensation_intent_digest(
        &self,
    ) -> Option<&InteractionEffectCompensationIntentDigestV1> {
        self.compensation_intent_digest.as_ref()
    }

    pub fn compensation_result_digest(
        &self,
    ) -> Option<&InteractionEffectCompensationResultDigestV1> {
        self.compensation_result_digest.as_ref()
    }

    pub fn action_plan_digest(&self) -> &InteractionActionPlanDigestV1 {
        &self.action_plan_digest
    }

    pub fn preflight_certificate_digest(&self) -> &InteractionPreflightCertificateDigestV1 {
        &self.preflight_certificate_digest
    }

    pub fn snapshot_digest(&self) -> &InteractionPreflightSnapshotDigestV1 {
        &self.snapshot_digest
    }

    pub fn certificate_issued_at(&self) -> DateTime<Utc> {
        self.certificate_issued_at
    }

    pub fn certificate_expires_at(&self) -> DateTime<Utc> {
        self.certificate_expires_at
    }

    pub fn origin(&self) -> &RuntimeInteractionEffectOriginV1 {
        &self.origin
    }

    pub fn planned_recovery_input(&self) -> &Value {
        &self.planned_recovery_input
    }

    pub fn planned_preimage(&self) -> &Value {
        &self.planned_preimage
    }

    pub fn resolved_input(&self) -> &Value {
        &self.resolved_input
    }

    pub fn resolved_preimage(&self) -> &Value {
        &self.resolved_preimage
    }

    pub fn resolved_preimage_digest(&self) -> &InteractionEffectPreimageDigestV1 {
        &self.resolved_preimage_digest
    }

    pub fn strict_recovery_binding_v1(
        &self,
    ) -> Result<RuntimeInteractionEffectRecoveryBindingV1, RuntimeInteractionPersistenceErrorV1>
    {
        RuntimeInteractionEffectRecoveryBindingV1::new(self)
    }

    pub fn strict_recovery_binding_from_definition_v1(
        &self,
        definition: &InteractionEffectDefinitionV1,
    ) -> Result<RuntimeInteractionEffectRecoveryBindingV1, RuntimeInteractionPersistenceErrorV1>
    {
        let recovered = RuntimeInteractionEffectRecoveryBindingV1::new(self)?;
        let action = definition.action();
        if action.receipt_identity() != self.key.identity
            || action.action_index() != self.key.action_index
            || action.kind() != self.kind
            || action.action_plan_digest() != &self.action_plan_digest
            || action.preflight_certificate_digest() != &self.preflight_certificate_digest
            || action.input_digest() != &self.input_digest
            || definition
                .dependencies()
                .iter()
                .map(|dependency| dependency.action_index())
                .collect::<Vec<_>>()
                != self.dependency_indices
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        recovered
            .binding
            .verify_resolved_definition(definition)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        Ok(recovered)
    }
}

impl Debug for RuntimeInteractionEffectRecoveryCandidateV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeInteractionEffectRecoveryCandidateV1")
            .field("action_index", &self.key.action_index().get())
            .field("kind", &self.kind)
            .field("state", &self.state)
            .field("payload", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeInteractionEffectRecoveryBindingV1 {
    binding: InteractionEffectRecoveryBindingV1,
    successful_output: Option<InteractionEffectObservedOutputV1>,
    instance_id: Option<InstanceId>,
}

impl RuntimeInteractionEffectRecoveryBindingV1 {
    pub fn new(
        candidate: &RuntimeInteractionEffectRecoveryCandidateV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        let (target, instance_id) = decode_recovery_target_v1(candidate)?;
        if target.kind() != candidate.kind {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let preimage = decode_recovery_preimage_v1(candidate, &target, instance_id.as_ref())?;
        if build_interaction_effect_preimage_digest_v1(&preimage)
            != candidate.resolved_preimage_digest
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        validate_persisted_payload_binding_v1(candidate, &target)?;
        let correlation = build_interaction_effect_recovery_correlation_v1(
            &candidate.planned_identity_digest,
            candidate.correlation_class,
        );
        if correlation.marker_digest() != &candidate.correlation_digest
            || correlation_marker_v1(&correlation) != candidate.correlation_marker
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let binding = InteractionEffectRecoveryBindingV1::new(
            target,
            preimage,
            candidate.planned_identity_digest.clone(),
            candidate.resolved_effect_identity_digest.clone(),
            candidate.expected_postimage_digest.clone(),
            correlation,
        )
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        if binding.output_class() != candidate.output_class {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let successful_output =
            recovery_successful_output_v1(candidate, &binding, instance_id.as_ref())?;
        Ok(Self {
            binding,
            successful_output,
            instance_id,
        })
    }

    pub fn binding(&self) -> &InteractionEffectRecoveryBindingV1 {
        &self.binding
    }

    pub fn successful_output(&self) -> Option<&InteractionEffectObservedOutputV1> {
        self.successful_output.as_ref()
    }

    pub fn instance_id(&self) -> Option<&InstanceId> {
        self.instance_id.as_ref()
    }

    pub fn verify_resolved_definition(
        &self,
        definition: &InteractionEffectDefinitionV1,
    ) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
        self.binding
            .verify_resolved_definition(definition)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectRecoveryScanPageV1 {
    candidates: Vec<RuntimeInteractionEffectRecoveryCandidateV1>,
    through: Option<RuntimeInteractionEffectRecoveryScanKeyV1>,
    observed_database_now: Option<DateTime<Utc>>,
    requested_limit: NonZeroUsize,
}

impl RuntimeInteractionEffectRecoveryScanPageV1 {
    pub(crate) fn new(
        candidates: Vec<RuntimeInteractionEffectRecoveryCandidateV1>,
        through: Option<RuntimeInteractionEffectRecoveryScanKeyV1>,
        observed_database_now: Option<DateTime<Utc>>,
        requested_limit: NonZeroUsize,
    ) -> Self {
        Self {
            candidates,
            through,
            observed_database_now,
            requested_limit,
        }
    }

    pub fn candidates(&self) -> &[RuntimeInteractionEffectRecoveryCandidateV1] {
        &self.candidates
    }

    pub fn through(&self) -> Option<&RuntimeInteractionEffectRecoveryScanKeyV1> {
        self.through.as_ref()
    }

    pub fn observed_database_now(&self) -> Option<DateTime<Utc>> {
        self.observed_database_now
    }

    pub fn next_cursor(&self) -> Option<RuntimeInteractionEffectRecoveryScanCursorV1> {
        let through = self.through.clone()?;
        let after = self
            .candidates
            .last()
            .map(|candidate| candidate.key.clone());
        RuntimeInteractionEffectRecoveryScanCursorV1::new(after, Some(through)).ok()
    }

    pub fn exhausted(&self) -> bool {
        self.candidates.is_empty()
            || self.candidates.len() < self.requested_limit.get()
            || self.candidates.last().map(|candidate| &candidate.key) == self.through.as_ref()
    }
}

impl Debug for RuntimeInteractionEffectRecoveryScanPageV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectRecoveryScanPageV1(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeInteractionEffectRecoveredDefinitionV1 {
    materialized: InteractionEffectMaterializedPlanV1,
    expected_postimage_digest: InteractionEffectExpectedPostimageDigestV1,
    resolved_instance_manifest_digest: Option<InteractionInstanceManifestDigestV1>,
}

impl RuntimeInteractionEffectRecoveredDefinitionV1 {
    pub fn new(
        candidate: &RuntimeInteractionEffectRecoveryCandidateV1,
        materialized: InteractionEffectMaterializedPlanV1,
        expected_postimage_digest: InteractionEffectExpectedPostimageDigestV1,
        resolved_instance_manifest_digest: Option<InteractionInstanceManifestDigestV1>,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        validate_recovered_definition_v1(
            candidate,
            &materialized,
            &expected_postimage_digest,
            resolved_instance_manifest_digest.as_ref(),
        )?;
        Ok(Self {
            materialized,
            expected_postimage_digest,
            resolved_instance_manifest_digest,
        })
    }

    pub fn materialized(&self) -> &InteractionEffectMaterializedPlanV1 {
        &self.materialized
    }

    pub fn expected_postimage_digest(&self) -> &InteractionEffectExpectedPostimageDigestV1 {
        &self.expected_postimage_digest
    }

    pub fn resolved_instance_manifest_digest(
        &self,
    ) -> Option<&InteractionInstanceManifestDigestV1> {
        self.resolved_instance_manifest_digest.as_ref()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectRecoveryClaimRequestV1 {
    candidate: RuntimeInteractionEffectRecoveryCandidateV1,
    expected_route: InteractionExpectedRouteV1,
    claim_lease: RuntimeInteractionReceiptClaimLeaseV1,
}

impl RuntimeInteractionEffectRecoveryClaimRequestV1 {
    pub fn new(
        candidate: RuntimeInteractionEffectRecoveryCandidateV1,
        expected_route: InteractionExpectedRouteV1,
        claim_lease: RuntimeInteractionReceiptClaimLeaseV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        if !matches!(
            candidate.state,
            InteractionEffectStateV1::Intended
                | InteractionEffectStateV1::Indeterminate
                | InteractionEffectStateV1::Observing
                | InteractionEffectStateV1::ObservationPending
                | InteractionEffectStateV1::CompensationIntended
                | InteractionEffectStateV1::CompensationIndeterminate
                | InteractionEffectStateV1::CompensationObserving
                | InteractionEffectStateV1::CompensationObservationPending
        ) || candidate.recovery_path() == RuntimeInteractionEffectRecoveryPathV1::ResponseTail
            && candidate.kind != InteractionEffectKindV1::EditResponse
        {
            return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
        }
        validate_recovery_route_v1(&candidate, &expected_route)?;
        Ok(Self {
            candidate,
            expected_route,
            claim_lease,
        })
    }

    pub fn candidate(&self) -> &RuntimeInteractionEffectRecoveryCandidateV1 {
        &self.candidate
    }

    pub fn expected_route(&self) -> &InteractionExpectedRouteV1 {
        &self.expected_route
    }

    pub fn claim_lease(&self) -> RuntimeInteractionReceiptClaimLeaseV1 {
        self.claim_lease
    }
}

impl Debug for RuntimeInteractionEffectRecoveryClaimRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectRecoveryClaimRequestV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectRecoveryClaimV1 {
    candidate: RuntimeInteractionEffectRecoveryCandidateV1,
    expected_route: InteractionExpectedRouteV1,
    disposition: RuntimeInteractionEffectMutationDispositionV1,
    state: InteractionEffectStateV1,
    effect_head_revision: u64,
    recovery_claim_revision: u64,
    recovery_claim_expires_at: DateTime<Utc>,
    observed_database_now: DateTime<Utc>,
}

impl RuntimeInteractionEffectRecoveryClaimV1 {
    pub(crate) fn new(
        request: RuntimeInteractionEffectRecoveryClaimRequestV1,
        disposition: RuntimeInteractionEffectMutationDispositionV1,
        state: InteractionEffectStateV1,
        effect_head_revision: u64,
        recovery_claim_revision: u64,
        recovery_claim_expires_at: DateTime<Utc>,
        observed_database_now: DateTime<Utc>,
    ) -> Self {
        Self {
            candidate: request.candidate,
            expected_route: request.expected_route,
            disposition,
            state,
            effect_head_revision,
            recovery_claim_revision,
            recovery_claim_expires_at,
            observed_database_now,
        }
    }

    pub fn candidate(&self) -> &RuntimeInteractionEffectRecoveryCandidateV1 {
        &self.candidate
    }

    pub fn disposition(&self) -> RuntimeInteractionEffectMutationDispositionV1 {
        self.disposition
    }

    pub fn state(&self) -> InteractionEffectStateV1 {
        self.state
    }

    pub fn effect_head_revision(&self) -> u64 {
        self.effect_head_revision
    }

    pub fn recovery_claim_revision(&self) -> u64 {
        self.recovery_claim_revision
    }

    pub fn recovery_claim_expires_at(&self) -> DateTime<Utc> {
        self.recovery_claim_expires_at
    }

    pub fn observed_database_now(&self) -> DateTime<Utc> {
        self.observed_database_now
    }

    pub(crate) fn observation_attempt(
        &self,
    ) -> Result<
        automation_runtime_interaction::InteractionEffectAttemptV1,
        RuntimeInteractionPersistenceErrorV1,
    > {
        let previous = if self.state == InteractionEffectStateV1::Observing {
            self.candidate.observation_attempt_count
        } else {
            self.candidate.compensation_observation_attempt_count
        };
        let attempt = previous
            .checked_add(1)
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        automation_runtime_interaction::InteractionEffectAttemptV1::new(attempt)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
    }
}

impl Debug for RuntimeInteractionEffectRecoveryClaimV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectRecoveryClaimV1(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeInteractionEffectRecoveryBlockedV1 {
    disposition: RuntimeInteractionEffectMutationDispositionV1,
    effect_head_revision: u64,
    recovery_claim_revision: u64,
    observed_database_now: DateTime<Utc>,
}

impl RuntimeInteractionEffectRecoveryBlockedV1 {
    pub(crate) fn new(
        disposition: RuntimeInteractionEffectMutationDispositionV1,
        effect_head_revision: u64,
        recovery_claim_revision: u64,
        observed_database_now: DateTime<Utc>,
    ) -> Self {
        Self {
            disposition,
            effect_head_revision,
            recovery_claim_revision,
            observed_database_now,
        }
    }

    pub fn disposition(&self) -> RuntimeInteractionEffectMutationDispositionV1 {
        self.disposition
    }

    pub fn effect_head_revision(&self) -> u64 {
        self.effect_head_revision
    }

    pub fn recovery_claim_revision(&self) -> u64 {
        self.recovery_claim_revision
    }

    pub fn observed_database_now(&self) -> DateTime<Utc> {
        self.observed_database_now
    }
}

#[derive(Debug)]
pub enum RuntimeInteractionEffectRecoveryClaimOutcomeV1 {
    Claimed(Box<RuntimeInteractionEffectRecoveryClaimV1>),
    RecoveryBlocked(RuntimeInteractionEffectRecoveryBlockedV1),
}

#[derive(Debug)]
pub enum RuntimeInteractionEffectCompensationIntendOutcomeV1 {
    Claimed(Box<RuntimeInteractionEffectCompensationClaimV1>),
    RecoveryBlocked(RuntimeInteractionEffectRecoveryBlockedV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectReconciliationOutcomeV1 {
    Observation(InteractionEffectObservationOutcomeV1),
    CompensationObservation(InteractionEffectCompensationObservationOutcomeV1),
    RecoveryBlocked(RuntimeInteractionEffectRecoveryBlockReasonV1),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionEffectRecoveryBlockReasonV1 {
    DiscordReadRejected,
    ResponseTokenUnavailable,
    ObservationProtocol,
    CompensationConflict,
    CompensationUnsupported,
    NonCompensable,
    InternalConflict,
    DiscordForbidden,
    InternalAuthority,
    AttemptBudgetExhausted,
}

impl RuntimeInteractionEffectRecoveryBlockReasonV1 {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::DiscordReadRejected => "recovery_blocked_discord_read_rejected",
            Self::ResponseTokenUnavailable => "recovery_blocked_response_token_unavailable",
            Self::ObservationProtocol => "recovery_blocked_observation_protocol",
            Self::CompensationConflict => "recovery_blocked_compensation_conflict",
            Self::CompensationUnsupported => "recovery_blocked_compensation_unsupported",
            Self::NonCompensable => "recovery_blocked_non_compensable",
            Self::InternalConflict => "recovery_blocked_internal_conflict",
            Self::DiscordForbidden => "recovery_blocked_discord_forbidden",
            Self::InternalAuthority => "recovery_blocked_internal_authority",
            Self::AttemptBudgetExhausted => "recovery_blocked_attempt_budget_exhausted",
        }
    }

    fn allows_observation(self) -> bool {
        matches!(
            self,
            Self::DiscordReadRejected
                | Self::ObservationProtocol
                | Self::InternalConflict
                | Self::DiscordForbidden
                | Self::InternalAuthority
        )
    }

    fn allows_compensation(self) -> bool {
        matches!(
            self,
            Self::DiscordReadRejected
                | Self::ObservationProtocol
                | Self::CompensationConflict
                | Self::CompensationUnsupported
                | Self::NonCompensable
                | Self::InternalConflict
                | Self::DiscordForbidden
                | Self::InternalAuthority
        )
    }

    pub(crate) fn allows_response_tail(self) -> bool {
        matches!(
            self,
            Self::DiscordReadRejected
                | Self::ResponseTokenUnavailable
                | Self::ObservationProtocol
                | Self::InternalConflict
                | Self::DiscordForbidden
                | Self::InternalAuthority
        )
    }
}

struct RuntimeInteractionEffectRecoveryBlockDigestMaterialV1<'a> {
    identity: InteractionReceiptIdentityV1,
    action_index: InteractionEffectActionIndexV1,
    effect_head_revision: u64,
    recovery_claim_revision: u64,
    expected_route: &'a InteractionExpectedRouteV1,
    source_effect_state: InteractionEffectStateV1,
    recovery_path: RuntimeInteractionEffectRecoveryPathV1,
    preflight_certificate_digest: &'a InteractionPreflightCertificateDigestV1,
    reason: RuntimeInteractionEffectRecoveryBlockReasonV1,
}

fn build_recovery_block_digest_v1(
    material: RuntimeInteractionEffectRecoveryBlockDigestMaterialV1<'_>,
) -> [u8; 32] {
    let document = format!(
        "starring-runtime-interaction-effect-recovery-block-v1|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        material.identity.application_id().get(),
        material.identity.interaction_id().get(),
        material.action_index.get(),
        material.effect_head_revision,
        material.recovery_claim_revision,
        material
            .expected_route
            .process_identity()
            .process_instance_id
            .as_str(),
        material.expected_route.gateway_shard_identity().as_str(),
        material.expected_route.runtime_build_revision().as_str(),
        material
            .expected_route
            .process_identity()
            .runtime_generation
            .get(),
        material.expected_route.route_fencing_token().get(),
        material.expected_route.route_incarnation().get(),
        effect_state_code_v1(material.source_effect_state),
        recovery_path_code_v1(material.recovery_path),
        material.preflight_certificate_digest.as_str(),
        material.reason.code()
    );
    Sha256::digest(document.as_bytes()).into()
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectReconcileRequestV1 {
    pub(crate) identity: InteractionReceiptIdentityV1,
    pub(crate) action_index: InteractionEffectActionIndexV1,
    pub(crate) effect_head_revision: u64,
    pub(crate) recovery_claim_revision: u64,
    pub(crate) process_instance_id: ProcessInstanceId,
    pub(crate) expected_route: InteractionExpectedRouteV1,
    pub(crate) source_effect_state: InteractionEffectStateV1,
    pub(crate) recovery_path: RuntimeInteractionEffectRecoveryPathV1,
    pub(crate) preflight_certificate_digest: InteractionPreflightCertificateDigestV1,
    outcome: RuntimeInteractionEffectReconciliationOutcomeV1,
    pub(crate) observation_digest: String,
    output_identity: Option<RuntimeInteractionEffectOutputIdentityV1>,
    pub(crate) retry_delay_milliseconds: i64,
}

impl RuntimeInteractionEffectReconcileRequestV1 {
    pub fn new(
        claim: &RuntimeInteractionEffectRecoveryClaimV1,
        recovered: &RuntimeInteractionEffectRecoveredDefinitionV1,
        outcome: RuntimeInteractionEffectReconciliationOutcomeV1,
        retry_delay: Duration,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        validate_recovered_definition_v1(
            claim.candidate(),
            recovered.materialized(),
            recovered.expected_postimage_digest(),
            recovered.resolved_instance_manifest_digest(),
        )?;
        let definition = recovered.materialized().definition();
        let attempt = claim.observation_attempt()?;
        let (observation_digest, output_identity) = match &outcome {
            RuntimeInteractionEffectReconciliationOutcomeV1::Observation(observation) => {
                if claim.state != InteractionEffectStateV1::Observing {
                    return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
                }
                validate_interaction_effect_observation_v1(definition, observation)
                    .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
                let intent = claim
                    .candidate
                    .intent_digest
                    .as_ref()
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
                let digest = build_interaction_effect_observation_digest_v1(
                    definition,
                    intent,
                    attempt,
                    observation,
                )
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
                let identity = match observation {
                    InteractionEffectObservationOutcomeV1::ExactMatch { output, .. } => {
                        output_identity_from_observed_v1(recovered.materialized(), output)?
                    }
                    _ => None,
                };
                (digest.as_str().to_string(), identity)
            }
            RuntimeInteractionEffectReconciliationOutcomeV1::CompensationObservation(
                observation,
            ) => {
                if claim.state != InteractionEffectStateV1::CompensationObserving {
                    return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
                }
                validate_interaction_effect_compensation_observation_v1(definition, observation)
                    .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
                let intent = claim
                    .candidate
                    .compensation_intent_digest
                    .as_ref()
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
                let digest = build_interaction_effect_compensation_observation_digest_v1(
                    definition,
                    intent,
                    attempt,
                    observation,
                )
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
                (digest.as_str().to_string(), None)
            }
            RuntimeInteractionEffectReconciliationOutcomeV1::RecoveryBlocked(_) => {
                return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput)
            }
        };
        Ok(Self {
            identity: claim.candidate.key.identity,
            action_index: claim.candidate.key.action_index,
            effect_head_revision: claim.effect_head_revision,
            recovery_claim_revision: claim.recovery_claim_revision,
            process_instance_id: claim
                .expected_route
                .process_identity()
                .process_instance_id
                .clone(),
            expected_route: claim.expected_route.clone(),
            source_effect_state: claim.state,
            recovery_path: claim.candidate.recovery_path(),
            preflight_certificate_digest: claim.candidate.preflight_certificate_digest.clone(),
            outcome,
            observation_digest,
            output_identity,
            retry_delay_milliseconds: retry_delay_milliseconds_v1(retry_delay)?,
        })
    }

    pub fn new_recovery_bound(
        claim: &RuntimeInteractionEffectRecoveryClaimV1,
        recovered: &RuntimeInteractionEffectRecoveryBindingV1,
        outcome: RuntimeInteractionEffectReconciliationOutcomeV1,
        retry_delay: Duration,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        validate_strict_recovery_binding_v1(claim.candidate(), recovered)?;
        let binding = recovered.binding();
        let attempt = claim.observation_attempt()?;
        let (observation_digest, output_identity) = match &outcome {
            RuntimeInteractionEffectReconciliationOutcomeV1::Observation(observation) => {
                if claim.state != InteractionEffectStateV1::Observing {
                    return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
                }
                validate_interaction_effect_recovery_observation_v1(binding, observation)
                    .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
                let intent = claim
                    .candidate
                    .intent_digest
                    .as_ref()
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
                let digest = build_interaction_effect_recovery_observation_digest_v1(
                    binding,
                    intent,
                    attempt,
                    observation,
                )
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
                let identity = match observation {
                    InteractionEffectObservationOutcomeV1::ExactMatch { output, .. } => {
                        output_identity_from_recovery_observed_v1(recovered, output)?
                    }
                    _ => None,
                };
                (digest.as_str().to_string(), identity)
            }
            RuntimeInteractionEffectReconciliationOutcomeV1::CompensationObservation(
                observation,
            ) => {
                if claim.state != InteractionEffectStateV1::CompensationObserving {
                    return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
                }
                validate_interaction_effect_recovery_compensation_observation_v1(
                    binding,
                    observation,
                )
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
                let intent = claim
                    .candidate
                    .compensation_intent_digest
                    .as_ref()
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
                let digest = build_interaction_effect_recovery_compensation_observation_digest_v1(
                    binding,
                    intent,
                    attempt,
                    observation,
                )
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
                (digest.as_str().to_string(), None)
            }
            RuntimeInteractionEffectReconciliationOutcomeV1::RecoveryBlocked(_) => {
                return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput)
            }
        };
        Ok(Self {
            identity: claim.candidate.key.identity,
            action_index: claim.candidate.key.action_index,
            effect_head_revision: claim.effect_head_revision,
            recovery_claim_revision: claim.recovery_claim_revision,
            process_instance_id: claim
                .expected_route
                .process_identity()
                .process_instance_id
                .clone(),
            expected_route: claim.expected_route.clone(),
            source_effect_state: claim.state,
            recovery_path: claim.candidate.recovery_path(),
            preflight_certificate_digest: claim.candidate.preflight_certificate_digest.clone(),
            outcome,
            observation_digest,
            output_identity,
            retry_delay_milliseconds: retry_delay_milliseconds_v1(retry_delay)?,
        })
    }

    pub fn recovery_blocked(
        claim: &RuntimeInteractionEffectRecoveryClaimV1,
        reason: RuntimeInteractionEffectRecoveryBlockReasonV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        if !matches!(
            claim.state,
            InteractionEffectStateV1::Observing | InteractionEffectStateV1::CompensationObserving
        ) || claim.state == InteractionEffectStateV1::Observing && !reason.allows_observation()
            || claim.state == InteractionEffectStateV1::CompensationObserving
                && !reason.allows_compensation()
        {
            return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
        }
        let identity = claim.candidate.key.identity;
        let action_index = claim.candidate.key.action_index;
        let process_instance_id = claim
            .expected_route
            .process_identity()
            .process_instance_id
            .clone();
        let digest =
            build_recovery_block_digest_v1(RuntimeInteractionEffectRecoveryBlockDigestMaterialV1 {
                identity,
                action_index,
                effect_head_revision: claim.effect_head_revision,
                recovery_claim_revision: claim.recovery_claim_revision,
                expected_route: &claim.expected_route,
                source_effect_state: claim.state,
                recovery_path: claim.candidate.recovery_path(),
                preflight_certificate_digest: &claim.candidate.preflight_certificate_digest,
                reason,
            });
        Ok(Self {
            identity,
            action_index,
            effect_head_revision: claim.effect_head_revision,
            recovery_claim_revision: claim.recovery_claim_revision,
            process_instance_id,
            expected_route: claim.expected_route.clone(),
            source_effect_state: claim.state,
            recovery_path: claim.candidate.recovery_path(),
            preflight_certificate_digest: claim.candidate.preflight_certificate_digest.clone(),
            outcome: RuntimeInteractionEffectReconciliationOutcomeV1::RecoveryBlocked(reason),
            observation_digest: bytes_to_lower_hex(&digest),
            output_identity: None,
            retry_delay_milliseconds: retry_delay_milliseconds_v1(
                MIN_RUNTIME_INTERACTION_EFFECT_RETRY_DELAY,
            )?,
        })
    }

    pub fn compensation_blocked(
        claim: &RuntimeInteractionEffectCompensationClaimV1,
        reason: RuntimeInteractionEffectRecoveryBlockReasonV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        if !reason.allows_compensation() {
            return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
        }
        let identity = claim.candidate.key.identity;
        let action_index = claim.candidate.key.action_index;
        let process_instance_id = claim
            .expected_route
            .process_identity()
            .process_instance_id
            .clone();
        let digest =
            build_recovery_block_digest_v1(RuntimeInteractionEffectRecoveryBlockDigestMaterialV1 {
                identity,
                action_index,
                effect_head_revision: claim.effect_head_revision,
                recovery_claim_revision: claim.recovery_claim_revision,
                expected_route: &claim.expected_route,
                source_effect_state: InteractionEffectStateV1::CompensationIntended,
                recovery_path: claim.candidate.recovery_path(),
                preflight_certificate_digest: &claim.candidate.preflight_certificate_digest,
                reason,
            });
        Ok(Self {
            identity,
            action_index,
            effect_head_revision: claim.effect_head_revision,
            recovery_claim_revision: claim.recovery_claim_revision,
            process_instance_id,
            expected_route: claim.expected_route.clone(),
            source_effect_state: InteractionEffectStateV1::CompensationIntended,
            recovery_path: claim.candidate.recovery_path(),
            preflight_certificate_digest: claim.candidate.preflight_certificate_digest.clone(),
            outcome: RuntimeInteractionEffectReconciliationOutcomeV1::RecoveryBlocked(reason),
            observation_digest: bytes_to_lower_hex(&digest),
            output_identity: None,
            retry_delay_milliseconds: retry_delay_milliseconds_v1(
                MIN_RUNTIME_INTERACTION_EFFECT_RETRY_DELAY,
            )?,
        })
    }

    pub fn outcome(&self) -> &RuntimeInteractionEffectReconciliationOutcomeV1 {
        &self.outcome
    }

    pub(crate) fn outcome_code(&self) -> &'static str {
        match &self.outcome {
            RuntimeInteractionEffectReconciliationOutcomeV1::Observation(
                InteractionEffectObservationOutcomeV1::ExactMatch { .. },
            ) => "adopted_success",
            RuntimeInteractionEffectReconciliationOutcomeV1::Observation(
                InteractionEffectObservationOutcomeV1::ExactAbsence { .. },
            ) => "observed_failure",
            RuntimeInteractionEffectReconciliationOutcomeV1::Observation(
                InteractionEffectObservationOutcomeV1::Pending { .. },
            ) => "deferred",
            RuntimeInteractionEffectReconciliationOutcomeV1::Observation(
                InteractionEffectObservationOutcomeV1::Conflict { .. },
            ) => "conflict",
            RuntimeInteractionEffectReconciliationOutcomeV1::Observation(
                InteractionEffectObservationOutcomeV1::Unsupported { .. },
            ) => "unsupported",
            RuntimeInteractionEffectReconciliationOutcomeV1::CompensationObservation(
                InteractionEffectCompensationObservationOutcomeV1::Restored { .. },
            ) => "compensation_restored",
            RuntimeInteractionEffectReconciliationOutcomeV1::CompensationObservation(
                InteractionEffectCompensationObservationOutcomeV1::Pending { .. },
            ) => "compensation_deferred",
            RuntimeInteractionEffectReconciliationOutcomeV1::CompensationObservation(
                InteractionEffectCompensationObservationOutcomeV1::Conflict { .. },
            ) => "compensation_conflict",
            RuntimeInteractionEffectReconciliationOutcomeV1::CompensationObservation(
                InteractionEffectCompensationObservationOutcomeV1::Unsupported { .. },
            ) => "compensation_unsupported",
            RuntimeInteractionEffectReconciliationOutcomeV1::RecoveryBlocked(reason) => {
                reason.code()
            }
        }
    }

    pub(crate) fn output_parameter(&self) -> String {
        self.output_identity
            .as_ref()
            .map(RuntimeInteractionEffectOutputIdentityV1::as_parameter)
            .unwrap_or_default()
    }
}

impl Debug for RuntimeInteractionEffectReconcileRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectReconcileRequestV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectCompensationIntendRequestV1 {
    pub(crate) candidate: RuntimeInteractionEffectRecoveryCandidateV1,
    pub(crate) expected_route: InteractionExpectedRouteV1,
    pub(crate) preflight_certificate_digest: InteractionPreflightCertificateDigestV1,
    pub(crate) compensation_intent_digest: InteractionEffectCompensationIntentDigestV1,
    pub(crate) retry_delay_milliseconds: i64,
}

impl RuntimeInteractionEffectCompensationIntendRequestV1 {
    pub fn new(
        candidate: RuntimeInteractionEffectRecoveryCandidateV1,
        expected_route: InteractionExpectedRouteV1,
        recovered: &RuntimeInteractionEffectRecoveredDefinitionV1,
        retry_delay: Duration,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        if !matches!(
            candidate.state,
            InteractionEffectStateV1::KnownSucceeded
                | InteractionEffectStateV1::ReconciledSucceeded
        ) || candidate.recovery_path() != RuntimeInteractionEffectRecoveryPathV1::Compensation
        {
            return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
        }
        validate_recovery_route_v1(&candidate, &expected_route)?;
        validate_recovered_definition_v1(
            &candidate,
            recovered.materialized(),
            recovered.expected_postimage_digest(),
            recovered.resolved_instance_manifest_digest(),
        )?;
        let success = candidate
            .success_binding
            .as_ref()
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        let output = successful_output_v1(&candidate, recovered.materialized())?;
        let attempt = automation_runtime_interaction::InteractionEffectAttemptV1::new(
            candidate
                .compensation_attempt_count
                .saturating_add(1)
                .min(64),
        )
        .ok()
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        let compensation_intent_digest = build_interaction_effect_compensation_intent_digest_v1(
            recovered.materialized().definition(),
            success.as_pure(),
            &output,
            attempt,
        )
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
        Ok(Self {
            preflight_certificate_digest: candidate.preflight_certificate_digest.clone(),
            candidate,
            expected_route,
            compensation_intent_digest,
            retry_delay_milliseconds: retry_delay_milliseconds_v1(retry_delay)?,
        })
    }

    pub fn new_recovery_bound(
        candidate: RuntimeInteractionEffectRecoveryCandidateV1,
        expected_route: InteractionExpectedRouteV1,
        recovered: &RuntimeInteractionEffectRecoveryBindingV1,
        retry_delay: Duration,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        if !matches!(
            candidate.state,
            InteractionEffectStateV1::KnownSucceeded
                | InteractionEffectStateV1::ReconciledSucceeded
        ) || candidate.recovery_path() != RuntimeInteractionEffectRecoveryPathV1::Compensation
        {
            return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
        }
        validate_recovery_route_v1(&candidate, &expected_route)?;
        validate_strict_recovery_binding_v1(&candidate, recovered)?;
        let success = candidate
            .success_binding
            .as_ref()
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        let output = recovered
            .successful_output()
            .ok_or(RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
        let attempt = automation_runtime_interaction::InteractionEffectAttemptV1::new(
            candidate
                .compensation_attempt_count
                .saturating_add(1)
                .min(64),
        )
        .ok()
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        let compensation_intent_digest =
            build_interaction_effect_recovery_compensation_intent_digest_v1(
                recovered.binding(),
                success.as_pure(),
                output,
                attempt,
            )
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
        Ok(Self {
            preflight_certificate_digest: candidate.preflight_certificate_digest.clone(),
            candidate,
            expected_route,
            compensation_intent_digest,
            retry_delay_milliseconds: retry_delay_milliseconds_v1(retry_delay)?,
        })
    }

    pub fn candidate(&self) -> &RuntimeInteractionEffectRecoveryCandidateV1 {
        &self.candidate
    }

    pub fn compensation_intent_digest(&self) -> &InteractionEffectCompensationIntentDigestV1 {
        &self.compensation_intent_digest
    }
}

impl Debug for RuntimeInteractionEffectCompensationIntendRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectCompensationIntendRequestV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectCompensationClaimV1 {
    candidate: RuntimeInteractionEffectRecoveryCandidateV1,
    expected_route: InteractionExpectedRouteV1,
    disposition: RuntimeInteractionEffectMutationDispositionV1,
    effect_head_revision: u64,
    recovery_claim_revision: u64,
    recovery_at: DateTime<Utc>,
    observed_database_now: DateTime<Utc>,
    compensation_intent_digest: InteractionEffectCompensationIntentDigestV1,
}

impl RuntimeInteractionEffectCompensationClaimV1 {
    pub(crate) fn new(
        request: RuntimeInteractionEffectCompensationIntendRequestV1,
        disposition: RuntimeInteractionEffectMutationDispositionV1,
        effect_head_revision: u64,
        recovery_claim_revision: u64,
        recovery_at: DateTime<Utc>,
        observed_database_now: DateTime<Utc>,
    ) -> Self {
        Self {
            candidate: request.candidate,
            expected_route: request.expected_route,
            disposition,
            effect_head_revision,
            recovery_claim_revision,
            recovery_at,
            observed_database_now,
            compensation_intent_digest: request.compensation_intent_digest,
        }
    }

    pub fn candidate(&self) -> &RuntimeInteractionEffectRecoveryCandidateV1 {
        &self.candidate
    }

    pub fn disposition(&self) -> RuntimeInteractionEffectMutationDispositionV1 {
        self.disposition
    }

    pub fn effect_head_revision(&self) -> u64 {
        self.effect_head_revision
    }

    pub fn recovery_claim_revision(&self) -> u64 {
        self.recovery_claim_revision
    }

    pub fn recovery_at(&self) -> DateTime<Utc> {
        self.recovery_at
    }

    pub fn observed_database_now(&self) -> DateTime<Utc> {
        self.observed_database_now
    }

    pub fn compensation_intent_digest(&self) -> &InteractionEffectCompensationIntentDigestV1 {
        &self.compensation_intent_digest
    }
}

impl Debug for RuntimeInteractionEffectCompensationClaimV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectCompensationClaimV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionEffectCompensationFinishRequestV1 {
    pub(crate) identity: InteractionReceiptIdentityV1,
    pub(crate) action_index: InteractionEffectActionIndexV1,
    pub(crate) effect_head_revision: u64,
    pub(crate) recovery_claim_revision: u64,
    pub(crate) process_instance_id: ProcessInstanceId,
    pub(crate) preflight_certificate_digest: InteractionPreflightCertificateDigestV1,
    outcome: InteractionEffectCompensationOutcomeV1,
    compensation_result_digest: InteractionEffectCompensationResultDigestV1,
    pub(crate) retry_delay_milliseconds: i64,
}

impl RuntimeInteractionEffectCompensationFinishRequestV1 {
    pub fn new(
        claim: &RuntimeInteractionEffectCompensationClaimV1,
        recovered: &RuntimeInteractionEffectRecoveredDefinitionV1,
        outcome: InteractionEffectCompensationOutcomeV1,
        retry_delay: Duration,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        validate_recovered_definition_v1(
            claim.candidate(),
            recovered.materialized(),
            recovered.expected_postimage_digest(),
            recovered.resolved_instance_manifest_digest(),
        )?;
        let compensation_result_digest = build_interaction_effect_compensation_result_digest_v1(
            recovered.materialized().definition(),
            &claim.compensation_intent_digest,
            &outcome,
        )
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
        Ok(Self {
            identity: claim.candidate.key.identity,
            action_index: claim.candidate.key.action_index,
            effect_head_revision: claim.effect_head_revision,
            recovery_claim_revision: claim.recovery_claim_revision,
            process_instance_id: claim
                .expected_route
                .process_identity()
                .process_instance_id
                .clone(),
            preflight_certificate_digest: claim.candidate.preflight_certificate_digest.clone(),
            outcome,
            compensation_result_digest,
            retry_delay_milliseconds: retry_delay_milliseconds_v1(retry_delay)?,
        })
    }

    pub fn new_recovery_bound(
        claim: &RuntimeInteractionEffectCompensationClaimV1,
        recovered: &RuntimeInteractionEffectRecoveryBindingV1,
        outcome: InteractionEffectCompensationOutcomeV1,
        retry_delay: Duration,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        validate_strict_recovery_binding_v1(claim.candidate(), recovered)?;
        let compensation_result_digest =
            build_interaction_effect_recovery_compensation_result_digest_v1(
                recovered.binding(),
                &claim.compensation_intent_digest,
                &outcome,
            )
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
        Ok(Self {
            identity: claim.candidate.key.identity,
            action_index: claim.candidate.key.action_index,
            effect_head_revision: claim.effect_head_revision,
            recovery_claim_revision: claim.recovery_claim_revision,
            process_instance_id: claim
                .expected_route
                .process_identity()
                .process_instance_id
                .clone(),
            preflight_certificate_digest: claim.candidate.preflight_certificate_digest.clone(),
            outcome,
            compensation_result_digest,
            retry_delay_milliseconds: retry_delay_milliseconds_v1(retry_delay)?,
        })
    }

    pub fn outcome(&self) -> &InteractionEffectCompensationOutcomeV1 {
        &self.outcome
    }

    pub fn compensation_result_digest(&self) -> &InteractionEffectCompensationResultDigestV1 {
        &self.compensation_result_digest
    }

    pub(crate) fn outcome_code(&self) -> &'static str {
        match self.outcome {
            InteractionEffectCompensationOutcomeV1::Succeeded { .. } => "compensated",
            InteractionEffectCompensationOutcomeV1::KnownFailed(_) => "definitive_failure",
            InteractionEffectCompensationOutcomeV1::Indeterminate(_) => "indeterminate",
        }
    }
}

impl Debug for RuntimeInteractionEffectCompensationFinishRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionEffectCompensationFinishRequestV1(<redacted>)")
    }
}

fn validate_strict_recovery_binding_v1(
    candidate: &RuntimeInteractionEffectRecoveryCandidateV1,
    recovered: &RuntimeInteractionEffectRecoveryBindingV1,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    if RuntimeInteractionEffectRecoveryBindingV1::new(candidate)? != *recovered {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn validate_persisted_payload_binding_v1(
    candidate: &RuntimeInteractionEffectRecoveryCandidateV1,
    target: &InteractionEffectRecoveryTargetV1,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    if let InteractionEffectRecoveryTargetV1::RegisterInstance {
        target,
        kind,
        manifest_digest,
    } = target
    {
        let planned = exact_json_object_v1(
            &candidate.planned_recovery_input,
            &[
                "references",
                "instance_id",
                "instance_kind",
                "manifest_digest",
            ],
        )?;
        let resolved = exact_json_object_v1(
            &candidate.resolved_input,
            &[
                "references",
                "instance_id",
                "instance_kind",
                "manifest_digest",
            ],
        )?;
        validate_register_payload_digest_roles_v1(
            planned,
            resolved,
            kind,
            manifest_digest,
            candidate.resolved_instance_manifest_digest.as_ref(),
        )?;
        let references = planned
            .get("references")
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        exact_planned_reference_slots_v1(references, &["guild_id"], candidate.key.action_index)?;
        let guild = references
            .as_array()
            .and_then(|references| references.first())
            .and_then(Value::as_object)
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        if guild.get("source").and_then(Value::as_str) != Some("existing")
            || canonical_decimal_string_v1(exact_json_string_v1(guild, "id")?, false)?
                != target.guild_id().get()
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        return Ok(());
    }
    let (payload_digest, reference_slots) = match target {
        InteractionEffectRecoveryTargetV1::PostPanel { payload_digest, .. } => {
            (payload_digest, &["channel_id", "guild_id"][..])
        }
        InteractionEffectRecoveryTargetV1::EditResponse { payload_digest, .. } => {
            (payload_digest, &[][..])
        }
        _ => return Ok(()),
    };
    let object = exact_json_object_v1(
        &candidate.planned_recovery_input,
        &["references", "payload_digest"],
    )?;
    if InteractionEffectPayloadDigestV1::parse(exact_json_string_v1(object, "payload_digest")?)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?
        != *payload_digest
    {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    let references = object
        .get("references")
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    exact_planned_reference_slots_v1(references, reference_slots, candidate.key.action_index)?;
    if let InteractionEffectRecoveryTargetV1::PostPanel {
        guild_id,
        channel_id,
        ..
    } = target
    {
        let references = references
            .as_array()
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        let channel = references[0]
            .as_object()
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        if channel.get("source").and_then(Value::as_str) == Some("existing")
            && canonical_decimal_string_v1(exact_json_string_v1(channel, "id")?, false)?
                != channel_id.get()
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let guild = references[1]
            .as_object()
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        if canonical_decimal_string_v1(exact_json_string_v1(guild, "id")?, false)? != guild_id.get()
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
    }
    Ok(())
}

fn validate_register_payload_digest_roles_v1(
    planned: &serde_json::Map<String, Value>,
    resolved: &serde_json::Map<String, Value>,
    kind: &InstanceKind,
    logical_manifest_digest: &InteractionEffectPayloadDigestV1,
    resolved_resource_manifest_digest: Option<&InteractionInstanceManifestDigestV1>,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    if resolved_resource_manifest_digest.is_none()
        || planned.get("instance_id") != resolved.get("instance_id")
        || planned.get("instance_kind") != resolved.get("instance_kind")
        || planned.get("manifest_digest") != resolved.get("manifest_digest")
        || exact_json_string_v1(resolved, "instance_kind")? != kind.0
        || InteractionEffectPayloadDigestV1::parse(exact_json_string_v1(
            resolved,
            "manifest_digest",
        )?)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?
            != *logical_manifest_digest
    {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn output_identity_from_recovery_observed_v1(
    recovered: &RuntimeInteractionEffectRecoveryBindingV1,
    output: &InteractionEffectObservedOutputV1,
) -> Result<Option<RuntimeInteractionEffectOutputIdentityV1>, RuntimeInteractionPersistenceErrorV1>
{
    recovered
        .binding()
        .validate_observed_output(output)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
    match output {
        InteractionEffectObservedOutputV1::CreatedRole { role_id, .. } => Ok(Some(
            RuntimeInteractionEffectOutputIdentityV1::discord(role_id.get())?,
        )),
        InteractionEffectObservedOutputV1::CreatedChannel { channel_id, .. } => Ok(Some(
            RuntimeInteractionEffectOutputIdentityV1::discord(channel_id.get())?,
        )),
        InteractionEffectObservedOutputV1::InstanceState { .. } => {
            Ok(Some(RuntimeInteractionEffectOutputIdentityV1::instance(
                recovered
                    .instance_id()
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?
                    .clone(),
            )))
        }
        InteractionEffectObservedOutputV1::RoleMembership { .. }
        | InteractionEffectObservedOutputV1::PermissionOverwrite { .. } => Ok(None),
        InteractionEffectObservedOutputV1::PostedMessage { .. }
        | InteractionEffectObservedOutputV1::OriginalResponse { .. } => {
            Err(RuntimeInteractionPersistenceErrorV1::InvalidInput)
        }
    }
}

fn decode_recovery_target_v1(
    candidate: &RuntimeInteractionEffectRecoveryCandidateV1,
) -> Result<
    (InteractionEffectRecoveryTargetV1, Option<InstanceId>),
    RuntimeInteractionPersistenceErrorV1,
> {
    let document = &candidate.resolved_input;
    let decoded = match candidate.kind {
        InteractionEffectKindV1::CreateRole => {
            let object = exact_json_object_v1(document, &["references"])?;
            let references = exact_resolved_references_v1(
                object
                    .get("references")
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
                &["guild_id"],
            )?;
            (
                InteractionEffectRecoveryTargetV1::CreateRole {
                    guild_id: effect_guild_id_v1(references[0])?,
                },
                None,
            )
        }
        InteractionEffectKindV1::CreateChannel => {
            let object = exact_json_object_v1(document, &["references"])?;
            let references = exact_resolved_references_v1(
                object
                    .get("references")
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
                &["guild_id"],
            )?;
            (
                InteractionEffectRecoveryTargetV1::CreateChannel {
                    guild_id: effect_guild_id_v1(references[0])?,
                },
                None,
            )
        }
        InteractionEffectKindV1::GrantRole => {
            let object = exact_json_object_v1(document, &["references"])?;
            let references = exact_resolved_references_v1(
                object
                    .get("references")
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
                &["guild_id", "role_id", "user_id"],
            )?;
            (
                InteractionEffectRecoveryTargetV1::GrantRole {
                    target: InteractionEffectRoleMembershipTargetV1::new(
                        effect_guild_id_v1(references[0])?,
                        effect_user_id_v1(references[2])?,
                        effect_role_id_v1(references[1])?,
                    ),
                },
                None,
            )
        }
        InteractionEffectKindV1::UpsertOverwrite => {
            let object = exact_json_object_v1(
                document,
                &[
                    "references",
                    "target_kind",
                    "permission_allow",
                    "permission_deny",
                ],
            )?;
            let references = exact_resolved_references_v1(
                object
                    .get("references")
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
                &["channel_id", "guild_id", "target_id"],
            )?;
            let target_kind = exact_json_string_v1(object, "target_kind")?;
            let overwrite_target = effect_overwrite_target_v1(target_kind, references[2])?;
            let desired = effect_permission_value_v1(
                canonical_decimal_value_v1(object, "permission_allow", true)?,
                canonical_decimal_value_v1(object, "permission_deny", true)?,
            )?;
            (
                InteractionEffectRecoveryTargetV1::UpsertOverwrite {
                    target: InteractionEffectPermissionTargetV1::new(
                        effect_guild_id_v1(references[1])?,
                        effect_channel_id_v1(references[0])?,
                        overwrite_target,
                    ),
                    desired,
                },
                None,
            )
        }
        InteractionEffectKindV1::PostPanel => {
            let object = exact_json_object_v1(document, &["references", "payload_digest"])?;
            let references = exact_resolved_references_v1(
                object
                    .get("references")
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
                &["channel_id", "guild_id"],
            )?;
            (
                InteractionEffectRecoveryTargetV1::PostPanel {
                    guild_id: effect_guild_id_v1(references[1])?,
                    channel_id: effect_channel_id_v1(references[0])?,
                    payload_digest: InteractionEffectPayloadDigestV1::parse(exact_json_string_v1(
                        object,
                        "payload_digest",
                    )?)
                    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
                },
                None,
            )
        }
        InteractionEffectKindV1::RegisterInstance | InteractionEffectKindV1::TeardownInstance => {
            let object = if candidate.kind == InteractionEffectKindV1::RegisterInstance {
                exact_json_object_v1(
                    document,
                    &[
                        "references",
                        "instance_id",
                        "instance_kind",
                        "manifest_digest",
                    ],
                )?
            } else {
                exact_json_object_v1(document, &["references", "instance_id"])?
            };
            let references = exact_resolved_references_v1(
                object
                    .get("references")
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
                &["guild_id"],
            )?;
            let instance_id = InstanceId::parse(exact_json_string_v1(object, "instance_id")?)
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
            let guild_id = effect_guild_id_v1(references[0])?;
            let planned =
                InteractionEffectPlannedInstanceTargetV1::new(guild_id, instance_id.clone());
            let exact_target = InteractionEffectInstanceTargetV1::new(
                guild_id,
                planned.instance_identity_digest().clone(),
            );
            let target = if candidate.kind == InteractionEffectKindV1::RegisterInstance {
                candidate
                    .resolved_instance_manifest_digest
                    .as_ref()
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
                InteractionEffectRecoveryTargetV1::RegisterInstance {
                    target: exact_target,
                    kind: effect_instance_kind_v1(exact_json_string_v1(object, "instance_kind")?)?,
                    manifest_digest: InteractionEffectPayloadDigestV1::parse(exact_json_string_v1(
                        object,
                        "manifest_digest",
                    )?)
                    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
                }
            } else {
                if candidate.resolved_instance_manifest_digest.is_some() {
                    return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
                }
                InteractionEffectRecoveryTargetV1::TeardownInstance {
                    target: exact_target,
                }
            };
            (target, Some(instance_id))
        }
        InteractionEffectKindV1::EditResponse => {
            let object = exact_json_object_v1(document, &["references", "payload_digest"])?;
            let references = exact_resolved_references_v1(
                object
                    .get("references")
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
                &[],
            )?;
            if !references.is_empty() {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            (
                InteractionEffectRecoveryTargetV1::EditResponse {
                    receipt_identity: candidate.key.identity,
                    payload_digest: InteractionEffectPayloadDigestV1::parse(exact_json_string_v1(
                        object,
                        "payload_digest",
                    )?)
                    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
                },
                None,
            )
        }
    };
    Ok(decoded)
}

fn decode_recovery_preimage_v1(
    candidate: &RuntimeInteractionEffectRecoveryCandidateV1,
    target: &InteractionEffectRecoveryTargetV1,
    instance_id: Option<&InstanceId>,
) -> Result<InteractionEffectPreimageV1, RuntimeInteractionPersistenceErrorV1> {
    let document = &candidate.resolved_preimage;
    match target {
        InteractionEffectRecoveryTargetV1::CreateRole { .. }
        | InteractionEffectRecoveryTargetV1::CreateChannel { .. }
        | InteractionEffectRecoveryTargetV1::PostPanel { .. }
        | InteractionEffectRecoveryTargetV1::EditResponse { .. } => {
            let object = exact_json_object_v1(document, &["kind"])?;
            if exact_json_string_v1(object, "kind")? != "none" {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            Ok(InteractionEffectPreimageV1::None)
        }
        InteractionEffectRecoveryTargetV1::GrantRole {
            target: expected_target,
        } => {
            let object = exact_json_object_v1(document, &["kind", "references", "present"])?;
            if exact_json_string_v1(object, "kind")? != "role_membership" {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            let references = exact_resolved_references_v1(
                object
                    .get("references")
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
                &["guild_id", "role_id", "user_id"],
            )?;
            let decoded_target = InteractionEffectRoleMembershipTargetV1::new(
                effect_guild_id_v1(references[0])?,
                effect_user_id_v1(references[2])?,
                effect_role_id_v1(references[1])?,
            );
            if &decoded_target != expected_target {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            Ok(InteractionEffectPreimageV1::RoleMembership {
                target: decoded_target,
                present: exact_json_bool_v1(object, "present")?,
            })
        }
        InteractionEffectRecoveryTargetV1::UpsertOverwrite {
            target: expected_target,
            ..
        } => {
            let base_keys = ["kind", "references", "target_kind", "state"];
            let state = document
                .as_object()
                .and_then(|object| object.get("state"))
                .and_then(Value::as_str)
                .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
            let object = match state {
                "absent" => exact_json_object_v1(document, &base_keys)?,
                "present" => exact_json_object_v1(
                    document,
                    &[
                        "kind",
                        "references",
                        "target_kind",
                        "state",
                        "permission_allow",
                        "permission_deny",
                    ],
                )?,
                _ => return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
            };
            if exact_json_string_v1(object, "kind")? != "permission_overwrite" {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            let references = exact_resolved_references_v1(
                object
                    .get("references")
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
                &["channel_id", "guild_id", "target_id"],
            )?;
            let target_kind = exact_json_string_v1(object, "target_kind")?;
            let decoded_target = InteractionEffectPermissionTargetV1::new(
                effect_guild_id_v1(references[1])?,
                effect_channel_id_v1(references[0])?,
                effect_overwrite_target_v1(target_kind, references[2])?,
            );
            if &decoded_target != expected_target {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            let before = match state {
                "absent" => InteractionEffectPermissionStateV1::Absent,
                "present" => {
                    InteractionEffectPermissionStateV1::Present(effect_permission_value_v1(
                        canonical_decimal_value_v1(object, "permission_allow", true)?,
                        canonical_decimal_value_v1(object, "permission_deny", true)?,
                    )?)
                }
                _ => unreachable!(),
            };
            Ok(InteractionEffectPreimageV1::PermissionOverwrite {
                target: decoded_target,
                before,
            })
        }
        InteractionEffectRecoveryTargetV1::RegisterInstance {
            target: expected_target,
            ..
        }
        | InteractionEffectRecoveryTargetV1::TeardownInstance {
            target: expected_target,
        } => {
            let state = document
                .as_object()
                .and_then(|object| object.get("state"))
                .and_then(Value::as_str)
                .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
            let object = match state {
                "absent" => {
                    exact_json_object_v1(document, &["kind", "references", "instance_id", "state"])?
                }
                "present" => exact_json_object_v1(
                    document,
                    &[
                        "kind",
                        "references",
                        "instance_id",
                        "state",
                        "manifest_digest",
                    ],
                )?,
                _ => return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
            };
            if exact_json_string_v1(object, "kind")? != "instance_registration" {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            let references = exact_resolved_references_v1(
                object
                    .get("references")
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
                &["guild_id"],
            )?;
            let expected_instance =
                instance_id.ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
            if exact_json_string_v1(object, "instance_id")? != expected_instance.as_str()
                || effect_guild_id_v1(references[0])? != expected_target.guild_id()
            {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            let before = match state {
                "absent" => InteractionEffectInstanceStateV1::Absent,
                "present" => InteractionEffectInstanceStateV1::Present {
                    manifest_digest: InteractionEffectPayloadDigestV1::parse(exact_json_string_v1(
                        object,
                        "manifest_digest",
                    )?)
                    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
                },
                _ => unreachable!(),
            };
            Ok(InteractionEffectPreimageV1::InstanceRegistration {
                target: expected_target.clone(),
                before,
            })
        }
    }
}

fn recovery_successful_output_v1(
    candidate: &RuntimeInteractionEffectRecoveryCandidateV1,
    binding: &InteractionEffectRecoveryBindingV1,
    instance_id: Option<&InstanceId>,
) -> Result<Option<InteractionEffectObservedOutputV1>, RuntimeInteractionPersistenceErrorV1> {
    if candidate.success_binding.is_none() {
        return Ok(None);
    }
    let discord_id = || match candidate.output_identity.as_ref() {
        Some(RuntimeInteractionEffectOutputIdentityV1::Discord(value)) => Ok(value.get()),
        _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    };
    let output = match binding.target() {
        InteractionEffectRecoveryTargetV1::CreateRole { guild_id } => {
            Some(InteractionEffectObservedOutputV1::CreatedRole {
                guild_id: *guild_id,
                role_id: effect_role_id_v1(discord_id()?)?,
            })
        }
        InteractionEffectRecoveryTargetV1::CreateChannel { guild_id } => {
            Some(InteractionEffectObservedOutputV1::CreatedChannel {
                guild_id: *guild_id,
                channel_id: effect_channel_id_v1(discord_id()?)?,
            })
        }
        InteractionEffectRecoveryTargetV1::GrantRole { target } => {
            if candidate.output_identity.is_some() {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            Some(InteractionEffectObservedOutputV1::RoleMembership {
                target: *target,
                present: true,
            })
        }
        InteractionEffectRecoveryTargetV1::UpsertOverwrite { target, desired } => {
            if candidate.output_identity.is_some() {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            Some(InteractionEffectObservedOutputV1::PermissionOverwrite {
                target: *target,
                state: InteractionEffectPermissionStateV1::Present(*desired),
            })
        }
        InteractionEffectRecoveryTargetV1::PostPanel {
            guild_id,
            channel_id,
            payload_digest,
        } => Some(InteractionEffectObservedOutputV1::PostedMessage {
            guild_id: *guild_id,
            channel_id: *channel_id,
            message_id: InteractionEffectMessageIdV1::new(discord_id()?)
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
            payload_digest: payload_digest.clone(),
        }),
        InteractionEffectRecoveryTargetV1::EditResponse {
            receipt_identity,
            payload_digest,
        } => {
            if candidate.output_identity.is_some() {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            Some(InteractionEffectObservedOutputV1::OriginalResponse {
                receipt_identity: *receipt_identity,
                payload_digest: payload_digest.clone(),
            })
        }
        InteractionEffectRecoveryTargetV1::RegisterInstance {
            target,
            manifest_digest,
            ..
        } => {
            exact_instance_output_identity_v1(candidate, instance_id)?;
            Some(InteractionEffectObservedOutputV1::InstanceState {
                target: target.clone(),
                state: InteractionEffectInstanceStateV1::Present {
                    manifest_digest: manifest_digest.clone(),
                },
            })
        }
        InteractionEffectRecoveryTargetV1::TeardownInstance { target } => {
            exact_instance_output_identity_v1(candidate, instance_id)?;
            Some(InteractionEffectObservedOutputV1::InstanceState {
                target: target.clone(),
                state: InteractionEffectInstanceStateV1::Absent,
            })
        }
    };
    if let Some(output) = &output {
        binding
            .validate_observed_output(output)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    }
    Ok(output)
}

fn exact_instance_output_identity_v1(
    candidate: &RuntimeInteractionEffectRecoveryCandidateV1,
    instance_id: Option<&InstanceId>,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    if !matches!(
        (candidate.output_identity.as_ref(), instance_id),
        (Some(RuntimeInteractionEffectOutputIdentityV1::Instance(observed)), Some(expected))
            if observed == expected
    ) {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn exact_json_object_v1<'a>(
    document: &'a Value,
    keys: &[&str],
) -> Result<&'a serde_json::Map<String, Value>, RuntimeInteractionPersistenceErrorV1> {
    let object = document
        .as_object()
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(object)
}

fn exact_json_string_v1<'a>(
    object: &'a serde_json::Map<String, Value>,
    key: &str,
) -> Result<&'a str, RuntimeInteractionPersistenceErrorV1> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn exact_json_bool_v1(
    object: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<bool, RuntimeInteractionPersistenceErrorV1> {
    object
        .get(key)
        .and_then(Value::as_bool)
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn exact_resolved_references_v1(
    document: &Value,
    expected_slots: &[&str],
) -> Result<Vec<u64>, RuntimeInteractionPersistenceErrorV1> {
    let references = document
        .as_array()
        .filter(|references| references.len() == expected_slots.len())
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    references
        .iter()
        .zip(expected_slots)
        .map(|(reference, expected_slot)| {
            let object = exact_json_object_v1(reference, &["slot", "id"])?;
            if exact_json_string_v1(object, "slot")? != *expected_slot {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            canonical_decimal_string_v1(exact_json_string_v1(object, "id")?, false)
        })
        .collect()
}

fn exact_planned_reference_slots_v1(
    document: &Value,
    expected_slots: &[&str],
    consumer: InteractionEffectActionIndexV1,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    let references = document
        .as_array()
        .filter(|references| references.len() == expected_slots.len())
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    for (reference, expected_slot) in references.iter().zip(expected_slots) {
        let object = reference
            .as_object()
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        if object.get("slot").and_then(Value::as_str) != Some(*expected_slot) {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        match object.get("source").and_then(Value::as_str) {
            Some("existing") => {
                exact_json_object_v1(reference, &["slot", "source", "id"])?;
                canonical_decimal_string_v1(exact_json_string_v1(object, "id")?, false)?;
            }
            Some("action_output") => {
                if *expected_slot != "channel_id" {
                    return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
                }
                exact_json_object_v1(
                    reference,
                    &[
                        "slot",
                        "source",
                        "action_index",
                        "output_kind",
                        "producer_identity_digest",
                    ],
                )?;
                let action_index = object
                    .get("action_index")
                    .and_then(Value::as_u64)
                    .and_then(|value| u16::try_from(value).ok())
                    .and_then(|value| InteractionEffectActionIndexV1::new(value).ok())
                    .filter(|value| *value < consumer)
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
                if object.get("output_kind").and_then(Value::as_str) != Some("created_channel")
                    || action_index >= consumer
                {
                    return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
                }
                InteractionEffectPlannedIdentityDigestV1::parse(exact_json_string_v1(
                    object,
                    "producer_identity_digest",
                )?)
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
            }
            _ => return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
        }
    }
    Ok(())
}

fn canonical_decimal_value_v1(
    object: &serde_json::Map<String, Value>,
    key: &str,
    allow_zero: bool,
) -> Result<u64, RuntimeInteractionPersistenceErrorV1> {
    canonical_decimal_string_v1(exact_json_string_v1(object, key)?, allow_zero)
}

fn canonical_decimal_string_v1(
    value: &str,
    allow_zero: bool,
) -> Result<u64, RuntimeInteractionPersistenceErrorV1> {
    value
        .parse::<u64>()
        .ok()
        .filter(|parsed| (allow_zero || *parsed > 0) && parsed.to_string() == value)
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn effect_guild_id_v1(
    value: u64,
) -> Result<InteractionEffectGuildIdV1, RuntimeInteractionPersistenceErrorV1> {
    InteractionEffectGuildIdV1::new(value)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn effect_role_id_v1(
    value: u64,
) -> Result<InteractionEffectRoleIdV1, RuntimeInteractionPersistenceErrorV1> {
    InteractionEffectRoleIdV1::new(value)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn effect_channel_id_v1(
    value: u64,
) -> Result<InteractionEffectChannelIdV1, RuntimeInteractionPersistenceErrorV1> {
    InteractionEffectChannelIdV1::new(value)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn effect_user_id_v1(
    value: u64,
) -> Result<InteractionEffectUserIdV1, RuntimeInteractionPersistenceErrorV1> {
    InteractionEffectUserIdV1::new(value)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn effect_overwrite_target_v1(
    kind: &str,
    value: u64,
) -> Result<InteractionEffectOverwriteTargetV1, RuntimeInteractionPersistenceErrorV1> {
    match kind {
        "role" => Ok(InteractionEffectOverwriteTargetV1::Role(effect_role_id_v1(
            value,
        )?)),
        "member" => Ok(InteractionEffectOverwriteTargetV1::Member(
            effect_user_id_v1(value)?,
        )),
        _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    }
}

fn effect_permission_value_v1(
    allow: u64,
    deny: u64,
) -> Result<InteractionEffectPermissionValueV1, RuntimeInteractionPersistenceErrorV1> {
    InteractionEffectPermissionValueV1::new(allow, deny)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn effect_instance_kind_v1(
    value: &str,
) -> Result<InstanceKind, RuntimeInteractionPersistenceErrorV1> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(InstanceKind(value.to_string()))
}

fn validate_effect_database_time_v1(
    value: DateTime<Utc>,
    allow_epoch: bool,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    let milliseconds = value.timestamp_millis();
    if milliseconds < 0 || (!allow_epoch && milliseconds == 0) {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn validate_recovery_route_v1(
    candidate: &RuntimeInteractionEffectRecoveryCandidateV1,
    expected: &InteractionExpectedRouteV1,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    let origin = candidate.origin.claim_root.route();
    if origin.scope() != expected.scope()
        || origin.process_identity().target != expected.process_identity().target
        || origin.process_identity().runtime_generation
            != expected.process_identity().runtime_generation
        || origin.serving_identity().route_fencing_token() != expected.route_fencing_token()
        || origin.serving_identity().route_incarnation() != expected.route_incarnation()
    {
        return Err(RuntimeInteractionPersistenceErrorV1::InvalidAuthority);
    }
    Ok(())
}

fn validate_recovered_definition_v1(
    candidate: &RuntimeInteractionEffectRecoveryCandidateV1,
    materialized: &InteractionEffectMaterializedPlanV1,
    expected_postimage_digest: &InteractionEffectExpectedPostimageDigestV1,
    resolved_instance_manifest_digest: Option<&InteractionInstanceManifestDigestV1>,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    let definition = materialized.definition();
    let action = definition.action();
    if action.receipt_identity() != candidate.key.identity
        || action.action_index() != candidate.key.action_index
        || action.kind() != candidate.kind
        || action.action_plan_digest() != &candidate.action_plan_digest
        || action.preflight_certificate_digest() != &candidate.preflight_certificate_digest
        || action.input_digest() != &candidate.input_digest
        || materialized.planned_identity_digest() != &candidate.planned_identity_digest
        || expected_postimage_digest != &candidate.expected_postimage_digest
        || build_interaction_effect_identity_digest_v1(definition)
            != candidate.resolved_effect_identity_digest
        || build_interaction_effect_preimage_digest_v1(definition.preimage())
            != candidate.resolved_preimage_digest
        || definition.output_class() != candidate.output_class
        || candidate.resolved_instance_manifest_digest.as_ref() != resolved_instance_manifest_digest
    {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    let planned_dependencies = definition
        .dependencies()
        .iter()
        .map(|dependency| dependency.planned().clone())
        .collect::<Vec<_>>();
    let planned = InteractionEffectPlanDefinitionV1::new(
        action.clone(),
        materialized.planned_recovery_input().clone(),
        planned_dependencies,
    )
    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    let plan_action =
        RuntimeInteractionEffectPlanActionV1::new(planned, expected_postimage_digest.clone())?;
    let resolved_input =
        resolved_recovery_input_document_v1(materialized, resolved_instance_manifest_digest)?;
    let resolved_preimage = resolved_preimage_document_v1(materialized)?;
    let correlation = build_interaction_effect_correlation_v1(definition);
    if plan_action.planned_recovery_input != candidate.planned_recovery_input
        || plan_action.planned_preimage != candidate.planned_preimage
        || plan_action.planned_preimage_digest != candidate.planned_preimage_digest
        || plan_action.correlation.class() != candidate.correlation_class
        || plan_action.correlation.marker_digest() != &candidate.correlation_digest
        || correlation.class() != candidate.correlation_class
        || correlation.marker_digest() != &candidate.correlation_digest
        || correlation_marker_v1(&correlation) != candidate.correlation_marker
        || resolved_input != candidate.resolved_input
        || resolved_preimage != candidate.resolved_preimage
        || definition
            .dependencies()
            .iter()
            .map(|dependency| dependency.action_index())
            .collect::<Vec<_>>()
            != candidate.dependency_indices
    {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn successful_output_v1(
    candidate: &RuntimeInteractionEffectRecoveryCandidateV1,
    materialized: &InteractionEffectMaterializedPlanV1,
) -> Result<InteractionEffectObservedOutputV1, RuntimeInteractionPersistenceErrorV1> {
    use automation_runtime_interaction::{
        InteractionEffectChannelIdV1, InteractionEffectInstanceStateV1,
        InteractionEffectMessageIdV1, InteractionEffectPermissionStateV1,
        InteractionEffectRoleIdV1,
    };
    let target = materialized.definition().target();
    let discord_id = || match candidate.output_identity.as_ref() {
        Some(RuntimeInteractionEffectOutputIdentityV1::Discord(value)) => Ok(value.get()),
        _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    };
    let output = match target {
        InteractionEffectTargetV1::CreateRole { guild_id } => {
            InteractionEffectObservedOutputV1::CreatedRole {
                guild_id: *guild_id,
                role_id: InteractionEffectRoleIdV1::new(discord_id()?)
                    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
            }
        }
        InteractionEffectTargetV1::CreateChannel { guild_id } => {
            InteractionEffectObservedOutputV1::CreatedChannel {
                guild_id: *guild_id,
                channel_id: InteractionEffectChannelIdV1::new(discord_id()?)
                    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
            }
        }
        InteractionEffectTargetV1::GrantRole { target } => {
            if candidate.output_identity.is_some() {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            InteractionEffectObservedOutputV1::RoleMembership {
                target: *target,
                present: true,
            }
        }
        InteractionEffectTargetV1::UpsertOverwrite { target, desired } => {
            if candidate.output_identity.is_some() {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            InteractionEffectObservedOutputV1::PermissionOverwrite {
                target: *target,
                state: InteractionEffectPermissionStateV1::Present(*desired),
            }
        }
        InteractionEffectTargetV1::PostPanel {
            guild_id,
            channel_id,
            payload_digest,
        } => InteractionEffectObservedOutputV1::PostedMessage {
            guild_id: *guild_id,
            channel_id: *channel_id,
            message_id: InteractionEffectMessageIdV1::new(discord_id()?)
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
            payload_digest: payload_digest.clone(),
        },
        InteractionEffectTargetV1::RegisterInstance {
            target,
            manifest_digest,
            ..
        } => {
            let expected_instance = planned_instance_id_v1(materialized)?;
            if !matches!(
                candidate.output_identity.as_ref(),
                Some(RuntimeInteractionEffectOutputIdentityV1::Instance(instance))
                    if instance == expected_instance
            ) {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            InteractionEffectObservedOutputV1::InstanceState {
                target: target.clone(),
                state: InteractionEffectInstanceStateV1::Present {
                    manifest_digest: manifest_digest.clone(),
                },
            }
        }
        InteractionEffectTargetV1::TeardownInstance { target } => {
            let expected_instance = planned_instance_id_v1(materialized)?;
            if !matches!(
                candidate.output_identity.as_ref(),
                Some(RuntimeInteractionEffectOutputIdentityV1::Instance(instance))
                    if instance == expected_instance
            ) {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            InteractionEffectObservedOutputV1::InstanceState {
                target: target.clone(),
                state: InteractionEffectInstanceStateV1::Absent,
            }
        }
        InteractionEffectTargetV1::EditResponse {
            receipt_identity,
            payload_digest,
        } => {
            if candidate.output_identity.is_some() {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            InteractionEffectObservedOutputV1::OriginalResponse {
                receipt_identity: *receipt_identity,
                payload_digest: payload_digest.clone(),
            }
        }
    };
    materialized
        .definition()
        .validate_observed_output(&output)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    Ok(output)
}

fn output_identity_from_observed_v1(
    materialized: &InteractionEffectMaterializedPlanV1,
    output: &InteractionEffectObservedOutputV1,
) -> Result<Option<RuntimeInteractionEffectOutputIdentityV1>, RuntimeInteractionPersistenceErrorV1>
{
    output_identity_v1(
        materialized,
        &InteractionEffectAttemptOutcomeV1::KnownSucceeded(output.clone()),
    )
}

fn validate_claim_definition_v1(
    claim: &RuntimeInteractionReceiptExclusiveClaimV1,
    definition: &InteractionEffectDefinitionV1,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    let action = definition.action();
    if action.receipt_identity() != claim.claim_root().identity()
        || claim.action_plan_digest() != Some(action.action_plan_digest())
    {
        return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
    }
    Ok(())
}

fn retry_delay_milliseconds_v1(
    delay: Duration,
) -> Result<i64, RuntimeInteractionPersistenceErrorV1> {
    if delay < MIN_RUNTIME_INTERACTION_EFFECT_RETRY_DELAY {
        return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
    }
    validate_millisecond_duration(delay, MAX_RUNTIME_INTERACTION_EFFECT_RETRY_DELAY)
}

fn validate_recovery_document_v1(
    document: &Value,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    let bytes = serde_json::to_vec(document)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
    if !document.is_object()
        || bytes.is_empty()
        || bytes.len() > MAX_RUNTIME_INTERACTION_EFFECT_RECOVERY_DOCUMENT_BYTES
    {
        return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
    }
    Ok(())
}

fn planned_recovery_input_document_v1(
    input: &InteractionEffectPlannedRecoveryInputV1,
) -> Result<Value, RuntimeInteractionPersistenceErrorV1> {
    match input.target() {
        InteractionEffectPlannedTargetV1::CreateRole { guild_id }
        | InteractionEffectPlannedTargetV1::CreateChannel { guild_id } => Ok(json!({
            "references": [existing_reference_v1("guild_id", guild_id.get())]
        })),
        InteractionEffectPlannedTargetV1::GrantRole { target } => Ok(json!({
            "references": planned_role_membership_references_v1(target)
        })),
        InteractionEffectPlannedTargetV1::UpsertOverwrite { target, desired } => Ok(json!({
            "references": planned_permission_references_v1(target),
            "target_kind": planned_overwrite_target_kind_v1(target.target()),
            "permission_allow": desired.allow().to_string(),
            "permission_deny": desired.deny().to_string()
        })),
        InteractionEffectPlannedTargetV1::PostPanel {
            guild_id,
            channel,
            payload_digest,
        } => Ok(json!({
            "references": [
                planned_channel_reference_v1("channel_id", channel),
                existing_reference_v1("guild_id", guild_id.get())
            ],
            "payload_digest": payload_digest.as_str()
        })),
        InteractionEffectPlannedTargetV1::RegisterInstance {
            target,
            kind,
            manifest_digest,
        } => Ok(json!({
            "references": [existing_reference_v1("guild_id", target.guild_id().get())],
            "instance_id": target.instance_id().as_str(),
            "instance_kind": kind.0,
            "manifest_digest": manifest_digest.as_str()
        })),
        InteractionEffectPlannedTargetV1::TeardownInstance { target } => Ok(json!({
            "references": [existing_reference_v1("guild_id", target.guild_id().get())],
            "instance_id": target.instance_id().as_str()
        })),
        InteractionEffectPlannedTargetV1::EditResponse { payload_digest, .. } => Ok(json!({
            "references": [],
            "payload_digest": payload_digest.as_str()
        })),
    }
}

fn planned_preimage_document_v1(
    preimage: &InteractionEffectPlannedPreimageV1,
) -> Result<Value, RuntimeInteractionPersistenceErrorV1> {
    match preimage {
        InteractionEffectPlannedPreimageV1::None => Ok(json!({ "kind": "none" })),
        InteractionEffectPlannedPreimageV1::RoleMembership { target, present } => Ok(json!({
            "kind": "role_membership",
            "references": planned_role_membership_references_v1(target),
            "present": present
        })),
        InteractionEffectPlannedPreimageV1::PermissionOverwrite { target, before } => {
            let mut document = json!({
                "kind": "permission_overwrite",
                "references": planned_permission_references_v1(target),
                "target_kind": planned_overwrite_target_kind_v1(target.target()),
                "state": permission_state_code_v1(before)
            });
            if let automation_runtime_interaction::InteractionEffectPermissionStateV1::Present(
                value,
            ) = before
            {
                let object = document
                    .as_object_mut()
                    .ok_or(RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
                object.insert(
                    "permission_allow".to_string(),
                    json!(value.allow().to_string()),
                );
                object.insert(
                    "permission_deny".to_string(),
                    json!(value.deny().to_string()),
                );
            }
            Ok(document)
        }
        InteractionEffectPlannedPreimageV1::InstanceRegistration { target, before } => {
            let mut document = json!({
                "kind": "instance_registration",
                "references": [existing_reference_v1("guild_id", target.guild_id().get())],
                "instance_id": target.instance_id().as_str(),
                "state": instance_state_code_v1(before)
            });
            if let automation_runtime_interaction::InteractionEffectInstanceStateV1::Present {
                manifest_digest,
            } = before
            {
                document
                    .as_object_mut()
                    .ok_or(RuntimeInteractionPersistenceErrorV1::InvalidInput)?
                    .insert(
                        "manifest_digest".to_string(),
                        json!(manifest_digest.as_str()),
                    );
            }
            Ok(document)
        }
    }
}

fn resolved_recovery_input_document_v1(
    materialized: &InteractionEffectMaterializedPlanV1,
    resolved_instance_manifest_digest: Option<&InteractionInstanceManifestDigestV1>,
) -> Result<Value, RuntimeInteractionPersistenceErrorV1> {
    match materialized.resolved_input().target() {
        InteractionEffectTargetV1::CreateRole { guild_id }
        | InteractionEffectTargetV1::CreateChannel { guild_id } => Ok(json!({
            "references": [resolved_reference_v1("guild_id", guild_id.get())]
        })),
        InteractionEffectTargetV1::GrantRole { target } => Ok(json!({
            "references": resolved_role_membership_references_v1(target)
        })),
        InteractionEffectTargetV1::UpsertOverwrite { target, desired } => Ok(json!({
            "references": resolved_permission_references_v1(target),
            "target_kind": resolved_overwrite_target_kind_v1(target.target()),
            "permission_allow": desired.allow().to_string(),
            "permission_deny": desired.deny().to_string()
        })),
        InteractionEffectTargetV1::PostPanel {
            guild_id,
            channel_id,
            payload_digest,
        } => Ok(json!({
            "references": [
                resolved_reference_v1("channel_id", channel_id.get()),
                resolved_reference_v1("guild_id", guild_id.get())
            ],
            "payload_digest": payload_digest.as_str()
        })),
        InteractionEffectTargetV1::RegisterInstance {
            target,
            kind,
            manifest_digest,
        } => {
            resolved_instance_manifest_digest
                .ok_or(RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
            Ok(json!({
                "references": [resolved_reference_v1("guild_id", target.guild_id().get())],
                "instance_id": planned_instance_id_v1(materialized)?.as_str(),
                "instance_kind": kind.0,
                "manifest_digest": manifest_digest.as_str()
            }))
        }
        InteractionEffectTargetV1::TeardownInstance { target } => Ok(json!({
            "references": [resolved_reference_v1("guild_id", target.guild_id().get())],
            "instance_id": planned_instance_id_v1(materialized)?.as_str()
        })),
        InteractionEffectTargetV1::EditResponse { payload_digest, .. } => Ok(json!({
            "references": [],
            "payload_digest": payload_digest.as_str()
        })),
    }
    .and_then(|document| {
        let is_register = matches!(
            materialized.resolved_input().target(),
            InteractionEffectTargetV1::RegisterInstance { .. }
        );
        if !is_register && resolved_instance_manifest_digest.is_some() {
            Err(RuntimeInteractionPersistenceErrorV1::InvalidInput)
        } else {
            Ok(document)
        }
    })
}

fn resolved_preimage_document_v1(
    materialized: &InteractionEffectMaterializedPlanV1,
) -> Result<Value, RuntimeInteractionPersistenceErrorV1> {
    match materialized.resolved_input().preimage() {
        InteractionEffectPreimageV1::None => Ok(json!({ "kind": "none" })),
        InteractionEffectPreimageV1::RoleMembership { target, present } => Ok(json!({
            "kind": "role_membership",
            "references": resolved_role_membership_references_v1(target),
            "present": present
        })),
        InteractionEffectPreimageV1::PermissionOverwrite { target, before } => {
            let mut document = json!({
                "kind": "permission_overwrite",
                "references": resolved_permission_references_v1(target),
                "target_kind": resolved_overwrite_target_kind_v1(target.target()),
                "state": permission_state_code_v1(before)
            });
            if let automation_runtime_interaction::InteractionEffectPermissionStateV1::Present(
                value,
            ) = before
            {
                let object = document
                    .as_object_mut()
                    .ok_or(RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
                object.insert(
                    "permission_allow".to_string(),
                    json!(value.allow().to_string()),
                );
                object.insert(
                    "permission_deny".to_string(),
                    json!(value.deny().to_string()),
                );
            }
            Ok(document)
        }
        InteractionEffectPreimageV1::InstanceRegistration { target, before } => {
            let mut document = json!({
                "kind": "instance_registration",
                "references": [resolved_reference_v1("guild_id", target.guild_id().get())],
                "instance_id": planned_instance_id_v1(materialized)?.as_str(),
                "state": instance_state_code_v1(before)
            });
            if let automation_runtime_interaction::InteractionEffectInstanceStateV1::Present {
                manifest_digest,
            } = before
            {
                document
                    .as_object_mut()
                    .ok_or(RuntimeInteractionPersistenceErrorV1::InvalidInput)?
                    .insert(
                        "manifest_digest".to_string(),
                        json!(manifest_digest.as_str()),
                    );
            }
            Ok(document)
        }
    }
}

fn planned_role_membership_references_v1(
    target: &InteractionEffectPlannedRoleMembershipTargetV1,
) -> Vec<Value> {
    vec![
        existing_reference_v1("guild_id", target.guild_id().get()),
        planned_role_reference_v1("role_id", target.role()),
        existing_reference_v1("user_id", target.user_id().get()),
    ]
}

fn planned_permission_references_v1(
    target: &InteractionEffectPlannedPermissionTargetV1,
) -> Vec<Value> {
    let target_reference = match target.target() {
        InteractionEffectPlannedOverwriteTargetV1::Role(role) => {
            planned_role_reference_v1("target_id", role)
        }
        InteractionEffectPlannedOverwriteTargetV1::Member(user_id) => {
            existing_reference_v1("target_id", user_id.get())
        }
    };
    vec![
        planned_channel_reference_v1("channel_id", target.channel()),
        existing_reference_v1("guild_id", target.guild_id().get()),
        target_reference,
    ]
}

fn resolved_role_membership_references_v1(
    target: &automation_runtime_interaction::InteractionEffectRoleMembershipTargetV1,
) -> Vec<Value> {
    vec![
        resolved_reference_v1("guild_id", target.guild_id().get()),
        resolved_reference_v1("role_id", target.role_id().get()),
        resolved_reference_v1("user_id", target.user_id().get()),
    ]
}

fn resolved_permission_references_v1(
    target: &automation_runtime_interaction::InteractionEffectPermissionTargetV1,
) -> Vec<Value> {
    let target_id = match target.target() {
        automation_runtime_interaction::InteractionEffectOverwriteTargetV1::Role(role_id) => {
            role_id.get()
        }
        automation_runtime_interaction::InteractionEffectOverwriteTargetV1::Member(user_id) => {
            user_id.get()
        }
    };
    vec![
        resolved_reference_v1("channel_id", target.channel_id().get()),
        resolved_reference_v1("guild_id", target.guild_id().get()),
        resolved_reference_v1("target_id", target_id),
    ]
}

fn existing_reference_v1(slot: &str, id: u64) -> Value {
    json!({ "slot": slot, "source": "existing", "id": id.to_string() })
}

fn resolved_reference_v1(slot: &str, id: u64) -> Value {
    json!({ "slot": slot, "id": id.to_string() })
}

fn planned_role_reference_v1(
    slot: &str,
    reference: &InteractionEffectPlannedRoleReferenceV1,
) -> Value {
    match reference {
        InteractionEffectPlannedRoleReferenceV1::Existing(role_id) => {
            existing_reference_v1(slot, role_id.get())
        }
        InteractionEffectPlannedRoleReferenceV1::Produced(dependency) => {
            action_output_reference_v1(slot, dependency)
        }
    }
}

fn planned_channel_reference_v1(
    slot: &str,
    reference: &InteractionEffectPlannedChannelReferenceV1,
) -> Value {
    match reference {
        InteractionEffectPlannedChannelReferenceV1::Existing(channel_id) => {
            existing_reference_v1(slot, channel_id.get())
        }
        InteractionEffectPlannedChannelReferenceV1::Produced(dependency) => {
            action_output_reference_v1(slot, dependency)
        }
    }
}

fn action_output_reference_v1(
    slot: &str,
    dependency: &automation_runtime_interaction::InteractionEffectPlannedDependencyV1,
) -> Value {
    json!({
        "slot": slot,
        "source": "action_output",
        "action_index": dependency.action_index().get(),
        "output_kind": output_class_code_v1(dependency.output_class()),
        "producer_identity_digest": dependency.producer_identity_digest().as_str()
    })
}

fn planned_overwrite_target_kind_v1(
    target: &InteractionEffectPlannedOverwriteTargetV1,
) -> &'static str {
    match target {
        InteractionEffectPlannedOverwriteTargetV1::Role(_) => "role",
        InteractionEffectPlannedOverwriteTargetV1::Member(_) => "member",
    }
}

fn resolved_overwrite_target_kind_v1(
    target: automation_runtime_interaction::InteractionEffectOverwriteTargetV1,
) -> &'static str {
    match target {
        automation_runtime_interaction::InteractionEffectOverwriteTargetV1::Role(_) => "role",
        automation_runtime_interaction::InteractionEffectOverwriteTargetV1::Member(_) => "member",
    }
}

fn permission_state_code_v1(
    state: &automation_runtime_interaction::InteractionEffectPermissionStateV1,
) -> &'static str {
    match state {
        automation_runtime_interaction::InteractionEffectPermissionStateV1::Absent => "absent",
        automation_runtime_interaction::InteractionEffectPermissionStateV1::Present(_) => "present",
    }
}

fn instance_state_code_v1(
    state: &automation_runtime_interaction::InteractionEffectInstanceStateV1,
) -> &'static str {
    match state {
        automation_runtime_interaction::InteractionEffectInstanceStateV1::Absent => "absent",
        automation_runtime_interaction::InteractionEffectInstanceStateV1::Present { .. } => {
            "present"
        }
    }
}

fn planned_instance_id_v1(
    materialized: &InteractionEffectMaterializedPlanV1,
) -> Result<&InstanceId, RuntimeInteractionPersistenceErrorV1> {
    match materialized.planned_recovery_input().target() {
        InteractionEffectPlannedTargetV1::RegisterInstance { target, .. }
        | InteractionEffectPlannedTargetV1::TeardownInstance { target } => Ok(target.instance_id()),
        _ => Err(RuntimeInteractionPersistenceErrorV1::InvalidInput),
    }
}

fn output_identity_v1(
    materialized: &InteractionEffectMaterializedPlanV1,
    outcome: &InteractionEffectAttemptOutcomeV1,
) -> Result<Option<RuntimeInteractionEffectOutputIdentityV1>, RuntimeInteractionPersistenceErrorV1>
{
    let InteractionEffectAttemptOutcomeV1::KnownSucceeded(output) = outcome else {
        return Ok(None);
    };
    match output {
        InteractionEffectObservedOutputV1::CreatedRole { role_id, .. } => Ok(Some(
            RuntimeInteractionEffectOutputIdentityV1::discord(role_id.get())?,
        )),
        InteractionEffectObservedOutputV1::CreatedChannel { channel_id, .. } => Ok(Some(
            RuntimeInteractionEffectOutputIdentityV1::discord(channel_id.get())?,
        )),
        InteractionEffectObservedOutputV1::PostedMessage { message_id, .. } => Ok(Some(
            RuntimeInteractionEffectOutputIdentityV1::discord(message_id.get())?,
        )),
        InteractionEffectObservedOutputV1::InstanceState { .. } => {
            Ok(Some(RuntimeInteractionEffectOutputIdentityV1::instance(
                planned_instance_id_v1(materialized)?.clone(),
            )))
        }
        InteractionEffectObservedOutputV1::RoleMembership { .. }
        | InteractionEffectObservedOutputV1::PermissionOverwrite { .. }
        | InteractionEffectObservedOutputV1::OriginalResponse { .. } => Ok(None),
    }
}

pub(crate) fn decode_effect_state_v1(
    value: &str,
) -> Result<InteractionEffectStateV1, RuntimeInteractionPersistenceErrorV1> {
    match value {
        "planned" => Ok(InteractionEffectStateV1::Planned),
        "intended" => Ok(InteractionEffectStateV1::Intended),
        "known_succeeded" => Ok(InteractionEffectStateV1::KnownSucceeded),
        "known_failed" => Ok(InteractionEffectStateV1::KnownFailed),
        "indeterminate" => Ok(InteractionEffectStateV1::Indeterminate),
        "observing" => Ok(InteractionEffectStateV1::Observing),
        "observation_pending" => Ok(InteractionEffectStateV1::ObservationPending),
        "reconciled_succeeded" => Ok(InteractionEffectStateV1::ReconciledSucceeded),
        "compensation_intended" => Ok(InteractionEffectStateV1::CompensationIntended),
        "compensated" => Ok(InteractionEffectStateV1::Compensated),
        "compensation_indeterminate" => Ok(InteractionEffectStateV1::CompensationIndeterminate),
        "compensation_observing" => Ok(InteractionEffectStateV1::CompensationObserving),
        "compensation_observation_pending" => {
            Ok(InteractionEffectStateV1::CompensationObservationPending)
        }
        "recovery_required" => Ok(InteractionEffectStateV1::RecoveryRequired),
        _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    }
}

pub(crate) fn effect_state_code_v1(state: InteractionEffectStateV1) -> &'static str {
    match state {
        InteractionEffectStateV1::Planned => "planned",
        InteractionEffectStateV1::Intended => "intended",
        InteractionEffectStateV1::KnownSucceeded => "known_succeeded",
        InteractionEffectStateV1::KnownFailed => "known_failed",
        InteractionEffectStateV1::Indeterminate => "indeterminate",
        InteractionEffectStateV1::Observing => "observing",
        InteractionEffectStateV1::ObservationPending => "observation_pending",
        InteractionEffectStateV1::ReconciledSucceeded => "reconciled_succeeded",
        InteractionEffectStateV1::CompensationIntended => "compensation_intended",
        InteractionEffectStateV1::Compensated => "compensated",
        InteractionEffectStateV1::CompensationIndeterminate => "compensation_indeterminate",
        InteractionEffectStateV1::CompensationObserving => "compensation_observing",
        InteractionEffectStateV1::CompensationObservationPending => {
            "compensation_observation_pending"
        }
        InteractionEffectStateV1::RecoveryRequired => "recovery_required",
    }
}

pub(crate) fn recovery_path_code_v1(path: RuntimeInteractionEffectRecoveryPathV1) -> &'static str {
    match path {
        RuntimeInteractionEffectRecoveryPathV1::Observation => "observation",
        RuntimeInteractionEffectRecoveryPathV1::Compensation => "compensation",
        RuntimeInteractionEffectRecoveryPathV1::ResponseTail => "response_tail",
    }
}

pub(crate) fn output_class_code_v1(class: InteractionEffectOutputClassV1) -> &'static str {
    match class {
        InteractionEffectOutputClassV1::CreatedRole => "created_role",
        InteractionEffectOutputClassV1::CreatedChannel => "created_channel",
        InteractionEffectOutputClassV1::RoleMembership => "role_membership",
        InteractionEffectOutputClassV1::PermissionOverwrite => "permission_overwrite",
        InteractionEffectOutputClassV1::PostedMessage => "posted_message",
        InteractionEffectOutputClassV1::InstanceState => "instance_state",
        InteractionEffectOutputClassV1::OriginalResponse => "original_response",
    }
}

pub(crate) fn correlation_class_code_v1(
    class: InteractionEffectCorrelationClassV1,
) -> &'static str {
    match class {
        InteractionEffectCorrelationClassV1::AuditLogReason => "audit_log_reason",
        InteractionEffectCorrelationClassV1::MessageNonce => "message_nonce",
        InteractionEffectCorrelationClassV1::InternalIdempotencyKey => "internal_idempotency_key",
        InteractionEffectCorrelationClassV1::InteractionReceipt => "interaction_receipt",
        InteractionEffectCorrelationClassV1::Unsupported => "unsupported",
    }
}

fn correlation_marker_v1(correlation: &InteractionEffectCorrelationV1) -> Option<String> {
    match correlation.class() {
        InteractionEffectCorrelationClassV1::AuditLogReason
        | InteractionEffectCorrelationClassV1::InternalIdempotencyKey => {
            Some(correlation.marker_digest().as_str().to_string())
        }
        InteractionEffectCorrelationClassV1::MessageNonce => correlation
            .message_nonce()
            .map(|nonce| nonce.get().to_string()),
        InteractionEffectCorrelationClassV1::InteractionReceipt
        | InteractionEffectCorrelationClassV1::Unsupported => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_resolved_references_require_exact_canonical_documents() {
        let exact = json!([
            {"slot": "channel_id", "id": "42"},
            {"slot": "guild_id", "id": "7"}
        ]);
        assert_eq!(
            exact_resolved_references_v1(&exact, &["channel_id", "guild_id"]),
            Ok(vec![42, 7])
        );

        for invalid in [
            json!([
                {"slot": "channel_id", "id": "42", "extra": false},
                {"slot": "guild_id", "id": "7"}
            ]),
            json!([
                {"slot": "channel_id", "id": 42},
                {"slot": "guild_id", "id": "7"}
            ]),
            json!([
                {"slot": "channel_id", "id": "042"},
                {"slot": "guild_id", "id": "7"}
            ]),
            json!([
                {"slot": "channel_id", "id": "0"},
                {"slot": "guild_id", "id": "7"}
            ]),
            json!([
                {"slot": "guild_id", "id": "7"},
                {"slot": "channel_id", "id": "42"}
            ]),
        ] {
            assert_eq!(
                exact_resolved_references_v1(&invalid, &["channel_id", "guild_id"]),
                Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
            );
        }
    }

    #[test]
    fn strict_planned_references_reject_nonprior_or_malformed_producers() {
        let producer = InteractionEffectPlannedIdentityDigestV1::from_canonical_bytes(b"producer");
        let exact = json!([
            {
                "slot": "channel_id",
                "source": "action_output",
                "action_index": 1,
                "output_kind": "created_channel",
                "producer_identity_digest": producer.as_str()
            },
            {"slot": "guild_id", "source": "existing", "id": "7"}
        ]);
        assert_eq!(
            exact_planned_reference_slots_v1(
                &exact,
                &["channel_id", "guild_id"],
                InteractionEffectActionIndexV1::new(2).unwrap(),
            ),
            Ok(())
        );

        let mut nonprior = exact.clone();
        nonprior[0]["action_index"] = json!(2);
        assert_eq!(
            exact_planned_reference_slots_v1(
                &nonprior,
                &["channel_id", "guild_id"],
                InteractionEffectActionIndexV1::new(2).unwrap(),
            ),
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );

        let mut malformed = exact;
        malformed[0]["producer_identity_digest"] = json!("ABC");
        assert_eq!(
            exact_planned_reference_slots_v1(
                &malformed,
                &["channel_id", "guild_id"],
                InteractionEffectActionIndexV1::new(2).unwrap(),
            ),
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );

        let invalid_guild_producer = json!([
            {"slot": "channel_id", "source": "existing", "id": "42"},
            {
                "slot": "guild_id",
                "source": "action_output",
                "action_index": 1,
                "output_kind": "created_channel",
                "producer_identity_digest": producer.as_str()
            }
        ]);
        assert_eq!(
            exact_planned_reference_slots_v1(
                &invalid_guild_producer,
                &["channel_id", "guild_id"],
                InteractionEffectActionIndexV1::new(2).unwrap(),
            ),
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );
    }

    #[test]
    fn strict_permission_decoder_rejects_overlapping_bitsets() {
        assert_eq!(
            effect_permission_value_v1(0b001, 0b010),
            InteractionEffectPermissionValueV1::new(0b001, 0b010)
                .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );
        assert_eq!(
            effect_permission_value_v1(0b011, 0b010),
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );
    }

    #[test]
    fn register_payload_binding_keeps_logical_and_resource_digests_distinct() {
        let logical = InteractionEffectPayloadDigestV1::parse("a".repeat(64)).unwrap();
        let resource = InteractionInstanceManifestDigestV1::parse("b".repeat(64)).unwrap();
        let planned = json!({
            "references": [{"slot": "guild_id", "source": "existing", "id": "7"}],
            "instance_id": "room",
            "instance_kind": "study_room",
            "manifest_digest": logical.as_str()
        });
        let resolved = json!({
            "references": [{"slot": "guild_id", "id": "7"}],
            "instance_id": "room",
            "instance_kind": "study_room",
            "manifest_digest": logical.as_str()
        });
        assert_eq!(
            validate_register_payload_digest_roles_v1(
                planned.as_object().unwrap(),
                resolved.as_object().unwrap(),
                &InstanceKind("study_room".to_string()),
                &logical,
                Some(&resource),
            ),
            Ok(())
        );
    }

    #[test]
    fn register_payload_binding_rejects_logical_resource_digest_swap() {
        let logical = InteractionEffectPayloadDigestV1::parse("a".repeat(64)).unwrap();
        let resource = InteractionInstanceManifestDigestV1::parse("b".repeat(64)).unwrap();
        let planned = json!({
            "references": [{"slot": "guild_id", "source": "existing", "id": "7"}],
            "instance_id": "room",
            "instance_kind": "study_room",
            "manifest_digest": logical.as_str()
        });
        let swapped = json!({
            "references": [{"slot": "guild_id", "id": "7"}],
            "instance_id": "room",
            "instance_kind": "study_room",
            "manifest_digest": resource.as_str()
        });
        assert_eq!(
            validate_register_payload_digest_roles_v1(
                planned.as_object().unwrap(),
                swapped.as_object().unwrap(),
                &InstanceKind("study_room".to_string()),
                &logical,
                Some(&resource),
            ),
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );
    }
}
