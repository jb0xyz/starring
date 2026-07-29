use std::collections::VecDeque;
use std::num::NonZeroU64;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use authoring_application::{
    ApplyProductPromotionV1, ApprovalPayloadDigestV1, ApproveProductPromotionV1,
    AuthenticatedActorV1, AuthenticatedSessionFingerprintV1, AuthenticationClaimsV1,
    AuthenticationError, AuthenticationPort, AuthorizedApplyProductV1, AuthorizedApprovalPreviewV1,
    AuthorizedApproveProductV1, AuthorizedCancelProductLifecycleV1, AuthorizedDeploymentStatusV1,
    AuthorizedProductStatusV1, AuthorizedRejectProductV1, CancelProductLifecycleMutationV1,
    CapabilityV1, DeploymentStatusPort, DeploymentStatusPortError, DeploymentStatusProjectionV1,
    FreshGuildAuthorityError, InstallationSelectorV1, MutationAuthenticationPort,
    ProductApplicationError, ProductApplyPort, ProductApprovalPort, ProductApprovalPreviewV1,
    ProductControlApplication, ProductControlPortError, ProductDecisionPhaseV1,
    ProductDecisionProjectionV1, ProductDecisionQueryPort, ProductDrainSelectorV1,
    ProductIdempotencyKeyV1, ProductLifecycleCancellationPort,
    ProductLifecycleCancellationReasonV1, ProductLifecycleCancellationReceiptV1,
    ProductMutationReceiptV1, ProductRejectionPort, ProductRequestIdV1, ProductRevisionV1,
    ProductStatusQueryV1, ProductStatusV1, PromotionSelectorV1, RejectProductPromotionV1,
    RejectionReasonV1,
};
use authoring_application_discord::{
    AuthorityClock, DiscordApplicationIdV1, DiscordAuthorityClientError, DiscordAuthorityConfigV1,
    DiscordAuthoritySourceError, DiscordBotUserIdV1, DiscordGuildApplyAuthoritySnapshotV1,
    DiscordGuildAuthorityAdapter, DiscordGuildAuthorityClient, DiscordGuildAuthoritySnapshotV1,
    DiscordRoleSnapshotV1, FreshDiscordAuthorityEvidenceV1, InstallationAuthorityRecordV1,
    InstallationAuthoritySource,
};
use authoring_promotion::{AutomationInstallationId, PrincipalId, PromotionId, TenantId};
use chrono::{TimeZone, Utc};
use discord_model::{GuildId, Permissions, RoleId, UserId};

#[derive(Clone)]
struct FixedClock(chrono::DateTime<Utc>);

impl AuthorityClock for FixedClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        self.0
    }
}

#[derive(Clone)]
struct SequenceClock {
    times: Arc<Mutex<VecDeque<chrono::DateTime<Utc>>>>,
}

impl SequenceClock {
    fn new(times: impl IntoIterator<Item = chrono::DateTime<Utc>>) -> Self {
        Self {
            times: Arc::new(Mutex::new(times.into_iter().collect())),
        }
    }
}

impl AuthorityClock for SequenceClock {
    fn now(&self) -> chrono::DateTime<Utc> {
        self.times
            .lock()
            .unwrap()
            .pop_front()
            .expect("authority clock sequence must contain enough values")
    }
}

struct Authentication;

impl AuthenticationPort for Authentication {
    type Credential = str;

    async fn authenticate(
        &self,
        credential: &str,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        if credential != "valid-credential" {
            return Err(AuthenticationError::InvalidCredential);
        }
        Ok(AuthenticationClaimsV1::from_authentication(
            PrincipalId::parse("principal-1").unwrap(),
            AuthenticatedSessionFingerprintV1::from_sha256_digest([9_u8; 32]),
        ))
    }
}

impl MutationAuthenticationPort for Authentication {
    type CsrfProof = str;

    async fn authenticate_mutation(
        &self,
        credential: &str,
        csrf: &str,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        if csrf != "valid-csrf" {
            return Err(AuthenticationError::InvalidCsrf);
        }
        self.authenticate(credential).await
    }
}

#[derive(Clone)]
struct Source {
    result: Result<InstallationAuthorityRecordV1, DiscordAuthoritySourceError>,
    calls: Arc<Mutex<usize>>,
}

impl InstallationAuthoritySource for Source {
    async fn load_for_actor(
        &self,
        actor: &AuthenticatedActorV1,
        installation: &InstallationSelectorV1,
    ) -> Result<InstallationAuthorityRecordV1, DiscordAuthoritySourceError> {
        assert_eq!(actor.principal_id().as_str(), "principal-1");
        assert_eq!(actor.session_fingerprint().as_bytes(), &[9_u8; 32]);
        assert_eq!(installation.installation_id().as_str(), "install-1");
        self.calls.lock().map(|mut calls| *calls += 1).unwrap();
        self.result.clone()
    }
}

#[derive(Clone)]
struct Client {
    application_id: DiscordApplicationIdV1,
    result: Result<DiscordGuildAuthoritySnapshotV1, DiscordAuthorityClientError>,
    calls: Arc<Mutex<usize>>,
}

impl DiscordGuildAuthorityClient for Client {
    fn application_id(&self) -> DiscordApplicationIdV1 {
        self.application_id
    }

