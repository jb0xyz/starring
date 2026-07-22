mod config;
mod dto;
mod failure;
mod persistence;
mod planner;
mod port;
mod retry;
mod session;
mod v2_binding;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "canonical integer helpers are consumed by staged V2 wire roots"
    )
)]
mod v2_canonical_value;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "typed digest helpers are consumed by staged V2 canonical roots"
    )
)]
mod v2_digest;
mod v2_drain;
mod v2_evidence;
mod v2_gateway;
mod v2_identity;
mod v2_product;
mod v2_product_drain_canonical;
mod v2_route;

pub use config::{RuntimeControllerConfigError, RuntimeControllerConfigV1};
pub use dto::{
    GatewayShardIdV1, PanelReportDigestV1, RuntimeAttestationIdV1, RuntimeBuildRevisionV1,
    RuntimeCertificationReceiptV1, RuntimeCertificationRequestV1, RuntimeClaimNextExecutionV1,
    RuntimeControllerDtoError, RuntimeConvergenceMutationV1, RuntimeDeploymentScopeV1,
    RuntimeDisconnectServingV1, RuntimeExecutionGuardV1, RuntimeExecutionReceiptV1,
    RuntimeExecutionUpdateReceiptV1, RuntimeHeartbeatServingV1, RuntimeLiveMetadataV1,
    RuntimeMutationReceiptV1, RuntimeMutationRequestV1, RuntimeObservePreviousServingV1,
    RuntimePreviousServingLeaseEvidenceV1, RuntimePreviousServingLeaseIdentityV1,
    RuntimePreviousServingObservationReceiptV1, RuntimePreviousServingStateV1,
    RuntimeRenewExecutionV1, RuntimeServingIdentityV1, RuntimeServingReceiptV1,
    RuntimeServingUpdateReceiptV1, RuntimeSessionActionIdV1, RuntimeStaleLiveRecoveryReceiptV1,
};
pub use failure::{
    runtime_failure_message_v1, RuntimeFailureDecisionV1, RuntimeFailureSourceV1,
    RuntimeRecordedFailureV1,
};
pub use persistence::{
    decode_runtime_live_attestation_record_v1, encode_runtime_live_attestation_record_v1,
    runtime_desired_target_digest_v1, runtime_live_attestation_digest_v1,
    RuntimeDesiredTargetDigestV1, RuntimeLiveAttestationRecordV1,
    RuntimePersistenceContractErrorV1,
};
pub use planner::{
    plan_runtime_action_v1, RuntimeControllerActionV1, RuntimeControllerPlanError,
    RuntimeControllerStopReasonV1,
};
pub use port::{
    RuntimeConvergenceErrorClassV1, RuntimeExecutionConvergencePort,
    RuntimePreviousServingObservationPort, RuntimeServingLeasePort,
};
pub use retry::{RetryPolicyError, RetryPolicyV1};
pub use session::{
    RuntimeConvergenceSessionError, RuntimeConvergenceSessionStateV1, RuntimeConvergenceSessionV1,
    RuntimeServingSessionStateV1, RuntimeServingSessionV1,
};
pub use v2_binding::RuntimeBindingPinV1;
pub use v2_canonical_value::{
    RuntimeCanonicalValueErrorV2, RuntimeServingLeaseMillisecondsV2, RuntimeUnixMicrosecondsV2,
};
pub use v2_digest::{
    RuntimeCertificationIntentFingerprintV2, RuntimeCertificationRequestDigestV2,
    RuntimeDrainIntentDigestV2, RuntimeLiveAttestationDigestV2, RuntimeProductMutationDigestV2,
    RuntimeProductSemanticRequestDigestV2, RuntimeSuspendAttemptDigestV2,
};
pub use v2_drain::{RuntimeDrainIntentKeyV2, RuntimeDrainIntentPreimageV2};
pub use v2_evidence::{
    RuntimeBarrierPauseWitnessV2, RuntimeEvidenceErrorV2, RuntimePanelEvidenceV2,
    RuntimeRouteAdmissionAttestationV2, RuntimeServingRouteAttestationV2,
};
pub use v2_gateway::{
    RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayOwnerLeaseReceiptV1,
    RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyKindV2,
};
pub use v2_identity::{
    RuntimeBarrierIdV1, RuntimeCertificationOperationIdV2, RuntimeDrainIntentIdV2,
    RuntimeGatewayAdmissionSequenceV2, RuntimeProductOperationIdV2, RuntimeRecoveryIdV2,
    RuntimeSuspensionIdV2,
};
pub use v2_product::{RuntimeProductMutationKindV2, RuntimeProductMutationPreimageV2};
pub use v2_product_drain_canonical::{
    RuntimeCanonicalProductDrainV2, RuntimeProductDrainCanonicalErrorV2,
    RuntimeProductDrainCanonicalFieldV2, RuntimeProductDrainCanonicalRootV2,
    RuntimeProductDrainCorrelationV2,
};
pub use v2_route::{RuntimeExactLocalRouteIdentityV2, RuntimeServingSlotV2};
