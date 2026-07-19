use std::collections::BTreeMap;
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use automation_ruleset::{
    GuardedActivationOutcome, GuardedRuleSetActivation, InMemoryRuleSetStore, PublishOutcome,
    PublishRuleSetRequest, RuleSetActivation, RuleSetContentHash, RuleSetKey, RuleSetStore,
    RuleSetStoreError, RuleSetVersion, RuleSetVersionId,
};
use automation_ruleset_activation::{
    approval_policy_digest_v1, product_approval_context_digest_v1, ActivationDigest,
    ActivationEnvironment, ActivationEnvironmentError, ActivationEnvironmentProvider,
    ActivationPromotionId, ActivationRequestId, ActivationRequestState, ActivationRequestStore,
    ActivationService, ActivationTarget, ApplyAttemptId, ApplyError, ApplyOutcome,
    ApprovalBindingContextV1, ApprovalPolicyBindingV1, ClaimOutcome, CreateActivationRequest,
    CreateProductActivationRequest, ExpectedActiveBaselineV1, InMemoryActivationRequestStore,
    LinkProductActivation, ManualActivationClock, ProductApprovalContextV1, RecoveryDisposition,
    RequestActivation, SupersessionReasonV1,
};
use automation_ruleset_readiness::{GuildCapabilities, GuildRoleHierarchyV1, GuildRoleStateV1};
use automation_state::{ActionSpec, InteractionRule, InteractionRuleSet, TriggerSpec};
use chrono::{Duration, TimeZone, Utc};
use desired_state::ResourceKey;
use discord_model::{GuildId, Permissions, RoleId, UserId};
use futures::executor::block_on;
use resource_resolution::{
    approval_binding_fingerprint_v1, ResolvedApprovalBinding, ResourceBindingMap,
};

const GUILD: GuildId = GuildId(7);

fn ready_role_hierarchy() -> GuildRoleHierarchyV1 {
    let bot_role = RoleId(70);
    GuildRoleHierarchyV1::new(
        GUILD,
        BTreeMap::from([
            (
                RoleId(GUILD.0),
                GuildRoleStateV1 {
                    position: 0,
                    managed: false,
                },
            ),
            (
                bot_role,
                GuildRoleStateV1 {
                    position: 10,
                    managed: true,
                },
            ),
        ]),
        vec![bot_role],
    )
    .unwrap()
}

fn key() -> RuleSetKey {
    RuleSetKey::parse("studyroom").unwrap()
}

fn definition(create_role: bool) -> InteractionRuleSet {
    let mut actions = vec![ActionSpec::DeferEphemeral];
    if create_role {
        actions.push(ActionSpec::CreateRole {
            key: "member".to_string(),
            name: "member".to_string(),
        });
    }
    actions.push(ActionSpec::EditResponse {
        content: "done".to_string(),
    });
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "apply".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "apply".to_string(),
            },
            actions,
        }],
    }
}

struct SpyRuleSetStore {
    inner: InMemoryRuleSetStore,
    activate_calls: AtomicUsize,
    fail_activate: AtomicBool,
}

impl SpyRuleSetStore {
    fn new() -> Self {
        Self {
            inner: InMemoryRuleSetStore::default(),
            activate_calls: AtomicUsize::new(0),
            fail_activate: AtomicBool::new(false),
        }
    }

    fn publish(&self, create_role: bool) -> RuleSetVersion {
        let outcome = block_on(self.inner.publish(PublishRuleSetRequest {
            guild_id: GUILD,
            ruleset_key: key(),
            definition: definition(create_role),
            created_by: UserId(1),
        }))
        .unwrap();
        match outcome {
            PublishOutcome::Created(version) | PublishOutcome::Reused(version) => version,
        }
    }

    fn activate_calls(&self) -> usize {
        self.activate_calls.load(Ordering::SeqCst)
    }
}

impl RuleSetStore for SpyRuleSetStore {
    async fn publish(
        &self,
        request: PublishRuleSetRequest,
    ) -> Result<PublishOutcome, RuleSetStoreError> {
        self.inner.publish(request).await
    }

    async fn get_version(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
        self.inner.get_version(guild_id, key, version).await
    }

