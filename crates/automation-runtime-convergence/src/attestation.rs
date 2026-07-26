use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    ActivationRequestId, PanelCertificateId, PanelReportDigestV1, ProcessInstanceId,
    RuntimeDeploymentTargetV1, RuntimeGeneration, RuntimeProcessIdentityV1,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreflightAttestationV1 {
    pub target: RuntimeDeploymentTargetV1,
    pub runtime_generation: RuntimeGeneration,
    pub observed_runtime: Option<RuntimeProcessIdentityV1>,
    pub checked_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrainAttestationV1 {
    pub previous_runtime: Option<RuntimeProcessIdentityV1>,
    pub target_runtime_generation: RuntimeGeneration,
    pub drained_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationOutcomeKindV1 {
    Activated,
    AlreadyActive,
    CrashRecovered,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationAttestationV1 {
    pub activation_request_id: ActivationRequestId,
    pub target: RuntimeDeploymentTargetV1,
    pub runtime_generation: RuntimeGeneration,
    pub kind: ActivationOutcomeKindV1,
    pub activated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelCertificateV1 {
    pub certificate_id: PanelCertificateId,
    pub report_digest: PanelReportDigestV1,
    pub target: RuntimeDeploymentTargetV1,
    pub runtime_generation: RuntimeGeneration,
    pub process_instance_id: ProcessInstanceId,
    pub declared_count: u32,
    pub installed_count: u32,
    pub unchanged_count: u32,
    pub skipped_transient_count: u32,
    pub skipped_unresolved_channel_count: u32,
    pub failed_count: u32,
    pub ambiguous_outcome_count: u32,
    pub stale_message_cleanup_pending_count: u32,
    pub orphan_message_cleanup_pending_count: u32,
    pub reposted_old_message_cleanup_pending_count: u32,
    pub reconciled_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayReadyKindV1 {
    DiscordReady,
    DiscordResumed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayReadyAttestationV1 {
    pub target: RuntimeDeploymentTargetV1,
    pub runtime_generation: RuntimeGeneration,
    pub process_instance_id: ProcessInstanceId,
    pub kind: GatewayReadyKindV1,
    pub ready_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveAttestationV1 {
    pub target: RuntimeDeploymentTargetV1,
    pub runtime_generation: RuntimeGeneration,
    pub process_instance_id: ProcessInstanceId,
    pub activation: ActivationAttestationV1,
    pub panel_certificate: PanelCertificateV1,
    pub gateway_ready: GatewayReadyAttestationV1,
    pub certified_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveLossKindV1 {
    ServingLeaseExpired,
    ServingDisconnected,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveRecoveryAttestationV1 {
    pub prior_live: LiveAttestationV1,
    pub kind: LiveLossKindV1,
    pub evidence_at: DateTime<Utc>,
    pub recovered_at: DateTime<Utc>,
}
