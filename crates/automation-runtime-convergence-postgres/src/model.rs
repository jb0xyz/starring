use std::num::NonZeroU32;
use std::time::Duration;

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

use crate::RuntimeConvergenceStoreError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostgresRuntimeConvergenceConfigV1 {
    pub maximum_controller_lease: Duration,
    pub maximum_serving_lease: Duration,
    pub maximum_retry_delay: Duration,
    pub maximum_future_clock_skew: Duration,
    pub maximum_gateway_ready_age: Duration,
    pub statement_timeout: Duration,
    pub lock_timeout: Duration,
}

impl Default for PostgresRuntimeConvergenceConfigV1 {
    fn default() -> Self {
        Self {
            maximum_controller_lease: Duration::from_secs(120),
            maximum_serving_lease: Duration::from_secs(60),
            maximum_retry_delay: Duration::from_secs(3600),
            maximum_future_clock_skew: Duration::from_secs(30),
            maximum_gateway_ready_age: Duration::from_secs(90),
            statement_timeout: Duration::from_secs(2),
            lock_timeout: Duration::from_secs(1),
        }
    }
}

impl PostgresRuntimeConvergenceConfigV1 {
    pub(crate) fn validate(&self) -> Result<(), RuntimeConvergenceStoreError> {
        if self.maximum_controller_lease.is_zero()
            || self.maximum_serving_lease.is_zero()
            || self.maximum_retry_delay.is_zero()
            || self.maximum_gateway_ready_age.is_zero()
            || self.statement_timeout.is_zero()
            || self.lock_timeout.is_zero()
            || self.statement_timeout.as_millis() == 0
            || self.lock_timeout.as_millis() == 0
            || self.maximum_future_clock_skew > Duration::from_secs(300)
            || self.maximum_gateway_ready_age > Duration::from_secs(600)
            || self.maximum_controller_lease > Duration::from_secs(600)
            || self.maximum_serving_lease > Duration::from_secs(300)
            || self.maximum_retry_delay > Duration::from_secs(86_400)
            || self.statement_timeout > Duration::from_secs(30)
            || self.lock_timeout > self.statement_timeout
            || self
                .statement_timeout
                .checked_add(self.lock_timeout)
                .is_none_or(|minimum| {
                    self.maximum_controller_lease <= minimum
                        || self.maximum_serving_lease <= minimum
                })
        {
            return Err(RuntimeConvergenceStoreError::InvalidInput(
                "runtime convergence configuration",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDeploymentScopeV1 {
    pub tenant_id: TenantId,
    pub installation_id: InstallationId,
    pub deployment_id: DeploymentId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnqueueDeploymentV1 {
    pub identity: RuntimeDeploymentIdentityV1,
    pub target: RuntimeDeploymentTargetV1,
    pub runtime_generation: RuntimeGeneration,
    pub previous_runtime: Option<RuntimeProcessIdentityV1>,
    pub installation_authority_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnqueueDeploymentOutcomeV1 {
    Created(RuntimeDeploymentSnapshotV1),
    ExactReplay(RuntimeDeploymentSnapshotV1),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimDeploymentV1 {
    pub scope: RuntimeDeploymentScopeV1,
    pub expected_revision: DeploymentRevision,
    pub controller_id: ControllerId,
    pub lease_for: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimNextDeploymentV1 {
    pub controller_id: ControllerId,
    pub lease_for: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimReceiptV1 {
    pub snapshot: RuntimeDeploymentSnapshotV1,
    pub controller_id: ControllerId,
    pub fencing_token: FencingToken,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeploymentMutationV1 {
    AcceptPreflight(PreflightAttestationV1),
    RequestDrain,
    AcceptDrain(DrainAttestationV1),
    BeginActivation,
    AcceptActivation(ActivationAttestationV1),
    RecordRetryableFailure {
        failure_id: RuntimeFailureId,
        kind: RuntimeFailureKindV1,
        code: String,
        message: String,
        attempt: NonZeroU32,
        retry_after: Duration,
    },
    RecordBlockedFailure {
        failure_id: RuntimeFailureId,
        kind: RuntimeFailureKindV1,
        code: String,
        message: String,
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
pub struct SubmitDeploymentMutationV1 {
    pub scope: RuntimeDeploymentScopeV1,
    pub expected_revision: DeploymentRevision,
    pub controller_id: ControllerId,
    pub fencing_token: FencingToken,
    pub runtime_generation: RuntimeGeneration,
    pub mutation: DeploymentMutationV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationReceiptV1 {
    pub outcome: TransitionOutcomeV1,
    pub snapshot: RuntimeDeploymentSnapshotV1,
}

macro_rules! define_safe_text {
    ($name:ident, $error:literal) => {
        #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, RuntimeConvergenceStoreError> {
                let value = value.into();
                if value.is_empty()
                    || value.len() > 128
                    || !value.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric()
                            || matches!(byte, b'_' | b'-' | b':' | b'.' | b'/')
                    })
                {
                    return Err(RuntimeConvergenceStoreError::InvalidInput($error));
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

define_safe_text!(RuntimeBuildRevisionV1, "runtime build revision");
define_safe_text!(GatewayShardIdV1, "gateway shard identity");

macro_rules! define_lower_hex_digest {
    ($name:ident, $error:literal) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, RuntimeConvergenceStoreError> {
                let value = value.into();
                if value.len() != 64
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                {
                    return Err(RuntimeConvergenceStoreError::InvalidInput($error));
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

define_lower_hex_digest!(AttestationIdV1, "runtime attestation identity");
define_lower_hex_digest!(PanelReportDigestV1, "panel report digest");
define_lower_hex_digest!(RuntimeDigestV1, "runtime digest");

impl RuntimeDigestV1 {
    pub(crate) fn from_sha256(bytes: [u8; 32]) -> Self {
        const LOWER_HEX: &[u8; 16] = b"0123456789abcdef";
        let mut value = String::with_capacity(64);
        for byte in bytes {
            value.push(char::from(LOWER_HEX[usize::from(byte >> 4)]));
            value.push(char::from(LOWER_HEX[usize::from(byte & 0x0f)]));
        }
        Self(value)
    }
}

impl From<RuntimeDigestV1> for AttestationIdV1 {
    fn from(value: RuntimeDigestV1) -> Self {
        Self(value.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveMetadataV1 {
    pub runtime_build_revision: RuntimeBuildRevisionV1,
    pub panel_report_digest: PanelReportDigestV1,
    pub gateway_shard_id: GatewayShardIdV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmitLiveAttestationV1 {
    pub scope: RuntimeDeploymentScopeV1,
    pub expected_revision: DeploymentRevision,
    pub controller_id: ControllerId,
    pub fencing_token: FencingToken,
    pub runtime_generation: RuntimeGeneration,
    pub gateway_ready: GatewayReadyAttestationV1,
    pub metadata: LiveMetadataV1,
    pub serving_lease_for: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttestationRecordV1 {
    pub live: automation_runtime_convergence::LiveAttestationV1,
    pub runtime_build_revision: RuntimeBuildRevisionV1,
    pub panel_report_digest: PanelReportDigestV1,
    pub gateway_shard_id: GatewayShardIdV1,
    pub controller_fencing_token: FencingToken,
    pub deployment_revision: DeploymentRevision,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServingLeaseIdentityV1 {
    pub scope: RuntimeDeploymentScopeV1,
    pub attestation_id: AttestationIdV1,
    pub process_instance_id: ProcessInstanceId,
    pub lease_epoch: u64,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeartbeatServingLeaseV1 {
    pub identity: ServingLeaseIdentityV1,
    pub lease_for: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkServingDisconnectedV1 {
    pub identity: ServingLeaseIdentityV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoverStaleLiveV1 {
    pub identity: ServingLeaseIdentityV1,
    pub expected_deployment_revision: DeploymentRevision,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServingLeaseReceiptV1 {
    pub identity: ServingLeaseIdentityV1,
    pub acquired_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub connected: bool,
    pub serving: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeploymentAvailabilityV1 {
    Live,
    RuntimePending,
    Blocked,
    Superseded,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrictLiveProjectionV1 {
    pub attestation_id: AttestationIdV1,
    pub process_instance_id: ProcessInstanceId,
    pub runtime_generation: RuntimeGeneration,
    pub lease_epoch: u64,
    pub serving_revision: u64,
    pub last_heartbeat_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub metadata: LiveMetadataV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDeploymentStatusV1 {
    pub snapshot: RuntimeDeploymentSnapshotV1,
    pub observed_at: DateTime<Utc>,
    pub availability: DeploymentAvailabilityV1,
    pub reason_code: &'static str,
    pub live: Option<StrictLiveProjectionV1>,
}
