#[cfg(test)]
mod tests;

use automation_runtime_convergence::{
    DeploymentRevision, RuntimeDeployment, RuntimeDeploymentSnapshotV1, RuntimeDeploymentTargetV1,
};

use crate::{
    RuntimeCanonicalProductDrainV2, RuntimeDeploymentScopeV1, RuntimeDrainIntentDigestV2,
    RuntimeDrainIntentIdV2, RuntimeProductDrainCanonicalErrorV2, RuntimeProductMutationDigestV2,
    RuntimeProductOperationIdV2, RuntimeServingSlotV2,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeProductOperationScopeV2 {
    scope: RuntimeDeploymentScopeV1,
    expected_revision: DeploymentRevision,
}

impl RuntimeProductOperationScopeV2 {
    pub fn scope(&self) -> &RuntimeDeploymentScopeV1 {
        &self.scope
    }

    pub fn expected_revision(&self) -> DeploymentRevision {
        self.expected_revision
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDrainIntentOperationScopeV2 {
    scope: RuntimeDeploymentScopeV1,
    slot: RuntimeServingSlotV2,
    expected_revision: DeploymentRevision,
}

impl RuntimeDrainIntentOperationScopeV2 {
    pub fn scope(&self) -> &RuntimeDeploymentScopeV1 {
        &self.scope
    }

    pub fn slot(&self) -> &RuntimeServingSlotV2 {
        &self.slot
    }

    pub fn expected_revision(&self) -> DeploymentRevision {
        self.expected_revision
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProductDrainOperationFieldV2 {
    ProductScope,
    ProductExpectedRevision,
    ProductSlot,
    ProductOperationId,
    DrainScope,
    DrainSlot,
    DrainExpectedRevision,
    DrainIntentId,
    ExpectedTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProductDrainOperationBuildErrorV2 {
    #[error("runtime Product drain operation snapshot is invalid")]
    InvalidSnapshot,
    #[error("runtime Product drain operation disagrees with its locked deployment on {field:?}")]
    RootCorrelationMismatch {
        field: RuntimeProductDrainOperationFieldV2,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProductDrainOperationPersistenceErrorV2 {
    #[error(transparent)]
    Canonical(#[from] RuntimeProductDrainCanonicalErrorV2),
    #[error("persisted Product drain operation disagrees on {field:?}")]
    PersistedCorrelationMismatch {
        field: RuntimeProductDrainOperationFieldV2,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProductDrainReplayErrorV2 {
    #[error("persisted Product drain roots do not match the proposed creation")]
    CreationMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeProductDrainOperationV2 {
    product_operation_scope: RuntimeProductOperationScopeV2,
    drain_intent_scope: RuntimeDrainIntentOperationScopeV2,
    canonical: RuntimeCanonicalProductDrainV2,
}

impl RuntimeProductDrainOperationV2 {
    pub fn new(
        locked_snapshot: &RuntimeDeploymentSnapshotV1,
        canonical: RuntimeCanonicalProductDrainV2,
    ) -> Result<Self, RuntimeProductDrainOperationBuildErrorV2> {
        validate_snapshot(locked_snapshot)?;
        validate_canonical_against_snapshot(locked_snapshot, &canonical)?;
        let (product_operation_scope, drain_intent_scope) = scopes_from_canonical(&canonical);
        Ok(Self {
            product_operation_scope,
            drain_intent_scope,
            canonical,
        })
    }

    pub fn product_operation_scope(&self) -> &RuntimeProductOperationScopeV2 {
        &self.product_operation_scope
    }

    pub fn drain_intent_scope(&self) -> &RuntimeDrainIntentOperationScopeV2 {
        &self.drain_intent_scope
    }

    pub fn product_operation_id(&self) -> &RuntimeProductOperationIdV2 {
        &self.canonical.product_preimage().operation_id
    }

    pub fn drain_intent_id(&self) -> &RuntimeDrainIntentIdV2 {
        &self.canonical.drain_preimage().key.intent_id
    }

    pub fn canonical(&self) -> &RuntimeCanonicalProductDrainV2 {
        &self.canonical
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

    pub fn scope_lookup(&self) -> RuntimeProductDrainScopeLookupV2 {
        RuntimeProductDrainScopeLookupV2 {
            product_operation_scope: self.product_operation_scope.clone(),
            drain_intent_scope: self.drain_intent_scope.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePersistedProductDrainRootV2 {
    product_operation_scope: RuntimeProductOperationScopeV2,
    drain_intent_scope: RuntimeDrainIntentOperationScopeV2,
    canonical: RuntimeCanonicalProductDrainV2,
}

impl RuntimePersistedProductDrainRootV2 {
    #[expect(
        clippy::too_many_arguments,
        reason = "the persisted aggregate checks every normalized identity against both roots"
    )]
    pub fn from_persisted(
        persisted_product_scope: RuntimeDeploymentScopeV1,
        persisted_product_expected_revision: DeploymentRevision,
        persisted_product_operation_id: &RuntimeProductOperationIdV2,
        persisted_drain_scope: RuntimeDeploymentScopeV1,
        persisted_drain_slot: RuntimeServingSlotV2,
        persisted_drain_expected_revision: DeploymentRevision,
        persisted_drain_intent_id: &RuntimeDrainIntentIdV2,
        persisted_expected_target: &RuntimeDeploymentTargetV1,
        product_mutation_request_bytes: &[u8],
        persisted_product_mutation_digest: &RuntimeProductMutationDigestV2,
        drain_intent_request_bytes: &[u8],
        persisted_drain_intent_digest: &RuntimeDrainIntentDigestV2,
    ) -> Result<Self, RuntimeProductDrainOperationPersistenceErrorV2> {
        let canonical = RuntimeCanonicalProductDrainV2::from_persisted(
            product_mutation_request_bytes,
            persisted_product_mutation_digest,
            drain_intent_request_bytes,
            persisted_drain_intent_digest,
        )?;
        validate_persisted_identity(
            &persisted_product_scope,
            persisted_product_expected_revision,
            persisted_product_operation_id,
            &persisted_drain_scope,
            &persisted_drain_slot,
            persisted_drain_expected_revision,
            persisted_drain_intent_id,
            persisted_expected_target,
            &canonical,
        )?;
        Ok(Self {
            product_operation_scope: RuntimeProductOperationScopeV2 {
                scope: persisted_product_scope,
                expected_revision: persisted_product_expected_revision,
            },
            drain_intent_scope: RuntimeDrainIntentOperationScopeV2 {
                scope: persisted_drain_scope,
                slot: persisted_drain_slot,
                expected_revision: persisted_drain_expected_revision,
            },
            canonical,
        })
    }

    pub fn product_operation_scope(&self) -> &RuntimeProductOperationScopeV2 {
        &self.product_operation_scope
    }

    pub fn drain_intent_scope(&self) -> &RuntimeDrainIntentOperationScopeV2 {
        &self.drain_intent_scope
    }

    pub fn product_operation_id(&self) -> &RuntimeProductOperationIdV2 {
        &self.canonical.product_preimage().operation_id
    }

    pub fn drain_intent_id(&self) -> &RuntimeDrainIntentIdV2 {
        &self.canonical.drain_preimage().key.intent_id
    }

    pub fn canonical(&self) -> &RuntimeCanonicalProductDrainV2 {
        &self.canonical
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

    pub fn require_byte_exact_replay(
        &self,
        proposed: &RuntimeProductDrainOperationV2,
    ) -> Result<(), RuntimeProductDrainReplayErrorV2> {
        if self.product_operation_scope == *proposed.product_operation_scope()
            && self.drain_intent_scope == *proposed.drain_intent_scope()
            && self.product_operation_id() == proposed.product_operation_id()
            && self.drain_intent_id() == proposed.drain_intent_id()
            && self.product_mutation_request_bytes() == proposed.product_mutation_request_bytes()
            && self.product_mutation_digest() == proposed.product_mutation_digest()
            && self.drain_intent_request_bytes() == proposed.drain_intent_request_bytes()
            && self.drain_intent_digest() == proposed.drain_intent_digest()
        {
            Ok(())
        } else {
            Err(RuntimeProductDrainReplayErrorV2::CreationMismatch)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeProductDrainScopeLookupV2 {
    product_operation_scope: RuntimeProductOperationScopeV2,
    drain_intent_scope: RuntimeDrainIntentOperationScopeV2,
}

impl RuntimeProductDrainScopeLookupV2 {
    pub fn from_locked_snapshot(
        locked_snapshot: &RuntimeDeploymentSnapshotV1,
    ) -> Result<Self, RuntimeProductDrainOperationBuildErrorV2> {
        validate_snapshot(locked_snapshot)?;
        let scope = RuntimeDeploymentScopeV1::from_identity(&locked_snapshot.identity);
        let slot = RuntimeServingSlotV2::from_target(&locked_snapshot.target);
        Ok(Self {
            product_operation_scope: RuntimeProductOperationScopeV2 {
                scope: scope.clone(),
                expected_revision: locked_snapshot.revision,
            },
            drain_intent_scope: RuntimeDrainIntentOperationScopeV2 {
                scope,
                slot,
                expected_revision: locked_snapshot.revision,
            },
        })
    }

    pub fn product_operation_scope(&self) -> &RuntimeProductOperationScopeV2 {
        &self.product_operation_scope
    }

    pub fn drain_intent_scope(&self) -> &RuntimeDrainIntentOperationScopeV2 {
        &self.drain_intent_scope
    }
}

fn validate_snapshot(
    snapshot: &RuntimeDeploymentSnapshotV1,
) -> Result<(), RuntimeProductDrainOperationBuildErrorV2> {
    RuntimeDeployment::restore(snapshot.clone())
        .map(|_| ())
        .map_err(|_| RuntimeProductDrainOperationBuildErrorV2::InvalidSnapshot)
}

fn validate_canonical_against_snapshot(
    snapshot: &RuntimeDeploymentSnapshotV1,
    canonical: &RuntimeCanonicalProductDrainV2,
) -> Result<(), RuntimeProductDrainOperationBuildErrorV2> {
    let product = canonical.product_preimage();
    let expected_scope = RuntimeDeploymentScopeV1::from_identity(&snapshot.identity);
    let expected_slot = RuntimeServingSlotV2::from_target(&snapshot.target);
    let mismatch = if product.scope != expected_scope {
        Some(RuntimeProductDrainOperationFieldV2::ProductScope)
    } else if product.expected_revision != snapshot.revision {
        Some(RuntimeProductDrainOperationFieldV2::ProductExpectedRevision)
    } else if product.slot != expected_slot {
        Some(RuntimeProductDrainOperationFieldV2::ProductSlot)
    } else if product.expected_target != snapshot.target {
        Some(RuntimeProductDrainOperationFieldV2::ExpectedTarget)
    } else {
        None
    };
    if let Some(field) = mismatch {
        Err(RuntimeProductDrainOperationBuildErrorV2::RootCorrelationMismatch { field })
    } else {
        Ok(())
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "normalized persistence identity is checked field by field against both roots"
)]
fn validate_persisted_identity(
    persisted_product_scope: &RuntimeDeploymentScopeV1,
    persisted_product_expected_revision: DeploymentRevision,
    persisted_product_operation_id: &RuntimeProductOperationIdV2,
    persisted_drain_scope: &RuntimeDeploymentScopeV1,
    persisted_drain_slot: &RuntimeServingSlotV2,
    persisted_drain_expected_revision: DeploymentRevision,
    persisted_drain_intent_id: &RuntimeDrainIntentIdV2,
    persisted_expected_target: &RuntimeDeploymentTargetV1,
    canonical: &RuntimeCanonicalProductDrainV2,
) -> Result<(), RuntimeProductDrainOperationPersistenceErrorV2> {
    let product = canonical.product_preimage();
    let drain = &canonical.drain_preimage().key;
    let mismatch = if persisted_product_scope != &product.scope {
        Some(RuntimeProductDrainOperationFieldV2::ProductScope)
    } else if persisted_product_expected_revision != product.expected_revision {
        Some(RuntimeProductDrainOperationFieldV2::ProductExpectedRevision)
    } else if persisted_product_operation_id != &product.operation_id {
        Some(RuntimeProductDrainOperationFieldV2::ProductOperationId)
    } else if persisted_drain_scope != &drain.scope {
        Some(RuntimeProductDrainOperationFieldV2::DrainScope)
    } else if persisted_drain_slot != &drain.slot {
        Some(RuntimeProductDrainOperationFieldV2::DrainSlot)
    } else if persisted_drain_expected_revision != drain.expected_revision {
        Some(RuntimeProductDrainOperationFieldV2::DrainExpectedRevision)
    } else if persisted_drain_intent_id != &drain.intent_id {
        Some(RuntimeProductDrainOperationFieldV2::DrainIntentId)
    } else if persisted_expected_target != &product.expected_target {
        Some(RuntimeProductDrainOperationFieldV2::ExpectedTarget)
    } else {
        None
    };
    if let Some(field) = mismatch {
        Err(RuntimeProductDrainOperationPersistenceErrorV2::PersistedCorrelationMismatch { field })
    } else {
        Ok(())
    }
}

fn scopes_from_canonical(
    canonical: &RuntimeCanonicalProductDrainV2,
) -> (
    RuntimeProductOperationScopeV2,
    RuntimeDrainIntentOperationScopeV2,
) {
    let product = canonical.product_preimage();
    let drain = &canonical.drain_preimage().key;
    (
        RuntimeProductOperationScopeV2 {
            scope: product.scope.clone(),
            expected_revision: product.expected_revision,
        },
        RuntimeDrainIntentOperationScopeV2 {
            scope: drain.scope.clone(),
            slot: drain.slot.clone(),
            expected_revision: drain.expected_revision,
        },
    )
}