    async fn fetch_authority_snapshot(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> Result<DiscordGuildAuthoritySnapshotV1, DiscordAuthorityClientError> {
        assert_eq!(guild_id, GuildId(10));
        assert_eq!(user_id, UserId(20));
        self.calls.lock().map(|mut calls| *calls += 1).unwrap();
        self.result.clone()
    }
}

#[derive(Clone)]
struct RuntimeClient {
    application_id: DiscordApplicationIdV1,
    bot_user_id: Option<DiscordBotUserIdV1>,
    authority_result: Result<DiscordGuildAuthoritySnapshotV1, DiscordAuthorityClientError>,
    apply_result: Result<DiscordGuildApplyAuthoritySnapshotV1, DiscordAuthorityClientError>,
    authority_calls: Arc<Mutex<usize>>,
    apply_calls: Arc<Mutex<usize>>,
}

impl DiscordGuildAuthorityClient for RuntimeClient {
    fn application_id(&self) -> DiscordApplicationIdV1 {
        self.application_id
    }

    fn bot_user_id(&self) -> Option<DiscordBotUserIdV1> {
        self.bot_user_id
    }

    async fn fetch_authority_snapshot(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> Result<DiscordGuildAuthoritySnapshotV1, DiscordAuthorityClientError> {
        assert_eq!(guild_id, GuildId(10));
        assert_eq!(user_id, UserId(20));
        self.authority_calls
            .lock()
            .map(|mut calls| *calls += 1)
            .unwrap();
        self.authority_result.clone()
    }

    async fn fetch_apply_authority_snapshot(
        &self,
        guild_id: GuildId,
        user_id: UserId,
    ) -> Result<DiscordGuildApplyAuthoritySnapshotV1, DiscordAuthorityClientError> {
        assert_eq!(guild_id, GuildId(10));
        assert_eq!(user_id, UserId(20));
        self.apply_calls
            .lock()
            .map(|mut calls| *calls += 1)
            .unwrap();
        self.apply_result.clone()
    }
}

struct InspectingApplyDecisions {
    evidence: Arc<Mutex<Option<FreshDiscordAuthorityEvidenceV1>>>,
}

impl ProductApplyPort<FreshDiscordAuthorityEvidenceV1> for InspectingApplyDecisions {
    async fn apply_idempotent(
        &self,
        request: AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        self.evidence
            .lock()
            .unwrap()
            .replace(request.evidence().clone());
        Err(ProductControlPortError::Backend(
            "inspection_complete".to_string(),
        ))
    }
}

struct InspectingLifecycleCancellations {
    evidence: Arc<Mutex<Option<FreshDiscordAuthorityEvidenceV1>>>,
}

impl ProductLifecycleCancellationPort<FreshDiscordAuthorityEvidenceV1>
    for InspectingLifecycleCancellations
{
    async fn cancel_lifecycle_idempotent(
        &self,
        request: AuthorizedCancelProductLifecycleV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductLifecycleCancellationReceiptV1, ProductControlPortError> {
        self.evidence
            .lock()
            .unwrap()
            .replace(request.evidence().clone());
        Err(ProductControlPortError::Backend(
            "inspection_complete".to_string(),
        ))
    }
}

struct Decisions;

impl ProductDecisionQueryPort<FreshDiscordAuthorityEvidenceV1> for Decisions {
    async fn load_approval_preview(
        &self,
        _request: AuthorizedApprovalPreviewV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductApprovalPreviewV1, ProductControlPortError> {
        unreachable!()
    }

    async fn load_product_status(
        &self,
        request: AuthorizedProductStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductDecisionProjectionV1, ProductControlPortError> {
        assert_eq!(request.evidence().capability(), CapabilityV1::Read);
        assert!(request
            .evidence()
            .is_fresh_at(Utc.with_ymd_and_hms(2026, 7, 19, 0, 0, 1).unwrap()));
        Ok(ProductDecisionProjectionV1::from_server_projection(
            request.scope().tenant_id().clone(),
            request.scope().installation_id().clone(),
            request.scope().guild_id(),
            request.promotion().promotion_id().clone(),
            ProductRevisionV1::new(1).unwrap(),
            ProductDecisionPhaseV1::PendingApproval,
        ))
    }
}

impl ProductApprovalPort<FreshDiscordAuthorityEvidenceV1> for Decisions {
    async fn approve_payload_bound(
        &self,
        _request: AuthorizedApproveProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        unreachable!()
    }
}

impl ProductRejectionPort<FreshDiscordAuthorityEvidenceV1> for Decisions {
    async fn reject_payload_bound(
        &self,
        _request: AuthorizedRejectProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        unreachable!()
    }
}

impl ProductApplyPort<FreshDiscordAuthorityEvidenceV1> for Decisions {
    async fn apply_idempotent(
        &self,
        _request: AuthorizedApplyProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        unreachable!()
    }
}

struct Deployments;

impl DeploymentStatusPort<FreshDiscordAuthorityEvidenceV1> for Deployments {
    async fn load_exact_deployment_status(
        &self,
        _request: AuthorizedDeploymentStatusV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<DeploymentStatusProjectionV1, DeploymentStatusPortError> {
        unreachable!()
    }
}

fn app_id() -> DiscordApplicationIdV1 {
    DiscordApplicationIdV1::new(99).unwrap()
}

fn record() -> InstallationAuthorityRecordV1 {
    InstallationAuthorityRecordV1 {
        tenant_id: TenantId::parse("tenant-1").unwrap(),
        installation_id: AutomationInstallationId::parse("install-1").unwrap(),
        application_id: app_id(),
        guild_id: GuildId(10),
        acting_user_id: UserId(20),
        authority_revision: NonZeroU64::new(3).unwrap(),
        authority_digest: "a".repeat(64),
    }
}

#[test]
fn installation_authority_record_debug_is_redacted() {
    let rendered = format!("{:?}", record());
    assert_eq!(rendered, "InstallationAuthorityRecordV1(<redacted>)");
    assert!(!rendered.contains("tenant-1"));
    assert!(!rendered.contains("install-1"));
    assert!(!rendered.contains(&"a".repeat(64)));
}

fn snapshot(permissions: Permissions) -> DiscordGuildAuthoritySnapshotV1 {
    DiscordGuildAuthoritySnapshotV1 {
        guild_id: GuildId(10),
        owner_id: UserId(21),
        member_user_id: UserId(20),
        member_is_bot: false,
        member_is_system: false,
        member_pending: false,
        member_role_ids: vec![RoleId(11)],
        roles: vec![
            DiscordRoleSnapshotV1 {
                role_id: RoleId(10),
                permissions: Permissions::VIEW_CHANNEL,
                position: 0,
                managed: false,
            },
            DiscordRoleSnapshotV1 {
                role_id: RoleId(11),
                permissions,
                position: 10,
                managed: false,
            },
        ],
    }
}

fn bot_id() -> DiscordBotUserIdV1 {
    DiscordBotUserIdV1::new(777).unwrap()
}

fn apply_snapshot(bot_permissions: Permissions) -> DiscordGuildApplyAuthoritySnapshotV1 {
    let mut authority = snapshot(Permissions::MANAGE_GUILD);
    authority.roles.push(DiscordRoleSnapshotV1 {
        role_id: RoleId(12),
        permissions: bot_permissions,
        position: 20,
        managed: true,
    });
    DiscordGuildApplyAuthoritySnapshotV1 {
        authority,
        bot_member_user_id: bot_id().to_user_id(),
        bot_member_is_bot: true,
        bot_member_is_system: false,
        bot_member_pending: false,
        bot_member_role_ids: vec![RoleId(12)],
    }
}

fn runtime_client(
    apply_result: Result<DiscordGuildApplyAuthoritySnapshotV1, DiscordAuthorityClientError>,
) -> RuntimeClient {
    RuntimeClient {
        application_id: app_id(),
        bot_user_id: Some(bot_id()),
        authority_result: Ok(snapshot(Permissions::MANAGE_GUILD)),
        apply_result,
        authority_calls: Arc::new(Mutex::new(0)),
        apply_calls: Arc::new(Mutex::new(0)),
    }
}

fn runtime_adapter(
    client: RuntimeClient,
) -> DiscordGuildAuthorityAdapter<Source, RuntimeClient, FixedClock> {
    DiscordGuildAuthorityAdapter::with_clock(
        Source {
            result: Ok(record()),
            calls: Arc::new(Mutex::new(0)),
        },
        client,
        FixedClock(Utc.with_ymd_and_hms(2026, 7, 19, 0, 0, 0).unwrap()),
        DiscordAuthorityConfigV1::default(),
    )
}

fn adapter(
    record: InstallationAuthorityRecordV1,
    snapshot: DiscordGuildAuthoritySnapshotV1,
) -> DiscordGuildAuthorityAdapter<Source, Client, FixedClock> {
    DiscordGuildAuthorityAdapter::with_clock(
        Source {
            result: Ok(record),
            calls: Arc::new(Mutex::new(0)),
        },
        Client {
            application_id: app_id(),
            result: Ok(snapshot),
            calls: Arc::new(Mutex::new(0)),
        },
        FixedClock(Utc.with_ymd_and_hms(2026, 7, 19, 0, 0, 0).unwrap()),
        DiscordAuthorityConfigV1::default(),
    )
}

fn installation() -> InstallationSelectorV1 {
    InstallationSelectorV1::new(AutomationInstallationId::parse("install-1").unwrap())
}

fn query() -> ProductStatusQueryV1 {
    ProductStatusQueryV1 {
        promotion: PromotionSelectorV1::new(PromotionId::parse(&"b".repeat(64)).unwrap()),
    }
}

fn apply_command() -> ApplyProductPromotionV1 {
    ApplyProductPromotionV1 {
        promotion: PromotionSelectorV1::new(PromotionId::parse(&"b".repeat(64)).unwrap()),
        expected_payload_digest: ApprovalPayloadDigestV1::parse(&"c".repeat(64)).unwrap(),
        expected_revision: ProductRevisionV1::new(1).unwrap(),
        idempotency_key: ProductIdempotencyKeyV1::parse("apply-request-1").unwrap(),
    }
}

fn cancellation_command() -> CancelProductLifecycleMutationV1 {
    CancelProductLifecycleMutationV1 {
        promotion: PromotionSelectorV1::new(PromotionId::parse(&"b".repeat(64)).unwrap()),
        expected_payload_digest: ApprovalPayloadDigestV1::parse(&"c".repeat(64)).unwrap(),
        expected_revision: ProductRevisionV1::new(1).unwrap(),
        drain_selector: ProductDrainSelectorV1::from_server_projection(
            "d".repeat(32),
            7,
            "e".repeat(64),
            "f".repeat(32),
            10,
        )
        .unwrap(),
        idempotency_key: ProductIdempotencyKeyV1::parse("cancel-request-1").unwrap(),
        reason: ProductLifecycleCancellationReasonV1::parse("retain the current deployment")
            .unwrap(),
    }
}

async fn capture_apply_evidence<K>(
    adapter: &DiscordGuildAuthorityAdapter<Source, RuntimeClient, K>,
) -> Result<FreshDiscordAuthorityEvidenceV1, ProductApplicationError>
where
    K: AuthorityClock,
{
    let captured = Arc::new(Mutex::new(None));
    let decisions = InspectingApplyDecisions {
        evidence: captured.clone(),
    };
    let result = ProductControlApplication::new(&Authentication, adapter, &decisions, &Deployments)
        .apply(
            "valid-credential",
            "valid-csrf",
            &ProductRequestIdV1::parse("request-1").unwrap(),
            &installation(),
            apply_command(),
        )
        .await;
    match result {
        Err(ProductApplicationError::Control(ProductControlPortError::Backend(code)))
            if code == "inspection_complete" => {}
        Err(error) => return Err(error),
        Ok(_) => panic!("inspection decision must stop apply"),
    }
    let evidence = captured
        .lock()
        .unwrap()
        .take()
        .expect("apply evidence must be captured");
    Ok(evidence)
}

async fn capture_cancellation_evidence<K>(
    adapter: &DiscordGuildAuthorityAdapter<Source, RuntimeClient, K>,
) -> Result<FreshDiscordAuthorityEvidenceV1, ProductApplicationError>
where
    K: AuthorityClock,
{
    let captured = Arc::new(Mutex::new(None));
    let cancellations = InspectingLifecycleCancellations {
        evidence: captured.clone(),
    };
    let result =
        ProductControlApplication::new(&Authentication, adapter, &cancellations, &Deployments)
            .cancel_product_lifecycle(
                "valid-credential",
                "valid-csrf",
                &ProductRequestIdV1::parse("request-1").unwrap(),
                &installation(),
                cancellation_command(),
            )
            .await;
    match result {
        Err(ProductApplicationError::Control(ProductControlPortError::Backend(code)))
            if code == "inspection_complete" => {}
        Err(error) => return Err(error),
        Ok(_) => panic!("inspection cancellation must stop persistence"),
    }
    let evidence = captured
        .lock()
        .unwrap()
        .take()
        .expect("cancellation evidence must be captured");
    Ok(evidence)
}

async fn status(
    adapter: &DiscordGuildAuthorityAdapter<Source, Client, FixedClock>,
) -> Result<ProductStatusV1, ProductApplicationError> {
    ProductControlApplication::new(&Authentication, adapter, &Decisions, &Deployments)
        .get_product_status("valid-credential", &installation(), query())
        .await
}

#[tokio::test]
async fn manager_is_authorized_with_exact_server_derived_evidence() {
    let status = status(&adapter(record(), snapshot(Permissions::MANAGE_GUILD)))
        .await
        .unwrap();
    assert_eq!(status, ProductStatusV1::PendingApproval);
}

#[tokio::test]
async fn owner_is_authorized_without_manager_role() {
    let mut snapshot = snapshot(Permissions::empty());
    snapshot.owner_id = UserId(20);
    assert_eq!(
        status(&adapter(record(), snapshot)).await.unwrap(),
        ProductStatusV1::PendingApproval
    );
}

#[tokio::test]
async fn administrator_is_authorized() {
    assert_eq!(
        status(&adapter(record(), snapshot(Permissions::ADMINISTRATOR)))
            .await
            .unwrap(),
        ProductStatusV1::PendingApproval
    );
}

#[tokio::test]
async fn ordinary_member_is_denied() {
    let error = status(&adapter(record(), snapshot(Permissions::VIEW_CHANNEL)))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Forbidden)
    );
}

#[tokio::test]
async fn mismatched_application_fails_before_discord_fetch() {
    let calls = Arc::new(Mutex::new(0));
    let adapter = DiscordGuildAuthorityAdapter::with_clock(
        Source {
            result: Ok(record()),
            calls: Arc::new(Mutex::new(0)),
        },
        Client {
            application_id: DiscordApplicationIdV1::new(100).unwrap(),
            result: Ok(snapshot(Permissions::MANAGE_GUILD)),
            calls: calls.clone(),
        },
        FixedClock(Utc.with_ymd_and_hms(2026, 7, 19, 0, 0, 0).unwrap()),
        DiscordAuthorityConfigV1::default(),
    );
    let error = status(&adapter).await.unwrap_err();
    assert_eq!(
        error,
        ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::ScopeMismatch)
    );
    assert_eq!(*calls.lock().unwrap(), 0);
}

#[tokio::test]
async fn missing_member_role_fails_closed() {
    let mut snapshot = snapshot(Permissions::MANAGE_GUILD);
    snapshot.member_role_ids = vec![RoleId(12)];
    let error = status(&adapter(record(), snapshot)).await.unwrap_err();
    assert!(matches!(
        error,
        ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Backend(_))
    ));
}

#[tokio::test]
async fn timeout_fails_as_stale() {
    #[derive(Clone)]
    struct SlowClient;

    impl DiscordGuildAuthorityClient for SlowClient {
        fn application_id(&self) -> DiscordApplicationIdV1 {
            app_id()
        }

        async fn fetch_authority_snapshot(
            &self,
            _guild_id: GuildId,
            _user_id: UserId,
        ) -> Result<DiscordGuildAuthoritySnapshotV1, DiscordAuthorityClientError> {
            tokio::time::sleep(Duration::from_millis(25)).await;
            Ok(snapshot(Permissions::MANAGE_GUILD))
        }
    }

    let adapter = DiscordGuildAuthorityAdapter::with_clock(
        Source {
            result: Ok(record()),
            calls: Arc::new(Mutex::new(0)),
        },
        SlowClient,
        FixedClock(Utc.with_ymd_and_hms(2026, 7, 19, 0, 0, 0).unwrap()),
        DiscordAuthorityConfigV1::new(
            Duration::from_millis(1),
            Duration::from_secs(5),
            Duration::from_secs(30),
        )
        .unwrap(),
    );
    let error = ProductControlApplication::new(&Authentication, &adapter, &Decisions, &Deployments)
        .get_product_status("valid-credential", &installation(), query())
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Stale)
    );
}

