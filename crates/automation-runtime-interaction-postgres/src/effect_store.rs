use std::num::NonZeroUsize;

use chrono::{TimeZone, Utc};
use sqlx::types::Json;

use crate::contract::{
    EFFECT_COMPENSATION_FINISH_QUERY, EFFECT_COMPENSATION_INTEND_QUERY, EFFECT_FINISH_QUERY,
    EFFECT_INTEND_QUERY, EFFECT_PLAN_BIND_QUERY, EFFECT_RECONCILE_QUERY,
    EFFECT_RECOVERY_CLAIM_QUERY, EFFECT_RECOVERY_SCAN_QUERY,
};
use crate::database::{begin_interaction_transaction, verify_runtime_interaction_binding_v1};
use crate::effect::{
    effect_state_code_v1, recovery_path_code_v1, RuntimeInteractionEffectCheckpointV1,
    RuntimeInteractionEffectCompensationFinishRequestV1,
    RuntimeInteractionEffectCompensationIntendOutcomeV1,
    RuntimeInteractionEffectCompensationIntendRequestV1, RuntimeInteractionEffectFinishRequestV1,
    RuntimeInteractionEffectIntendRequestV1, RuntimeInteractionEffectPlanBindOutcomeV1,
    RuntimeInteractionEffectPlanBindRequestV1, RuntimeInteractionEffectReconcileRequestV1,
    RuntimeInteractionEffectRecoveryClaimOutcomeV1, RuntimeInteractionEffectRecoveryClaimRequestV1,
    RuntimeInteractionEffectRecoveryScanCursorV1, RuntimeInteractionEffectRecoveryScanKeyV1,
    RuntimeInteractionEffectRecoveryScanPageV1, MAX_RUNTIME_INTERACTION_EFFECT_RECOVERY_SCAN_BATCH,
};
use crate::effect_row::{
    EffectCheckpointRowV1, EffectCompensationIntendRowV1, EffectPlanBindRowV1,
    EffectRecoveryClaimRowV1, EffectRecoveryScanRowV1,
};
use crate::error::{map_mutation_commit_error, map_mutation_error, map_query_error};
use crate::receipt_row::digest_bytes;
use crate::{PostgresRuntimeInteractionV1, RuntimeInteractionPersistenceErrorV1};

