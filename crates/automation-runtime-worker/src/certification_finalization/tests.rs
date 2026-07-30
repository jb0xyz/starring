use std::future::{ready, Future};
use std::num::{NonZeroU32, NonZeroU64};
use std::task::{Context, Poll, Waker};
use std::thread::JoinHandle;
use std::time::Duration;

use automation_runtime_controller::{
    GatewayShardIdV1, RuntimeBarrierIdV1, RuntimeBarrierPauseWitnessV2, RuntimeBindingPinV1,
    RuntimeBuildRevisionV1, RuntimeCanonicalProductDrainV2,
    RuntimeCertificationIntentReservationOutcomeV2, RuntimeCertificationLookupV2,
    RuntimeCertificationObservationV2, RuntimeCertificationOperationIdV2,
    RuntimeCertificationReceiptV2, RuntimeCertificationReservationInputV2,
    RuntimeCertificationReservationScopeLookupV2, RuntimeConvergenceSessionV1,
    RuntimeDeploymentScopeV1, RuntimeDrainIntentDigestV2, RuntimeGatewayAdmissionSequenceV2,
    RuntimeGatewayOwnerLeaseIdV1, RuntimeGatewayReadyAttestationV2, RuntimeGatewayReadyKindV2,
    RuntimePanelEvidenceV2, RuntimeProductMutationDigestV2, RuntimeRouteAdmissionAttestationV2,
    RuntimeServingIdentityV2, RuntimeServingReceiptV2, RuntimeServingRouteAttestationV2,
};
use automation_runtime_convergence::{
    ActivationAttestationV1, ActivationOutcomeKindV1, ActivationRequestId, BindingRevision,
    CommandGuardV1, ControllerId, DeploymentId, DrainAttestationV1, FencingToken,
    GatewayReadyAttestationV1, GatewayReadyKindV1, InstallationId, LeaseRequestV1,
    PanelCertificateId, PanelCertificateV1, PanelReportDigestV1, PreflightAttestationV1,
    ProcessInstanceId, PromotionId, RuntimeDeployment, RuntimeDeploymentIdentityV1,
    RuntimeDeploymentSnapshotV1, RuntimeDeploymentTargetV1, RuntimeGeneration,
    RuntimeProcessIdentityV1, TenantId,
};
use chrono::{DateTime, Utc};

use super::{
    RuntimeAbortErrorV2, RuntimeAbortRecoveryPortV2, RuntimeAuthorizedCertificationRequestV2,
    RuntimeCertificationAbortOutcomeV2, RuntimeCertificationAuthorizationErrorV2,
    RuntimeCertificationFinalizationOutcomeV2, RuntimeCertificationFinalizerPortV2,
    RuntimeCertificationFinalizerRegistrationV2, RuntimeCertificationFinalizerRejectionV2,
    RuntimeCertificationPrepareFailedV2, RuntimeCertificationRecoveryOutcomeV2,
    RuntimeCertificationRecoveryResolutionV2, RuntimeCertificationReservationProposalV2,
    RuntimeCommitCompletionErrorV2, RuntimeCommitRecoveryPortV2, RuntimeLiveCertificationPortV2,
    RuntimePreparedCertificationV2, RuntimePreparedLiveCertificationPortV2,
    RuntimeReservedCertificationV2,
};
use crate::{
    RuntimeCertificationReservationPortV2, RuntimeGatewayCoordinatorGenerationV2,
    RuntimePausedGatewayObservationV2, RuntimePausedGatewaySequenceV2, RuntimeRecoveryPendingV2,
};

fn complete<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("test future unexpectedly pending"),
    }
}

fn at(second: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(second, 0).unwrap()
}

fn non_zero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
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
        promotion_id: PromotionId::parse("c".repeat(64)).unwrap(),
        activation_request_id: ActivationRequestId::parse("activation:1").unwrap(),
    }
}

fn process_identity() -> RuntimeProcessIdentityV1 {
    RuntimeProcessIdentityV1 {
        target: target(),
        runtime_generation: RuntimeGeneration::new(4).unwrap(),
        process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
    }
}

