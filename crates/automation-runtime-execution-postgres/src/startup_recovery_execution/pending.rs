use std::future::Future;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use automation_runtime_controller::{
    RuntimeDrainIntentIdV2, RuntimeGatewayOwnerLeaseReceiptV1, RuntimeServingSlotV2,
};
use automation_runtime_convergence::RuntimeDeploymentTargetV1;
use automation_runtime_worker::{
    RuntimeAuthorizedPendingDrainAcknowledgementV2, RuntimeAuthorizedPendingDrainClaimV2,
    RuntimeAuthorizedPendingDrainSelectionV2, RuntimePendingDrainAcknowledgementExecutionPortV2,
    RuntimePendingDrainAcknowledgementReceiptV2, RuntimePendingDrainCandidateV2,
    RuntimePendingDrainClaimExecutionPortV2, RuntimePendingDrainClaimReceiptV2,
    RuntimePendingDrainNoCandidateReceiptV2, RuntimePendingDrainNoCandidateRecorderPortV2,
    RuntimePendingDrainRegistrySealWitnessV2, RuntimePendingDrainSelectionOutcomeV2,
    RuntimePendingDrainSelectionPortV2, RuntimePendingDrainSelectionReceiptV2,
    RuntimePendingDrainStateDigestV2, RuntimeSelectedPendingDrainNoCandidateV2,
    RuntimeStartupRecoveryClassV2, RuntimeStartupRecoveryExecutionActionIdentityV2,
    RuntimeStartupRecoveryExecutionRequestV2, RuntimeStartupRecoveryExecutionTerminalDigestV2,
};
use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::PgConnection;

use super::closed_evidence::RuntimeClosedRecoveryExpectedEvidenceV2;
use super::digest::lowercase_sha256_bytes;
use super::pending_semantic::{
    RuntimePendingDrainExpectationV2, RuntimePendingDrainSealExpectationV2,
};
use super::query::{
    EXECUTE_PENDING_DRAIN_STARTUP_RECOVERY_QUERY, RECORD_PENDING_DRAIN_NO_CANDIDATE_QUERY,
    SELECT_PENDING_DRAIN_STARTUP_RECOVERY_QUERY,
};
use super::row::{
    RuntimePendingDrainNoCandidateDatabaseReceiptV2,
    RuntimePendingDrainProgressedDatabaseReceiptV2, RuntimeStartupRecoveryExecutionExpectedV2,
    RuntimeStartupRecoveryExecutionRowV2,
};
use crate::connection::ExecutionConnectionGuardV1;
use crate::database::{
    begin_execution_mutation_transaction, begin_execution_serializable_observation_transaction,
    verify_runtime_execution_binding_v1,
};
use crate::error::{map_mutation_commit_error, map_query_error};
use crate::gateway_owner::MAX_RUNTIME_GATEWAY_OWNER_LEASE_DURATION;
use crate::{PostgresRuntimeExecutionV1, RuntimeExecutionPersistenceErrorV1};

const PENDING_RECOVERY_CLASS: &str = "pending_runtime_drain_intent";

#[derive(Clone, Debug, sqlx::FromRow)]
struct RuntimePendingDrainSelectionRowV2 {
    selection_outcome_name: String,
    observed_database_now: DateTime<Utc>,
    observed_owner_expires_at: DateTime<Utc>,
    selected_drain_intent_id: Option<String>,
    selected_source_intent_revision: Option<i64>,
    selected_source_state_digest: Option<String>,
    selected_slot_guild_id: Option<String>,
    selected_slot_ruleset_key: Option<String>,
    selected_target_version: Option<i64>,
    selected_target_content_hash: Option<String>,
    selected_target_binding_revision: Option<i64>,
    selected_target_binding_fingerprint: Option<String>,
}

struct RuntimePendingDrainSelectedCandidateFieldsV2 {
    intent_id: String,
    source_revision: i64,
    source_digest: String,
    guild_id: String,
    ruleset_key: String,
    version: i64,
    content_hash: String,
    binding_revision: i64,
    binding_fingerprint: String,
}

impl RuntimePendingDrainSelectionRowV2 {
    fn decode(
        self,
        authorization: &RuntimeAuthorizedPendingDrainSelectionV2,
    ) -> Result<RuntimePendingDrainSelectionReceiptV2, RuntimeExecutionPersistenceErrorV1> {
        let request = authorization.request();
        let owner_receipt = pending_owner_receipt_v2(
            request,
            self.observed_database_now,
            self.observed_owner_expires_at,
            request.minimum_database_now(),
        )?;
        let outcome = self.decode_outcome()?;
        Ok(RuntimePendingDrainSelectionReceiptV2::new(
            request.correlation().clone(),
            owner_receipt,
            outcome,
        ))
    }

    fn decode_outcome(
        &self,
    ) -> Result<RuntimePendingDrainSelectionOutcomeV2, RuntimeExecutionPersistenceErrorV1> {
        let selected = (
            self.selected_drain_intent_id.as_ref(),
            self.selected_source_intent_revision,
            self.selected_source_state_digest.as_ref(),
            self.selected_slot_guild_id.as_ref(),
            self.selected_slot_ruleset_key.as_ref(),
            self.selected_target_version,
            self.selected_target_content_hash.as_ref(),
            self.selected_target_binding_revision,
            self.selected_target_binding_fingerprint.as_ref(),
        );
        let outcome = match (self.selection_outcome_name.as_str(), selected) {
            (
                "candidate",
                (
                    Some(intent_id),
                    Some(source_revision),
                    Some(source_digest),
                    Some(guild_id),
                    Some(ruleset_key),
                    Some(version),
                    Some(content_hash),
                    Some(binding_revision),
                    Some(binding_fingerprint),
                ),
            ) => RuntimePendingDrainSelectionOutcomeV2::Candidate(decode_pending_candidate_v2(
                RuntimePendingDrainSelectedCandidateFieldsV2 {
                    intent_id: intent_id.clone(),
                    source_revision,
                    source_digest: source_digest.clone(),
                    guild_id: guild_id.clone(),
                    ruleset_key: ruleset_key.clone(),
                    version,
                    content_hash: content_hash.clone(),
                    binding_revision,
                    binding_fingerprint: binding_fingerprint.clone(),
                },
            )?),
            ("no_candidate", (None, None, None, None, None, None, None, None, None)) => {
                RuntimePendingDrainSelectionOutcomeV2::NoCandidate
            }
            _ => return Err(invalid()),
        };
        Ok(outcome)
    }
}