#[tokio::test]
async fn write_authority_window_starts_before_the_discord_fetch() {
    let started_at = Utc.with_ymd_and_hms(2026, 7, 19, 0, 0, 0).unwrap();
    let completed_at = started_at + chrono::Duration::seconds(5);
    let client = runtime_client(Ok(apply_snapshot(Permissions::MANAGE_ROLES)));
    let apply_calls = client.apply_calls.clone();
    let adapter = DiscordGuildAuthorityAdapter::with_clock(
        Source {
            result: Ok(record()),
            calls: Arc::new(Mutex::new(0)),
        },
        client,
        SequenceClock::new([started_at, completed_at]),
        DiscordAuthorityConfigV1::default(),
    );

    let error = capture_apply_evidence(&adapter).await.unwrap_err();

    assert_eq!(
        error,
        ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Stale)
    );
    assert_eq!(*apply_calls.lock().unwrap(), 1);
}

#[tokio::test]
async fn apply_captures_canonical_runtime_environment_in_the_bounded_observation() {
    let mut snapshot = apply_snapshot(
        Permissions::VIEW_CHANNEL | Permissions::MANAGE_ROLES | Permissions::MANAGE_CHANNELS,
    );
    snapshot.authority.roles.push(DiscordRoleSnapshotV1 {
        role_id: RoleId(13),
        permissions: Permissions::SEND_MESSAGES,
        position: 19,
        managed: false,
    });
    snapshot.bot_member_role_ids = vec![RoleId(13), RoleId(12)];
    let client = runtime_client(Ok(snapshot));
    let authority_calls = client.authority_calls.clone();
    let apply_calls = client.apply_calls.clone();
    let evidence = capture_apply_evidence(&runtime_adapter(client))
        .await
        .unwrap();
    let environment = evidence.apply_runtime_environment().unwrap();
    assert_eq!(evidence.capability(), CapabilityV1::Apply);
    assert_eq!(environment.guild_id(), GuildId(10));
    assert_eq!(environment.bot_user_id(), bot_id());
    assert_eq!(environment.bot_role_ids(), &[RoleId(12), RoleId(13)]);
    assert_eq!(environment.guild_role_permissions().len(), 4);
    let bot_role = environment.guild_roles().get(&RoleId(12)).unwrap();
    assert_eq!(bot_role.position, 20);
    assert!(bot_role.managed);
    assert_eq!(
        environment
            .guild_role_permissions()
            .get(&RoleId(12))
            .copied(),
        Some(Permissions::VIEW_CHANNEL | Permissions::MANAGE_ROLES | Permissions::MANAGE_CHANNELS)
    );
    assert_eq!(*authority_calls.lock().unwrap(), 0);
    assert_eq!(*apply_calls.lock().unwrap(), 1);
    assert_eq!(
        evidence.observation_digest(),
        "a71fdf2dd8713c2aa44216d6850e07047c0327a10f1f1e73cbb8b0bc2d7000ee"
    );
    let environment_debug = format!("{environment:?}");
    let evidence_debug = format!("{evidence:?}");
    for sensitive in ["777", "permissions: ", evidence.observation_digest()] {
        assert!(!environment_debug.contains(sensitive));
        assert!(!evidence_debug.contains(sensitive));
    }
}

