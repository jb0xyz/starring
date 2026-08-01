use std::num::NonZeroUsize;

use chrono::{TimeZone, Utc};

use crate::contract::{
    EFFECT_RESPONSE_TAIL_CLAIM_QUERY, EFFECT_RESPONSE_TAIL_FINALIZE_QUERY,
    EFFECT_RESPONSE_TAIL_SCAN_QUERY,
};
use crate::database::{begin_interaction_transaction, verify_runtime_interaction_binding_v1};
use crate::effect::MAX_RUNTIME_INTERACTION_EFFECT_RECOVERY_SCAN_BATCH;
use crate::error::{map_mutation_commit_error, map_mutation_error, map_query_error};
use crate::receipt_row::digest_bytes;
use crate::response_tail::{
    RuntimeInteractionEffectResponseTailClaimOutcomeV1,
    RuntimeInteractionEffectResponseTailClaimRequestV1,
    RuntimeInteractionEffectResponseTailFinalizeOutcomeV1,
    RuntimeInteractionEffectResponseTailFinalizeRequestV1,
    RuntimeInteractionEffectResponseTailScanCursorV1,
    RuntimeInteractionEffectResponseTailScanKeyV1, RuntimeInteractionEffectResponseTailScanPageV1,
};
use crate::response_tail_row::{
    EffectResponseTailClaimRowV1, EffectResponseTailFinalizeRowV1, EffectResponseTailScanRowV1,
};
use crate::{PostgresRuntimeInteractionV1, RuntimeInteractionPersistenceErrorV1};

impl PostgresRuntimeInteractionV1 {
    pub async fn scan_recoverable_interaction_response_tails_v1(
        &self,
        cursor: &RuntimeInteractionEffectResponseTailScanCursorV1,
        limit: NonZeroUsize,
    ) -> Result<RuntimeInteractionEffectResponseTailScanPageV1, RuntimeInteractionPersistenceErrorV1>
    {
        if limit.get() > MAX_RUNTIME_INTERACTION_EFFECT_RECOVERY_SCAN_BATCH {
            return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
        }
        let epoch = Utc
            .timestamp_opt(0, 0)
            .single()
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        let (after_at, after_application, after_interaction, after_action) =
            response_scan_parameters_v1(cursor.after(), epoch);
        let (through_at, through_application, through_interaction, through_action) =
            response_scan_parameters_v1(cursor.through(), epoch);
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows =
            sqlx::query_as::<_, EffectResponseTailScanRowV1>(EFFECT_RESPONSE_TAIL_SCAN_QUERY)
                .bind(after_at)
                .bind(after_application)
                .bind(after_interaction)
                .bind(after_action)
                .bind(through_at)
                .bind(through_application)
                .bind(through_interaction)
                .bind(through_action)
                .bind(
                    i64::try_from(limit.get())
                        .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?,
                )
                .fetch_all(&mut *transaction)
                .await
                .map_err(|error| map_query_error(&error))?;
        let page = decode_response_scan_page_v1(rows, cursor.after(), limit)?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        Ok(page)
    }