impl PostgresRuntimeExecutionV1 {
    async fn select_pending_drain_v2(
        &self,
        authorization: &RuntimeAuthorizedPendingDrainSelectionV2,
        operation_cutoff: Instant,
    ) -> Result<RuntimePendingDrainSelectionReceiptV2, RuntimeExecutionPersistenceErrorV1> {
        let bindings = RuntimePendingDrainCommonBindingsV2::from_request(authorization.request())?;
        validate_acknowledgement_identity_v2(
            authorization.request(),
            authorization.acknowledgement_action_identity(),
        )?;
        let effective_cutoff = self.pending_effective_cutoff_v2(operation_cutoff)?;
        let deadline = tokio::time::Instant::from_std(effective_cutoff);
        let mut connection = self
            .acquire_pending_connection_v2(deadline, effective_cutoff)
            .await?;
        let database_connection = connection
            .connection_mut()
            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        let result = tokio::time::timeout_at(
            deadline,
            self.select_pending_drain_on_connection_v2(
                database_connection,
                authorization,
                &bindings,
            ),
        )
        .await;
        match result {
            Ok(Ok(receipt)) if Instant::now() < effective_cutoff => {
                connection.release_to_pool();
                Ok(receipt)
            }
            Ok(Ok(_)) => Err(RuntimeExecutionPersistenceErrorV1::Timeout),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(RuntimeExecutionPersistenceErrorV1::Timeout),
        }
    }

