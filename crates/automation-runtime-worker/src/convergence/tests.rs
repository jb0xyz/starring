use std::collections::VecDeque;
use std::future::{ready, Future};
use std::num::{NonZeroU32, NonZeroU64};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use automation_runtime_controller::{
    runtime_desired_target_digest_v1, RuntimeAttestationIdV1, RuntimeBindingPinV1,
    RuntimeCanonicalProductDrainV2, RuntimeControllerConfigV1, RuntimeConvergenceMutationV1,
    RuntimeDeploymentScopeV1, RuntimeDesiredTargetDigestV1, RuntimeDisconnectServingV1,
    RuntimeDrainIntentDigestV2, RuntimeExecutionGuardV1, RuntimeExecutionReceiptV1,
    RuntimeMutationReceiptV1, RuntimeMutationRequestV1, RuntimePreviousServingLeaseEvidenceV1,
    RuntimePreviousServingLeaseIdentityV1, RuntimePreviousServingObservationReceiptV1,
    RuntimePreviousServingStateV1, RuntimeProductMutationDigestV2, RuntimeServingReceiptV1,
    RuntimeServingSlotV2, RuntimeServingUpdateReceiptV1,
};
use automation_runtime_convergence::{
    ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
    CommandGuardV1, ControllerId, DeploymentId, DrainAttestationV1, FencingToken, InstallationId,
    LeaseRequestV1, PanelCertificateId, PanelCertificateV1, PanelReportDigestV1, ProcessInstanceId,
    PromotionId, RuntimeDeployment, RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1,
    RuntimeFailureId, RuntimeFailureKindV1, RuntimeFailureV1, RuntimeGeneration,
    RuntimePendingConditionV1, TenantId,
};
use chrono::{DateTime, Utc};

use super::*;
use crate::production_lifecycle::{
    runtime_serving_slot_work_test_authority_v2, RuntimeServingSlotWorkTestHandleV2,
};

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn target() -> RuntimeDeploymentTargetV1 {
    let product = br#"{"format_version":2,"operation_id":"00112233445566778899aabbccddeeff","scope":{"tenant_id":"tenant:1","installation_id":"installation:1","deployment_id":"deployment:1"},"expected_revision":11,"slot":{"guild_id":"9223372036854775808","ruleset_key":"study"},"expected_target":{"guild_id":"9223372036854775808","ruleset_key":"study","version":1,"content_hash":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","binding_revision":3,"binding_fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"mutation_kind":"authority_change","product_semantic_request_digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"}"#;
    let drain = br#"{"format_version":2,"key":{"intent_id":"ffeeddccbbaa99887766554433221100","product_operation_id":"00112233445566778899aabbccddeeff","product_mutation_digest":"0d703a8b41ea72fd1398e8868e61a4f43c0a7a95455e8fa266c439c7d7763a1c","scope":{"tenant_id":"tenant:1","installation_id":"installation:1","deployment_id":"deployment:1"},"expected_revision":11,"slot":{"guild_id":"9223372036854775808","ruleset_key":"study"},"expected_target":{"guild_id":"9223372036854775808","ruleset_key":"study","version":1,"content_hash":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","binding_revision":3,"binding_fingerprint":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"mutation_kind":"authority_change"}}"#;
    RuntimeCanonicalProductDrainV2::from_persisted(
        product,
        &RuntimeProductMutationDigestV2::parse(
            "0d703a8b41ea72fd1398e8868e61a4f43c0a7a95455e8fa266c439c7d7763a1c",
        )
        .unwrap(),
        drain,
        &RuntimeDrainIntentDigestV2::parse(
            "91bf01157dcc984e89ddc91e8cfdd66ad4eff0b3f8c093cd2198970dbbcc4168",
        )
        .unwrap(),
    )
    .unwrap()
    .product_preimage()
    .expected_target
    .clone()
}

fn identity() -> RuntimeDeploymentIdentityV1 {
    RuntimeDeploymentIdentityV1 {
        deployment_id: DeploymentId::parse("deployment:1").unwrap(),
        tenant_id: TenantId::parse("tenant:1").unwrap(),
        installation_id: InstallationId::parse("installation:1").unwrap(),
        promotion_id: PromotionId::parse("1".repeat(64)).unwrap(),
        activation_request_id: ActivationRequestId::parse("activation:1").unwrap(),
    }
}

fn receipt() -> RuntimeExecutionReceiptV1 {
    let mut deployment =
        RuntimeDeployment::request(identity(), target(), RuntimeGeneration::FIRST, None, at(1))
            .unwrap();
    let controller_id = ControllerId::parse("controller:1").unwrap();
    let fencing_token = FencingToken::new(1).unwrap();
    deployment
        .acquire_lease(LeaseRequestV1 {
            expected_revision: deployment.revision(),
            controller_id: controller_id.clone(),
            fencing_token,
            now: at(10),
            expires_at: at(100),
        })
        .unwrap();
    RuntimeExecutionReceiptV1 {
        snapshot: deployment.snapshot(),
        controller_id,
        fencing_token,
        convergence_attempt: NonZeroU32::MIN,
        acquired_at: at(10),
        expires_at: at(100),
    }
}

fn receipt_with_previous(previous_runtime: RuntimeProcessIdentityV1) -> RuntimeExecutionReceiptV1 {
    let mut deployment = RuntimeDeployment::request(
        identity(),
        target(),
        previous_runtime.runtime_generation.next().unwrap(),
        Some(previous_runtime),
        at(1),
    )
    .unwrap();
    let controller_id = ControllerId::parse("controller:1").unwrap();
    let fencing_token = FencingToken::new(1).unwrap();
    deployment
        .acquire_lease(LeaseRequestV1 {
            expected_revision: deployment.revision(),
            controller_id: controller_id.clone(),
            fencing_token,
            now: at(10),
            expires_at: at(100),
        })
        .unwrap();
    RuntimeExecutionReceiptV1 {
        snapshot: deployment.snapshot(),
        controller_id,
        fencing_token,
        convergence_attempt: NonZeroU32::MIN,
        acquired_at: at(10),
        expires_at: at(100),
    }
}

fn claimed() -> (
    RuntimeClaimedConvergenceV2,
    RuntimeServingSlotWorkTestHandleV2,
    RuntimeExecutionReceiptV1,
) {
    let receipt = receipt();
    claimed_from_receipt(receipt, at(20))
}

fn claimed_from_receipt(
    receipt: RuntimeExecutionReceiptV1,
    observed_at: DateTime<Utc>,
) -> (
    RuntimeClaimedConvergenceV2,
    RuntimeServingSlotWorkTestHandleV2,
    RuntimeExecutionReceiptV1,
) {
    let slot = RuntimeServingSlotV2::from_target(&receipt.snapshot.target);
    let (permit, handle) = runtime_serving_slot_work_test_authority_v2(
        slot,
        ProcessInstanceId::parse("runtime:1").unwrap(),
    );
    let claimed = RuntimeClaimedConvergenceV2::from_claim(
        receipt.clone(),
        permit,
        observed_at,
        RuntimeControllerConfigV1::default(),
    )
    .unwrap();
    (claimed, handle, receipt)
}

fn binding_pin(receipt: &RuntimeExecutionReceiptV1) -> RuntimeBindingPinV1 {
    RuntimeBindingPinV1 {
        tenant_id: receipt.snapshot.identity.tenant_id.clone(),
        installation_id: receipt.snapshot.identity.installation_id.clone(),
        installation_authority_revision: NonZeroU64::new(7).unwrap(),
        binding_revision: receipt.snapshot.target.binding_revision,
        binding_fingerprint: receipt.snapshot.target.binding_fingerprint.clone(),
    }
}

fn hydration_observation(
    receipt: &RuntimeExecutionReceiptV1,
) -> RuntimeExactTargetObservationV2<u64> {
    let authority_revision = NonZeroU64::new(7).unwrap();
    RuntimeExactTargetObservationV2 {
        execution: receipt.clone(),
        persisted_desired_target_digest: runtime_desired_target_digest_v1(
            &receipt.snapshot.identity,
            &receipt.snapshot.target,
            receipt.snapshot.runtime_generation.get(),
            authority_revision.get(),
            receipt.snapshot.previous_runtime.as_ref(),
        ),
        installation_authority_revision: authority_revision,
        current_authority_revision: NonZeroU64::new(8).unwrap(),
        installation_authority_payload_digest: RuntimeAuthorityPayloadDigestV2::parse(
            "d".repeat(64),
        )
        .unwrap(),
        current_authority_payload_digest: RuntimeAuthorityPayloadDigestV2::parse("e".repeat(64))
            .unwrap(),
        artifact_target: receipt.snapshot.target.clone(),
        binding_pin: binding_pin(receipt),
        observed_database_now: at(21),
        hydrated: 41,
    }
}

struct HydrationPort {
    observation: Mutex<Option<RuntimeExactTargetObservationV2<u64>>>,
    seal: Option<RuntimeServingSlotWorkTestHandleV2>,
}

impl RuntimeExactTargetHydrationPortV2 for HydrationPort {
    type Error = ();
    type Hydrated = u64;