    async fn list_versions(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Vec<RuleSetVersion>, RuleSetStoreError> {
        self.inner.list_versions(guild_id, key).await
    }

    async fn activate(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<RuleSetActivation, RuleSetStoreError> {
        self.activate_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_activate.load(Ordering::SeqCst) {
            return Err(RuleSetStoreError::Backend("outcome unknown".to_string()));
        }
        self.inner.activate(guild_id, key, version).await
    }

    async fn activate_guarded(
        &self,
        request: GuardedRuleSetActivation,
    ) -> Result<GuardedActivationOutcome, RuleSetStoreError> {
        self.activate_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_activate.load(Ordering::SeqCst) {
            return Err(RuleSetStoreError::Backend("outcome unknown".to_string()));
        }
        self.inner.activate_guarded(request).await
    }

    async fn active(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
        self.inner.active(guild_id, key).await
    }
}

enum ProviderMode {
    Ready,
    ProductReady,
    ProductRoleHierarchyMissing,
    ProductBindingRevisionDrift,
    ProductBindingFingerprintDrift,
    MissingCapability,
    Fail,
}

struct SpyProvider {
    mode: ProviderMode,
    calls: AtomicUsize,
}

impl SpyProvider {
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl ActivationEnvironmentProvider for SpyProvider {
    async fn load_fresh(
        &self,
        _: &ActivationTarget,
    ) -> Result<ActivationEnvironment, ActivationEnvironmentError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.mode {
            ProviderMode::Ready => Ok(ActivationEnvironment {
                binding_revision: None,
                bindings: ResourceBindingMap::default(),
                guild_capabilities: GuildCapabilities {
                    base_permissions: Permissions::ADMINISTRATOR,
                },
                role_permissions: BTreeMap::new(),
                role_hierarchy: Some(ready_role_hierarchy()),
            }),
            ProviderMode::ProductReady => Ok(ActivationEnvironment {
                binding_revision: NonZeroU64::new(3),
                bindings: ResourceBindingMap::default(),
                guild_capabilities: GuildCapabilities {
                    base_permissions: Permissions::ADMINISTRATOR,
                },
                role_permissions: BTreeMap::new(),
                role_hierarchy: Some(ready_role_hierarchy()),
            }),
            ProviderMode::ProductRoleHierarchyMissing => Ok(ActivationEnvironment {
                binding_revision: NonZeroU64::new(3),
                bindings: ResourceBindingMap::default(),
                guild_capabilities: GuildCapabilities {
                    base_permissions: Permissions::ADMINISTRATOR,
                },
                role_permissions: BTreeMap::new(),
                role_hierarchy: None,
            }),
            ProviderMode::ProductBindingRevisionDrift => Ok(ActivationEnvironment {
                binding_revision: NonZeroU64::new(4),
                bindings: ResourceBindingMap::default(),
                guild_capabilities: GuildCapabilities {
                    base_permissions: Permissions::ADMINISTRATOR,
                },
                role_permissions: BTreeMap::new(),
                role_hierarchy: Some(ready_role_hierarchy()),
            }),
            ProviderMode::ProductBindingFingerprintDrift => {
                let mut bindings = ResourceBindingMap::default();
                bindings
                    .role_bindings
                    .insert(ResourceKey("member".to_string()), RoleId(202));
                Ok(ActivationEnvironment {
                    binding_revision: NonZeroU64::new(3),
                    bindings,
                    guild_capabilities: GuildCapabilities {
                        base_permissions: Permissions::ADMINISTRATOR,
                    },
                    role_permissions: BTreeMap::new(),
                    role_hierarchy: Some(ready_role_hierarchy()),
                })
            }
            ProviderMode::MissingCapability => Ok(ActivationEnvironment {
                binding_revision: None,
                bindings: ResourceBindingMap::default(),
                guild_capabilities: GuildCapabilities {
                    base_permissions: Permissions::SEND_MESSAGES,
                },
                role_permissions: BTreeMap::new(),
                role_hierarchy: Some(ready_role_hierarchy()),
            }),
            ProviderMode::Fail => Err(ActivationEnvironmentError::Load(
                "snapshot failed".to_string(),
            )),
        }
    }
}

struct Fixture {
    clock: ManualActivationClock,
    requests: InMemoryActivationRequestStore<ManualActivationClock>,
    rulesets: SpyRuleSetStore,
    provider: SpyProvider,
}

impl Fixture {
    fn new(provider: ProviderMode) -> Self {
        let clock = ManualActivationClock::new(Utc.with_ymd_and_hms(2026, 7, 12, 0, 0, 0).unwrap());
        Self {
            requests: InMemoryActivationRequestStore::with_clock(clock.clone()),
            clock,
            rulesets: SpyRuleSetStore::new(),
            provider: SpyProvider {
                mode: provider,
                calls: AtomicUsize::new(0),
            },
        }
    }

    fn service(
        &self,
    ) -> ActivationService<
        '_,
        InMemoryActivationRequestStore<ManualActivationClock>,
        SpyRuleSetStore,
        SpyProvider,
    > {
        ActivationService::new(&self.requests, &self.rulesets, &self.provider)
    }

    fn request(&self, id: &str, create_role: bool, required: u32) -> ActivationRequestId {
        let version = self.rulesets.publish(create_role);
        let id = ActivationRequestId::parse(id).unwrap();
        block_on(self.service().request_activation(RequestActivation {
            id: id.clone(),
            guild_id: GUILD,
            ruleset_key: key(),
            version: version.version,
            requester: UserId(10),
            required_approvals: required,
            ttl: Duration::minutes(30),
        }))
        .unwrap();
        id
    }

    fn approve(&self, id: &ActivationRequestId) {
        block_on(self.service().approve(id, UserId(20))).unwrap();
    }
}

fn attempt(value: &str) -> ApplyAttemptId {
    ApplyAttemptId::parse(value).unwrap()
}

fn digest(value: char) -> ActivationDigest {
    ActivationDigest::parse(&value.to_string().repeat(64)).unwrap()
}

fn product_context(
    id: &ActivationRequestId,
    target: &RuleSetVersion,
    baseline: ExpectedActiveBaselineV1,
) -> ProductApprovalContextV1 {
    let binding_revision = NonZeroU64::new(3).unwrap();
    let policy_revision = NonZeroU64::new(7).unwrap();
    let required_approvals = NonZeroU32::new(1).unwrap();
    let ttl_seconds = NonZeroU64::new(1_800).unwrap();
    let activation_target = ActivationTarget {
        guild_id: target.guild_id,
        ruleset_key: target.ruleset_key.clone(),
        version: target.version,
        content_hash: target.content_hash,
    };
    let mut context = ProductApprovalContextV1 {
        promotion_id: ActivationPromotionId::parse(&"a".repeat(64)).unwrap(),
        promotion_request_digest: digest('b'),
        approval_payload_digest: digest('c'),
        approval_context_digest: digest('d'),
        binding: ApprovalBindingContextV1 {
            revision: binding_revision,
            required_bindings: Vec::new(),
            fingerprint: approval_binding_fingerprint_v1(GUILD, binding_revision, &[]).unwrap(),
        },
        baseline,
        policy: ApprovalPolicyBindingV1 {
            revision: policy_revision,
            required_approvals,
            ttl_seconds,
            digest: approval_policy_digest_v1(policy_revision, required_approvals, ttl_seconds),
        },
    };
    context.approval_context_digest =
        product_approval_context_digest_v1(id, &activation_target, UserId(10), &context);
    context
}

fn product_context_with_required_bindings(
    id: &ActivationRequestId,
    target: &RuleSetVersion,
    baseline: ExpectedActiveBaselineV1,
    required_bindings: Vec<ResolvedApprovalBinding>,
) -> ProductApprovalContextV1 {
    let mut context = product_context(id, target, baseline);
    context.binding.fingerprint =
        approval_binding_fingerprint_v1(GUILD, context.binding.revision, &required_bindings)
            .unwrap();
    context.binding.required_bindings = required_bindings;
    context.approval_context_digest = product_approval_context_digest_v1(
        id,
        &ActivationTarget {
            guild_id: target.guild_id,
            ruleset_key: target.ruleset_key.clone(),
            version: target.version,
            content_hash: target.content_hash,
        },
        UserId(10),
        &context,
    );
    context
}

fn create_approved_product(
    fixture: &Fixture,
    id_value: &str,
    target: &RuleSetVersion,
    baseline: ExpectedActiveBaselineV1,
) -> (ActivationRequestId, ProductApprovalContextV1) {
    let id = ActivationRequestId::parse(id_value).unwrap();
    let context = product_context(&id, target, baseline);
    create_approved_product_with_context(fixture, id, target, context)
}

fn create_approved_product_with_context(
    fixture: &Fixture,
    id: ActivationRequestId,
    target: &RuleSetVersion,
    context: ProductApprovalContextV1,
) -> (ActivationRequestId, ProductApprovalContextV1) {
    block_on(
        fixture
            .requests
            .create_product(CreateProductActivationRequest {
                id: id.clone(),
                target: ActivationTarget {
                    guild_id: target.guild_id,
                    ruleset_key: target.ruleset_key.clone(),
                    version: target.version,
                    content_hash: target.content_hash,
                },
                requester: UserId(10),
                context: context.clone(),
            }),
    )
    .unwrap();
    block_on(fixture.requests.link_product(
        &id,
        LinkProductActivation {
            promotion_id: context.promotion_id.clone(),
            promotion_request_digest: context.promotion_request_digest.clone(),
            approval_context_digest: context.approval_context_digest.clone(),
        },
    ))
    .unwrap();
    block_on(
        fixture
            .service()
            .approve_bound(&id, UserId(20), &context.approval_payload_digest),
    )
    .unwrap();
    (id, context)
}

#[test]
fn product_approval_service_requires_the_exact_payload_digest() {
    let fixture = Fixture::new(ProviderMode::ProductReady);
    let target = fixture.rulesets.publish(false);
    let id = ActivationRequestId::parse("bound_service").unwrap();
    let context = product_context(&id, &target, ExpectedActiveBaselineV1::Absent);
    block_on(
        fixture
            .requests
            .create_product(CreateProductActivationRequest {
                id: id.clone(),
                target: ActivationTarget {
                    guild_id: target.guild_id,
                    ruleset_key: target.ruleset_key.clone(),
                    version: target.version,
                    content_hash: target.content_hash,
                },
                requester: UserId(10),
                context: context.clone(),
            }),
    )
    .unwrap();
    block_on(fixture.requests.link_product(
        &id,
        LinkProductActivation {
            promotion_id: context.promotion_id.clone(),
            promotion_request_digest: context.promotion_request_digest.clone(),
            approval_context_digest: context.approval_context_digest.clone(),
        },
    ))
    .unwrap();

    assert_eq!(
        block_on(
            fixture
                .service()
                .approve_bound(&id, UserId(20), &digest('f'))
        )
        .unwrap_err(),
        automation_ruleset_activation::ApproveError::PayloadMismatch
    );
    let approved = block_on(fixture.service().approve_bound(
        &id,
        UserId(20),
        &context.approval_payload_digest,
    ))
    .unwrap();
    assert_eq!(approved.state, ActivationRequestState::Approved);
}

#[test]
fn request_captures_target_hash_and_observed_active() {
    let fixture = Fixture::new(ProviderMode::Ready);
    let active = fixture.rulesets.publish(false);
    block_on(
        fixture
            .rulesets
            .inner
            .activate(GUILD, &key(), active.version),
    )
    .unwrap();
    let target = fixture.rulesets.publish(true);
    let id = ActivationRequestId::parse("request").unwrap();

    let request = block_on(fixture.service().request_activation(RequestActivation {
        id,
        guild_id: GUILD,
        ruleset_key: key(),
        version: target.version,
        requester: UserId(10),
        required_approvals: 2,
        ttl: Duration::minutes(30),
    }))
    .unwrap();

    assert_eq!(request.target.content_hash, target.content_hash);
    assert_eq!(request.observed_active.unwrap().version, active.version);
    assert_eq!(request.required_approvals, 2);
}

#[test]
fn pending_request_cannot_apply() {
    let fixture = Fixture::new(ProviderMode::Ready);
    let id = fixture.request("pending", false, 1);

    assert_eq!(
        block_on(fixture.service().apply(&id, attempt("a1"), UserId(10))).unwrap_err(),
        ApplyError::NotApproved
    );
    assert_eq!(fixture.provider.calls(), 0);
    assert_eq!(fixture.rulesets.activate_calls(), 0);
}

#[test]
fn applying_request_returns_in_progress_without_activation() {
    let fixture = Fixture::new(ProviderMode::Ready);
    let id = fixture.request("applying", false, 1);
    fixture.approve(&id);
    assert!(matches!(
        block_on(fixture.requests.claim_apply(&id, attempt("owner"), 60)).unwrap(),
        ClaimOutcome::Claimed(_)
    ));

    let outcome = block_on(fixture.service().apply(&id, attempt("other"), UserId(10))).unwrap();
    assert!(matches!(
        outcome,
        ApplyOutcome::InProgress {
            blocking_request_id,
            lease_expired: false,
            ..
        } if blocking_request_id == id
    ));
    assert_eq!(fixture.provider.calls(), 0);
    assert_eq!(fixture.rulesets.activate_calls(), 0);
}

#[test]
fn resume_reclaims_only_an_expired_applying_lease() {
    let fixture = Fixture::new(ProviderMode::Ready);
    let id = fixture.request("resume", false, 1);
    fixture.approve(&id);
    assert!(matches!(
        block_on(fixture.requests.claim_apply(&id, attempt("owner"), 60)).unwrap(),
        ClaimOutcome::Claimed(_)
    ));

    assert!(matches!(
        block_on(fixture.service().resume(&id, attempt("early"), UserId(10))).unwrap(),
        ApplyOutcome::InProgress {
            lease_expired: false,
            ..
        }
    ));
    assert_eq!(fixture.rulesets.activate_calls(), 0);

    fixture.clock.advance(Duration::seconds(61));
    assert_eq!(
        block_on(
            fixture
                .service()
                .resume(&id, attempt("resumed"), UserId(10))
        )
        .unwrap(),
        ApplyOutcome::Activated
    );
    let stored = block_on(fixture.requests.get(&id)).unwrap().unwrap();
    assert_eq!(stored.state, ActivationRequestState::Applied);
    assert_eq!(stored.apply_attempt_no, 2);
}

#[test]
fn already_active_short_circuits_snapshot_and_activate() {
    let fixture = Fixture::new(ProviderMode::Ready);
    let id = fixture.request("already", false, 1);
    fixture.approve(&id);
    let request = block_on(fixture.requests.get(&id)).unwrap().unwrap();
    block_on(fixture.rulesets.inner.activate(
        request.target.guild_id,
        &request.target.ruleset_key,
        request.target.version,
    ))
    .unwrap();

    let outcome = block_on(fixture.service().apply(&id, attempt("a1"), UserId(10))).unwrap();
    assert_eq!(outcome, ApplyOutcome::AlreadyActive);
    assert_eq!(fixture.provider.calls(), 0);
    assert_eq!(fixture.rulesets.activate_calls(), 0);
    let stored = block_on(fixture.requests.get(&id)).unwrap().unwrap();
    assert_eq!(stored.state, ActivationRequestState::Applied);
}

#[test]
fn version_equal_hash_difference_is_target_corrupt() {
    let fixture = Fixture::new(ProviderMode::Ready);
    let version = fixture.rulesets.publish(false);
    let id = ActivationRequestId::parse("corrupt").unwrap();
    block_on(fixture.requests.create(CreateActivationRequest {
        id: id.clone(),
        target: ActivationTarget {
            guild_id: GUILD,
            ruleset_key: key(),
            version: version.version,
            content_hash: RuleSetContentHash::parse_hex(&"ff".repeat(32)).unwrap(),
        },
        requester: UserId(10),
        required_approvals: 1,
        ttl: Duration::minutes(30),
        observed_active: None,
    }))
    .unwrap();
    block_on(fixture.requests.approve(&id, UserId(20))).unwrap();

    assert_eq!(
        block_on(fixture.service().apply(&id, attempt("a1"), UserId(10))).unwrap_err(),
        ApplyError::TargetCorrupt
    );
    let stored = block_on(fixture.requests.get(&id)).unwrap().unwrap();
    assert_eq!(stored.state, ActivationRequestState::Approved);
    assert!(stored.last_apply_error.is_some());
}

#[test]
fn known_failure_releases_to_approved_and_keeps_approvals() {
    let fixture = Fixture::new(ProviderMode::Fail);
    let id = fixture.request("known", false, 1);
    fixture.approve(&id);

    assert!(matches!(
        block_on(fixture.service().apply(&id, attempt("a1"), UserId(10))).unwrap_err(),
        ApplyError::Environment(_)
    ));
    let stored = block_on(fixture.requests.get(&id)).unwrap().unwrap();
    assert_eq!(stored.state, ActivationRequestState::Approved);
    assert_eq!(stored.approvals.len(), 1);
    assert!(stored.last_apply_error.is_some());
}

#[test]
fn indeterminate_activation_failure_stays_applying() {
    let fixture = Fixture::new(ProviderMode::Ready);
    let id = fixture.request("unknown", false, 1);
    fixture.approve(&id);
    fixture.rulesets.fail_activate.store(true, Ordering::SeqCst);

    assert!(matches!(
        block_on(fixture.service().apply(&id, attempt("a1"), UserId(10))).unwrap_err(),
        ApplyError::IndeterminateActivation(_)
    ));
    assert_eq!(
        block_on(fixture.requests.get(&id)).unwrap().unwrap().state,
        ActivationRequestState::Applying
    );
}

#[test]
fn success_marks_applied_with_notices_and_requester_may_apply() {
    let fixture = Fixture::new(ProviderMode::Ready);
    let id = fixture.request("success", false, 1);
    fixture.approve(&id);

    let outcome = block_on(fixture.service().apply(&id, attempt("a1"), UserId(10))).unwrap();
    assert_eq!(outcome, ApplyOutcome::Activated);
    assert_eq!(fixture.provider.calls(), 1);
    assert_eq!(fixture.rulesets.activate_calls(), 1);
    let stored = block_on(fixture.requests.get(&id)).unwrap().unwrap();
    assert_eq!(stored.state, ActivationRequestState::Applied);
    assert!(stored.completion.unwrap().notices.is_some());
}

#[test]
fn product_activation_uses_guarded_pointer_cas() {
    let fixture = Fixture::new(ProviderMode::ProductReady);
    let target = fixture.rulesets.publish(false);
    let (id, _) = create_approved_product(
        &fixture,
        "product_success",
        &target,
        ExpectedActiveBaselineV1::Absent,
    );

    let outcome = block_on(
        fixture
            .service()
            .apply(&id, attempt("product_attempt"), UserId(10)),
    )
    .unwrap();

    assert_eq!(outcome, ApplyOutcome::Activated);
    assert_eq!(fixture.rulesets.activate_calls(), 1);
    assert_eq!(fixture.provider.calls(), 1);
    assert_eq!(
        block_on(fixture.rulesets.active(GUILD, &key()))
            .unwrap()
            .unwrap()
            .version,
        target.version
    );
    assert_eq!(
        block_on(fixture.requests.get(&id)).unwrap().unwrap().state,
        ActivationRequestState::Applied
    );
}

#[test]
fn product_activation_releases_claim_when_role_hierarchy_is_unavailable() {
    let fixture = Fixture::new(ProviderMode::ProductRoleHierarchyMissing);
    let target = fixture.rulesets.publish(true);
    let (id, _) = create_approved_product(
        &fixture,
        "product_role_hierarchy_missing",
        &target,
        ExpectedActiveBaselineV1::Absent,
    );

    assert_eq!(
        block_on(
            fixture
                .service()
                .apply(&id, attempt("product_role_hierarchy"), UserId(10)),
        )
        .unwrap_err(),
        ApplyError::RoleHierarchyNotReady(
            automation_ruleset_readiness::RoleHierarchyReadinessErrorV1::EvidenceUnavailable,
        )
    );
    assert_eq!(fixture.rulesets.activate_calls(), 0);
    assert!(block_on(fixture.rulesets.active(GUILD, &key()))
        .unwrap()
        .is_none());
    assert_eq!(
        block_on(fixture.requests.get(&id)).unwrap().unwrap().state,
        ActivationRequestState::Approved
    );
}

#[test]
fn product_activation_is_superseded_before_snapshot_when_baseline_drifted() {
    let fixture = Fixture::new(ProviderMode::ProductReady);
    let target = fixture.rulesets.publish(false);
    let (id, _) = create_approved_product(
        &fixture,
        "product_baseline_drift",
        &target,
        ExpectedActiveBaselineV1::Absent,
    );
    let competing = fixture.rulesets.publish(true);
    block_on(
        fixture
            .rulesets
            .inner
            .activate(GUILD, &key(), competing.version),
    )
    .unwrap();

    let outcome = block_on(
        fixture
            .service()
            .apply(&id, attempt("product_drift"), UserId(10)),
    )
    .unwrap();

    assert!(matches!(
        outcome,
        ApplyOutcome::Superseded {
            reason: SupersessionReasonV1::ActiveBaselineDrift { .. }
        }
    ));
    assert_eq!(fixture.provider.calls(), 0);
    assert_eq!(fixture.rulesets.activate_calls(), 0);
    assert_eq!(
        block_on(fixture.rulesets.active(GUILD, &key()))
            .unwrap()
            .unwrap()
            .version,
        competing.version
    );
    assert_eq!(
        block_on(fixture.requests.get(&id)).unwrap().unwrap().state,
        ActivationRequestState::Superseded
    );
}

#[test]
fn product_activation_is_superseded_when_approved_binding_revision_drifted() {
    let fixture = Fixture::new(ProviderMode::ProductBindingRevisionDrift);
    let target = fixture.rulesets.publish(false);
    let (id, _) = create_approved_product(
        &fixture,
        "product_binding_drift",
        &target,
        ExpectedActiveBaselineV1::Absent,
    );

    let outcome = block_on(
        fixture
            .service()
            .apply(&id, attempt("product_binding"), UserId(10)),
    )
    .unwrap();

    assert!(matches!(
        outcome,
        ApplyOutcome::Superseded {
            reason: SupersessionReasonV1::BindingDrift { .. }
        }
    ));
    assert_eq!(fixture.rulesets.activate_calls(), 0);
    assert!(block_on(fixture.rulesets.active(GUILD, &key()))
        .unwrap()
        .is_none());
}

#[test]
fn policy_drift_reason_roundtrips_with_exact_policy_evidence() {
    let reason = SupersessionReasonV1::PolicyDrift {
        expected_revision: NonZeroU64::new(3).unwrap(),
        observed_revision: NonZeroU64::new(4).unwrap(),
        expected_required_approvals: NonZeroU32::new(2).unwrap(),
        observed_required_approvals: NonZeroU32::new(3).unwrap(),
        expected_ttl_seconds: NonZeroU64::new(1_800).unwrap(),
        observed_ttl_seconds: NonZeroU64::new(900).unwrap(),
    };

    let value = serde_json::to_value(&reason).unwrap();

    assert_eq!(value["reason"], "policy_drift");
    assert_eq!(value["expected_revision"], 3);
    assert_eq!(value["observed_revision"], 4);
    assert_eq!(value["expected_required_approvals"], 2);
    assert_eq!(value["observed_required_approvals"], 3);
    assert_eq!(value["expected_ttl_seconds"], 1_800);
    assert_eq!(value["observed_ttl_seconds"], 900);
    assert_eq!(
        serde_json::from_value::<SupersessionReasonV1>(value).unwrap(),
        reason
    );
}

#[test]
fn policy_drift_reason_rejects_zero_and_unknown_evidence() {
    let zero = serde_json::json!({
        "reason": "policy_drift",
        "expected_revision": 0,
        "observed_revision": 4,
        "expected_required_approvals": 2,
        "observed_required_approvals": 3,
        "expected_ttl_seconds": 1800,
        "observed_ttl_seconds": 900
    });
    let unknown = serde_json::json!({
        "reason": "policy_drift",
        "expected_revision": 3,
        "observed_revision": 4,
        "expected_required_approvals": 2,
        "observed_required_approvals": 3,
        "expected_ttl_seconds": 1800,
        "observed_ttl_seconds": 900,
        "unexpected": true
    });

    assert!(serde_json::from_value::<SupersessionReasonV1>(zero).is_err());
    assert!(serde_json::from_value::<SupersessionReasonV1>(unknown).is_err());
}

#[test]
fn product_exact_active_target_is_superseded_when_binding_revision_drifted() {
    let fixture = Fixture::new(ProviderMode::ProductBindingRevisionDrift);
    let target = fixture.rulesets.publish(false);
    let (id, _) = create_approved_product(
        &fixture,
        "product_active_revision_drift",
        &target,
        ExpectedActiveBaselineV1::Absent,
    );
    block_on(
        fixture
            .rulesets
            .inner
            .activate(GUILD, &key(), target.version),
    )
    .unwrap();

    let outcome = block_on(fixture.service().apply(
        &id,
        attempt("product_active_revision"),
        UserId(10),
    ))
    .unwrap();

    assert!(matches!(
        outcome,
        ApplyOutcome::Superseded {
            reason: SupersessionReasonV1::BindingDrift {
                expected_revision,
                observed_revision,
                ..
            }
        } if expected_revision.get() == 3 && observed_revision.get() == 4
    ));
    assert_eq!(fixture.provider.calls(), 1);
    assert_eq!(fixture.rulesets.activate_calls(), 0);
    assert_eq!(
        block_on(fixture.requests.get(&id)).unwrap().unwrap().state,
        ActivationRequestState::Superseded
    );
}

#[test]
fn product_exact_active_target_is_superseded_when_binding_fingerprint_drifted() {
    let fixture = Fixture::new(ProviderMode::ProductBindingFingerprintDrift);
    let target = fixture.rulesets.publish(false);
    let id = ActivationRequestId::parse("product_active_fingerprint_drift").unwrap();
    let context = product_context_with_required_bindings(
        &id,
        &target,
        ExpectedActiveBaselineV1::Absent,
        vec![ResolvedApprovalBinding::Role {
            key: ResourceKey("member".to_string()),
            id: RoleId(101),
        }],
    );
    let (id, _) = create_approved_product_with_context(&fixture, id, &target, context);
    block_on(
        fixture
            .rulesets
            .inner
            .activate(GUILD, &key(), target.version),
    )
    .unwrap();

    let outcome = block_on(fixture.service().apply(
        &id,
        attempt("product_active_fingerprint"),
        UserId(10),
    ))
    .unwrap();

    assert!(matches!(
        outcome,
        ApplyOutcome::Superseded {
            reason: SupersessionReasonV1::BindingDrift {
                expected_revision,
                observed_revision,
                expected_fingerprint,
                observed_fingerprint: Some(observed_fingerprint),
            }
        } if expected_revision == observed_revision
            && expected_fingerprint != observed_fingerprint
    ));
    assert_eq!(fixture.provider.calls(), 1);
    assert_eq!(fixture.rulesets.activate_calls(), 0);
    assert_eq!(
        block_on(fixture.requests.get(&id)).unwrap().unwrap().state,
        ActivationRequestState::Superseded
    );
}

#[test]
fn product_activation_without_authoritative_binding_revision_fails_closed() {
    let fixture = Fixture::new(ProviderMode::Ready);
    let target = fixture.rulesets.publish(false);
    let (id, _) = create_approved_product(
        &fixture,
        "product_unversioned_binding",
        &target,
        ExpectedActiveBaselineV1::Absent,
    );

    assert!(matches!(
        block_on(
            fixture
                .service()
                .apply(&id, attempt("product_unversioned"), UserId(10))
        ),
        Err(ApplyError::Environment(_))
    ));
    assert_eq!(
        block_on(fixture.requests.get(&id)).unwrap().unwrap().state,
        ActivationRequestState::Approved
    );
    assert_eq!(fixture.rulesets.activate_calls(), 0);
}

struct BaselineDriftingProvider<'a> {
    rulesets: &'a SpyRuleSetStore,
    competing_version: RuleSetVersionId,
}

impl ActivationEnvironmentProvider for BaselineDriftingProvider<'_> {
    async fn load_fresh(
        &self,
        _: &ActivationTarget,
    ) -> Result<ActivationEnvironment, ActivationEnvironmentError> {
        self.rulesets
            .inner
            .activate(GUILD, &key(), self.competing_version)
            .await
            .unwrap();
        Ok(ActivationEnvironment {
            binding_revision: NonZeroU64::new(3),
            bindings: ResourceBindingMap::default(),
            guild_capabilities: GuildCapabilities {
                base_permissions: Permissions::ADMINISTRATOR,
            },
            role_permissions: BTreeMap::new(),
            role_hierarchy: Some(ready_role_hierarchy()),
        })
    }
}