    async fn select_pending_drain_on_connection_v2(
        &self,
        connection: &mut PgConnection,
        authorization: &RuntimeAuthorizedPendingDrainSelectionV2,
        bindings: &RuntimePendingDrainCommonBindingsV2,
    ) -> Result<RuntimePendingDrainSelectionReceiptV2, RuntimeExecutionPersistenceErrorV1> {
        let mut transaction =
            begin_execution_serializable_observation_transaction(connection, self.timeouts).await?;
        verify_runtime_execution_binding_v1(&mut transaction, &self.expectation).await?;
        let request = authorization.request();
        let owner = request.gateway_owner_lease_id();
        let mut rows = sqlx::query_as::<_, RuntimePendingDrainSelectionRowV2>(
            SELECT_PENDING_DRAIN_STARTUP_RECOVERY_QUERY,
        )
        .bind(owner.gateway_shard_id.as_str())
        .bind(owner.process_instance_id.as_str())
        .bind(bindings.owner_lease_epoch)
        .bind(owner.expected_build_revision.as_str())
        .bind(bindings.owner_revision)
        .bind(request.expected_owner_expires_at())
        .bind(request.minimum_database_now())
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_query_error)?;
        if rows.len() != 1 {
            return Err(invalid());
        }
        let receipt = rows.pop().ok_or_else(invalid)?.decode(authorization)?;
        transaction.commit().await.map_err(map_query_error)?;
        Ok(receipt)
    }

    async fn record_pending_drain_no_candidate_v2(
        &self,
        selection: &RuntimeSelectedPendingDrainNoCandidateV2,
        operation_cutoff: Instant,
    ) -> Result<RuntimePendingDrainNoCandidateReceiptV2, RuntimeExecutionPersistenceErrorV1> {
        let request = selection.request();
        let bindings = RuntimePendingDrainCommonBindingsV2::from_request(request)?;
        let minimum_database_now = selection
            .selection_owner_receipt()
            .database_now
            .max(request.minimum_database_now());
        let effective_cutoff = self.pending_effective_cutoff_v2(operation_cutoff)?;
        let deadline = tokio::time::Instant::from_std(effective_cutoff);
        let mut connection = self
            .acquire_pending_connection_v2(deadline, effective_cutoff)
            .await?;
        let database_connection = connection
            .connection_mut()
            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        let mutation_dispatched = AtomicBool::new(false);
        let result = tokio::time::timeout_at(
            deadline,
            self.record_pending_drain_no_candidate_on_connection_v2(
                database_connection,
                request,
                &bindings,
                minimum_database_now,
                &mutation_dispatched,
            ),
        )
        .await;
        let database_receipt = match result {
            Ok(Ok(receipt)) if Instant::now() < effective_cutoff => {
                connection.release_to_pool();
                receipt
            }
            Ok(Ok(_)) => return Err(client_cutoff_error(true)),
            Ok(Err(error)) => return Err(error),
            Err(_) => {
                return Err(client_cutoff_error(
                    mutation_dispatched.load(Ordering::Acquire),
                ));
            }
        };
        Ok(RuntimePendingDrainNoCandidateReceiptV2::new(
            request.action_identity().clone(),
            database_receipt.terminal_digest,
            database_receipt.owner_receipt,
        ))
    }

    async fn record_pending_drain_no_candidate_on_connection_v2(
        &self,
        connection: &mut PgConnection,
        request: &RuntimeStartupRecoveryExecutionRequestV2,
        bindings: &RuntimePendingDrainCommonBindingsV2,
        minimum_database_now: DateTime<Utc>,
        mutation_dispatched: &AtomicBool,
    ) -> Result<RuntimePendingDrainNoCandidateDatabaseReceiptV2, RuntimeExecutionPersistenceErrorV1>
    {
        let mut transaction =
            begin_execution_mutation_transaction(connection, self.timeouts).await?;
        verify_runtime_execution_binding_v1(&mut transaction, &self.expectation).await?;
        let owner = request.gateway_owner_lease_id();
        let evidence = &bindings.closed_evidence;
        mutation_dispatched.store(true, Ordering::Release);
        let mut rows = sqlx::query_as::<_, RuntimeStartupRecoveryExecutionRowV2>(
            RECORD_PENDING_DRAIN_NO_CANDIDATE_QUERY,
        )
        .bind(request.correlation().recovery_id().as_str())
        .bind(bindings.originating_emergency_generation)
        .bind(bindings.coordinator_generation)
        .bind(bindings.claim_action_authority_revision)
        .bind(bindings.claim_selection_authority_revision)
        .bind(owner.gateway_shard_id.as_str())
        .bind(owner.process_instance_id.as_str())
        .bind(bindings.owner_lease_epoch)
        .bind(owner.expected_build_revision.as_str())
        .bind(bindings.owner_revision)
        .bind(request.expected_owner_expires_at())
        .bind(minimum_database_now)
        .bind(evidence.paused_process_instance_id.as_str())
        .bind(evidence.paused_coordinator_generation)
        .bind(evidence.paused_connection_epoch)
        .bind(evidence.paused_ready_kind)
        .bind(evidence.paused_admission_revision)
        .bind(evidence.paused_transition_sequence)
        .bind(evidence.paused_connected_event_sequence)
        .bind(evidence.paused_last_resume_sequence.unwrap_or(0))
        .bind(evidence.registry_process_instance_id.as_str())
        .bind(evidence.registry_observation_sequence)
        .bind(evidence.registry_retained_slot_count)
        .bind(evidence.registry_retained_empty_tombstone_count)
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_pending_mutation_dispatch_error)?;
        if rows.len() != 1 {
            return Err(invalid());
        }
        let expected = bindings.expected_action(
            request,
            bindings.claim_action_authority_revision,
            bindings.claim_selection_authority_revision,
            minimum_database_now,
        );
        let receipt = rows
            .pop()
            .ok_or_else(invalid)?
            .decode_pending_no_candidate(&expected, evidence)?;
        transaction
            .commit()
            .await
            .map_err(map_mutation_commit_error)?;
        Ok(receipt)
    }

    async fn execute_pending_drain_progressed_v2(
        &self,
        execution: RuntimePendingDrainProgressedExecutionV2<'_>,
        operation_cutoff: Instant,
    ) -> Result<RuntimePendingDrainProgressedDatabaseReceiptV2, RuntimeExecutionPersistenceErrorV1>
    {
        let bindings = RuntimePendingDrainCommonBindingsV2::from_request(execution.request())?;
        execution.validate_action_identity(&bindings)?;
        let seal = RuntimePendingDrainSealBindingsV2::from_witness(execution.seal())?;
        let effective_cutoff = self.pending_effective_cutoff_v2(operation_cutoff)?;
        let deadline = tokio::time::Instant::from_std(effective_cutoff);
        let mut connection = self
            .acquire_pending_connection_v2(deadline, effective_cutoff)
            .await?;
        let database_connection = connection
            .connection_mut()
            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        let mutation_dispatched = AtomicBool::new(false);
        let result = tokio::time::timeout_at(
            deadline,
            self.execute_pending_drain_progressed_on_connection_v2(
                database_connection,
                execution,
                &bindings,
                &seal,
                &mutation_dispatched,
            ),
        )
        .await;
        match result {
            Ok(Ok(receipt)) if Instant::now() < effective_cutoff => {
                connection.release_to_pool();
                Ok(receipt)
            }
            Ok(Ok(_)) => Err(client_cutoff_error(true)),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(client_cutoff_error(
                mutation_dispatched.load(Ordering::Acquire),
            )),
        }
    }

    async fn execute_pending_drain_progressed_on_connection_v2(
        &self,
        connection: &mut PgConnection,
        execution: RuntimePendingDrainProgressedExecutionV2<'_>,
        bindings: &RuntimePendingDrainCommonBindingsV2,
        seal: &RuntimePendingDrainSealBindingsV2,
        mutation_dispatched: &AtomicBool,
    ) -> Result<RuntimePendingDrainProgressedDatabaseReceiptV2, RuntimeExecutionPersistenceErrorV1>
    {
        let mut transaction =
            begin_execution_mutation_transaction(connection, self.timeouts).await?;
        verify_runtime_execution_binding_v1(&mut transaction, &self.expectation).await?;
        let request = execution.request();
        let owner = request.gateway_owner_lease_id();
        let evidence = &bindings.closed_evidence;
        let candidate = execution.candidate();
        let prior_claim_terminal_digest = execution.prior_claim_terminal_digest();
        mutation_dispatched.store(true, Ordering::Release);
        let mut rows = sqlx::query_as::<_, RuntimeStartupRecoveryExecutionRowV2>(
            EXECUTE_PENDING_DRAIN_STARTUP_RECOVERY_QUERY,
        )
        .bind(request.correlation().recovery_id().as_str())
        .bind(bindings.originating_emergency_generation)
        .bind(bindings.coordinator_generation)
        .bind(bindings.claim_action_authority_revision)
        .bind(bindings.claim_selection_authority_revision)
        .bind(bindings.acknowledgement_action_authority_revision)
        .bind(bindings.acknowledgement_selection_authority_revision)
        .bind(execution.stage_name())
        .bind(owner.gateway_shard_id.as_str())
        .bind(owner.process_instance_id.as_str())
        .bind(bindings.owner_lease_epoch)
        .bind(owner.expected_build_revision.as_str())
        .bind(bindings.owner_revision)
        .bind(request.expected_owner_expires_at())
        .bind(execution.minimum_database_now())
        .bind(evidence.paused_process_instance_id.as_str())
        .bind(evidence.paused_coordinator_generation)
        .bind(evidence.paused_connection_epoch)
        .bind(evidence.paused_ready_kind)
        .bind(evidence.paused_admission_revision)
        .bind(evidence.paused_transition_sequence)
        .bind(evidence.paused_connected_event_sequence)
        .bind(evidence.paused_last_resume_sequence.unwrap_or(0))
        .bind(evidence.registry_process_instance_id.as_str())
        .bind(evidence.registry_observation_sequence)
        .bind(evidence.registry_retained_slot_count)
        .bind(evidence.registry_retained_empty_tombstone_count)
        .bind(candidate.intent_id().as_str())
        .bind(positive_i64(candidate.source_intent_revision().get())?)
        .bind(lowercase_hex(candidate.source_state_digest().as_bytes()))
        .bind(seal.pre_slot_present)
        .bind(seal.pre_slot_admission_generation)
        .bind(seal.pre_slot_observation_sequence)
        .bind(seal.seal_key.as_slice())
        .bind(seal.seal_generation)
        .bind(seal.post_slot_admission_generation)
        .bind(seal.post_slot_observation_sequence)
        .bind(seal.post_global_observation_sequence)
        .bind(seal.post_retained_slot_count)
        .bind(seal.post_retained_empty_tombstone_count)
        .bind(seal.post_staged_route_count)
        .bind(seal.post_serving_route_count)
        .bind(seal.post_draining_route_count)
        .bind(seal.post_sealed_slot_count)
        .bind(seal.post_active_interaction_count)
        .bind(seal.post_failed_closed_slot_count)
        .bind(seal.post_registry_failed_closed)
        .bind(prior_claim_terminal_digest.as_str())
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_pending_mutation_dispatch_error)?;
        if rows.len() != 1 {
            return Err(invalid());
        }
        let expected = bindings.expected_action(
            request,
            execution.action_authority_revision(),
            execution.selection_authority_revision(),
            execution.minimum_database_now(),
        );
        let semantic = RuntimePendingDrainExpectationV2 {
            recovery_id: request.correlation().recovery_id().as_str(),
            originating_emergency_generation: bindings.originating_emergency_generation,
            coordinator_generation: bindings.coordinator_generation,
            action_authority_revision: execution.action_authority_revision(),
            selection_authority_revision: execution.selection_authority_revision(),
            claim_action_authority_revision: bindings.claim_action_authority_revision,
            gateway_owner_lease_id: owner,
            owner_revision: bindings.owner_revision,
            owner_expires_at: request.expected_owner_expires_at(),
            candidate: candidate.clone(),
            source_intent_revision: execution.source_intent_revision(),
            source_state_digest: execution.source_state_digest().clone(),
            prior_claim_terminal_digest: (!prior_claim_terminal_digest.is_empty())
                .then_some(prior_claim_terminal_digest.as_str()),
            seal: seal.semantic_expectation(),
            evidence,
        };
        let row = rows.pop().ok_or_else(invalid)?;
        let receipt = match execution {
            RuntimePendingDrainProgressedExecutionV2::Claim(_) => {
                row.decode_pending_claimed(&expected, &semantic)?
            }
            RuntimePendingDrainProgressedExecutionV2::Acknowledgement(_) => {
                row.decode_pending_acknowledged(&expected, &semantic)?
            }
        };
        transaction
            .commit()
            .await
            .map_err(map_mutation_commit_error)?;
        Ok(receipt)
    }

    pub(super) fn pending_effective_cutoff_v2(
        &self,
        operation_cutoff: Instant,
    ) -> Result<Instant, RuntimeExecutionPersistenceErrorV1> {
        if Instant::now() >= operation_cutoff {
            return Err(RuntimeExecutionPersistenceErrorV1::Timeout);
        }
        let statement_cutoff = Instant::now()
            .checked_add(self.timeouts.statement_timeout())
            .ok_or(RuntimeExecutionPersistenceErrorV1::InvalidInput)?;
        Ok(operation_cutoff.min(statement_cutoff))
    }

    pub(super) async fn acquire_pending_connection_v2(
        &self,
        deadline: tokio::time::Instant,
        effective_cutoff: Instant,
    ) -> Result<ExecutionConnectionGuardV1, RuntimeExecutionPersistenceErrorV1> {
        let connection = match tokio::time::timeout_at(deadline, self.pool.acquire()).await {
            Ok(Ok(connection)) => connection,
            Ok(Err(error)) => {
                if Instant::now() >= effective_cutoff {
                    return Err(RuntimeExecutionPersistenceErrorV1::Timeout);
                }
                return Err(map_query_error(error));
            }
            Err(_) => return Err(RuntimeExecutionPersistenceErrorV1::Timeout),
        };
        if Instant::now() >= effective_cutoff {
            drop(connection);
            return Err(RuntimeExecutionPersistenceErrorV1::Timeout);
        }
        Ok(ExecutionConnectionGuardV1::new(connection))
    }
}