#[tokio::test]
async fn cancel_lifecycle_captures_full_runtime_environment_with_write_lifetime() {
    let mut snapshot = apply_snapshot(
        Permissions::VIEW_CHANNEL | Permissions::MANAGE_ROLES | Permissions::MANAGE_CHANNELS,
    );
    snapshot.authority.roles.push(DiscordRoleSnapshotV1 {
        role_id: RoleId(13),
        permissions: Permissions::SEND_MESSAGES,
        position: 19,
        managed: false,
    });
    snapshot.bot_member_role_ids = vec![RoleId(13), RoleId(12)];
    let client = runtime_client(Ok(snapshot));
    let authority_calls = client.authority_calls.clone();
    let apply_calls = client.apply_calls.clone();
    let evidence = capture_cancellation_evidence(&runtime_adapter(client))
        .await
        .unwrap();
    let environment = evidence.runtime_environment().unwrap();
    assert_eq!(evidence.capability(), CapabilityV1::CancelLifecycle);
    assert_eq!(
        evidence.expires_at() - evidence.observed_at(),
        chrono::Duration::seconds(5)
    );
    assert_eq!(evidence.apply_runtime_environment(), Some(environment));
    assert_eq!(environment.guild_id(), GuildId(10));
    assert_eq!(environment.bot_user_id(), bot_id());
    assert_eq!(environment.bot_role_ids(), &[RoleId(12), RoleId(13)]);
    assert_eq!(environment.guild_role_permissions().len(), 4);
    let bot_role = environment.guild_roles().get(&RoleId(12)).unwrap();
    assert_eq!(bot_role.position, 20);
    assert!(bot_role.managed);
    assert_eq!(
        environment
            .guild_role_permissions()
            .get(&RoleId(12))
            .copied(),
        Some(Permissions::VIEW_CHANNEL | Permissions::MANAGE_ROLES | Permissions::MANAGE_CHANNELS)
    );
    assert_eq!(*authority_calls.lock().unwrap(), 0);
    assert_eq!(*apply_calls.lock().unwrap(), 1);
    assert_eq!(
        evidence.observation_digest(),
        "79b4ee87069a5a135f125e1bba24469faa2b62f28df94d7a3aa69bc5604b90b4"
    );
}

