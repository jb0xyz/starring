use std::future::{ready, Future};
use std::num::{NonZeroU32, NonZeroU64};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use automation_runtime_controller::{
    runtime_desired_target_digest_v1, RuntimeBindingPinV1, RuntimeCanonicalProductDrainV2,
    RuntimeControllerConfigV1, RuntimeConvergenceMutationV1, RuntimeDesiredTargetDigestV1,
    RuntimeDrainIntentDigestV2, RuntimeExecutionReceiptV1, RuntimeMutationReceiptV1,
    RuntimeMutationRequestV1, RuntimeProductMutationDigestV2, RuntimeServingSlotV2,
};
use automation_runtime_convergence::{
    ActivationRequestId, BindingRevision, CommandGuardV1, ControllerId, DeploymentId, FencingToken,
    InstallationId, LeaseRequestV1, ProcessInstanceId, PromotionId, RuntimeDeployment,
    RuntimeDeploymentIdentityV1, RuntimeDeploymentTargetV1, RuntimeGeneration, TenantId,
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
