use std::num::NonZeroUsize;

use automation_runtime_interaction::{
    InteractionActionPlanDigestV1, InteractionExecutionRouteV1, InteractionReceiptClaimCandidateV1,
    InteractionReceiptStateV1,
};
use chrono::{TimeZone, Utc};

use crate::contract::{
    RECEIPT_ACKNOWLEDGEMENT_FINISH_QUERY, RECEIPT_ACKNOWLEDGEMENT_INTEND_QUERY,
    RECEIPT_AUTHORITY_OBSERVE_QUERY, RECEIPT_CLAIM_QUERY, RECEIPT_EXECUTION_INTEND_QUERY,
    RECEIPT_FINISH_QUERY, RECEIPT_PLAN_BIND_QUERY, RECEIPT_RECOVERY_SCAN_QUERY,
    RECEIPT_RECOVER_QUERY, RECEIPT_TERMINALIZE_EXPIRED_QUERY, RECEIPT_TOKEN_EXPIRE_QUERY,
};
use crate::database::{begin_interaction_transaction, verify_runtime_interaction_binding_v1};
use crate::error::{map_mutation_commit_error, map_mutation_error, map_query_error};
use crate::receipt::{
    datetime_from_unix_milliseconds, RuntimeInteractionReceiptAuthorityV1,
    RuntimeInteractionReceiptClaimOutcomeV1, RuntimeInteractionReceiptClaimRequestV1,
    RuntimeInteractionReceiptExclusiveClaimV1,
    RuntimeInteractionReceiptInitialResponseIntentDispositionV1,
    RuntimeInteractionReceiptInitialResponseIntentV1,
    RuntimeInteractionReceiptInitialResponseKindV1,
    RuntimeInteractionReceiptInitialResponseResultKindV1,
    RuntimeInteractionReceiptInitialResponseResultV1,
    RuntimeInteractionReceiptMutationDispositionV1, RuntimeInteractionReceiptRecoveryOutcomeV1,
    RuntimeInteractionReceiptRecoveryRequestV1, RuntimeInteractionReceiptRecoveryScanCursorV1,
    RuntimeInteractionReceiptRecoveryScanPageV1, RuntimeInteractionReceiptRouteV1,
    RuntimeInteractionReceiptTerminalOutcomeV1, RuntimeInteractionReceiptTerminalStateV1,
    RuntimeInteractionReceiptTerminalizeExpiredOutcomeV1,
    RuntimeInteractionReceiptTerminalizeExpiredRequestV1,
    RuntimeInteractionReceiptTokenExpiryOutcomeV1, RuntimeInteractionReceiptTokenExpiryRequestV1,
    MAX_RUNTIME_INTERACTION_RECEIPT_RECOVERY_SCAN_BATCH,
};
use crate::receipt_row::{
    digest_bytes, ReceiptAuthorityRowV1, ReceiptCheckpointV1, ReceiptClaimRowV1,
    ReceiptMutationRowV1, ReceiptRecoverRowV1, ReceiptRecoveryScanRowV1,
    ReceiptTerminalizeExpiredRowV1, ReceiptTokenExpiryRowV1,
};
use crate::{PostgresRuntimeInteractionV1, RuntimeInteractionPersistenceErrorV1};

