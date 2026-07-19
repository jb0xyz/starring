use std::num::NonZeroU32;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    ActivationAttestationV1, ControllerId, DeploymentRevision, DrainAttestationV1, FencingToken,
    GatewayReadyAttestationV1, LiveAttestationV1, LiveLossKindV1, LiveRecoveryAttestationV1,
    PanelCertificateV1, PreflightAttestationV1, RuntimeDeploymentIdentityV1,
    RuntimeDeploymentTargetV1, RuntimeFailureId, RuntimeGeneration, RuntimeProcessIdentityV1,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFailureKindV1 {
    EnvironmentUnavailable,
    ActivationNotObservable,
    PanelReconciliation,
    GatewayStart,
    GatewayReadyTimeout,
    InvariantViolation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeFailureV1 {
    pub failure_id: RuntimeFailureId,
    pub kind: RuntimeFailureKindV1,
    pub code: String,
    pub message: String,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "disposition", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeFailureDispositionV1 {
    Retryable {
        failure: RuntimeFailureV1,
        attempt: NonZeroU32,
        retry_not_before: DateTime<Utc>,
    },
    Blocked {
        failure: RuntimeFailureV1,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "condition", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimePendingConditionV1 {
    Ready,
    Retryable {
        failure: RuntimeFailureV1,
        attempt: NonZeroU32,
        retry_not_before: DateTime<Utc>,
    },
    Blocked {
        failure: RuntimeFailureV1,
    },
}

impl RuntimePendingConditionV1 {
    pub fn disposition(&self) -> Option<RuntimeFailureDispositionV1> {
        match self {
            Self::Ready => None,
            Self::Retryable {
                failure,
                attempt,
                retry_not_before,
            } => Some(RuntimeFailureDispositionV1::Retryable {
                failure: failure.clone(),
                attempt: *attempt,
                retry_not_before: *retry_not_before,
            }),
            Self::Blocked { failure } => Some(RuntimeFailureDispositionV1::Blocked {
                failure: failure.clone(),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeDeploymentPhaseKindV1 {
    Requested,
    PreflightReady,
    DrainRequested,
    Drained,
    ActivationApplying,
    RuntimePending,
    ReconcilingPanels,
    AwaitingGatewayReady,
    Live,
    Superseded,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuntimeDeploymentPhaseV1 {
    Requested,
    PreflightReady,
    DrainRequested,
    Drained,
    ActivationApplying,
    RuntimePending {
        condition: RuntimePendingConditionV1,
    },
    ReconcilingPanels,
    AwaitingGatewayReady,
    Live,
    Superseded {
        by: SupersedingDeploymentV1,
        reason: String,
        superseded_at: DateTime<Utc>,
    },
    Cancelled {
        reason: String,
        cancelled_at: DateTime<Utc>,
    },
}

impl RuntimeDeploymentPhaseV1 {
    pub fn kind(&self) -> RuntimeDeploymentPhaseKindV1 {
        match self {
            Self::Requested => RuntimeDeploymentPhaseKindV1::Requested,
            Self::PreflightReady => RuntimeDeploymentPhaseKindV1::PreflightReady,
            Self::DrainRequested => RuntimeDeploymentPhaseKindV1::DrainRequested,
            Self::Drained => RuntimeDeploymentPhaseKindV1::Drained,
            Self::ActivationApplying => RuntimeDeploymentPhaseKindV1::ActivationApplying,
            Self::RuntimePending { .. } => RuntimeDeploymentPhaseKindV1::RuntimePending,
            Self::ReconcilingPanels => RuntimeDeploymentPhaseKindV1::ReconcilingPanels,
            Self::AwaitingGatewayReady => RuntimeDeploymentPhaseKindV1::AwaitingGatewayReady,
            Self::Live => RuntimeDeploymentPhaseKindV1::Live,
            Self::Superseded { .. } => RuntimeDeploymentPhaseKindV1::Superseded,
            Self::Cancelled { .. } => RuntimeDeploymentPhaseKindV1::Cancelled,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Live | Self::Superseded { .. } | Self::Cancelled { .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerLeaseV1 {
    pub controller_id: ControllerId,
    pub fencing_token: FencingToken,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaseRequestV1 {
    pub expected_revision: DeploymentRevision,
    pub controller_id: ControllerId,
    pub fencing_token: FencingToken,
    pub now: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandGuardV1 {
    pub expected_revision: DeploymentRevision,
    pub controller_id: ControllerId,
    pub fencing_token: FencingToken,
    pub runtime_generation: RuntimeGeneration,
    pub now: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoverLiveRequestV1 {
    pub expected_revision: DeploymentRevision,
    pub expected_runtime_generation: RuntimeGeneration,
    pub expected_process_instance_id: crate::ProcessInstanceId,
    pub kind: LiveLossKindV1,
    pub evidence_at: DateTime<Utc>,
    pub recovered_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupersedingDeploymentV1 {
    pub identity: RuntimeDeploymentIdentityV1,
    pub target: RuntimeDeploymentTargetV1,
    pub runtime_generation: RuntimeGeneration,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeDeploymentSnapshotV1 {
    pub identity: RuntimeDeploymentIdentityV1,
    pub target: RuntimeDeploymentTargetV1,
    pub runtime_generation: RuntimeGeneration,
    pub previous_runtime: Option<RuntimeProcessIdentityV1>,
    pub requested_at: DateTime<Utc>,
    pub revision: DeploymentRevision,
    pub phase: RuntimeDeploymentPhaseV1,
    pub controller_lease: Option<ControllerLeaseV1>,
    pub last_fencing_token: Option<FencingToken>,
    pub preflight: Option<PreflightAttestationV1>,
    pub drain: Option<DrainAttestationV1>,
    pub activation: Option<ActivationAttestationV1>,
    pub panel_certificate: Option<PanelCertificateV1>,
    pub gateway_ready: Option<GatewayReadyAttestationV1>,
    pub live: Option<LiveAttestationV1>,
    pub last_live_recovery: Option<LiveRecoveryAttestationV1>,
    pub last_runtime_failure: Option<RuntimeFailureDispositionV1>,
}