    fn load_exact_target<'a>(
        &'a self,
        request: &'a RuntimeExactTargetHydrationRequestV2,
    ) -> RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimeExactTargetObservationV2<Self::Hydrated>, Self::Error>,
    > {
        assert_eq!(request.execution().snapshot.target, target());
        if let Some(handle) = &self.seal {
            handle.seal();
        }
        let observation = self
            .observation
            .lock()
            .unwrap()
            .take()
            .expect("one hydration observation");
        Box::pin(ready(Ok(observation)))
    }
}

struct PreflightPort {
    observation: Mutex<Option<RuntimeDiscordPreflightObservationV2>>,
}

impl RuntimeDiscordPreflightPortV2<u64> for PreflightPort {
    type Error = ();

    fn verify_preflight<'a>(
        &'a self,
        request: &'a RuntimeDiscordPreflightRequestV2,
        hydrated: &'a u64,
    ) -> RuntimeConvergenceFutureV2<'a, Result<RuntimeDiscordPreflightObservationV2, Self::Error>>
    {
        assert_eq!(*hydrated, 41);
        assert_eq!(request.target(), &target());
        let observation = self
            .observation
            .lock()
            .unwrap()
            .take()
            .expect("one preflight observation");
        Box::pin(ready(Ok(observation)))
    }
}

struct MutationPort {
    requested_snapshot: automation_runtime_convergence::RuntimeDeploymentSnapshotV1,
}

impl RuntimeConvergenceMutationPortV2 for MutationPort {
    type Error = ();

    fn mutate<'a>(
        &'a self,
        request: &'a RuntimeMutationRequestV1,
    ) -> RuntimeConvergenceFutureV2<'a, Result<RuntimeMutationReceiptV1, Self::Error>> {
        let attestation = match &request.mutation {
            RuntimeConvergenceMutationV1::AcceptPreflight(attestation) => attestation.clone(),
            _ => panic!("unexpected mutation"),
        };
        let mut deployment = RuntimeDeployment::restore(self.requested_snapshot.clone()).unwrap();
        let outcome = deployment
            .accept_preflight(
                &CommandGuardV1 {
                    expected_revision: request.guard.expected_revision,
                    controller_id: request.guard.controller_id.clone(),
                    fencing_token: request.guard.fencing_token,
                    runtime_generation: request.guard.runtime_generation,
                    now: attestation.checked_at,
                },
                attestation,
            )
            .unwrap();
        Box::pin(ready(Ok(RuntimeMutationReceiptV1 {
            action_id: request.action_id,
            outcome,
            snapshot: deployment.snapshot(),
            convergence_attempt: request.guard.convergence_attempt,
        })))
    }
}

struct StagedGuard {
    drops: Arc<AtomicUsize>,
    active_drops: Option<(RuntimeServingSlotWorkTestHandleV2, Arc<AtomicUsize>)>,
}