#[tokio::test]
async fn cancel_lifecycle_and_apply_use_distinct_authority_digest_domains() {
    let snapshot = apply_snapshot(Permissions::MANAGE_ROLES);
    let apply = capture_apply_evidence(&runtime_adapter(runtime_client(Ok(snapshot.clone()))))
        .await
        .unwrap();
    let cancellation =
        capture_cancellation_evidence(&runtime_adapter(runtime_client(Ok(snapshot))))
            .await
            .unwrap();
    assert_ne!(
        cancellation.observation_digest(),
        apply.observation_digest()
    );
}

#[tokio::test]
async fn cancel_lifecycle_digest_binds_permissions_hierarchy_and_managed_state() {
    let original = apply_snapshot(Permissions::MANAGE_ROLES);
    let mut permissions = original.clone();
    permissions
        .authority
        .roles
        .iter_mut()
        .find(|role| role.role_id == RoleId(12))
        .unwrap()
        .permissions = Permissions::MANAGE_CHANNELS;
    let mut hierarchy = original.clone();
    hierarchy
        .authority
        .roles
        .iter_mut()
        .find(|role| role.role_id == RoleId(12))
        .unwrap()
        .position = 21;
    let mut managed = original.clone();
    managed
        .authority
        .roles
        .iter_mut()
        .find(|role| role.role_id == RoleId(12))
        .unwrap()
        .managed = false;
    let original = capture_cancellation_evidence(&runtime_adapter(runtime_client(Ok(original))))
        .await
        .unwrap();
    for changed in [permissions, hierarchy, managed] {
        let changed = capture_cancellation_evidence(&runtime_adapter(runtime_client(Ok(changed))))
            .await
            .unwrap();
        assert_ne!(original.observation_digest(), changed.observation_digest());
    }
}

