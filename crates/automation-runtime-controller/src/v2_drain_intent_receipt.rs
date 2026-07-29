#[cfg(test)]
mod tests;

use std::num::NonZeroU64;

use automation_runtime_convergence::{ControllerId, DeploymentRevision};
use chrono::{DateTime, Utc};

use crate::v2_canonical_value::RuntimePersistenceU64V2;
use crate::{
    RuntimeClosedRecoveryRouteWitnessV2, RuntimeDrainCertificationResolutionKindV2,
    RuntimeDrainCertificationResolutionV2, RuntimeDrainClaimProgressKindV2, RuntimeDrainClaimV2,
    RuntimeDrainIntentStateKindV2, RuntimeDrainIntentV2, RuntimeProductDrainOperationV2,
    RuntimeRouteMutationProvenanceV2, RuntimeUnixMicrosecondsV2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeDrainIntentMutationOutcomeV2 {
    Inserted,
    Replayed,
    Claimed,
    Refenced,
    Acknowledged,
    Consumed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeDrainIntentReceiptErrorV2 {
    #[error("runtime drain-intent receipt does not match its immutable operation")]
    OperationMismatch,
    #[error("runtime inserted drain-intent receipt does not contain its initial mutable state")]
    InitialStateMismatch,
    #[error("runtime drain-intent transition source has the wrong mutable state")]
    SourceStateMismatch,
    #[error("runtime drain-intent transition result has the wrong mutable state")]
    ResultStateMismatch,
    #[error("runtime drain-intent transition changed its immutable roots")]
    ImmutableRootMismatch,
    #[error("runtime drain-intent revision is not the exact successor")]
    IntentRevisionNotNewer,
    #[error("runtime drain-claim replay changed its persisted aggregate")]
    ClaimReplayMismatch,
    #[error("runtime drain-claim transition changed its immutable identity")]
    ClaimIdentityMismatch,
    #[error("runtime drain-claim revision is not the exact successor")]
    ClaimRevisionNotNewer,
    #[error("runtime drain-claim transition changed its sealed progress basis")]
    ClaimProgressMismatch,
    #[error("runtime route-absence acknowledgement does not contain the exact source claim")]
    AcknowledgementMismatch,
    #[error("runtime drain succession predecessor is not expired at database time")]
    SuccessionPredecessorNotExpired,
    #[error("runtime drain succession database time is not canonical")]
    SuccessionDatabaseTimeInvalid,
    #[error("runtime drain succession process is not distinct from its predecessor")]
    SuccessionProcessNotDistinct,
    #[error("runtime drain succession owner identity is internally inconsistent")]
    SuccessionOwnerMismatch,
    #[error("runtime drain succession recovery generation is not the exact emergency successor")]
    SuccessionRecoveryGenerationMismatch,
    #[error("runtime drain succession pause does not follow its connected event")]
    SuccessionPauseSequenceMismatch,
    #[error("runtime drain succession gateway shard changed")]
    SuccessionShardMismatch,
    #[error("runtime drain succession owner lease epoch is not strictly newer")]
    SuccessionOwnerEpochNotNewer,
    #[error("runtime drain succession owner lease is not current at database time")]
    SuccessionOwnerExpired,
    #[error("runtime drain succession result is not the exact intent-revision successor")]
    SuccessionIntentRevisionMismatch,
    #[error("runtime drain succession result has the wrong claim identity")]
    SuccessionClaimMismatch,
    #[error("runtime drain succession claim revision is not the exact successor")]
    SuccessionClaimRevisionMismatch,
    #[error("runtime drain succession controller fence is not the exact successor")]
    SuccessionFenceMismatch,
    #[error("runtime drain succession seal does not match current recovery evidence")]
    SuccessionSealMismatch,
    #[error("runtime drain succession acknowledgement does not match current recovery evidence")]
    SuccessionAcknowledgementMismatch,
    #[error("runtime drain succession certification is not eligible for direct acknowledgement")]
    SuccessionCertificationMismatch,
    #[error("runtime drain terminal result is not the exact intent-revision successor")]
    TerminalIntentRevisionMismatch,
    #[error("runtime drain consumption resulting deployment revision does not match")]
    ConsumptionResultingRevisionMismatch,
    #[error("runtime drain consumption resulting deployment revision is not persistable")]
    ConsumptionResultingRevisionInvalid,
    #[error("runtime drain terminal timestamp is not canonical")]
    TerminalTimestampInvalid,
    #[error("runtime drain cancellation timestamp does not match")]
    CancellationTimestampMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDrainRefenceSourceV2 {
    source: RuntimeDrainIntentV2,
}

impl RuntimeDrainRefenceSourceV2 {
    pub fn from_claimed(
        source: RuntimeDrainIntentV2,
    ) -> Result<Self, RuntimeDrainIntentReceiptErrorV2> {
        let claim = pending_claimed(&source)?;
        if claim.progress().seal().expected_route().is_none() {
            return Err(RuntimeDrainIntentReceiptErrorV2::SourceStateMismatch);
        }
        Ok(Self { source })
    }

    pub fn source(&self) -> &RuntimeDrainIntentV2 {
        &self.source
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDrainAcknowledgementSourceV2 {
    source: RuntimeDrainIntentV2,
}

impl RuntimeDrainAcknowledgementSourceV2 {
    pub fn from_route_absence_candidate(
        source: RuntimeDrainIntentV2,
    ) -> Result<Self, RuntimeDrainIntentReceiptErrorV2> {
        let claim = pending_claim(&source)?;
        let valid = match claim.progress().kind() {
            RuntimeDrainClaimProgressKindV2::Claimed => {
                claim.progress().seal().expected_route().is_none()
            }
            RuntimeDrainClaimProgressKindV2::Refenced => true,
        };
        if !valid {
            return Err(RuntimeDrainIntentReceiptErrorV2::SourceStateMismatch);
        }
        Ok(Self { source })
    }

    pub fn source(&self) -> &RuntimeDrainIntentV2 {
        &self.source
    }

    pub fn from_refenced(
        source: RuntimeDrainIntentV2,
    ) -> Result<Self, RuntimeDrainIntentReceiptErrorV2> {
        if pending_claim(&source)?.progress().kind() != RuntimeDrainClaimProgressKindV2::Refenced {
            return Err(RuntimeDrainIntentReceiptErrorV2::SourceStateMismatch);
        }
        Ok(Self { source })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDrainSuccessionAcknowledgementExpectationV2 {
    pub database_now: DateTime<Utc>,
    pub recovery_witness: RuntimeClosedRecoveryRouteWitnessV2,
    pub controller_id: ControllerId,
    pub seal_generation: NonZeroU64,
    pub seal_observation_sequence: NonZeroU64,
    pub acknowledgement_observation_sequence: NonZeroU64,
    pub certification: RuntimeDrainCertificationResolutionV2,
    pub acknowledged_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDrainSuccessionAcknowledgementSourceV2 {
    source: RuntimeDrainIntentV2,
    expectation: RuntimeDrainSuccessionAcknowledgementExpectationV2,
}

impl RuntimeDrainSuccessionAcknowledgementSourceV2 {
    pub fn from_expired_route_absent_claimed(
        source: RuntimeDrainIntentV2,
        expectation: RuntimeDrainSuccessionAcknowledgementExpectationV2,
    ) -> Result<Self, RuntimeDrainIntentReceiptErrorV2> {
        let predecessor = pending_claimed(&source)?;
        if predecessor.progress().seal().expected_route().is_some() {
            return Err(RuntimeDrainIntentReceiptErrorV2::SourceStateMismatch);
        }
        validate_succession_owner(predecessor, &expectation)?;
        if expectation.certification.kind()
            == RuntimeDrainCertificationResolutionKindV2::CommittedAndDisconnected
        {
            return Err(RuntimeDrainIntentReceiptErrorV2::SuccessionCertificationMismatch);
        }
        Ok(Self {
            source,
            expectation,
        })
    }

    pub fn source(&self) -> &RuntimeDrainIntentV2 {
        &self.source
    }

    pub fn expectation(&self) -> &RuntimeDrainSuccessionAcknowledgementExpectationV2 {
        &self.expectation
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeRouteAbsentDrainIntentSourceV2 {
    source: RuntimeDrainIntentV2,
}

impl RuntimeRouteAbsentDrainIntentSourceV2 {
    pub fn from_acknowledged(
        source: RuntimeDrainIntentV2,
    ) -> Result<Self, RuntimeDrainIntentReceiptErrorV2> {
        if source.state().kind() != RuntimeDrainIntentStateKindV2::RouteAbsentAcknowledged
            || source.state().acknowledgement().is_none()
        {
            return Err(RuntimeDrainIntentReceiptErrorV2::SourceStateMismatch);
        }
        Ok(Self { source })
    }

    pub fn source(&self) -> &RuntimeDrainIntentV2 {
        &self.source
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDrainConsumptionSourceV2 {
    source: RuntimeRouteAbsentDrainIntentSourceV2,
    expected_resulting_revision: DeploymentRevision,
}

impl RuntimeDrainConsumptionSourceV2 {
    pub fn from_acknowledged(
        source: RuntimeRouteAbsentDrainIntentSourceV2,
        expected_resulting_revision: DeploymentRevision,
    ) -> Result<Self, RuntimeDrainIntentReceiptErrorV2> {
        RuntimePersistenceU64V2::from_u64(expected_resulting_revision.get())
            .map_err(|_| RuntimeDrainIntentReceiptErrorV2::ConsumptionResultingRevisionInvalid)?;
        Ok(Self {
            source,
            expected_resulting_revision,
        })
    }

    pub fn source(&self) -> &RuntimeDrainIntentV2 {
        self.source.source()
    }

    pub fn expected_resulting_revision(&self) -> DeploymentRevision {
        self.expected_resulting_revision
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDrainCancellationSourceV2 {
    source: RuntimeRouteAbsentDrainIntentSourceV2,
    cancelled_at: RuntimeUnixMicrosecondsV2,
}

impl RuntimeDrainCancellationSourceV2 {
    pub fn from_acknowledged(
        source: RuntimeRouteAbsentDrainIntentSourceV2,
        cancelled_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeDrainIntentReceiptErrorV2> {
        let cancelled_at = RuntimeUnixMicrosecondsV2::from_datetime(cancelled_at)
            .map_err(|_| RuntimeDrainIntentReceiptErrorV2::TerminalTimestampInvalid)?;
        Ok(Self {
            source,
            cancelled_at,
        })
    }

    pub fn source(&self) -> &RuntimeDrainIntentV2 {
        self.source.source()
    }

    pub fn cancelled_at(&self) -> DateTime<Utc> {
        self.cancelled_at.to_datetime()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDrainIntentReceiptV2 {
    outcome: RuntimeDrainIntentMutationOutcomeV2,
    intent: RuntimeDrainIntentV2,
}

impl RuntimeDrainIntentReceiptV2 {
    pub fn inserted(
        operation: &RuntimeProductDrainOperationV2,
        persisted_intent: RuntimeDrainIntentV2,
    ) -> Result<Self, RuntimeDrainIntentReceiptErrorV2> {
        validate_operation(operation, &persisted_intent)?;
        if persisted_intent.state().kind() != RuntimeDrainIntentStateKindV2::Pending
            || persisted_intent.state().pending_claim().is_some()
        {
            return Err(RuntimeDrainIntentReceiptErrorV2::InitialStateMismatch);
        }
        Ok(Self::from_result(
            RuntimeDrainIntentMutationOutcomeV2::Inserted,
            persisted_intent,
        ))
    }

    pub fn replayed(
        proposed_operation: &RuntimeProductDrainOperationV2,
        persisted_intent: RuntimeDrainIntentV2,
    ) -> Result<Self, RuntimeDrainIntentReceiptErrorV2> {
        validate_operation(proposed_operation, &persisted_intent)?;
        Ok(Self::from_result(
            RuntimeDrainIntentMutationOutcomeV2::Replayed,
            persisted_intent,
        ))
    }

    pub fn claim_replayed(
        source: RuntimeDrainIntentV2,
        persisted_intent: RuntimeDrainIntentV2,
    ) -> Result<Self, RuntimeDrainIntentReceiptErrorV2> {
        pending_claimed(&source)?;
        if persisted_intent != source {
            return Err(RuntimeDrainIntentReceiptErrorV2::ClaimReplayMismatch);
        }
        Ok(Self::from_result(
            RuntimeDrainIntentMutationOutcomeV2::Claimed,
            persisted_intent,
        ))
    }

    pub fn refenced(
        source: &RuntimeDrainRefenceSourceV2,
        persisted_intent: RuntimeDrainIntentV2,
    ) -> Result<Self, RuntimeDrainIntentReceiptErrorV2> {
        validate_immutable_roots(source.source(), &persisted_intent)?;
        let source_claim = pending_claimed(source.source())?;
        let result_claim = pending_refenced(&persisted_intent)?;
        if !is_exact_successor(
            source.source().intent_revision().get(),
            persisted_intent.intent_revision().get(),
        ) {
            return Err(RuntimeDrainIntentReceiptErrorV2::IntentRevisionNotNewer);
        }
        if !claim_identity_matches(source_claim, result_claim) {
            return Err(RuntimeDrainIntentReceiptErrorV2::ClaimIdentityMismatch);
        }
        if !is_exact_successor(
            source_claim.claim_revision().get(),
            result_claim.claim_revision().get(),
        ) {
            return Err(RuntimeDrainIntentReceiptErrorV2::ClaimRevisionNotNewer);
        }
        if result_claim.progress().seal() != source_claim.progress().seal() {
            return Err(RuntimeDrainIntentReceiptErrorV2::ClaimProgressMismatch);
        }
        Ok(Self::from_result(
            RuntimeDrainIntentMutationOutcomeV2::Refenced,
            persisted_intent,
        ))
    }

    pub fn acknowledged(
        source: &RuntimeDrainAcknowledgementSourceV2,
        persisted_intent: RuntimeDrainIntentV2,
    ) -> Result<Self, RuntimeDrainIntentReceiptErrorV2> {
        validate_immutable_roots(source.source(), &persisted_intent)?;
        let source_claim = pending_claim(source.source())?;
        let acknowledgement = persisted_intent
            .state()
            .acknowledgement()
            .ok_or(RuntimeDrainIntentReceiptErrorV2::ResultStateMismatch)?;
        if !is_exact_successor(
            source.source().intent_revision().get(),
            persisted_intent.intent_revision().get(),
        ) {
            return Err(RuntimeDrainIntentReceiptErrorV2::IntentRevisionNotNewer);
        }
        if acknowledgement.claim() != source_claim {
            return Err(RuntimeDrainIntentReceiptErrorV2::AcknowledgementMismatch);
        }
        Ok(Self::from_result(
            RuntimeDrainIntentMutationOutcomeV2::Acknowledged,
            persisted_intent,
        ))
    }

    pub fn succession_acknowledged(
        source: &RuntimeDrainSuccessionAcknowledgementSourceV2,
        persisted_intent: RuntimeDrainIntentV2,
    ) -> Result<Self, RuntimeDrainIntentReceiptErrorV2> {
        validate_immutable_roots(source.source(), &persisted_intent)?;
        let predecessor = pending_claimed(source.source())?;
        let acknowledgement = persisted_intent
            .state()
            .acknowledgement()
            .ok_or(RuntimeDrainIntentReceiptErrorV2::ResultStateMismatch)?;
        let expectation = source.expectation();
        if !is_exact_successor(
            source.source().intent_revision().get(),
            persisted_intent.intent_revision().get(),
        ) {
            return Err(RuntimeDrainIntentReceiptErrorV2::SuccessionIntentRevisionMismatch);
        }
        validate_succession_acknowledgement_v2(predecessor, acknowledgement, expectation)?;
        Ok(Self::from_result(
            RuntimeDrainIntentMutationOutcomeV2::Acknowledged,
            persisted_intent,
        ))
    }

    pub fn consumed(
        source: &RuntimeDrainConsumptionSourceV2,
        persisted_intent: RuntimeDrainIntentV2,
    ) -> Result<Self, RuntimeDrainIntentReceiptErrorV2> {
        validate_immutable_roots(source.source(), &persisted_intent)?;
        if !is_exact_successor(
            source.source().intent_revision().get(),
            persisted_intent.intent_revision().get(),
        ) {
            return Err(RuntimeDrainIntentReceiptErrorV2::TerminalIntentRevisionMismatch);
        }
        if persisted_intent.state().kind() != RuntimeDrainIntentStateKindV2::Consumed {
            return Err(RuntimeDrainIntentReceiptErrorV2::ResultStateMismatch);
        }
        validate_terminal_timestamp(persisted_intent.state().consumed_at())?;
        if persisted_intent.state().resulting_revision()
            != Some(source.expected_resulting_revision())
        {
            return Err(RuntimeDrainIntentReceiptErrorV2::ConsumptionResultingRevisionMismatch);
        }
        Ok(Self::from_result(
            RuntimeDrainIntentMutationOutcomeV2::Consumed,
            persisted_intent,
        ))
    }

    pub fn cancelled(
        source: &RuntimeDrainCancellationSourceV2,
        persisted_intent: RuntimeDrainIntentV2,
    ) -> Result<Self, RuntimeDrainIntentReceiptErrorV2> {
        validate_immutable_roots(source.source(), &persisted_intent)?;
        if !is_exact_successor(
            source.source().intent_revision().get(),
            persisted_intent.intent_revision().get(),
        ) {
            return Err(RuntimeDrainIntentReceiptErrorV2::TerminalIntentRevisionMismatch);
        }
        if persisted_intent.state().kind() != RuntimeDrainIntentStateKindV2::Cancelled {
            return Err(RuntimeDrainIntentReceiptErrorV2::ResultStateMismatch);
        }
        let cancelled_at = validate_terminal_timestamp(persisted_intent.state().cancelled_at())?;
        if cancelled_at != source.cancelled_at {
            return Err(RuntimeDrainIntentReceiptErrorV2::CancellationTimestampMismatch);
        }
        Ok(Self::from_result(
            RuntimeDrainIntentMutationOutcomeV2::Cancelled,
            persisted_intent,
        ))
    }

    pub fn outcome(&self) -> RuntimeDrainIntentMutationOutcomeV2 {
        self.outcome
    }

    pub fn intent(&self) -> &RuntimeDrainIntentV2 {
        &self.intent
    }

    fn from_result(
        outcome: RuntimeDrainIntentMutationOutcomeV2,
        intent: RuntimeDrainIntentV2,
    ) -> Self {
        Self { outcome, intent }
    }
}

fn validate_operation(
    operation: &RuntimeProductDrainOperationV2,
    intent: &RuntimeDrainIntentV2,
) -> Result<(), RuntimeDrainIntentReceiptErrorV2> {
    if operation.canonical() == intent.canonical() {
        Ok(())
    } else {
        Err(RuntimeDrainIntentReceiptErrorV2::OperationMismatch)
    }
}

fn validate_immutable_roots(
    source: &RuntimeDrainIntentV2,
    result: &RuntimeDrainIntentV2,
) -> Result<(), RuntimeDrainIntentReceiptErrorV2> {
    if source.canonical() == result.canonical() {
        Ok(())
    } else {
        Err(RuntimeDrainIntentReceiptErrorV2::ImmutableRootMismatch)
    }
}

fn validate_terminal_timestamp(
    value: Option<DateTime<Utc>>,
) -> Result<RuntimeUnixMicrosecondsV2, RuntimeDrainIntentReceiptErrorV2> {
    value
        .ok_or(RuntimeDrainIntentReceiptErrorV2::ResultStateMismatch)
        .and_then(|value| {
            RuntimeUnixMicrosecondsV2::from_datetime(value)
                .map_err(|_| RuntimeDrainIntentReceiptErrorV2::TerminalTimestampInvalid)
        })
}

fn pending_claim(
    intent: &RuntimeDrainIntentV2,
) -> Result<&RuntimeDrainClaimV2, RuntimeDrainIntentReceiptErrorV2> {
    if intent.state().kind() != RuntimeDrainIntentStateKindV2::Pending {
        return Err(RuntimeDrainIntentReceiptErrorV2::SourceStateMismatch);
    }
    intent
        .state()
        .pending_claim()
        .ok_or(RuntimeDrainIntentReceiptErrorV2::SourceStateMismatch)
}

fn pending_claimed(
    intent: &RuntimeDrainIntentV2,
) -> Result<&RuntimeDrainClaimV2, RuntimeDrainIntentReceiptErrorV2> {
    let claim = pending_claim(intent)?;
    if claim.progress().kind() != RuntimeDrainClaimProgressKindV2::Claimed {
        return Err(RuntimeDrainIntentReceiptErrorV2::SourceStateMismatch);
    }
    Ok(claim)
}

fn pending_refenced(
    intent: &RuntimeDrainIntentV2,
) -> Result<&RuntimeDrainClaimV2, RuntimeDrainIntentReceiptErrorV2> {
    if intent.state().kind() != RuntimeDrainIntentStateKindV2::Pending {
        return Err(RuntimeDrainIntentReceiptErrorV2::ResultStateMismatch);
    }
    let claim = intent
        .state()
        .pending_claim()
        .ok_or(RuntimeDrainIntentReceiptErrorV2::ResultStateMismatch)?;
    if claim.progress().kind() != RuntimeDrainClaimProgressKindV2::Refenced {
        return Err(RuntimeDrainIntentReceiptErrorV2::ResultStateMismatch);
    }
    Ok(claim)
}

fn claim_identity_matches(source: &RuntimeDrainClaimV2, result: &RuntimeDrainClaimV2) -> bool {
    source.gateway_owner_lease_id() == result.gateway_owner_lease_id()
        && source.observed_owner_revision() == result.observed_owner_revision()
        && source.process_instance_id() == result.process_instance_id()
        && source.controller_id() == result.controller_id()
        && source.controller_fencing_token() == result.controller_fencing_token()
        && source.claim_epoch() == result.claim_epoch()
        && source.expires_at() == result.expires_at()
}

fn validate_succession_owner(
    predecessor: &RuntimeDrainClaimV2,
    expectation: &RuntimeDrainSuccessionAcknowledgementExpectationV2,
) -> Result<(), RuntimeDrainIntentReceiptErrorV2> {
    let witness = &expectation.recovery_witness;
    if RuntimeUnixMicrosecondsV2::from_datetime(expectation.database_now).is_err() {
        return Err(RuntimeDrainIntentReceiptErrorV2::SuccessionDatabaseTimeInvalid);
    }
    if expectation.database_now < predecessor.expires_at() {
        return Err(RuntimeDrainIntentReceiptErrorV2::SuccessionPredecessorNotExpired);
    }
    if witness.gateway_owner_lease_id.process_instance_id != witness.process_instance_id {
        return Err(RuntimeDrainIntentReceiptErrorV2::SuccessionOwnerMismatch);
    }
    if witness
        .originating_emergency_generation
        .get()
        .checked_add(1)
        != Some(witness.recovery_generation.get())
    {
        return Err(RuntimeDrainIntentReceiptErrorV2::SuccessionRecoveryGenerationMismatch);
    }
    if witness.pause_sequence.get() <= witness.connected_event_sequence.get() {
        return Err(RuntimeDrainIntentReceiptErrorV2::SuccessionPauseSequenceMismatch);
    }
    if predecessor.process_instance_id() == &witness.process_instance_id {
        return Err(RuntimeDrainIntentReceiptErrorV2::SuccessionProcessNotDistinct);
    }
    if predecessor.gateway_owner_lease_id().gateway_shard_id
        != witness.gateway_owner_lease_id.gateway_shard_id
    {
        return Err(RuntimeDrainIntentReceiptErrorV2::SuccessionShardMismatch);
    }
    if witness.gateway_owner_lease_id.lease_epoch
        <= predecessor.gateway_owner_lease_id().lease_epoch
    {
        return Err(RuntimeDrainIntentReceiptErrorV2::SuccessionOwnerEpochNotNewer);
    }
    if expectation.database_now >= witness.owner_expires_at {
        return Err(RuntimeDrainIntentReceiptErrorV2::SuccessionOwnerExpired);
    }
    Ok(())
}

fn validate_succession_claim(
    predecessor: &RuntimeDrainClaimV2,
    successor: &RuntimeDrainClaimV2,
    expectation: &RuntimeDrainSuccessionAcknowledgementExpectationV2,
) -> Result<(), RuntimeDrainIntentReceiptErrorV2> {
    let witness = &expectation.recovery_witness;
    if successor.progress().kind() != RuntimeDrainClaimProgressKindV2::Claimed
        || successor.gateway_owner_lease_id() != &witness.gateway_owner_lease_id
        || successor.observed_owner_revision() != witness.observed_owner_revision
        || successor.process_instance_id() != &witness.process_instance_id
        || successor.controller_id() != &expectation.controller_id
        || successor.claim_epoch() != witness.recovery_generation
        || successor.expires_at() != witness.owner_expires_at
    {
        return Err(RuntimeDrainIntentReceiptErrorV2::SuccessionClaimMismatch);
    }
    if !is_exact_successor(
        predecessor.claim_revision().get(),
        successor.claim_revision().get(),
    ) {
        return Err(RuntimeDrainIntentReceiptErrorV2::SuccessionClaimRevisionMismatch);
    }
    if !is_exact_successor(
        predecessor.controller_fencing_token().get(),
        successor.controller_fencing_token().get(),
    ) {
        return Err(RuntimeDrainIntentReceiptErrorV2::SuccessionFenceMismatch);
    }
    let seal = successor.progress().seal();
    if seal.process_instance_id() != &witness.process_instance_id
        || seal.seal_generation() != expectation.seal_generation
        || seal.registry_observation_sequence() != expectation.seal_observation_sequence
        || seal.expected_route().is_some()
    {
        return Err(RuntimeDrainIntentReceiptErrorV2::SuccessionSealMismatch);
    }
    Ok(())
}

pub(crate) fn validate_succession_acknowledgement_v2(
    predecessor: &RuntimeDrainClaimV2,
    acknowledgement: &crate::RuntimeRouteAbsentAcknowledgementV2,
    expectation: &RuntimeDrainSuccessionAcknowledgementExpectationV2,
) -> Result<(), RuntimeDrainIntentReceiptErrorV2> {
    validate_succession_owner(predecessor, expectation)?;
    validate_succession_claim(predecessor, acknowledgement.claim(), expectation)?;
    let expected_provenance =
        RuntimeRouteMutationProvenanceV2::ClosedRecovery(expectation.recovery_witness.clone());
    if acknowledgement.expected_route().is_some()
        || acknowledgement.provenance() != &expected_provenance
        || acknowledgement.registry_observation_sequence()
            != expectation.acknowledgement_observation_sequence
        || acknowledgement.acknowledged_at() != expectation.acknowledged_at
    {
        return Err(RuntimeDrainIntentReceiptErrorV2::SuccessionAcknowledgementMismatch);
    }
    if acknowledgement.certification() != &expectation.certification
        || acknowledgement.certification().kind()
            == RuntimeDrainCertificationResolutionKindV2::CommittedAndDisconnected
    {
        return Err(RuntimeDrainIntentReceiptErrorV2::SuccessionCertificationMismatch);
    }
    Ok(())
}

fn is_exact_successor(current: u64, successor: u64) -> bool {
    current.checked_add(1) == Some(successor)
}
