use automation_runtime_convergence::{
    RuntimeDeployment, RuntimeDeploymentPhaseV1, RuntimeDeploymentSnapshotV1,
    RuntimeFailureDispositionV1, RuntimePendingConditionV1,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use sqlx::types::Json;

use crate::digest::{desired_target_digest, live_attestation_digest};
use crate::model::{AttestationIdV1, AttestationRecordV1, LiveMetadataV1, RuntimeDigestV1};
use crate::RuntimeConvergenceStoreError;

pub(crate) const DEPLOYMENT_COLUMNS: &str = "deployment_id, tenant_id, installation_id, \
    promotion_id, activation_request_id, installation_authority_revision, guild_id, ruleset_key, \
    target_version, target_content_hash, binding_revision, binding_fingerprint, \
    desired_target_digest, runtime_generation, previous_runtime, requested_at, \
    snapshot_format_version, snapshot, revision, phase, controller_id, \
    controller_fencing_token, controller_acquired_at, controller_lease_expires_at, \
    last_fencing_token, next_retry_at, last_stable_error_code, live_attestation_id, live_at, \
    blocked_at, superseded_at, cancelled_at, created_at, updated_at";

#[derive(sqlx::FromRow, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeploymentRow {
    pub deployment_id: String,
    pub tenant_id: String,
    pub installation_id: String,
    pub promotion_id: String,
    pub activation_request_id: String,
    pub installation_authority_revision: i64,
    pub guild_id: String,
    pub ruleset_key: String,
    pub target_version: i64,
    pub target_content_hash: String,
    pub binding_revision: i64,
    pub binding_fingerprint: String,
    pub desired_target_digest: String,
    pub runtime_generation: i64,
    pub previous_runtime: Option<Json<Value>>,
    pub requested_at: DateTime<Utc>,
    pub snapshot_format_version: i16,
    pub snapshot: Json<Value>,
    pub revision: i64,
    pub phase: String,
    pub controller_id: Option<String>,
    pub controller_fencing_token: Option<i64>,
    pub controller_acquired_at: Option<DateTime<Utc>>,
    pub controller_lease_expires_at: Option<DateTime<Utc>>,
    pub last_fencing_token: Option<i64>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub last_stable_error_code: Option<String>,
    pub live_attestation_id: Option<String>,
    pub live_at: Option<DateTime<Utc>>,
    pub blocked_at: Option<DateTime<Utc>>,
    pub superseded_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub(crate) struct PersistedDeployment {
    pub deployment: RuntimeDeployment,
    pub installation_authority_revision: u64,
    pub desired_target_digest: RuntimeDigestV1,
    pub live_attestation_id: Option<AttestationIdV1>,
}

impl DeploymentRow {
    pub fn decode(self) -> Result<PersistedDeployment, RuntimeConvergenceStoreError> {
        if self.snapshot_format_version != 1 {
            return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "unsupported deployment snapshot format",
            ));
        }
        let snapshot = serde_json::from_value::<RuntimeDeploymentSnapshotV1>(self.snapshot.0)
            .map_err(|_| {
                RuntimeConvergenceStoreError::InvalidPersistedState("deployment snapshot JSON")
            })?;
        let deployment = RuntimeDeployment::restore(snapshot.clone())?;
        let authority_revision =
            u64::try_from(self.installation_authority_revision).map_err(|_| {
                RuntimeConvergenceStoreError::InvalidPersistedState("authority revision")
            })?;
        let persisted_desired_target_digest = RuntimeDigestV1::parse(self.desired_target_digest)?;
        let live_attestation_id = self
            .live_attestation_id
            .map(AttestationIdV1::parse)
            .transpose()?;
        let expected_previous = snapshot
            .previous_runtime
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|_| {
                RuntimeConvergenceStoreError::InvalidPersistedState("previous runtime JSON")
            })?;
        let projection = DeploymentProjection::from_snapshot(&snapshot)?;
        let row_previous = self.previous_runtime.map(|value| value.0);
        let target = &snapshot.target;
        let identity = &snapshot.identity;
        let recomputed_desired_target_digest = desired_target_digest(
            identity,
            target,
            snapshot.runtime_generation.get(),
            authority_revision,
            snapshot.previous_runtime.as_ref(),
        );
        let valid = self.deployment_id == identity.deployment_id.as_str()
            && self.tenant_id == identity.tenant_id.as_str()
            && self.installation_id == identity.installation_id.as_str()
            && self.promotion_id == identity.promotion_id.as_str()
            && self.activation_request_id == identity.activation_request_id.as_str()
            && self.guild_id == target.guild_id.to_string()
            && self.ruleset_key == target.ruleset_key.as_str()
            && self.target_version == i64::from(target.version.get())
            && self.target_content_hash == target.content_hash.to_hex()
            && self.binding_revision == runtime_i64(target.binding_revision.get())?
            && self.binding_fingerprint == target.binding_fingerprint.as_str()
            && self.runtime_generation == runtime_i64(snapshot.runtime_generation.get())?
            && self.revision == runtime_i64(snapshot.revision.get())?
            && self.phase == phase_name(&snapshot.phase)
            && row_previous == expected_previous
            && self.requested_at == snapshot.requested_at
            && self.controller_id == projection.controller_id
            && self.controller_fencing_token == projection.controller_fencing_token
            && self.controller_acquired_at == projection.controller_acquired_at
            && self.controller_lease_expires_at == projection.controller_lease_expires_at
            && self.last_fencing_token == projection.last_fencing_token
            && self.next_retry_at == projection.next_retry_at
            && self.last_stable_error_code == projection.last_stable_error_code
            && self.live_at == projection.live_at
            && self.blocked_at == projection.blocked_at
            && self.superseded_at == projection.superseded_at
            && self.cancelled_at == projection.cancelled_at
            && self.created_at <= self.updated_at
            && persisted_desired_target_digest == recomputed_desired_target_digest;
        if !valid {
            return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "deployment projections",
            ));
        }
        Ok(PersistedDeployment {
            deployment,
            installation_authority_revision: authority_revision,
            desired_target_digest: persisted_desired_target_digest,
            live_attestation_id,
        })
    }
}

