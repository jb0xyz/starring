#[cfg(test)]
mod tests;

use crate::{
    RuntimeDrainClaimProgressKindV2, RuntimeDrainClaimV2, RuntimeDrainIntentStateKindV2,
    RuntimeDrainIntentV2, RuntimeProductDrainOperationV2,
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
    #[error("runtime drain-intent revision is not strictly newer")]
    IntentRevisionNotNewer,
    #[error("runtime drain-claim replay changed its persisted aggregate")]
    ClaimReplayMismatch,
    #[error("runtime drain-claim transition changed its immutable identity")]
    ClaimIdentityMismatch,
    #[error("runtime drain-claim revision is not strictly newer")]
    ClaimRevisionNotNewer,
    #[error("runtime drain-claim transition changed its sealed progress basis")]
    ClaimProgressMismatch,
    #[error("runtime route-absence acknowledgement does not contain the exact source claim")]
    AcknowledgementMismatch,
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
        if persisted_intent.intent_revision() <= source.source().intent_revision() {
            return Err(RuntimeDrainIntentReceiptErrorV2::IntentRevisionNotNewer);
        }
        if !claim_identity_matches(source_claim, result_claim) {
            return Err(RuntimeDrainIntentReceiptErrorV2::ClaimIdentityMismatch);
        }
        if result_claim.claim_revision() <= source_claim.claim_revision() {
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
        if persisted_intent.intent_revision() <= source.source().intent_revision() {
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