#[tokio::test]
async fn cancel_lifecycle_never_falls_back_to_the_weaker_authority_snapshot() {
    let client = runtime_client(Err(DiscordAuthorityClientError::Unavailable));
    let authority_calls = client.authority_calls.clone();
    let apply_calls = client.apply_calls.clone();
    let error = capture_cancellation_evidence(&runtime_adapter(client))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Backend(
            "discord_authority_unavailable".to_string()
        ))
    );
    assert_eq!(*authority_calls.lock().unwrap(), 0);
    assert_eq!(*apply_calls.lock().unwrap(), 1);
}

#[tokio::test]
async fn cancel_lifecycle_runtime_identity_and_snapshot_fail_closed() {
    let mut missing_identity = runtime_client(Ok(apply_snapshot(Permissions::MANAGE_ROLES)));
    missing_identity.bot_user_id = None;
    let missing_identity_error = capture_cancellation_evidence(&runtime_adapter(missing_identity))
        .await
        .unwrap_err();
    assert_eq!(
        missing_identity_error,
        ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Backend(
            "discord_apply_bot_identity_unavailable".to_string()
        ))
    );

    let mut mismatched_identity = apply_snapshot(Permissions::MANAGE_ROLES);
    mismatched_identity.bot_member_user_id = UserId(778);
    let mismatch_error =
        capture_cancellation_evidence(&runtime_adapter(runtime_client(Ok(mismatched_identity))))
            .await
            .unwrap_err();
    assert_eq!(
        mismatch_error,
        ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::ScopeMismatch)
    );

    let mut invalid_member = apply_snapshot(Permissions::MANAGE_ROLES);
    invalid_member.bot_member_pending = true;
    let invalid_member_error =
        capture_cancellation_evidence(&runtime_adapter(runtime_client(Ok(invalid_member))))
            .await
            .unwrap_err();
    assert_eq!(
        invalid_member_error,
        ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Backend(
            "discord_apply_bot_member_invalid".to_string()
        ))
    );

    let mut duplicate_roles = apply_snapshot(Permissions::MANAGE_ROLES);
    duplicate_roles.bot_member_role_ids = vec![RoleId(12), RoleId(12)];
    let duplicate_roles_error =
        capture_cancellation_evidence(&runtime_adapter(runtime_client(Ok(duplicate_roles))))
            .await
            .unwrap_err();
    assert_eq!(
        duplicate_roles_error,
        ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Backend(
            "discord_authority_duplicate_member_role".to_string()
        ))
    );
}