impl RuntimePendingDrainSelectionPortV2 for PostgresRuntimeExecutionV1 {
    type Error = RuntimeExecutionPersistenceErrorV1;

    fn select_pending_drain(
        &self,
        authorization: &RuntimeAuthorizedPendingDrainSelectionV2,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimePendingDrainSelectionReceiptV2, Self::Error>> + Send
    {
        self.select_pending_drain_v2(authorization, operation_cutoff)
    }
}

impl RuntimePendingDrainNoCandidateRecorderPortV2 for PostgresRuntimeExecutionV1 {
    type Error = RuntimeExecutionPersistenceErrorV1;

    fn record_pending_drain_no_candidate(
        &self,
        selection: &RuntimeSelectedPendingDrainNoCandidateV2,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimePendingDrainNoCandidateReceiptV2, Self::Error>> + Send
    {
        self.record_pending_drain_no_candidate_v2(selection, operation_cutoff)
    }
}

impl RuntimePendingDrainClaimExecutionPortV2 for PostgresRuntimeExecutionV1 {
    type Error = RuntimeExecutionPersistenceErrorV1;

    async fn execute_pending_drain_claim(
        &self,
        authorization: &RuntimeAuthorizedPendingDrainClaimV2,
        operation_cutoff: Instant,
    ) -> Result<RuntimePendingDrainClaimReceiptV2, Self::Error> {
        let receipt = self
            .execute_pending_drain_progressed_v2(
                RuntimePendingDrainProgressedExecutionV2::Claim(authorization),
                operation_cutoff,
            )
            .await?;
        Ok(RuntimePendingDrainClaimReceiptV2::new(
            authorization.action_identity().clone(),
            authorization.candidate().clone(),
            authorization.seal().clone(),
            receipt.successor_intent_revision,
            receipt.successor_state_digest,
            receipt.terminal_digest,
            receipt.owner_receipt,
        ))
    }
}

impl RuntimePendingDrainAcknowledgementExecutionPortV2 for PostgresRuntimeExecutionV1 {
    type Error = RuntimeExecutionPersistenceErrorV1;