fn guard(deployment: &RuntimeDeployment, now: DateTime<Utc>) -> CommandGuardV1 {
    CommandGuardV1 {
        expected_revision: deployment.revision(),
        controller_id: ControllerId::parse("controller:1").unwrap(),
        fencing_token: FencingToken::new(3).unwrap(),
        runtime_generation: RuntimeGeneration::new(4).unwrap(),
        now,
    }
}

fn awaiting_deployment() -> RuntimeDeployment {
    let mut deployment = RuntimeDeployment::request(
        identity(),
        target(),
        RuntimeGeneration::new(4).unwrap(),
        None,
        at(1),
    )
    .unwrap();
    deployment
        .acquire_lease(LeaseRequestV1 {
            expected_revision: deployment.revision(),
            controller_id: ControllerId::parse("controller:1").unwrap(),
            fencing_token: FencingToken::new(3).unwrap(),
            now: at(10),
            expires_at: at(1_000),
        })
        .unwrap();
    deployment
        .accept_preflight(
            &guard(&deployment, at(11)),
            PreflightAttestationV1 {
                target: target(),
                runtime_generation: RuntimeGeneration::new(4).unwrap(),
                observed_runtime: None,
                checked_at: at(11),
            },
        )
        .unwrap();
    deployment
        .request_drain(&guard(&deployment, at(12)))
        .unwrap();
    deployment
        .accept_drain(
            &guard(&deployment, at(13)),
            DrainAttestationV1 {
                previous_runtime: None,
                target_runtime_generation: RuntimeGeneration::new(4).unwrap(),
                drained_at: at(13),
            },
        )
        .unwrap();
    deployment
        .begin_activation(&guard(&deployment, at(14)))
        .unwrap();
    deployment
        .accept_activation(
            &guard(&deployment, at(15)),
            ActivationAttestationV1 {
                activation_request_id: identity().activation_request_id,
                target: target(),
                runtime_generation: RuntimeGeneration::new(4).unwrap(),
                kind: ActivationOutcomeKindV1::Activated,
                activated_at: at(15),
            },
        )
        .unwrap();
    deployment
        .begin_panel_reconciliation(&guard(&deployment, at(16)))
        .unwrap();
    deployment
        .accept_panel_certificate(
            &guard(&deployment, at(17)),
            PanelCertificateV1 {
                certificate_id: PanelCertificateId::parse("panel:1").unwrap(),
                report_digest: PanelReportDigestV1::parse("d".repeat(64)).unwrap(),
                target: target(),
                runtime_generation: RuntimeGeneration::new(4).unwrap(),
                process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
                declared_count: 1,
                installed_count: 1,
                unchanged_count: 0,
                skipped_transient_count: 0,
                skipped_unresolved_channel_count: 0,
                failed_count: 0,
                ambiguous_outcome_count: 0,
                stale_message_cleanup_pending_count: 0,
                orphan_message_cleanup_pending_count: 0,
                reposted_old_message_cleanup_pending_count: 0,
                reconciled_at: at(17),
            },
        )
        .unwrap();
    deployment
}

fn awaiting_session() -> RuntimeConvergenceSessionV1 {
    let deployment = awaiting_deployment();
    RuntimeConvergenceSessionV1::from_claim(
        automation_runtime_controller::RuntimeExecutionReceiptV1 {
            snapshot: deployment.snapshot(),
            controller_id: ControllerId::parse("controller:1").unwrap(),
            fencing_token: FencingToken::new(3).unwrap(),
            convergence_attempt: NonZeroU32::new(5).unwrap(),
            acquired_at: at(10),
            expires_at: at(1_000),
        },
    )
    .unwrap()
}