impl PostgresRuntimeInteractionV1 {
    pub async fn observe_interaction_receipt_authority_v1(
        &self,
        candidate: InteractionReceiptClaimCandidateV1,
        route: RuntimeInteractionReceiptRouteV1,
    ) -> Result<RuntimeInteractionReceiptAuthorityV1, RuntimeInteractionPersistenceErrorV1> {
        let identity = candidate.identity();
        let expected = candidate.expected_route();
        let process = expected.process_identity();
        let target = &process.target;
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows = sqlx::query_as::<_, ReceiptAuthorityRowV1>(RECEIPT_AUTHORITY_OBSERVE_QUERY)
            .bind(identity.application_id().get().to_string())
            .bind(expected.scope().tenant_id().as_str())
            .bind(expected.scope().installation_id().as_str())
            .bind(expected.scope().deployment_id().as_str())
            .bind(target.guild_id.to_string())
            .bind(target.ruleset_key.as_str())
            .bind(i64::from(target.version.get()))
            .bind(target.content_hash.to_hex())
            .bind(to_i64(target.binding_revision.get())?)
            .bind(target.binding_fingerprint.as_str())
            .bind(to_i64(process.runtime_generation.get())?)
            .bind(to_i64(expected.route_fencing_token().get())?)
            .bind(to_i64(expected.route_incarnation().get())?)
            .bind(process.process_instance_id.as_str())
            .bind(expected.gateway_shard_identity().as_str())
            .bind(expected.runtime_build_revision().as_str())
            .bind(route.kind_code())
            .bind(route.instance_id_parameter())
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_query_error(&error))?;
        let row = exactly_one(rows)?;
        let authority = row.decode(candidate, route)?;
        transaction
            .commit()
            .await
            .map_err(|error| map_query_error(&error))?;
        Ok(authority)
    }

    pub async fn claim_interaction_receipt_v1(
        &self,
        request: RuntimeInteractionReceiptClaimRequestV1,
    ) -> Result<RuntimeInteractionReceiptClaimOutcomeV1, RuntimeInteractionPersistenceErrorV1> {
        let root = request.claim_root();
        let identity = root.identity();
        let route = root.route();
        let process = route.process_identity();
        let target = &process.target;
        let serving = route.serving_identity();
        let (execution_version, execution_hash, instance_manifest_digest, expected_instance_id) =
            match route.execution_route() {
                InteractionExecutionRouteV1::Static {
                    ruleset_version,
                    ruleset_content_hash,
                } => (
                    i64::from(ruleset_version.get()),
                    ruleset_content_hash.to_hex(),
                    String::new(),
                    String::new(),
                ),
                InteractionExecutionRouteV1::Instance {
                    instance_id,
                    pinned_ruleset_version,
                    pinned_ruleset_content_hash,
                    resource_manifest_digest,
                } => (
                    i64::from(pinned_ruleset_version.get()),
                    pinned_ruleset_content_hash.to_hex(),
                    resource_manifest_digest.as_str().to_string(),
                    instance_id.as_str().to_string(),
                ),
            };
        if expected_instance_id != request.authority().route().instance_id_parameter() {
            return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
        }
        let request_digest = digest_bytes(root.request_digest().as_str())?;
        let encrypted_token = request.encrypted_token();
        let token_aad_digest = digest_bytes(encrypted_token.authenticated_data_digest().as_str())?;
        let token_issued_at =
            datetime_from_unix_milliseconds(encrypted_token.time().issued_at_unix_milliseconds())?;
        let token_expires_at =
            datetime_from_unix_milliseconds(encrypted_token.time().expires_at_unix_milliseconds())?;
        let token_suite_version = i16::try_from(encrypted_token.encryption_suite_version())
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows = sqlx::query_as::<_, ReceiptClaimRowV1>(RECEIPT_CLAIM_QUERY)
            .bind(identity.application_id().get().to_string())
            .bind(identity.interaction_id().get().to_string())
            .bind(route.scope().tenant_id().as_str())
            .bind(route.scope().installation_id().as_str())
            .bind(route.scope().deployment_id().as_str())
            .bind(target.guild_id.to_string())
            .bind(request.channel_id().to_string())
            .bind(request.actor_user_id().to_string())
            .bind(request.request_kind().code())
            .bind(target.ruleset_key.as_str())
            .bind(i64::from(target.version.get()))
            .bind(target.content_hash.to_hex())
            .bind(to_i64(target.binding_revision.get())?)
            .bind(target.binding_fingerprint.as_str())
            .bind(to_i64(process.runtime_generation.get())?)
            .bind(to_i64(serving.route_fencing_token().get())?)
            .bind(to_i64(serving.route_incarnation().get())?)
            .bind(process.process_instance_id.as_str())
            .bind(serving.gateway_shard_identity().as_str())
            .bind(serving.runtime_build_revision().as_str())
            .bind(request.authority().route().kind_code())
            .bind(request.authority().route().route_key())
            .bind(expected_instance_id)
            .bind(serving.attestation_digest().as_str())
            .bind(to_i64(serving.lease_epoch().get())?)
            .bind(to_i64(serving.lease_revision().get())?)
            .bind(to_i64(serving.gateway_owner_lease_epoch().get())?)
            .bind(to_i64(serving.gateway_owner_revision().get())?)
            .bind(execution_version)
            .bind(execution_hash)
            .bind(instance_manifest_digest)
            .bind(request_digest)
            .bind(request.claim_lease().milliseconds())
            .bind(encrypted_token.encryption_suite())
            .bind(token_suite_version)
            .bind(encrypted_token.encryption_key_id())
            .bind(encrypted_token.nonce())
            .bind(encrypted_token.ciphertext())
            .bind(token_aad_digest)
            .bind(token_issued_at)
            .bind(token_expires_at)
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_mutation_error(&error))?;
        let row = exactly_one(rows)?;
        let outcome = row.decode(&request)?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        Ok(outcome)
    }

    pub async fn bind_interaction_receipt_action_plan_v1(
        &self,
        claim: &mut RuntimeInteractionReceiptExclusiveClaimV1,
        action_plan_digest: InteractionActionPlanDigestV1,
    ) -> Result<RuntimeInteractionReceiptMutationDispositionV1, RuntimeInteractionPersistenceErrorV1>
    {
        if claim
            .action_plan_digest()
            .is_some_and(|existing| existing != &action_plan_digest)
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let digest = digest_bytes(action_plan_digest.as_str())?;
        let checkpoint = self
            .receipt_digest_mutation_v1(RECEIPT_PLAN_BIND_QUERY, claim, digest)
            .await?;
        let disposition = validate_checkpoint(
            &checkpoint,
            claim,
            "plan_bound",
            InteractionReceiptStateV1::Prepared,
        )?;
        claim.update_checkpoint(
            checkpoint.state,
            checkpoint.head_revision,
            checkpoint.claim_revision,
            checkpoint.claim_expires_at,
            checkpoint.observed_database_now,
        );
        claim.set_action_plan(action_plan_digest);
        Ok(disposition)
    }

    pub async fn intend_interaction_receipt_initial_response_v1(
        &self,
        claim: &mut RuntimeInteractionReceiptExclusiveClaimV1,
        intent: RuntimeInteractionReceiptInitialResponseIntentV1,
    ) -> Result<
        RuntimeInteractionReceiptInitialResponseIntentDispositionV1,
        RuntimeInteractionPersistenceErrorV1,
    > {
        if intent.kind() == RuntimeInteractionReceiptInitialResponseKindV1::DeferEphemeral {
            if claim.action_plan_digest().is_some() {
                return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
            }
        } else if claim.action_plan_digest().is_none() {
            return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
        }
        if claim
            .acknowledgement_intent()
            .is_some_and(|existing| existing != &intent)
        {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let identity = claim.claim_root().identity();
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows = sqlx::query_as::<_, ReceiptMutationRowV1>(RECEIPT_ACKNOWLEDGEMENT_INTEND_QUERY)
            .bind(identity.application_id().get().to_string())
            .bind(identity.interaction_id().get().to_string())
            .bind(to_i64(claim.head_revision())?)
            .bind(to_i64(claim.claim_revision())?)
            .bind(claim.claim_process_instance_id().as_str())
            .bind(intent.kind().code())
            .bind(intent.digest().as_bytes().as_slice())
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_mutation_error(&error))?;
        let checkpoint =
            exactly_one(rows)?.decode(claim.claim_revision(), claim.claim_expires_at())?;
        let expected_state = if claim.state() == InteractionReceiptStateV1::Executing {
            InteractionReceiptStateV1::Executing
        } else {
            InteractionReceiptStateV1::Acknowledging
        };
        let disposition = validate_checkpoint(
            &checkpoint,
            claim,
            "acknowledgement_intended",
            expected_state,
        )?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        claim.update_checkpoint(
            checkpoint.state,
            checkpoint.head_revision,
            checkpoint.claim_revision,
            checkpoint.claim_expires_at,
            checkpoint.observed_database_now,
        );
        claim.set_acknowledgement_intent(intent);
        Ok(RuntimeInteractionReceiptInitialResponseIntentDispositionV1::from_mutation(disposition))
    }

    pub async fn finish_interaction_receipt_initial_response_v1(
        &self,
        claim: &mut RuntimeInteractionReceiptExclusiveClaimV1,
        result: RuntimeInteractionReceiptInitialResponseResultV1,
    ) -> Result<RuntimeInteractionReceiptMutationDispositionV1, RuntimeInteractionPersistenceErrorV1>
    {
        let intent = claim
            .acknowledgement_intent()
            .ok_or(RuntimeInteractionPersistenceErrorV1::Conflict)?;
        if intent.digest() != result.intent_digest() {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let was_executing = claim.state() == InteractionReceiptStateV1::Executing;
        let expected_state = match result.result() {
            RuntimeInteractionReceiptInitialResponseResultKindV1::Succeeded if was_executing => {
                InteractionReceiptStateV1::Executing
            }
            RuntimeInteractionReceiptInitialResponseResultKindV1::Succeeded
                if intent.kind()
                    == RuntimeInteractionReceiptInitialResponseKindV1::DeferEphemeral =>
            {
                InteractionReceiptStateV1::Deferred
            }
            RuntimeInteractionReceiptInitialResponseResultKindV1::Succeeded => {
                InteractionReceiptStateV1::Prepared
            }
            RuntimeInteractionReceiptInitialResponseResultKindV1::DefinitiveFailure
                if !was_executing =>
            {
                InteractionReceiptStateV1::Failed
            }
            RuntimeInteractionReceiptInitialResponseResultKindV1::DefinitiveFailure
            | RuntimeInteractionReceiptInitialResponseResultKindV1::Indeterminate => {
                InteractionReceiptStateV1::RecoveryRequired
            }
        };
        let expected_outcome = match result.result() {
            RuntimeInteractionReceiptInitialResponseResultKindV1::Succeeded => {
                "acknowledgement_succeeded"
            }
            RuntimeInteractionReceiptInitialResponseResultKindV1::DefinitiveFailure
                if was_executing =>
            {
                "acknowledgement_failure_after_execution_intent"
            }
            RuntimeInteractionReceiptInitialResponseResultKindV1::DefinitiveFailure => {
                "acknowledgement_definitive_failure"
            }
            RuntimeInteractionReceiptInitialResponseResultKindV1::Indeterminate => {
                "acknowledgement_indeterminate"
            }
        };
        let identity = claim.claim_root().identity();
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows = sqlx::query_as::<_, ReceiptMutationRowV1>(RECEIPT_ACKNOWLEDGEMENT_FINISH_QUERY)
            .bind(identity.application_id().get().to_string())
            .bind(identity.interaction_id().get().to_string())
            .bind(to_i64(claim.head_revision())?)
            .bind(to_i64(claim.claim_revision())?)
            .bind(claim.claim_process_instance_id().as_str())
            .bind(result.intent_digest().as_bytes().as_slice())
            .bind(result.result().code())
            .bind(result.result_digest().as_bytes().as_slice())
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_mutation_error(&error))?;
        let checkpoint =
            exactly_one(rows)?.decode(claim.claim_revision(), claim.claim_expires_at())?;
        let disposition =
            validate_checkpoint(&checkpoint, claim, expected_outcome, expected_state)?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        claim.update_checkpoint(
            checkpoint.state,
            checkpoint.head_revision,
            checkpoint.claim_revision,
            checkpoint.claim_expires_at,
            checkpoint.observed_database_now,
        );
        Ok(disposition)
    }

    pub async fn intend_interaction_receipt_execution_v1(
        &self,
        claim: &mut RuntimeInteractionReceiptExclusiveClaimV1,
    ) -> Result<RuntimeInteractionReceiptMutationDispositionV1, RuntimeInteractionPersistenceErrorV1>
    {
        let digest = claim
            .action_plan_digest()
            .ok_or(RuntimeInteractionPersistenceErrorV1::Conflict)?;
        if !matches!(
            claim.state(),
            InteractionReceiptStateV1::Prepared | InteractionReceiptStateV1::Executing
        ) {
            return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
        }
        let digest = digest_bytes(digest.as_str())?;
        let checkpoint = self
            .receipt_digest_mutation_v1(RECEIPT_EXECUTION_INTEND_QUERY, claim, digest)
            .await?;
        let disposition = validate_checkpoint(
            &checkpoint,
            claim,
            "execution_intended",
            InteractionReceiptStateV1::Executing,
        )?;
        claim.update_checkpoint(
            checkpoint.state,
            checkpoint.head_revision,
            checkpoint.claim_revision,
            checkpoint.claim_expires_at,
            checkpoint.observed_database_now,
        );
        Ok(disposition)
    }

    pub async fn finish_interaction_receipt_v1(
        &self,
        claim: &mut RuntimeInteractionReceiptExclusiveClaimV1,
        terminal: RuntimeInteractionReceiptTerminalOutcomeV1,
    ) -> Result<RuntimeInteractionReceiptMutationDispositionV1, RuntimeInteractionPersistenceErrorV1>
    {
        if terminal.state() == RuntimeInteractionReceiptTerminalStateV1::Completed
            && claim.action_plan_digest().is_none()
        {
            return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
        }
        if terminal.state() == RuntimeInteractionReceiptTerminalStateV1::Failed
            && claim.state() == InteractionReceiptStateV1::Executing
        {
            return Err(RuntimeInteractionPersistenceErrorV1::Conflict);
        }
        if claim.state().is_terminal() && claim.state() != terminal.state().state() {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let identity = claim.claim_root().identity();
        let plan_digest = claim
            .action_plan_digest()
            .map(|digest| digest_bytes(digest.as_str()))
            .transpose()?
            .unwrap_or_default();
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows = sqlx::query_as::<_, ReceiptMutationRowV1>(RECEIPT_FINISH_QUERY)
            .bind(identity.application_id().get().to_string())
            .bind(identity.interaction_id().get().to_string())
            .bind(to_i64(claim.head_revision())?)
            .bind(to_i64(claim.claim_revision())?)
            .bind(claim.claim_process_instance_id().as_str())
            .bind(plan_digest)
            .bind(terminal.state().code())
            .bind(terminal.outcome_code())
            .bind(terminal.result_digest().as_bytes().as_slice())
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_mutation_error(&error))?;
        let checkpoint =
            exactly_one(rows)?.decode(claim.claim_revision(), claim.claim_expires_at())?;
        let disposition = validate_checkpoint(
            &checkpoint,
            claim,
            terminal.outcome_code(),
            terminal.state().state(),
        )?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        claim.update_checkpoint(
            checkpoint.state,
            checkpoint.head_revision,
            checkpoint.claim_revision,
            checkpoint.claim_expires_at,
            checkpoint.observed_database_now,
        );
        Ok(disposition)
    }

    pub async fn scan_recoverable_interaction_receipts_v1(
        &self,
        cursor: &RuntimeInteractionReceiptRecoveryScanCursorV1,
        limit: NonZeroUsize,
    ) -> Result<RuntimeInteractionReceiptRecoveryScanPageV1, RuntimeInteractionPersistenceErrorV1>
    {
        if limit.get() > MAX_RUNTIME_INTERACTION_RECEIPT_RECOVERY_SCAN_BATCH {
            return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
        }
        let epoch = Utc
            .timestamp_opt(0, 0)
            .single()
            .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
        let after_time = cursor
            .after()
            .map(|key| key.claim_expires_at())
            .unwrap_or(epoch);
        let after_application = cursor
            .after()
            .map(|key| key.identity().application_id().get().to_string())
            .unwrap_or_default();
        let after_interaction = cursor
            .after()
            .map(|key| key.identity().interaction_id().get().to_string())
            .unwrap_or_default();
        let through_time = cursor
            .through()
            .map(|key| key.claim_expires_at())
            .unwrap_or(epoch);
        let through_application = cursor
            .through()
            .map(|key| key.identity().application_id().get().to_string())
            .unwrap_or_default();
        let through_interaction = cursor
            .through()
            .map(|key| key.identity().interaction_id().get().to_string())
            .unwrap_or_default();
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows = sqlx::query_as::<_, ReceiptRecoveryScanRowV1>(RECEIPT_RECOVERY_SCAN_QUERY)
            .bind(after_time)
            .bind(after_application)
            .bind(after_interaction)
            .bind(through_time)
            .bind(through_application)
            .bind(through_interaction)
            .bind(
                i64::try_from(limit.get())
                    .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?,
            )
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_query_error(&error))?;
        if rows.len() > limit.get() {
            return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
        }
        let mut candidates = Vec::with_capacity(rows.len());
        let mut observed_database_now = None;
        let mut through = None;
        for row in rows {
            let (candidate, row_through, row_observed) = row.decode()?;
            if observed_database_now.is_some_and(|observed| observed != row_observed)
                || through
                    .as_ref()
                    .is_some_and(|existing| existing != &row_through)
                || cursor
                    .through()
                    .is_some_and(|expected| expected != &row_through)
                || cursor
                    .after()
                    .is_some_and(|after| candidate.key().cmp_c(after).is_le())
                || candidates.last().is_some_and(
                    |previous: &crate::RuntimeInteractionReceiptRecoveryCandidateV1| {
                        candidate.key().cmp_c(previous.key()).is_le()
                    },
                )
            {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
            observed_database_now = Some(row_observed);
            through = Some(row_through);
            candidates.push(candidate);
        }
        if candidates.is_empty() {
            through = cursor.through().cloned();
        }
        transaction
            .commit()
            .await
            .map_err(|error| map_query_error(&error))?;
        Ok(RuntimeInteractionReceiptRecoveryScanPageV1::new(
            candidates,
            through,
            observed_database_now,
            limit,
        ))
    }

    pub async fn recover_interaction_receipt_v1(
        &self,
        request: RuntimeInteractionReceiptRecoveryRequestV1,
    ) -> Result<RuntimeInteractionReceiptRecoveryOutcomeV1, RuntimeInteractionPersistenceErrorV1>
    {
        let identity = request.candidate().key().identity();
        let expected = request.expected_route();
        let process = expected.process_identity();
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows = sqlx::query_as::<_, ReceiptRecoverRowV1>(RECEIPT_RECOVER_QUERY)
            .bind(identity.application_id().get().to_string())
            .bind(identity.interaction_id().get().to_string())
            .bind(to_i64(request.candidate().head_revision())?)
            .bind(to_i64(request.candidate().claim_revision())?)
            .bind(process.process_instance_id.as_str())
            .bind(to_i64(process.runtime_generation.get())?)
            .bind(to_i64(expected.route_fencing_token().get())?)
            .bind(to_i64(expected.route_incarnation().get())?)
            .bind(expected.gateway_shard_identity().as_str())
            .bind(expected.runtime_build_revision().as_str())
            .bind(request.observation_kind().code())
            .bind(request.observation_digest().as_bytes().as_slice())
            .bind(request.claim_lease().milliseconds())
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_mutation_error(&error))?;
        let outcome = exactly_one(rows)?.decode(&request)?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        Ok(outcome)
    }

    pub async fn expire_interaction_receipt_token_v1(
        &self,
        request: RuntimeInteractionReceiptTokenExpiryRequestV1,
    ) -> Result<RuntimeInteractionReceiptTokenExpiryOutcomeV1, RuntimeInteractionPersistenceErrorV1>
    {
        let identity = request.identity();
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows = sqlx::query_as::<_, ReceiptTokenExpiryRowV1>(RECEIPT_TOKEN_EXPIRE_QUERY)
            .bind(identity.application_id().get().to_string())
            .bind(identity.interaction_id().get().to_string())
            .bind(to_i64(request.expected_head_revision())?)
            .bind(to_i64(request.expected_claim_revision())?)
            .bind(request.observation_digest().as_bytes().as_slice())
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_mutation_error(&error))?;
        let outcome = exactly_one(rows)?.decode(
            request.expected_head_revision(),
            request.expected_claim_revision(),
        )?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        Ok(outcome)
    }

    pub async fn terminalize_expired_interaction_receipt_v1(
        &self,
        request: RuntimeInteractionReceiptTerminalizeExpiredRequestV1,
    ) -> Result<
        RuntimeInteractionReceiptTerminalizeExpiredOutcomeV1,
        RuntimeInteractionPersistenceErrorV1,
    > {
        let identity = request.identity();
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let rows =
            sqlx::query_as::<_, ReceiptTerminalizeExpiredRowV1>(RECEIPT_TERMINALIZE_EXPIRED_QUERY)
                .bind(identity.application_id().get().to_string())
                .bind(identity.interaction_id().get().to_string())
                .bind(to_i64(request.expected_head_revision())?)
                .bind(to_i64(request.expected_claim_revision())?)
                .bind(request.expected_process_instance_id().as_str())
                .bind(request.expected_runtime_build_revision().as_str())
                .bind(request.observation_digest().as_bytes().as_slice())
                .fetch_all(&mut *transaction)
                .await
                .map_err(|error| map_mutation_error(&error))?;
        let outcome = exactly_one(rows)?.decode(
            request.expected_head_revision(),
            request.expected_claim_revision(),
        )?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        Ok(outcome)
    }

    async fn receipt_digest_mutation_v1(
        &self,
        query_text: &'static str,
        claim: &RuntimeInteractionReceiptExclusiveClaimV1,
        digest: Vec<u8>,
    ) -> Result<ReceiptCheckpointV1, RuntimeInteractionPersistenceErrorV1> {
        let identity = claim.claim_root().identity();
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts).await?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
        let query = sqlx::query_as::<_, ReceiptMutationRowV1>(query_text)
            .bind(identity.application_id().get().to_string())
            .bind(identity.interaction_id().get().to_string())
            .bind(to_i64(claim.head_revision())?)
            .bind(to_i64(claim.claim_revision())?)
            .bind(claim.claim_process_instance_id().as_str())
            .bind(digest);
        let rows = query
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_mutation_error(&error))?;
        let checkpoint =
            exactly_one(rows)?.decode(claim.claim_revision(), claim.claim_expires_at())?;
        transaction
            .commit()
            .await
            .map_err(|error| map_mutation_commit_error(&error))?;
        Ok(checkpoint)
    }
}