#[test]
fn guarded_cas_catches_baseline_drift_during_fresh_environment_load() {
    let fixture = Fixture::new(ProviderMode::ProductReady);
    let target = fixture.rulesets.publish(false);
    let (id, _) = create_approved_product(
        &fixture,
        "product_midflight_drift",
        &target,
        ExpectedActiveBaselineV1::Absent,
    );
    let competing = fixture.rulesets.publish(true);
    let provider = BaselineDriftingProvider {
        rulesets: &fixture.rulesets,
        competing_version: competing.version,
    };
    let service = ActivationService::new(&fixture.requests, &fixture.rulesets, &provider);

    let outcome = block_on(service.apply(&id, attempt("midflight_drift"), UserId(10))).unwrap();

    assert!(matches!(
        outcome,
        ApplyOutcome::Superseded {
            reason: SupersessionReasonV1::ActiveBaselineDrift { .. }
        }
    ));
    assert_eq!(fixture.rulesets.activate_calls(), 1);
    assert_eq!(
        block_on(fixture.rulesets.active(GUILD, &key()))
            .unwrap()
            .unwrap()
            .version,
        competing.version
    );
}

struct StealingProvider<'a> {
    requests: &'a InMemoryActivationRequestStore<ManualActivationClock>,
    clock: ManualActivationClock,
    request_id: ActivationRequestId,
}