fn reservation_input() -> RuntimeCertificationReservationInputV2 {
    RuntimeCertificationReservationInputV2 {
        operation_id: RuntimeCertificationOperationIdV2::parse("00112233445566778899aabbccddeeff")
            .unwrap(),
        binding_pin: RuntimeBindingPinV1 {
            tenant_id: TenantId::parse("tenant:1").unwrap(),
            installation_id: InstallationId::parse("installation:1").unwrap(),
            installation_authority_revision: non_zero(6),
            binding_revision: BindingRevision::new(3).unwrap(),
            binding_fingerprint: target().binding_fingerprint,
        },
        gateway_owner_lease_id: RuntimeGatewayOwnerLeaseIdV1 {
            gateway_shard_id: GatewayShardIdV1::parse("shard:0").unwrap(),
            process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
            lease_epoch: non_zero(5),
            expected_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
        },
        observed_owner_revision: non_zero(7),
        runtime_build_revision: RuntimeBuildRevisionV1::parse("build:1").unwrap(),
        panel: RuntimePanelEvidenceV2 {
            certificate_id: PanelCertificateId::parse("panel:1").unwrap(),
            report_digest: PanelReportDigestV1::parse("d".repeat(64)).unwrap(),
            process_identity: process_identity(),
            controller_fencing_token: FencingToken::new(3).unwrap(),
        },
        serving_lease_for: Duration::from_secs(30),
    }
}

fn route_admission() -> RuntimeRouteAdmissionAttestationV2 {
    RuntimeRouteAdmissionAttestationV2 {
        barrier_id: RuntimeBarrierIdV1::parse("ffeeddccbbaa99887766554433221100").unwrap(),
        pause: RuntimeBarrierPauseWitnessV2 {
            coordinator_generation: non_zero(8),
            connection_epoch: non_zero(9),
            paused_admission_revision: non_zero(10),
            pause_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(12)),
        },
        gateway: RuntimeGatewayReadyAttestationV2 {
            process_instance_id: ProcessInstanceId::parse("process:1").unwrap(),
            connection_epoch: non_zero(9),
            kind: RuntimeGatewayReadyKindV2::Ready,
            admission_revision: non_zero(10),
            connected_event_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(11)),
            resume_sequence: RuntimeGatewayAdmissionSequenceV2::new(non_zero(13)),
        },
        gateway_owner_lease_id: reservation_input().gateway_owner_lease_id,
        attested_owner_revision: non_zero(7),
        route: RuntimeServingRouteAttestationV2 {
            identity: process_identity(),
            controller_fencing_token: FencingToken::new(3).unwrap(),
            route_incarnation: non_zero(14),
            activation_sequence: non_zero(15),
        },
    }
}

fn barrier_id() -> RuntimeBarrierIdV1 {
    RuntimeBarrierIdV1::parse("ffeeddccbbaa99887766554433221100").unwrap()
}

fn paused_gateway() -> RuntimePausedGatewayObservationV2 {
    paused_gateway_with_kind(RuntimeGatewayReadyKindV2::Ready)
}

fn paused_gateway_with_kind(kind: RuntimeGatewayReadyKindV2) -> RuntimePausedGatewayObservationV2 {
    RuntimePausedGatewayObservationV2::new(
        RuntimeGatewayCoordinatorGenerationV2::new(non_zero(8)),
        ProcessInstanceId::parse("process:1").unwrap(),
        non_zero(9),
        kind,
        non_zero(10),
        RuntimePausedGatewaySequenceV2::new(
            RuntimeGatewayAdmissionSequenceV2::new(non_zero(12)),
            RuntimeGatewayAdmissionSequenceV2::new(non_zero(11)),
            None,
        )
        .unwrap(),
    )
}

struct ReservationPort {
    outcome: RuntimeCertificationIntentReservationOutcomeV2,
}

impl RuntimeCertificationReservationPortV2 for ReservationPort {
    type Error = &'static str;

    fn reserve_certification_intent(
        &self,
        _reservation: automation_runtime_controller::RuntimeReservedCertificationIntentV2,
    ) -> impl Future<Output = Result<RuntimeCertificationIntentReservationOutcomeV2, Self::Error>> + Send
    {
        ready(Ok(self.outcome.clone()))
    }

    fn observe_certification_reservation_scope(
        &self,
        _lookup: RuntimeCertificationReservationScopeLookupV2,
    ) -> impl Future<
        Output = Result<
            automation_runtime_controller::RuntimeCertificationReservationScopeObservationV2,
            Self::Error,
        >,
    > + Send {
        ready(Err("unused"))
    }
}

