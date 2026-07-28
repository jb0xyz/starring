use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use automation_runtime_controller::{
    RuntimeCanonicalProductDrainV2, RuntimeDrainIntentDigestV2, RuntimePersistedProductDrainRootV2,
    RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2,
    RuntimePersistedUnclaimedPendingDrainIntentV2, RuntimeProductMutationDigestV2,
};
use automation_runtime_worker::{
    RuntimeAuthorizedPendingDrainSelectionV3,
    RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3, RuntimePendingDrainCandidateV2,
    RuntimePendingDrainPreviousOwnerClaimedCandidateInputV3,
    RuntimePendingDrainPreviousOwnerClaimedCandidateV3, RuntimePendingDrainSelectionOutcomeV3,
    RuntimePendingDrainSelectionPortV3, RuntimePendingDrainSelectionReceiptV3,
    RuntimePendingDrainStateDigestV2, RuntimePendingDrainSuccessionAcknowledgementExecutionPortV3,
    RuntimePendingDrainSuccessionAcknowledgementReceiptV3, RuntimeStartupRecoveryClassV2,
    RuntimeStartupRecoveryExecutionRequestV2, RuntimeStartupRecoveryExecutionTerminalDigestV2,
};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgConnection;

use super::digest::lowercase_sha256_bytes;
use super::pending::{
    client_cutoff_error, lowercase_hex, map_pending_mutation_dispatch_error,
    pending_closed_recovery_evidence_v2, pending_owner_receipt_v2, positive_i64, positive_non_zero,
    RuntimePendingDrainSealBindingsV2,
};
use super::pending_succession_semantic::validate_pending_drain_succession_projection_v3;
use super::query::{
    EXECUTE_PENDING_DRAIN_SUCCESSION_STARTUP_RECOVERY_V3_QUERY,
    SELECT_PENDING_DRAIN_STARTUP_RECOVERY_V3_QUERY,
};
use super::row::{RuntimeStartupRecoveryExecutionExpectedV2, RuntimeStartupRecoveryExecutionRowV2};
use crate::database::{
    begin_execution_mutation_transaction, begin_execution_serializable_observation_transaction,
    verify_runtime_execution_binding_v1,
};
use crate::error::{map_mutation_commit_error, map_query_error};
use crate::{PostgresRuntimeExecutionV1, RuntimeExecutionPersistenceErrorV1};

#[derive(Clone, Debug, sqlx::FromRow)]
struct RuntimePendingDrainSelectionRowV3 {
    selection_outcome_name: String,
    observed_database_now: DateTime<Utc>,
    observed_owner_expires_at: DateTime<Utc>,
    selected_drain_intent_id: Option<String>,
    selected_source_intent_revision: Option<i64>,
    selected_source_state_digest: Option<String>,
    selected_source_state_bytes: Option<Vec<u8>>,
    selected_product_operation_id: Option<String>,
    selected_product_mutation_digest: Option<String>,
    selected_tenant_id: Option<String>,
    selected_installation_id: Option<String>,
    selected_deployment_id: Option<String>,
    selected_expected_revision: Option<i64>,
    selected_product_mutation_request_bytes: Option<Vec<u8>>,
    selected_drain_intent_request_bytes: Option<Vec<u8>>,
    selected_drain_intent_digest: Option<String>,
    selected_slot_guild_id: Option<String>,
    selected_slot_ruleset_key: Option<String>,
    selected_target_version: Option<i64>,
    selected_target_content_hash: Option<String>,
    selected_target_binding_revision: Option<i64>,
    selected_target_binding_fingerprint: Option<String>,
    predecessor_claim_terminal_digest: Option<String>,
    predecessor_gateway_shard_id: Option<String>,
    predecessor_process_instance_id: Option<String>,
    predecessor_lease_epoch: Option<i64>,
    predecessor_runtime_build_revision: Option<String>,
    predecessor_owner_revision: Option<i64>,
    predecessor_controller_id: Option<String>,
    predecessor_controller_fencing_token: Option<i64>,
    predecessor_claim_epoch: Option<i64>,
    predecessor_claim_revision: Option<i64>,
    predecessor_claim_expires_at: Option<DateTime<Utc>>,
    predecessor_seal_process_instance_id: Option<String>,
    predecessor_seal_generation: Option<i64>,
    predecessor_seal_observation_sequence: Option<i64>,
}