pub(crate) struct DeploymentProjection {
    pub snapshot: Json<Value>,
    pub phase: &'static str,
    pub controller_id: Option<String>,
    pub controller_fencing_token: Option<i64>,
    pub controller_acquired_at: Option<DateTime<Utc>>,
    pub controller_lease_expires_at: Option<DateTime<Utc>>,
    pub last_fencing_token: Option<i64>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub last_stable_error_code: Option<String>,
    pub live_at: Option<DateTime<Utc>>,
    pub blocked_at: Option<DateTime<Utc>>,
    pub superseded_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
}

impl DeploymentProjection {
    pub fn from_snapshot(
        snapshot: &RuntimeDeploymentSnapshotV1,
    ) -> Result<Self, RuntimeConvergenceStoreError> {
        let controller = snapshot.controller_lease.as_ref();
        let (next_retry_at, blocked_at) = match &snapshot.phase {
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition:
                    RuntimePendingConditionV1::Retryable {
                        retry_not_before, ..
                    },
            } => (Some(*retry_not_before), None),
            RuntimeDeploymentPhaseV1::RuntimePending {
                condition: RuntimePendingConditionV1::Blocked { failure },
            } => (None, Some(failure.recorded_at)),
            _ => (None, None),
        };
        let last_stable_error_code =
            snapshot
                .last_runtime_failure
                .as_ref()
                .map(|failure| match failure {
                    RuntimeFailureDispositionV1::Retryable { failure, .. }
                    | RuntimeFailureDispositionV1::Blocked { failure } => failure.code.clone(),
                });
        let (superseded_at, cancelled_at) = match &snapshot.phase {
            RuntimeDeploymentPhaseV1::Superseded { superseded_at, .. } => {
                (Some(*superseded_at), None)
            }
            RuntimeDeploymentPhaseV1::Cancelled { cancelled_at, .. } => (None, Some(*cancelled_at)),
            _ => (None, None),
        };
        Ok(Self {
            snapshot: Json(serde_json::to_value(snapshot).map_err(|_| {
                RuntimeConvergenceStoreError::InvalidPersistedState(
                    "deployment snapshot serialization",
                )
            })?),
            phase: phase_name(&snapshot.phase),
            controller_id: controller.map(|lease| lease.controller_id.as_str().to_string()),
            controller_fencing_token: controller
                .map(|lease| runtime_i64(lease.fencing_token.get()))
                .transpose()?,
            controller_acquired_at: controller.map(|lease| lease.acquired_at),
            controller_lease_expires_at: controller.map(|lease| lease.expires_at),
            last_fencing_token: snapshot
                .last_fencing_token
                .map(|token| runtime_i64(token.get()))
                .transpose()?,
            next_retry_at,
            last_stable_error_code,
            live_at: snapshot.live.as_ref().map(|live| live.certified_at),
            blocked_at,
            superseded_at,
            cancelled_at,
        })
    }
}