impl PostgresRuntimeInteractionV1 {
    pub async fn bind_interaction_effect_plan_v1(
        &self,
        request: RuntimeInteractionEffectPlanBindRequestV1,
    ) -> Result<RuntimeInteractionEffectPlanBindOutcomeV1, RuntimeInteractionPersistenceErrorV1>
    {
        let identity = request.identity();
        let requested_count = request.actions().len();
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows = sqlx::query_as::<_, EffectPlanBindRowV1>(EFFECT_PLAN_BIND_QUERY)
            .bind(identity.application_id().get().to_string())
            .bind(identity.interaction_id().get().to_string())
            .bind(to_i64(request.receipt_head_revision())?)
            .bind(to_i64(request.receipt_claim_revision())?)
            .bind(request.process_instance_id().as_str())
            .bind(digest_bytes(request.action_plan_digest().as_str())?)
            .bind(digest_bytes(
                request.preflight_certificate_digest().as_str(),
            )?)
            .bind(digest_bytes(request.snapshot_digest().as_str())?)
            .bind(Json(request.action_document().clone()))
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_mutation_error(&error))?;
        let outcome = exactly_one(rows)?.decode(requested_count)?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        Ok(outcome)
    }

    pub async fn intend_interaction_effect_v1(
        &self,
        request: RuntimeInteractionEffectIntendRequestV1,
    ) -> Result<RuntimeInteractionEffectCheckpointV1, RuntimeInteractionPersistenceErrorV1> {
        let identity = request.identity();
        let resolved_manifest = request
            .resolved_instance_manifest_digest()
            .map(|digest| digest_bytes(digest.as_str()))
            .transpose()?
            .unwrap_or_default();
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows = sqlx::query_as::<_, EffectCheckpointRowV1>(EFFECT_INTEND_QUERY)
            .bind(identity.application_id().get().to_string())
            .bind(identity.interaction_id().get().to_string())
            .bind(to_i64(request.receipt_head_revision())?)
            .bind(to_i64(request.receipt_claim_revision())?)
            .bind(request.process_instance_id().as_str())
            .bind(digest_bytes(
                request.preflight_certificate_digest().as_str(),
            )?)
            .bind(i64::from(request.action_index().get()))
            .bind(to_i64(request.effect_head_revision())?)
            .bind(digest_bytes(request.intent_digest().as_str())?)
            .bind(digest_bytes(
                request.resolved_effect_identity_digest().as_str(),
            )?)
            .bind(resolved_manifest)
            .bind(Json(request.resolved_input().clone()))
            .bind(digest_bytes(request.resolved_preimage_digest().as_str())?)
            .bind(Json(request.resolved_preimage().clone()))
            .bind(request.recovery_delay_milliseconds())
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_mutation_error(&error))?;
        let checkpoint = exactly_one(rows)?.decode("intended")?;
        validate_checkpoint(
            &checkpoint,
            automation_runtime_interaction::InteractionEffectStateV1::Intended,
            request.effect_head_revision() + 1,
            true,
        )?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        Ok(checkpoint)
    }

    pub async fn finish_interaction_effect_v1(
        &self,
        request: RuntimeInteractionEffectFinishRequestV1,
    ) -> Result<RuntimeInteractionEffectCheckpointV1, RuntimeInteractionPersistenceErrorV1> {
        let identity = request.identity();
        let outcome_code = request.outcome_code();
        let (expected_state, recovery_expected) = match outcome_code {
            "succeeded" => (
                automation_runtime_interaction::InteractionEffectStateV1::KnownSucceeded,
                false,
            ),
            "definitive_failure" => (
                automation_runtime_interaction::InteractionEffectStateV1::KnownFailed,
                false,
            ),
            "indeterminate" => (
                automation_runtime_interaction::InteractionEffectStateV1::Indeterminate,
                true,
            ),
            _ => return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput),
        };
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows = sqlx::query_as::<_, EffectCheckpointRowV1>(EFFECT_FINISH_QUERY)
            .bind(identity.application_id().get().to_string())
            .bind(identity.interaction_id().get().to_string())
            .bind(to_i64(request.receipt_head_revision())?)
            .bind(to_i64(request.receipt_claim_revision())?)
            .bind(request.process_instance_id().as_str())
            .bind(digest_bytes(
                request.preflight_certificate_digest().as_str(),
            )?)
            .bind(i64::from(request.action_index().get()))
            .bind(to_i64(request.effect_head_revision())?)
            .bind(digest_bytes(request.result_digest().as_str())?)
            .bind(outcome_code)
            .bind(request.output_parameter())
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_mutation_error(&error))?;
        let checkpoint = exactly_one(rows)?.decode(outcome_code)?;
        validate_checkpoint(
            &checkpoint,
            expected_state,
            request.effect_head_revision() + 1,
            recovery_expected,
        )?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        Ok(checkpoint)
    }

    pub async fn scan_recoverable_interaction_effects_v1(
        &self,
        cursor: RuntimeInteractionEffectRecoveryScanCursorV1,
        limit: NonZeroUsize,
    ) -> Result<RuntimeInteractionEffectRecoveryScanPageV1, RuntimeInteractionPersistenceErrorV1>
    {
        if limit.get() > MAX_RUNTIME_INTERACTION_EFFECT_RECOVERY_SCAN_BATCH {
            return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
        }
        let epoch = Utc
            .timestamp_millis_opt(0)
            .single()
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        let after = scan_parameters(cursor.after(), epoch);
        let through = scan_parameters(cursor.through(), epoch);
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows = sqlx::query_as::<_, EffectRecoveryScanRowV1>(EFFECT_RECOVERY_SCAN_QUERY)
            .bind(after.0)
            .bind(after.1)
            .bind(after.2)
            .bind(after.3)
            .bind(through.0)
            .bind(through.1)
            .bind(through.2)
            .bind(through.3)
            .bind(
                i64::try_from(limit.get())
                    .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?,
            )
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_query_error(&error))?;
        let page = decode_scan_page(rows, cursor.after(), limit)?;
        transaction
            .commit()
            .await
            .map_err(|error| map_query_error(&error))?;
        Ok(page)
    }

    pub async fn claim_interaction_effect_recovery_v1(
        &self,
        request: RuntimeInteractionEffectRecoveryClaimRequestV1,
    ) -> Result<RuntimeInteractionEffectRecoveryClaimOutcomeV1, RuntimeInteractionPersistenceErrorV1>
    {
        let identity = request.candidate().key().identity();
        let action_index = request.candidate().key().action_index();
        let expected = request.expected_route();
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows = sqlx::query_as::<_, EffectRecoveryClaimRowV1>(EFFECT_RECOVERY_CLAIM_QUERY)
            .bind(identity.application_id().get().to_string())
            .bind(identity.interaction_id().get().to_string())
            .bind(i64::from(action_index.get()))
            .bind(to_i64(request.candidate().effect_head_revision())?)
            .bind(expected.process_identity().process_instance_id.as_str())
            .bind(expected.gateway_shard_identity().as_str())
            .bind(expected.runtime_build_revision().as_str())
            .bind(to_i64(
                expected.process_identity().runtime_generation.get(),
            )?)
            .bind(to_i64(expected.route_fencing_token().get())?)
            .bind(to_i64(expected.route_incarnation().get())?)
            .bind(request.claim_lease().milliseconds())
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_mutation_error(&error))?;
        let claim = exactly_one(rows)?.decode(request)?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        Ok(claim)
    }

    pub async fn reconcile_interaction_effect_v1(
        &self,
        request: RuntimeInteractionEffectReconcileRequestV1,
    ) -> Result<RuntimeInteractionEffectCheckpointV1, RuntimeInteractionPersistenceErrorV1> {
        let outcome_code = request.outcome_code();
        let (expected_state, recovery_expected) = reconcile_expected_checkpoint(outcome_code)?;
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows = sqlx::query_as::<_, EffectCheckpointRowV1>(EFFECT_RECONCILE_QUERY)
            .bind(request.identity.application_id().get().to_string())
            .bind(request.identity.interaction_id().get().to_string())
            .bind(i64::from(request.action_index.get()))
            .bind(to_i64(request.effect_head_revision)?)
            .bind(to_i64(request.recovery_claim_revision)?)
            .bind(request.process_instance_id.as_str())
            .bind(request.expected_route.gateway_shard_identity().as_str())
            .bind(request.expected_route.runtime_build_revision().as_str())
            .bind(to_i64(
                request
                    .expected_route
                    .process_identity()
                    .runtime_generation
                    .get(),
            )?)
            .bind(to_i64(request.expected_route.route_fencing_token().get())?)
            .bind(to_i64(request.expected_route.route_incarnation().get())?)
            .bind(effect_state_code_v1(request.source_effect_state))
            .bind(recovery_path_code_v1(request.recovery_path))
            .bind(digest_bytes(request.preflight_certificate_digest.as_str())?)
            .bind(outcome_code)
            .bind(digest_bytes(&request.observation_digest)?)
            .bind(request.output_parameter())
            .bind(request.retry_delay_milliseconds)
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_mutation_error(&error))?;
        let checkpoint = exactly_one(rows)?.decode(outcome_code)?;
        validate_checkpoint(
            &checkpoint,
            expected_state,
            request.effect_head_revision + 1,
            recovery_expected,
        )?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        Ok(checkpoint)
    }

    pub async fn intend_interaction_effect_compensation_v1(
        &self,
        request: RuntimeInteractionEffectCompensationIntendRequestV1,
    ) -> Result<
        RuntimeInteractionEffectCompensationIntendOutcomeV1,
        RuntimeInteractionPersistenceErrorV1,
    > {
        let identity = request.candidate.key.identity();
        let expected = &request.expected_route;
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows =
            sqlx::query_as::<_, EffectCompensationIntendRowV1>(EFFECT_COMPENSATION_INTEND_QUERY)
                .bind(identity.application_id().get().to_string())
                .bind(identity.interaction_id().get().to_string())
                .bind(i64::from(request.candidate.key.action_index().get()))
                .bind(to_i64(request.candidate.effect_head_revision)?)
                .bind(expected.process_identity().process_instance_id.as_str())
                .bind(expected.gateway_shard_identity().as_str())
                .bind(expected.runtime_build_revision().as_str())
                .bind(to_i64(
                    expected.process_identity().runtime_generation.get(),
                )?)
                .bind(to_i64(expected.route_fencing_token().get())?)
                .bind(to_i64(expected.route_incarnation().get())?)
                .bind(digest_bytes(request.preflight_certificate_digest.as_str())?)
                .bind(digest_bytes(request.compensation_intent_digest.as_str())?)
                .bind(request.retry_delay_milliseconds)
                .fetch_all(&mut *transaction)
                .await
                .map_err(|error| map_mutation_error(&error))?;
        let claim = exactly_one(rows)?.decode(request)?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        Ok(claim)
    }

    pub async fn finish_interaction_effect_compensation_v1(
        &self,
        request: RuntimeInteractionEffectCompensationFinishRequestV1,
    ) -> Result<RuntimeInteractionEffectCheckpointV1, RuntimeInteractionPersistenceErrorV1> {
        let outcome_code = request.outcome_code();
        let (expected_state, recovery_expected) = match outcome_code {
            "compensated" => (
                automation_runtime_interaction::InteractionEffectStateV1::Compensated,
                false,
            ),
            "indeterminate" => (
                automation_runtime_interaction::InteractionEffectStateV1::CompensationIndeterminate,
                true,
            ),
            "definitive_failure" => (
                automation_runtime_interaction::InteractionEffectStateV1::RecoveryRequired,
                false,
            ),
            _ => return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput),
        };
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows = sqlx::query_as::<_, EffectCheckpointRowV1>(EFFECT_COMPENSATION_FINISH_QUERY)
            .bind(request.identity.application_id().get().to_string())
            .bind(request.identity.interaction_id().get().to_string())
            .bind(i64::from(request.action_index.get()))
            .bind(to_i64(request.effect_head_revision)?)
            .bind(to_i64(request.recovery_claim_revision)?)
            .bind(request.process_instance_id.as_str())
            .bind(digest_bytes(request.preflight_certificate_digest.as_str())?)
            .bind(outcome_code)
            .bind(digest_bytes(request.compensation_result_digest().as_str())?)
            .bind(request.retry_delay_milliseconds)
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_mutation_error(&error))?;
        let checkpoint = exactly_one(rows)?.decode(outcome_code)?;
        validate_checkpoint(
            &checkpoint,
            expected_state,
            request.effect_head_revision + 1,
            recovery_expected,
        )?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        Ok(checkpoint)
    }
}

