use std::fmt::{Display, Formatter};
use std::num::{NonZeroU32, NonZeroU64};
use std::time::Duration;

pub use automation_runtime_convergence::PanelReportDigestV1;
use automation_runtime_convergence::{
    ActivationAttestationV1, ControllerId, DeploymentId, DeploymentRevision, DrainAttestationV1,
    FencingToken, GatewayReadyAttestationV1, InstallationId, PanelCertificateV1,
    PreflightAttestationV1, ProcessInstanceId, RuntimeDeploymentIdentityV1,
    RuntimeDeploymentSnapshotV1, RuntimeDeploymentTargetV1, RuntimeFailureId, RuntimeFailureKindV1,
    RuntimeGeneration, RuntimeProcessIdentityV1, SupersedingDeploymentV1, TenantId,
    TransitionOutcomeV1,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeControllerDtoError {
    #[error("runtime controller text field is invalid")]
    InvalidText,
    #[error("runtime controller digest field is invalid")]
    InvalidDigest,
}

macro_rules! define_safe_text {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, RuntimeControllerDtoError> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > 128
                    || !value.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric()
                            || matches!(byte, b'_' | b'-' | b':' | b'.' | b'/')
                    })
                {
                    return Err(RuntimeControllerDtoError::InvalidText);
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

macro_rules! define_digest {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, RuntimeControllerDtoError> {
                let value = value.into();
                if value.len() != 64
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    return Err(RuntimeControllerDtoError::InvalidDigest);
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::parse(value).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_safe_text!(RuntimeBuildRevisionV1);
define_safe_text!(GatewayShardIdV1);
define_digest!(RuntimeAttestationIdV1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeSessionActionIdV1(NonZeroU64);

impl RuntimeSessionActionIdV1 {
    pub(crate) fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDeploymentScopeV1 {
    pub tenant_id: TenantId,
    pub installation_id: InstallationId,
    pub deployment_id: DeploymentId,
}

impl RuntimeDeploymentScopeV1 {
    pub fn from_identity(identity: &RuntimeDeploymentIdentityV1) -> Self {
        Self {
            tenant_id: identity.tenant_id.clone(),
            installation_id: identity.installation_id.clone(),
            deployment_id: identity.deployment_id.clone(),
        }
    }

    pub fn matches(&self, identity: &RuntimeDeploymentIdentityV1) -> bool {
        self.tenant_id == identity.tenant_id
            && self.installation_id == identity.installation_id
            && self.deployment_id == identity.deployment_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeClaimNextExecutionV1 {
    pub controller_id: ControllerId,
    pub lease_for: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeExecutionReceiptV1 {
    pub snapshot: RuntimeDeploymentSnapshotV1,
    pub controller_id: ControllerId,
    pub fencing_token: FencingToken,
    pub convergence_attempt: NonZeroU32,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeExecutionGuardV1 {
    pub scope: RuntimeDeploymentScopeV1,
    pub expected_revision: DeploymentRevision,
    pub controller_id: ControllerId,
    pub fencing_token: FencingToken,
    pub runtime_generation: RuntimeGeneration,
    pub convergence_attempt: NonZeroU32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeRenewExecutionV1 {
    pub action_id: RuntimeSessionActionIdV1,
    pub guard: RuntimeExecutionGuardV1,
    pub lease_for: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeExecutionUpdateReceiptV1 {
    pub action_id: RuntimeSessionActionIdV1,
    pub execution: RuntimeExecutionReceiptV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeObservePreviousServingV1 {
    pub action_id: RuntimeSessionActionIdV1,
    pub guard: RuntimeExecutionGuardV1,
    pub expected_target: RuntimeDeploymentTargetV1,
    pub expected_previous_runtime: Option<RuntimeProcessIdentityV1>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePreviousServingLeaseIdentityV1 {
    pub scope: RuntimeDeploymentScopeV1,
    pub attestation_id: RuntimeAttestationIdV1,
    pub process: RuntimeProcessIdentityV1,
    pub lease_epoch: NonZeroU64,
    pub revision: NonZeroU64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePreviousServingLeaseEvidenceV1 {
    pub identity: RuntimePreviousServingLeaseIdentityV1,
    pub acquired_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimePreviousServingStateV1 {
    Absent,
    Disconnected {
        lease: RuntimePreviousServingLeaseEvidenceV1,
        disconnected_at: DateTime<Utc>,
    },
    Expired {
        lease: RuntimePreviousServingLeaseEvidenceV1,
        expires_at: DateTime<Utc>,
    },
    Serving {
        lease: RuntimePreviousServingLeaseEvidenceV1,
        expires_at: DateTime<Utc>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimePreviousServingObservationReceiptV1 {
    pub action_id: RuntimeSessionActionIdV1,
    pub guard: RuntimeExecutionGuardV1,
    pub observed_at: DateTime<Utc>,
    pub expected_target: RuntimeDeploymentTargetV1,
    pub expected_previous_runtime: Option<RuntimeProcessIdentityV1>,
    pub state: RuntimePreviousServingStateV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeConvergenceMutationV1 {
    AcceptPreflight(PreflightAttestationV1),
    RequestDrain,
    AcceptDrain(DrainAttestationV1),
    BeginActivation,
    AcceptActivation(ActivationAttestationV1),
    RecordRetryableFailure {
        failure_id: RuntimeFailureId,
        kind: RuntimeFailureKindV1,
        code: String,
        attempt: NonZeroU32,
        retry_after: Duration,
    },
    RecordBlockedFailure {
        failure_id: RuntimeFailureId,
        kind: RuntimeFailureKindV1,
        code: String,
    },
    ResumeRuntimePending,
    BeginPanelReconciliation,
    AcceptPanelCertificate(PanelCertificateV1),
    Supersede {
        by: SupersedingDeploymentV1,
        reason: String,
    },
    Cancel {
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeMutationRequestV1 {
    pub action_id: RuntimeSessionActionIdV1,
    pub guard: RuntimeExecutionGuardV1,
    pub mutation: RuntimeConvergenceMutationV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeMutationReceiptV1 {
    pub action_id: RuntimeSessionActionIdV1,
    pub outcome: TransitionOutcomeV1,
    pub snapshot: RuntimeDeploymentSnapshotV1,
    pub convergence_attempt: NonZeroU32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLiveMetadataV1 {
    pub runtime_build_revision: RuntimeBuildRevisionV1,
    pub panel_report_digest: PanelReportDigestV1,
    pub gateway_shard_id: GatewayShardIdV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCertificationRequestV1 {
    pub action_id: RuntimeSessionActionIdV1,
    pub guard: RuntimeExecutionGuardV1,
    pub gateway_ready: GatewayReadyAttestationV1,
    pub metadata: RuntimeLiveMetadataV1,
    pub serving_lease_for: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeServingIdentityV1 {
    pub scope: RuntimeDeploymentScopeV1,
    pub attestation_id: RuntimeAttestationIdV1,
    pub process_instance_id: ProcessInstanceId,
    pub runtime_generation: RuntimeGeneration,
    pub lease_epoch: NonZeroU64,
    pub expected_revision: NonZeroU64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeServingReceiptV1 {
    pub identity: RuntimeServingIdentityV1,
    pub runtime_generation: RuntimeGeneration,
    pub acquired_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub connected: bool,
    pub serving: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeCertificationReceiptV1 {
    pub action_id: RuntimeSessionActionIdV1,
    pub outcome: TransitionOutcomeV1,
    pub snapshot: RuntimeDeploymentSnapshotV1,
    pub convergence_attempt: NonZeroU32,
    pub metadata: RuntimeLiveMetadataV1,
    pub serving: RuntimeServingReceiptV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeHeartbeatServingV1 {
    pub action_id: RuntimeSessionActionIdV1,
    pub identity: RuntimeServingIdentityV1,
    pub lease_for: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDisconnectServingV1 {
    pub action_id: RuntimeSessionActionIdV1,
    pub identity: RuntimeServingIdentityV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeServingUpdateReceiptV1 {
    pub action_id: RuntimeSessionActionIdV1,
    pub serving: RuntimeServingReceiptV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeStaleLiveRecoveryReceiptV1 {
    pub outcome: TransitionOutcomeV1,
    pub snapshot: RuntimeDeploymentSnapshotV1,
}