pub(crate) const ATTESTATION_COLUMNS: &str = "attestation_id, attestation_digest, deployment_id, \
    deployment_revision, tenant_id, installation_id, promotion_id, activation_request_id, \
    guild_id, ruleset_key, target_version, target_content_hash, binding_revision, \
    binding_fingerprint, runtime_generation, controller_fencing_token, process_instance_id, \
    runtime_build_revision, panel_certificate_id, panel_report_digest, gateway_shard_id, \
    gateway_ready_kind, gateway_ready_at, certified_at, record_format_version, record, created_at";

#[derive(sqlx::FromRow, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttestationRow {
    pub attestation_id: String,
    pub attestation_digest: String,
    pub deployment_id: String,
    pub deployment_revision: i64,
    pub tenant_id: String,
    pub installation_id: String,
    pub promotion_id: String,
    pub activation_request_id: String,
    pub guild_id: String,
    pub ruleset_key: String,
    pub target_version: i64,
    pub target_content_hash: String,
    pub binding_revision: i64,
    pub binding_fingerprint: String,
    pub runtime_generation: i64,
    pub controller_fencing_token: i64,
    pub process_instance_id: String,
    pub runtime_build_revision: String,
    pub panel_certificate_id: String,
    pub panel_report_digest: String,
    pub gateway_shard_id: String,
    pub gateway_ready_kind: String,
    pub gateway_ready_at: DateTime<Utc>,
    pub certified_at: DateTime<Utc>,
    pub record_format_version: i16,
    pub record: Json<Value>,
    pub created_at: DateTime<Utc>,
}

pub(crate) struct PersistedAttestation {
    pub id: AttestationIdV1,
    pub record: AttestationRecordV1,
    pub deployment_id: String,
    pub tenant_id: String,
    pub installation_id: String,
    pub promotion_id: String,
    pub activation_request_id: String,
}

impl AttestationRow {
    pub fn decode(self) -> Result<PersistedAttestation, RuntimeConvergenceStoreError> {
        if self.record_format_version != 1 || self.attestation_id != self.attestation_digest {
            return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "attestation format or digest",
            ));
        }
        let id = AttestationIdV1::parse(self.attestation_id)?;
        let record =
            serde_json::from_value::<AttestationRecordV1>(self.record.0).map_err(|_| {
                RuntimeConvergenceStoreError::InvalidPersistedState("attestation record JSON")
            })?;
        let live = &record.live;
        let target = &live.target;
        let recomputed_id =
            AttestationIdV1::from(live_attestation_digest(&record).map_err(|_| {
                RuntimeConvergenceStoreError::InvalidPersistedState("attestation record digest")
            })?);
        let valid = self.deployment_revision == runtime_i64(record.deployment_revision.get())?
            && self.guild_id == target.guild_id.to_string()
            && self.ruleset_key == target.ruleset_key.as_str()
            && self.target_version == i64::from(target.version.get())
            && self.target_content_hash == target.content_hash.to_hex()
            && self.binding_revision == runtime_i64(target.binding_revision.get())?
            && self.binding_fingerprint == target.binding_fingerprint.as_str()
            && self.runtime_generation == runtime_i64(live.runtime_generation.get())?
            && self.controller_fencing_token == runtime_i64(record.controller_fencing_token.get())?
            && self.process_instance_id == live.process_instance_id.as_str()
            && self.runtime_build_revision == record.runtime_build_revision.as_str()
            && self.panel_certificate_id == live.panel_certificate.certificate_id.as_str()
            && self.panel_report_digest == record.panel_report_digest.as_str()
            && self.gateway_shard_id == record.gateway_shard_id.as_str()
            && self.gateway_ready_kind == gateway_ready_kind_name(live.gateway_ready.kind)
            && self.gateway_ready_at == live.gateway_ready.ready_at
            && self.certified_at == live.certified_at
            && self.certified_at == self.created_at
            && !self.deployment_id.is_empty()
            && !self.tenant_id.is_empty()
            && !self.installation_id.is_empty()
            && !self.promotion_id.is_empty()
            && !self.activation_request_id.is_empty()
            && id == recomputed_id;
        if !valid {
            return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "attestation projections",
            ));
        }
        Ok(PersistedAttestation {
            id,
            record,
            deployment_id: self.deployment_id,
            tenant_id: self.tenant_id,
            installation_id: self.installation_id,
            promotion_id: self.promotion_id,
            activation_request_id: self.activation_request_id,
        })
    }
}

