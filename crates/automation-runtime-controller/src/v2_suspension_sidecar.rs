#[cfg(test)]
mod tests;

use std::num::NonZeroU64;

use automation_runtime_convergence::RuntimeFailureV1;
use chrono::{DateTime, Utc};

use crate::v2_canonical_value::RuntimePersistenceU64V2;
use crate::v2_suspension_canonical::validate_suspend_attempt_mutable_state;
use crate::{
    RuntimeAttemptDispositionV2, RuntimeCanonicalSuspendAttemptV2, RuntimeCanonicalValueErrorV2,
    RuntimeDrainObligationV2, RuntimeExecutionGuardV1, RuntimeLocalRouteEffectV2,
    RuntimePersistedSuspendAttemptRootV2, RuntimeResumeCheckpointV2,
    RuntimeRouteMutationProvenanceV2, RuntimeSuspendAttemptCanonicalErrorV2,
    RuntimeSuspendAttemptDigestV2, RuntimeSuspendAttemptOperationScopeV2,
    RuntimeSuspendAttemptOperationV2, RuntimeSuspendedRouteLifecycleV2, RuntimeSuspensionIdV2,
    RuntimeSuspensionSourcePhaseV2, RuntimeUnixMicrosecondsV2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeSuspendedAttemptStateFieldV2 {
    SidecarRevision,
    SuspendedAt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeSuspendedAttemptStateErrorV2 {
    #[error(transparent)]
    Canonical(#[from] RuntimeSuspendAttemptCanonicalErrorV2),
    #[error("runtime suspended-attempt field {field:?} is invalid: {reason}")]
    CanonicalValue {
        field: RuntimeSuspendedAttemptStateFieldV2,
        reason: RuntimeCanonicalValueErrorV2,
    },
    #[error("runtime inserted suspended-attempt state differs from its immutable root")]
    InitialStateMismatch,
    #[error("runtime suspended-attempt mutable state is unreachable from its immutable root")]
    UnreachableMutableState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeSuspendAttemptDrainProgressErrorV2 {
    #[error("runtime suspension sidecar has no exact local route to drain")]
    NoExactLocalRoute,
    #[error(transparent)]
    State(#[from] RuntimeSuspendedAttemptStateErrorV2),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSuspendedAttemptV2 {
    operation_scope: RuntimeSuspendAttemptOperationScopeV2,
    canonical_attempt: RuntimeCanonicalSuspendAttemptV2,
    sidecar_revision: NonZeroU64,
    local_effect: RuntimeLocalRouteEffectV2,
    drain_obligation: RuntimeDrainObligationV2,
    suspended_at: DateTime<Utc>,
}

impl RuntimeSuspendedAttemptV2 {
    pub fn from_inserted(
        operation: &RuntimeSuspendAttemptOperationV2,
        sidecar_revision: NonZeroU64,
        local_effect: RuntimeLocalRouteEffectV2,
        drain_obligation: RuntimeDrainObligationV2,
        suspended_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeSuspendedAttemptStateErrorV2> {
        let request = operation.canonical_attempt().request();
        if local_effect != request.local_effect || drain_obligation != request.drain_obligation {
            return Err(RuntimeSuspendedAttemptStateErrorV2::InitialStateMismatch);
        }
        Self::build(
            operation.operation_scope().clone(),
            operation.canonical_attempt().clone(),
            sidecar_revision,
            local_effect,
            drain_obligation,
            suspended_at,
        )
    }

    pub fn from_persisted(
        root: &RuntimePersistedSuspendAttemptRootV2,
        sidecar_revision: NonZeroU64,
        local_effect: RuntimeLocalRouteEffectV2,
        drain_obligation: RuntimeDrainObligationV2,
        suspended_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeSuspendedAttemptStateErrorV2> {
        Self::build(
            root.operation_scope().clone(),
            root.canonical_attempt().clone(),
            sidecar_revision,
            local_effect,
            drain_obligation,
            suspended_at,
        )
    }

    pub fn operation_scope(&self) -> &RuntimeSuspendAttemptOperationScopeV2 {
        &self.operation_scope
    }

    pub fn suspension_id(&self) -> &RuntimeSuspensionIdV2 {
        &self.canonical_attempt.request().suspension_id
    }

    pub fn canonical_attempt(&self) -> &RuntimeCanonicalSuspendAttemptV2 {
        &self.canonical_attempt
    }

    pub fn source_guard(&self) -> &RuntimeExecutionGuardV1 {
        &self.canonical_attempt.request().guard
    }

    pub fn source_phase(&self) -> RuntimeSuspensionSourcePhaseV2 {
        self.canonical_attempt.request().source_phase
    }

    pub fn failure(&self) -> &RuntimeFailureV1 {
        &self.canonical_attempt.request().failure
    }

    pub fn disposition(&self) -> &RuntimeAttemptDispositionV2 {
        &self.canonical_attempt.request().disposition
    }

    pub fn checkpoint(&self) -> RuntimeResumeCheckpointV2 {
        self.canonical_attempt.request().checkpoint
    }

    pub fn suspend_attempt_request_bytes(&self) -> &[u8] {
        self.canonical_attempt.suspend_attempt_request_bytes()
    }

    pub fn request_digest(&self) -> &RuntimeSuspendAttemptDigestV2 {
        self.canonical_attempt.suspend_attempt_digest()
    }

    pub fn sidecar_revision(&self) -> NonZeroU64 {
        self.sidecar_revision
    }

    pub fn local_effect(&self) -> &RuntimeLocalRouteEffectV2 {
        &self.local_effect
    }

    pub fn drain_obligation(&self) -> &RuntimeDrainObligationV2 {
        &self.drain_obligation
    }

    pub fn suspended_at(&self) -> DateTime<Utc> {
        self.suspended_at
    }

    fn build(
        operation_scope: RuntimeSuspendAttemptOperationScopeV2,
        canonical_attempt: RuntimeCanonicalSuspendAttemptV2,
        sidecar_revision: NonZeroU64,
        local_effect: RuntimeLocalRouteEffectV2,
        drain_obligation: RuntimeDrainObligationV2,
        suspended_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeSuspendedAttemptStateErrorV2> {
        validate_sidecar_revision(sidecar_revision)?;
        validate_suspended_at(suspended_at)?;
        validate_mutable_state(&canonical_attempt, &local_effect, &drain_obligation)?;
        Ok(Self {
            operation_scope,
            canonical_attempt,
            sidecar_revision,
            local_effect,
            drain_obligation,
            suspended_at,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeSuspendAttemptDrainProgressV2 {
    source: RuntimeSuspendedAttemptV2,
    replacement_local_effect: RuntimeLocalRouteEffectV2,
    replacement_drain_obligation: RuntimeDrainObligationV2,
}

impl RuntimeSuspendAttemptDrainProgressV2 {
    pub fn record_local_absent(
        source: RuntimeSuspendedAttemptV2,
        provenance: RuntimeRouteMutationProvenanceV2,
        observed_sequence: NonZeroU64,
    ) -> Result<Self, RuntimeSuspendAttemptDrainProgressErrorV2> {
        let route = match source.local_effect() {
            RuntimeLocalRouteEffectV2::ExactRoute { route, .. } => route.clone(),
            RuntimeLocalRouteEffectV2::None | RuntimeLocalRouteEffectV2::RouteAbsent { .. } => {
                return Err(RuntimeSuspendAttemptDrainProgressErrorV2::NoExactLocalRoute);
            }
        };
        let replacement_drain_obligation = match source.drain_obligation() {
            RuntimeDrainObligationV2::ExactLocalRoute(local) if local == &route => {
                RuntimeDrainObligationV2::None
            }
            RuntimeDrainObligationV2::LocalAndPrevious { local, previous } if local == &route => {
                RuntimeDrainObligationV2::PreviousServing(previous.clone())
            }
            RuntimeDrainObligationV2::None
            | RuntimeDrainObligationV2::ExactLocalRoute(_)
            | RuntimeDrainObligationV2::PreviousServing(_)
            | RuntimeDrainObligationV2::LocalAndPrevious { .. } => {
                return Err(RuntimeSuspendAttemptDrainProgressErrorV2::NoExactLocalRoute);
            }
        };
        let replacement_local_effect = RuntimeLocalRouteEffectV2::RouteAbsent {
            slot: route.slot(),
            expected_route: Some(route),
            provenance,
            observed_sequence,
        };
        validate_mutable_state(
            &source.canonical_attempt,
            &replacement_local_effect,
            &replacement_drain_obligation,
        )?;
        Ok(Self {
            source,
            replacement_local_effect,
            replacement_drain_obligation,
        })
    }

    pub fn source(&self) -> &RuntimeSuspendedAttemptV2 {
        &self.source
    }

    pub fn expected_sidecar_revision(&self) -> NonZeroU64 {
        self.source.sidecar_revision()
    }

    pub fn expected_local_effect(&self) -> &RuntimeLocalRouteEffectV2 {
        self.source.local_effect()
    }

    pub fn expected_drain_obligation(&self) -> &RuntimeDrainObligationV2 {
        self.source.drain_obligation()
    }

    pub fn replacement_local_effect(&self) -> &RuntimeLocalRouteEffectV2 {
        &self.replacement_local_effect
    }

    pub fn replacement_drain_obligation(&self) -> &RuntimeDrainObligationV2 {
        &self.replacement_drain_obligation
    }
}

fn validate_sidecar_revision(
    sidecar_revision: NonZeroU64,
) -> Result<(), RuntimeSuspendedAttemptStateErrorV2> {
    RuntimePersistenceU64V2::from_non_zero(sidecar_revision)
        .map(|_| ())
        .map_err(
            |reason| RuntimeSuspendedAttemptStateErrorV2::CanonicalValue {
                field: RuntimeSuspendedAttemptStateFieldV2::SidecarRevision,
                reason,
            },
        )
}

fn validate_suspended_at(
    suspended_at: DateTime<Utc>,
) -> Result<(), RuntimeSuspendedAttemptStateErrorV2> {
    RuntimeUnixMicrosecondsV2::from_datetime(suspended_at).map_err(|reason| {
        RuntimeSuspendedAttemptStateErrorV2::CanonicalValue {
            field: RuntimeSuspendedAttemptStateFieldV2::SuspendedAt,
            reason,
        }
    })?;
    Ok(())
}

fn validate_mutable_state(
    canonical_attempt: &RuntimeCanonicalSuspendAttemptV2,
    local_effect: &RuntimeLocalRouteEffectV2,
    drain_obligation: &RuntimeDrainObligationV2,
) -> Result<(), RuntimeSuspendedAttemptStateErrorV2> {
    let root = canonical_attempt.request();
    validate_suspend_attempt_mutable_state(root, local_effect, drain_obligation)?;
    if mutable_state_is_reachable(
        &root.local_effect,
        &root.drain_obligation,
        local_effect,
        drain_obligation,
    ) {
        Ok(())
    } else {
        Err(RuntimeSuspendedAttemptStateErrorV2::UnreachableMutableState)
    }
}

fn mutable_state_is_reachable(
    root_effect: &RuntimeLocalRouteEffectV2,
    root_obligation: &RuntimeDrainObligationV2,
    current_effect: &RuntimeLocalRouteEffectV2,
    current_obligation: &RuntimeDrainObligationV2,
) -> bool {
    match (root_effect, root_obligation) {
        (RuntimeLocalRouteEffectV2::None, _) => {
            current_effect == root_effect && current_obligation == root_obligation
        }
        (RuntimeLocalRouteEffectV2::RouteAbsent { .. }, _) => {
            current_effect == root_effect && current_obligation == root_obligation
        }
        (
            RuntimeLocalRouteEffectV2::ExactRoute { route, lifecycle },
            RuntimeDrainObligationV2::ExactLocalRoute(local),
        ) if route == local => {
            (exact_route_state_matches(route, *lifecycle, current_effect, current_obligation)
                && matches!(
                    current_obligation,
                    RuntimeDrainObligationV2::ExactLocalRoute(current_local)
                        if current_local == route
                ))
                || absent_state_matches(
                    route,
                    current_effect,
                    current_obligation,
                    &RuntimeDrainObligationV2::None,
                )
        }
        (
            RuntimeLocalRouteEffectV2::ExactRoute { route, lifecycle },
            RuntimeDrainObligationV2::LocalAndPrevious { local, previous },
        ) if route == local => {
            (exact_route_state_matches(route, *lifecycle, current_effect, current_obligation)
                && matches!(
                    current_obligation,
                    RuntimeDrainObligationV2::LocalAndPrevious {
                        local: current_local,
                        previous: current_previous,
                    } if current_local == route && current_previous == previous
                ))
                || absent_state_matches(
                    route,
                    current_effect,
                    current_obligation,
                    &RuntimeDrainObligationV2::PreviousServing(previous.clone()),
                )
        }
        _ => false,
    }
}

fn exact_route_state_matches(
    root_route: &crate::RuntimeExactLocalRouteIdentityV2,
    root_lifecycle: RuntimeSuspendedRouteLifecycleV2,
    current_effect: &RuntimeLocalRouteEffectV2,
    current_obligation: &RuntimeDrainObligationV2,
) -> bool {
    let effect_matches = matches!(
        current_effect,
        RuntimeLocalRouteEffectV2::ExactRoute {
            route,
            lifecycle,
        } if route == root_route && *lifecycle == root_lifecycle
    );
    let obligation_matches = match current_obligation {
        RuntimeDrainObligationV2::ExactLocalRoute(local) => local == root_route,
        RuntimeDrainObligationV2::LocalAndPrevious { local, .. } => local == root_route,
        RuntimeDrainObligationV2::None | RuntimeDrainObligationV2::PreviousServing(_) => false,
    };
    effect_matches && obligation_matches
}

fn absent_state_matches(
    root_route: &crate::RuntimeExactLocalRouteIdentityV2,
    current_effect: &RuntimeLocalRouteEffectV2,
    current_obligation: &RuntimeDrainObligationV2,
    expected_obligation: &RuntimeDrainObligationV2,
) -> bool {
    matches!(
        current_effect,
        RuntimeLocalRouteEffectV2::RouteAbsent {
            slot,
            expected_route: Some(expected_route),
            ..
        } if slot == &root_route.slot() && expected_route == root_route
    ) && current_obligation == expected_obligation
}