    async fn execute_pending_drain_acknowledgement(
        &self,
        authorization: &RuntimeAuthorizedPendingDrainAcknowledgementV2,
        operation_cutoff: Instant,
    ) -> Result<RuntimePendingDrainAcknowledgementReceiptV2, Self::Error> {
        let receipt = self
            .execute_pending_drain_progressed_v2(
                RuntimePendingDrainProgressedExecutionV2::Acknowledgement(authorization),
                operation_cutoff,
            )
            .await?;
        Ok(RuntimePendingDrainAcknowledgementReceiptV2::new(
            authorization.action_identity().clone(),
            authorization.request().action_identity().clone(),
            authorization.candidate().clone(),
            authorization.seal().clone(),
            authorization.claimed_intent_revision(),
            authorization.claimed_state_digest().clone(),
            duplicate_terminal_digest_v2(authorization.claim_terminal_digest()),
            receipt.successor_intent_revision,
            receipt.successor_state_digest,
            receipt.terminal_digest,
            receipt.owner_receipt,
        ))
    }
}

struct RuntimePendingDrainCommonBindingsV2 {
    originating_emergency_generation: i64,
    coordinator_generation: i64,
    claim_action_authority_revision: i64,
    claim_selection_authority_revision: i64,
    acknowledgement_action_authority_revision: i64,
    acknowledgement_selection_authority_revision: i64,
    owner_lease_epoch: i64,
    owner_revision: i64,
    closed_evidence: RuntimeClosedRecoveryExpectedEvidenceV2,
}

impl RuntimePendingDrainCommonBindingsV2 {
    fn from_request(
        request: &RuntimeStartupRecoveryExecutionRequestV2,
    ) -> Result<Self, RuntimeExecutionPersistenceErrorV1> {
        if request.class() != RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent {
            return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
        }
        let correlation = request.correlation();
        let claim_action_authority_revision = positive_i64(correlation.authority_revision().get())?;
        let claim_selection_authority_revision =
            positive_i64(correlation.selection_authority_revision().get())?;
        if claim_selection_authority_revision.checked_add(1)
            != Some(claim_action_authority_revision)
        {
            return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
        }
        let acknowledgement_action_authority_revision = claim_action_authority_revision
            .checked_add(1)
            .ok_or(RuntimeExecutionPersistenceErrorV1::InvalidInput)?;
        Ok(Self {
            originating_emergency_generation: positive_i64(
                correlation.originating_emergency_generation().get(),
            )?,
            coordinator_generation: positive_i64(correlation.coordinator_generation().get())?,
            claim_action_authority_revision,
            claim_selection_authority_revision,
            acknowledgement_action_authority_revision,
            acknowledgement_selection_authority_revision: claim_action_authority_revision,
            owner_lease_epoch: positive_i64(request.gateway_owner_lease_id().lease_epoch.get())?,
            owner_revision: positive_i64(request.expected_owner_revision().get())?,
            closed_evidence: pending_closed_recovery_evidence_v2(request)?,
        })
    }

    fn expected_action(
        &self,
        request: &RuntimeStartupRecoveryExecutionRequestV2,
        action_authority_revision: i64,
        selection_authority_revision: i64,
        minimum_database_now: DateTime<Utc>,
    ) -> RuntimeStartupRecoveryExecutionExpectedV2 {
        RuntimeStartupRecoveryExecutionExpectedV2 {
            recovery_id: request.correlation().recovery_id().as_str().to_owned(),
            originating_emergency_generation: self.originating_emergency_generation,
            coordinator_generation: self.coordinator_generation,
            action_authority_revision,
            selection_authority_revision,
            recovery_class: PENDING_RECOVERY_CLASS,
            gateway_owner_lease_id: request.gateway_owner_lease_id().clone(),
            owner_revision: self.owner_revision,
            owner_expires_at: request.expected_owner_expires_at(),
            minimum_database_now,
            closed_evidence: Some(self.closed_evidence.clone()),
        }
    }
}

pub(super) struct RuntimePendingDrainSealBindingsV2 {
    pub(super) pre_slot_present: bool,
    pub(super) pre_slot_admission_generation: i64,
    pub(super) pre_slot_observation_sequence: i64,
    pub(super) seal_key: [u8; 16],
    pub(super) seal_generation: i64,
    pub(super) post_slot_admission_generation: i64,
    pub(super) post_slot_observation_sequence: i64,
    pub(super) post_global_observation_sequence: i64,
    pub(super) post_retained_slot_count: i64,
    pub(super) post_retained_empty_tombstone_count: i64,
    pub(super) post_staged_route_count: i64,
    pub(super) post_serving_route_count: i64,
    pub(super) post_draining_route_count: i64,
    pub(super) post_sealed_slot_count: i64,
    pub(super) post_active_interaction_count: i64,
    pub(super) post_failed_closed_slot_count: i64,
    pub(super) post_registry_failed_closed: bool,
}

impl RuntimePendingDrainSealBindingsV2 {
    pub(super) fn from_witness(
        witness: &RuntimePendingDrainRegistrySealWitnessV2,
    ) -> Result<Self, RuntimeExecutionPersistenceErrorV1> {
        let pre = witness.pre_slot_observation();
        let post = witness.post_registry_observation();
        Ok(Self {
            pre_slot_present: pre.is_some(),
            pre_slot_admission_generation: pre
                .map(|value| positive_i64(value.admission_generation.get()))
                .transpose()?
                .unwrap_or(0),
            pre_slot_observation_sequence: pre
                .map(|value| positive_i64(value.observation_sequence.get()))
                .transpose()?
                .unwrap_or(0),
            seal_key: *witness.seal_key(),
            seal_generation: positive_i64(witness.seal_generation().get())?,
            post_slot_admission_generation: positive_i64(
                witness.post_slot_admission_generation().get(),
            )?,
            post_slot_observation_sequence: positive_i64(
                witness.post_slot_observation_sequence().get(),
            )?,
            post_global_observation_sequence: positive_i64(post.observation_sequence.get())?,
            post_retained_slot_count: nonnegative_i64(post.retained_slot_count)?,
            post_retained_empty_tombstone_count: nonnegative_i64(
                post.retained_empty_tombstone_count,
            )?,
            post_staged_route_count: nonnegative_i64(post.staged_route_count)?,
            post_serving_route_count: nonnegative_i64(post.serving_route_count)?,
            post_draining_route_count: nonnegative_i64(post.draining_route_count)?,
            post_sealed_slot_count: nonnegative_i64(post.sealed_slot_count)?,
            post_active_interaction_count: nonnegative_i64(post.active_interaction_count)?,
            post_failed_closed_slot_count: nonnegative_i64(post.failed_closed_slot_count)?,
            post_registry_failed_closed: post.registry_failed_closed,
        })
    }

