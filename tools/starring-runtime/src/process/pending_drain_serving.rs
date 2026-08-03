use automation_runtime_controller::RuntimeServingSlotV2;
pub(super) use automation_runtime_controller::{RuntimeServingIdentityV2, RuntimeServingReceiptV2};
use automation_runtime_serving_postgres::{
    RuntimePendingDrainServingObservationV1, RuntimePendingDrainServingSourceEvidenceV1,
};
use automation_runtime_worker::{
    RuntimeCompletedStartupRecoveryExecutionV2, RuntimePendingDrainCompoundErrorV2,
    RuntimePendingDrainServingSourceCorrelationV3, RuntimePendingDrainStateDigestV2,
    RuntimeSelectedPendingDrainCandidateV2,
};

pub(super) enum RuntimeUnclaimedPendingDrainServingClassificationV1 {
    Deferred(Box<RuntimeCompletedStartupRecoveryExecutionV2>),
    Ready(Box<RuntimeSelectedPendingDrainCandidateV2>),
    Disconnect(Box<RuntimeCheckedPendingDrainExpiredServingV1>),
}

pub(crate) struct RuntimeCheckedPendingDrainExpiredServingV1 {
    selection: Box<RuntimeSelectedPendingDrainCandidateV2>,
    serving: Box<RuntimeServingReceiptV2>,
}