impl Drop for StagedGuard {
    fn drop(&mut self) {
        if let Some((handle, active_drops)) = &self.active_drops {
            if handle.active_count() == 1 {
                active_drops.fetch_add(1, Ordering::SeqCst);
            }
        }
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

struct StagePort {
    lifecycle: RuntimeRouteLifecycleV2,
    drops: Arc<AtomicUsize>,
    expected_hydrated: u64,
    active_drops: Option<(RuntimeServingSlotWorkTestHandleV2, Arc<AtomicUsize>)>,
}

impl RuntimeStagedRoutePortV2<u64> for StagePort {
    type Error = ();
    type Staged = StagedGuard;

    fn install_staged(
        &self,
        request: &RuntimeRouteStageRequestV2,
        hydrated: &u64,
    ) -> Result<RuntimeRouteStageObservationV2<Self::Staged>, Self::Error> {
        assert_eq!(*hydrated, self.expected_hydrated);
        Ok(RuntimeRouteStageObservationV2 {
            outcome: RuntimeRouteStageOutcomeV2::Installed,
            witness: RuntimeRouteWitnessV2 {
                identity: request.process_identity().clone(),
                controller_fencing_token: request.execution_guard().fencing_token,
                route_incarnation: NonZeroU64::new(2).unwrap(),
                lifecycle: self.lifecycle.clone(),
                active_interactions: 0,
                admission_generation: NonZeroU64::new(3).unwrap(),
                registry_observation_sequence: request.route_set_sequence().as_non_zero(),
            },
            staged: StagedGuard {
                drops: Arc::clone(&self.drops),
                active_drops: self.active_drops.clone(),
            },
        })
    }
}

fn block_on_ready<T>(future: impl Future<Output = T>) -> T {
    let mut future = Box::pin(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("test future must be ready"),
    }
}

fn stage_ready() -> (
    RuntimeStageReadyConvergenceV2<u64>,
    RuntimeServingSlotWorkTestHandleV2,
    RuntimeExecutionReceiptV1,
) {
    let (claimed, handle, receipt) = claimed();
    let hydration = claimed.begin_hydration().unwrap();
    let hydrated = block_on_ready(hydration.execute(&HydrationPort {
        observation: Mutex::new(Some(hydration_observation(&receipt))),
        seal: None,
    }))
    .unwrap();
    let preflight = hydrated.begin_discord_preflight(at(22)).unwrap();
    let outcome = block_on_ready(preflight.execute(&PreflightPort {
        observation: Mutex::new(Some(RuntimeDiscordPreflightObservationV2 {
            target: receipt.snapshot.target.clone(),
            runtime_generation: receipt.snapshot.runtime_generation,
            binding_pin: binding_pin(&receipt),
            checked_at: at(23),
        })),
    }))
    .unwrap();
    let preflighted = match outcome {
        RuntimeDiscordPreflightOutcomeV2::AcceptPreflight(preflighted) => *preflighted,
        RuntimeDiscordPreflightOutcomeV2::StageReady(_) => panic!("mutation must be required"),
    };
    let mutation = preflighted.begin_accept_preflight().unwrap();
    let stage_ready = block_on_ready(mutation.execute(
        &MutationPort {
            requested_snapshot: receipt.snapshot.clone(),
        },
        at(24),
    ))
    .unwrap();
    let receipt = preflight_ready_receipt(&receipt, at(23));
    (stage_ready, handle, receipt)
}

fn preflight_ready_receipt(
    receipt: &RuntimeExecutionReceiptV1,
    checked_at: DateTime<Utc>,
) -> RuntimeExecutionReceiptV1 {
    let attestation = automation_runtime_convergence::PreflightAttestationV1 {
        target: receipt.snapshot.target.clone(),
        runtime_generation: receipt.snapshot.runtime_generation,
        observed_runtime: receipt.snapshot.previous_runtime.clone(),
        checked_at,
    };
    let mut deployment = RuntimeDeployment::restore(receipt.snapshot.clone()).unwrap();
    deployment
        .accept_preflight(
            &CommandGuardV1 {
                expected_revision: receipt.snapshot.revision,
                controller_id: receipt.controller_id.clone(),
                fencing_token: receipt.fencing_token,
                runtime_generation: receipt.snapshot.runtime_generation,
                now: checked_at,
            },
            attestation,
        )
        .unwrap();
    let mut receipt = receipt.clone();
    receipt.snapshot = deployment.snapshot();
    receipt
}

fn refresh_stage_ready(
    stage_ready: RuntimeStageReadyConvergenceV2<u64>,
    receipt: &RuntimeExecutionReceiptV1,
    observed_database_now: DateTime<Utc>,
    hydrated: u64,
) -> RuntimeRefreshedStageReadyConvergenceV2<u64> {
    let mut observation = hydration_observation(receipt);
    observation.observed_database_now = observed_database_now;
    observation.hydrated = hydrated;
    block_on_ready(
        stage_ready
            .begin_exact_hydration_refresh()
            .unwrap()
            .execute(&HydrationPort {
                observation: Mutex::new(Some(observation)),
                seal: None,
            }),
    )
    .unwrap()
}

fn drifted_stage_refresh(
    mutate: impl FnOnce(&mut RuntimeExactTargetObservationV2<u64>),
) -> RuntimeStageReadyHydrationRefreshResultV2<u64, ()> {
    let (stage_ready, _handle, receipt) = stage_ready();
    let mut observation = hydration_observation(&receipt);
    observation.observed_database_now = at(25);
    mutate(&mut observation);
    block_on_ready(
        stage_ready
            .begin_exact_hydration_refresh()
            .unwrap()
            .execute(&HydrationPort {
                observation: Mutex::new(Some(observation)),
                seal: None,
            }),
    )
}

#[test]
fn requested_exact_slice_reaches_staged_with_owned_guard() {
    let drops = Arc::new(AtomicUsize::new(0));
    let (stage_ready, _handle, receipt) = stage_ready();
    let stage_ready = refresh_stage_ready(stage_ready, &receipt, at(25), 42);
    let staged = stage_ready
        .stage(
            &StagePort {
                lifecycle: RuntimeRouteLifecycleV2::Staged,
                drops: Arc::clone(&drops),
                expected_hydrated: 42,
                active_drops: None,
            },
            at(26),
        )
        .unwrap();

    assert!(matches!(
        staged.witness().lifecycle,
        RuntimeRouteLifecycleV2::Staged
    ));
    assert_eq!(staged.witness().active_interactions, 0);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    staged.ensure_active().unwrap();
    drop(staged);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn desired_target_drift_fails_before_preflight() {
    let (claimed, _handle, receipt) = claimed();
    let mut observation = hydration_observation(&receipt);
    observation.persisted_desired_target_digest =
        RuntimeDesiredTargetDigestV1::parse("f".repeat(64)).unwrap();
    let result = block_on_ready(claimed.begin_hydration().unwrap().execute(&HydrationPort {
        observation: Mutex::new(Some(observation)),
        seal: None,
    }));

    assert!(matches!(
        result,
        Err(RuntimeExactTargetHydrationErrorV2::Evidence(
            RuntimeExactTargetEvidenceErrorV2::DesiredTargetDigestMismatch
        ))
    ));
}

#[test]
fn preflight_binding_drift_fails_before_mutation() {
    let (claimed, _handle, receipt) = claimed();
    let hydrated = block_on_ready(claimed.begin_hydration().unwrap().execute(&HydrationPort {
        observation: Mutex::new(Some(hydration_observation(&receipt))),
        seal: None,
    }))
    .unwrap();
    let mut wrong_pin = binding_pin(&receipt);
    wrong_pin.installation_authority_revision = NonZeroU64::new(9).unwrap();
    let result = block_on_ready(hydrated.begin_discord_preflight(at(22)).unwrap().execute(
        &PreflightPort {
            observation: Mutex::new(Some(RuntimeDiscordPreflightObservationV2 {
                target: receipt.snapshot.target.clone(),
                runtime_generation: receipt.snapshot.runtime_generation,
                binding_pin: wrong_pin,
                checked_at: at(23),
            })),
        },
    ));

    assert!(matches!(
        result,
        Err(RuntimeDiscordPreflightErrorV2::Evidence(
            RuntimeDiscordPreflightEvidenceErrorV2::BindingPinMismatch
        ))
    ));
}

#[test]
fn preflight_ready_replay_rechecks_discord_and_stages_without_mutation() {
    let mut receipt = receipt();
    let durable = automation_runtime_convergence::PreflightAttestationV1 {
        target: receipt.snapshot.target.clone(),
        runtime_generation: receipt.snapshot.runtime_generation,
        observed_runtime: receipt.snapshot.previous_runtime.clone(),
        checked_at: at(23),
    };
    let mut deployment = RuntimeDeployment::restore(receipt.snapshot.clone()).unwrap();
    deployment
        .accept_preflight(
            &CommandGuardV1 {
                expected_revision: receipt.snapshot.revision,
                controller_id: receipt.controller_id.clone(),
                fencing_token: receipt.fencing_token,
                runtime_generation: receipt.snapshot.runtime_generation,
                now: at(23),
            },
            durable,
        )
        .unwrap();
    receipt.snapshot = deployment.snapshot();
    let (claimed, _handle, receipt) = claimed_from_receipt(receipt, at(24));
    assert_eq!(
        claimed.claim_kind(),
        RuntimeConvergenceClaimKindV2::PreflightReady
    );
    let mut exact = hydration_observation(&receipt);
    exact.observed_database_now = at(25);
    let hydrated = block_on_ready(claimed.begin_hydration().unwrap().execute(&HydrationPort {
        observation: Mutex::new(Some(exact)),
        seal: None,
    }))
    .unwrap();
    let outcome = block_on_ready(hydrated.begin_discord_preflight(at(26)).unwrap().execute(
        &PreflightPort {
            observation: Mutex::new(Some(RuntimeDiscordPreflightObservationV2 {
                target: receipt.snapshot.target.clone(),
                runtime_generation: receipt.snapshot.runtime_generation,
                binding_pin: binding_pin(&receipt),
                checked_at: at(27),
            })),
        },
    ))
    .unwrap();
    let stage_ready = match outcome {
        RuntimeDiscordPreflightOutcomeV2::StageReady(stage_ready) => *stage_ready,
        RuntimeDiscordPreflightOutcomeV2::AcceptPreflight(_) => {
            panic!("durable preflight must not be mutated again")
        }
    };
    let mut refreshed = hydration_observation(&receipt);
    refreshed.observed_database_now = at(28);
    refreshed.hydrated = 42;
    let stage_ready = block_on_ready(
        stage_ready
            .begin_exact_hydration_refresh()
            .unwrap()
            .execute(&HydrationPort {
                observation: Mutex::new(Some(refreshed)),
                seal: None,
            }),
    )
    .unwrap();
    let drops = Arc::new(AtomicUsize::new(0));
    let staged = stage_ready
        .stage(
            &StagePort {
                lifecycle: RuntimeRouteLifecycleV2::Staged,
                drops: Arc::clone(&drops),
                expected_hydrated: 42,
                active_drops: None,
            },
            at(29),
        )
        .unwrap();
    assert!(matches!(
        staged.witness().lifecycle,
        RuntimeRouteLifecycleV2::Staged
    ));
}

#[test]
fn authority_digest_evidence_is_canonical_preserved_and_revision_ordered() {
    assert_eq!(
        RuntimeAuthorityPayloadDigestV2::parse("A".repeat(64)).unwrap_err(),
        RuntimeAuthorityPayloadDigestErrorV2::InvalidDigest
    );
    let (claimed_state, _handle, receipt) = claimed();
    let mut regressed = hydration_observation(&receipt);
    regressed.current_authority_revision = NonZeroU64::new(6).unwrap();
    let result = block_on_ready(
        claimed_state
            .begin_hydration()
            .unwrap()
            .execute(&HydrationPort {
                observation: Mutex::new(Some(regressed)),
                seal: None,
            }),
    );
    assert!(matches!(
        result,
        Err(RuntimeExactTargetHydrationErrorV2::Evidence(
            RuntimeExactTargetEvidenceErrorV2::CurrentAuthorityRevisionRegression
        ))
    ));

    let (claimed, _handle, receipt) = claimed();
    let hydrated = block_on_ready(claimed.begin_hydration().unwrap().execute(&HydrationPort {
        observation: Mutex::new(Some(hydration_observation(&receipt))),
        seal: None,
    }))
    .unwrap();
    assert_eq!(
        hydrated
            .evidence()
            .installation_authority_payload_digest()
            .as_str(),
        "d".repeat(64)
    );
    assert_eq!(
        hydrated
            .evidence()
            .current_authority_payload_digest()
            .as_str(),
        "e".repeat(64)
    );
    assert_ne!(
        hydrated.evidence().installation_authority_payload_digest(),
        hydrated.evidence().current_authority_payload_digest()
    );
}

#[test]
fn slot_authority_loss_during_hydration_fails_closed() {
    let (claimed, handle, receipt) = claimed();
    let result = block_on_ready(claimed.begin_hydration().unwrap().execute(&HydrationPort {
        observation: Mutex::new(Some(hydration_observation(&receipt))),
        seal: Some(handle),
    }));

    assert!(matches!(
        result,
        Err(RuntimeExactTargetHydrationErrorV2::SlotWork(
            RuntimeServingSlotWorkErrorV2::SupervisorSealed
        ))
    ));
}

#[test]
fn invalid_stage_witness_drops_the_opaque_guard() {
    let drops = Arc::new(AtomicUsize::new(0));
    let (stage_ready, _handle, receipt) = stage_ready();
    let stage_ready = refresh_stage_ready(stage_ready, &receipt, at(25), 42);
    let result = stage_ready.stage(
        &StagePort {
            lifecycle: RuntimeRouteLifecycleV2::Serving,
            drops: Arc::clone(&drops),
            expected_hydrated: 42,
            active_drops: None,
        },
        at(26),
    );

    assert!(matches!(
        result,
        Err(RuntimeRouteStageErrorV2::Evidence(
            RuntimeRouteStageEvidenceErrorV2::LifecycleMismatch
        ))
    ));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}

#[test]
fn stage_rejects_the_controller_renewal_window_before_registry_install() {
    let drops = Arc::new(AtomicUsize::new(0));
    let (stage_ready, _handle, receipt) = stage_ready();
    let stage_ready = refresh_stage_ready(stage_ready, &receipt, at(25), 42);
    let result = stage_ready.stage(
        &StagePort {
            lifecycle: RuntimeRouteLifecycleV2::Staged,
            drops: Arc::clone(&drops),
            expected_hydrated: 42,
            active_drops: None,
        },
        at(70),
    );

    assert!(matches!(
        result,
        Err(RuntimeRouteStageErrorV2::InvalidCompletionTime)
    ));
    assert_eq!(drops.load(Ordering::SeqCst), 0);
}

#[test]
fn stage_completion_time_overflow_fails_closed() {
    assert!(!super::staging::valid_stage_completion_time(
        at(25),
        at(100),
        Duration::MAX,
        at(26),
    ));
}

#[test]
fn requested_stage_refresh_uses_the_post_mutation_execution_receipt() {
    let (stage_ready, _handle, _preflight_ready_receipt) = stage_ready();
    let requested = receipt();
    let mut stale = hydration_observation(&requested);
    stale.observed_database_now = at(25);
    let result = block_on_ready(
        stage_ready
            .begin_exact_hydration_refresh()
            .unwrap()
            .execute(&HydrationPort {
                observation: Mutex::new(Some(stale)),
                seal: None,
            }),
    );

    assert!(matches!(
        result,
        Err(RuntimeExactTargetHydrationErrorV2::Evidence(
            RuntimeExactTargetEvidenceErrorV2::ExecutionMismatch
        ))
    ));
}

#[test]
fn desired_target_drift_during_preflight_prevents_staging() {
    let (stage_ready, _handle, receipt) = stage_ready();
    let mut drifted = hydration_observation(&receipt);
    drifted.observed_database_now = at(25);
    drifted.persisted_desired_target_digest =
        RuntimeDesiredTargetDigestV1::parse("f".repeat(64)).unwrap();
    let result = block_on_ready(
        stage_ready
            .begin_exact_hydration_refresh()
            .unwrap()
            .execute(&HydrationPort {
                observation: Mutex::new(Some(drifted)),
                seal: None,
            }),
    );

    assert!(matches!(
        result,
        Err(RuntimeExactTargetHydrationErrorV2::Evidence(
            RuntimeExactTargetEvidenceErrorV2::DesiredTargetDigestMismatch
        ))
    ));
}

#[test]
fn artifact_authority_and_binding_drift_during_preflight_prevent_staging() {
    let artifact = drifted_stage_refresh(|observation| {
        observation.artifact_target.binding_revision = BindingRevision::new(4).unwrap();
    });
    assert!(matches!(
        artifact,
        Err(RuntimeExactTargetHydrationErrorV2::Evidence(
            RuntimeExactTargetEvidenceErrorV2::ArtifactMismatch
        ))
    ));

    let authority = drifted_stage_refresh(|observation| {
        observation.current_authority_revision = NonZeroU64::new(6).unwrap();
    });
    assert!(matches!(
        authority,
        Err(RuntimeExactTargetHydrationErrorV2::Evidence(
            RuntimeExactTargetEvidenceErrorV2::CurrentAuthorityRevisionRegression
        ))
    ));

    let binding = drifted_stage_refresh(|observation| {
        observation.binding_pin.binding_revision = BindingRevision::new(4).unwrap();
    });
    assert!(matches!(
        binding,
        Err(RuntimeExactTargetHydrationErrorV2::Evidence(
            RuntimeExactTargetEvidenceErrorV2::BindingPinMismatch
        ))
    ));
}

#[test]
fn stage_refresh_rejects_a_database_observation_before_stage_readiness() {
    let (stage_ready, _handle, receipt) = stage_ready();
    let mut stale = hydration_observation(&receipt);
    stale.observed_database_now = at(23);
    let result = block_on_ready(
        stage_ready
            .begin_exact_hydration_refresh()
            .unwrap()
            .execute(&HydrationPort {
                observation: Mutex::new(Some(stale)),
                seal: None,
            }),
    );

    assert!(matches!(
        result,
        Err(RuntimeExactTargetHydrationErrorV2::Evidence(
            RuntimeExactTargetEvidenceErrorV2::DatabaseObservationBeforeStageReady
        ))
    ));
}

#[test]
fn slot_authority_loss_during_stage_refresh_fails_closed() {
    let (stage_ready, handle, receipt) = stage_ready();
    let mut observation = hydration_observation(&receipt);
    observation.observed_database_now = at(25);
    let result = block_on_ready(
        stage_ready
            .begin_exact_hydration_refresh()
            .unwrap()
            .execute(&HydrationPort {
                observation: Mutex::new(Some(observation)),
                seal: Some(handle),
            }),
    );

    assert!(matches!(
        result,
        Err(RuntimeExactTargetHydrationErrorV2::SlotWork(
            RuntimeServingSlotWorkErrorV2::SupervisorSealed
        ))
    ));
}

#[test]
fn panic_drops_the_staged_guard_before_the_slot_permit() {
    let drops = Arc::new(AtomicUsize::new(0));
    let active_drops = Arc::new(AtomicUsize::new(0));
    let (stage_ready, handle, receipt) = stage_ready();
    let stage_ready = refresh_stage_ready(stage_ready, &receipt, at(25), 42);
    let observed_handle = handle.clone();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let staged = stage_ready
            .stage(
                &StagePort {
                    lifecycle: RuntimeRouteLifecycleV2::Staged,
                    drops: Arc::clone(&drops),
                    expected_hydrated: 42,
                    active_drops: Some((observed_handle, Arc::clone(&active_drops))),
                },
                at(26),
            )
            .unwrap();
        assert_eq!(handle.active_count(), 1);
        std::hint::black_box(&staged);
        panic!("exercise implicit staged typestate drop");
    }));