    fn semantic_expectation(&self) -> RuntimePendingDrainSealExpectationV2 {
        RuntimePendingDrainSealExpectationV2 {
            pre_slot_admission_generation: self
                .pre_slot_present
                .then_some(self.pre_slot_admission_generation),
            pre_slot_observation_sequence: self
                .pre_slot_present
                .then_some(self.pre_slot_observation_sequence),
            seal_generation: self.seal_generation,
            post_admission_generation: self.post_slot_admission_generation,
            post_slot_observation_sequence: self.post_slot_observation_sequence,
            post_global_sequence: self.post_global_observation_sequence,
            post_retained_slots: self.post_retained_slot_count,
            post_retained_empty: self.post_retained_empty_tombstone_count,
            post_staged: self.post_staged_route_count,
            post_serving: self.post_serving_route_count,
            post_draining: self.post_draining_route_count,
            post_sealed: self.post_sealed_slot_count,
            post_active: self.post_active_interaction_count,
            post_failed_closed_slots: self.post_failed_closed_slot_count,
        }
    }
}

#[derive(Clone, Copy)]
enum RuntimePendingDrainProgressedExecutionV2<'a> {
    Claim(&'a RuntimeAuthorizedPendingDrainClaimV2),
    Acknowledgement(&'a RuntimeAuthorizedPendingDrainAcknowledgementV2),
}

impl<'a> RuntimePendingDrainProgressedExecutionV2<'a> {
    fn request(self) -> &'a RuntimeStartupRecoveryExecutionRequestV2 {
        match self {
            Self::Claim(authorization) => authorization.request(),
            Self::Acknowledgement(authorization) => authorization.request(),
        }
    }

    fn candidate(self) -> &'a RuntimePendingDrainCandidateV2 {
        match self {
            Self::Claim(authorization) => authorization.candidate(),
            Self::Acknowledgement(authorization) => authorization.candidate(),
        }
    }

    fn seal(self) -> &'a RuntimePendingDrainRegistrySealWitnessV2 {
        match self {
            Self::Claim(authorization) => authorization.seal(),
            Self::Acknowledgement(authorization) => authorization.seal(),
        }
    }

    fn minimum_database_now(self) -> DateTime<Utc> {
        match self {
            Self::Claim(authorization) => authorization.minimum_database_now(),
            Self::Acknowledgement(authorization) => authorization.minimum_database_now(),
        }
    }

    fn source_intent_revision(self) -> NonZeroU64 {
        match self {
            Self::Claim(authorization) => authorization.candidate().source_intent_revision(),
            Self::Acknowledgement(authorization) => authorization.claimed_intent_revision(),
        }
    }

    fn source_state_digest(self) -> &'a RuntimePendingDrainStateDigestV2 {
        match self {
            Self::Claim(authorization) => authorization.candidate().source_state_digest(),
            Self::Acknowledgement(authorization) => authorization.claimed_state_digest(),
        }
    }

    fn prior_claim_terminal_digest(self) -> String {
        match self {
            Self::Claim(_) => String::new(),
            Self::Acknowledgement(authorization) => {
                lowercase_hex(authorization.claim_terminal_digest().as_bytes())
            }
        }
    }

    fn stage_name(self) -> &'static str {
        match self {
            Self::Claim(_) => "claim",
            Self::Acknowledgement(_) => "acknowledge",
        }
    }

    fn action_authority_revision(self) -> i64 {
        let correlation = match self {
            Self::Claim(authorization) => authorization.action_identity().correlation(),
            Self::Acknowledgement(authorization) => authorization.action_identity().correlation(),
        };
        i64::try_from(correlation.authority_revision().get())
            .expect("validated pending action authority revision fits PostgreSQL")
    }

    fn selection_authority_revision(self) -> i64 {
        let correlation = match self {
            Self::Claim(authorization) => authorization.action_identity().correlation(),
            Self::Acknowledgement(authorization) => authorization.action_identity().correlation(),
        };
        i64::try_from(correlation.selection_authority_revision().get())
            .expect("validated pending selection authority revision fits PostgreSQL")
    }

    fn validate_action_identity(
        self,
        bindings: &RuntimePendingDrainCommonBindingsV2,
    ) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
        let (action, selection) = match self {
            Self::Claim(_) => (
                bindings.claim_action_authority_revision,
                bindings.claim_selection_authority_revision,
            ),
            Self::Acknowledgement(authorization) => {
                validate_acknowledgement_identity_v2(
                    authorization.request(),
                    authorization.action_identity(),
                )?;
                (
                    bindings.acknowledgement_action_authority_revision,
                    bindings.acknowledgement_selection_authority_revision,
                )
            }
        };
        if self.action_authority_revision() != action
            || self.selection_authority_revision() != selection
        {
            Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
        } else {
            Ok(())
        }
    }
}