struct RuntimePendingDrainSelectedSourceFieldsV3 {
    intent_id: String,
    source_revision: i64,
    source_digest: String,
    source_bytes: Vec<u8>,
    product_operation_id: String,
    product_mutation_digest: String,
    tenant_id: String,
    installation_id: String,
    deployment_id: String,
    expected_revision: i64,
    product_mutation_request_bytes: Vec<u8>,
    drain_intent_request_bytes: Vec<u8>,
    drain_intent_digest: String,
    guild_id: String,
    ruleset_key: String,
    target_version: i64,
    target_content_hash: String,
    target_binding_revision: i64,
    target_binding_fingerprint: String,
}

struct RuntimePendingDrainPredecessorFieldsV3 {
    terminal_digest: String,
    gateway_shard_id: String,
    process_instance_id: String,
    lease_epoch: i64,
    runtime_build_revision: String,
    owner_revision: i64,
    controller_id: String,
    controller_fencing_token: i64,
    claim_epoch: i64,
    claim_revision: i64,
    claim_expires_at: DateTime<Utc>,
    seal_process_instance_id: String,
    seal_generation: i64,
    seal_observation_sequence: i64,
}

struct RuntimePendingDrainSuccessionBindingsV3 {
    originating_emergency_generation: i64,
    coordinator_generation: i64,
    action_authority_revision: i64,
    selection_authority_revision: i64,
    owner_lease_epoch: i64,
    owner_revision: i64,
    closed_evidence: super::closed_evidence::RuntimeClosedRecoveryExpectedEvidenceV2,
}

impl RuntimePendingDrainSuccessionBindingsV3 {
    fn from_request(
        request: &RuntimeStartupRecoveryExecutionRequestV2,
    ) -> Result<Self, RuntimeExecutionPersistenceErrorV1> {
        if request.class() != RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent {
            return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
        }
        let correlation = request.correlation();
        let (selection_authority_revision, action_authority_revision) =
            direct_succession_authority_revisions_v3(
                correlation.selection_authority_revision().get(),
                correlation.authority_revision().get(),
            )?;
        Ok(Self {
            originating_emergency_generation: positive_i64(
                correlation.originating_emergency_generation().get(),
            )?,
            coordinator_generation: positive_i64(correlation.coordinator_generation().get())?,
            action_authority_revision,
            selection_authority_revision,
            owner_lease_epoch: positive_i64(request.gateway_owner_lease_id().lease_epoch.get())?,
            owner_revision: positive_i64(request.expected_owner_revision().get())?,
            closed_evidence: pending_closed_recovery_evidence_v2(request)?,
        })
    }

    fn expected_action(
        &self,
        request: &RuntimeStartupRecoveryExecutionRequestV2,
        minimum_database_now: DateTime<Utc>,
    ) -> RuntimeStartupRecoveryExecutionExpectedV2 {
        RuntimeStartupRecoveryExecutionExpectedV2 {
            recovery_id: request.correlation().recovery_id().as_str().to_owned(),
            originating_emergency_generation: self.originating_emergency_generation,
            coordinator_generation: self.coordinator_generation,
            action_authority_revision: self.action_authority_revision,
            selection_authority_revision: self.selection_authority_revision,
            recovery_class: "pending_runtime_drain_intent",
            gateway_owner_lease_id: request.gateway_owner_lease_id().clone(),
            owner_revision: self.owner_revision,
            owner_expires_at: request.expected_owner_expires_at(),
            minimum_database_now,
            closed_evidence: Some(self.closed_evidence.clone()),
        }
    }
}