#[tokio::test]
async fn cancel_lifecycle_write_window_starts_before_the_discord_fetch() {
    let started_at = Utc.with_ymd_and_hms(2026, 7, 19, 0, 0, 0).unwrap();
    let completed_at = started_at + chrono::Duration::seconds(5);
    let client = runtime_client(Ok(apply_snapshot(Permissions::MANAGE_ROLES)));
    let apply_calls = client.apply_calls.clone();
    let adapter = DiscordGuildAuthorityAdapter::with_clock(
        Source {
            result: Ok(record()),
            calls: Arc::new(Mutex::new(0)),
        },
        client,
        SequenceClock::new([started_at, completed_at]),
        DiscordAuthorityConfigV1::default(),
    );
    let error = capture_cancellation_evidence(&adapter).await.unwrap_err();
    assert_eq!(
        error,
        ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Stale)
    );
    assert_eq!(*apply_calls.lock().unwrap(), 1);
}

#[tokio::test]
async fn non_apply_authority_never_fetches_the_bot_member() {
    let client = runtime_client(Err(DiscordAuthorityClientError::Unavailable));
    let authority_calls = client.authority_calls.clone();
    let apply_calls = client.apply_calls.clone();
    let result = ProductControlApplication::new(
        &Authentication,
        &runtime_adapter(client),
        &Decisions,
        &Deployments,
    )
    .get_product_status("valid-credential", &installation(), query())
    .await;
    assert_eq!(result.unwrap(), ProductStatusV1::PendingApproval);
    assert_eq!(*authority_calls.lock().unwrap(), 1);
    assert_eq!(*apply_calls.lock().unwrap(), 0);
}

#[tokio::test]
async fn apply_observation_digest_binds_bot_roles_and_guild_permissions() {
    let first = capture_apply_evidence(&runtime_adapter(runtime_client(Ok(apply_snapshot(
        Permissions::MANAGE_ROLES,
    )))))
    .await
    .unwrap();
    let second = capture_apply_evidence(&runtime_adapter(runtime_client(Ok(apply_snapshot(
        Permissions::MANAGE_CHANNELS,
    )))))
    .await
    .unwrap();
    assert_ne!(first.observation_digest(), second.observation_digest());
}

#[tokio::test]
async fn apply_observation_digest_binds_role_hierarchy_and_managed_state() {
    let original = apply_snapshot(Permissions::MANAGE_ROLES);
    let mut moved = original.clone();
    moved
        .authority
        .roles
        .iter_mut()
        .find(|role| role.role_id == RoleId(12))
        .unwrap()
        .position = 21;
    let mut assignable = original.clone();
    assignable
        .authority
        .roles
        .iter_mut()
        .find(|role| role.role_id == RoleId(12))
        .unwrap()
        .managed = false;

    let original = capture_apply_evidence(&runtime_adapter(runtime_client(Ok(original))))
        .await
        .unwrap();
    let moved = capture_apply_evidence(&runtime_adapter(runtime_client(Ok(moved))))
        .await
        .unwrap();
    let assignable = capture_apply_evidence(&runtime_adapter(runtime_client(Ok(assignable))))
        .await
        .unwrap();

    assert_ne!(original.observation_digest(), moved.observation_digest());
    assert_ne!(
        original.observation_digest(),
        assignable.observation_digest()
    );
}

#[tokio::test]
async fn apply_requires_an_explicit_bot_user_identity() {
    let mut client = runtime_client(Ok(apply_snapshot(Permissions::MANAGE_ROLES)));
    client.bot_user_id = None;
    let apply_calls = client.apply_calls.clone();
    let error = capture_apply_evidence(&runtime_adapter(client))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Backend(
            "discord_apply_bot_identity_unavailable".to_string()
        ))
    );
    assert_eq!(*apply_calls.lock().unwrap(), 0);
}

#[tokio::test]
async fn apply_never_derives_bot_user_identity_from_application_identity() {
    let mut snapshot = apply_snapshot(Permissions::MANAGE_ROLES);
    snapshot.bot_member_user_id = UserId(app_id().get());
    let error = capture_apply_evidence(&runtime_adapter(runtime_client(Ok(snapshot))))
        .await
        .unwrap_err();
    assert_eq!(
        error,
        ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::ScopeMismatch)
    );
}

#[tokio::test]
async fn invalid_bot_member_flags_fail_closed() {
    let mut not_bot = apply_snapshot(Permissions::MANAGE_ROLES);
    not_bot.bot_member_is_bot = false;
    let mut system = apply_snapshot(Permissions::MANAGE_ROLES);
    system.bot_member_is_system = true;
    let mut pending = apply_snapshot(Permissions::MANAGE_ROLES);
    pending.bot_member_pending = true;
    for snapshot in [not_bot, system, pending] {
        let error = capture_apply_evidence(&runtime_adapter(runtime_client(Ok(snapshot))))
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Backend(
                "discord_apply_bot_member_invalid".to_string()
            ))
        );
    }
}

#[tokio::test]
async fn bot_dependency_failures_never_become_actor_forbidden() {
    for (client_error, expected_code) in [
        (
            DiscordAuthorityClientError::BotCredentialRejected,
            "discord_bot_credential_rejected",
        ),
        (
            DiscordAuthorityClientError::BotInstallationInaccessible,
            "discord_bot_installation_inaccessible",
        ),
        (
            DiscordAuthorityClientError::BotMemberInaccessible,
            "discord_apply_bot_member_inaccessible",
        ),
    ] {
        let error = capture_apply_evidence(&runtime_adapter(runtime_client(Err(client_error))))
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Backend(
                expected_code.to_string()
            ))
        );
    }
}

