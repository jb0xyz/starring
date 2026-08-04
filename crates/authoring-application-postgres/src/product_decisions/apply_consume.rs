use std::num::NonZeroU64;

use authoring_application::{
    AuthorizedApplyProductV1, FreshGuildAuthorityEvidence, ProductControlPortError,
    ProductDrainSelectorV1,
};
use authoring_application_discord::FreshDiscordAuthorityEvidenceV1;
use automation_runtime_controller::{
    RuntimeCanonicalDrainIntentStateV2, RuntimeDrainConsumptionSourceV2,
    RuntimeDrainIntentCanonicalStateKindV2, RuntimeDrainIntentReceiptV2,
    RuntimePersistedProductDrainRootV2, RuntimeRouteAbsentDrainIntentSourceV2,
    RuntimeUnixMicrosecondsV2,
};
use automation_runtime_convergence::{
    RuntimeDeployment, RuntimeDeploymentSnapshotV1, SupersedingDeploymentV1,
};
use automation_runtime_convergence_postgres::{
    prepare_product_drain_source_supersession_v1, PreparedProductDrainSourceSupersessionV1,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::types::Json;
use sqlx::{Postgres, Transaction};

use super::apply_projection::{prepare_product_apply_v1, PreparedProductApplyV1};
use super::digest::ApplyDigests;

const APPLY_DRAIN_SUPERSESSION_REASON: &str = "correlated Product apply";
const CONSUME_RUNTIME_DRAIN_QUERY: &str = "SELECT outcome_name, preparation_ready, \
    exact_replay, requires_commit, preparation_token, locked_product_projection, \
    source_deployment_snapshot, source_acknowledged_at, product_operation_id, \
    product_mutation_digest, drain_intent_digest, source_deployment_id, \
    source_deployment_revision, source_result_deployment_revision, \
    source_result_deployment_snapshot, source_result_deployment_snapshot_digest, \
    result_deployment_id, result_deployment_revision, result_deployment_snapshot, \
    result_deployment_snapshot_digest, product_resulting_revision, \
    product_resulting_state, product_receipt_id, product_audit_event_id, \
    drain_intent_id, source_intent_revision, source_state_bytes, source_state_digest, \
    result_intent_revision, result_intent_state, result_state_bytes, result_state_digest, \
    source_slot_epoch, successor_slot_epoch, terminal_action_id, \
    terminal_projection_bytes, terminal_projection_digest, terminal_database_time \
    FROM public.starring_product_apply_consume_runtime_drain_v2(\
        requested_phase => $1, expected_tenant_id => $2, expected_installation_id => $3, \
        expected_promotion_id => $4, expected_product_revision => $5, \
        expected_payload_digest => $6, expected_principal_id => $7, \
        expected_product_session_digest => $8, session_subject_digest => $9, \
        expected_acting_user_id => $10, expected_discord_application_id => $11, \
        expected_guild_id => $12, expected_capability => $13, \
        expected_authority_revision => $14, expected_authority_payload_digest => $15, \
        expected_authority_observation_digest => $16, expected_authority_observed_at => $17, \
        expected_authority_expires_at => $18, expected_effective_permission_bits => $19, \
        expected_guild_owner => $20, product_request_id => $21, \
        active_idempotency_key_digest => $22, idempotency_key_digest_candidates => $23, \
        idempotency_digest_key_id_candidates => $24, \
        idempotency_digest_key_fingerprint_candidates => $25, \
        idempotency_digest_key_id => $26, semantic_request_digest => $27, \
        new_receipt_id => $28, new_audit_event_id => $29, new_apply_attempt_id => $30, \
        new_deployment_id => $31, expected_drain_intent_id => $32, \
        expected_source_intent_revision => $33, expected_source_state_bytes => $34, \
        expected_source_state_digest => $35, expected_product_operation_id => $36, \
        expected_source_deployment_id => $37, expected_source_deployment_revision => $38, \
        proposed_terminal_action_id => $39, expected_preparation_token => $40, \
        prepared_source_result_snapshot_bytes => $41, \
        prepared_source_result_snapshot_digest => $42, \
        prepared_result_deployment_snapshot_bytes => $43, \
        prepared_result_deployment_snapshot_digest => $44, \
        prepared_desired_target_digest => $45, prepared_activation_notices_bytes => $46)";

pub(super) struct ValidatedRuntimeDrainConsumptionV2 {
    root: RuntimePersistedProductDrainRootV2,
    source: RuntimeRouteAbsentDrainIntentSourceV2,
    source_state_bytes: Box<[u8]>,
    source_state_digest: String,
    source_deployment: RuntimeDeploymentSnapshotV1,
    source_intent_revision: i64,
    source_deployment_revision: i64,
}

impl ValidatedRuntimeDrainConsumptionV2 {
    pub(super) fn new(
        root: RuntimePersistedProductDrainRootV2,
        source_state: RuntimeCanonicalDrainIntentStateV2,
        source_state_digest: String,
        source_deployment: RuntimeDeploymentSnapshotV1,
    ) -> Result<Self, ProductControlPortError> {
        if source_state.state_kind().map_err(|_| invalid_result())?
            != RuntimeDrainIntentCanonicalStateKindV2::RouteAbsentAcknowledged
            || !digest_matches(source_state.state_bytes(), &source_state_digest)
            || source_state.intent().canonical() != root.canonical()
            || source_deployment.identity.tenant_id
                != root.product_operation_scope().scope().tenant_id
            || source_deployment.identity.installation_id
                != root.product_operation_scope().scope().installation_id
            || source_deployment.identity.deployment_id
                != root.product_operation_scope().scope().deployment_id
            || source_deployment.revision != root.product_operation_scope().expected_revision()
        {
            return Err(invalid_result());
        }
        RuntimeDeployment::restore(source_deployment.clone()).map_err(|_| invalid_result())?;
        let source =
            RuntimeRouteAbsentDrainIntentSourceV2::from_acknowledged(source_state.intent().clone())
                .map_err(|_| invalid_result())?;
        let source_intent_revision =
            database_revision_input(source.source().intent_revision().get())?;
        let source_deployment_revision = database_revision_input(source_deployment.revision.get())?;
        Ok(Self {
            root,
            source,
            source_state_bytes: source_state.state_bytes().to_vec().into_boxed_slice(),
            source_state_digest,
            source_deployment,
            source_intent_revision,
            source_deployment_revision,
        })
    }

    pub(super) fn selector(&self) -> Result<ProductDrainSelectorV1, ProductControlPortError> {
        ProductDrainSelectorV1::from_server_projection(
            self.root.drain_intent_id().as_str(),
            self.source.source().intent_revision().get(),
            self.source_state_digest.clone(),
            self.root.product_operation_id().as_str(),
            self.source_deployment.revision.get(),
        )
        .map_err(|_| invalid_result())
    }

    fn acknowledged_at(&self) -> Result<DateTime<Utc>, ProductControlPortError> {
        self.source
            .source()
            .state()
            .acknowledgement()
            .map(|acknowledgement| acknowledgement.acknowledged_at())
            .ok_or_else(invalid_result)
    }
}

#[derive(sqlx::FromRow)]
pub(super) struct ApplyConsumeRuntimeDrainRow {
    pub outcome_name: String,
    pub preparation_ready: bool,
    pub exact_replay: bool,
    pub requires_commit: bool,
    pub preparation_token: Option<String>,
    pub locked_product_projection: Option<Json<Value>>,
    pub source_deployment_snapshot: Option<Json<Value>>,
    pub source_acknowledged_at: Option<DateTime<Utc>>,
    pub product_operation_id: Option<String>,
    pub product_mutation_digest: Option<String>,
    pub drain_intent_digest: Option<String>,
    pub source_deployment_id: Option<String>,
    pub source_deployment_revision: Option<i64>,
    pub source_result_deployment_revision: Option<i64>,
    pub source_result_deployment_snapshot: Option<Json<Value>>,
    pub source_result_deployment_snapshot_digest: Option<String>,
    pub result_deployment_id: Option<String>,
    pub result_deployment_revision: Option<i64>,
    pub result_deployment_snapshot: Option<Json<Value>>,
    pub result_deployment_snapshot_digest: Option<String>,
    pub product_resulting_revision: Option<i64>,
    pub product_resulting_state: Option<String>,
    pub product_receipt_id: Option<String>,
    pub product_audit_event_id: Option<String>,
    pub drain_intent_id: Option<String>,
    pub source_intent_revision: Option<i64>,
    pub source_state_bytes: Option<Vec<u8>>,
    pub source_state_digest: Option<String>,
    pub result_intent_revision: Option<i64>,
    pub result_intent_state: Option<String>,
    pub result_state_bytes: Option<Vec<u8>>,
    pub result_state_digest: Option<String>,
    pub source_slot_epoch: Option<i64>,
    pub successor_slot_epoch: Option<i64>,
    pub terminal_action_id: Option<String>,
    pub terminal_projection_bytes: Option<Vec<u8>>,
    pub terminal_projection_digest: Option<String>,
    pub terminal_database_time: Option<DateTime<Utc>>,
}

impl ApplyConsumeRuntimeDrainRow {
    pub(super) fn failure_is_closed(&self) -> bool {
        !self.preparation_ready
            && !self.exact_replay
            && !self.requires_commit
            && self.preparation_token.is_none()
            && self.locked_product_projection.is_none()
            && self.source_deployment_snapshot.is_none()
            && self.source_acknowledged_at.is_none()
            && self.product_operation_id.is_none()
            && self.product_mutation_digest.is_none()
            && self.drain_intent_digest.is_none()
            && self.source_deployment_id.is_none()
            && self.source_deployment_revision.is_none()
            && self.source_result_deployment_revision.is_none()
            && self.source_result_deployment_snapshot.is_none()
            && self.source_result_deployment_snapshot_digest.is_none()
            && self.result_deployment_id.is_none()
            && self.result_deployment_revision.is_none()
            && self.result_deployment_snapshot.is_none()
            && self.result_deployment_snapshot_digest.is_none()
            && self.product_resulting_revision.is_none()
            && self.product_resulting_state.is_none()
            && self.product_receipt_id.is_none()
            && self.product_audit_event_id.is_none()
            && self.drain_intent_id.is_none()
            && self.source_intent_revision.is_none()
            && self.source_state_bytes.is_none()
            && self.source_state_digest.is_none()
            && self.result_intent_revision.is_none()
            && self.result_intent_state.is_none()
            && self.result_state_bytes.is_none()
            && self.result_state_digest.is_none()
            && self.source_slot_epoch.is_none()
            && self.successor_slot_epoch.is_none()
            && self.terminal_action_id.is_none()
            && self.terminal_projection_bytes.is_none()
            && self.terminal_projection_digest.is_none()
            && self.terminal_database_time.is_none()
    }
}

pub(super) struct PreparedRuntimeDrainConsumptionV2 {
    preparation_token: String,
    source: PreparedProductDrainSourceSupersessionV1,
    product: PreparedProductApplyV1,
    activation_notices_bytes: Vec<u8>,
    terminal_database_time: DateTime<Utc>,
    source_slot_epoch: i64,
    source_result_deployment_revision: i64,
    product_deployment_revision: i64,
    product_resulting_revision: i64,
}

pub(super) struct ValidatedRuntimeDrainConsumptionResultV2 {
    pub product_revision: u64,
    pub deployment_id: String,
    pub desired_target_digest: String,
    pub exact_replay: bool,
}

pub(super) struct RuntimeDrainConsumptionCallV2<'request, 'authorization> {
    request: &'request AuthorizedApplyProductV1<'authorization, FreshDiscordAuthorityEvidenceV1>,
    digests: &'request ApplyDigests,
    expected_revision: i64,
    authority_revision: i64,
    source: &'request ValidatedRuntimeDrainConsumptionV2,
}

impl<'request, 'authorization> RuntimeDrainConsumptionCallV2<'request, 'authorization> {
    pub(super) fn new(
        request: &'request AuthorizedApplyProductV1<
            'authorization,
            FreshDiscordAuthorityEvidenceV1,
        >,
        digests: &'request ApplyDigests,
        expected_revision: i64,
        authority_revision: i64,
        source: &'request ValidatedRuntimeDrainConsumptionV2,
    ) -> Self {
        Self {
            request,
            digests,
            expected_revision,
            authority_revision,
            source,
        }
    }
}

pub(super) async fn call_consume_runtime_drain(
    transaction: &mut Transaction<'_, Postgres>,
    phase: &str,
    call: &RuntimeDrainConsumptionCallV2<'_, '_>,
    prepared: Option<&PreparedRuntimeDrainConsumptionV2>,
) -> Result<ApplyConsumeRuntimeDrainRow, sqlx::Error> {
    let request = call.request;
    let digests = call.digests;
    let source = call.source;
    let evidence = request.evidence();
    let empty = &[][..];
    sqlx::query_as::<_, ApplyConsumeRuntimeDrainRow>(CONSUME_RUNTIME_DRAIN_QUERY)
        .bind(phase)
        .bind(request.scope().tenant_id().as_str())
        .bind(request.scope().installation_id().as_str())
        .bind(request.command().promotion.promotion_id().as_str())
        .bind(call.expected_revision)
        .bind(request.command().expected_payload_digest.as_str())
        .bind(request.actor().principal_id().as_str())
        .bind(request.session_fingerprint().as_bytes().as_slice())
        .bind(&digests.session_subject)
        .bind(evidence.acting_user_id().to_string())
        .bind(evidence.discord_application_id().get().to_string())
        .bind(evidence.guild_id().to_string())
        .bind("apply")
        .bind(call.authority_revision)
        .bind(evidence.installation_authority_digest())
        .bind(evidence.observation_digest())
        .bind(evidence.observed_at())
        .bind(evidence.expires_at())
        .bind(evidence.effective_permissions_bits().to_string())
        .bind(evidence.guild_owner())
        .bind(request.request_id().as_str())
        .bind(&digests.active_idempotency)
        .bind(&digests.idempotency_candidates)
        .bind(&digests.idempotency_candidate_key_ids)
        .bind(&digests.idempotency_candidate_key_fingerprints)
        .bind(&digests.active_key_id)
        .bind(&digests.semantic_request)
        .bind(&digests.receipt_id)
        .bind(&digests.audit_event_id)
        .bind(&digests.apply_attempt_id)
        .bind(&digests.deployment_id)
        .bind(source.root.drain_intent_id().as_str())
        .bind(source.source_intent_revision)
        .bind(source.source_state_bytes.as_ref())
        .bind(&source.source_state_digest)
        .bind(source.root.product_operation_id().as_str())
        .bind(source.source_deployment.identity.deployment_id.as_str())
        .bind(source.source_deployment_revision)
        .bind(&digests.drain_consume_terminal_action_id)
        .bind(
            prepared
                .map(|value| value.preparation_token.as_str())
                .unwrap_or(""),
        )
        .bind(
            prepared
                .map(|value| value.source.snapshot_bytes())
                .unwrap_or(empty),
        )
        .bind(
            prepared
                .map(|value| value.source.snapshot_digest())
                .unwrap_or(""),
        )
        .bind(
            prepared
                .map(|value| value.product.deployment.snapshot_bytes())
                .unwrap_or(empty),
        )
        .bind(
            prepared
                .map(|value| value.product.deployment.snapshot_digest())
                .unwrap_or(""),
        )
        .bind(
            prepared
                .map(|value| value.product.deployment.desired_target_digest())
                .unwrap_or(""),
        )
        .bind(
            prepared
                .map(|value| value.activation_notices_bytes.as_slice())
                .unwrap_or(empty),
        )
        .fetch_one(&mut **transaction)
        .await
}

pub(super) fn prepare_runtime_drain_consumption(
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    digests: &ApplyDigests,
    source: &ValidatedRuntimeDrainConsumptionV2,
    row: &ApplyConsumeRuntimeDrainRow,
) -> Result<PreparedRuntimeDrainConsumptionV2, ProductControlPortError> {
    if !prepare_row_matches(row, request, digests, source)? {
        return Err(invalid_result());
    }
    let product = prepare_product_apply_v1(
        row.locked_product_projection
            .as_ref()
            .ok_or_else(invalid_result)?
            .0
            .clone(),
        request,
        digests,
    )?;
    let source_snapshot = serde_json::from_value::<RuntimeDeploymentSnapshotV1>(
        row.source_deployment_snapshot
            .as_ref()
            .ok_or_else(invalid_result)?
            .0
            .clone(),
    )
    .map_err(|_| invalid_result())?;
    if source_snapshot != source.source_deployment {
        return Err(invalid_result());
    }
    let terminal_database_time = row.terminal_database_time.ok_or_else(invalid_result)?;
    let successor_snapshot = product.deployment.snapshot();
    let source_prepared = prepare_product_drain_source_supersession_v1(
        source_snapshot,
        source.source_deployment.revision,
        source.acknowledged_at()?,
        SupersedingDeploymentV1 {
            identity: successor_snapshot.identity.clone(),
            target: successor_snapshot.target.clone(),
            runtime_generation: successor_snapshot.runtime_generation,
        },
        APPLY_DRAIN_SUPERSESSION_REASON.to_string(),
        terminal_database_time,
    )
    .map_err(|_| invalid_result())?;
    let activation_notices_bytes =
        serde_json::to_vec(&product.activation_notices).map_err(|_| invalid_result())?;
    let source_result_deployment_revision =
        database_revision_input(source_prepared.resulting_revision().get())?;
    let product_deployment_revision =
        database_revision_input(product.deployment.snapshot().revision.get())?;
    let product_resulting_revision = request
        .command()
        .expected_revision
        .get()
        .checked_add(2)
        .ok_or_else(invalid_result)
        .and_then(database_revision_input)?;
    Ok(PreparedRuntimeDrainConsumptionV2 {
        preparation_token: row.preparation_token.clone().ok_or_else(invalid_result)?,
        source: source_prepared,
        product,
        activation_notices_bytes,
        terminal_database_time,
        source_slot_epoch: row.source_slot_epoch.ok_or_else(invalid_result)?,
        source_result_deployment_revision,
        product_deployment_revision,
        product_resulting_revision,
    })
}

pub(super) fn validate_runtime_drain_consumption_result(
    digests: &ApplyDigests,
    source: &ValidatedRuntimeDrainConsumptionV2,
    prepared: &PreparedRuntimeDrainConsumptionV2,
    row: &ApplyConsumeRuntimeDrainRow,
) -> Result<ValidatedRuntimeDrainConsumptionResultV2, ProductControlPortError> {
    let result_revision = database_revision(row.result_intent_revision)?;
    let result_state_bytes = row
        .result_state_bytes
        .as_deref()
        .ok_or_else(invalid_result)?;
    let result_state_digest = row
        .result_state_digest
        .as_deref()
        .ok_or_else(invalid_result)?;
    let canonical = RuntimeCanonicalDrainIntentStateV2::from_persisted(
        &source.root,
        result_revision,
        row.result_intent_state
            .as_deref()
            .ok_or_else(invalid_result)?,
        result_state_bytes,
    )
    .map_err(|_| invalid_result())?;
    let consumption_source = RuntimeDrainConsumptionSourceV2::from_acknowledged(
        source.source.clone(),
        prepared.product.deployment.snapshot().revision,
    )
    .map_err(|_| invalid_result())?;
    RuntimeDrainIntentReceiptV2::consumed(&consumption_source, canonical.intent().clone())
        .map_err(|_| invalid_result())?;
    let source_result = decode_snapshot(row.source_result_deployment_snapshot.as_ref())?;
    let product_result = decode_snapshot(row.result_deployment_snapshot.as_ref())?;
    let expected_product_revision =
        u64::try_from(prepared.product_resulting_revision).map_err(|_| invalid_result())?;
    if row.outcome_name != "applied"
        || row.preparation_ready
        || row.exact_replay
        || row.requires_commit
        || row.preparation_token.is_some()
        || row.locked_product_projection.is_none()
        || !source_snapshot_matches(
            row.source_deployment_snapshot.as_ref(),
            &source.source_deployment,
        )?
        || row.source_acknowledged_at != Some(source.acknowledged_at()?)
        || !source_projection_matches(row, source)
        || row.source_result_deployment_revision != Some(prepared.source_result_deployment_revision)
        || source_result != *prepared.source.snapshot()
        || row.source_result_deployment_snapshot_digest.as_deref()
            != Some(prepared.source.snapshot_digest())
        || row.result_deployment_id.as_deref()
            != Some(
                prepared
                    .product
                    .deployment
                    .snapshot()
                    .identity
                    .deployment_id
                    .as_str(),
            )
        || row.result_deployment_revision != Some(prepared.product_deployment_revision)
        || product_result != *prepared.product.deployment.snapshot()
        || row.result_deployment_snapshot_digest.as_deref()
            != Some(prepared.product.deployment.snapshot_digest())
        || row.product_resulting_revision != Some(prepared.product_resulting_revision)
        || row.product_resulting_state.as_deref() != Some("applied")
        || row.product_receipt_id.as_deref() != Some(digests.receipt_id.as_str())
        || row.product_audit_event_id.as_deref() != Some(digests.audit_event_id.as_str())
        || !digest_matches(result_state_bytes, result_state_digest)
        || row.source_slot_epoch != Some(prepared.source_slot_epoch)
        || row.successor_slot_epoch != prepared.source_slot_epoch.checked_add(1)
        || row.terminal_action_id.as_deref()
            != Some(digests.drain_consume_terminal_action_id.as_str())
        || row.terminal_projection_bytes.is_none()
        || !row
            .terminal_projection_bytes
            .as_deref()
            .zip(row.terminal_projection_digest.as_deref())
            .is_some_and(|(bytes, digest)| digest_matches(bytes, digest))
        || row.terminal_database_time != Some(prepared.terminal_database_time)
        || canonical.intent().state().consumed_at() != Some(prepared.terminal_database_time)
    {
        return Err(invalid_result());
    }
    Ok(ValidatedRuntimeDrainConsumptionResultV2 {
        product_revision: expected_product_revision,
        deployment_id: prepared
            .product
            .deployment
            .snapshot()
            .identity
            .deployment_id
            .as_str()
            .to_string(),
        desired_target_digest: prepared
            .product
            .deployment
            .desired_target_digest()
            .to_string(),
        exact_replay: false,
    })
}

fn prepare_row_matches(
    row: &ApplyConsumeRuntimeDrainRow,
    request: &AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    digests: &ApplyDigests,
    source: &ValidatedRuntimeDrainConsumptionV2,
) -> Result<bool, ProductControlPortError> {
    let terminal_database_time = row.terminal_database_time.ok_or_else(invalid_result)?;
    RuntimeUnixMicrosecondsV2::from_datetime(terminal_database_time)
        .map_err(|_| invalid_result())?;
    let acknowledged_at = source.acknowledged_at()?;
    Ok(row.outcome_name == "drain_pending"
        && row.preparation_ready
        && !row.exact_replay
        && row.requires_commit
        && row
            .preparation_token
            .as_deref()
            .is_some_and(valid_preparation_token)
        && row.locked_product_projection.is_some()
        && row.source_deployment_snapshot.is_some()
        && row.source_acknowledged_at == Some(acknowledged_at)
        && source_projection_matches(row, source)
        && row.source_result_deployment_revision.is_none()
        && row.source_result_deployment_snapshot.is_none()
        && row.source_result_deployment_snapshot_digest.is_none()
        && row.result_deployment_id.is_none()
        && row.result_deployment_revision.is_none()
        && row.result_deployment_snapshot.is_none()
        && row.result_deployment_snapshot_digest.is_none()
        && row.product_resulting_revision.is_none()
        && row.product_resulting_state.is_none()
        && row.product_receipt_id.as_deref() == Some(digests.receipt_id.as_str())
        && row.product_audit_event_id.as_deref() == Some(digests.audit_event_id.as_str())
        && row.result_intent_revision.is_none()
        && row.result_intent_state.is_none()
        && row.result_state_bytes.is_none()
        && row.result_state_digest.is_none()
        && row
            .source_slot_epoch
            .is_some_and(|epoch| epoch > 0 && epoch < i64::MAX)
        && row.successor_slot_epoch.is_none()
        && row.terminal_action_id.as_deref()
            == Some(digests.drain_consume_terminal_action_id.as_str())
        && row.terminal_projection_bytes.is_none()
        && row.terminal_projection_digest.is_none()
        && terminal_database_time >= acknowledged_at
        && request
            .command()
            .expected_revision
            .get()
            .checked_add(2)
            .and_then(|revision| i64::try_from(revision).ok())
            .is_some())
}

fn source_projection_matches(
    row: &ApplyConsumeRuntimeDrainRow,
    source: &ValidatedRuntimeDrainConsumptionV2,
) -> bool {
    row.product_operation_id.as_deref() == Some(source.root.product_operation_id().as_str())
        && row.product_mutation_digest.as_deref()
            == Some(source.root.product_mutation_digest().as_str())
        && row.drain_intent_digest.as_deref() == Some(source.root.drain_intent_digest().as_str())
        && row.source_deployment_id.as_deref()
            == Some(source.source_deployment.identity.deployment_id.as_str())
        && row.source_deployment_revision == Some(source.source_deployment_revision)
        && row.drain_intent_id.as_deref() == Some(source.root.drain_intent_id().as_str())
        && row.source_intent_revision == Some(source.source_intent_revision)
        && row.source_state_bytes.as_deref() == Some(source.source_state_bytes.as_ref())
        && row.source_state_digest.as_deref() == Some(source.source_state_digest.as_str())
}

fn database_revision(value: Option<i64>) -> Result<NonZeroU64, ProductControlPortError> {
    value
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value <= i64::MAX as u64)
        .and_then(NonZeroU64::new)
        .ok_or_else(invalid_result)
}