    pub async fn claim_interaction_response_tail_v1(
        &self,
        request: RuntimeInteractionEffectResponseTailClaimRequestV1,
    ) -> Result<
        RuntimeInteractionEffectResponseTailClaimOutcomeV1,
        RuntimeInteractionPersistenceErrorV1,
    > {
        let candidate = &request.candidate;
        let identity = candidate.identity();
        let expected = &request.expected_route;
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows =
            sqlx::query_as::<_, EffectResponseTailClaimRowV1>(EFFECT_RESPONSE_TAIL_CLAIM_QUERY)
                .bind(identity.application_id().get().to_string())
                .bind(identity.interaction_id().get().to_string())
                .bind(i64::from(candidate.key.action_index().get()))
                .bind(to_i64_v1(candidate.effect_head_revision)?)
                .bind(expected.process_identity().process_instance_id.as_str())
                .bind(expected.gateway_shard_identity().as_str())
                .bind(expected.runtime_build_revision().as_str())
                .bind(to_i64_v1(
                    expected.process_identity().runtime_generation.get(),
                )?)
                .bind(to_i64_v1(expected.route_fencing_token().get())?)
                .bind(to_i64_v1(expected.route_incarnation().get())?)
                .bind(digest_bytes(
                    candidate.preflight_certificate_digest.as_str(),
                )?)
                .bind(digest_bytes(candidate.expected_postimage_digest.as_str())?)
                .bind(request.unrecoverable_digest.as_bytes().as_slice())
                .bind(request.claim_lease.milliseconds())
                .fetch_all(&mut *transaction)
                .await
                .map_err(|error| map_mutation_error(&error))?;
        let outcome = exactly_one_v1(rows)?.decode(request)?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        Ok(outcome)
    }

    pub async fn finalize_interaction_response_tail_v1(
        &self,
        request: RuntimeInteractionEffectResponseTailFinalizeRequestV1,
    ) -> Result<
        RuntimeInteractionEffectResponseTailFinalizeOutcomeV1,
        RuntimeInteractionPersistenceErrorV1,
    > {
        let expected = &request.expected_route;
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows = sqlx::query_as::<_, EffectResponseTailFinalizeRowV1>(
            EFFECT_RESPONSE_TAIL_FINALIZE_QUERY,
        )
        .bind(request.identity.application_id().get().to_string())
        .bind(request.identity.interaction_id().get().to_string())
        .bind(i64::from(request.action_index.get()))
        .bind(to_i64_v1(request.receipt_head_revision)?)
        .bind("executing")
        .bind(to_i64_v1(request.effect_head_revision)?)
        .bind(to_i64_v1(request.recovery_claim_revision)?)
        .bind(expected.process_identity().process_instance_id.as_str())
        .bind(expected.gateway_shard_identity().as_str())
        .bind(expected.runtime_build_revision().as_str())
        .bind(to_i64_v1(
            expected.process_identity().runtime_generation.get(),
        )?)
        .bind(to_i64_v1(expected.route_fencing_token().get())?)
        .bind(to_i64_v1(expected.route_incarnation().get())?)
        .bind(digest_bytes(request.preflight_certificate_digest.as_str())?)
        .bind(digest_bytes(request.expected_postimage_digest.as_str())?)
        .bind(request.observation_outcome_code)
        .bind(request.observation_digest.as_slice())
        .bind(request.terminal_result_digest.as_slice())
        .bind(request.retry_delay_milliseconds)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|error| map_mutation_error(&error))?;
        let row = exactly_one_v1(rows)?;
        let outcome = row.decode(&request)?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        Ok(outcome)
    }
}

fn response_scan_parameters_v1(
    key: Option<&RuntimeInteractionEffectResponseTailScanKeyV1>,
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

fn decode_response_scan_page_v1(
    rows: Vec<EffectResponseTailScanRowV1>,
    after: Option<&RuntimeInteractionEffectResponseTailScanKeyV1>,
    requested_limit: NonZeroUsize,
) -> Result<RuntimeInteractionEffectResponseTailScanPageV1, RuntimeInteractionPersistenceErrorV1> {
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
                |previous: &crate::response_tail::RuntimeInteractionEffectResponseTailCandidateV1| {
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
    Ok(RuntimeInteractionEffectResponseTailScanPageV1::new(
        candidates,
        through,
        observed_database_now,
        requested_limit,
    ))
}

fn exactly_one_v1<T>(rows: Vec<T>) -> Result<T, RuntimeInteractionPersistenceErrorV1> {
    let mut rows = rows.into_iter();
    let row = rows
        .next()
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    if rows.next().is_some() {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(row)
}

fn to_i64_v1(value: u64) -> Result<i64, RuntimeInteractionPersistenceErrorV1> {
    i64::try_from(value).map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)
}