fn reserved_fixture() -> (RuntimeReservedCertificationV2, RuntimeDeploymentSnapshotV1) {
    let mut session = awaiting_session();
    let execution = session.current_execution_receipt().unwrap();
    let proposed = session
        .begin_certification_reservation_v2(reservation_input())
        .unwrap();
    let lookup =
        RuntimeCertificationReservationScopeLookupV2::from_awaiting_execution(&execution).unwrap();
    let proposal =
        RuntimeCertificationReservationProposalV2::from_reserved_intent(proposed.clone(), lookup)
            .unwrap();
    let checked = complete(proposal.reserve(&ReservationPort {
        outcome: RuntimeCertificationIntentReservationOutcomeV2::Reserved(proposed),
    }))
    .unwrap();
    let authority = session
        .apply_certification_reservation_v2(checked.into_session_outcome())
        .unwrap();
    (
        RuntimeReservedCertificationV2::from_reservation_authority(authority),
        execution.snapshot,
    )
}

enum CommitMode {
    Committed,
    RolledBack,
    Unknown,
}

struct Prepared {
    awaiting: RuntimeDeploymentSnapshotV1,
    mode: CommitMode,
}

struct AbortRecovery;

impl RuntimeAbortRecoveryPortV2 for AbortRecovery {
    type Error = &'static str;
    type TransactionEnded = ();

    fn quiesce(
        self,
        _timeout: Duration,
    ) -> impl Future<Output = Result<(), RuntimeRecoveryPendingV2<Self::Error, Self>>> + Send {
        ready(Ok(()))
    }
}

struct CommitRecovery {
    lookup: RuntimeCertificationLookupV2,
    observation: RuntimeCertificationObservationV2,
}

impl RuntimeCommitRecoveryPortV2 for CommitRecovery {
    type Error = &'static str;
    type TransactionEnded = ();

    fn lookup(&self) -> &RuntimeCertificationLookupV2 {
        &self.lookup
    }

    fn quiesce_and_observe(
        self,
        _timeout: Duration,
    ) -> impl Future<
        Output = Result<
            RuntimeCertificationRecoveryOutcomeV2<()>,
            RuntimeRecoveryPendingV2<Self::Error, Self>,
        >,
    > + Send {
        ready(Ok(RuntimeCertificationRecoveryOutcomeV2 {
            transaction_ended: (),
            observation: self.observation,
        }))
    }
}

impl RuntimePreparedLiveCertificationPortV2 for Prepared {
    type Error = &'static str;
    type TransactionEnded = ();
    type AbortRecovery = AbortRecovery;
    type CommitRecovery = CommitRecovery;

    fn must_commit_before(&self) -> DateTime<Utc> {
        at(900)
    }

    fn commit_live_v2(
        self,
        authorized: RuntimeAuthorizedCertificationRequestV2,
    ) -> impl Future<
        Output = Result<
            RuntimeCertificationReceiptV2,
            RuntimeCommitCompletionErrorV2<
                Self::Error,
                Self::CommitRecovery,
                Self::TransactionEnded,
            >,
        >,
    > + Send {
        let result = match self.mode {
            CommitMode::Committed => Ok(committed_receipt(self.awaiting, &authorized)),
            CommitMode::RolledBack => Err(RuntimeCommitCompletionErrorV2::DefinitelyRolledBack {
                source: "rolled back",
                transaction_ended: (),
            }),
            CommitMode::Unknown => {
                let canonical = authorized.canonical();
                let request = authorized.request();
                let lookup = RuntimeCertificationLookupV2 {
                    scope: request.intent.guard.scope.clone(),
                    deployment_revision: request.intent.guard.expected_revision,
                    convergence_attempt: request.intent.guard.convergence_attempt,
                    operation_id: request.intent.operation_id.clone(),
                    request_digest: canonical.request_digest().clone(),
                };
                let observation = RuntimeCertificationObservationV2::NotCommitted {
                    snapshot: self.awaiting,
                    convergence_attempt: lookup.convergence_attempt,
                    operation_id: lookup.operation_id.clone(),
                    request_digest: lookup.request_digest.clone(),
                    observed_deployment_revision: lookup.deployment_revision,
                    observed_at: at(101),
                };
                Err(RuntimeCommitCompletionErrorV2::CommitUnknown {
                    source: "unknown",
                    recovery: CommitRecovery {
                        lookup,
                        observation,
                    },
                })
            }
        };
        ready(result)
    }