fn validate_checkpoint(
    checkpoint: &ReceiptCheckpointV1,
    claim: &RuntimeInteractionReceiptExclusiveClaimV1,
    applied_outcome: &str,
    applied_state: InteractionReceiptStateV1,
) -> Result<RuntimeInteractionReceiptMutationDispositionV1, RuntimeInteractionPersistenceErrorV1> {
    validate_checkpoint_values(
        checkpoint,
        claim.state(),
        claim.head_revision(),
        applied_outcome,
        applied_state,
    )
}

fn validate_checkpoint_values(
    checkpoint: &ReceiptCheckpointV1,
    current_state: InteractionReceiptStateV1,
    current_head_revision: u64,
    applied_outcome: &str,
    applied_state: InteractionReceiptStateV1,
) -> Result<RuntimeInteractionReceiptMutationDispositionV1, RuntimeInteractionPersistenceErrorV1> {
    let next_revision = current_head_revision
        .checked_add(1)
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    match checkpoint.outcome_name.as_str() {
        outcome
            if outcome == applied_outcome
                && checkpoint.state == applied_state
                && checkpoint.head_revision == next_revision =>
        {
            Ok(RuntimeInteractionReceiptMutationDispositionV1::Applied)
        }
        "exact_replay"
            if (checkpoint.head_revision == current_head_revision
                && checkpoint.state == current_state)
                || (checkpoint.head_revision == next_revision
                    && checkpoint.state == applied_state) =>
        {
            Ok(RuntimeInteractionReceiptMutationDispositionV1::ExactReplay)
        }
        _ => Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt),
    }
}