    assert!(result.is_err());
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(active_drops.load(Ordering::SeqCst), 1);
    assert_eq!(handle.active_count(), 0);
}

fn previous_process(process_instance_id: &str) -> RuntimeProcessIdentityV1 {
    RuntimeProcessIdentityV1 {
        target: target(),
        runtime_generation: RuntimeGeneration::FIRST,
        process_instance_id: ProcessInstanceId::parse(process_instance_id).unwrap(),
    }
}

fn drain_requested_receipt(
    receipt: &RuntimeExecutionReceiptV1,
    requested_at: DateTime<Utc>,
) -> RuntimeExecutionReceiptV1 {
    let mut deployment = RuntimeDeployment::restore(receipt.snapshot.clone()).unwrap();
    deployment
        .request_drain(&CommandGuardV1 {
            expected_revision: deployment.revision(),
            controller_id: receipt.controller_id.clone(),
            fencing_token: receipt.fencing_token,
            runtime_generation: deployment.runtime_generation(),
            now: requested_at,
        })
        .unwrap();
    RuntimeExecutionReceiptV1 {
        snapshot: deployment.snapshot(),
        ..receipt.clone()
    }
}

fn recovery_phase_receipts() -> Vec<(RuntimeStagedRecoveryPhaseV2, RuntimeExecutionReceiptV1)> {
    let requested = receipt();
    let preflight_ready = preflight_ready_receipt(&requested, at(23));
    let drain_requested = drain_requested_receipt(&preflight_ready, at(27));
    let mut deployment = RuntimeDeployment::restore(drain_requested.snapshot.clone()).unwrap();
    deployment
        .accept_drain(
            &CommandGuardV1 {
                expected_revision: deployment.revision(),
                controller_id: drain_requested.controller_id.clone(),
                fencing_token: drain_requested.fencing_token,
                runtime_generation: drain_requested.snapshot.runtime_generation,
                now: at(28),
            },
            DrainAttestationV1 {
                previous_runtime: drain_requested.snapshot.previous_runtime.clone(),
                target_runtime_generation: drain_requested.snapshot.runtime_generation,
                drained_at: at(28),
            },
        )
        .unwrap();
    let drained = RuntimeExecutionReceiptV1 {
        snapshot: deployment.snapshot(),
        ..drain_requested.clone()
    };
    deployment
        .begin_activation(&CommandGuardV1 {
            expected_revision: deployment.revision(),
            controller_id: drained.controller_id.clone(),
            fencing_token: drained.fencing_token,
            runtime_generation: drained.snapshot.runtime_generation,
            now: at(29),
        })
        .unwrap();
    let activation_applying = RuntimeExecutionReceiptV1 {
        snapshot: deployment.snapshot(),
        ..drained.clone()
    };
    deployment
        .accept_activation(
            &CommandGuardV1 {
                expected_revision: deployment.revision(),
                controller_id: activation_applying.controller_id.clone(),
                fencing_token: activation_applying.fencing_token,
                runtime_generation: activation_applying.snapshot.runtime_generation,
                now: at(30),
            },
            ActivationAttestationV1 {
                activation_request_id: activation_applying
                    .snapshot
                    .identity
                    .activation_request_id
                    .clone(),
                target: activation_applying.snapshot.target.clone(),
                runtime_generation: activation_applying.snapshot.runtime_generation,
                kind: ActivationOutcomeKindV1::CrashRecovered,
                activated_at: at(30),
            },
        )
        .unwrap();
    let runtime_pending = RuntimeExecutionReceiptV1 {
        snapshot: deployment.snapshot(),
        ..activation_applying.clone()
    };
    deployment
        .begin_panel_reconciliation(&CommandGuardV1 {
            expected_revision: deployment.revision(),
            controller_id: runtime_pending.controller_id.clone(),
            fencing_token: runtime_pending.fencing_token,
            runtime_generation: runtime_pending.snapshot.runtime_generation,
            now: at(31),
        })
        .unwrap();
    let reconciling = RuntimeExecutionReceiptV1 {
        snapshot: deployment.snapshot(),
        ..runtime_pending.clone()
    };
    deployment
        .accept_panel_certificate(
            &CommandGuardV1 {
                expected_revision: deployment.revision(),
                controller_id: reconciling.controller_id.clone(),
                fencing_token: reconciling.fencing_token,
                runtime_generation: reconciling.snapshot.runtime_generation,
                now: at(32),
            },
            PanelCertificateV1 {
                certificate_id: PanelCertificateId::parse("panel:recovery").unwrap(),
                report_digest: PanelReportDigestV1::parse("c".repeat(64)).unwrap(),
                target: reconciling.snapshot.target.clone(),
                runtime_generation: reconciling.snapshot.runtime_generation,
                process_instance_id: ProcessInstanceId::parse("runtime:1").unwrap(),
                declared_count: 0,
                installed_count: 0,
                unchanged_count: 0,
                skipped_transient_count: 0,
                skipped_unresolved_channel_count: 0,
                failed_count: 0,
                ambiguous_outcome_count: 0,
                stale_message_cleanup_pending_count: 0,
                orphan_message_cleanup_pending_count: 0,
                reposted_old_message_cleanup_pending_count: 0,
                reconciled_at: at(32),
            },
        )
        .unwrap();
    let awaiting = RuntimeExecutionReceiptV1 {
        snapshot: deployment.snapshot(),
        ..reconciling.clone()
    };
    vec![
        (RuntimeStagedRecoveryPhaseV2::Drained, drained),
        (
            RuntimeStagedRecoveryPhaseV2::ActivationApplying,
            activation_applying,
        ),
        (
            RuntimeStagedRecoveryPhaseV2::RuntimePendingReady,
            runtime_pending,
        ),
        (RuntimeStagedRecoveryPhaseV2::ReconcilingPanels, reconciling),
        (RuntimeStagedRecoveryPhaseV2::AwaitingGatewayReady, awaiting),
    ]
}