impl ActivationEnvironmentProvider for StealingProvider<'_> {
    async fn load_fresh(
        &self,
        _: &ActivationTarget,
    ) -> Result<ActivationEnvironment, ActivationEnvironmentError> {
        self.clock.advance(Duration::seconds(61));
        let outcome = self
            .requests
            .claim_resume(&self.request_id, attempt("stolen"), 60)
            .await
            .unwrap();
        assert!(matches!(outcome, ClaimOutcome::Claimed(_)));
        Ok(ActivationEnvironment {
            binding_revision: None,
            bindings: ResourceBindingMap::default(),
            guild_capabilities: GuildCapabilities {
                base_permissions: Permissions::ADMINISTRATOR,
            },
            role_permissions: BTreeMap::new(),
            role_hierarchy: Some(ready_role_hierarchy()),
        })
    }
}

#[test]
fn lease_loss_before_mutation_skips_activate() {
    let fixture = Fixture::new(ProviderMode::Ready);
    let id = fixture.request("lease", false, 1);
    fixture.approve(&id);
    let provider = StealingProvider {
        requests: &fixture.requests,
        clock: fixture.clock.clone(),
        request_id: id.clone(),
    };
    let service = ActivationService::new(&fixture.requests, &fixture.rulesets, &provider);

    assert_eq!(
        block_on(service.apply(&id, attempt("owner"), UserId(10))).unwrap_err(),
        ApplyError::LeaseLost
    );
    assert_eq!(fixture.rulesets.activate_calls(), 0);
}