impl RuntimePendingDrainSelectionRowV3 {
    fn decode(
        self,
        authorization: &RuntimeAuthorizedPendingDrainSelectionV3,
    ) -> Result<RuntimePendingDrainSelectionReceiptV3, RuntimeExecutionPersistenceErrorV1> {
        let request = authorization.request();
        let owner_receipt = pending_owner_receipt_v2(
            request,
            self.observed_database_now,
            self.observed_owner_expires_at,
            request.minimum_database_now(),
        )?;
        let selected = self.selected_fields()?;
        let predecessor = self.predecessor_fields()?;
        let outcome = match (self.selection_outcome_name.as_str(), selected, predecessor) {
            ("no_candidate", None, None) => RuntimePendingDrainSelectionOutcomeV3::NoCandidate,
            ("unclaimed", Some(selected), None) => {
                RuntimePendingDrainSelectionOutcomeV3::Unclaimed(decode_unclaimed_candidate_v3(
                    selected,
                )?)
            }
            ("fresh_previous_owner", Some(selected), Some(predecessor)) => {
                let candidate = decode_previous_owner_candidate_v3(
                    selected,
                    predecessor,
                    self.observed_database_now,
                    false,
                )?;
                RuntimePendingDrainSelectionOutcomeV3::FreshPreviousOwner(candidate)
            }
            ("expired_previous_owner", Some(selected), Some(predecessor)) => {
                let candidate = decode_previous_owner_candidate_v3(
                    selected,
                    predecessor,
                    self.observed_database_now,
                    true,
                )?;
                RuntimePendingDrainSelectionOutcomeV3::ExpiredPreviousOwner(candidate)
            }
            _ => return Err(invalid()),
        };
        Ok(RuntimePendingDrainSelectionReceiptV3::new(
            request.correlation().clone(),
            owner_receipt,
            outcome,
        ))
    }

    fn selected_fields(
        &self,
    ) -> Result<Option<RuntimePendingDrainSelectedSourceFieldsV3>, RuntimeExecutionPersistenceErrorV1>
    {
        let present = [
            self.selected_drain_intent_id.is_some(),
            self.selected_source_intent_revision.is_some(),
            self.selected_source_state_digest.is_some(),
            self.selected_source_state_bytes.is_some(),
            self.selected_product_operation_id.is_some(),
            self.selected_product_mutation_digest.is_some(),
            self.selected_tenant_id.is_some(),
            self.selected_installation_id.is_some(),
            self.selected_deployment_id.is_some(),
            self.selected_expected_revision.is_some(),
            self.selected_product_mutation_request_bytes.is_some(),
            self.selected_drain_intent_request_bytes.is_some(),
            self.selected_drain_intent_digest.is_some(),
            self.selected_slot_guild_id.is_some(),
            self.selected_slot_ruleset_key.is_some(),
            self.selected_target_version.is_some(),
            self.selected_target_content_hash.is_some(),
            self.selected_target_binding_revision.is_some(),
            self.selected_target_binding_fingerprint.is_some(),
        ];
        if present.iter().all(|value| !value) {
            return Ok(None);
        }
        if !present.iter().all(|value| *value) {
            return Err(invalid());
        }
        Ok(Some(RuntimePendingDrainSelectedSourceFieldsV3 {
            intent_id: self.selected_drain_intent_id.clone().ok_or_else(invalid)?,
            source_revision: self.selected_source_intent_revision.ok_or_else(invalid)?,
            source_digest: self
                .selected_source_state_digest
                .clone()
                .ok_or_else(invalid)?,
            source_bytes: self
                .selected_source_state_bytes
                .clone()
                .ok_or_else(invalid)?,
            product_operation_id: self
                .selected_product_operation_id
                .clone()
                .ok_or_else(invalid)?,
            product_mutation_digest: self
                .selected_product_mutation_digest
                .clone()
                .ok_or_else(invalid)?,
            tenant_id: self.selected_tenant_id.clone().ok_or_else(invalid)?,
            installation_id: self.selected_installation_id.clone().ok_or_else(invalid)?,
            deployment_id: self.selected_deployment_id.clone().ok_or_else(invalid)?,
            expected_revision: self.selected_expected_revision.ok_or_else(invalid)?,
            product_mutation_request_bytes: self
                .selected_product_mutation_request_bytes
                .clone()
                .ok_or_else(invalid)?,
            drain_intent_request_bytes: self
                .selected_drain_intent_request_bytes
                .clone()
                .ok_or_else(invalid)?,
            drain_intent_digest: self
                .selected_drain_intent_digest
                .clone()
                .ok_or_else(invalid)?,
            guild_id: self.selected_slot_guild_id.clone().ok_or_else(invalid)?,
            ruleset_key: self.selected_slot_ruleset_key.clone().ok_or_else(invalid)?,
            target_version: self.selected_target_version.ok_or_else(invalid)?,
            target_content_hash: self
                .selected_target_content_hash
                .clone()
                .ok_or_else(invalid)?,
            target_binding_revision: self.selected_target_binding_revision.ok_or_else(invalid)?,
            target_binding_fingerprint: self
                .selected_target_binding_fingerprint
                .clone()
                .ok_or_else(invalid)?,
        }))
    }