impl RuntimeCheckedPendingDrainExpiredServingV1 {
    fn new(
        selection: Box<RuntimeSelectedPendingDrainCandidateV2>,
        source: &RuntimePendingDrainServingSourceEvidenceV1,
        serving: Box<RuntimeServingReceiptV2>,
        observed_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Self, RuntimePendingDrainCompoundErrorV2> {
        validate_pending_drain_serving_binding_v1(&selection, source, &serving, observed_at, true)?;
        Ok(Self { selection, serving })
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        Box<RuntimeSelectedPendingDrainCandidateV2>,
        Box<RuntimeServingReceiptV2>,
    ) {
        (self.selection, self.serving)
    }
}

pub(super) fn classify_unclaimed_pending_drain_serving_v1(
    selection: Box<RuntimeSelectedPendingDrainCandidateV2>,
    observation: RuntimePendingDrainServingObservationV1,
) -> Result<RuntimeUnclaimedPendingDrainServingClassificationV1, RuntimePendingDrainCompoundErrorV2>
{
    match observation {
        RuntimePendingDrainServingObservationV1::Fresh {
            source,
            serving,
            observed_at,
        } => {
            let source = pending_drain_serving_source_correlation_v1(&selection, &source)?;
            let evidence = selection.check_fresh_serving_v3(source, *serving, observed_at)?;
            selection
                .defer_for_fresh_serving_v3(evidence)
                .map(Box::new)
                .map(RuntimeUnclaimedPendingDrainServingClassificationV1::Deferred)
        }
        RuntimePendingDrainServingObservationV1::Expired {
            source,
            serving,
            observed_at,
        } => RuntimeCheckedPendingDrainExpiredServingV1::new(
            selection,
            &source,
            serving,
            observed_at,
        )
        .map(|evidence| {
            RuntimeUnclaimedPendingDrainServingClassificationV1::Disconnect(Box::new(evidence))
        }),
        RuntimePendingDrainServingObservationV1::Disconnected {
            source,
            serving,
            observed_at,
        } => {
            validate_pending_drain_serving_binding_v1(
                &selection,
                &source,
                &serving,
                observed_at,
                false,
            )?;
            Ok(RuntimeUnclaimedPendingDrainServingClassificationV1::Ready(
                selection,
            ))
        }
        RuntimePendingDrainServingObservationV1::Absent { .. }
        | RuntimePendingDrainServingObservationV1::Diverged { .. } => {
            Err(RuntimePendingDrainCompoundErrorV2::ServingEvidenceMismatch)
        }
    }
}

fn pending_drain_serving_source_correlation_v1(
    selection: &RuntimeSelectedPendingDrainCandidateV2,
    source: &RuntimePendingDrainServingSourceEvidenceV1,
) -> Result<RuntimePendingDrainServingSourceCorrelationV3, RuntimePendingDrainCompoundErrorV2> {
    let digest = RuntimePendingDrainStateDigestV2::new(*source.source_state_digest())
        .map_err(|_| RuntimePendingDrainCompoundErrorV2::ServingEvidenceMismatch)?;
    if source.intent_id() != selection.candidate().intent_id()
        || source.source_intent_revision() != selection.candidate().source_intent_revision()
        || &digest != selection.candidate().source_state_digest()
    {
        return Err(RuntimePendingDrainCompoundErrorV2::ServingEvidenceMismatch);
    }
    Ok(RuntimePendingDrainServingSourceCorrelationV3::new(
        source.intent_id().clone(),
        source.source_intent_revision(),
        digest,
    ))
}

fn validate_pending_drain_serving_binding_v1(
    selection: &RuntimeSelectedPendingDrainCandidateV2,
    source: &RuntimePendingDrainServingSourceEvidenceV1,
    serving: &RuntimeServingReceiptV2,
    observed_at: chrono::DateTime<chrono::Utc>,
    connected: bool,
) -> Result<(), RuntimePendingDrainCompoundErrorV2> {
    pending_drain_serving_source_correlation_v1(selection, source)?;
    let candidate = selection.candidate();
    let target = &serving.identity.process_identity.target;
    if target != candidate.expected_target()
        || RuntimeServingSlotV2::from_target(target) != *candidate.slot()
    {
        return Err(RuntimePendingDrainCompoundErrorV2::ServingEvidenceMismatch);
    }
    if observed_at < selection.selection_owner_receipt().database_now
        || serving.connected != connected
        || serving.serving != connected
        || serving.acquired_at > serving.last_heartbeat_at
        || serving.last_heartbeat_at > observed_at
        || serving.last_heartbeat_at > serving.expires_at
    {
        return Err(RuntimePendingDrainCompoundErrorV2::ServingClassificationMismatch);
    }
    if connected && serving.expires_at > observed_at {
        return Err(RuntimePendingDrainCompoundErrorV2::ServingClassificationMismatch);
    }
    if !connected && serving.last_heartbeat_at != serving.expires_at {
        return Err(RuntimePendingDrainCompoundErrorV2::ServingClassificationMismatch);
    }
    Ok(())
}

pub(super) fn validate_pending_drain_serving_disconnect_v1(
    source: &RuntimeServingReceiptV2,
    disconnected: &RuntimeServingReceiptV2,
) -> Result<(), RuntimePendingDrainCompoundErrorV2> {
    let source_identity = &source.identity;
    let disconnected_identity = &disconnected.identity;
    let next_revision = source_identity
        .revision
        .get()
        .checked_add(1)
        .and_then(std::num::NonZeroU64::new)
        .ok_or(RuntimePendingDrainCompoundErrorV2::ServingEvidenceMismatch)?;
    if source_identity.scope != disconnected_identity.scope
        || source_identity.operation_id != disconnected_identity.operation_id
        || source_identity.attestation_digest != disconnected_identity.attestation_digest
        || source_identity.process_identity != disconnected_identity.process_identity
        || source_identity.lease_epoch != disconnected_identity.lease_epoch
        || disconnected_identity.revision != next_revision
        || source.acquired_at != disconnected.acquired_at
        || !source.connected
        || !source.serving
        || disconnected.connected
        || disconnected.serving
        || disconnected.last_heartbeat_at != disconnected.expires_at
        || disconnected.last_heartbeat_at < source.last_heartbeat_at
        || disconnected.expires_at < source.expires_at
    {
        return Err(RuntimePendingDrainCompoundErrorV2::ServingEvidenceMismatch);
    }
    Ok(())
}

pub(super) fn validate_pending_drain_disconnected_reobservation_v1(
    selection: &RuntimeSelectedPendingDrainCandidateV2,
    source: &RuntimeServingReceiptV2,
    expected: Option<&RuntimeServingReceiptV2>,
    observation: RuntimePendingDrainServingObservationV1,
) -> Result<(), RuntimePendingDrainCompoundErrorV2> {
    let RuntimePendingDrainServingObservationV1::Disconnected {
        source: observed_source,
        serving,
        observed_at,
    } = observation
    else {
        return Err(RuntimePendingDrainCompoundErrorV2::ServingEvidenceMismatch);
    };
    validate_pending_drain_serving_binding_v1(
        selection,
        &observed_source,
        &serving,
        observed_at,
        false,
    )?;
    validate_pending_drain_serving_disconnect_v1(source, &serving)?;
    if expected.is_some_and(|expected| expected != &*serving) {
        return Err(RuntimePendingDrainCompoundErrorV2::ServingEvidenceMismatch);
    }
    Ok(())
}