    fn abort(
        self,
    ) -> impl Future<Output = Result<(), RuntimeAbortErrorV2<Self::Error, Self::AbortRecovery>>> + Send
    {
        ready(Ok(()))
    }
}

struct LivePort {
    awaiting: RuntimeDeploymentSnapshotV1,
    mode: Option<CommitMode>,
}

impl RuntimeLiveCertificationPortV2 for LivePort {
    type Error = &'static str;
    type Prepared = Prepared;

    fn prepare_live_v2(
        &self,
        _reservation: automation_runtime_controller::RuntimeReservedCertificationIntentV2,
    ) -> impl Future<Output = Result<Self::Prepared, Self::Error>> + Send {
        ready(match &self.mode {
            Some(CommitMode::Committed) => Ok(Prepared {
                awaiting: self.awaiting.clone(),
                mode: CommitMode::Committed,
            }),
            Some(CommitMode::RolledBack) => Ok(Prepared {
                awaiting: self.awaiting.clone(),
                mode: CommitMode::RolledBack,
            }),
            Some(CommitMode::Unknown) => Ok(Prepared {
                awaiting: self.awaiting.clone(),
                mode: CommitMode::Unknown,
            }),
            None => Err("prepare failed"),
        })
    }

    fn observe_live_v2(
        &self,
        _lookup: RuntimeCertificationLookupV2,
    ) -> impl Future<Output = Result<RuntimeCertificationObservationV2, Self::Error>> + Send {
        ready(Err("unused"))
    }
}

fn committed_receipt(
    awaiting: RuntimeDeploymentSnapshotV1,
    authorized: &RuntimeAuthorizedCertificationRequestV2,
) -> RuntimeCertificationReceiptV2 {
    let request = authorized.request();
    let mut deployment = RuntimeDeployment::restore(awaiting).unwrap();
    let outcome = deployment
        .certify_live(
            &CommandGuardV1 {
                expected_revision: request.intent.guard.expected_revision,
                controller_id: request.intent.guard.controller_id.clone(),
                fencing_token: request.intent.guard.fencing_token,
                runtime_generation: request.intent.guard.runtime_generation,
                now: at(100),
            },
            GatewayReadyAttestationV1 {
                target: request.intent.target.clone(),
                runtime_generation: request.intent.guard.runtime_generation,
                process_instance_id: request.intent.process_identity.process_instance_id.clone(),
                kind: match request.route_admission.gateway.kind {
                    RuntimeGatewayReadyKindV2::Ready => GatewayReadyKindV1::DiscordReady,
                    RuntimeGatewayReadyKindV2::Resumed => GatewayReadyKindV1::DiscordResumed,
                },
                ready_at: at(99),
            },
            at(100),
        )
        .unwrap();
    RuntimeCertificationReceiptV2 {
        action_id: request.intent.action_id,
        outcome,
        snapshot: deployment.snapshot(),
        convergence_attempt: request.intent.guard.convergence_attempt,
        operation_id: request.intent.operation_id.clone(),
        intent_fingerprint: request.intent_fingerprint.clone(),
        request_digest: authorized.canonical().request_digest().clone(),
        attestation_digest: authorized.canonical().live_attestation_digest().clone(),
        route_admission: request.route_admission.clone(),
        serving: RuntimeServingReceiptV2 {
            identity: RuntimeServingIdentityV2 {
                scope: RuntimeDeploymentScopeV1::from_identity(deployment.identity()),
                operation_id: request.intent.operation_id.clone(),
                attestation_digest: authorized.canonical().live_attestation_digest().clone(),
                process_identity: request.intent.process_identity.clone(),
                lease_epoch: non_zero(16),
                revision: non_zero(17),
            },
            acquired_at: at(100),
            last_heartbeat_at: at(100),
            expires_at: at(130),
            connected: true,
            serving: true,
        },
        certified_at: at(100),
    }
}