fn decode_pending_candidate_v2(
    fields: RuntimePendingDrainSelectedCandidateFieldsV2,
) -> Result<RuntimePendingDrainCandidateV2, RuntimeExecutionPersistenceErrorV1> {
    let target = serde_json::from_value::<RuntimeDeploymentTargetV1>(json!({
        "guild_id": fields.guild_id,
        "ruleset_key": fields.ruleset_key,
        "version": fields.version,
        "content_hash": fields.content_hash,
        "binding_revision": fields.binding_revision,
        "binding_fingerprint": fields.binding_fingerprint
    }))
    .map_err(|_| invalid())?;
    RuntimePendingDrainCandidateV2::new(
        RuntimeDrainIntentIdV2::parse(fields.intent_id).map_err(|_| invalid())?,
        RuntimeServingSlotV2::from_target(&target),
        target,
        positive_non_zero(fields.source_revision)?,
        RuntimePendingDrainStateDigestV2::new(lowercase_sha256_bytes(&fields.source_digest)?)
            .map_err(|_| invalid())?,
    )
    .map_err(|_| invalid())
}

pub(super) fn pending_owner_receipt_v2(
    request: &RuntimeStartupRecoveryExecutionRequestV2,
    database_now: DateTime<Utc>,
    owner_expires_at: DateTime<Utc>,
    minimum_database_now: DateTime<Utc>,
) -> Result<RuntimeGatewayOwnerLeaseReceiptV1, RuntimeExecutionPersistenceErrorV1> {
    if owner_expires_at != request.expected_owner_expires_at()
        || database_now < minimum_database_now
        || database_now >= owner_expires_at
    {
        return Err(invalid());
    }
    let duration = owner_expires_at
        .signed_duration_since(database_now)
        .to_std()
        .map_err(|_| invalid())?;
    if duration.is_zero() || duration > MAX_RUNTIME_GATEWAY_OWNER_LEASE_DURATION {
        return Err(invalid());
    }
    Ok(RuntimeGatewayOwnerLeaseReceiptV1 {
        lease_id: request.gateway_owner_lease_id().clone(),
        owner_revision: request.expected_owner_revision(),
        database_now,
        expires_at: owner_expires_at,
    })
}

pub(super) fn pending_closed_recovery_evidence_v2(
    request: &RuntimeStartupRecoveryExecutionRequestV2,
) -> Result<RuntimeClosedRecoveryExpectedEvidenceV2, RuntimeExecutionPersistenceErrorV1> {
    let paused = request.paused_gateway();
    let owner_process = request
        .gateway_owner_lease_id()
        .process_instance_id
        .as_str();
    if paused.process_instance_id().as_str() != owner_process
        || paused.coordinator_generation().get()
            != request
                .correlation()
                .originating_emergency_generation()
                .get()
        || request.registry_process_instance_id().as_str() != owner_process
        || request.registry_retained_slot_count()
            != request.registry_retained_empty_tombstone_count()
    {
        return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
    }
    let paused_ready_kind = match paused.kind() {
        automation_runtime_controller::RuntimeGatewayReadyKindV2::Ready => "ready",
        automation_runtime_controller::RuntimeGatewayReadyKindV2::Resumed => "resumed",
    };
    Ok(RuntimeClosedRecoveryExpectedEvidenceV2 {
        paused_process_instance_id: paused.process_instance_id().as_str().to_owned(),
        paused_coordinator_generation: positive_i64(paused.coordinator_generation().get())?,
        paused_connection_epoch: positive_i64(paused.connection_epoch().get())?,
        paused_ready_kind,
        paused_admission_revision: positive_i64(paused.admission_revision().get())?,
        paused_transition_sequence: positive_i64(paused.transition_sequence().get())?,
        paused_connected_event_sequence: positive_i64(paused.connected_event_sequence().get())?,
        paused_last_resume_sequence: paused
            .last_resume_sequence()
            .map(|sequence| positive_i64(sequence.get()))
            .transpose()?,
        registry_process_instance_id: request.registry_process_instance_id().as_str().to_owned(),
        registry_observation_sequence: positive_i64(request.registry_observation_sequence().get())?,
        registry_retained_slot_count: nonnegative_i64(request.registry_retained_slot_count())?,
        registry_retained_empty_tombstone_count: nonnegative_i64(
            request.registry_retained_empty_tombstone_count(),
        )?,
    })
}

fn validate_acknowledgement_identity_v2(
    request: &RuntimeStartupRecoveryExecutionRequestV2,
    acknowledgement: &RuntimeStartupRecoveryExecutionActionIdentityV2,
) -> Result<(), RuntimeExecutionPersistenceErrorV1> {
    let claim = request.action_identity();
    let claim_correlation = claim.correlation();
    let acknowledgement_correlation = acknowledgement.correlation();
    if acknowledgement.class() != RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent
        || acknowledgement_correlation.recovery_id() != claim_correlation.recovery_id()
        || acknowledgement_correlation.originating_emergency_generation()
            != claim_correlation.originating_emergency_generation()
        || acknowledgement_correlation.coordinator_generation()
            != claim_correlation.coordinator_generation()
        || acknowledgement_correlation.selection_authority_revision()
            != claim_correlation.authority_revision()
        || claim_correlation.authority_revision().get().checked_add(1)
            != Some(acknowledgement_correlation.authority_revision().get())
    {
        Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
    } else {
        Ok(())
    }
}

fn duplicate_terminal_digest_v2(
    digest: &RuntimeStartupRecoveryExecutionTerminalDigestV2,
) -> RuntimeStartupRecoveryExecutionTerminalDigestV2 {
    RuntimeStartupRecoveryExecutionTerminalDigestV2::new(*digest.as_bytes())
        .expect("accepted pending terminal digest is nonzero")
}

pub(super) fn positive_i64(value: u64) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    i64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(RuntimeExecutionPersistenceErrorV1::InvalidInput)
}

fn nonnegative_i64(value: u64) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    i64::try_from(value).map_err(|_| RuntimeExecutionPersistenceErrorV1::InvalidInput)
}

