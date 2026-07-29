use std::fmt::{Debug, Formatter};

use chrono::{DateTime, Utc};

use crate::{
    DeploymentRevision, LiveLossKindV1, LiveRecoveryAttestationV1, RuntimeDeploymentError,
    RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1, SupersedingDeploymentV1,
};

use super::{RuntimeDeployment, TransitionOutcomeV1};

pub struct ProductDrainSourceSupersessionPermitV1 {
    source: RuntimeDeploymentSnapshotV1,
    acknowledged_at: DateTime<Utc>,
}

pub struct ProductDrainSourceCancellationPermitV1 {
    source: RuntimeDeploymentSnapshotV1,
    acknowledged_at: DateTime<Utc>,
}

impl ProductDrainSourceSupersessionPermitV1 {
    pub fn from_adapter_validated_durable_route_absence_acknowledgement(
        source: &RuntimeDeployment,
        expected_revision: DeploymentRevision,
        acknowledged_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeDeploymentError> {
        let snapshot = validated_product_drain_source_acknowledgement(
            source,
            expected_revision,
            acknowledged_at,
            "prove_product_drain_route_absence_acknowledgement",
        )?;
        Ok(Self {
            source: snapshot,
            acknowledged_at,
        })
    }

    fn resulting_deployment(
        &self,
        by: SupersedingDeploymentV1,
        reason: String,
        superseded_at: DateTime<Utc>,
    ) -> Result<RuntimeDeployment, RuntimeDeploymentError> {
        let source = RuntimeDeployment::restore(self.source.clone())?;
        source.validate_supersession(&by, &reason, superseded_at)?;
        if superseded_at < self.acknowledged_at {
            return Err(RuntimeDeploymentError::AttestationTimeRegression);
        }
        let mut result = self.source.clone();
        result.revision = result
            .revision
            .next()
            .map_err(|_| RuntimeDeploymentError::RevisionOverflow)?;
        if matches!(result.phase, RuntimeDeploymentPhaseV1::Live) {
            let prior_live = result
                .live
                .take()
                .ok_or(RuntimeDeploymentError::InvalidSnapshot)?;
            result.last_live_recovery = Some(LiveRecoveryAttestationV1 {
                prior_live,
                kind: LiveLossKindV1::ServingDisconnected,
                evidence_at: self.acknowledged_at,
                recovered_at: superseded_at,
            });
            result.panel_certificate = None;
            result.gateway_ready = None;
        }
        result.controller_lease = None;
        result.phase = RuntimeDeploymentPhaseV1::Superseded {
            by,
            reason,
            superseded_at,
        };
        RuntimeDeployment::restore(result)
    }
}

impl ProductDrainSourceCancellationPermitV1 {
    pub fn from_adapter_validated_durable_route_absence_acknowledgement(
        source: &RuntimeDeployment,
        expected_revision: DeploymentRevision,
        acknowledged_at: DateTime<Utc>,
    ) -> Result<Self, RuntimeDeploymentError> {
        let snapshot = validated_product_drain_source_acknowledgement(
            source,
            expected_revision,
            acknowledged_at,
            "prove_product_drain_route_absence_cancellation",
        )?;
        Ok(Self {
            source: snapshot,
            acknowledged_at,
        })
    }

    fn resulting_deployment(
        &self,
        cancelled_at: DateTime<Utc>,
    ) -> Result<RuntimeDeployment, RuntimeDeploymentError> {
        RuntimeDeployment::restore(self.source.clone())?;
        if cancelled_at < self.acknowledged_at {
            return Err(RuntimeDeploymentError::AttestationTimeRegression);
        }
        let mut result = self.source.clone();
        result.revision = result
            .revision
            .next()
            .map_err(|_| RuntimeDeploymentError::RevisionOverflow)?;
        RuntimeDeployment::restore(result)
    }
}

fn validated_product_drain_source_acknowledgement(
    source: &RuntimeDeployment,
    expected_revision: DeploymentRevision,
    acknowledged_at: DateTime<Utc>,
    operation: &'static str,
) -> Result<RuntimeDeploymentSnapshotV1, RuntimeDeploymentError> {
    source.require_revision(expected_revision)?;
    let snapshot = source.snapshot();
    let evidence_floor = match &snapshot.phase {
        RuntimeDeploymentPhaseV1::AwaitingGatewayReady => snapshot
            .panel_certificate
            .as_ref()
            .map(|certificate| certificate.reconciled_at),
        RuntimeDeploymentPhaseV1::Live => snapshot
            .live
            .as_ref()
            .map(|attestation| attestation.certified_at),
        _ => return Err(source.invalid_transition(operation)),
    }
    .ok_or(RuntimeDeploymentError::InvalidSnapshot)?;
    if acknowledged_at < evidence_floor {
        return Err(RuntimeDeploymentError::AttestationTimeRegression);
    }
    Ok(snapshot)
}

impl Debug for ProductDrainSourceSupersessionPermitV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductDrainSourceSupersessionPermitV1(<opaque>)")
    }
}

impl Debug for ProductDrainSourceCancellationPermitV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductDrainSourceCancellationPermitV1(<opaque>)")
    }
}

impl RuntimeDeployment {
    pub fn supersede_product_drain_source(
        &mut self,
        permit: ProductDrainSourceSupersessionPermitV1,
        by: SupersedingDeploymentV1,
        reason: String,
        superseded_at: DateTime<Utc>,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        let result = permit.resulting_deployment(by, reason, superseded_at)?;
        if self == &result {
            return Ok(self.replayed());
        }
        if self.snapshot() != permit.source {
            return Err(RuntimeDeploymentError::ProductDrainSupersessionSourceMismatch);
        }
        let revision = result.revision();
        let _previous = std::mem::replace(self, result);
        Ok(TransitionOutcomeV1::Applied { revision })
    }

    pub fn cancel_product_drain_source(
        &mut self,
        permit: ProductDrainSourceCancellationPermitV1,
        cancelled_at: DateTime<Utc>,
    ) -> Result<TransitionOutcomeV1, RuntimeDeploymentError> {
        if self.snapshot() != permit.source {
            return Err(RuntimeDeploymentError::ProductDrainCancellationSourceMismatch);
        }
        let result = permit.resulting_deployment(cancelled_at)?;
        let revision = result.revision();
        let _previous = std::mem::replace(self, result);
        Ok(TransitionOutcomeV1::Applied { revision })
    }
}