#[test]
fn not_ready_is_known_safe_and_pointer_stays_unchanged() {
    let fixture = Fixture::new(ProviderMode::MissingCapability);
    let id = fixture.request("not_ready", true, 1);
    fixture.approve(&id);

    assert!(matches!(
        block_on(fixture.service().apply(&id, attempt("a1"), UserId(10))).unwrap_err(),
        ApplyError::NotReady(_)
    ));
    assert!(block_on(fixture.rulesets.active(GUILD, &key()))
        .unwrap()
        .is_none());
    assert_eq!(
        block_on(fixture.requests.get(&id)).unwrap().unwrap().state,
        ActivationRequestState::Approved
    );
}

#[test]
fn boot_bookkeeping_only_recovers_exact_active_target() {
    let fixture = Fixture::new(ProviderMode::Ready);
    let exact = fixture.request("exact", false, 1);
    fixture.approve(&exact);
    block_on(fixture.requests.claim_apply(&exact, attempt("owner"), 60)).unwrap();
    let request = block_on(fixture.requests.get(&exact)).unwrap().unwrap();
    block_on(fixture.rulesets.inner.activate(
        request.target.guild_id,
        &request.target.ruleset_key,
        request.target.version,
    ))
    .unwrap();

    let report = block_on(fixture.service().recover_applying(GUILD)).unwrap();
    assert!(matches!(
        report.entries.as_slice(),
        [entry] if entry.disposition == RecoveryDisposition::Recovered
    ));
    assert_eq!(fixture.provider.calls(), 0);
    assert_eq!(fixture.rulesets.activate_calls(), 0);
    assert_eq!(
        block_on(fixture.requests.get(&exact))
            .unwrap()
            .unwrap()
            .state,
        ActivationRequestState::Applied
    );
}