pub(crate) const SERVING_LEASE_COLUMNS: &str = "guild_id, ruleset_key, tenant_id, \
    installation_id, deployment_id, attestation_id, process_instance_id, runtime_generation, \
    target_version, target_content_hash, binding_revision, binding_fingerprint, lease_epoch, \
    revision, connected, serving, acquired_at, last_heartbeat_at, expires_at";

#[derive(sqlx::FromRow, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServingLeaseRow {
    pub guild_id: String,
    pub ruleset_key: String,
    pub tenant_id: String,
    pub installation_id: String,
    pub deployment_id: String,
    pub attestation_id: String,
    pub process_instance_id: String,
    pub runtime_generation: i64,
    pub target_version: i64,
    pub target_content_hash: String,
    pub binding_revision: i64,
    pub binding_fingerprint: String,
    pub lease_epoch: i64,
    pub revision: i64,
    pub connected: bool,
    pub serving: bool,
    pub acquired_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl ServingLeaseRow {
    pub fn validate(&self) -> Result<(), RuntimeConvergenceStoreError> {
        self.checked_epoch()?;
        self.checked_revision()?;
        if self.connected != self.serving
            || self.acquired_at > self.last_heartbeat_at
            || self.last_heartbeat_at > self.expires_at
        {
            return Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                "serving lease projections",
            ));
        }
        Ok(())
    }

    pub fn checked_epoch(&self) -> Result<u64, RuntimeConvergenceStoreError> {
        positive_u64(self.lease_epoch, "serving lease epoch")
    }

    pub fn checked_revision(&self) -> Result<u64, RuntimeConvergenceStoreError> {
        positive_u64(self.revision, "serving lease revision")
    }
}

pub(crate) fn phase_name(phase: &RuntimeDeploymentPhaseV1) -> &'static str {
    match phase {
        RuntimeDeploymentPhaseV1::Requested => "requested",
        RuntimeDeploymentPhaseV1::PreflightReady => "preflight_ready",
        RuntimeDeploymentPhaseV1::DrainRequested => "drain_requested",
        RuntimeDeploymentPhaseV1::Drained => "drained",
        RuntimeDeploymentPhaseV1::ActivationApplying => "activation_applying",
        RuntimeDeploymentPhaseV1::RuntimePending { .. } => "runtime_pending",
        RuntimeDeploymentPhaseV1::ReconcilingPanels => "reconciling_panels",
        RuntimeDeploymentPhaseV1::AwaitingGatewayReady => "awaiting_gateway_ready",
        RuntimeDeploymentPhaseV1::Live => "live",
        RuntimeDeploymentPhaseV1::Superseded { .. } => "superseded",
        RuntimeDeploymentPhaseV1::Cancelled { .. } => "cancelled",
    }
}

pub(crate) fn gateway_ready_kind_name(
    kind: automation_runtime_convergence::GatewayReadyKindV1,
) -> &'static str {
    match kind {
        automation_runtime_convergence::GatewayReadyKindV1::DiscordReady => "discord_ready",
        automation_runtime_convergence::GatewayReadyKindV1::DiscordResumed => "discord_resumed",
    }
}

pub(crate) fn runtime_i64(value: u64) -> Result<i64, RuntimeConvergenceStoreError> {
    i64::try_from(value).map_err(|_| {
        RuntimeConvergenceStoreError::InvalidInput("runtime revision exceeds PostgreSQL BIGINT")
    })
}

pub(crate) fn positive_u64(
    value: i64,
    field: &'static str,
) -> Result<u64, RuntimeConvergenceStoreError> {
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or(RuntimeConvergenceStoreError::InvalidPersistedState(field))
}

pub(crate) fn metadata(record: &AttestationRecordV1) -> LiveMetadataV1 {
    LiveMetadataV1 {
        runtime_build_revision: record.runtime_build_revision.clone(),
        panel_report_digest: record.panel_report_digest.clone(),
        gateway_shard_id: record.gateway_shard_id.clone(),
    }
}
