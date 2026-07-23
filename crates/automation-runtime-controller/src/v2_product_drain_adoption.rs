#[cfg(test)]
mod tests;

use automation_runtime_convergence::{RuntimeDeploymentSnapshotV1, RuntimeDeploymentTargetV1};

use crate::{
    RuntimeObservedProductDrainV2, RuntimeProductDrainOperationBuildErrorV2,
    RuntimeProductDrainOperationV2, RuntimeProductDrainScopeCorruptionV2,
    RuntimeProductDrainScopeLookupV2, RuntimeProductDrainScopeObservationV2,
    RuntimeProductMutationKindV2, RuntimeProductSemanticRequestDigestV2,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeProductDrainSemanticExpectationV2 {
    lookup: RuntimeProductDrainScopeLookupV2,
    expected_target: RuntimeDeploymentTargetV1,
    mutation_kind: RuntimeProductMutationKindV2,
    product_semantic_request_digest: RuntimeProductSemanticRequestDigestV2,
}

impl RuntimeProductDrainSemanticExpectationV2 {
    pub fn from_locked_snapshot(
        snapshot: &RuntimeDeploymentSnapshotV1,
        mutation_kind: RuntimeProductMutationKindV2,
        product_semantic_request_digest: RuntimeProductSemanticRequestDigestV2,
    ) -> Result<Self, RuntimeProductDrainOperationBuildErrorV2> {
        Ok(Self {
            lookup: RuntimeProductDrainScopeLookupV2::from_locked_snapshot(snapshot)?,
            expected_target: snapshot.target.clone(),
            mutation_kind,
            product_semantic_request_digest,
        })
    }

    pub fn from_proposed(proposed: &RuntimeProductDrainOperationV2) -> Self {
        let product = proposed.canonical().product_preimage();
        Self {
            lookup: proposed.scope_lookup(),
            expected_target: product.expected_target.clone(),
            mutation_kind: product.mutation_kind,
            product_semantic_request_digest: product.product_semantic_request_digest.clone(),
        }
    }

    pub fn lookup(&self) -> &RuntimeProductDrainScopeLookupV2 {
        &self.lookup
    }

    pub fn expected_target(&self) -> &RuntimeDeploymentTargetV1 {
        &self.expected_target
    }

    pub fn mutation_kind(&self) -> RuntimeProductMutationKindV2 {
        self.mutation_kind
    }

    pub fn product_semantic_request_digest(&self) -> &RuntimeProductSemanticRequestDigestV2 {
        &self.product_semantic_request_digest
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProductDrainAdoptionKindV2 {
    Absent,
    ExactProposedRoot,
    PersistedRoot,
    Diverged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProductDrainSemanticFieldV2 {
    ExpectedTarget,
    MutationKind,
    ProductSemanticRequestDigest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeProductDrainAdoptionDivergenceV2 {
    SemanticMismatch {
        field: RuntimeProductDrainSemanticFieldV2,
    },
    CanonicalMismatch,
    PersistenceCorrupt {
        corruption: RuntimeProductDrainScopeCorruptionV2,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeProductDrainAdoptionErrorV2 {
    #[error("runtime Product drain semantic expectation and observation lookup do not match")]
    ObservationLookupMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RuntimeProductDrainAdoptionStateV2 {
    Absent,
    ExactProposedRoot,
    PersistedRoot,
    Diverged(RuntimeProductDrainAdoptionDivergenceV2),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeProductDrainAdoptionV2 {
    expectation: RuntimeProductDrainSemanticExpectationV2,
    observation: RuntimeProductDrainScopeObservationV2,
    state: RuntimeProductDrainAdoptionStateV2,
}

impl RuntimeProductDrainAdoptionV2 {
    pub fn classify_proposed(
        proposed: &RuntimeProductDrainOperationV2,
        observation: RuntimeProductDrainScopeObservationV2,
    ) -> Result<Self, RuntimeProductDrainAdoptionErrorV2> {
        Self::classify(
            RuntimeProductDrainSemanticExpectationV2::from_proposed(proposed),
            observation,
            Some(proposed),
        )
    }

    pub fn classify_semantic_recovery(
        expectation: RuntimeProductDrainSemanticExpectationV2,
        observation: RuntimeProductDrainScopeObservationV2,
    ) -> Result<Self, RuntimeProductDrainAdoptionErrorV2> {
        Self::classify(expectation, observation, None)
    }

    pub fn kind(&self) -> RuntimeProductDrainAdoptionKindV2 {
        match &self.state {
            RuntimeProductDrainAdoptionStateV2::Absent => RuntimeProductDrainAdoptionKindV2::Absent,
            RuntimeProductDrainAdoptionStateV2::ExactProposedRoot => {
                RuntimeProductDrainAdoptionKindV2::ExactProposedRoot
            }
            RuntimeProductDrainAdoptionStateV2::PersistedRoot => {
                RuntimeProductDrainAdoptionKindV2::PersistedRoot
            }
            RuntimeProductDrainAdoptionStateV2::Diverged(_) => {
                RuntimeProductDrainAdoptionKindV2::Diverged
            }
        }
    }

    pub fn expectation(&self) -> &RuntimeProductDrainSemanticExpectationV2 {
        &self.expectation
    }

    pub fn observation(&self) -> &RuntimeProductDrainScopeObservationV2 {
        &self.observation
    }

    pub fn divergence(&self) -> Option<RuntimeProductDrainAdoptionDivergenceV2> {
        match &self.state {
            RuntimeProductDrainAdoptionStateV2::Diverged(divergence) => Some(*divergence),
            RuntimeProductDrainAdoptionStateV2::Absent
            | RuntimeProductDrainAdoptionStateV2::ExactProposedRoot
            | RuntimeProductDrainAdoptionStateV2::PersistedRoot => None,
        }
    }

    pub fn persisted(&self) -> Option<&RuntimeObservedProductDrainV2> {
        match &self.state {
            RuntimeProductDrainAdoptionStateV2::ExactProposedRoot
            | RuntimeProductDrainAdoptionStateV2::PersistedRoot => self.observation.persisted(),
            RuntimeProductDrainAdoptionStateV2::Absent
            | RuntimeProductDrainAdoptionStateV2::Diverged(_) => None,
        }
    }

    pub fn into_observation(self) -> RuntimeProductDrainScopeObservationV2 {
        self.observation
    }

    fn classify(
        expectation: RuntimeProductDrainSemanticExpectationV2,
        observation: RuntimeProductDrainScopeObservationV2,
        proposed: Option<&RuntimeProductDrainOperationV2>,
    ) -> Result<Self, RuntimeProductDrainAdoptionErrorV2> {
        if expectation.lookup() != observation.lookup() {
            return Err(RuntimeProductDrainAdoptionErrorV2::ObservationLookupMismatch);
        }
        let state = if let Some(corruption) = observation.corruption() {
            RuntimeProductDrainAdoptionStateV2::Diverged(
                RuntimeProductDrainAdoptionDivergenceV2::PersistenceCorrupt { corruption },
            )
        } else if let Some(persisted) = observation.persisted() {
            if let Some(field) = semantic_mismatch(&expectation, persisted) {
                RuntimeProductDrainAdoptionStateV2::Diverged(
                    RuntimeProductDrainAdoptionDivergenceV2::SemanticMismatch { field },
                )
            } else if proposed
                .is_some_and(|proposed| persisted.require_byte_exact_replay(proposed).is_err())
            {
                RuntimeProductDrainAdoptionStateV2::Diverged(
                    RuntimeProductDrainAdoptionDivergenceV2::CanonicalMismatch,
                )
            } else if proposed.is_some() {
                RuntimeProductDrainAdoptionStateV2::ExactProposedRoot
            } else {
                RuntimeProductDrainAdoptionStateV2::PersistedRoot
            }
        } else {
            RuntimeProductDrainAdoptionStateV2::Absent
        };
        Ok(Self {
            expectation,
            observation,
            state,
        })
    }
}

fn semantic_mismatch(
    expectation: &RuntimeProductDrainSemanticExpectationV2,
    persisted: &RuntimeObservedProductDrainV2,
) -> Option<RuntimeProductDrainSemanticFieldV2> {
    let product = persisted.root().canonical().product_preimage();
    if &product.expected_target != expectation.expected_target() {
        Some(RuntimeProductDrainSemanticFieldV2::ExpectedTarget)
    } else if product.mutation_kind != expectation.mutation_kind() {
        Some(RuntimeProductDrainSemanticFieldV2::MutationKind)
    } else if &product.product_semantic_request_digest
        != expectation.product_semantic_request_digest()
    {
        Some(RuntimeProductDrainSemanticFieldV2::ProductSemanticRequestDigest)
    } else {
        None
    }
}