struct ThreadFinalizer;

impl RuntimeCertificationFinalizerPortV2<Prepared> for ThreadFinalizer {
    type Error = &'static str;
    type Accepted =
        JoinHandle<RuntimeCertificationFinalizationOutcomeV2<&'static str, CommitRecovery, ()>>;

    fn accept_certification_finalizer(
        &self,
        registration: RuntimeCertificationFinalizerRegistrationV2<Prepared>,
    ) -> Result<Self::Accepted, RuntimeCertificationFinalizerRejectionV2<Prepared, Self::Error>>
    {
        let job = registration.into_owned_job();
        Ok(std::thread::spawn(move || complete(job.run())))
    }
}

fn prepared_fixture(mode: CommitMode) -> RuntimePreparedCertificationV2<Prepared> {
    let (reserved, awaiting) = reserved_fixture();
    complete(reserved.prepare(&LivePort {
        awaiting,
        mode: Some(mode),
    }))
    .unwrap()
}

#[test]
fn reservation_outcome_must_cross_the_session_before_worker_authority_exists() {
    let (reserved, snapshot) = reserved_fixture();

    assert_eq!(
        reserved.reserved_intent().operation_scope().scope(),
        &RuntimeDeploymentScopeV1::from_identity(&snapshot.identity)
    );
    assert_eq!(
        reserved.reserved_intent().operation_id().as_str(),
        "00112233445566778899aabbccddeeff"
    );
}

#[test]
fn prepare_failure_has_no_finalizer_or_commit_authority() {
    let (reserved, awaiting) = reserved_fixture();
    let failure: RuntimeCertificationPrepareFailedV2<&str> =
        complete(reserved.prepare(&LivePort {
            awaiting,
            mode: None,
        }))
        .unwrap_err();

    assert_eq!(failure.source(), &"prepare failed");
    assert_eq!(
        failure.reserved().reserved_intent().operation_id().as_str(),
        "00112233445566778899aabbccddeeff"
    );
}

#[test]
fn accepted_finalizer_owns_the_irreversible_job_and_commits_exactly() {
    let route_admission = route_admission();
    let completed = prepared_fixture(CommitMode::Committed)
        .complete_barrier_b_v2(barrier_id(), paused_gateway(), route_admission.clone())
        .unwrap();
    assert_eq!(completed.request().route_admission, route_admission);
    let registration = completed
        .authorize_finalization()
        .into_owned_job()
        .into_registration();
    let accepted = registration.accept(&ThreadFinalizer).unwrap();
    let outcome = accepted.join().unwrap();

    assert!(matches!(
        outcome,
        RuntimeCertificationFinalizationOutcomeV2::Committed(committed)
            if committed.receipt().operation_id.as_str()
                == "00112233445566778899aabbccddeeff"
                && committed.canonical().request_digest()
                    == &committed.receipt().request_digest
    ));
}

#[test]
fn barrier_b_completion_preserves_the_discord_ready_kind() {
    for kind in [
        RuntimeGatewayReadyKindV2::Ready,
        RuntimeGatewayReadyKindV2::Resumed,
    ] {
        let mut admission = route_admission();
        admission.gateway.kind = kind;
        let completed = prepared_fixture(CommitMode::Committed)
            .complete_barrier_b_v2(barrier_id(), paused_gateway_with_kind(kind), admission)
            .unwrap();
        assert_eq!(completed.request().route_admission.gateway.kind, kind);
    }
}