fn staged_replacement(
    previous_runtime: RuntimeProcessIdentityV1,
    active_drops: Option<Arc<AtomicUsize>>,
) -> (
    RuntimeStagedConvergenceV2<u64, StagedGuard>,
    RuntimeServingSlotWorkTestHandleV2,
    RuntimeExecutionReceiptV1,
    Arc<AtomicUsize>,
) {
    let requested = receipt_with_previous(previous_runtime);
    let (claimed, handle, requested) = claimed_from_receipt(requested, at(20));
    let hydrated = block_on_ready(claimed.begin_hydration().unwrap().execute(&HydrationPort {
        observation: Mutex::new(Some(hydration_observation(&requested))),
        seal: None,
    }))
    .unwrap();
    let preflight = block_on_ready(hydrated.begin_discord_preflight(at(22)).unwrap().execute(
        &PreflightPort {
            observation: Mutex::new(Some(RuntimeDiscordPreflightObservationV2 {
                target: requested.snapshot.target.clone(),
                runtime_generation: requested.snapshot.runtime_generation,
                binding_pin: binding_pin(&requested),
                checked_at: at(23),
            })),
        },
    ))
    .unwrap();
    let RuntimeDiscordPreflightOutcomeV2::AcceptPreflight(preflighted) = preflight else {
        panic!("requested replacement must persist preflight")
    };
    let stage_ready = block_on_ready(preflighted.begin_accept_preflight().unwrap().execute(
        &MutationPort {
            requested_snapshot: requested.snapshot.clone(),
        },
        at(24),
    ))
    .unwrap();
    let preflight_ready = preflight_ready_receipt(&requested, at(23));
    let stage_ready = refresh_stage_ready(stage_ready, &preflight_ready, at(25), 42);
    let drops = Arc::new(AtomicUsize::new(0));
    let staged = stage_ready
        .stage(
            &StagePort {
                lifecycle: RuntimeRouteLifecycleV2::Staged,
                drops: Arc::clone(&drops),
                expected_hydrated: 42,
                active_drops: active_drops.map(|active_drops| (handle.clone(), active_drops)),
            },
            at(26),
        )
        .unwrap();
    (staged, handle, preflight_ready, drops)
}

struct ReplacementMutationPort {
    deployment: Mutex<RuntimeDeployment>,
    guards: Arc<Mutex<Vec<RuntimeExecutionGuardV1>>>,
}

impl ReplacementMutationPort {
    fn new(receipt: &RuntimeExecutionReceiptV1) -> Self {
        Self {
            deployment: Mutex::new(RuntimeDeployment::restore(receipt.snapshot.clone()).unwrap()),
            guards: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl RuntimeConvergenceMutationPortV2 for ReplacementMutationPort {
    type Error = ();

    fn mutate<'a>(
        &'a self,
        request: &'a RuntimeMutationRequestV1,
    ) -> RuntimeConvergenceFutureV2<'a, Result<RuntimeMutationReceiptV1, Self::Error>> {
        self.guards.lock().unwrap().push(request.guard.clone());
        let mut deployment = self.deployment.lock().unwrap();
        let now = match &request.mutation {
            RuntimeConvergenceMutationV1::RequestDrain => at(27),
            RuntimeConvergenceMutationV1::AcceptDrain(attestation) => attestation.drained_at,
            _ => panic!("unexpected replacement mutation"),
        };
        let guard = CommandGuardV1 {
            expected_revision: request.guard.expected_revision,
            controller_id: request.guard.controller_id.clone(),
            fencing_token: request.guard.fencing_token,
            runtime_generation: request.guard.runtime_generation,
            now,
        };
        let outcome = match &request.mutation {
            RuntimeConvergenceMutationV1::RequestDrain => deployment.request_drain(&guard).unwrap(),
            RuntimeConvergenceMutationV1::AcceptDrain(attestation) => deployment
                .accept_drain(&guard, attestation.clone())
                .unwrap(),
            _ => unreachable!(),
        };
        Box::pin(ready(Ok(RuntimeMutationReceiptV1 {
            action_id: request.action_id,
            outcome,
            snapshot: deployment.snapshot(),
            convergence_attempt: request.guard.convergence_attempt,
        })))
    }
}

struct FailedReplacementMutationPort;

impl RuntimeConvergenceMutationPortV2 for FailedReplacementMutationPort {
    type Error = &'static str;

    fn mutate<'a>(
        &'a self,
        _request: &'a RuntimeMutationRequestV1,
    ) -> RuntimeConvergenceFutureV2<'a, Result<RuntimeMutationReceiptV1, Self::Error>> {
        Box::pin(ready(Err("retry")))
    }
}

fn previous_lease(
    receipt: &RuntimeExecutionReceiptV1,
    previous_runtime: RuntimeProcessIdentityV1,
    last_heartbeat_at: DateTime<Utc>,
) -> RuntimePreviousServingLeaseEvidenceV1 {
    RuntimePreviousServingLeaseEvidenceV1 {
        identity: RuntimePreviousServingLeaseIdentityV1 {
            scope: RuntimeDeploymentScopeV1 {
                tenant_id: receipt.snapshot.identity.tenant_id.clone(),
                installation_id: receipt.snapshot.identity.installation_id.clone(),
                deployment_id: DeploymentId::parse("deployment:previous").unwrap(),
            },
            attestation_id: RuntimeAttestationIdV1::parse("6".repeat(64)).unwrap(),
            process: previous_runtime,
            lease_epoch: NonZeroU64::new(1).unwrap(),
            revision: NonZeroU64::new(1).unwrap(),
        },
        acquired_at: at(0),
        last_heartbeat_at,
    }
}

struct PreviousServingPort {
    observations: Mutex<VecDeque<(DateTime<Utc>, RuntimePreviousServingStateV1)>>,
    guards: Arc<Mutex<Vec<RuntimeExecutionGuardV1>>>,
}