    fn predecessor_fields(
        &self,
    ) -> Result<Option<RuntimePendingDrainPredecessorFieldsV3>, RuntimeExecutionPersistenceErrorV1>
    {
        let present = [
            self.predecessor_claim_terminal_digest.is_some(),
            self.predecessor_gateway_shard_id.is_some(),
            self.predecessor_process_instance_id.is_some(),
            self.predecessor_lease_epoch.is_some(),
            self.predecessor_runtime_build_revision.is_some(),
            self.predecessor_owner_revision.is_some(),
            self.predecessor_controller_id.is_some(),
            self.predecessor_controller_fencing_token.is_some(),
            self.predecessor_claim_epoch.is_some(),
            self.predecessor_claim_revision.is_some(),
            self.predecessor_claim_expires_at.is_some(),
            self.predecessor_seal_process_instance_id.is_some(),
            self.predecessor_seal_generation.is_some(),
            self.predecessor_seal_observation_sequence.is_some(),
        ];
        if present.iter().all(|value| !value) {
            return Ok(None);
        }
        if !present.iter().all(|value| *value) {
            return Err(invalid());
        }
        Ok(Some(RuntimePendingDrainPredecessorFieldsV3 {
            terminal_digest: self
                .predecessor_claim_terminal_digest
                .clone()
                .ok_or_else(invalid)?,
            gateway_shard_id: self
                .predecessor_gateway_shard_id
                .clone()
                .ok_or_else(invalid)?,
            process_instance_id: self
                .predecessor_process_instance_id
                .clone()
                .ok_or_else(invalid)?,
            lease_epoch: self.predecessor_lease_epoch.ok_or_else(invalid)?,
            runtime_build_revision: self
                .predecessor_runtime_build_revision
                .clone()
                .ok_or_else(invalid)?,
            owner_revision: self.predecessor_owner_revision.ok_or_else(invalid)?,
            controller_id: self.predecessor_controller_id.clone().ok_or_else(invalid)?,
            controller_fencing_token: self
                .predecessor_controller_fencing_token
                .ok_or_else(invalid)?,
            claim_epoch: self.predecessor_claim_epoch.ok_or_else(invalid)?,
            claim_revision: self.predecessor_claim_revision.ok_or_else(invalid)?,
            claim_expires_at: self.predecessor_claim_expires_at.ok_or_else(invalid)?,
            seal_process_instance_id: self
                .predecessor_seal_process_instance_id
                .clone()
                .ok_or_else(invalid)?,
            seal_generation: self.predecessor_seal_generation.ok_or_else(invalid)?,
            seal_observation_sequence: self
                .predecessor_seal_observation_sequence
                .ok_or_else(invalid)?,
        }))
    }
}

impl PostgresRuntimeExecutionV1 {
    async fn select_pending_drain_v3(
        &self,
        authorization: &RuntimeAuthorizedPendingDrainSelectionV3,
        operation_cutoff: Instant,
    ) -> Result<RuntimePendingDrainSelectionReceiptV3, RuntimeExecutionPersistenceErrorV1> {
        let bindings =
            RuntimePendingDrainSuccessionBindingsV3::from_request(authorization.request())?;
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
            self.select_pending_drain_on_connection_v3(
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
            Ok(Ok(_)) | Err(_) => Err(RuntimeExecutionPersistenceErrorV1::Timeout),
            Ok(Err(error)) => Err(error),
        }
    }