#[tokio::test]
async fn duplicate_and_invalid_bot_roles_fail_closed() {
    let mut duplicate = apply_snapshot(Permissions::MANAGE_ROLES);
    duplicate.bot_member_role_ids = vec![RoleId(12), RoleId(12)];
    let mut everyone = apply_snapshot(Permissions::MANAGE_ROLES);
    everyone.bot_member_role_ids = vec![RoleId(10)];
    let mut missing = apply_snapshot(Permissions::MANAGE_ROLES);
    missing.bot_member_role_ids = vec![RoleId(14)];
    for snapshot in [duplicate, everyone, missing] {
        let error = capture_apply_evidence(&runtime_adapter(runtime_client(Ok(snapshot))))
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Backend(_))
        ));
    }
}

#[tokio::test]
async fn guild_and_member_role_snapshots_are_strictly_bounded() {
    let mut too_many_guild_roles = apply_snapshot(Permissions::MANAGE_ROLES);
    too_many_guild_roles.authority.roles = (10_u64..=260)
        .map(|role_id| DiscordRoleSnapshotV1 {
            role_id: RoleId(role_id),
            permissions: if role_id == 11 {
                Permissions::MANAGE_GUILD
            } else {
                Permissions::empty()
            },
            position: i64::try_from(role_id - 10).unwrap(),
            managed: false,
        })
        .collect();
    let guild_error =
        capture_apply_evidence(&runtime_adapter(runtime_client(Ok(too_many_guild_roles))))
            .await
            .unwrap_err();
    assert_eq!(
        guild_error,
        ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Backend(
            "discord_authority_role_limit_exceeded".to_string()
        ))
    );

    let mut too_many_bot_roles = apply_snapshot(Permissions::MANAGE_ROLES);
    too_many_bot_roles.bot_member_role_ids = vec![RoleId(12); 251];
    let member_error =
        capture_apply_evidence(&runtime_adapter(runtime_client(Ok(too_many_bot_roles))))
            .await
            .unwrap_err();
    assert_eq!(
        member_error,
        ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Backend(
            "discord_authority_member_role_limit_exceeded".to_string()
        ))
    );
}

#[tokio::test]
async fn duplicate_guild_roles_and_invalid_actor_flags_fail_closed() {
    let mut duplicate = apply_snapshot(Permissions::MANAGE_ROLES);
    duplicate.authority.roles.push(DiscordRoleSnapshotV1 {
        role_id: RoleId(12),
        permissions: Permissions::MANAGE_CHANNELS,
        position: 20,
        managed: true,
    });
    let duplicate_error = capture_apply_evidence(&runtime_adapter(runtime_client(Ok(duplicate))))
        .await
        .unwrap_err();
    assert_eq!(
        duplicate_error,
        ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Backend(
            "discord_authority_duplicate_role".to_string()
        ))
    );

    let mut bot_actor = apply_snapshot(Permissions::MANAGE_ROLES);
    bot_actor.authority.member_is_bot = true;
    let actor_error = capture_apply_evidence(&runtime_adapter(runtime_client(Ok(bot_actor))))
        .await
        .unwrap_err();
    assert_eq!(
        actor_error,
        ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Forbidden)
    );
}

#[tokio::test]
async fn invalid_everyone_role_hierarchy_fails_closed() {
    let mut moved = apply_snapshot(Permissions::MANAGE_ROLES);
    moved
        .authority
        .roles
        .iter_mut()
        .find(|role| role.role_id == RoleId(10))
        .unwrap()
        .position = 1;
    let mut managed = apply_snapshot(Permissions::MANAGE_ROLES);
    managed
        .authority
        .roles
        .iter_mut()
        .find(|role| role.role_id == RoleId(10))
        .unwrap()
        .managed = true;
    for snapshot in [moved, managed] {
        let error = capture_apply_evidence(&runtime_adapter(runtime_client(Ok(snapshot))))
            .await
            .unwrap_err();
        assert_eq!(
            error,
            ProductApplicationError::FreshAuthority(FreshGuildAuthorityError::Backend(
                "discord_authority_invalid_everyone_role".to_string()
            ))
        );
    }
}

#[test]
fn configuration_caps_authority_freshness() {
    assert_eq!(
        DiscordAuthorityConfigV1::new(
            Duration::from_secs(5),
            Duration::from_secs(6),
            Duration::from_secs(30)
        )
        .unwrap_err()
        .to_string(),
        "write authority lifetime must be positive and at most 5 seconds"
    );
}

#[test]
fn authority_types_are_not_client_deserializable() {
    let sources = [
        include_str!("../src/evidence.rs"),
        include_str!("../src/snapshot.rs"),
    ];
    for source in sources {
        assert!(!source.contains("Deserialize"));
    }
    let _ = ApprovalPayloadDigestV1::parse(&"c".repeat(64)).unwrap();
    let _ = ProductIdempotencyKeyV1::parse("request-1").unwrap();
    let _ = RejectionReasonV1::parse("reason").unwrap();
    let _: Option<ApproveProductPromotionV1> = None;
    let _: Option<RejectProductPromotionV1> = None;
    let _: Option<ApplyProductPromotionV1> = None;
}