impl PreviousServingPort {
    fn new(observations: Vec<(DateTime<Utc>, RuntimePreviousServingStateV1)>) -> Self {
        Self {
            observations: Mutex::new(observations.into()),
            guards: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl RuntimeExactPreviousServingObservationPortV2 for PreviousServingPort {
    type Error = ();

    fn observe_previous_serving<'a>(
        &'a self,
        request: &'a automation_runtime_controller::RuntimeObservePreviousServingV1,
    ) -> RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimePreviousServingObservationReceiptV1, Self::Error>,
    > {
        self.guards.lock().unwrap().push(request.guard.clone());
        let (observed_at, state) = self
            .observations
            .lock()
            .unwrap()
            .pop_front()
            .expect("one exact previous-serving observation");
        Box::pin(ready(Ok(RuntimePreviousServingObservationReceiptV1 {
            action_id: request.action_id,
            guard: request.guard.clone(),
            observed_at,
            expected_target: request.expected_target.clone(),
            expected_previous_runtime: request.expected_previous_runtime.clone(),
            state,
        })))
    }
}

#[derive(Debug)]
struct PauseToken;

#[derive(Debug)]
struct ResumedToken;

struct BarrierPausePort {
    drift_correlation: bool,
}

impl RuntimeBarrierAPausePortV2 for BarrierPausePort {
    type Error = ();
    type Paused = PauseToken;

    fn pause_barrier_a<'a>(
        &'a self,
        request: &'a RuntimeBarrierAPauseRequestV2,
    ) -> RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimeBarrierAPauseObservationV2<Self::Paused>, Self::Error>,
    > {
        let mut correlation = request.correlation().clone();
        if self.drift_correlation {
            correlation.execution_guard.fencing_token = FencingToken::new(9).unwrap();
        }
        Box::pin(ready(Ok(RuntimeBarrierAPauseObservationV2 {
            correlation,
            coordinator_generation: NonZeroU64::new(5).unwrap(),
            connection_epoch: NonZeroU64::new(7).unwrap(),
            admission_revision: NonZeroU64::new(11).unwrap(),
            connected_event_sequence: NonZeroU64::new(8).unwrap(),
            pause_sequence: NonZeroU64::new(9).unwrap(),
            paused_at: at(29),
            paused: PauseToken,
        })))
    }
}

fn advance_route_witness(
    source: &RuntimeRouteWitnessV2,
    admission_generation: u64,
    registry_observation_sequence: u64,
) -> RuntimeRouteWitnessV2 {
    let mut witness = source.clone();
    witness.admission_generation = NonZeroU64::new(admission_generation).unwrap();
    witness.registry_observation_sequence = NonZeroU64::new(registry_observation_sequence).unwrap();
    witness
}

struct PredecessorTransitionPort {
    initial_active_interactions: u32,
    drift_predecessor_identity: bool,
    drift_successor_fence: bool,
}

impl RuntimeRoutePredecessorTransitionPortV2<u64, StagedGuard, PauseToken>
    for PredecessorTransitionPort
{
    type Error = ();

    fn transition_predecessor_to_draining(
        &self,
        request: &RuntimeRoutePredecessorTransitionRequestV2,
        hydrated: &u64,
        _staged: &StagedGuard,
        _paused: &PauseToken,
    ) -> Result<RuntimeRoutePredecessorTransitionObservationV2, Self::Error> {
        assert_eq!(*hydrated, 42);
        let local_previous = request.expected_previous_runtime().is_some_and(|previous| {
            previous.process_instance_id
                == request.correlation().successor_identity.process_instance_id
        });
        let predecessor = if local_previous {
            let mut identity = request.expected_previous_runtime().unwrap().clone();
            if self.drift_predecessor_identity {
                identity.process_instance_id = ProcessInstanceId::parse("runtime:drift").unwrap();
            }
            Some(RuntimeRouteWitnessV2 {
                identity,
                controller_fencing_token: FencingToken::new(1).unwrap(),
                route_incarnation: NonZeroU64::new(1).unwrap(),
                lifecycle: RuntimeRouteLifecycleV2::Draining,
                active_interactions: self.initial_active_interactions,
                admission_generation: NonZeroU64::new(4).unwrap(),
                registry_observation_sequence: NonZeroU64::new(2).unwrap(),
            })
        } else {
            None
        };
        let mut successor = advance_route_witness(request.initial_successor(), 4, 2);
        if self.drift_successor_fence {
            successor.controller_fencing_token = FencingToken::new(9).unwrap();
        }
        Ok(RuntimeRoutePredecessorTransitionObservationV2 {
            correlation: request.correlation().clone(),
            predecessor,
            successor,
            transitioned_at: at(30),
        })
    }
}

struct BarrierResumePort {
    admission: RuntimeAdmissionDispositionV2,
}

impl RuntimeBarrierAResumePortV2<PauseToken> for BarrierResumePort {
    type Error = ();
    type Resumed = ResumedToken;

    fn resume_barrier_a_closed<'a>(
        &'a self,
        request: &'a RuntimeBarrierAResumeRequestV2,
        _paused: &'a PauseToken,
    ) -> RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimeBarrierAResumeObservationV2<Self::Resumed>, Self::Error>,
    > {
        Box::pin(ready(Ok(RuntimeBarrierAResumeObservationV2 {
            correlation: request.correlation().clone(),
            coordinator_generation: request.coordinator_generation(),
            connection_epoch: request.connection_epoch(),
            admission_revision: request.pause_admission_revision(),
            connected_event_sequence: request.connected_event_sequence(),
            pause_sequence: request.pause_sequence(),
            resume_sequence: NonZeroU64::new(request.pause_sequence().get() + 1).unwrap(),
            admission: self.admission,
            resumed_at: at(31),
            resumed: ResumedToken,
        })))
    }
}

struct PreviousServingDisconnectPort;

impl RuntimePreviousServingDisconnectPortV2 for PreviousServingDisconnectPort {
    type Error = ();

    fn disconnect_previous_serving<'a>(
        &'a self,
        request: &'a RuntimeDisconnectServingV1,
    ) -> RuntimeConvergenceFutureV2<'a, Result<RuntimeServingUpdateReceiptV1, Self::Error>> {
        let mut identity = request.identity.clone();
        identity.expected_revision = NonZeroU64::new(identity.expected_revision.get() + 1).unwrap();
        Box::pin(ready(Ok(RuntimeServingUpdateReceiptV1 {
            action_id: request.action_id,
            serving: RuntimeServingReceiptV1 {
                runtime_generation: identity.runtime_generation,
                identity,
                acquired_at: at(0),
                last_heartbeat_at: at(32),
                expires_at: at(32),
                connected: false,
                serving: false,
            },
        })))
    }
}

struct PredecessorRetirementPort {
    final_observation: Mutex<Option<(DateTime<Utc>, RuntimePreviousServingStateV1)>>,
    removed_active_interactions: u32,
    drift_removed_identity: bool,
    initial_active_interactions: Arc<AtomicUsize>,
}

impl RuntimePredecessorRetirementPortV2<u64, StagedGuard, ResumedToken>
    for PredecessorRetirementPort
{
    type Error = ();

    fn retire_predecessor<'a>(
        &'a self,
        request: &'a RuntimePredecessorRetirementRequestV2,
        hydrated: &'a u64,
        _staged: &'a StagedGuard,
        _resumed: &'a ResumedToken,
    ) -> RuntimeConvergenceFutureV2<
        'a,
        Result<RuntimePredecessorRetirementObservationV2, Self::Error>,
    > {
        assert_eq!(*hydrated, 42);
        if let Some(predecessor) = request.predecessor() {
            self.initial_active_interactions
                .store(predecessor.active_interactions as usize, Ordering::SeqCst);
        }
        let (observed_at, state) = self
            .final_observation
            .lock()
            .unwrap()
            .take()
            .expect("one retirement observation");
        let previous_request = request.previous_serving_request();
        let previous_serving = RuntimePreviousServingObservationReceiptV1 {
            action_id: previous_request.action_id,
            guard: previous_request.guard.clone(),
            observed_at,
            expected_target: previous_request.expected_target.clone(),
            expected_previous_runtime: previous_request.expected_previous_runtime.clone(),
            state,
        };
        let removed_predecessor = request.predecessor().map(|predecessor| {
            let mut removed = advance_route_witness(predecessor, 5, 3);
            removed.active_interactions = self.removed_active_interactions;
            if self.drift_removed_identity {
                removed.identity.process_instance_id =
                    ProcessInstanceId::parse("runtime:removed-drift").unwrap();
            }
            removed
        });
        let successor = advance_route_witness(request.successor(), 5, 3);
        Box::pin(ready(Ok(RuntimePredecessorRetirementObservationV2 {
            previous_serving,
            route: RuntimeRoutePredecessorRemovalObservationV2 {
                removed_predecessor,
                successor,
                observed_at: at(33),
            },
        })))
    }
}

fn request_drain_and_observe(
    staged: RuntimeStagedConvergenceV2<u64, StagedGuard>,
    mutations: &ReplacementMutationPort,
    previous: &PreviousServingPort,
) -> RuntimeObservedPreviousServingConvergenceV2<u64, StagedGuard> {
    let requested = block_on_ready(
        staged
            .begin_request_drain()
            .unwrap()
            .execute(mutations, at(27)),
    )
    .unwrap();
    block_on_ready(
        requested
            .begin_previous_serving_observation()
            .unwrap()
            .execute(previous),
    )
    .unwrap()
}

fn pause_and_transition(
    observed: RuntimeObservedPreviousServingConvergenceV2<u64, StagedGuard>,
    transition: &PredecessorTransitionPort,
) -> RuntimeRoutePredecessorDrainingConvergenceV2<u64, StagedGuard, PauseToken> {
    let paused = block_on_ready(observed.begin_barrier_a_pause(at(29)).unwrap().execute(
        &BarrierPausePort {
            drift_correlation: false,
        },
    ))
    .unwrap();
    paused
        .begin_predecessor_transition()
        .execute(transition)
        .unwrap()
}

fn resume_barrier(
    draining: RuntimeRoutePredecessorDrainingConvergenceV2<u64, StagedGuard, PauseToken>,
) -> RuntimeBarrierAResumedConvergenceV2<u64, StagedGuard, ResumedToken> {
    block_on_ready(
        draining
            .begin_barrier_a_resume()
            .execute(&BarrierResumePort {
                admission: RuntimeAdmissionDispositionV2::Closed,
            }),
    )
    .unwrap()
}