#[test]
fn caller_cannot_supply_environment_and_service_loads_fresh() {
    let fixture = Fixture::new(ProviderMode::Ready);
    let id = fixture.request("fresh", false, 1);
    fixture.approve(&id);

    block_on(fixture.service().apply(&id, attempt("a1"), UserId(10))).unwrap();
    assert_eq!(fixture.provider.calls(), 1);
}

#[cfg(feature = "unsafe-dev-activation")]
#[test]
fn unsafe_dev_activation_keeps_readiness_gate() {
    let fixture = Fixture::new(ProviderMode::MissingCapability);
    let version = fixture.rulesets.publish(true);
    let target = ActivationTarget {
        guild_id: GUILD,
        ruleset_key: key(),
        version: version.version,
        content_hash: version.content_hash,
    };

    assert!(matches!(
        block_on(automation_ruleset_activation::unsafe_dev_activate(
            &fixture.rulesets,
            &fixture.provider,
            target,
            UserId(10),
        )),
        Err(ApplyError::NotReady(_))
    ));
    assert!(block_on(fixture.rulesets.active(GUILD, &key()))
        .unwrap()
        .is_none());
}

#[test]
fn unsafe_symbol_is_feature_gated() {
    let source = include_str!("../src/service.rs");
    assert!(source.contains("#[cfg(feature = \"unsafe-dev-activation\")]"));
    assert!(source.contains("pub async fn unsafe_dev_activate"));
}
