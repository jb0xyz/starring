#[cfg(test)]
mod tests;

use std::num::NonZeroU64;

use automation_runtime_convergence::DeploymentRevision;
use chrono::{DateTime, Utc};

use crate::v2_canonical_value::RuntimePersistenceU64V2;
use crate::v2_drain_claim::{
    validate_drain_claim_for_key, validate_route_absent_acknowledgement_for_key,
};
use crate::{
    RuntimeCanonicalProductDrainV2, RuntimeCanonicalValueErrorV2, RuntimeDrainClaimErrorV2,
    RuntimeDrainClaimV2, RuntimeDrainIntentDigestV2, RuntimeDrainIntentKeyV2,
    RuntimePersistedProductDrainRootV2, RuntimeProductDrainOperationV2,
    RuntimeProductMutationDigestV2, RuntimeRouteAbsentAcknowledgementV2, RuntimeUnixMicrosecondsV2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeDrainIntentStateKindV2 {
    Pending,
    RouteAbsentAcknowledged,
    Consumed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeDrainIntentStateFieldV2 {
    IntentRevision,
    ResultingRevision,
    ConsumedAt,
    CancelledAt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeDrainIntentStateErrorV2 {
    #[error("runtime drain-intent field {field:?} is invalid: {reason}")]
    CanonicalValue {
        field: RuntimeDrainIntentStateFieldV2,
        reason: RuntimeCanonicalValueErrorV2,
    },
    #[error(transparent)]
    Claim(#[from] RuntimeDrainClaimErrorV2),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RuntimeDrainIntentStateValueV2 {
    Pending {
        claim: Option<Box<RuntimeDrainClaimV2>>,
    },
    RouteAbsentAcknowledged {
        acknowledgement: Box<RuntimeRouteAbsentAcknowledgementV2>,
    },
    Consumed {
        resulting_revision: DeploymentRevision,
        consumed_at: DateTime<Utc>,
    },
    Cancelled {
        cancelled_at: DateTime<Utc>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDrainIntentStateV2 {
    value: RuntimeDrainIntentStateValueV2,
}

impl RuntimeDrainIntentStateV2 {
    pub fn kind(&self) -> RuntimeDrainIntentStateKindV2 {
        match &self.value {
            RuntimeDrainIntentStateValueV2::Pending { .. } => {
                RuntimeDrainIntentStateKindV2::Pending
            }
            RuntimeDrainIntentStateValueV2::RouteAbsentAcknowledged { .. } => {
                RuntimeDrainIntentStateKindV2::RouteAbsentAcknowledged
            }
            RuntimeDrainIntentStateValueV2::Consumed { .. } => {
                RuntimeDrainIntentStateKindV2::Consumed
            }
            RuntimeDrainIntentStateValueV2::Cancelled { .. } => {
                RuntimeDrainIntentStateKindV2::Cancelled
            }
        }
    }

    pub fn freezes_serving_slot(&self) -> bool {
        matches!(
            &self.value,
            RuntimeDrainIntentStateValueV2::Pending { .. }
                | RuntimeDrainIntentStateValueV2::RouteAbsentAcknowledged { .. }
        )
    }

    pub fn is_runtime_terminal(&self) -> bool {
        matches!(
            &self.value,
            RuntimeDrainIntentStateValueV2::RouteAbsentAcknowledged { .. }
                | RuntimeDrainIntentStateValueV2::Consumed { .. }
                | RuntimeDrainIntentStateValueV2::Cancelled { .. }
        )
    }

    pub fn pending_claim(&self) -> Option<&RuntimeDrainClaimV2> {
        match &self.value {
            RuntimeDrainIntentStateValueV2::Pending { claim } => claim.as_deref(),
            RuntimeDrainIntentStateValueV2::RouteAbsentAcknowledged { .. }
            | RuntimeDrainIntentStateValueV2::Consumed { .. }
            | RuntimeDrainIntentStateValueV2::Cancelled { .. } => None,
        }
    }

    pub fn acknowledgement(&self) -> Option<&RuntimeRouteAbsentAcknowledgementV2> {
        match &self.value {
            RuntimeDrainIntentStateValueV2::RouteAbsentAcknowledged { acknowledgement } => {
                Some(acknowledgement.as_ref())
            }
            RuntimeDrainIntentStateValueV2::Pending { .. }
            | RuntimeDrainIntentStateValueV2::Consumed { .. }
            | RuntimeDrainIntentStateValueV2::Cancelled { .. } => None,
        }
    }

    pub fn resulting_revision(&self) -> Option<DeploymentRevision> {
        match &self.value {
            RuntimeDrainIntentStateValueV2::Consumed {
                resulting_revision, ..
            } => Some(*resulting_revision),
            RuntimeDrainIntentStateValueV2::Pending { .. }
            | RuntimeDrainIntentStateValueV2::RouteAbsentAcknowledged { .. }
            | RuntimeDrainIntentStateValueV2::Cancelled { .. } => None,
        }
    }

    pub fn consumed_at(&self) -> Option<DateTime<Utc>> {
        match &self.value {
            RuntimeDrainIntentStateValueV2::Consumed { consumed_at, .. } => Some(*consumed_at),
            RuntimeDrainIntentStateValueV2::Pending { .. }
            | RuntimeDrainIntentStateValueV2::RouteAbsentAcknowledged { .. }
            | RuntimeDrainIntentStateValueV2::Cancelled { .. } => None,
        }
    }

    pub fn cancelled_at(&self) -> Option<DateTime<Utc>> {
        match &self.value {
            RuntimeDrainIntentStateValueV2::Cancelled { cancelled_at } => Some(*cancelled_at),
            RuntimeDrainIntentStateValueV2::Pending { .. }
            | RuntimeDrainIntentStateValueV2::RouteAbsentAcknowledged { .. }
            | RuntimeDrainIntentStateValueV2::Consumed { .. } => None,
        }
    }

    fn pending(claim: Option<RuntimeDrainClaimV2>) -> Self {
        Self {
            value: RuntimeDrainIntentStateValueV2::Pending {
                claim: claim.map(Box::new),
            },
        }
    }

    fn route_absent_acknowledged(acknowledgement: RuntimeRouteAbsentAcknowledgementV2) -> Self {
        Self {
            value: RuntimeDrainIntentStateValueV2::RouteAbsentAcknowledged {
                acknowledgement: Box::new(acknowledgement),
            },
        }
    }

    fn consumed(resulting_revision: DeploymentRevision, consumed_at: DateTime<Utc>) -> Self {
        Self {
            value: RuntimeDrainIntentStateValueV2::Consumed {
                resulting_revision,
                consumed_at,
            },
        }
    }

    fn cancelled(cancelled_at: DateTime<Utc>) -> Self {
        Self {
            value: RuntimeDrainIntentStateValueV2::Cancelled { cancelled_at },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDrainIntentV2 {
    canonical: RuntimeCanonicalProductDrainV2,
    intent_revision: NonZeroU64,
    state: RuntimeDrainIntentStateV2,
}

impl RuntimeDrainIntentV2 {
    pub fn from_inserted(
        operation: &RuntimeProductDrainOperationV2,
        intent_revision: NonZeroU64,
    ) -> Result<Self, RuntimeDrainIntentStateErrorV2> {
        Self::build(
            operation.canonical().clone(),
            intent_revision,
            RuntimeDrainIntentStateV2::pending(None),
        )
    }

    pub fn pending_from_persisted(
        root: &RuntimePersistedProductDrainRootV2,
        intent_revision: NonZeroU64,
        claim: Option<RuntimeDrainClaimV2>,
    ) -> Result<Self, RuntimeDrainIntentStateErrorV2> {
        Self::build(
            root.canonical().clone(),
            intent_revision,
            RuntimeDrainIntentStateV2::pending(claim),
        )
    }

    pub fn route_absent_acknowledged_from_persisted(
        root: &RuntimePersistedProductDrainRootV2,
        intent_revision: NonZeroU64,
        acknowledgement: RuntimeRouteAbsentAcknowledgementV2,
    ) -> Result<Self, RuntimeDrainIntentStateErrorV2> {
        Self::build(
            root.canonical().clone(),
            intent_revision,
            RuntimeDrainIntentStateV2::route_absent_acknowledged(acknowledgement),
        )
    }

    pub fn consumed_from_persisted(
        root: &RuntimePersistedProductDrainRootV2,
        intent_revision: NonZeroU64,
        resulting_revision: DeploymentRevision,
        consumed_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeDrainIntentStateErrorV2> {
        Self::build(
            root.canonical().clone(),
            intent_revision,
            RuntimeDrainIntentStateV2::consumed(resulting_revision, consumed_at),
        )
    }

    pub fn cancelled_from_persisted(
        root: &RuntimePersistedProductDrainRootV2,
        intent_revision: NonZeroU64,
        cancelled_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeDrainIntentStateErrorV2> {
        Self::build(
            root.canonical().clone(),
            intent_revision,
            RuntimeDrainIntentStateV2::cancelled(cancelled_at),
        )
    }

    pub fn canonical(&self) -> &RuntimeCanonicalProductDrainV2 {
        &self.canonical
    }

    pub fn key(&self) -> &RuntimeDrainIntentKeyV2 {
        &self.canonical.drain_preimage().key
    }

    pub fn product_mutation_request_bytes(&self) -> &[u8] {
        self.canonical.product_mutation_request_bytes()
    }

    pub fn product_mutation_digest(&self) -> &RuntimeProductMutationDigestV2 {
        self.canonical.product_mutation_digest()
    }

    pub fn drain_intent_request_bytes(&self) -> &[u8] {
        self.canonical.drain_intent_request_bytes()
    }

    pub fn drain_intent_digest(&self) -> &RuntimeDrainIntentDigestV2 {
        self.canonical.drain_intent_digest()
    }

    pub fn intent_revision(&self) -> NonZeroU64 {
        self.intent_revision
    }

    pub fn state(&self) -> &RuntimeDrainIntentStateV2 {
        &self.state
    }

    fn build(
        canonical: RuntimeCanonicalProductDrainV2,
        intent_revision: NonZeroU64,
        state: RuntimeDrainIntentStateV2,
    ) -> Result<Self, RuntimeDrainIntentStateErrorV2> {
        validate_persistence_u64(
            intent_revision.get(),
            RuntimeDrainIntentStateFieldV2::IntentRevision,
        )?;
        validate_state(&canonical.drain_preimage().key, &state)?;
        Ok(Self {
            canonical,
            intent_revision,
            state,
        })
    }
}

fn validate_state(
    key: &RuntimeDrainIntentKeyV2,
    state: &RuntimeDrainIntentStateV2,
) -> Result<(), RuntimeDrainIntentStateErrorV2> {
    match &state.value {
        RuntimeDrainIntentStateValueV2::Pending { claim } => {
            if let Some(claim) = claim.as_deref() {
                validate_drain_claim_for_key(claim, key)?;
            }
        }
        RuntimeDrainIntentStateValueV2::RouteAbsentAcknowledged { acknowledgement } => {
            validate_route_absent_acknowledgement_for_key(acknowledgement, key)?;
        }
        RuntimeDrainIntentStateValueV2::Consumed {
            resulting_revision,
            consumed_at,
        } => {
            validate_persistence_u64(
                resulting_revision.get(),
                RuntimeDrainIntentStateFieldV2::ResultingRevision,
            )?;
            validate_datetime(*consumed_at, RuntimeDrainIntentStateFieldV2::ConsumedAt)?;
        }
        RuntimeDrainIntentStateValueV2::Cancelled { cancelled_at } => {
            validate_datetime(*cancelled_at, RuntimeDrainIntentStateFieldV2::CancelledAt)?;
        }
    }
    Ok(())
}

fn validate_persistence_u64(
    value: u64,
    field: RuntimeDrainIntentStateFieldV2,
) -> Result<(), RuntimeDrainIntentStateErrorV2> {
    RuntimePersistenceU64V2::from_u64(value)
        .map(|_| ())
        .map_err(|reason| RuntimeDrainIntentStateErrorV2::CanonicalValue { field, reason })
}

fn validate_datetime(
    value: DateTime<Utc>,
    field: RuntimeDrainIntentStateFieldV2,
) -> Result<(), RuntimeDrainIntentStateErrorV2> {
    RuntimeUnixMicrosecondsV2::from_datetime(value)
        .map(|_| ())
        .map_err(|reason| RuntimeDrainIntentStateErrorV2::CanonicalValue { field, reason })
}
