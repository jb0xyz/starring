use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use authoring_application::{
    ApprovalPayloadDigestV1, AuthenticatedActorV1, AuthenticatedSessionFingerprintV1,
    AuthenticationClaimsV1, AuthenticationError, AuthenticationPort,
    AuthorizedCancelProductLifecycleV1, AuthorizedInstallationScopeV1, AuthorizedInstallationV1,
    CancelProductLifecycleMutationV1, CapabilityV1, FreshGuildAuthorityError,
    FreshGuildAuthorityPort, InstallationSelectorV1, MutationAuthenticationPort,
    ProductApplicationError, ProductControlApplication, ProductControlPortError,
    ProductDecisionPhaseV1, ProductDecisionProjectionV1, ProductDrainSelectorError,
    ProductDrainSelectorV1, ProductIdempotencyKeyV1,
    ProductLifecycleCancellationDeploymentProjectionV1,
    ProductLifecycleCancellationDrainProjectionV1, ProductLifecycleCancellationPort,
    ProductLifecycleCancellationReasonError, ProductLifecycleCancellationReasonV1,
    ProductLifecycleCancellationReceiptError, ProductLifecycleCancellationReceiptV1,
    ProductLifecycleCancellationSlotProjectionV1, ProductRequestIdV1, ProductRevisionV1,
    PromotionSelectorV1,
};
use authoring_promotion::{AutomationInstallationId, PrincipalId, PromotionId, TenantId};
use discord_model::{GuildId, UserId};
use futures::executor::block_on;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Evidence;

struct Authentication {
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl AuthenticationPort for Authentication {
    type Credential = str;

    async fn authenticate(
        &self,
        _credential: &Self::Credential,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        panic!("cancellation must use mutation authentication")
    }
}

impl MutationAuthenticationPort for Authentication {
    type CsrfProof = str;