fn exactly_one<T>(rows: Vec<T>) -> Result<T, RuntimeInteractionPersistenceErrorV1> {
    let [row]: [T; 1] = rows
        .try_into()
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    Ok(row)
}

fn to_i64(value: u64) -> Result<i64, RuntimeInteractionPersistenceErrorV1> {
    i64::try_from(value).map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn checkpoint(
        outcome_name: &str,
        state: InteractionReceiptStateV1,
        head_revision: u64,
    ) -> ReceiptCheckpointV1 {
        ReceiptCheckpointV1 {
            outcome_name: outcome_name.to_string(),
            state,
            head_revision,
            claim_revision: 3,
            claim_expires_at: Utc.timestamp_millis_opt(2_000).single().unwrap(),
            observed_database_now: Utc.timestamp_millis_opt(1_000).single().unwrap(),
        }
    }

    #[test]
    fn checkpoint_decoder_accepts_only_applied_or_exact_successor_replay() {
        let current = InteractionReceiptStateV1::Deferred;
        let applied = InteractionReceiptStateV1::Prepared;
        assert_eq!(
            validate_checkpoint_values(
                &checkpoint("plan_bound", applied, 5),
                current,
                4,
                "plan_bound",
                applied
            ),
            Ok(RuntimeInteractionReceiptMutationDispositionV1::Applied)
        );
        assert_eq!(
            validate_checkpoint_values(
                &checkpoint("exact_replay", current, 4),
                current,
                4,
                "plan_bound",
                applied
            ),
            Ok(RuntimeInteractionReceiptMutationDispositionV1::ExactReplay)
        );
        assert_eq!(
            validate_checkpoint_values(
                &checkpoint("exact_replay", applied, 5),
                current,
                4,
                "plan_bound",
                applied
            ),
            Ok(RuntimeInteractionReceiptMutationDispositionV1::ExactReplay)
        );
        assert_eq!(
            validate_checkpoint_values(
                &checkpoint("exact_replay", current, 5),
                current,
                4,
                "plan_bound",
                applied
            ),
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );
        assert_eq!(
            validate_checkpoint_values(
                &checkpoint("exact_replay", applied, 6),
                current,
                4,
                "plan_bound",
                applied
            ),
            Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
        );
    }
}
