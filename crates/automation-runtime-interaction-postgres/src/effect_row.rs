use automation_instance::InstanceId;
use automation_ruleset::{RuleSetContentHash, RuleSetKey};
use automation_runtime_convergence::{
    BindingRevision, RuntimeDeploymentTargetV1, RuntimeGeneration, RuntimeProcessIdentityV1,
};
use automation_runtime_interaction::{
    InteractionActionPlanDigestV1, InteractionEffectActionIndexV1,
    InteractionEffectCompensationIntentDigestV1, InteractionEffectCompensationResultDigestV1,
    InteractionEffectCorrelationClassV1, InteractionEffectCorrelationDigestV1,
    InteractionEffectExpectedPostimageDigestV1, InteractionEffectIdentityDigestV1,
    InteractionEffectInputDigestV1, InteractionEffectIntentDigestV1, InteractionEffectKindV1,
    InteractionEffectObservationDigestV1, InteractionEffectOutputClassV1,
    InteractionEffectPlannedIdentityDigestV1, InteractionEffectPlannedPreimageDigestV1,
    InteractionEffectPreimageDigestV1, InteractionEffectResultDigestV1,
    InteractionGatewayShardIdentityV1, InteractionInstanceManifestDigestV1,
    InteractionPreflightCertificateDigestV1, InteractionPreflightSnapshotDigestV1,
    InteractionReceiptClaimCandidateV1, InteractionRequestDigestV1,
};
use chrono::{DateTime, Utc};
use discord_model::{ChannelId, UserId};
use resource_resolution::ResourceBindingFingerprint;
use serde_json::Value;

use crate::effect::{
    decode_effect_state_v1, RuntimeInteractionEffectCheckpointV1,
    RuntimeInteractionEffectCompensationClaimV1,
    RuntimeInteractionEffectCompensationIntendOutcomeV1,
    RuntimeInteractionEffectCompensationIntendRequestV1,
    RuntimeInteractionEffectMutationDispositionV1, RuntimeInteractionEffectOriginV1,
    RuntimeInteractionEffectOutputIdentityV1, RuntimeInteractionEffectPlanBindOutcomeV1,
    RuntimeInteractionEffectRecoveryBlockedV1, RuntimeInteractionEffectRecoveryCandidateV1,
    RuntimeInteractionEffectRecoveryClaimOutcomeV1, RuntimeInteractionEffectRecoveryClaimRequestV1,
    RuntimeInteractionEffectRecoveryClaimV1, RuntimeInteractionEffectRecoveryScanKeyV1,
    RuntimeInteractionEffectSuccessBindingV1,
};
use crate::receipt::{
    validate_database_time, RuntimeInteractionReceiptRequestKindV1,
    RuntimeInteractionReceiptRouteV1,
};
use crate::receipt_row::{
    bytes_to_lower_hex, decode_binding, decode_guild_id, decode_receipt_identity,
    decode_ruleset_version, decode_scope, positive_u64,
};
use crate::RuntimeInteractionPersistenceErrorV1;

