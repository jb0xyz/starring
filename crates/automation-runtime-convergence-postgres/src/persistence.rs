use automation_runtime_controller::{
    runtime_desired_target_digest_v1, runtime_live_attestation_digest_v1,
    GatewayShardIdV1 as ControllerGatewayShardIdV1,
    PanelReportDigestV1 as ControllerPanelReportDigestV1,
    RuntimeBuildRevisionV1 as ControllerRuntimeBuildRevisionV1, RuntimeLiveAttestationRecordV1,
};
use automation_runtime_convergence::{
    DeploymentRevision, FencingToken, LiveAttestationV1, RuntimeDeploymentIdentityV1,
    RuntimeDeploymentTargetV1, RuntimeProcessIdentityV1,
};

use crate::model::{
    AttestationIdV1, GatewayShardIdV1, LiveMetadataV1, PanelReportDigestV1, RuntimeBuildRevisionV1,
    RuntimeDigestV1,
};
use crate::RuntimeConvergenceStoreError;

pub(crate) fn desired_target_digest_v1(
    identity: &RuntimeDeploymentIdentityV1,
    target: &RuntimeDeploymentTargetV1,
    runtime_generation: u64,
    authority_revision: u64,
    previous_runtime: Option<&RuntimeProcessIdentityV1>,
) -> Result<RuntimeDigestV1, RuntimeConvergenceStoreError> {
    RuntimeDigestV1::parse(
        runtime_desired_target_digest_v1(
            identity,
            target,
            runtime_generation,
            authority_revision,
            previous_runtime,
        )
        .as_str(),
    )
}

pub(crate) fn live_attestation_record_v1(
    live: LiveAttestationV1,
    metadata: &LiveMetadataV1,
    controller_fencing_token: FencingToken,
    deployment_revision: DeploymentRevision,
) -> Result<RuntimeLiveAttestationRecordV1, RuntimeConvergenceStoreError> {
    Ok(RuntimeLiveAttestationRecordV1 {
        live,
        runtime_build_revision: ControllerRuntimeBuildRevisionV1::parse(
            metadata.runtime_build_revision.as_str(),
        )
        .map_err(|_| invalid_record())?,
        panel_report_digest: ControllerPanelReportDigestV1::parse(
            metadata.panel_report_digest.as_str(),
        )
        .map_err(|_| invalid_record())?,
        gateway_shard_id: ControllerGatewayShardIdV1::parse(metadata.gateway_shard_id.as_str())
            .map_err(|_| invalid_record())?,
        controller_fencing_token,
        deployment_revision,
    })
}

pub(crate) fn live_attestation_id_v1(
    record: &RuntimeLiveAttestationRecordV1,
) -> Result<AttestationIdV1, RuntimeConvergenceStoreError> {
    let digest = runtime_live_attestation_digest_v1(record).map_err(|_| invalid_record())?;
    AttestationIdV1::parse(digest.as_str()).map_err(|_| invalid_record())
}

pub(crate) fn live_metadata_v1(
    record: &RuntimeLiveAttestationRecordV1,
) -> Result<LiveMetadataV1, RuntimeConvergenceStoreError> {
    Ok(LiveMetadataV1 {
        runtime_build_revision: RuntimeBuildRevisionV1::parse(
            record.runtime_build_revision.as_str(),
        )
        .map_err(|_| invalid_record())?,
        panel_report_digest: PanelReportDigestV1::parse(record.panel_report_digest.as_str())
            .map_err(|_| invalid_record())?,
        gateway_shard_id: GatewayShardIdV1::parse(record.gateway_shard_id.as_str())
            .map_err(|_| invalid_record())?,
    })
}

fn invalid_record() -> RuntimeConvergenceStoreError {
    RuntimeConvergenceStoreError::InvalidPersistedState("runtime persistence contract")
}
