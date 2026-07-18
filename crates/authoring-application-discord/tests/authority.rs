use std::num::NonZeroU64;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use authoring_application::{
    ApplyProductPromotionV1, ApprovalPayloadDigestV1, ApproveProductPromotionV1,
    AuthenticatedActorV1, AuthenticatedIdentityV1, AuthenticationError, AuthenticationPort,
    AuthorizedApplyProductV1, AuthorizedApprovalPreviewV1, AuthorizedApproveProductV1,
    AuthorizedDeploymentStatusV1, AuthorizedProductStatusV1, AuthorizedRejectProductV1,
    CapabilityV1, DeploymentStatusPort, DeploymentStatusPortError, DeploymentStatusProjectionV1,
    FreshGuildAuthorityError, InstallationSelectorV1, ProductApplicationError,
    ProductApprovalPreviewV1, ProductControlApplication, ProductControlPortError,
    ProductDecisionPhaseV1, ProductDecisionPort, ProductDecisionProjectionV1,
    ProductIdempotencyKeyV1, ProductMutationReceiptV1, ProductRevisionV1, ProductStatusQueryV1,
    ProductStatusV1, PromotionSelectorV1, RejectProductPromotionV1, RejectionReasonV1,
};
use authoring_application_discord::{
    AuthorityClock, DiscordApplicationIdV1, DiscordAuthorityClientError, DiscordAuthorityConfigV1,
    DiscordAuthoritySourceError, DiscordGuildAuthorityAdapter, DiscordGuildAuthorityClient,
    DiscordGuildAuthoritySnapshotV1, DiscordRoleSnapshotV1, FreshDiscordAuthorityEvidenceV1,
    InstallationAuthorityRecordV1, InstallationAuthoritySource,
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

struct Authentication;

impl AuthenticationPort for Authentication {
    type Credential = str;

    async fn authenticate(
        &self,
        credential: &str,
    ) -> Result<AuthenticatedIdentityV1, AuthenticationError> {
        if credential != "valid-credential" {
            return Err(AuthenticationError::InvalidCredential);
        }
        Ok(AuthenticatedIdentityV1::from_authentication(
            PrincipalId::parse("principal-1").unwrap(),
        ))
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

struct Decisions;

impl ProductDecisionPort<FreshDiscordAuthorityEvidenceV1> for Decisions {
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

    async fn approve_payload_bound(
        &self,
        _request: AuthorizedApproveProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        unreachable!()
    }

    async fn reject_payload_bound(
        &self,
        _request: AuthorizedRejectProductV1<'_, FreshDiscordAuthorityEvidenceV1>,
    ) -> Result<ProductMutationReceiptV1, ProductControlPortError> {
        unreachable!()
    }

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
            },
            DiscordRoleSnapshotV1 {
                role_id: RoleId(11),
                permissions,
            },
        ],
    }
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