#[derive(sqlx::FromRow)]
pub(crate) struct EffectPlanBindRowV1 {
    pub(crate) outcome_name: String,
    pub(crate) resulting_action_count: i16,
    pub(crate) resulting_certificate_issued_at: DateTime<Utc>,
    pub(crate) resulting_certificate_expires_at: DateTime<Utc>,
    pub(crate) observed_database_now: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct EffectCheckpointRowV1 {
    pub(crate) outcome_name: String,
    pub(crate) effect_state: String,
    pub(crate) resulting_effect_head_revision: i64,
    pub(crate) resulting_recovery_at: Option<DateTime<Utc>>,
    pub(crate) observed_database_now: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct EffectRecoveryClaimRowV1 {
    pub(crate) outcome_name: String,
    pub(crate) effect_state: String,
    pub(crate) resulting_effect_head_revision: i64,
    pub(crate) resulting_recovery_claim_revision: i64,
    pub(crate) resulting_recovery_claim_expires_at: DateTime<Utc>,
    pub(crate) observed_database_now: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct EffectCompensationIntendRowV1 {
    pub(crate) outcome_name: String,
    pub(crate) effect_state: String,
    pub(crate) resulting_effect_head_revision: i64,
    pub(crate) resulting_recovery_claim_revision: i64,
    pub(crate) resulting_recovery_at: DateTime<Utc>,
    pub(crate) observed_database_now: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct EffectOriginRowV1 {
    pub(crate) tenant_id: String,
    pub(crate) installation_id: String,
    pub(crate) deployment_id: String,
    pub(crate) attestation_id: String,
    pub(crate) attestation_digest: String,
    pub(crate) guild_id: String,
    pub(crate) channel_id: String,
    pub(crate) actor_user_id: String,
    pub(crate) interaction_kind: String,
    pub(crate) ruleset_key: String,
    pub(crate) target_version: i64,
    pub(crate) target_content_hash: String,
    pub(crate) binding_revision: i64,
    pub(crate) binding_fingerprint: String,
    pub(crate) runtime_generation: i64,
    pub(crate) route_controller_fencing_token: i64,
    pub(crate) route_incarnation: i64,
    pub(crate) origin_process_instance_id: String,
    pub(crate) origin_serving_lease_epoch: i64,
    pub(crate) origin_serving_revision: i64,
    pub(crate) origin_gateway_shard_id: String,
    pub(crate) origin_gateway_owner_lease_epoch: i64,
    pub(crate) origin_gateway_owner_revision: i64,
    pub(crate) runtime_build_revision: String,
    pub(crate) route_kind: String,
    pub(crate) route_key: String,
    pub(crate) instance_id: Option<String>,
    pub(crate) execution_ruleset_version: i64,
    pub(crate) execution_ruleset_content_hash: String,
    pub(crate) instance_manifest_digest: Option<String>,
    pub(crate) request_digest: Vec<u8>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct EffectRecoveryScanRowV1 {
    pub(crate) application_id: String,
    pub(crate) interaction_id: String,
    pub(crate) action_index: i16,
    pub(crate) action_kind: String,
    pub(crate) effect_state: String,
    pub(crate) effect_head_revision: i64,
    pub(crate) recovery_claim_revision: i64,
    pub(crate) attempt_count: i32,
    pub(crate) observation_attempt_count: i32,
    pub(crate) compensation_attempt_count: i32,
    pub(crate) compensation_observation_attempt_count: i32,
    pub(crate) dependency_indices: Vec<i16>,
    pub(crate) planned_identity_digest: Vec<u8>,
    pub(crate) input_digest: Vec<u8>,
    pub(crate) expected_postimage_digest: Vec<u8>,
    pub(crate) planned_recovery_input: Value,
    pub(crate) planned_preimage_digest: Vec<u8>,
    pub(crate) planned_preimage: Value,
    pub(crate) resolved_input: Value,
    pub(crate) resolved_preimage_digest: Vec<u8>,
    pub(crate) resolved_preimage: Value,
    pub(crate) resolved_effect_identity_digest: Vec<u8>,
    pub(crate) resolved_instance_manifest_digest: Option<Vec<u8>>,
    pub(crate) output_kind: String,
    pub(crate) output_id: Option<String>,
    pub(crate) correlation_class: String,
    pub(crate) correlation_digest: Vec<u8>,
    pub(crate) correlation_marker: Option<String>,
    pub(crate) intent_digest: Option<Vec<u8>>,
    pub(crate) result_digest: Option<Vec<u8>>,
    pub(crate) success_binding_kind: Option<String>,
    pub(crate) success_binding_digest: Option<Vec<u8>>,
    pub(crate) compensation_intent_digest: Option<Vec<u8>>,
    pub(crate) compensation_result_digest: Option<Vec<u8>>,
    pub(crate) next_recovery_at: DateTime<Utc>,
    pub(crate) action_plan_digest: Vec<u8>,
    pub(crate) preflight_certificate_digest: Vec<u8>,
    pub(crate) snapshot_digest: Vec<u8>,
    pub(crate) certificate_issued_at: DateTime<Utc>,
    pub(crate) certificate_expires_at: DateTime<Utc>,
    #[sqlx(flatten)]
    pub(crate) origin: EffectOriginRowV1,
    pub(crate) through_recovery_at: DateTime<Utc>,
    pub(crate) through_application_id: String,
    pub(crate) through_interaction_id: String,
    pub(crate) through_action_index: i16,
    pub(crate) observed_database_now: DateTime<Utc>,
}

impl EffectPlanBindRowV1 {
    pub(crate) fn decode(
        self,
        requested_count: usize,
    ) -> Result<RuntimeInteractionEffectPlanBindOutcomeV1, RuntimeInteractionPersistenceErrorV1>
    {
        validate_database_time(self.resulting_certificate_issued_at, false)?;
        validate_database_time(self.resulting_certificate_expires_at, false)?;
        validate_database_time(self.observed_database_now, false)?;
        let action_count = usize::try_from(self.resulting_action_count)
            .ok()
            .filter(|count| *count == requested_count)
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        if self.resulting_certificate_issued_at >= self.resulting_certificate_expires_at
            || self.resulting_certificate_issued_at > self.observed_database_now
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        Ok(RuntimeInteractionEffectPlanBindOutcomeV1::new(
            decode_disposition(&self.outcome_name, "plan_bound")?,
            action_count,
            self.resulting_certificate_issued_at,
            self.resulting_certificate_expires_at,
            self.observed_database_now,
        ))
    }
}

impl EffectCheckpointRowV1 {
    pub(crate) fn decode(
        self,
        applied_outcome: &str,
    ) -> Result<RuntimeInteractionEffectCheckpointV1, RuntimeInteractionPersistenceErrorV1> {
        validate_database_time(self.observed_database_now, false)?;
        if let Some(recovery_at) = self.resulting_recovery_at {
            validate_database_time(recovery_at, false)?;
        }
        Ok(RuntimeInteractionEffectCheckpointV1::new(
            decode_disposition(&self.outcome_name, applied_outcome)?,
            decode_effect_state_v1(&self.effect_state)?,
            positive_u64(self.resulting_effect_head_revision)?,
            self.resulting_recovery_at,
            self.observed_database_now,
        ))
    }
}

impl EffectRecoveryScanRowV1 {
    pub(crate) fn decode(
        self,
    ) -> Result<
        (
            RuntimeInteractionEffectRecoveryCandidateV1,
            RuntimeInteractionEffectRecoveryScanKeyV1,
            DateTime<Utc>,
        ),
        RuntimeInteractionPersistenceErrorV1,
    > {
        validate_database_time(self.next_recovery_at, false)?;
        validate_database_time(self.through_recovery_at, false)?;
        validate_database_time(self.observed_database_now, false)?;
        validate_database_time(self.certificate_issued_at, false)?;
        validate_database_time(self.certificate_expires_at, false)?;
        if self.certificate_issued_at >= self.certificate_expires_at {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let identity =
            decode_receipt_identity(self.application_id.clone(), self.interaction_id.clone())?;
        let origin = decode_origin(&self.origin, identity)?;
        let action_index = decode_action_index(self.action_index)?;
        let state = decode_effect_state_v1(&self.effect_state)?;
        let kind = decode_effect_kind(&self.action_kind)?;
        validate_recoverable_state(state)?;
        let key = RuntimeInteractionEffectRecoveryScanKeyV1::new(
            self.next_recovery_at,
            identity,
            action_index,
        )?;
        let through = RuntimeInteractionEffectRecoveryScanKeyV1::new(
            self.through_recovery_at,
            decode_receipt_identity(self.through_application_id, self.through_interaction_id)?,
            decode_action_index(self.through_action_index)?,
        )?;
        if key.cmp_c(&through).is_gt() {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let dependency_indices = decode_dependencies(&self.dependency_indices, action_index)?;
        let planned_identity_digest = parse_digest(
            self.planned_identity_digest,
            InteractionEffectPlannedIdentityDigestV1::parse,
        )?;
        let input_digest = parse_digest(self.input_digest, InteractionEffectInputDigestV1::parse)?;
        let expected_postimage_digest = parse_digest(
            self.expected_postimage_digest,
            InteractionEffectExpectedPostimageDigestV1::parse,
        )?;
        let planned_preimage_digest = parse_digest(
            self.planned_preimage_digest,
            InteractionEffectPlannedPreimageDigestV1::parse,
        )?;
        let resolved_preimage_digest = parse_digest(
            self.resolved_preimage_digest,
            InteractionEffectPreimageDigestV1::parse,
        )?;
        let resolved_effect_identity_digest = parse_digest(
            self.resolved_effect_identity_digest,
            InteractionEffectIdentityDigestV1::parse,
        )?;
        let resolved_instance_manifest_digest = self
            .resolved_instance_manifest_digest
            .map(|value| parse_digest(value, InteractionInstanceManifestDigestV1::parse))
            .transpose()?;
        validate_instance_manifest_shape(kind, resolved_instance_manifest_digest.as_ref())?;
        let output_class = decode_output_class(&self.output_kind)?;
        let output_identity = decode_output_identity(output_class, self.output_id)?;
        validate_output_shape(state, output_class, output_identity.as_ref())?;
        let correlation_class = decode_correlation_class(&self.correlation_class)?;
        let correlation_digest = parse_digest(
            self.correlation_digest,
            InteractionEffectCorrelationDigestV1::parse,
        )?;
        validate_correlation_marker(
            kind,
            correlation_class,
            &correlation_digest,
            self.correlation_marker.as_deref(),
        )?;
        let intent_digest =
            parse_optional_digest(self.intent_digest, InteractionEffectIntentDigestV1::parse)?;
        let result_digest =
            parse_optional_digest(self.result_digest, InteractionEffectResultDigestV1::parse)?;
        let success_binding =
            decode_success_binding(self.success_binding_kind, self.success_binding_digest)?;
        let compensation_intent_digest = parse_optional_digest(
            self.compensation_intent_digest,
            InteractionEffectCompensationIntentDigestV1::parse,
        )?;
        let compensation_result_digest = parse_optional_digest(
            self.compensation_result_digest,
            InteractionEffectCompensationResultDigestV1::parse,
        )?;
        validate_effect_digest_shape(
            state,
            intent_digest.as_ref(),
            result_digest.as_ref(),
            success_binding.as_ref(),
            compensation_intent_digest.as_ref(),
            compensation_result_digest.as_ref(),
        )?;
        let candidate = RuntimeInteractionEffectRecoveryCandidateV1 {
            key,
            kind,
            state,
            effect_head_revision: positive_u64(self.effect_head_revision)?,
            recovery_claim_revision: nonnegative_u64(self.recovery_claim_revision)?,
            attempt_count: decode_attempt(self.attempt_count)?,
            observation_attempt_count: decode_attempt(self.observation_attempt_count)?,
            compensation_attempt_count: decode_attempt(self.compensation_attempt_count)?,
            compensation_observation_attempt_count: decode_attempt(
                self.compensation_observation_attempt_count,
            )?,
            dependency_indices,
            planned_identity_digest,
            input_digest,
            expected_postimage_digest,
            planned_recovery_input: validate_document(self.planned_recovery_input)?,
            planned_preimage_digest,
            planned_preimage: validate_document(self.planned_preimage)?,
            resolved_input: validate_document(self.resolved_input)?,
            resolved_preimage_digest,
            resolved_preimage: validate_document(self.resolved_preimage)?,
            resolved_effect_identity_digest,
            resolved_instance_manifest_digest,
            output_class,
            output_identity,
            correlation_class,
            correlation_digest,
            correlation_marker: self.correlation_marker,
            intent_digest,
            result_digest,
            success_binding,
            compensation_intent_digest,
            compensation_result_digest,
            action_plan_digest: parse_digest(
                self.action_plan_digest,
                InteractionActionPlanDigestV1::parse,
            )?,
            preflight_certificate_digest: parse_digest(
                self.preflight_certificate_digest,
                InteractionPreflightCertificateDigestV1::parse,
            )?,
            snapshot_digest: parse_digest(
                self.snapshot_digest,
                InteractionPreflightSnapshotDigestV1::parse,
            )?,
            certificate_issued_at: self.certificate_issued_at,
            certificate_expires_at: self.certificate_expires_at,
            origin,
        };
        candidate.strict_recovery_binding_v1()?;
        Ok((candidate, through, self.observed_database_now))
    }
}

impl EffectRecoveryClaimRowV1 {
    pub(crate) fn decode(
        self,
        request: RuntimeInteractionEffectRecoveryClaimRequestV1,
    ) -> Result<RuntimeInteractionEffectRecoveryClaimOutcomeV1, RuntimeInteractionPersistenceErrorV1>
    {
        validate_database_time(self.resulting_recovery_claim_expires_at, false)?;
        validate_database_time(self.observed_database_now, false)?;
        if self.effect_state == "recovery_required"
            && matches!(
                self.outcome_name.as_str(),
                "recovery_blocked_attempt_budget_exhausted" | "exact_replay"
            )
        {
            let candidate = request.candidate();
            let exhausted = if matches!(
                candidate.state(),
                automation_runtime_interaction::InteractionEffectStateV1::CompensationIntended
                    | automation_runtime_interaction::InteractionEffectStateV1::CompensationIndeterminate
                    | automation_runtime_interaction::InteractionEffectStateV1::CompensationObserving
                    | automation_runtime_interaction::InteractionEffectStateV1::CompensationObservationPending
            ) {
                candidate.compensation_observation_attempt_count() >= 64
            } else {
                candidate.observation_attempt_count() >= 64
            };
            let disposition = decode_disposition(
                &self.outcome_name,
                "recovery_blocked_attempt_budget_exhausted",
            )?;
            let head_revision = positive_u64(self.resulting_effect_head_revision)?;
            let claim_revision = nonnegative_u64(self.resulting_recovery_claim_revision)?;
            if !exhausted
                || head_revision != request.candidate().effect_head_revision() + 1
                || claim_revision != request.candidate().recovery_claim_revision()
                || self.resulting_recovery_claim_expires_at != self.observed_database_now
            {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            return Ok(
                RuntimeInteractionEffectRecoveryClaimOutcomeV1::RecoveryBlocked(
                    RuntimeInteractionEffectRecoveryBlockedV1::new(
                        disposition,
                        head_revision,
                        claim_revision,
                        self.observed_database_now,
                    ),
                ),
            );
        }
        let expected_state = if matches!(
            request.candidate().state(),
            automation_runtime_interaction::InteractionEffectStateV1::CompensationIntended
                | automation_runtime_interaction::InteractionEffectStateV1::CompensationIndeterminate
                | automation_runtime_interaction::InteractionEffectStateV1::CompensationObserving
                | automation_runtime_interaction::InteractionEffectStateV1::CompensationObservationPending
        ) {
            automation_runtime_interaction::InteractionEffectStateV1::CompensationObserving
        } else {
            automation_runtime_interaction::InteractionEffectStateV1::Observing
        };
        let applied_outcome = if expected_state
            == automation_runtime_interaction::InteractionEffectStateV1::CompensationObserving
        {
            "compensation_observation_claimed"
        } else {
            "recovery_claimed"
        };
        let disposition = decode_disposition(&self.outcome_name, applied_outcome)?;
        let state = decode_effect_state_v1(&self.effect_state)?;
        let head_revision = positive_u64(self.resulting_effect_head_revision)?;
        let claim_revision = positive_u64(self.resulting_recovery_claim_revision)?;
        if state != expected_state
            || head_revision != request.candidate().effect_head_revision() + 1
            || claim_revision != request.candidate().recovery_claim_revision() + 1
            || self.resulting_recovery_claim_expires_at <= self.observed_database_now
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        Ok(RuntimeInteractionEffectRecoveryClaimOutcomeV1::Claimed(
            Box::new(RuntimeInteractionEffectRecoveryClaimV1::new(
                request,
                disposition,
                state,
                head_revision,
                claim_revision,
                self.resulting_recovery_claim_expires_at,
                self.observed_database_now,
            )),
        ))
    }
}

impl EffectCompensationIntendRowV1 {
    pub(crate) fn decode(
        self,
        request: RuntimeInteractionEffectCompensationIntendRequestV1,
    ) -> Result<
        RuntimeInteractionEffectCompensationIntendOutcomeV1,
        RuntimeInteractionPersistenceErrorV1,
    > {
        validate_database_time(self.resulting_recovery_at, false)?;
        validate_database_time(self.observed_database_now, false)?;
        if self.effect_state == "recovery_required"
            && matches!(
                self.outcome_name.as_str(),
                "recovery_blocked_attempt_budget_exhausted" | "exact_replay"
            )
        {
            if request.candidate().compensation_attempt_count() < 64 {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            let disposition = decode_disposition(
                &self.outcome_name,
                "recovery_blocked_attempt_budget_exhausted",
            )?;
            let head_revision = positive_u64(self.resulting_effect_head_revision)?;
            let claim_revision = nonnegative_u64(self.resulting_recovery_claim_revision)?;
            if head_revision != request.candidate().effect_head_revision() + 1
                || claim_revision != request.candidate().recovery_claim_revision()
                || self.resulting_recovery_at != self.observed_database_now
            {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            return Ok(
                RuntimeInteractionEffectCompensationIntendOutcomeV1::RecoveryBlocked(
                    RuntimeInteractionEffectRecoveryBlockedV1::new(
                        disposition,
                        head_revision,
                        claim_revision,
                        self.observed_database_now,
                    ),
                ),
            );
        }
        let disposition = decode_disposition(&self.outcome_name, "compensation_intended")?;
        let state = decode_effect_state_v1(&self.effect_state)?;
        let head_revision = positive_u64(self.resulting_effect_head_revision)?;
        let claim_revision = positive_u64(self.resulting_recovery_claim_revision)?;
        if state != automation_runtime_interaction::InteractionEffectStateV1::CompensationIntended
            || head_revision != request.candidate().effect_head_revision() + 1
            || claim_revision != request.candidate().recovery_claim_revision() + 1
            || self.resulting_recovery_at <= self.observed_database_now
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        Ok(
            RuntimeInteractionEffectCompensationIntendOutcomeV1::Claimed(Box::new(
                RuntimeInteractionEffectCompensationClaimV1::new(
                    request,
                    disposition,
                    head_revision,
                    claim_revision,
                    self.resulting_recovery_at,
                    self.observed_database_now,
                ),
            )),
        )
    }
}

pub(crate) fn decode_origin(
    row: &EffectOriginRowV1,
    identity: automation_runtime_interaction::InteractionReceiptIdentityV1,
) -> Result<RuntimeInteractionEffectOriginV1, RuntimeInteractionPersistenceErrorV1> {
    validate_lower_hex_64(&row.attestation_id)?;
    let scope = decode_scope(
        row.tenant_id.clone(),
        row.installation_id.clone(),
        row.deployment_id.clone(),
    )?;
    let target = RuntimeDeploymentTargetV1 {
        guild_id: decode_guild_id(&row.guild_id)?,
        ruleset_key: RuleSetKey::parse(&row.ruleset_key)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        version: decode_ruleset_version(row.target_version)?,
        content_hash: RuleSetContentHash::parse_hex(&row.target_content_hash)
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        binding_revision: BindingRevision::new(positive_u64(row.binding_revision)?)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        binding_fingerprint: ResourceBindingFingerprint::parse(&row.binding_fingerprint)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
    };
    let process = RuntimeProcessIdentityV1 {
        target,
        runtime_generation: RuntimeGeneration::new(positive_u64(row.runtime_generation)?)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        process_instance_id: automation_runtime_convergence::ProcessInstanceId::parse(
            &row.origin_process_instance_id,
        )
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
    };
    let route = match row.route_kind.as_str() {
        "static" if row.instance_id.is_none() && row.instance_manifest_digest.is_none() => {
            RuntimeInteractionReceiptRouteV1::static_route(row.route_key.clone())?
        }
        "instance" => RuntimeInteractionReceiptRouteV1::instance_route(
            row.route_key.clone(),
            InstanceId::parse(
                row.instance_id
                    .as_ref()
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
            )
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        )?,
        _ => return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    };
    let binding = decode_binding(
        scope,
        process,
        InteractionGatewayShardIdentityV1::parse(&row.origin_gateway_shard_id)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
        row.attestation_digest.clone(),
        row.origin_serving_lease_epoch,
        row.origin_serving_revision,
        row.origin_gateway_owner_lease_epoch,
        row.origin_gateway_owner_revision,
        row.route_controller_fencing_token,
        row.route_incarnation,
        row.runtime_build_revision.clone(),
        &route,
        row.execution_ruleset_version,
        row.execution_ruleset_content_hash.clone(),
        row.instance_manifest_digest.clone(),
    )?;
    let request_digest = InteractionRequestDigestV1::parse(bytes_to_lower_hex(&row.request_digest))
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    let root = InteractionReceiptClaimCandidateV1::new(
        identity,
        automation_runtime_interaction::InteractionExpectedRouteV1::from_authoritative(&binding),
        request_digest,
    )
    .bind_authoritative(binding)
    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    Ok(RuntimeInteractionEffectOriginV1::new(
        root,
        route,
        row.attestation_id.clone(),
        ChannelId(decode_discord_id(&row.channel_id)?),
        UserId(decode_discord_id(&row.actor_user_id)?),
        decode_request_kind(&row.interaction_kind)?,
    ))
}

fn decode_disposition(
    value: &str,
    applied: &str,
) -> Result<RuntimeInteractionEffectMutationDispositionV1, RuntimeInteractionPersistenceErrorV1> {
    match value {
        value if value == applied => Ok(RuntimeInteractionEffectMutationDispositionV1::Applied),
        "exact_replay" => Ok(RuntimeInteractionEffectMutationDispositionV1::ExactReplay),
        _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    }
}

pub(crate) fn decode_action_index(
    value: i16,
) -> Result<InteractionEffectActionIndexV1, RuntimeInteractionPersistenceErrorV1> {
    u16::try_from(value)
        .ok()
        .and_then(|value| InteractionEffectActionIndexV1::new(value).ok())
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn decode_dependencies(
    values: &[i16],
    consumer: InteractionEffectActionIndexV1,
) -> Result<Vec<InteractionEffectActionIndexV1>, RuntimeInteractionPersistenceErrorV1> {
    let mut decoded = values
        .iter()
        .map(|value| decode_action_index(*value))
        .collect::<Result<Vec<_>, _>>()?;
    if decoded.len() > 32
        || decoded.iter().any(|value| *value >= consumer)
        || decoded.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    decoded.shrink_to_fit();
    Ok(decoded)
}

fn decode_effect_kind(
    value: &str,
) -> Result<InteractionEffectKindV1, RuntimeInteractionPersistenceErrorV1> {
    match value {
        "create_role" => Ok(InteractionEffectKindV1::CreateRole),
        "create_channel" => Ok(InteractionEffectKindV1::CreateChannel),
        "grant_role" => Ok(InteractionEffectKindV1::GrantRole),
        "upsert_overwrite" => Ok(InteractionEffectKindV1::UpsertOverwrite),
        "post_panel" => Ok(InteractionEffectKindV1::PostPanel),
        "register_instance" => Ok(InteractionEffectKindV1::RegisterInstance),
        "teardown_instance" => Ok(InteractionEffectKindV1::TeardownInstance),
        "edit_response" => Ok(InteractionEffectKindV1::EditResponse),
        _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    }
}

fn decode_output_class(
    value: &str,
) -> Result<InteractionEffectOutputClassV1, RuntimeInteractionPersistenceErrorV1> {
    match value {
        "created_role" => Ok(InteractionEffectOutputClassV1::CreatedRole),
        "created_channel" => Ok(InteractionEffectOutputClassV1::CreatedChannel),
        "role_membership" => Ok(InteractionEffectOutputClassV1::RoleMembership),
        "permission_overwrite" => Ok(InteractionEffectOutputClassV1::PermissionOverwrite),
        "posted_message" => Ok(InteractionEffectOutputClassV1::PostedMessage),
        "instance_state" => Ok(InteractionEffectOutputClassV1::InstanceState),
        "original_response" => Ok(InteractionEffectOutputClassV1::OriginalResponse),
        _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    }
}

fn decode_correlation_class(
    value: &str,
) -> Result<InteractionEffectCorrelationClassV1, RuntimeInteractionPersistenceErrorV1> {
    match value {
        "audit_log_reason" => Ok(InteractionEffectCorrelationClassV1::AuditLogReason),
        "message_nonce" => Ok(InteractionEffectCorrelationClassV1::MessageNonce),
        "internal_idempotency_key" => {
            Ok(InteractionEffectCorrelationClassV1::InternalIdempotencyKey)
        }
        "interaction_receipt" => Ok(InteractionEffectCorrelationClassV1::InteractionReceipt),
        "unsupported" => Ok(InteractionEffectCorrelationClassV1::Unsupported),
        _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    }
}

fn decode_output_identity(
    class: InteractionEffectOutputClassV1,
    value: Option<String>,
) -> Result<Option<RuntimeInteractionEffectOutputIdentityV1>, RuntimeInteractionPersistenceErrorV1>
{
    match (class, value) {
        (
            InteractionEffectOutputClassV1::CreatedRole
            | InteractionEffectOutputClassV1::CreatedChannel
            | InteractionEffectOutputClassV1::PostedMessage,
            Some(value),
        ) => Ok(Some(RuntimeInteractionEffectOutputIdentityV1::discord(
            decode_discord_id(&value)?,
        )?)),
        (InteractionEffectOutputClassV1::InstanceState, Some(value)) => {
            Ok(Some(RuntimeInteractionEffectOutputIdentityV1::instance(
                InstanceId::parse(&value)
                    .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?,
            )))
        }
        (
            InteractionEffectOutputClassV1::RoleMembership
            | InteractionEffectOutputClassV1::PermissionOverwrite
            | InteractionEffectOutputClassV1::OriginalResponse,
            None,
        ) => Ok(None),
        (_, None) => Ok(None),
        _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    }
}

fn decode_success_binding(
    kind: Option<String>,
    digest: Option<Vec<u8>>,
) -> Result<Option<RuntimeInteractionEffectSuccessBindingV1>, RuntimeInteractionPersistenceErrorV1>
{
    match (kind.as_deref(), digest) {
        (None, None) => Ok(None),
        (Some("attempt_result"), Some(digest)) => Ok(Some(
            RuntimeInteractionEffectSuccessBindingV1::AttemptResult(parse_digest(
                digest,
                InteractionEffectResultDigestV1::parse,
            )?),
        )),
        (Some("observation"), Some(digest)) => {
            Ok(Some(RuntimeInteractionEffectSuccessBindingV1::Observation(
                parse_digest(digest, InteractionEffectObservationDigestV1::parse)?,
            )))
        }
        _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    }
}

fn validate_correlation_marker(
    kind: InteractionEffectKindV1,
    class: InteractionEffectCorrelationClassV1,
    digest: &InteractionEffectCorrelationDigestV1,
    marker: Option<&str>,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    let valid = match class {
        InteractionEffectCorrelationClassV1::AuditLogReason
        | InteractionEffectCorrelationClassV1::InternalIdempotencyKey => {
            marker == Some(digest.as_str())
        }
        InteractionEffectCorrelationClassV1::MessageNonce => marker
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|value| value > 0 && value.to_string() == marker.unwrap_or_default()),
        InteractionEffectCorrelationClassV1::InteractionReceipt
        | InteractionEffectCorrelationClassV1::Unsupported => marker.is_none(),
    };
    let kind_valid = matches!(
        (kind, class),
        (
            InteractionEffectKindV1::CreateRole
                | InteractionEffectKindV1::CreateChannel
                | InteractionEffectKindV1::GrantRole
                | InteractionEffectKindV1::UpsertOverwrite,
            InteractionEffectCorrelationClassV1::AuditLogReason
        ) | (
            InteractionEffectKindV1::PostPanel,
            InteractionEffectCorrelationClassV1::MessageNonce
                | InteractionEffectCorrelationClassV1::Unsupported
        ) | (
            InteractionEffectKindV1::RegisterInstance | InteractionEffectKindV1::TeardownInstance,
            InteractionEffectCorrelationClassV1::InternalIdempotencyKey
        ) | (
            InteractionEffectKindV1::EditResponse,
            InteractionEffectCorrelationClassV1::InteractionReceipt
        )
    );
    if valid && kind_valid {
        Ok(())
    } else {
        Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
    }
}

fn validate_instance_manifest_shape(
    kind: InteractionEffectKindV1,
    digest: Option<&InteractionInstanceManifestDigestV1>,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    if matches!(kind, InteractionEffectKindV1::RegisterInstance) != digest.is_some() {
        Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
    } else {
        Ok(())
    }
}

fn validate_effect_digest_shape(
    state: automation_runtime_interaction::InteractionEffectStateV1,
    intent: Option<&InteractionEffectIntentDigestV1>,
    result: Option<&InteractionEffectResultDigestV1>,
    success: Option<&RuntimeInteractionEffectSuccessBindingV1>,
    compensation_intent: Option<&InteractionEffectCompensationIntentDigestV1>,
    compensation_result: Option<&InteractionEffectCompensationResultDigestV1>,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    let requires_success = matches!(
        state,
        automation_runtime_interaction::InteractionEffectStateV1::KnownSucceeded
            | automation_runtime_interaction::InteractionEffectStateV1::ReconciledSucceeded
            | automation_runtime_interaction::InteractionEffectStateV1::CompensationIntended
            | automation_runtime_interaction::InteractionEffectStateV1::CompensationIndeterminate
            | automation_runtime_interaction::InteractionEffectStateV1::CompensationObserving
            | automation_runtime_interaction::InteractionEffectStateV1::CompensationObservationPending
    );
    let requires_compensation = matches!(
        state,
        automation_runtime_interaction::InteractionEffectStateV1::CompensationIntended
            | automation_runtime_interaction::InteractionEffectStateV1::CompensationIndeterminate
            | automation_runtime_interaction::InteractionEffectStateV1::CompensationObserving
            | automation_runtime_interaction::InteractionEffectStateV1::CompensationObservationPending
    );
    let success_matches = match success {
        Some(RuntimeInteractionEffectSuccessBindingV1::AttemptResult(binding)) => {
            result == Some(binding)
        }
        Some(RuntimeInteractionEffectSuccessBindingV1::Observation(_)) | None => true,
    };
    if intent.is_none()
        || matches!(
            state,
            automation_runtime_interaction::InteractionEffectStateV1::Indeterminate
                | automation_runtime_interaction::InteractionEffectStateV1::KnownSucceeded
        ) && result.is_none()
        || requires_success != success.is_some()
        || requires_compensation != compensation_intent.is_some()
        || !success_matches
        || state == automation_runtime_interaction::InteractionEffectStateV1::CompensationIntended
            && compensation_result.is_some()
        || state
            == automation_runtime_interaction::InteractionEffectStateV1::CompensationIndeterminate
            && compensation_result.is_none()
    {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn validate_output_shape(
    state: automation_runtime_interaction::InteractionEffectStateV1,
    class: InteractionEffectOutputClassV1,
    output: Option<&RuntimeInteractionEffectOutputIdentityV1>,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    let successful = matches!(
        state,
        automation_runtime_interaction::InteractionEffectStateV1::KnownSucceeded
            | automation_runtime_interaction::InteractionEffectStateV1::ReconciledSucceeded
            | automation_runtime_interaction::InteractionEffectStateV1::CompensationIntended
            | automation_runtime_interaction::InteractionEffectStateV1::CompensationIndeterminate
            | automation_runtime_interaction::InteractionEffectStateV1::CompensationObserving
            | automation_runtime_interaction::InteractionEffectStateV1::CompensationObservationPending
    );
    let identity_required = matches!(
        class,
        InteractionEffectOutputClassV1::CreatedRole
            | InteractionEffectOutputClassV1::CreatedChannel
            | InteractionEffectOutputClassV1::PostedMessage
            | InteractionEffectOutputClassV1::InstanceState
    );
    if output.is_some() != (successful && identity_required) {
        Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
    } else {
        Ok(())
    }
}

fn validate_recoverable_state(
    state: automation_runtime_interaction::InteractionEffectStateV1,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    if matches!(
        state,
        automation_runtime_interaction::InteractionEffectStateV1::Intended
            | automation_runtime_interaction::InteractionEffectStateV1::Indeterminate
            | automation_runtime_interaction::InteractionEffectStateV1::Observing
            | automation_runtime_interaction::InteractionEffectStateV1::ObservationPending
            | automation_runtime_interaction::InteractionEffectStateV1::KnownSucceeded
            | automation_runtime_interaction::InteractionEffectStateV1::ReconciledSucceeded
            | automation_runtime_interaction::InteractionEffectStateV1::CompensationIntended
            | automation_runtime_interaction::InteractionEffectStateV1::CompensationIndeterminate
            | automation_runtime_interaction::InteractionEffectStateV1::CompensationObserving
            | automation_runtime_interaction::InteractionEffectStateV1::CompensationObservationPending
    ) {
        Ok(())
    } else {
        Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
    }
}

fn validate_document(value: Value) -> Result<Value, RuntimeInteractionPersistenceErrorV1> {
    let encoded = serde_json::to_vec(&value)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    if !value.is_object() || !(2..=4096).contains(&encoded.len()) {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(value)
}

fn validate_lower_hex_64(value: &str) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
    }
}

pub(crate) fn parse_digest<T, E>(
    value: Vec<u8>,
    parse: impl FnOnce(String) -> Result<T, E>,
) -> Result<T, RuntimeInteractionPersistenceErrorV1> {
    if value.len() != 32 {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    parse(bytes_to_lower_hex(&value))
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn parse_optional_digest<T, E>(
    value: Option<Vec<u8>>,
    parse: impl FnOnce(String) -> Result<T, E>,
) -> Result<Option<T>, RuntimeInteractionPersistenceErrorV1> {
    value.map(|value| parse_digest(value, parse)).transpose()
}

fn decode_attempt(value: i32) -> Result<u16, RuntimeInteractionPersistenceErrorV1> {
    u16::try_from(value)
        .ok()
        .filter(|value| {
            value.le(&automation_runtime_interaction::MAX_INTERACTION_EFFECT_ATTEMPTS_V1)
        })
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

pub(crate) fn nonnegative_u64(value: i64) -> Result<u64, RuntimeInteractionPersistenceErrorV1> {
    u64::try_from(value).map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn decode_discord_id(value: &str) -> Result<u64, RuntimeInteractionPersistenceErrorV1> {
    value
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0 && id.to_string() == value)
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn decode_request_kind(
    value: &str,
) -> Result<RuntimeInteractionReceiptRequestKindV1, RuntimeInteractionPersistenceErrorV1> {
    match value {
        "message_component" => Ok(RuntimeInteractionReceiptRequestKindV1::MessageComponent),
        "modal_submit" => Ok(RuntimeInteractionReceiptRequestKindV1::ModalSubmit),
        _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automation_runtime_interaction::InteractionEffectStateV1;

    fn intent() -> InteractionEffectIntentDigestV1 {
        InteractionEffectIntentDigestV1::from_canonical_bytes(b"intent")
    }

    fn result(value: &[u8]) -> InteractionEffectResultDigestV1 {
        InteractionEffectResultDigestV1::from_canonical_bytes(value)
    }

    fn observation() -> InteractionEffectObservationDigestV1 {
        InteractionEffectObservationDigestV1::from_canonical_bytes(b"observation")
    }

    fn compensation_intent() -> InteractionEffectCompensationIntentDigestV1 {
        InteractionEffectCompensationIntentDigestV1::from_canonical_bytes(b"compensation")
    }

    #[test]
    fn observation_bound_compensation_intent_accepts_no_attempt_result() {
        assert_eq!(
            validate_effect_digest_shape(
                InteractionEffectStateV1::CompensationIntended,
                Some(&intent()),
                None,
                Some(&RuntimeInteractionEffectSuccessBindingV1::Observation(
                    observation(),
                )),
                Some(&compensation_intent()),
                None,
            ),
            Ok(())
        );
    }

    #[test]
    fn attempt_result_success_binding_must_match_the_result() {
        assert_eq!(
            validate_effect_digest_shape(
                InteractionEffectStateV1::KnownSucceeded,
                Some(&intent()),
                Some(&result(b"first")),
                Some(&RuntimeInteractionEffectSuccessBindingV1::AttemptResult(
                    result(b"second"),
                )),
                None,
                None,
            ),
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );
    }

    #[test]
    fn successful_created_resource_requires_exact_output_identity() {
        assert_eq!(
            validate_output_shape(
                InteractionEffectStateV1::KnownSucceeded,
                InteractionEffectOutputClassV1::CreatedRole,
                None,
            ),
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );
        let output = RuntimeInteractionEffectOutputIdentityV1::discord(42).unwrap();
        assert_eq!(
            validate_output_shape(
                InteractionEffectStateV1::KnownSucceeded,
                InteractionEffectOutputClassV1::CreatedRole,
                Some(&output),
            ),
            Ok(())
        );
    }

    #[test]
    fn audit_correlation_marker_must_equal_the_bound_digest() {
        let digest = InteractionEffectCorrelationDigestV1::from_canonical_bytes(b"correlation");
        assert_eq!(
            validate_correlation_marker(
                InteractionEffectKindV1::CreateRole,
                InteractionEffectCorrelationClassV1::AuditLogReason,
                &digest,
                Some(digest.as_str()),
            ),
            Ok(())
        );
        assert_eq!(
            validate_correlation_marker(
                InteractionEffectKindV1::CreateRole,
                InteractionEffectCorrelationClassV1::AuditLogReason,
                &digest,
                Some("0000000000000000000000000000000000000000000000000000000000000000"),
            ),
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );
    }
}