fn database_revision_input(value: u64) -> Result<i64, ProductControlPortError> {
    i64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(invalid_result)
}

fn decode_snapshot(
    value: Option<&Json<Value>>,
) -> Result<RuntimeDeploymentSnapshotV1, ProductControlPortError> {
    let snapshot = serde_json::from_value::<RuntimeDeploymentSnapshotV1>(
        value.ok_or_else(invalid_result)?.0.clone(),
    )
    .map_err(|_| invalid_result())?;
    RuntimeDeployment::restore(snapshot.clone()).map_err(|_| invalid_result())?;
    Ok(snapshot)
}

fn source_snapshot_matches(
    value: Option<&Json<Value>>,
    expected: &RuntimeDeploymentSnapshotV1,
) -> Result<bool, ProductControlPortError> {
    Ok(decode_snapshot(value)? == *expected)
}

fn valid_preparation_token(value: &str) -> bool {
    value.len() == 67
        && value.starts_with("v2:")
        && value[3..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn digest_matches(bytes: &[u8], expected: &str) -> bool {
    if expected.len() != 64 {
        return false;
    }
    let actual = Sha256::digest(bytes);
    actual.iter().enumerate().all(|(index, byte)| {
        let high = hex_digit(byte >> 4);
        let low = hex_digit(byte & 0x0f);
        expected.as_bytes().get(index * 2) == Some(&high)
            && expected.as_bytes().get(index * 2 + 1) == Some(&low)
    })
}

fn hex_digit(value: u8) -> u8 {
    match value {
        0..=9 => b'0' + value,
        10..=15 => b'a' + value - 10,
        _ => unreachable!(),
    }
}

fn invalid_result() -> ProductControlPortError {
    ProductControlPortError::Backend(
        "product apply runtime drain consume returned an invalid result".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
    use automation_runtime_convergence::{
        ActivationRequestId, BindingRevision, DeploymentId, InstallationId, PromotionId,
        RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1, RuntimeGeneration, TenantId,
    };
    use chrono::{SecondsFormat, TimeZone};
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;

    use super::*;

    fn requested_snapshot() -> RuntimeDeploymentSnapshotV1 {
        RuntimeDeployment::request(
            RuntimeDeploymentIdentityV1 {
                deployment_id: DeploymentId::parse("deployment-utc-equivalence").unwrap(),
                tenant_id: TenantId::parse("tenant-utc-equivalence").unwrap(),
                installation_id: InstallationId::parse("installation-utc-equivalence").unwrap(),
                promotion_id: PromotionId::parse("a".repeat(64)).unwrap(),
                activation_request_id: ActivationRequestId::parse("activation-utc-equivalence")
                    .unwrap(),
            },
            RuntimeDeploymentTargetV1 {
                guild_id: GuildId(42),
                ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
                version: RuleSetVersionId::FIRST,
                content_hash: RuleSetContentHash::parse_hex(&"b".repeat(64)).unwrap(),
                binding_revision: BindingRevision::new(1).unwrap(),
                binding_fingerprint: ResourceBindingFingerprint::parse(&"c".repeat(64)).unwrap(),
            },
            RuntimeGeneration::new(1).unwrap(),
            None,
            Utc.timestamp_opt(1_800_000_000, 123_456_000).unwrap(),
        )
        .unwrap()
        .snapshot()
    }

    #[test]
    fn source_snapshot_validation_accepts_equivalent_utc_spellings() {
        let expected = requested_snapshot();
        let mut value = serde_json::to_value(&expected).unwrap();
        let timestamp = value["requested_at"].as_str().unwrap();
        assert!(timestamp.ends_with('Z'));
        value["requested_at"] =
            Value::String(format!("{}+00:00", timestamp.strip_suffix('Z').unwrap()));
        assert_ne!(value, serde_json::to_value(&expected).unwrap());
        assert!(source_snapshot_matches(Some(&Json(value)), &expected).unwrap());
    }

    #[test]
    fn source_snapshot_validation_rejects_other_instants_and_malformed_values() {
        let expected = requested_snapshot();
        let mut different = serde_json::to_value(&expected).unwrap();
        different["requested_at"] = Value::String(
            Utc.timestamp_opt(1_800_000_001, 123_456_000)
                .unwrap()
                .to_rfc3339_opts(SecondsFormat::Micros, true),
        );
        assert!(!source_snapshot_matches(Some(&Json(different)), &expected).unwrap());
        let mut malformed = serde_json::to_value(&expected).unwrap();
        malformed["requested_at"] = Value::String("invalid".to_string());
        assert!(source_snapshot_matches(Some(&Json(malformed)), &expected).is_err());
    }
}