#[test]
fn rollback_and_unknown_commit_have_disjoint_terminal_paths() {
    let outcome = complete(
        prepared_fixture(CommitMode::RolledBack)
            .complete_barrier_b_v2(barrier_id(), paused_gateway(), route_admission())
            .unwrap()
            .authorize_finalization()
            .into_owned_job()
            .run(),
    );
    assert!(matches!(
        outcome,
        RuntimeCertificationFinalizationOutcomeV2::DefinitelyRolledBack {
            source: "rolled back",
            ..
        }
    ));

    let outcome = complete(
        prepared_fixture(CommitMode::Unknown)
            .complete_barrier_b_v2(barrier_id(), paused_gateway(), route_admission())
            .unwrap()
            .authorize_finalization()
            .into_owned_job()
            .run(),
    );
    let RuntimeCertificationFinalizationOutcomeV2::Indeterminate(recovery) = outcome else {
        panic!("expected lookup-only recovery")
    };
    let recovered = match complete(recovery.quiesce_and_observe(Duration::from_secs(1))) {
        Ok(recovered) => recovered,
        Err(_) => panic!("recovery unexpectedly pending"),
    };
    assert!(matches!(
        recovered,
        RuntimeCertificationRecoveryResolutionV2::DefinitelyRolledBack { .. }
    ));
}

#[test]
fn prepared_abort_is_explicitly_definite_and_never_a_commit_unknown() {
    let (reserved, awaiting) = reserved_fixture();
    let prepared = complete(reserved.prepare(&LivePort {
        awaiting,
        mode: Some(CommitMode::Committed),
    }))
    .unwrap();

    assert!(matches!(
        complete(prepared.abort()),
        RuntimeCertificationAbortOutcomeV2::DefinitelyRolledBack(_)
    ));
}

#[test]
fn barrier_b_completion_rejects_mismatched_barrier_and_retains_prepared() {
    let failure = prepared_fixture(CommitMode::Committed)
        .complete_barrier_b_v2(
            RuntimeBarrierIdV1::parse("00112233445566778899aabbccddeeff").unwrap(),
            paused_gateway(),
            route_admission(),
        )
        .unwrap_err();

    assert!(matches!(
        failure.source(),
        RuntimeCertificationAuthorizationErrorV2::BarrierIdMismatch
    ));
    assert_eq!(
        failure.prepared().reserved_intent().operation_id().as_str(),
        "00112233445566778899aabbccddeeff"
    );
}

#[test]
fn barrier_b_completion_rejects_paused_or_resumed_gateway_drift() {
    let mut pause_drift = route_admission();
    pause_drift.pause.paused_admission_revision = non_zero(11);
    let failure = prepared_fixture(CommitMode::Committed)
        .complete_barrier_b_v2(barrier_id(), paused_gateway(), pause_drift)
        .unwrap_err();
    assert!(matches!(
        failure.source(),
        RuntimeCertificationAuthorizationErrorV2::PausedGatewayMismatch
    ));

    let mut resume_drift = route_admission();
    resume_drift.gateway.kind = RuntimeGatewayReadyKindV2::Resumed;
    let failure = prepared_fixture(CommitMode::Committed)
        .complete_barrier_b_v2(barrier_id(), paused_gateway(), resume_drift)
        .unwrap_err();
    assert!(matches!(
        failure.source(),
        RuntimeCertificationAuthorizationErrorV2::ResumedGatewayMismatch
    ));
}

#[test]
fn barrier_b_completion_rejects_route_owner_and_target_mismatch_canonically() {
    let mut owner_drift = route_admission();
    owner_drift.attested_owner_revision = non_zero(8);
    let failure = prepared_fixture(CommitMode::Committed)
        .complete_barrier_b_v2(barrier_id(), paused_gateway(), owner_drift)
        .unwrap_err();
    assert!(matches!(
        failure.source(),
        RuntimeCertificationAuthorizationErrorV2::Canonical(_)
    ));

    let mut route_drift = route_admission();
    route_drift.route.identity.runtime_generation = RuntimeGeneration::new(5).unwrap();
    let failure = prepared_fixture(CommitMode::Committed)
        .complete_barrier_b_v2(barrier_id(), paused_gateway(), route_drift)
        .unwrap_err();
    assert!(matches!(
        failure.source(),
        RuntimeCertificationAuthorizationErrorV2::Canonical(_)
    ));
}