pub(super) fn positive_non_zero(
    value: i64,
) -> Result<NonZeroU64, RuntimeExecutionPersistenceErrorV1> {
    u64::try_from(value)
        .ok()
        .and_then(NonZeroU64::new)
        .ok_or_else(invalid)
}

pub(super) fn lowercase_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn client_cutoff_error(mutation_dispatched: bool) -> RuntimeExecutionPersistenceErrorV1 {
    if mutation_dispatched {
        RuntimeExecutionPersistenceErrorV1::Indeterminate
    } else {
        RuntimeExecutionPersistenceErrorV1::Timeout
    }
}

pub(super) fn map_pending_mutation_dispatch_error(
    error: sqlx::Error,
) -> RuntimeExecutionPersistenceErrorV1 {
    let mapped = map_query_error(error);
    if mapped == RuntimeExecutionPersistenceErrorV1::Unavailable {
        RuntimeExecutionPersistenceErrorV1::Indeterminate
    } else {
        mapped
    }
}

fn invalid() -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::error::Error;
    use std::fmt::{Display, Formatter};

    use sqlx::error::{DatabaseError, ErrorKind};

    use super::*;

    #[derive(Debug)]
    struct TestDatabaseError(&'static str);

    impl Display for TestDatabaseError {
        fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("private database detail")
        }
    }

    impl Error for TestDatabaseError {}

    impl DatabaseError for TestDatabaseError {
        fn message(&self) -> &str {
            "private database detail"
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed(self.0))
        }

        fn as_error(&self) -> &(dyn Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn Error + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> ErrorKind {
            ErrorKind::Other
        }
    }

    fn database_error(code: &'static str) -> sqlx::Error {
        sqlx::Error::Database(Box::new(TestDatabaseError(code)))
    }

    fn candidate_selection_row() -> RuntimePendingDrainSelectionRowV2 {
        RuntimePendingDrainSelectionRowV2 {
            selection_outcome_name: "candidate".to_owned(),
            observed_database_now: DateTime::from_timestamp(100, 0).unwrap(),
            observed_owner_expires_at: DateTime::from_timestamp(200, 0).unwrap(),
            selected_drain_intent_id: Some("00112233445566778899aabbccddeeff".to_owned()),
            selected_source_intent_revision: Some(3),
            selected_source_state_digest: Some("a".repeat(64)),
            selected_slot_guild_id: Some("42".to_owned()),
            selected_slot_ruleset_key: Some("studyroom".to_owned()),
            selected_target_version: Some(4),
            selected_target_content_hash: Some("b".repeat(64)),
            selected_target_binding_revision: Some(5),
            selected_target_binding_fingerprint: Some("c".repeat(64)),
        }
    }

    #[test]
    fn pending_binding_ranges_are_closed() {
        assert_eq!(positive_i64(1).unwrap(), 1);
        assert_eq!(nonnegative_i64(0).unwrap(), 0);
        assert!(positive_i64(0).is_err());
        assert!(positive_i64(u64::try_from(i64::MAX).unwrap() + 1).is_err());
        assert!(nonnegative_i64(u64::try_from(i64::MAX).unwrap() + 1).is_err());
    }

    #[test]
    fn pending_cutoff_changes_only_after_mutation_dispatch() {
        assert_eq!(
            client_cutoff_error(false),
            RuntimeExecutionPersistenceErrorV1::Timeout
        );
        assert_eq!(
            client_cutoff_error(true),
            RuntimeExecutionPersistenceErrorV1::Indeterminate
        );
    }

    #[test]
    fn pending_transport_failure_is_indeterminate_after_dispatch() {
        assert_eq!(
            map_pending_mutation_dispatch_error(database_error("08006")),
            RuntimeExecutionPersistenceErrorV1::Indeterminate
        );
        assert_eq!(
            map_pending_mutation_dispatch_error(database_error("57014")),
            RuntimeExecutionPersistenceErrorV1::Timeout
        );
        assert_eq!(
            map_pending_mutation_dispatch_error(database_error("RX001")),
            RuntimeExecutionPersistenceErrorV1::OwnershipLost
        );
    }

    #[test]
    fn selection_decodes_exact_candidate_and_rejects_partial_payloads() {
        let row = candidate_selection_row();
        let RuntimePendingDrainSelectionOutcomeV2::Candidate(candidate) =
            row.decode_outcome().unwrap()
        else {
            panic!("candidate expected");
        };
        assert_eq!(
            candidate.intent_id().as_str(),
            "00112233445566778899aabbccddeeff"
        );
        assert_eq!(candidate.source_intent_revision().get(), 3);
        assert_eq!(candidate.slot().guild_id.to_string(), "42");
        assert_eq!(candidate.slot().ruleset_key.as_str(), "studyroom");
        assert_eq!(u64::from(candidate.expected_target().version.get()), 4);
        assert_eq!(candidate.expected_target().binding_revision.get(), 5);

        let mut partial = candidate_selection_row();
        partial.selected_target_binding_fingerprint = None;
        assert!(partial.decode_outcome().is_err());
    }

    #[test]
    fn selection_rejects_noncanonical_digest_target_and_outcome_shapes() {
        let mut digest = candidate_selection_row();
        digest.selected_source_state_digest = Some("A".repeat(64));
        assert!(digest.decode_outcome().is_err());

        let mut target = candidate_selection_row();
        target.selected_target_version = Some(0);
        assert!(target.decode_outcome().is_err());

        let mut absent = candidate_selection_row();
        absent.selection_outcome_name = "no_candidate".to_owned();
        assert!(absent.decode_outcome().is_err());

        absent.selected_drain_intent_id = None;
        absent.selected_source_intent_revision = None;
        absent.selected_source_state_digest = None;
        absent.selected_slot_guild_id = None;
        absent.selected_slot_ruleset_key = None;
        absent.selected_target_version = None;
        absent.selected_target_content_hash = None;
        absent.selected_target_binding_revision = None;
        absent.selected_target_binding_fingerprint = None;
        assert!(matches!(
            absent.decode_outcome().unwrap(),
            RuntimePendingDrainSelectionOutcomeV2::NoCandidate
        ));
    }
}