fn local_observed_replacement() -> (
    RuntimeObservedPreviousServingConvergenceV2<u64, StagedGuard>,
    ReplacementMutationPort,
    RuntimeExecutionReceiptV1,
    RuntimePreviousServingLeaseEvidenceV1,
    RuntimeServingSlotWorkTestHandleV2,
    Arc<AtomicUsize>,
) {
    let previous_runtime = previous_process("runtime:1");
    let (staged, handle, receipt, drops) = staged_replacement(previous_runtime.clone(), None);
    let mutations = ReplacementMutationPort::new(&receipt);
    let lease = previous_lease(&receipt, previous_runtime, at(27));
    let previous = PreviousServingPort::new(vec![(
        at(28),
        RuntimePreviousServingStateV1::Serving {
            lease: lease.clone(),
            expires_at: at(50),
        },
    )]);
    let observed = request_drain_and_observe(staged, &mutations, &previous);
    (observed, mutations, receipt, lease, handle, drops)
}

fn disconnect_resumed(
    resumed: RuntimeBarrierAResumedConvergenceV2<u64, StagedGuard, ResumedToken>,
    mut lease: RuntimePreviousServingLeaseEvidenceV1,
) -> (
    RuntimePredecessorRetirementReadyConvergenceV2<u64, StagedGuard, ResumedToken>,
    RuntimePreviousServingStateV1,
) {
    let disconnect = match resumed.begin_previous_serving_disconnect().unwrap() {
        RuntimePreviousServingDisconnectOutcomeV2::Required(disconnect) => disconnect,
        RuntimePreviousServingDisconnectOutcomeV2::NotRequired(_) => {
            panic!("fresh local previous serving must disconnect")
        }
    };
    let ready = block_on_ready(disconnect.execute(&PreviousServingDisconnectPort)).unwrap();
    let receipt = ready.previous_serving_disconnect().unwrap();
    lease.identity.revision = receipt.identity.expected_revision;
    lease.last_heartbeat_at = receipt.last_heartbeat_at;
    (
        ready,
        RuntimePreviousServingStateV1::Disconnected {
            lease,
            disconnected_at: at(32),
        },
    )
}

#[test]
fn exact_replacement_reuses_one_claim_and_reaches_drained() {
    let previous_runtime = previous_process("runtime:1");
    let active_drops = Arc::new(AtomicUsize::new(0));
    let (staged, handle, receipt, drops) =
        staged_replacement(previous_runtime.clone(), Some(Arc::clone(&active_drops)));
    let mutations = ReplacementMutationPort::new(&receipt);
    let lease = previous_lease(&receipt, previous_runtime.clone(), at(27));
    let previous = PreviousServingPort::new(vec![(
        at(28),
        RuntimePreviousServingStateV1::Serving {
            lease: lease.clone(),
            expires_at: at(50),
        },
    )]);
    let observed = request_drain_and_observe(staged, &mutations, &previous);
    let draining = pause_and_transition(
        observed,
        &PredecessorTransitionPort {
            initial_active_interactions: 2,
            drift_predecessor_identity: false,
            drift_successor_fence: false,
        },
    );
    let resumed = resume_barrier(draining);
    let disconnect = match resumed.begin_previous_serving_disconnect().unwrap() {
        RuntimePreviousServingDisconnectOutcomeV2::Required(disconnect) => disconnect,
        RuntimePreviousServingDisconnectOutcomeV2::NotRequired(_) => {
            panic!("fresh exact previous serving lease must disconnect")
        }
    };
    assert_eq!(
        disconnect.request().identity.process_instance_id,
        previous_runtime.process_instance_id
    );
    let ready = block_on_ready(disconnect.execute(&PreviousServingDisconnectPort)).unwrap();
    let disconnect_receipt = ready.previous_serving_disconnect().unwrap().clone();
    let mut disconnected_lease = lease;
    disconnected_lease.identity.revision = disconnect_receipt.identity.expected_revision;
    disconnected_lease.last_heartbeat_at = disconnect_receipt.last_heartbeat_at;
    let seen_active = Arc::new(AtomicUsize::new(0));
    let removed = block_on_ready(ready.begin_predecessor_retirement().unwrap().execute(
        &PredecessorRetirementPort {
            final_observation: Mutex::new(Some((
                at(32),
                RuntimePreviousServingStateV1::Disconnected {
                    lease: disconnected_lease,
                    disconnected_at: at(32),
                },
            ))),
            removed_active_interactions: 0,
            drift_removed_identity: false,
            initial_active_interactions: Arc::clone(&seen_active),
        },
    ))
    .unwrap();
    assert_eq!(seen_active.load(Ordering::SeqCst), 2);
    let drained = block_on_ready(
        removed
            .begin_accept_drain()
            .unwrap()
            .execute(&mutations, at(34)),
    )
    .unwrap();
    assert_eq!(handle.active_count(), 1);
    assert_eq!(
        drained.previous_serving_disconnect(),
        Some(&disconnect_receipt)
    );
    assert_eq!(
        drained
            .barrier_evidence()
            .pause()
            .connected_event_sequence(),
        NonZeroU64::new(8).unwrap()
    );
    assert_eq!(
        drained.barrier_evidence().pause().pause_sequence(),
        NonZeroU64::new(9).unwrap()
    );
    assert_eq!(
        drained.barrier_evidence().resume().resume_sequence(),
        NonZeroU64::new(10).unwrap()
    );
    let mutation_guards = mutations.guards.lock().unwrap();
    assert_eq!(mutation_guards.len(), 2);
    assert!(mutation_guards.iter().all(|guard| {
        guard.controller_id == mutation_guards[0].controller_id
            && guard.fencing_token == mutation_guards[0].fencing_token
            && guard.convergence_attempt == mutation_guards[0].convergence_attempt
    }));
    drop(mutation_guards);
    let observation_guards = previous.guards.lock().unwrap();
    assert_eq!(observation_guards.len(), 1);
    assert_eq!(
        observation_guards[0].fencing_token,
        drained.previous_serving().guard.fencing_token
    );
    drop(observation_guards);
    let handoff = drained.into_handoff();
    assert!(matches!(
        handoff.1.snapshot().phase,
        RuntimeDeploymentPhaseV1::Drained
    ));
    assert_eq!(handoff.1.fencing_token(), receipt.fencing_token);
    drop(handoff);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(active_drops.load(Ordering::SeqCst), 1);
    assert_eq!(handle.active_count(), 0);
}

#[test]
fn durable_drain_requested_recovery_stages_and_resumes_without_request_drain() {
    let previous_runtime = previous_process("runtime:1");
    let requested = receipt_with_previous(previous_runtime);
    let preflight_ready = preflight_ready_receipt(&requested, at(23));
    let drain_requested = drain_requested_receipt(&preflight_ready, at(27));
    let (claimed, handle, drain_requested) = claimed_from_receipt(drain_requested, at(28));
    assert_eq!(
        claimed.claim_kind(),
        RuntimeConvergenceClaimKindV2::DrainRequested
    );
    let mut initial = hydration_observation(&drain_requested);
    initial.observed_database_now = at(29);
    let hydrated = block_on_ready(claimed.begin_hydration().unwrap().execute(&HydrationPort {
        observation: Mutex::new(Some(initial)),
        seal: None,
    }))
    .unwrap();
    let preflight = block_on_ready(hydrated.begin_discord_preflight(at(30)).unwrap().execute(
        &PreflightPort {
            observation: Mutex::new(Some(RuntimeDiscordPreflightObservationV2 {
                target: drain_requested.snapshot.target.clone(),
                runtime_generation: drain_requested.snapshot.runtime_generation,
                binding_pin: binding_pin(&drain_requested),
                checked_at: at(31),
            })),
        },
    ))
    .unwrap();
    let RuntimeDiscordPreflightOutcomeV2::StageReady(stage_ready) = preflight else {
        panic!("durable DrainRequested must not issue another mutation")
    };
    let stage_ready = refresh_stage_ready(*stage_ready, &drain_requested, at(32), 42);
    let drops = Arc::new(AtomicUsize::new(0));
    let staged = stage_ready
        .stage(
            &StagePort {
                lifecycle: RuntimeRouteLifecycleV2::Staged,
                drops: Arc::clone(&drops),
                expected_hydrated: 42,
                active_drops: None,
            },
            at(33),
        )
        .unwrap();
    let resumed = staged.resume_drain_requested(at(34)).unwrap();
    assert_eq!(resumed.requested_at(), at(34));
    assert_eq!(
        resumed.witness().identity.runtime_generation,
        drain_requested.snapshot.runtime_generation
    );
    assert_eq!(handle.active_count(), 1);
    drop(resumed);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(handle.active_count(), 0);
}