    async fn select_pending_drain_on_connection_v3(
        &self,
        connection: &mut PgConnection,
        authorization: &RuntimeAuthorizedPendingDrainSelectionV3,
        bindings: &RuntimePendingDrainSuccessionBindingsV3,
    ) -> Result<RuntimePendingDrainSelectionReceiptV3, RuntimeExecutionPersistenceErrorV1> {
        let mut transaction =
            begin_execution_serializable_observation_transaction(connection, self.timeouts).await?;
        verify_runtime_execution_binding_v1(&mut transaction, &self.expectation).await?;
        let request = authorization.request();
        let owner = request.gateway_owner_lease_id();
        let mut rows = sqlx::query_as::<_, RuntimePendingDrainSelectionRowV3>(
            SELECT_PENDING_DRAIN_STARTUP_RECOVERY_V3_QUERY,
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

    async fn execute_pending_drain_succession_v3(
        &self,
        authorization: &RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
        operation_cutoff: Instant,
    ) -> Result<
        RuntimePendingDrainSuccessionAcknowledgementReceiptV3,
        RuntimeExecutionPersistenceErrorV1,
    > {
        let bindings =
            RuntimePendingDrainSuccessionBindingsV3::from_request(authorization.request())?;
        let seal = RuntimePendingDrainSealBindingsV2::from_witness(authorization.seal())?;
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
            self.execute_pending_drain_succession_on_connection_v3(
                database_connection,
                authorization,
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

    async fn execute_pending_drain_succession_on_connection_v3(
        &self,
        connection: &mut PgConnection,
        authorization: &RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
        bindings: &RuntimePendingDrainSuccessionBindingsV3,
        seal: &RuntimePendingDrainSealBindingsV2,
        mutation_dispatched: &AtomicBool,
    ) -> Result<
        RuntimePendingDrainSuccessionAcknowledgementReceiptV3,
        RuntimeExecutionPersistenceErrorV1,
    > {
        let mut transaction =
            begin_execution_mutation_transaction(connection, self.timeouts).await?;
        verify_runtime_execution_binding_v1(&mut transaction, &self.expectation).await?;
        let request = authorization.request();
        let owner = request.gateway_owner_lease_id();
        let evidence = &bindings.closed_evidence;
        let candidate = authorization.candidate();
        mutation_dispatched.store(true, Ordering::Release);
        let mut rows = sqlx::query_as::<_, RuntimeStartupRecoveryExecutionRowV2>(
            EXECUTE_PENDING_DRAIN_SUCCESSION_STARTUP_RECOVERY_V3_QUERY,
        )
        .bind(request.correlation().recovery_id().as_str())
        .bind(bindings.originating_emergency_generation)
        .bind(bindings.coordinator_generation)
        .bind(bindings.action_authority_revision)
        .bind(bindings.selection_authority_revision)
        .bind(owner.gateway_shard_id.as_str())
        .bind(owner.process_instance_id.as_str())
        .bind(bindings.owner_lease_epoch)
        .bind(owner.expected_build_revision.as_str())
        .bind(bindings.owner_revision)
        .bind(request.expected_owner_expires_at())
        .bind(authorization.minimum_database_now())
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
        .bind(lowercase_hex(
            candidate.predecessor_claim_terminal_digest().as_bytes(),
        ))
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
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_pending_mutation_dispatch_error)?;
        if rows.len() != 1 {
            return Err(invalid());
        }
        let expected = bindings.expected_action(request, authorization.minimum_database_now());
        let record = rows
            .pop()
            .ok_or_else(invalid)?
            .decode_pending_succession(&expected)?;
        let semantic = validate_pending_drain_succession_projection_v3(
            &record.terminal_projection_bytes,
            authorization,
            evidence,
            seal,
            record.minimum_database_now,
            record.database_now,
            record.recorded_at,
        )?;
        transaction
            .commit()
            .await
            .map_err(map_mutation_commit_error)?;
        Ok(RuntimePendingDrainSuccessionAcknowledgementReceiptV3::new(
            authorization.action_identity().clone(),
            candidate.clone(),
            authorization.seal().clone(),
            semantic.successor_intent_revision,
            semantic.successor_state_digest,
            record.terminal_digest,
            record.owner_receipt,
        ))
    }
}

impl RuntimePendingDrainSelectionPortV3 for PostgresRuntimeExecutionV1 {
    type Error = RuntimeExecutionPersistenceErrorV1;

    fn select_pending_drain_v3(
        &self,
        authorization: &RuntimeAuthorizedPendingDrainSelectionV3,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimePendingDrainSelectionReceiptV3, Self::Error>> + Send
    {
        self.select_pending_drain_v3(authorization, operation_cutoff)
    }
}

impl RuntimePendingDrainSuccessionAcknowledgementExecutionPortV3 for PostgresRuntimeExecutionV1 {
    type Error = RuntimeExecutionPersistenceErrorV1;

    fn execute_pending_drain_succession_acknowledgement(
        &self,
        authorization: &RuntimeAuthorizedPendingDrainSuccessionAcknowledgementV3,
        operation_cutoff: Instant,
    ) -> impl Future<
        Output = Result<RuntimePendingDrainSuccessionAcknowledgementReceiptV3, Self::Error>,
    > + Send {
        self.execute_pending_drain_succession_v3(authorization, operation_cutoff)
    }
}

fn decode_unclaimed_candidate_v3(
    selected: RuntimePendingDrainSelectedSourceFieldsV3,
) -> Result<RuntimePendingDrainCandidateV2, RuntimeExecutionPersistenceErrorV1> {
    let decoded = decode_selected_source_v3(&selected)?;
    let source = RuntimePersistedUnclaimedPendingDrainIntentV2::from_persisted(
        &decoded.root,
        decoded.source_revision,
        "pending",
        &selected.source_bytes,
    )
    .map_err(|_| invalid())?;
    let key = source.canonical().intent().key();
    RuntimePendingDrainCandidateV2::new(
        key.intent_id.clone(),
        key.slot.clone(),
        key.expected_target.clone(),
        decoded.source_revision,
        decoded.source_digest,
    )
    .map_err(|_| invalid())
}

fn decode_previous_owner_candidate_v3(
    selected: RuntimePendingDrainSelectedSourceFieldsV3,
    predecessor: RuntimePendingDrainPredecessorFieldsV3,
    observed_database_now: DateTime<Utc>,
    expected_expired: bool,
) -> Result<RuntimePendingDrainPreviousOwnerClaimedCandidateV3, RuntimeExecutionPersistenceErrorV1>
{
    let decoded = decode_selected_source_v3(&selected)?;
    let source = RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2::from_persisted(
        &decoded.root,
        decoded.source_revision,
        "pending",
        &selected.source_bytes,
    )
    .map_err(|_| invalid())?;
    let claim = source
        .canonical()
        .intent()
        .state()
        .pending_claim()
        .ok_or_else(invalid)?;
    let owner = claim.gateway_owner_lease_id();
    let seal = claim.progress().seal();
    if predecessor.gateway_shard_id != owner.gateway_shard_id.as_str()
        || predecessor.process_instance_id != owner.process_instance_id.as_str()
        || predecessor.process_instance_id != claim.process_instance_id().as_str()
        || positive_u64(predecessor.lease_epoch)? != owner.lease_epoch.get()
        || predecessor.runtime_build_revision != owner.expected_build_revision.as_str()
        || positive_u64(predecessor.owner_revision)? != claim.observed_owner_revision().get()
        || predecessor.controller_id != claim.controller_id().as_str()
        || positive_u64(predecessor.controller_fencing_token)?
            != claim.controller_fencing_token().get()
        || positive_u64(predecessor.claim_epoch)? != claim.claim_epoch().get()
        || positive_u64(predecessor.claim_revision)? != claim.claim_revision().get()
        || predecessor.claim_expires_at != claim.expires_at()
        || predecessor.seal_process_instance_id != seal.process_instance_id().as_str()
        || positive_u64(predecessor.seal_generation)? != seal.seal_generation().get()
        || positive_u64(predecessor.seal_observation_sequence)?
            != seal.registry_observation_sequence().get()
        || (observed_database_now >= claim.expires_at()) != expected_expired
    {
        return Err(invalid());
    }
    let terminal_digest = RuntimeStartupRecoveryExecutionTerminalDigestV2::new(
        lowercase_sha256_bytes(&predecessor.terminal_digest)?,
    )
    .map_err(|_| invalid())?;
    RuntimePendingDrainPreviousOwnerClaimedCandidateV3::new(
        RuntimePendingDrainPreviousOwnerClaimedCandidateInputV3 {
            source,
            source_state_digest: decoded.source_digest,
            predecessor_claim_terminal_digest: terminal_digest,
            product_mutation_request_sha256: decoded.product_mutation_request_sha256,
            drain_intent_request_sha256: decoded.drain_intent_request_sha256,
        },
    )
    .map_err(|_| invalid())
}

struct RuntimePendingDrainDecodedSourceV3 {
    root: RuntimePersistedProductDrainRootV2,
    source_revision: std::num::NonZeroU64,
    source_digest: RuntimePendingDrainStateDigestV2,
    product_mutation_request_sha256: [u8; 32],
    drain_intent_request_sha256: [u8; 32],
}

fn decode_selected_source_v3(
    selected: &RuntimePendingDrainSelectedSourceFieldsV3,
) -> Result<RuntimePendingDrainDecodedSourceV3, RuntimeExecutionPersistenceErrorV1> {
    let product_digest = RuntimeProductMutationDigestV2::parse(&selected.product_mutation_digest)
        .map_err(|_| invalid())?;
    let drain_digest =
        RuntimeDrainIntentDigestV2::parse(&selected.drain_intent_digest).map_err(|_| invalid())?;
    let canonical = RuntimeCanonicalProductDrainV2::from_persisted(
        &selected.product_mutation_request_bytes,
        &product_digest,
        &selected.drain_intent_request_bytes,
        &drain_digest,
    )
    .map_err(|_| invalid())?;
    let product = canonical.product_preimage();
    let drain = &canonical.drain_preimage().key;
    let target = &product.expected_target;
    if selected.intent_id != drain.intent_id.as_str()
        || selected.product_operation_id != product.operation_id.as_str()
        || selected.product_operation_id != drain.product_operation_id.as_str()
        || selected.product_mutation_digest != drain.product_mutation_digest.as_str()
        || selected.tenant_id != product.scope.tenant_id.as_str()
        || selected.tenant_id != drain.scope.tenant_id.as_str()
        || selected.installation_id != product.scope.installation_id.as_str()
        || selected.installation_id != drain.scope.installation_id.as_str()
        || selected.deployment_id != product.scope.deployment_id.as_str()
        || selected.deployment_id != drain.scope.deployment_id.as_str()
        || positive_u64(selected.expected_revision)? != product.expected_revision.get()
        || product.expected_revision != drain.expected_revision
        || selected.guild_id != drain.slot.guild_id.to_string()
        || selected.ruleset_key != drain.slot.ruleset_key.as_str()
        || drain.slot != product.slot
        || selected.guild_id != target.guild_id.to_string()
        || selected.ruleset_key != target.ruleset_key.as_str()
        || positive_u64(selected.target_version)? != u64::from(target.version.get())
        || selected.target_content_hash != target.content_hash.to_hex()
        || positive_u64(selected.target_binding_revision)? != target.binding_revision.get()
        || selected.target_binding_fingerprint != target.binding_fingerprint.as_str()
        || drain.expected_target != *target
    {
        return Err(invalid());
    }
    let persisted_digest = lowercase_sha256_bytes(&selected.source_digest)?;
    let derived_digest: [u8; 32] = Sha256::digest(&selected.source_bytes).into();
    if persisted_digest != derived_digest {
        return Err(invalid());
    }
    let root = RuntimePersistedProductDrainRootV2::from_persisted(
        product.scope.clone(),
        product.expected_revision,
        &product.operation_id,
        drain.scope.clone(),
        drain.slot.clone(),
        drain.expected_revision,
        &drain.intent_id,
        &drain.expected_target,
        &selected.product_mutation_request_bytes,
        &product_digest,
        &selected.drain_intent_request_bytes,
        &drain_digest,
    )
    .map_err(|_| invalid())?;
    Ok(RuntimePendingDrainDecodedSourceV3 {
        root,
        source_revision: positive_non_zero(selected.source_revision)?,
        source_digest: RuntimePendingDrainStateDigestV2::new(derived_digest)
            .map_err(|_| invalid())?,
        product_mutation_request_sha256: Sha256::digest(&selected.product_mutation_request_bytes)
            .into(),
        drain_intent_request_sha256: Sha256::digest(&selected.drain_intent_request_bytes).into(),
    })
}

fn positive_u64(value: i64) -> Result<u64, RuntimeExecutionPersistenceErrorV1> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(invalid)
}

fn direct_succession_authority_revisions_v3(
    selection: u64,
    action: u64,
) -> Result<(i64, i64), RuntimeExecutionPersistenceErrorV1> {
    let selection = positive_i64(selection)?;
    let action = positive_i64(action)?;
    if selection.checked_add(1) == Some(action) {
        Ok((selection, action))
    } else {
        Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
    }
}

fn invalid() -> RuntimeExecutionPersistenceErrorV1 {
    RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_row_rejects_partial_selected_projection() {
        let mut row = empty_selection_row_v3();
        row.selected_drain_intent_id = Some("intent".to_owned());

        assert!(matches!(
            row.selected_fields(),
            Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        ));
    }

    #[test]
    fn selection_row_rejects_partial_predecessor_projection() {
        let mut row = empty_selection_row_v3();
        row.predecessor_claim_terminal_digest = Some("0".repeat(64));

        assert!(matches!(
            row.predecessor_fields(),
            Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)
        ));
    }

    #[test]
    fn direct_succession_accepts_the_maximum_action_authority_revision() {
        assert_eq!(
            direct_succession_authority_revisions_v3(i64::MAX as u64 - 1, i64::MAX as u64).unwrap(),
            (i64::MAX - 1, i64::MAX)
        );
    }

    #[test]
    fn direct_succession_rejects_nonconsecutive_authority_revisions() {
        assert!(matches!(
            direct_succession_authority_revisions_v3(7, 9),
            Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
        ));
    }

    fn empty_selection_row_v3() -> RuntimePendingDrainSelectionRowV3 {
        let now = Utc::now();
        RuntimePendingDrainSelectionRowV3 {
            selection_outcome_name: "no_candidate".to_owned(),
            observed_database_now: now,
            observed_owner_expires_at: now,
            selected_drain_intent_id: None,
            selected_source_intent_revision: None,
            selected_source_state_digest: None,
            selected_source_state_bytes: None,
            selected_product_operation_id: None,
            selected_product_mutation_digest: None,
            selected_tenant_id: None,
            selected_installation_id: None,
            selected_deployment_id: None,
            selected_expected_revision: None,
            selected_product_mutation_request_bytes: None,
            selected_drain_intent_request_bytes: None,
            selected_drain_intent_digest: None,
            selected_slot_guild_id: None,
            selected_slot_ruleset_key: None,
            selected_target_version: None,
            selected_target_content_hash: None,
            selected_target_binding_revision: None,
            selected_target_binding_fingerprint: None,
            predecessor_claim_terminal_digest: None,
            predecessor_gateway_shard_id: None,
            predecessor_process_instance_id: None,
            predecessor_lease_epoch: None,
            predecessor_runtime_build_revision: None,
            predecessor_owner_revision: None,
            predecessor_controller_id: None,
            predecessor_controller_fencing_token: None,
            predecessor_claim_epoch: None,
            predecessor_claim_revision: None,
            predecessor_claim_expires_at: None,
            predecessor_seal_process_instance_id: None,
            predecessor_seal_generation: None,
            predecessor_seal_observation_sequence: None,
        }
    }
}
