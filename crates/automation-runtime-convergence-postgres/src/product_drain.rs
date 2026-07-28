use std::fmt::{Debug, Formatter};

use automation_runtime_controller::RuntimeUnixMicrosecondsV2;
use automation_runtime_convergence::{
    DeploymentRevision, ProductDrainSourceSupersessionPermitV1, RuntimeDeployment,
    RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1, SupersedingDeploymentV1,
    TransitionOutcomeV1,
};
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::prepare::{prepare_runtime_deployment_snapshot_v1, PreparedRuntimeDeploymentSnapshotV1};
use crate::RuntimeConvergenceStoreError;

#[derive(Clone, PartialEq, Eq)]
pub struct PreparedProductDrainSourceSupersessionV1 {
    prepared_snapshot: PreparedRuntimeDeploymentSnapshotV1,
    resulting_revision: DeploymentRevision,
}

impl PreparedProductDrainSourceSupersessionV1 {
    pub fn snapshot(&self) -> &RuntimeDeploymentSnapshotV1 {
        self.prepared_snapshot.snapshot()
    }

    pub fn resulting_revision(&self) -> DeploymentRevision {
        self.resulting_revision
    }

    pub fn snapshot_json(&self) -> &Value {
        self.prepared_snapshot.snapshot_json()
    }

    pub fn snapshot_bytes(&self) -> &[u8] {
        self.prepared_snapshot.snapshot_bytes()
    }

    pub fn snapshot_digest(&self) -> &str {
        self.prepared_snapshot.snapshot_digest()
    }
}

impl Debug for PreparedProductDrainSourceSupersessionV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PreparedProductDrainSourceSupersessionV1(<opaque>)")
    }
}

pub fn prepare_product_drain_source_supersession_v1(
    locked_source: RuntimeDeploymentSnapshotV1,
    expected_revision: DeploymentRevision,
    acknowledged_at: DateTime<Utc>,
    successor: SupersedingDeploymentV1,
    reason: String,
    terminal_database_time: DateTime<Utc>,
) -> Result<PreparedProductDrainSourceSupersessionV1, RuntimeConvergenceStoreError> {
    RuntimeUnixMicrosecondsV2::from_datetime(acknowledged_at).map_err(|_| {
        RuntimeConvergenceStoreError::InvalidInput(
            "Product drain acknowledgement database timestamp",
        )
    })?;
    RuntimeUnixMicrosecondsV2::from_datetime(terminal_database_time).map_err(|_| {
        RuntimeConvergenceStoreError::InvalidInput("Product drain terminal database timestamp")
    })?;
    let mut deployment = RuntimeDeployment::restore(locked_source)?;
    let permit =
        ProductDrainSourceSupersessionPermitV1::from_adapter_validated_durable_route_absence_acknowledgement(
            &deployment,
            expected_revision,
            acknowledged_at,
        )?;
    let resulting_revision = expected_revision
        .next()
        .map_err(|_| RuntimeConvergenceStoreError::InvalidInput("runtime deployment revision"))?;
    if i64::try_from(resulting_revision.get()).is_err()
        || i64::try_from(deployment.runtime_generation().get()).is_err()
        || i64::try_from(deployment.target().binding_revision.get()).is_err()
        || i64::try_from(successor.runtime_generation.get()).is_err()
        || i64::try_from(successor.target.binding_revision.get()).is_err()
    {
        return Err(RuntimeConvergenceStoreError::InvalidInput(
            "runtime deployment projection",
        ));
    }
    let outcome = deployment.supersede_product_drain_source(
        permit,
        successor.clone(),
        reason.clone(),
        terminal_database_time,
    )?;
    if outcome
        != (TransitionOutcomeV1::Applied {
            revision: resulting_revision,
        })
    {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
            "Product drain source supersession outcome",
        ));
    }
    let snapshot = deployment.snapshot();
    if snapshot.revision != resulting_revision
        || !matches!(
            &snapshot.phase,
            RuntimeDeploymentPhaseV1::Superseded {
                by,
                reason: actual_reason,
                superseded_at,
            } if by == &successor
                && actual_reason == &reason
                && *superseded_at == terminal_database_time
        )
    {
        return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
            "Product drain source supersession projection",
        ));
    }
    let prepared_snapshot = prepare_runtime_deployment_snapshot_v1(snapshot)?;
    Ok(PreparedProductDrainSourceSupersessionV1 {
        prepared_snapshot,
        resulting_revision,
    })
}