#[test]
fn every_supported_durable_phase_rechecks_and_returns_exact_staged_recovery() {
    for (expected_phase, execution) in recovery_phase_receipts() {
        let (claimed, handle, execution) = claimed_from_receipt(execution, at(40));
        assert_eq!(
            claimed.claim_kind(),
            RuntimeConvergenceClaimKindV2::StagedRecovery(expected_phase)
        );
        let mut initial = hydration_observation(&execution);
        initial.observed_database_now = at(41);
        let hydrated = block_on_ready(claimed.begin_hydration().unwrap().execute(&HydrationPort {
            observation: Mutex::new(Some(initial)),
            seal: None,
        }))
        .unwrap();
        let preflight = block_on_ready(hydrated.begin_discord_preflight(at(42)).unwrap().execute(
            &PreflightPort {
                observation: Mutex::new(Some(RuntimeDiscordPreflightObservationV2 {
                    target: execution.snapshot.target.clone(),
                    runtime_generation: execution.snapshot.runtime_generation,
                    binding_pin: binding_pin(&execution),
                    checked_at: at(43),
                })),
            },
        ))
        .unwrap();
        let RuntimeDiscordPreflightOutcomeV2::StageReady(stage_ready) = preflight else {
            panic!("durable recovery must not replay preflight")
        };
        let stage_ready = refresh_stage_ready(*stage_ready, &execution, at(44), 42);
        let drops = Arc::new(AtomicUsize::new(0));
        let staged = stage_ready
            .stage(
                &StagePort {
                    lifecycle: RuntimeRouteLifecycleV2::Staged,
                    drops: Arc::clone(&drops),
                    expected_hydrated: 42,
                    active_drops: None,
                },
                at(45),
            )
            .unwrap();
        let recovery = staged.into_staged_recovery().unwrap();
        assert_eq!(recovery.phase(), expected_phase);
        assert_eq!(
            recovery.route().session().snapshot().phase,
            execution.snapshot.phase
        );
        assert_eq!(handle.active_count(), 1);
        let handoff = recovery.into_route().into_handoff();
        assert_eq!(handoff.1.snapshot().phase, execution.snapshot.phase);
        drop(handoff);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(handle.active_count(), 0);
    }
}

#[test]
fn retryable_blocked_live_and_terminal_claims_cannot_enter_staged_recovery() {
    let failure = RuntimeFailureV1 {
        failure_id: RuntimeFailureId::parse("failure:recovery").unwrap(),
        kind: RuntimeFailureKindV1::InvariantViolation,
        code: "recovery_blocked".to_owned(),
        message: "blocked".to_owned(),
        recorded_at: at(30),
    };
    let phases = [
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Retryable {
                failure: failure.clone(),
                attempt: NonZeroU32::MIN,
                retry_not_before: at(50),
            },
        },
        RuntimeDeploymentPhaseV1::RuntimePending {
            condition: RuntimePendingConditionV1::Blocked {
                failure: failure.clone(),
            },
        },
        RuntimeDeploymentPhaseV1::Live,
        RuntimeDeploymentPhaseV1::Cancelled {
            reason: "terminal".to_owned(),
            cancelled_at: at(30),
        },
    ];
    for phase in phases {
        let mut execution = receipt();
        execution.snapshot.phase = phase;
        let slot = RuntimeServingSlotV2::from_target(&execution.snapshot.target);
        let (permit, _handle) = runtime_serving_slot_work_test_authority_v2(
            slot,
            ProcessInstanceId::parse("runtime:1").unwrap(),
        );
        let result = RuntimeClaimedConvergenceV2::from_claim(
            execution,
            permit,
            at(40),
            RuntimeControllerConfigV1::default(),
        );
        assert!(matches!(
            result,
            Err(RuntimeConvergenceStartErrorV2::SupportedPhaseRequired)
        ));
    }
}

#[test]
fn request_drain_port_failure_retains_the_staged_route_for_one_claim_retry() {
    let previous_runtime = previous_process("runtime:1");
    let (staged, handle, receipt, drops) = staged_replacement(previous_runtime, None);
    let failure = block_on_ready(
        staged
            .begin_request_drain()
            .unwrap()
            .execute(&FailedReplacementMutationPort, at(27)),
    )
    .unwrap_err();
    let (staged, source) = failure.into_retained().unwrap();
    assert_eq!(source, "retry");
    assert_eq!(handle.active_count(), 1);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    let mutations = ReplacementMutationPort::new(&receipt);
    let requested = block_on_ready(
        staged
            .begin_request_drain()
            .unwrap()
            .execute(&mutations, at(27)),
    )
    .unwrap();
    assert_eq!(requested.requested_at(), at(27));
    assert_eq!(mutations.guards.lock().unwrap().len(), 1);
    drop(requested);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(handle.active_count(), 0);
}

#[test]
fn fresh_foreign_previous_serving_is_rejected_before_barrier_a() {
    let previous_runtime = previous_process("runtime:foreign");
    let (staged, handle, receipt, drops) = staged_replacement(previous_runtime.clone(), None);
    let mutations = ReplacementMutationPort::new(&receipt);
    let requested = block_on_ready(
        staged
            .begin_request_drain()
            .unwrap()
            .execute(&mutations, at(27)),
    )
    .unwrap();
    let lease = previous_lease(&receipt, previous_runtime, at(27));
    let previous = PreviousServingPort::new(vec![(
        at(28),
        RuntimePreviousServingStateV1::Serving {
            lease,
            expires_at: at(50),
        },
    )]);
    let result = block_on_ready(
        requested
            .begin_previous_serving_observation()
            .unwrap()
            .execute(&previous),
    );
    assert!(matches!(
        result,
        Err(RuntimeReplacementExecutionErrorV2::Failed(
            RuntimeExactPreviousServingErrorV2::Evidence(
                RuntimeExactPreviousServingEvidenceErrorV2::FreshForeignPredecessor
            )
        ))
    ));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(handle.active_count(), 0);
}

#[test]
fn barrier_pause_correlation_drift_fails_closed() {
    let (observed, _mutations, _receipt, _lease, handle, drops) = local_observed_replacement();
    let result = block_on_ready(observed.begin_barrier_a_pause(at(29)).unwrap().execute(
        &BarrierPausePort {
            drift_correlation: true,
        },
    ));
    assert!(matches!(
        result,
        Err(RuntimeReplacementExecutionErrorV2::Failed(
            RuntimeBarrierAPauseErrorV2::Evidence(
                RuntimeBarrierAPauseEvidenceErrorV2::CorrelationMismatch
            )
        ))
    ));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(handle.active_count(), 0);
}

#[test]
fn predecessor_identity_and_successor_fence_drift_fail_closed() {
    for (drift_predecessor_identity, drift_successor_fence, expected) in [
        (
            true,
            false,
            RuntimeRoutePredecessorTransitionEvidenceErrorV2::PredecessorIdentityMismatch,
        ),
        (
            false,
            true,
            RuntimeRoutePredecessorTransitionEvidenceErrorV2::SuccessorMismatch,
        ),
    ] {
        let (observed, _mutations, _receipt, _lease, handle, drops) = local_observed_replacement();
        let paused = block_on_ready(observed.begin_barrier_a_pause(at(29)).unwrap().execute(
            &BarrierPausePort {
                drift_correlation: false,
            },
        ))
        .unwrap();
        let result = paused
            .begin_predecessor_transition()
            .execute(&PredecessorTransitionPort {
                initial_active_interactions: 2,
                drift_predecessor_identity,
                drift_successor_fence,
            });
        match result {
            Err(RuntimeReplacementExecutionErrorV2::Failed(
                RuntimeRoutePredecessorTransitionErrorV2::Evidence(actual),
            )) => assert_eq!(actual, expected),
            _ => panic!("drifted registry evidence must fail closed"),
        }
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(handle.active_count(), 0);
    }
}

#[test]
fn barrier_resume_cannot_open_public_admission() {
    let (observed, _mutations, _receipt, _lease, handle, drops) = local_observed_replacement();
    let draining = pause_and_transition(
        observed,
        &PredecessorTransitionPort {
            initial_active_interactions: 0,
            drift_predecessor_identity: false,
            drift_successor_fence: false,
        },
    );
    let result = block_on_ready(
        draining
            .begin_barrier_a_resume()
            .execute(&BarrierResumePort {
                admission: RuntimeAdmissionDispositionV2::Open,
            }),
    );
    assert!(matches!(
        result,
        Err(RuntimeReplacementExecutionErrorV2::Failed(
            RuntimeBarrierAResumeErrorV2::Evidence(
                RuntimeBarrierAResumeEvidenceErrorV2::PublicAdmissionOpened
            )
        ))
    ));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(handle.active_count(), 0);
}

#[test]
fn predecessor_removal_waits_for_active_interactions_to_reach_zero() {
    let (observed, _mutations, _receipt, lease, handle, drops) = local_observed_replacement();
    let resumed = resume_barrier(pause_and_transition(
        observed,
        &PredecessorTransitionPort {
            initial_active_interactions: 2,
            drift_predecessor_identity: false,
            drift_successor_fence: false,
        },
    ));
    let (ready, disconnected) = disconnect_resumed(resumed, lease);
    let seen_active = Arc::new(AtomicUsize::new(0));
    let result = block_on_ready(ready.begin_predecessor_retirement().unwrap().execute(
        &PredecessorRetirementPort {
            final_observation: Mutex::new(Some((at(32), disconnected))),
            removed_active_interactions: 1,
            drift_removed_identity: false,
            initial_active_interactions: Arc::clone(&seen_active),
        },
    ));
    assert!(matches!(
        result,
        Err(RuntimeReplacementExecutionErrorV2::Failed(
            RuntimePredecessorRetirementErrorV2::Evidence(
                RuntimePredecessorRetirementEvidenceErrorV2::ActiveInteractionsPresent
            )
        ))
    ));
    assert_eq!(seen_active.load(Ordering::SeqCst), 2);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(handle.active_count(), 0);
}