fn exactly_one<T>(rows: Vec<T>) -> Result<T, RuntimeInteractionPersistenceErrorV1> {
    let mut rows = rows.into_iter();
    let row = rows
        .next()
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    if rows.next().is_some() {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(row)
}

fn to_i64(value: u64) -> Result<i64, RuntimeInteractionPersistenceErrorV1> {
    i64::try_from(value).map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)
}

fn scan_parameters(
    key: Option<&RuntimeInteractionEffectRecoveryScanKeyV1>,
    epoch: chrono::DateTime<Utc>,
) -> (chrono::DateTime<Utc>, String, String, i64) {
    key.map(|key| {
        (
            key.recovery_at(),
            key.identity().application_id().get().to_string(),
            key.identity().interaction_id().get().to_string(),
            i64::from(key.action_index().get()),
        )
    })
    .unwrap_or((epoch, String::new(), String::new(), -1))
}

fn decode_scan_page(
    rows: Vec<EffectRecoveryScanRowV1>,
    after: Option<&RuntimeInteractionEffectRecoveryScanKeyV1>,
    requested_limit: NonZeroUsize,
) -> Result<RuntimeInteractionEffectRecoveryScanPageV1, RuntimeInteractionPersistenceErrorV1> {
    let mut candidates = Vec::with_capacity(rows.len());
    let mut through = None;
    let mut observed_database_now = None;
    for row in rows {
        let (candidate, row_through, row_database_now) = row.decode()?;
        if through
            .as_ref()
            .is_some_and(|existing| existing != &row_through)
            || observed_database_now.is_some_and(|existing| existing != row_database_now)
            || after.is_some_and(|after| candidate.key().cmp_c(after).is_le())
            || candidates.last().is_some_and(
                |previous: &crate::effect::RuntimeInteractionEffectRecoveryCandidateV1| {
                    candidate.key().cmp_c(previous.key()).is_le()
                },
            )
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        through = Some(row_through);
        observed_database_now = Some(row_database_now);
        candidates.push(candidate);
    }
    Ok(RuntimeInteractionEffectRecoveryScanPageV1::new(
        candidates,
        through,
        observed_database_now,
        requested_limit,
    ))
}