    async fn authenticate_mutation(
        &self,
        credential: &Self::Credential,
        csrf: &Self::CsrfProof,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        self.events.lock().unwrap().push("authenticate_mutation");
        assert_eq!(credential, "session");
        if csrf != "csrf" {
            return Err(AuthenticationError::InvalidCsrf);
        }
        Ok(AuthenticationClaimsV1::from_authentication(
            PrincipalId::parse("principal-1").unwrap(),
            AuthenticatedSessionFingerprintV1::from_sha256_digest([9_u8; 32]),
        ))
    }
}

struct Authority {
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl FreshGuildAuthorityPort for Authority {
    type Evidence = Evidence;

    async fn authorize_installation(
        &self,
        actor: &AuthenticatedActorV1,
        installation: &InstallationSelectorV1,
        capability: CapabilityV1,
    ) -> Result<AuthorizedInstallationV1<Self::Evidence>, FreshGuildAuthorityError> {
        self.events.lock().unwrap().push("authorize");
        assert_eq!(actor.principal_id().as_str(), "principal-1");
        assert_eq!(actor.session_fingerprint().as_bytes(), &[9_u8; 32]);
        assert_eq!(installation.installation_id().as_str(), "installation-1");
        assert_eq!(capability, CapabilityV1::CancelLifecycle);
        Ok(AuthorizedInstallationV1::from_fresh_authority(
            AuthorizedInstallationScopeV1::from_fresh_authority(
                TenantId::parse("tenant-1").unwrap(),
                AutomationInstallationId::parse("installation-1").unwrap(),
                GuildId(100),
                UserId(200),
            ),
            Evidence,
        ))
    }
}

#[derive(Clone, Copy)]
enum ReceiptFault {
    None,
    Replay,
    ProductScope,
    ProductPhase,
    ProductRevision,
    Drain,
    DeploymentRevision,
    DrainRevision,
    Epoch,
    ReplayDrain,
}

struct CancellationPort {
    events: Arc<Mutex<Vec<&'static str>>>,
    fault: ReceiptFault,
}

impl ProductLifecycleCancellationPort<Evidence> for CancellationPort {
    async fn cancel_lifecycle_idempotent(
        &self,
        request: AuthorizedCancelProductLifecycleV1<'_, Evidence>,
    ) -> Result<ProductLifecycleCancellationReceiptV1, ProductControlPortError> {
        self.events.lock().unwrap().push("cancel_lifecycle");
        assert_eq!(request.request_id().as_str(), "request-1");
        assert_eq!(request.actor().principal_id().as_str(), "principal-1");
        assert_eq!(request.session_fingerprint().as_bytes(), &[9_u8; 32]);
        assert_eq!(request.scope().tenant_id().as_str(), "tenant-1");
        assert_eq!(request.scope().installation_id().as_str(), "installation-1");
        assert_eq!(request.scope().guild_id(), GuildId(100));
        assert_eq!(request.scope().acting_user_id(), UserId(200));
        assert_eq!(request.evidence(), &Evidence);
        assert_eq!(
            format!("{:?}", request.context()),
            "ProductMutationContextV1(<redacted>)"
        );
        assert_eq!(
            request.command().expected_payload_digest.as_str(),
            "c".repeat(64)
        );
        assert_eq!(request.command().expected_revision.get(), 3);
        assert_eq!(
            request.command().idempotency_key.as_str(),
            "cancel-lifecycle-key"
        );
        assert_eq!(
            request.command().reason.as_str(),
            "keep the current automation"
        );
        let tenant_id = if matches!(self.fault, ReceiptFault::ProductScope) {
            TenantId::parse("tenant-2").unwrap()
        } else {
            request.scope().tenant_id().clone()
        };
        let phase = if matches!(self.fault, ReceiptFault::ProductPhase) {
            ProductDecisionPhaseV1::Rejected
        } else {
            ProductDecisionPhaseV1::Approved
        };
        let revision = if matches!(self.fault, ReceiptFault::ProductRevision) {
            ProductRevisionV1::new(4).unwrap()
        } else {
            ProductRevisionV1::new(3).unwrap()
        };
        let decision = ProductDecisionProjectionV1::from_server_projection(
            tenant_id,
            request.scope().installation_id().clone(),
            request.scope().guild_id(),
            request.command().promotion.promotion_id().clone(),
            revision,
            phase,
        );
        let source_drain_selector =
            if matches!(self.fault, ReceiptFault::Drain | ReceiptFault::ReplayDrain) {
                alternate_drain_selector()
            } else {
                request.command().drain_selector.clone()
            };
        let resulting_runtime_deployment_revision =
            if matches!(self.fault, ReceiptFault::DeploymentRevision) {
                12
            } else {
                11
            };
        let terminal_intent_revision = if matches!(self.fault, ReceiptFault::DrainRevision) {
            9
        } else {
            8
        };
        let successor_slot_writer_epoch = if matches!(self.fault, ReceiptFault::Epoch) {
            22
        } else {
            21
        };
        let exact_replay = matches!(self.fault, ReceiptFault::Replay | ReceiptFault::ReplayDrain);
        let deployment =
            ProductLifecycleCancellationDeploymentProjectionV1::from_server_projection(
                resulting_runtime_deployment_revision,
            )
            .map_err(|error| ProductControlPortError::Backend(error.to_string()))?;
        let drain = ProductLifecycleCancellationDrainProjectionV1::from_server_projection(
            source_drain_selector,
            terminal_intent_revision,
            "d".repeat(64),
        )
        .map_err(|error| ProductControlPortError::Backend(error.to_string()))?;
        let slot = ProductLifecycleCancellationSlotProjectionV1::from_server_projection(
            20,
            successor_slot_writer_epoch,
        )
        .map_err(|error| ProductControlPortError::Backend(error.to_string()))?;
        ProductLifecycleCancellationReceiptV1::from_server_projection(
            decision,
            deployment,
            drain,
            slot,
            UNIX_EPOCH + Duration::from_secs(100),
            exact_replay,
        )
        .map_err(|error| ProductControlPortError::Backend(error.to_string()))
    }
}

fn installation() -> InstallationSelectorV1 {
    InstallationSelectorV1::new(AutomationInstallationId::parse("installation-1").unwrap())
}

fn promotion() -> PromotionSelectorV1 {
    PromotionSelectorV1::new(PromotionId::parse(&"a".repeat(64)).unwrap())
}

fn drain_selector() -> ProductDrainSelectorV1 {
    ProductDrainSelectorV1::from_server_projection(
        "1".repeat(32),
        7,
        "b".repeat(64),
        "2".repeat(32),
        10,
    )
    .unwrap()
}

fn alternate_drain_selector() -> ProductDrainSelectorV1 {
    ProductDrainSelectorV1::from_server_projection(
        "3".repeat(32),
        7,
        "b".repeat(64),
        "4".repeat(32),
        10,
    )
    .unwrap()
}

fn command() -> CancelProductLifecycleMutationV1 {
    CancelProductLifecycleMutationV1 {
        promotion: promotion(),
        expected_payload_digest: ApprovalPayloadDigestV1::parse(&"c".repeat(64)).unwrap(),
        expected_revision: ProductRevisionV1::new(3).unwrap(),
        drain_selector: drain_selector(),
        idempotency_key: ProductIdempotencyKeyV1::parse("cancel-lifecycle-key").unwrap(),
        reason: ProductLifecycleCancellationReasonV1::parse("keep the current automation").unwrap(),
    }
}

fn request_id() -> ProductRequestIdV1 {
    ProductRequestIdV1::parse("request-1").unwrap()
}

fn cancellation_result(
    fault: ReceiptFault,
) -> Result<ProductLifecycleCancellationReceiptV1, ProductApplicationError> {
    block_on(async {
        let events = Arc::new(Mutex::new(Vec::new()));
        let authentication = Authentication {
            events: events.clone(),
        };
        let authority = Authority {
            events: events.clone(),
        };
        let cancellations = CancellationPort { events, fault };
        ProductControlApplication::new(&authentication, &authority, &cancellations, &())
            .cancel_product_lifecycle("session", "csrf", &request_id(), &installation(), command())
            .await
    })
}

#[test]
fn drain_selector_is_exact_bounded_checked_and_opaque() {
    let selector = drain_selector();
    assert_eq!(selector.drain_intent_id(), "1".repeat(32));
    assert_eq!(selector.acknowledged_intent_revision().get(), 7);
    assert_eq!(selector.acknowledged_state_digest(), "b".repeat(64));
    assert_eq!(selector.product_operation_id(), "2".repeat(32));
    assert_eq!(selector.expected_runtime_deployment_revision().get(), 10);
    assert_eq!(format!("{selector:?}"), "ProductDrainSelectorV1(<opaque>)");

    for (drain_id, revision, digest, operation_id, deployment_revision, expected) in [
        (
            "1".repeat(31),
            7,
            "b".repeat(64),
            "2".repeat(32),
            10,
            ProductDrainSelectorError::InvalidDrainIntentId,
        ),
        (
            "A".repeat(32),
            7,
            "b".repeat(64),
            "2".repeat(32),
            10,
            ProductDrainSelectorError::InvalidDrainIntentId,
        ),
        (
            "1".repeat(32),
            0,
            "b".repeat(64),
            "2".repeat(32),
            10,
            ProductDrainSelectorError::InvalidAcknowledgedIntentRevision,
        ),
        (
            "1".repeat(32),
            7,
            "B".repeat(64),
            "2".repeat(32),
            10,
            ProductDrainSelectorError::InvalidAcknowledgedStateDigest,
        ),
        (
            "1".repeat(32),
            7,
            "b".repeat(64),
            "2".repeat(31),
            10,
            ProductDrainSelectorError::InvalidProductOperationId,
        ),
        (
            "1".repeat(32),
            7,
            "b".repeat(64),
            "1".repeat(32),
            10,
            ProductDrainSelectorError::IdentityCollision,
        ),
        (
            "1".repeat(32),
            7,
            "b".repeat(64),
            "2".repeat(32),
            0,
            ProductDrainSelectorError::InvalidExpectedRuntimeDeploymentRevision,
        ),
        (
            "1".repeat(32),
            7,
            "b".repeat(64),
            "2".repeat(32),
            i64::MAX as u64 + 1,
            ProductDrainSelectorError::InvalidExpectedRuntimeDeploymentRevision,
        ),
    ] {
        assert_eq!(
            ProductDrainSelectorV1::from_server_projection(
                drain_id,
                revision,
                digest,
                operation_id,
                deployment_revision,
            )
            .unwrap_err(),
            expected
        );
    }
}

#[test]
fn cancellation_reason_enforces_whitespace_scalar_byte_and_control_bounds() {
    assert_eq!(
        ProductLifecycleCancellationReasonV1::parse(" \t ").unwrap_err(),
        ProductLifecycleCancellationReasonError::Empty
    );
    assert_eq!(
        ProductLifecycleCancellationReasonV1::parse(&"x".repeat(1_001)).unwrap_err(),
        ProductLifecycleCancellationReasonError::TooLong
    );
    assert_eq!(
        ProductLifecycleCancellationReasonV1::parse(&"😀".repeat(1_001)).unwrap_err(),
        ProductLifecycleCancellationReasonError::TooLong
    );
    assert_eq!(
        ProductLifecycleCancellationReasonV1::parse("line\nbreak").unwrap_err(),
        ProductLifecycleCancellationReasonError::ControlCharacter
    );
    let reason =
        ProductLifecycleCancellationReasonV1::parse(&format!("  {}  ", "😀".repeat(1_000)))
            .unwrap();
    assert_eq!(reason.as_str().chars().count(), 1_000);
    assert_eq!(reason.as_str().len(), 4_000);
    assert_eq!(
        format!("{reason:?}"),
        "ProductLifecycleCancellationReasonV1(<redacted>)"
    );
}

#[test]
fn cancellation_receipt_rejects_invalid_local_scalar_shapes() {
    let decision = ProductDecisionProjectionV1::from_server_projection(
        TenantId::parse("tenant-1").unwrap(),
        AutomationInstallationId::parse("installation-1").unwrap(),
        GuildId(100),
        promotion().promotion_id().clone(),
        ProductRevisionV1::new(3).unwrap(),
        ProductDecisionPhaseV1::Approved,
    );
    assert_eq!(
        ProductLifecycleCancellationDeploymentProjectionV1::from_server_projection(0).unwrap_err(),
        ProductLifecycleCancellationReceiptError::InvalidResultingRuntimeDeploymentRevision
    );
    assert_eq!(
        ProductLifecycleCancellationDrainProjectionV1::from_server_projection(
            drain_selector(),
            0,
            "d".repeat(64)
        )
        .unwrap_err(),
        ProductLifecycleCancellationReceiptError::InvalidTerminalIntentRevision
    );
    assert_eq!(
        ProductLifecycleCancellationDrainProjectionV1::from_server_projection(
            drain_selector(),
            8,
            "D".repeat(64)
        )
        .unwrap_err(),
        ProductLifecycleCancellationReceiptError::InvalidTerminalStateDigest
    );
    assert_eq!(
        ProductLifecycleCancellationSlotProjectionV1::from_server_projection(0, 21).unwrap_err(),
        ProductLifecycleCancellationReceiptError::InvalidSourceSlotWriterEpoch
    );
    assert_eq!(
        ProductLifecycleCancellationSlotProjectionV1::from_server_projection(20, 0).unwrap_err(),
        ProductLifecycleCancellationReceiptError::InvalidSuccessorSlotWriterEpoch
    );
    let build = |cancelled_at| {
        ProductLifecycleCancellationReceiptV1::from_server_projection(
            decision.clone(),
            ProductLifecycleCancellationDeploymentProjectionV1::from_server_projection(11).unwrap(),
            ProductLifecycleCancellationDrainProjectionV1::from_server_projection(
                drain_selector(),
                8,
                "d".repeat(64),
            )
            .unwrap(),
            ProductLifecycleCancellationSlotProjectionV1::from_server_projection(20, 21).unwrap(),
            cancelled_at,
            false,
        )
    };
    assert_eq!(
        build(UNIX_EPOCH + Duration::from_nanos(1)).unwrap_err(),
        ProductLifecycleCancellationReceiptError::InvalidCancellationTime
    );
}

#[test]
fn cancellation_uses_mutation_auth_cancel_capability_and_one_bound_port_call() {
    block_on(async {
        let events = Arc::new(Mutex::new(Vec::new()));
        let authentication = Authentication {
            events: events.clone(),
        };
        let authority = Authority {
            events: events.clone(),
        };
        let cancellations = CancellationPort {
            events: events.clone(),
            fault: ReceiptFault::None,
        };
        let receipt =
            ProductControlApplication::new(&authentication, &authority, &cancellations, &())
                .cancel_product_lifecycle(
                    "session",
                    "csrf",
                    &request_id(),
                    &installation(),
                    command(),
                )
                .await
                .unwrap();
        assert!(!receipt.exact_replay());
        assert_eq!(
            receipt.decision().phase(),
            &ProductDecisionPhaseV1::Approved
        );
        assert_eq!(receipt.decision().revision().get(), 3);
        assert_eq!(receipt.source_drain_selector(), &drain_selector());
        assert_eq!(receipt.resulting_runtime_deployment_revision().get(), 11);
        assert_eq!(receipt.terminal_intent_revision().get(), 8);
        assert_eq!(receipt.terminal_state_digest(), "d".repeat(64));
        assert_eq!(receipt.source_slot_writer_epoch().get(), 20);
        assert_eq!(receipt.successor_slot_writer_epoch().get(), 21);
        assert_eq!(
            receipt.cancelled_at(),
            UNIX_EPOCH + Duration::from_secs(100)
        );
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["authenticate_mutation", "authorize", "cancel_lifecycle"]
        );
    });
}

#[test]
fn cancellation_accepts_exact_replay_and_revalidates_its_complete_projection() {
    assert!(cancellation_result(ReceiptFault::Replay)
        .unwrap()
        .exact_replay());
    assert_eq!(
        cancellation_result(ReceiptFault::ReplayDrain).unwrap_err(),
        ProductApplicationError::InvalidProjection
    );
}

#[test]
fn cancellation_rejects_invalid_product_deployment_drain_and_epoch_projections() {
    for fault in [
        ReceiptFault::ProductScope,
        ReceiptFault::ProductPhase,
        ReceiptFault::ProductRevision,
        ReceiptFault::Drain,
        ReceiptFault::DeploymentRevision,
        ReceiptFault::DrainRevision,
        ReceiptFault::Epoch,
    ] {
        assert_eq!(
            cancellation_result(fault).unwrap_err(),
            ProductApplicationError::InvalidProjection
        );
    }
}

#[test]
fn cancellation_rejects_invalid_csrf_before_authority_or_persistence() {
    block_on(async {
        let events = Arc::new(Mutex::new(Vec::new()));
        let authentication = Authentication {
            events: events.clone(),
        };
        let authority = Authority {
            events: events.clone(),
        };
        let cancellations = CancellationPort {
            events: events.clone(),
            fault: ReceiptFault::None,
        };
        let error =
            ProductControlApplication::new(&authentication, &authority, &cancellations, &())
                .cancel_product_lifecycle(
                    "session",
                    "wrong",
                    &request_id(),
                    &installation(),
                    command(),
                )
                .await
                .unwrap_err();
        assert_eq!(
            error,
            ProductApplicationError::Authentication(AuthenticationError::InvalidCsrf)
        );
        assert_eq!(events.lock().unwrap().as_slice(), ["authenticate_mutation"]);
    });
}

#[test]
fn structured_drain_pending_and_cancelled_surfaces_preserve_the_selector() {
    let selector = drain_selector();
    assert_eq!(
        ProductControlPortError::RuntimeDrainPending(selector.clone()),
        ProductControlPortError::RuntimeDrainPending(selector.clone())
    );
    assert_eq!(
        ProductControlPortError::LifecycleCancelled(selector.clone()),
        ProductControlPortError::LifecycleCancelled(selector)
    );
    assert_eq!(
        ProductControlPortError::RuntimeDrainRequired.to_string(),
        "runtime drain is required before product apply can continue"
    );
}
