#[cfg(test)]
mod tests;

use automation_runtime_convergence::{RuntimeDeployment, RuntimeDeploymentSnapshotV1};
use chrono::{DateTime, Utc};

use crate::{
    RuntimeCanonicalValueErrorV2, RuntimeDrainIntentV2, RuntimePersistedProductDrainRootV2,
    RuntimeProductDrainOperationV2, RuntimeProductDrainReplayErrorV2,
    RuntimeProductDrainScopeLookupV2, RuntimeServingSlotV2, RuntimeUnixMicrosecondsV2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProductDrainNaturalScopeV2 {
    ProductOperation,
    DrainIntent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProductDrainScopeCorruptionV2 {
    Ambiguous(RuntimeProductDrainNaturalScopeV2),
    PartialPair {
        present: RuntimeProductDrainNaturalScopeV2,
    },
    PairMismatch,
    PersistedRootInvalid,
    PersistedIntentInvalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProductDrainScopeObservationKindV2 {
    Absent,
    Present,
    PersistenceCorrupt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProductDrainScopeObservationFieldV2 {
    ProductScope,
    ProductExpectedRevision,
    DrainScope,
    DrainSlot,
    DrainExpectedRevision,
    ExpectedTarget,
    ImmutableRoots,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProductDrainScopeObservationErrorV2 {
    #[error("runtime Product drain scope observation snapshot is invalid")]
    InvalidSnapshot,
    #[error("runtime Product drain scope lookup disagrees with its snapshot on {field:?}")]
    LookupMismatch {
        field: RuntimeProductDrainScopeObservationFieldV2,
    },
    #[error("persisted Product drain observation disagrees on {field:?}")]
    PersistedMismatch {
        field: RuntimeProductDrainScopeObservationFieldV2,
    },
    #[error("runtime Product drain observation time is invalid: {reason}")]
    InvalidObservedAt {
        reason: RuntimeCanonicalValueErrorV2,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeObservedProductDrainV2 {
    root: RuntimePersistedProductDrainRootV2,
    intent: RuntimeDrainIntentV2,
}

impl RuntimeObservedProductDrainV2 {
    pub fn from_exact_parts(
        root: RuntimePersistedProductDrainRootV2,
        intent: RuntimeDrainIntentV2,
    ) -> Result<Self, RuntimeProductDrainScopeObservationErrorV2> {
        if root.canonical() != intent.canonical() {
            return Err(
                RuntimeProductDrainScopeObservationErrorV2::PersistedMismatch {
                    field: RuntimeProductDrainScopeObservationFieldV2::ImmutableRoots,
                },
            );
        }
        Ok(Self { root, intent })
    }

    pub fn root(&self) -> &RuntimePersistedProductDrainRootV2 {
        &self.root
    }

    pub fn intent(&self) -> &RuntimeDrainIntentV2 {
        &self.intent
    }

    pub fn into_intent(self) -> RuntimeDrainIntentV2 {
        self.intent
    }

    pub fn require_byte_exact_replay(
        &self,
        proposed: &RuntimeProductDrainOperationV2,
    ) -> Result<(), RuntimeProductDrainReplayErrorV2> {
        self.root.require_byte_exact_replay(proposed)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RuntimeProductDrainScopeObservationStateV2 {
    Absent,
    Present(Box<RuntimeObservedProductDrainV2>),
    PersistenceCorrupt(RuntimeProductDrainScopeCorruptionV2),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeProductDrainScopeObservationV2 {
    lookup: RuntimeProductDrainScopeLookupV2,
    locked_snapshot: RuntimeDeploymentSnapshotV1,
    observed_at: DateTime<Utc>,
    state: RuntimeProductDrainScopeObservationStateV2,
}

impl RuntimeProductDrainScopeObservationV2 {
    pub fn absent(
        lookup: RuntimeProductDrainScopeLookupV2,
        locked_snapshot: RuntimeDeploymentSnapshotV1,
        observed_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeProductDrainScopeObservationErrorV2> {
        Self::build(
            lookup,
            locked_snapshot,
            observed_at,
            RuntimeProductDrainScopeObservationStateV2::Absent,
        )
    }

    pub fn present(
        lookup: RuntimeProductDrainScopeLookupV2,
        locked_snapshot: RuntimeDeploymentSnapshotV1,
        persisted: RuntimeObservedProductDrainV2,
        observed_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeProductDrainScopeObservationErrorV2> {
        Self::build(
            lookup,
            locked_snapshot,
            observed_at,
            RuntimeProductDrainScopeObservationStateV2::Present(Box::new(persisted)),
        )
    }

    pub fn persistence_corrupt(
        lookup: RuntimeProductDrainScopeLookupV2,
        locked_snapshot: RuntimeDeploymentSnapshotV1,
        corruption: RuntimeProductDrainScopeCorruptionV2,
        observed_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeProductDrainScopeObservationErrorV2> {
        Self::build(
            lookup,
            locked_snapshot,
            observed_at,
            RuntimeProductDrainScopeObservationStateV2::PersistenceCorrupt(corruption),
        )
    }

    pub fn kind(&self) -> RuntimeProductDrainScopeObservationKindV2 {
        match &self.state {
            RuntimeProductDrainScopeObservationStateV2::Absent => {
                RuntimeProductDrainScopeObservationKindV2::Absent
            }
            RuntimeProductDrainScopeObservationStateV2::Present(_) => {
                RuntimeProductDrainScopeObservationKindV2::Present
            }
            RuntimeProductDrainScopeObservationStateV2::PersistenceCorrupt(_) => {
                RuntimeProductDrainScopeObservationKindV2::PersistenceCorrupt
            }
        }
    }

    pub fn lookup(&self) -> &RuntimeProductDrainScopeLookupV2 {
        &self.lookup
    }

    pub fn locked_snapshot(&self) -> &RuntimeDeploymentSnapshotV1 {
        &self.locked_snapshot
    }

    pub fn observed_at(&self) -> DateTime<Utc> {
        self.observed_at
    }

    pub fn persisted(&self) -> Option<&RuntimeObservedProductDrainV2> {
        match &self.state {
            RuntimeProductDrainScopeObservationStateV2::Present(persisted) => {
                Some(persisted.as_ref())
            }
            RuntimeProductDrainScopeObservationStateV2::Absent
            | RuntimeProductDrainScopeObservationStateV2::PersistenceCorrupt(_) => None,
        }
    }

    pub fn corruption(&self) -> Option<RuntimeProductDrainScopeCorruptionV2> {
        match &self.state {
            RuntimeProductDrainScopeObservationStateV2::PersistenceCorrupt(corruption) => {
                Some(*corruption)
            }
            RuntimeProductDrainScopeObservationStateV2::Absent
            | RuntimeProductDrainScopeObservationStateV2::Present(_) => None,
        }
    }

    pub fn into_persisted(self) -> Option<RuntimeObservedProductDrainV2> {
        match self.state {
            RuntimeProductDrainScopeObservationStateV2::Present(persisted) => Some(*persisted),
            RuntimeProductDrainScopeObservationStateV2::Absent
            | RuntimeProductDrainScopeObservationStateV2::PersistenceCorrupt(_) => None,
        }
    }

    fn build(
        lookup: RuntimeProductDrainScopeLookupV2,
        locked_snapshot: RuntimeDeploymentSnapshotV1,
        observed_at: DateTime<Utc>,
        state: RuntimeProductDrainScopeObservationStateV2,
    ) -> Result<Self, RuntimeProductDrainScopeObservationErrorV2> {
        validate_lookup(&lookup, &locked_snapshot)?;
        validate_observed_at(observed_at)?;
        if let RuntimeProductDrainScopeObservationStateV2::Present(persisted) = &state {
            validate_persisted(&lookup, &locked_snapshot, persisted)?;
        }
        Ok(Self {
            lookup,
            locked_snapshot,
            observed_at,
            state,
        })
    }
}

fn validate_lookup(
    lookup: &RuntimeProductDrainScopeLookupV2,
    snapshot: &RuntimeDeploymentSnapshotV1,
) -> Result<(), RuntimeProductDrainScopeObservationErrorV2> {
    RuntimeDeployment::restore(snapshot.clone())
        .map_err(|_| RuntimeProductDrainScopeObservationErrorV2::InvalidSnapshot)?;
    let expected_scope = crate::RuntimeDeploymentScopeV1::from_identity(&snapshot.identity);
    let product = lookup.product_operation_scope();
    let drain = lookup.drain_intent_scope();
    let mismatch = if product.scope() != &expected_scope {
        Some(RuntimeProductDrainScopeObservationFieldV2::ProductScope)
    } else if product.expected_revision() != snapshot.revision {
        Some(RuntimeProductDrainScopeObservationFieldV2::ProductExpectedRevision)
    } else if drain.scope() != &expected_scope {
        Some(RuntimeProductDrainScopeObservationFieldV2::DrainScope)
    } else if drain.slot() != &RuntimeServingSlotV2::from_target(&snapshot.target) {
        Some(RuntimeProductDrainScopeObservationFieldV2::DrainSlot)
    } else if drain.expected_revision() != snapshot.revision {
        Some(RuntimeProductDrainScopeObservationFieldV2::DrainExpectedRevision)
    } else {
        None
    };
    if let Some(field) = mismatch {
        Err(RuntimeProductDrainScopeObservationErrorV2::LookupMismatch { field })
    } else {
        Ok(())
    }
}

fn validate_persisted(
    lookup: &RuntimeProductDrainScopeLookupV2,
    snapshot: &RuntimeDeploymentSnapshotV1,
    persisted: &RuntimeObservedProductDrainV2,
) -> Result<(), RuntimeProductDrainScopeObservationErrorV2> {
    let root = persisted.root();
    let mismatch =
        if root.product_operation_scope().scope() != lookup.product_operation_scope().scope() {
            Some(RuntimeProductDrainScopeObservationFieldV2::ProductScope)
        } else if root.product_operation_scope().expected_revision()
            != lookup.product_operation_scope().expected_revision()
        {
            Some(RuntimeProductDrainScopeObservationFieldV2::ProductExpectedRevision)
        } else if root.drain_intent_scope().scope() != lookup.drain_intent_scope().scope() {
            Some(RuntimeProductDrainScopeObservationFieldV2::DrainScope)
        } else if root.drain_intent_scope().slot() != lookup.drain_intent_scope().slot() {
            Some(RuntimeProductDrainScopeObservationFieldV2::DrainSlot)
        } else if root.drain_intent_scope().expected_revision()
            != lookup.drain_intent_scope().expected_revision()
        {
            Some(RuntimeProductDrainScopeObservationFieldV2::DrainExpectedRevision)
        } else if root.canonical().product_preimage().expected_target != snapshot.target {
            Some(RuntimeProductDrainScopeObservationFieldV2::ExpectedTarget)
        } else {
            None
        };
    if let Some(field) = mismatch {
        Err(RuntimeProductDrainScopeObservationErrorV2::PersistedMismatch { field })
    } else {
        Ok(())
    }
}

fn validate_observed_at(
    observed_at: DateTime<Utc>,
) -> Result<(), RuntimeProductDrainScopeObservationErrorV2> {
    RuntimeUnixMicrosecondsV2::from_datetime(observed_at)
        .map(|_| ())
        .map_err(|reason| RuntimeProductDrainScopeObservationErrorV2::InvalidObservedAt { reason })
}