fn validate_checkpoint(
    checkpoint: &RuntimeInteractionEffectCheckpointV1,
    expected_state: automation_runtime_interaction::InteractionEffectStateV1,
    expected_revision: u64,
    recovery_expected: bool,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    if checkpoint.state() != expected_state
        || checkpoint.effect_head_revision() != expected_revision
        || checkpoint.recovery_at().is_some() != recovery_expected
    {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn reconcile_expected_checkpoint(
    outcome: &str,
) -> Result<
    (
        automation_runtime_interaction::InteractionEffectStateV1,
        bool,
    ),
    RuntimeInteractionPersistenceErrorV1,
> {
    use automation_runtime_interaction::InteractionEffectStateV1;
    match outcome {
        "adopted_success" => Ok((InteractionEffectStateV1::ReconciledSucceeded, false)),
        "observed_failure" => Ok((InteractionEffectStateV1::KnownFailed, false)),
        "deferred" => Ok((InteractionEffectStateV1::ObservationPending, true)),
        "conflict"
        | "unsupported"
        | "compensation_conflict"
        | "compensation_unsupported"
        | "recovery_blocked_discord_read_rejected"
        | "recovery_blocked_response_token_unavailable"
        | "recovery_blocked_observation_protocol"
        | "recovery_blocked_compensation_conflict"
        | "recovery_blocked_compensation_unsupported"
        | "recovery_blocked_non_compensable"
        | "recovery_blocked_internal_conflict"
        | "recovery_blocked_discord_forbidden"
        | "recovery_blocked_internal_authority"
        | "recovery_blocked_attempt_budget_exhausted" => {
            Ok((InteractionEffectStateV1::RecoveryRequired, false))
        }
        "compensation_restored" => Ok((InteractionEffectStateV1::Compensated, false)),
        "compensation_deferred" => Ok((
            InteractionEffectStateV1::CompensationObservationPending,
            true,
        )),
        _ => Err(RuntimeInteractionPersistenceErrorV1::InvalidInput),
    }
}
