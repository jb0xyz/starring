mod config;
mod dto;
mod failure;
mod persistence;
mod planner;
mod port;
mod retry;
mod session;
mod v2_awaiting_reset;
mod v2_binding;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "canonical integer helpers are consumed by staged V2 wire roots"
    )
)]
mod v2_canonical_value;
mod v2_certification;
mod v2_certification_canonical;
mod v2_certification_operation;
mod v2_certification_outcome;
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
mod v2_route_provenance;
mod v2_startup_recovery;
mod v2_suspension;
mod v2_writer_fence;

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
pub use v2_awaiting_reset::{
    RuntimeAwaitingGatewayReadyResetBasisKindV2, RuntimeAwaitingGatewayReadyResetBasisV2,
    RuntimeAwaitingGatewayReadyResetClassificationV2, RuntimeAwaitingGatewayReadyResetOutcomeV2,
    RuntimeAwaitingGatewayReadyResetReceiptErrorV2, RuntimeAwaitingGatewayReadyResetReceiptV2,
    RuntimeCertificationReservationResetReceiptV2, RuntimeResetAwaitingGatewayReadyV2,
};
pub use v2_binding::RuntimeBindingPinV1;
pub use v2_canonical_value::{
    RuntimeCanonicalValueErrorV2, RuntimeServingLeaseMillisecondsV2, RuntimeUnixMicrosecondsV2,
};
pub use v2_certification::{RuntimeCertificationIntentV2, RuntimeCertificationRequestV2};
pub use v2_certification_canonical::{
    RuntimeCanonicalCertificationIntentV2, RuntimeCanonicalLiveAttestationV2,
    RuntimeCertificationCanonicalErrorV2, RuntimeCertificationCanonicalFieldV2,
    RuntimeCertificationCanonicalRootV2, RuntimeCertificationIntentCorrelationV2,
    RuntimeCertificationRequestCorrelationV2, RuntimeLiveAttestationRecordV2,
};
pub use v2_certification_operation::{
    RuntimeCertificationIntentReservationOutcomeV2, RuntimeCertificationOperationBuildErrorV2,
    RuntimeCertificationOperationFieldV2, RuntimeCertificationOperationPersistenceErrorV2,
    RuntimeCertificationOperationScopeV2, RuntimeCertificationReservationObservationErrorV2,
    RuntimeCertificationReservationScopeLookupV2,
    RuntimeCertificationReservationScopeObservationKindV2,
    RuntimeCertificationReservationScopeObservationV2, RuntimeReservedCertificationIntentV2,
};
pub use v2_certification_outcome::{
    AwaitingCertificationScopeObservationV2, RuntimeCertificationDivergenceV2,
    RuntimeCertificationLookupV2, RuntimeCertificationObservationV2, RuntimeCertificationReceiptV2,
    RuntimeCertificationRecoveryDispositionV2, RuntimeServingIdentityV2, RuntimeServingReceiptV2,
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
    RuntimeAcquireGatewayOwnerLeaseOutcomeV1, RuntimeAcquireGatewayOwnerLeaseV1,
    RuntimeGatewayOwnerLeaseDurationV1, RuntimeGatewayOwnerLeaseIdV1,
    RuntimeGatewayOwnerLeaseObservationV1, RuntimeGatewayOwnerLeaseReceiptV1,
    RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyKindV2, RuntimeObserveGatewayOwnerLeaseV1,
    RuntimeObservedGatewayOwnerLeaseV1, RuntimeReleaseGatewayOwnerLeaseOutcomeV1,
    RuntimeReleaseGatewayOwnerLeaseV1, RuntimeRenewGatewayOwnerLeaseOutcomeV1,
    RuntimeRenewGatewayOwnerLeaseV1,
};
pub use v2_identity::{
    RuntimeBarrierIdV1, RuntimeCertificationOperationIdV2, RuntimeCutoverCoordinatorIdV1,
    RuntimeDrainIntentIdV2, RuntimeGatewayAdmissionSequenceV2, RuntimeProductOperationIdV2,
    RuntimeRecoveryIdV2, RuntimeSuspensionIdV2,
};
pub use v2_product::{RuntimeProductMutationKindV2, RuntimeProductMutationPreimageV2};
pub use v2_product_drain_canonical::{
    RuntimeCanonicalProductDrainV2, RuntimeProductDrainCanonicalErrorV2,
    RuntimeProductDrainCanonicalFieldV2, RuntimeProductDrainCanonicalRootV2,
    RuntimeProductDrainCorrelationV2,
};
pub use v2_route::{RuntimeExactLocalRouteIdentityV2, RuntimeServingSlotV2};
pub use v2_route_provenance::{
    RuntimeClosedRecoveryRouteWitnessV2, RuntimeRouteMutationProvenanceV2,
    RuntimeShutdownRouteWitnessV2,
};
pub use v2_startup_recovery::{
    RuntimeStartupRecoveryObservationCorrelationV2, RuntimeStartupRecoveryObservationReceiptV2,
    RuntimeStartupRecoveryObservationRequestV2, RuntimeStartupRecoveryStateV2,
    RuntimeStartupServingStateV2,
};
pub use v2_suspension::{
    RuntimeAttemptDispositionV2, RuntimeDrainObligationV2, RuntimeLocalRouteEffectV2,
    RuntimeResumeCheckpointV2, RuntimeSuspendAttemptRequestV2, RuntimeSuspendedRouteLifecycleV2,
    RuntimeSuspensionSourcePhaseV2,
};
pub use v2_writer_fence::{
    RuntimeObserveWriterFenceV1, RuntimeObservedWriterFenceClosedV1,
    RuntimeWriterFenceClosedLeaseIdV1, RuntimeWriterFenceGenerationV1,
    RuntimeWriterFenceObservationV1,
};
