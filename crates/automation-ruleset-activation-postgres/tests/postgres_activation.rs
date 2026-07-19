use std::collections::BTreeMap;
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::atomic::{AtomicUsize, Ordering};

use automation_ruleset::{
    GuardedActivationOutcome, GuardedRuleSetActivation, PublishOutcome, PublishRuleSetRequest,
    RuleSetActivation, RuleSetKey, RuleSetStore, RuleSetStoreError, RuleSetVersion,
    RuleSetVersionId,
};
use automation_ruleset_activation::{
    approval_policy_digest_v1, product_approval_context_digest_v1, ActivationDigest,
    ActivationEnvironment, ActivationEnvironmentError, ActivationEnvironmentProvider,
    ActivationLinkStateV1, ActivationPromotionId, ActivationRequest, ActivationRequestId,
    ActivationRequestState, ActivationRequestStore, ActivationService, ActivationStoreError,
    ActivationTarget, ApplyAttemptId, ApplyError, ApplyErrorRecord, ApplyFailureKind, ApplyOutcome,
    ApprovalBindingContextV1, ApprovalPolicyBindingV1, ApproveError, ClaimOutcome, CompletionKind,
    CreateActivationRequest, CreateProductActivationRequest, ExpectedActiveBaselineV1,
    LinkProductActivation, LinkProductError, ProductApprovalContextV1, RecoveryDisposition,
    RejectError, SupersessionReasonV1, WithdrawError,
};
use automation_ruleset_activation_postgres::{PostgresActivationRequestStore, MIGRATOR};
use automation_ruleset_postgres::PostgresRuleSetStore;
use automation_ruleset_readiness::GuildCapabilities;
use automation_state::InteractionRuleSet;
use chrono::Duration;
use discord_model::{GuildId, Permissions, UserId};
use resource_resolution::{approval_binding_fingerprint_v1, ResourceBindingMap};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tokio::sync::Notify;

const PRODUCT_TENANT_ID: &str = "activation-postgres-tests";
const PRODUCT_PRINCIPAL_ID: &str = "activation-postgres-tests";
const PRODUCT_APPLICATION_ID: &str = "9100000";
const PRODUCT_DISCORD_USER_ID: &str = "10";

fn database_url() -> String {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL must be set for ignored postgres tests");
    assert!(
        url.contains("test"),
        "refusing to run against a database whose name does not contain 'test'"
    );
    url
}

async fn pool() -> PgPool {
    let pool = PgPoolOptions::new()
        .max_connections(16)
        .connect(&database_url())
        .await
        .expect("connect");
    MIGRATOR.run(&pool).await.expect("migrate");
    pool
}

async fn cleanup(pool: &PgPool, guild_id: GuildId) {
    let guild = guild_id.to_string();
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE public.activation_request_approvals IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "ALTER TABLE public.activation_request_approvals \
         DISABLE TRIGGER activation_request_approvals_reject_mutation",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "DELETE FROM public.activation_request_approvals AS approval \
         USING public.activation_requests AS activation \
         WHERE approval.request_id = activation.id AND activation.guild_id = $1",
    )
    .bind(&guild)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE public.activation_request_approvals \
         ENABLE TRIGGER activation_request_approvals_reject_mutation",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "WITH deleted_requests AS ( \
             DELETE FROM public.activation_requests WHERE guild_id = $1 \
             RETURNING promotion_id \
         ) \
         DELETE FROM public.authoring_promotions AS promotion \
         USING deleted_requests AS request \
         WHERE request.promotion_id IS NOT NULL AND promotion.id = request.promotion_id",
    )
    .bind(&guild)
    .execute(&mut *transaction)
    .await
    .unwrap();
    for table in [
        "automation_ruleset_activations",
        "automation_ruleset_versions",
        "automation_ruleset_heads",
    ] {
        sqlx::query(&format!("DELETE FROM {table} WHERE guild_id = $1"))
            .bind(&guild)
            .execute(&mut *transaction)
            .await
            .unwrap();
    }
    transaction.commit().await.unwrap();
}

fn key() -> RuleSetKey {
    RuleSetKey::parse("studyroom").unwrap()
}

fn definition(version: u32) -> InteractionRuleSet {
    InteractionRuleSet {
        version,
        panels: vec![],
        modals: vec![],
        rules: vec![],
    }
}

fn request_id(value: &str) -> ActivationRequestId {
    ActivationRequestId::parse(value).unwrap()
}

fn attempt_id(value: &str) -> ApplyAttemptId {
    ApplyAttemptId::parse(value).unwrap()
}

fn digest(value: char) -> ActivationDigest {
    ActivationDigest::parse(&value.to_string().repeat(64)).unwrap()
}

fn product_context(
    id: &ActivationRequestId,
    target: &RuleSetVersion,
    promotion_value: char,
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
        promotion_id: ActivationPromotionId::parse(&promotion_value.to_string().repeat(64))
            .unwrap(),
        promotion_request_digest: digest('b'),
        approval_payload_digest: digest('c'),
        approval_context_digest: digest('d'),
        binding: ApprovalBindingContextV1 {
            revision: binding_revision,
            required_bindings: Vec::new(),
            fingerprint: approval_binding_fingerprint_v1(target.guild_id, binding_revision, &[])
                .unwrap(),
        },
        baseline: ExpectedActiveBaselineV1::Absent,
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

fn product_input(
    id: ActivationRequestId,
    target: &RuleSetVersion,
    context: ProductApprovalContextV1,
) -> CreateProductActivationRequest {
    CreateProductActivationRequest {
        id,
        target: ActivationTarget {
            guild_id: target.guild_id,
            ruleset_key: target.ruleset_key.clone(),
            version: target.version,
            content_hash: target.content_hash,
        },
        requester: UserId(10),
        context,
    }
}

fn product_link(context: &ProductApprovalContextV1) -> LinkProductActivation {
    LinkProductActivation {
        promotion_id: context.promotion_id.clone(),
        promotion_request_digest: context.promotion_request_digest.clone(),
        approval_context_digest: context.approval_context_digest.clone(),
    }
}

fn product_installation_id(guild_id: GuildId) -> String {
    format!("activation-postgres-{guild_id}")
}

async fn prepare_product_promotion(
    pool: &PgPool,
    target: &RuleSetVersion,
    context: &ProductApprovalContextV1,
) {
    let installation_id = product_installation_id(target.guild_id);
    let record = json!({
        "id": context.promotion_id.as_str(),
        "revision": 2,
        "request_digest": context.promotion_request_digest.as_str(),
        "intent": {
            "authority": {
                "tenant_id": PRODUCT_TENANT_ID,
                "installation_id": installation_id,
                "principal_id": PRODUCT_PRINCIPAL_ID,
                "guild_id": target.guild_id,
                "ruleset_key": target.ruleset_key
            }
        },
        "stage": {
            "state": "published"
        }
    });
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO public.product_principals (principal_id, discord_user_id) \
         VALUES ($1, $2) ON CONFLICT (principal_id) DO NOTHING",
    )
    .bind(PRODUCT_PRINCIPAL_ID)
    .bind(PRODUCT_DISCORD_USER_ID)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.product_tenants \
         (tenant_id, lifecycle_state, display_name, display_metadata) \
         VALUES ($1, 'active', 'Activation PostgreSQL Tests', '{}'::JSONB) \
         ON CONFLICT (tenant_id) DO NOTHING",
    )
    .bind(PRODUCT_TENANT_ID)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installations \
         (installation_id, tenant_id, discord_application_id, discord_guild_id, ruleset_key, \
          lifecycle_state, current_authority_revision) \
         VALUES ($1, $2, $3, $4, $5, 'active', 1) \
         ON CONFLICT (installation_id) DO NOTHING",
    )
    .bind(&installation_id)
    .bind(PRODUCT_TENANT_ID)
    .bind(PRODUCT_APPLICATION_ID)
    .bind(target.guild_id.to_string())
    .bind(target.ruleset_key.as_str())
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.automation_installation_authority_versions \
         (installation_id, revision, tenant_id, binding_revision, resource_bindings, \
          binding_fingerprint, policy_revision, required_approvals, activation_ttl_seconds, \
          authority_payload_digest, created_by_principal_id, created_by_request_digest) \
         SELECT $1, 1, $2, $3, '{}'::JSONB, $4, $5, $6, $7, $8, $9, $10 \
         WHERE NOT EXISTS ( \
             SELECT 1 FROM public.automation_installation_authority_versions \
             WHERE installation_id = $1 AND revision = 1 \
         ) \
         ON CONFLICT (installation_id, revision) DO NOTHING",
    )
    .bind(&installation_id)
    .bind(PRODUCT_TENANT_ID)
    .bind(i64::try_from(context.binding.revision.get()).unwrap())
    .bind(context.binding.fingerprint.as_str())
    .bind(i64::try_from(context.policy.revision.get()).unwrap())
    .bind(i32::try_from(context.policy.required_approvals.get()).unwrap())
    .bind(i64::try_from(context.policy.ttl_seconds.get()).unwrap())
    .bind(context.approval_context_digest.as_str())
    .bind(PRODUCT_PRINCIPAL_ID)
    .bind(context.promotion_request_digest.as_str())
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO public.authoring_promotions \
         (id, record_format_version, revision, stage, request_digest, tenant_id, \
          installation_id, principal_id, record) \
         VALUES ($1, 1, 2, 'published', $2, $3, $4, $5, $6)",
    )
    .bind(context.promotion_id.as_str())
    .bind(context.promotion_request_digest.as_str())
    .bind(PRODUCT_TENANT_ID)
    .bind(&installation_id)
    .bind(PRODUCT_PRINCIPAL_ID)
    .bind(record)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn journal_product_link(
    pool: &PgPool,
    id: &ActivationRequestId,
    target: &RuleSetVersion,
    context: &ProductApprovalContextV1,
) {
    let (created_at, expires_at) =
        sqlx::query_as::<_, (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>(
            "SELECT created_at, expires_at FROM activation_requests WHERE id = $1",
        )
        .bind(id.as_str())
        .fetch_one(pool)
        .await
        .unwrap();
    let activation_target = ActivationTarget {
        guild_id: target.guild_id,
        ruleset_key: target.ruleset_key.clone(),
        version: target.version,
        content_hash: target.content_hash,
    };
    let record = json!({
        "id": context.promotion_id.as_str(),
        "revision": 3,
        "request_digest": context.promotion_request_digest.as_str(),
        "intent": {
            "authority": {
                "tenant_id": PRODUCT_TENANT_ID,
                "installation_id": product_installation_id(target.guild_id),
                "principal_id": PRODUCT_PRINCIPAL_ID,
                "guild_id": target.guild_id,
                "ruleset_key": target.ruleset_key
            }
        },
        "stage": {
            "state": "activation_pending",
            "activation": {
                "request_id": id,
                "target": activation_target,
                "requester": UserId(10),
                "required_approvals": context.policy.required_approvals,
                "created_at": created_at,
                "expires_at": expires_at,
                "request_state_at_journal": "pending",
                "approval_context": context
            }
        }
    });
    sqlx::query(
        "UPDATE public.authoring_promotions \
         SET revision = 3, stage = 'activation_pending', record = $2 \
         WHERE id = $1",
    )
    .bind(context.promotion_id.as_str())
    .bind(record)
    .execute(pool)
    .await
    .unwrap();
}

struct CountingRuleSetStore {
    inner: PostgresRuleSetStore,
    activate_calls: AtomicUsize,
}

impl CountingRuleSetStore {
    fn new(pool: PgPool) -> Self {
        Self {
            inner: PostgresRuleSetStore::new(pool),
            activate_calls: AtomicUsize::new(0),
        }
    }

    fn activate_calls(&self) -> usize {
        self.activate_calls.load(Ordering::SeqCst)
    }
}

impl RuleSetStore for CountingRuleSetStore {
    async fn publish(
        &self,
        request: PublishRuleSetRequest,
    ) -> Result<PublishOutcome, RuleSetStoreError> {
        self.inner.publish(request).await
    }

    async fn get_version(
        &self,
        guild_id: GuildId,
        ruleset_key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
        self.inner.get_version(guild_id, ruleset_key, version).await
    }

    async fn list_versions(
        &self,
        guild_id: GuildId,
        ruleset_key: &RuleSetKey,
    ) -> Result<Vec<RuleSetVersion>, RuleSetStoreError> {
        self.inner.list_versions(guild_id, ruleset_key).await
    }

    async fn activate(
        &self,
        guild_id: GuildId,
        ruleset_key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<RuleSetActivation, RuleSetStoreError> {
        self.activate_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.activate(guild_id, ruleset_key, version).await
    }

    async fn activate_guarded(
        &self,
        request: GuardedRuleSetActivation,
    ) -> Result<GuardedActivationOutcome, RuleSetStoreError> {
        self.activate_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.activate_guarded(request).await
    }

    async fn active(
        &self,
        guild_id: GuildId,
        ruleset_key: &RuleSetKey,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
        self.inner.active(guild_id, ruleset_key).await
    }
}

struct ReadyProvider;

fn ready_environment() -> ActivationEnvironment {
    ActivationEnvironment {
        binding_revision: NonZeroU64::new(3),
        bindings: ResourceBindingMap::default(),
        guild_capabilities: GuildCapabilities {
            base_permissions: Permissions::ADMINISTRATOR,
        },
        role_permissions: BTreeMap::new(),
        role_hierarchy: None,
    }
}

impl ActivationEnvironmentProvider for ReadyProvider {
    async fn load_fresh(
        &self,
        _: &ActivationTarget,
    ) -> Result<ActivationEnvironment, ActivationEnvironmentError> {
        Ok(ready_environment())
    }
}

struct FailingProvider;

impl ActivationEnvironmentProvider for FailingProvider {
    async fn load_fresh(
        &self,
        _: &ActivationTarget,
    ) -> Result<ActivationEnvironment, ActivationEnvironmentError> {
        Err(ActivationEnvironmentError::Load(
            "snapshot unavailable".to_string(),
        ))
    }
}

struct BlockingProvider {
    entered: Notify,
    release: Notify,
}

impl BlockingProvider {
    fn new() -> Self {
        Self {
            entered: Notify::new(),
            release: Notify::new(),
        }
    }
}

impl ActivationEnvironmentProvider for BlockingProvider {
    async fn load_fresh(
        &self,
        _: &ActivationTarget,
    ) -> Result<ActivationEnvironment, ActivationEnvironmentError> {
        self.entered.notify_one();
        self.release.notified().await;
        Ok(ready_environment())
    }
}

async fn publish(store: &impl RuleSetStore, guild_id: GuildId, version: u32) -> RuleSetVersion {
    let outcome = store
        .publish(PublishRuleSetRequest {
            guild_id,
            ruleset_key: key(),
            definition: definition(version),
            created_by: UserId(1),
        })
        .await
        .unwrap();
    match outcome {
        PublishOutcome::Created(version) | PublishOutcome::Reused(version) => version,
    }
}

async fn create_request(
    store: &PostgresActivationRequestStore,
    id: &str,
    target: &RuleSetVersion,
    required_approvals: u32,
) -> ActivationRequestId {
    let id = request_id(id);
    store
        .create(CreateActivationRequest {
            id: id.clone(),
            target: ActivationTarget {
                guild_id: target.guild_id,
                ruleset_key: target.ruleset_key.clone(),
                version: target.version,
                content_hash: target.content_hash,
            },
            requester: UserId(10),
            required_approvals,
            ttl: Duration::minutes(30),
            observed_active: None,
        })
        .await
        .unwrap();
    id
}

async fn approve(
    store: &PostgresActivationRequestStore,
    id: &ActivationRequestId,
    approver: u64,
) -> ActivationRequest {
    store.approve(id, UserId(approver)).await.unwrap()
}

async fn decision_snapshot(pool: &PgPool, id: &ActivationRequestId) -> (String, Option<i64>, i64) {
    sqlx::query_as(
        "SELECT activation.state, activation.product_revision, \
         (SELECT pg_catalog.count(*) FROM public.activation_request_approvals AS approval \
          WHERE approval.request_id = activation.id) \
         FROM public.activation_requests AS activation WHERE activation.id = $1",
    )
    .bind(id.as_str())
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn seed_product_approval(
    pool: &PgPool,
    id: &ActivationRequestId,
    context: &ProductApprovalContextV1,
) {
    let mut transaction = pool.begin().await.unwrap();
    let bound = sqlx::query_scalar::<_, String>(
        "SELECT pg_catalog.set_config('starring.product_approval_gate', $1, TRUE)",
    )
    .bind(context.approval_context_digest.as_str())
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(bound, context.approval_context_digest.as_str());
    sqlx::query(
        "INSERT INTO public.activation_request_approvals \
         (request_id, approver_id, approved_at, approval_payload_digest) \
         VALUES ($1, '20', clock_timestamp(), $2)",
    )
    .bind(id.as_str())
    .bind(context.approval_payload_digest.as_str())
    .execute(&mut *transaction)
    .await
    .unwrap();
    let updated = sqlx::query(
        "UPDATE public.activation_requests \
         SET state = 'approved', product_revision = product_revision + 1 \
         WHERE id = $1 AND authority_kind = 'product_authoring' \
         AND state = 'pending' AND product_revision = 1",
    )
    .bind(id.as_str())
    .execute(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(updated.rows_affected(), 1);
    transaction.commit().await.unwrap();
}

async fn create_approved(
    store: &PostgresActivationRequestStore,
    id: &str,
    target: &RuleSetVersion,
) -> ActivationRequestId {
    let id = create_request(store, id, target, 1).await;
    approve(store, &id, 20).await;
    id
}

async fn create_approved_product(
    pool: &PgPool,
    store: &PostgresActivationRequestStore,
    id_value: &str,
    target: &RuleSetVersion,
    promotion_value: char,
) -> (ActivationRequestId, ProductApprovalContextV1) {
    let id = request_id(id_value);
    let context = product_context(&id, target, promotion_value);
    prepare_product_promotion(pool, target, &context).await;
    store
        .create_product(product_input(id.clone(), target, context.clone()))
        .await
        .unwrap();
    journal_product_link(pool, &id, target, &context).await;
    store
        .link_product(&id, product_link(&context))
        .await
        .unwrap();
    seed_product_approval(pool, &id, &context).await;
    (id, context)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn concurrent_apply_serializes_and_activates_once() {
    let pool = pool().await;
    let guild = GuildId(9_100_001);
    cleanup(&pool, guild).await;
    let requests = PostgresActivationRequestStore::new(pool.clone());
    let rulesets = CountingRuleSetStore::new(pool.clone());
    let first = publish(&rulesets, guild, 1).await;
    let second = publish(&rulesets, guild, 2).await;
    let first_id = create_approved(&requests, "serial_a", &first).await;
    let second_id = create_approved(&requests, "serial_b", &second).await;
    let provider = BlockingProvider::new();
    let service = ActivationService::new(&requests, &rulesets, &provider);

    let first_apply = service.apply(&first_id, attempt_id("serial_attempt_a"), UserId(10));
    let second_apply = async {
        provider.entered.notified().await;
        let outcome = service
            .apply(&second_id, attempt_id("serial_attempt_b"), UserId(10))
            .await;
        provider.release.notify_one();
        outcome
    };
    let (first_outcome, second_outcome) = tokio::join!(first_apply, second_apply);

    assert_eq!(first_outcome.unwrap(), ApplyOutcome::Activated);
    assert!(matches!(
        second_outcome.unwrap(),
        ApplyOutcome::InProgress {
            blocking_request_id,
            lease_expired: false,
            ..
        } if blocking_request_id == first_id
    ));
    assert_eq!(rulesets.activate_calls(), 1);
    assert_eq!(
        requests.get(&first_id).await.unwrap().unwrap().state,
        ActivationRequestState::Applied
    );
    assert_eq!(
        requests.get(&second_id).await.unwrap().unwrap().state,
        ActivationRequestState::Approved
    );
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn expired_blocker_blocks_new_apply_without_mutation() {
    let pool = pool().await;
    let guild = GuildId(9_100_002);
    cleanup(&pool, guild).await;
    let requests = PostgresActivationRequestStore::new(pool.clone());
    let rulesets = CountingRuleSetStore::new(pool.clone());
    let first = publish(&rulesets, guild, 1).await;
    let second = publish(&rulesets, guild, 2).await;
    let first_id = create_approved(&requests, "expired_a", &first).await;
    let second_id = create_approved(&requests, "expired_b", &second).await;
    assert!(matches!(
        requests
            .claim_apply(&first_id, attempt_id("expired_owner"), 60)
            .await
            .unwrap(),
        ClaimOutcome::Claimed(_)
    ));
    sqlx::query(
        "UPDATE activation_requests SET apply_lease_until = NOW() - INTERVAL '1 second' WHERE id = $1",
    )
    .bind(first_id.as_str())
    .execute(&pool)
    .await
    .unwrap();
    let service = ActivationService::new(&requests, &rulesets, &ReadyProvider);

    let outcome = service
        .apply(&second_id, attempt_id("expired_other"), UserId(10))
        .await
        .unwrap();

    assert!(matches!(
        outcome,
        ApplyOutcome::InProgress {
            blocking_request_id,
            lease_expired: true,
            ..
        } if blocking_request_id == first_id
    ));
    assert_eq!(rulesets.activate_calls(), 0);
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn resume_completes_blocker_and_releases_slot() {
    let pool = pool().await;
    let guild = GuildId(9_100_003);
    cleanup(&pool, guild).await;
    let requests = PostgresActivationRequestStore::new(pool.clone());
    let rulesets = CountingRuleSetStore::new(pool.clone());
    let first = publish(&rulesets, guild, 1).await;
    let second = publish(&rulesets, guild, 2).await;
    let first_id = create_approved(&requests, "resume_a", &first).await;
    let second_id = create_approved(&requests, "resume_b", &second).await;
    requests
        .claim_apply(&first_id, attempt_id("resume_owner"), 60)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE activation_requests SET apply_lease_until = NOW() - INTERVAL '1 second' WHERE id = $1",
    )
    .bind(first_id.as_str())
    .execute(&pool)
    .await
    .unwrap();
    let service = ActivationService::new(&requests, &rulesets, &ReadyProvider);

    assert_eq!(
        service
            .resume(&first_id, attempt_id("resume_new"), UserId(10))
            .await
            .unwrap(),
        ApplyOutcome::Activated
    );
    assert!(matches!(
        requests
            .claim_apply(&second_id, attempt_id("resume_b_owner"), 60)
            .await
            .unwrap(),
        ClaimOutcome::Claimed(_)
    ));
    assert_eq!(rulesets.activate_calls(), 1);
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn known_failure_releases_slot_for_next_request() {
    let pool = pool().await;
    let guild = GuildId(9_100_004);
    cleanup(&pool, guild).await;
    let requests = PostgresActivationRequestStore::new(pool.clone());
    let rulesets = CountingRuleSetStore::new(pool.clone());
    let first = publish(&rulesets, guild, 1).await;
    let second = publish(&rulesets, guild, 2).await;
    let first_id = create_approved(&requests, "known_a", &first).await;
    let second_id = create_approved(&requests, "known_b", &second).await;
    let service = ActivationService::new(&requests, &rulesets, &FailingProvider);

    assert!(matches!(
        service
            .apply(&first_id, attempt_id("known_attempt"), UserId(10))
            .await
            .unwrap_err(),
        ApplyError::Environment(_)
    ));
    let first_request = requests.get(&first_id).await.unwrap().unwrap();
    assert_eq!(first_request.state, ActivationRequestState::Approved);
    assert_eq!(first_request.approvals.len(), 1);
    assert!(first_request.last_apply_error.is_some());
    assert!(matches!(
        requests
            .claim_apply(&second_id, attempt_id("known_next"), 60)
            .await
            .unwrap(),
        ClaimOutcome::Claimed(_)
    ));
    assert_eq!(rulesets.activate_calls(), 0);
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn crash_bookkeeping_recovers_and_releases_slot() {
    let pool = pool().await;
    let guild = GuildId(9_100_005);
    cleanup(&pool, guild).await;
    let requests = PostgresActivationRequestStore::new(pool.clone());
    let rulesets = CountingRuleSetStore::new(pool.clone());
    let first = publish(&rulesets, guild, 1).await;
    let second = publish(&rulesets, guild, 2).await;
    let first_id = create_approved(&requests, "crash_a", &first).await;
    let second_id = create_approved(&requests, "crash_b", &second).await;
    requests
        .claim_apply(&first_id, attempt_id("crash_owner"), 60)
        .await
        .unwrap();
    rulesets
        .inner
        .activate(guild, &key(), first.version)
        .await
        .unwrap();
    let service = ActivationService::new(&requests, &rulesets, &ReadyProvider);

    let report = service.recover_applying(guild).await.unwrap();

    assert!(matches!(
        report.entries.as_slice(),
        [entry] if entry.request_id == first_id
            && entry.disposition == RecoveryDisposition::Recovered
    ));
    assert!(matches!(
        requests
            .claim_apply(&second_id, attempt_id("crash_next"), 60)
            .await
            .unwrap(),
        ClaimOutcome::Claimed(_)
    ));
    assert_eq!(rulesets.activate_calls(), 0);
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn different_guilds_may_apply_concurrently() {
    let pool = pool().await;
    let first_guild = GuildId(9_100_006);
    let second_guild = GuildId(9_100_007);
    cleanup(&pool, first_guild).await;
    cleanup(&pool, second_guild).await;
    let requests = PostgresActivationRequestStore::new(pool.clone());
    let rulesets = PostgresRuleSetStore::new(pool.clone());
    let first = publish(&rulesets, first_guild, 1).await;
    let second = publish(&rulesets, second_guild, 1).await;
    let first_id = create_approved(&requests, "guild_a", &first).await;
    let second_id = create_approved(&requests, "guild_b", &second).await;

    let (first_claim, second_claim) = tokio::join!(
        requests.claim_apply(&first_id, attempt_id("guild_attempt_a"), 60),
        requests.claim_apply(&second_id, attempt_id("guild_attempt_b"), 60)
    );

    assert!(matches!(first_claim.unwrap(), ClaimOutcome::Claimed(_)));
    assert!(matches!(second_claim.unwrap(), ClaimOutcome::Claimed(_)));
    cleanup(&pool, first_guild).await;
    cleanup(&pool, second_guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn duplicate_targets_share_one_applying_slot() {
    let pool = pool().await;
    let guild = GuildId(9_100_008);
    cleanup(&pool, guild).await;
    let requests = PostgresActivationRequestStore::new(pool.clone());
    let rulesets = PostgresRuleSetStore::new(pool.clone());
    let target = publish(&rulesets, guild, 1).await;
    let first_id = create_approved(&requests, "duplicate_a", &target).await;
    let second_id = create_approved(&requests, "duplicate_b", &target).await;

    let (first, second) = tokio::join!(
        requests.claim_apply(&first_id, attempt_id("duplicate_attempt_a"), 60),
        requests.claim_apply(&second_id, attempt_id("duplicate_attempt_b"), 60)
    );
    let outcomes = [first.unwrap(), second.unwrap()];

    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ClaimOutcome::Claimed(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ClaimOutcome::InProgress { .. }))
            .count(),
        1
    );
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn concurrent_approve_and_reject_choose_one_state() {
    let pool = pool().await;
    let guild = GuildId(9_100_009);
    cleanup(&pool, guild).await;
    let requests = PostgresActivationRequestStore::new(pool.clone());
    let rulesets = PostgresRuleSetStore::new(pool.clone());
    let target = publish(&rulesets, guild, 1).await;
    let id = create_request(&requests, "approve_reject", &target, 1).await;

    let (approval, rejection) = tokio::join!(
        requests.approve(&id, UserId(20)),
        requests.reject(&id, UserId(30), "no".to_string())
    );

    assert_ne!(approval.is_ok(), rejection.is_ok());
    let stored = requests.get(&id).await.unwrap().unwrap();
    assert!(matches!(
        stored.state,
        ActivationRequestState::Approved | ActivationRequestState::Rejected
    ));
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn concurrent_final_approvals_reach_quorum_once() {
    let pool = pool().await;
    let guild = GuildId(9_100_010);
    cleanup(&pool, guild).await;
    let requests = PostgresActivationRequestStore::new(pool.clone());
    let rulesets = PostgresRuleSetStore::new(pool.clone());
    let target = publish(&rulesets, guild, 1).await;
    let id = create_request(&requests, "approval_quorum", &target, 2).await;

    let (first, second) = tokio::join!(
        requests.approve(&id, UserId(20)),
        requests.approve(&id, UserId(30))
    );
    let states = [first.unwrap().state, second.unwrap().state];

    assert_eq!(
        states
            .iter()
            .filter(|state| **state == ActivationRequestState::Approved)
            .count(),
        1
    );
    let stored = requests.get(&id).await.unwrap().unwrap();
    assert_eq!(stored.state, ActivationRequestState::Approved);
    assert_eq!(stored.approvals.len(), 2);
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn expiry_and_apply_claim_cannot_both_transition() {
    let pool = pool().await;
    let guild = GuildId(9_100_011);
    cleanup(&pool, guild).await;
    let requests = PostgresActivationRequestStore::new(pool.clone());
    let rulesets = PostgresRuleSetStore::new(pool.clone());
    let target = publish(&rulesets, guild, 1).await;
    let id = create_approved(&requests, "expiry_claim", &target).await;
    sqlx::query(
        "UPDATE activation_requests \
         SET expires_at = clock_timestamp() + INTERVAL '50 milliseconds' WHERE id = $1",
    )
    .bind(id.as_str())
    .execute(&pool)
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let (expired, claim) = tokio::join!(
        requests.mark_expired(&id),
        requests.claim_apply(&id, attempt_id("expiry_attempt"), 60)
    );

    assert!(expired.unwrap() || matches!(claim.as_ref().unwrap(), ClaimOutcome::Expired));
    let stored = requests.get(&id).await.unwrap().unwrap();
    assert_eq!(stored.state, ActivationRequestState::Expired);
    assert!(stored.apply_attempt_id.is_none());
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn stale_attempt_cannot_overwrite_new_attempt() {
    let pool = pool().await;
    let guild = GuildId(9_100_012);
    cleanup(&pool, guild).await;
    let requests = PostgresActivationRequestStore::new(pool.clone());
    let rulesets = PostgresRuleSetStore::new(pool.clone());
    let target = publish(&rulesets, guild, 1).await;
    let id = create_approved(&requests, "stale_attempt", &target).await;
    let stale = attempt_id("stale_owner");
    let current = attempt_id("current_owner");
    requests.claim_apply(&id, stale.clone(), 60).await.unwrap();
    sqlx::query(
        "UPDATE activation_requests SET apply_lease_until = NOW() - INTERVAL '1 second' WHERE id = $1",
    )
    .bind(id.as_str())
    .execute(&pool)
    .await
    .unwrap();
    assert!(matches!(
        requests
            .claim_resume(&id, current.clone(), 60)
            .await
            .unwrap(),
        ClaimOutcome::Claimed(_)
    ));

    assert!(!requests
        .complete_applied(&id, &stale, UserId(10), CompletionKind::Activated, None)
        .await
        .unwrap());
    assert!(!requests
        .release_to_approved(
            &id,
            &stale,
            ApplyErrorRecord {
                kind: ApplyFailureKind::Activation,
                message: "stale".to_string(),
            }
        )
        .await
        .unwrap());
    assert!(requests
        .complete_applied(&id, &current, UserId(10), CompletionKind::Activated, None)
        .await
        .unwrap());
    let stored = requests.get(&id).await.unwrap().unwrap();
    assert_eq!(stored.state, ActivationRequestState::Applied);
    assert_eq!(stored.apply_attempt_no, 2);
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn database_checks_reject_invalid_rows() {
    let pool = pool().await;
    let guild = GuildId(9_100_013);
    cleanup(&pool, guild).await;
    let base = "INSERT INTO activation_requests \
        (id, guild_id, ruleset_key, target_version, target_content_hash, requester_id, \
         required_approvals, state, expires_at) \
        VALUES ($1, $2, 'studyroom', 1, $3, '10', $4, $5, NOW() + INTERVAL '1 hour')";

    let invalid_quorum = sqlx::query(base)
        .bind("check_quorum")
        .bind(guild.to_string())
        .bind("11".repeat(32))
        .bind(0_i32)
        .bind("pending")
        .execute(&pool)
        .await;
    let invalid_applying = sqlx::query(base)
        .bind("check_applying")
        .bind(guild.to_string())
        .bind("22".repeat(32))
        .bind(1_i32)
        .bind("applying")
        .execute(&pool)
        .await;
    let invalid_applied = sqlx::query(base)
        .bind("check_applied")
        .bind(guild.to_string())
        .bind("33".repeat(32))
        .bind(1_i32)
        .bind("applied")
        .execute(&pool)
        .await;

    assert!(invalid_quorum.is_err());
    assert!(invalid_applying.is_err());
    assert!(invalid_applied.is_err());
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn reconnect_preserves_request_approvals_and_attempt() {
    let guild = GuildId(9_100_014);
    let id = request_id("reconnect_request");
    let attempt = attempt_id("reconnect_attempt");
    {
        let first_pool = pool().await;
        cleanup(&first_pool, guild).await;
        let requests = PostgresActivationRequestStore::new(first_pool.clone());
        let rulesets = PostgresRuleSetStore::new(first_pool.clone());
        let target = publish(&rulesets, guild, 1).await;
        create_approved(&requests, id.as_str(), &target).await;
        requests
            .claim_apply(&id, attempt.clone(), 60)
            .await
            .unwrap();
        first_pool.close().await;
    }

    let second_pool = pool().await;
    let requests = PostgresActivationRequestStore::new(second_pool.clone());
    let stored = requests.get(&id).await.unwrap().unwrap();

    assert_eq!(stored.state, ActivationRequestState::Applying);
    assert_eq!(stored.approvals.len(), 1);
    assert_eq!(stored.apply_attempt_id.as_ref(), Some(&attempt));
    assert!(requests
        .complete_applied(
            &id,
            &attempt,
            UserId(10),
            CompletionKind::Activated,
            Some(vec!["durable".to_string()])
        )
        .await
        .unwrap());
    let completed = requests.get(&id).await.unwrap().unwrap();
    assert_eq!(completed.state, ActivationRequestState::Applied);
    assert_eq!(
        completed.completion.unwrap().notices,
        Some(vec!["durable".to_string()])
    );
    cleanup(&second_pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn product_request_stays_inert_until_link_and_reconnects_with_exact_evidence() {
    let guild = GuildId(9_100_015);
    let id = request_id("product_reconnect");
    let context;
    {
        let first_pool = pool().await;
        cleanup(&first_pool, guild).await;
        let requests = PostgresActivationRequestStore::new(first_pool.clone());
        let rulesets = PostgresRuleSetStore::new(first_pool.clone());
        let target = publish(&rulesets, guild, 1).await;
        context = product_context(&id, &target, '1');
        prepare_product_promotion(&first_pool, &target, &context).await;
        let created = requests
            .create_product(product_input(id.clone(), &target, context.clone()))
            .await
            .unwrap();
        assert_eq!(created.link_state, ActivationLinkStateV1::Unlinked);
        assert_eq!(
            requests.approve(&id, UserId(20)).await.unwrap_err(),
            ApproveError::BoundApprovalRequired
        );
        assert_eq!(
            requests
                .approve_bound(&id, UserId(20), &context.approval_payload_digest)
                .await
                .unwrap_err(),
            ApproveError::BoundApprovalRequired
        );
        assert_eq!(
            requests
                .claim_apply(&id, attempt_id("product_unlinked"), 60)
                .await
                .unwrap(),
            ClaimOutcome::Unlinked
        );
        let mut wrong = product_link(&context);
        wrong.approval_context_digest = digest('e');
        assert_eq!(
            requests.link_product(&id, wrong).await.unwrap_err(),
            LinkProductError::Conflict
        );
        journal_product_link(&first_pool, &id, &target, &context).await;
        let linked = requests
            .link_product(&id, product_link(&context))
            .await
            .unwrap();
        assert!(matches!(
            linked.link_state,
            ActivationLinkStateV1::Linked { .. }
        ));
        assert_eq!(
            requests
                .link_product(&id, product_link(&context))
                .await
                .unwrap(),
            linked
        );
        let before = decision_snapshot(&first_pool, &id).await;
        assert_eq!(before, ("pending".to_string(), Some(1), 0));
        assert_eq!(
            requests.approve(&id, UserId(20)).await.unwrap_err(),
            ApproveError::BoundApprovalRequired
        );
        assert_eq!(
            requests
                .approve_bound(&id, UserId(20), &digest('f'))
                .await
                .unwrap_err(),
            ApproveError::BoundApprovalRequired
        );
        assert_eq!(
            requests
                .approve_bound(&id, UserId(20), &context.approval_payload_digest)
                .await
                .unwrap_err(),
            ApproveError::BoundApprovalRequired
        );
        assert_eq!(decision_snapshot(&first_pool, &id).await, before);
        seed_product_approval(&first_pool, &id, &context).await;
        requests
            .claim_apply(&id, attempt_id("product_apply"), 60)
            .await
            .unwrap();
        assert_eq!(requests.list_applying(guild).await.unwrap().len(), 1);
        first_pool.close().await;
    }

    let second_pool = pool().await;
    let requests = PostgresActivationRequestStore::new(second_pool.clone());
    let stored = requests.get(&id).await.unwrap().unwrap();
    assert!(matches!(
        stored.link_state,
        ActivationLinkStateV1::Linked { .. }
    ));
    assert_eq!(stored.approvals.len(), 1);
    assert_eq!(
        stored.approvals[0].approval_payload_digest.as_ref(),
        Some(&context.approval_payload_digest)
    );
    assert_eq!(stored.state, ActivationRequestState::Applying);
    cleanup(&second_pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn legacy_approval_adapter_cannot_race_product_link_or_mutate_product_request() {
    let pool = pool().await;
    let guild = GuildId(9_100_016);
    cleanup(&pool, guild).await;
    let requests = PostgresActivationRequestStore::new(pool.clone());
    let rulesets = PostgresRuleSetStore::new(pool.clone());
    let target = publish(&rulesets, guild, 1).await;
    let id = request_id("product_concurrent");
    let context = product_context(&id, &target, '2');
    prepare_product_promotion(&pool, &target, &context).await;
    requests
        .create_product(product_input(id.clone(), &target, context.clone()))
        .await
        .unwrap();
    journal_product_link(&pool, &id, &target, &context).await;

    let (link, approval) = tokio::join!(
        requests.link_product(&id, product_link(&context)),
        requests.approve_bound(&id, UserId(20), &context.approval_payload_digest)
    );
    assert!(link.is_ok());
    assert_eq!(approval.unwrap_err(), ApproveError::BoundApprovalRequired);
    let product_control_required = ActivationStoreError::InvalidRequest(
        "product activation requires authenticated product control".to_string(),
    );
    assert_eq!(
        requests
            .reject(&id, UserId(30), "reject".to_string())
            .await
            .unwrap_err(),
        RejectError::Store(product_control_required.clone())
    );
    assert_eq!(
        requests
            .withdraw(&id, UserId(10), "withdraw".to_string())
            .await
            .unwrap_err(),
        WithdrawError::Store(product_control_required)
    );
    let stored = requests.get(&id).await.unwrap().unwrap();
    assert_eq!(stored.state, ActivationRequestState::Pending);
    assert!(stored.approvals.is_empty());
    assert_eq!(
        decision_snapshot(&pool, &id).await,
        ("pending".to_string(), Some(1), 0)
    );

    let legacy_id = create_request(&requests, "legacy_after_product", &target, 1).await;
    let legacy = requests.approve(&legacy_id, UserId(20)).await.unwrap();
    assert_eq!(legacy.state, ActivationRequestState::Approved);
    assert_eq!(legacy.approvals.len(), 1);
    assert_eq!(
        decision_snapshot(&pool, &legacy_id).await,
        ("approved".to_string(), None, 1)
    );
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn database_blocks_legacy_writes_and_duplicate_requests_for_product_authority() {
    let pool = pool().await;
    let guild = GuildId(9_100_017);
    cleanup(&pool, guild).await;
    let requests = PostgresActivationRequestStore::new(pool.clone());
    let rulesets = PostgresRuleSetStore::new(pool.clone());
    let target = publish(&rulesets, guild, 1).await;
    let first_id = request_id("product_trigger_first");
    let context = product_context(&first_id, &target, '3');
    prepare_product_promotion(&pool, &target, &context).await;
    requests
        .create_product(product_input(first_id.clone(), &target, context.clone()))
        .await
        .unwrap();
    journal_product_link(&pool, &first_id, &target, &context).await;
    requests
        .link_product(&first_id, product_link(&context))
        .await
        .unwrap();

    let unbound = sqlx::query(
        "INSERT INTO activation_request_approvals \
         (request_id, approver_id, approved_at, approval_payload_digest) \
         VALUES ($1, '20', clock_timestamp(), $2)",
    )
    .bind(first_id.as_str())
    .bind(context.approval_payload_digest.as_str())
    .execute(&pool)
    .await;
    assert!(unbound.is_err());
    let before = decision_snapshot(&pool, &first_id).await;
    assert_eq!(before, ("pending".to_string(), Some(1), 0));
    assert_eq!(
        requests
            .approve_bound(&first_id, UserId(20), &context.approval_payload_digest)
            .await
            .unwrap_err(),
        ApproveError::BoundApprovalRequired
    );
    assert_eq!(decision_snapshot(&pool, &first_id).await, before);
    let unguarded_executor = sqlx::query(
        "UPDATE activation_requests SET state = 'applying', apply_attempt_id = 'old_binary', \
         apply_attempt_no = apply_attempt_no + 1, \
         apply_lease_until = clock_timestamp() + INTERVAL '1 minute' WHERE id = $1",
    )
    .bind(first_id.as_str())
    .execute(&pool)
    .await;
    assert!(unguarded_executor.is_err());
    assert_eq!(
        requests.get(&first_id).await.unwrap().unwrap().state,
        ActivationRequestState::Pending
    );

    let second_id = request_id("product_trigger_second");
    let mut duplicate_context = product_context(&second_id, &target, '3');
    duplicate_context.promotion_request_digest = digest('4');
    duplicate_context.approval_context_digest = product_approval_context_digest_v1(
        &second_id,
        &ActivationTarget {
            guild_id: target.guild_id,
            ruleset_key: target.ruleset_key.clone(),
            version: target.version,
            content_hash: target.content_hash,
        },
        UserId(10),
        &duplicate_context,
    );
    assert!(requests
        .create_product(product_input(second_id, &target, duplicate_context))
        .await
        .is_err());
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn guarded_product_apply_supersedes_a_second_request_from_the_same_baseline() {
    let pool = pool().await;
    let guild = GuildId(9_100_018);
    cleanup(&pool, guild).await;
    let requests = PostgresActivationRequestStore::new(pool.clone());
    let rulesets = CountingRuleSetStore::new(pool.clone());
    let first_target = publish(&rulesets, guild, 1).await;
    let second_target = publish(&rulesets, guild, 2).await;
    let (first_id, _) = create_approved_product(
        &pool,
        &requests,
        "guarded_product_first",
        &first_target,
        '4',
    )
    .await;
    let (second_id, _) = create_approved_product(
        &pool,
        &requests,
        "guarded_product_second",
        &second_target,
        '5',
    )
    .await;
    let service = ActivationService::new(&requests, &rulesets, &ReadyProvider);

    assert_eq!(
        service
            .apply(&first_id, attempt_id("guarded_product_a"), UserId(10))
            .await
            .unwrap(),
        ApplyOutcome::Activated
    );
    let second = service
        .apply(&second_id, attempt_id("guarded_product_b"), UserId(10))
        .await
        .unwrap();
    assert!(matches!(
        second,
        ApplyOutcome::Superseded {
            reason: SupersessionReasonV1::ActiveBaselineDrift { .. }
        }
    ));
    assert_eq!(rulesets.activate_calls(), 1);
    assert_eq!(
        rulesets
            .active(guild, &key())
            .await
            .unwrap()
            .unwrap()
            .version,
        first_target.version
    );
    let stored = requests.get(&second_id).await.unwrap().unwrap();
    assert_eq!(stored.state, ActivationRequestState::Superseded);
    assert_eq!(stored.approvals.len(), 1);
    assert!(matches!(
        service
            .apply(&second_id, attempt_id("guarded_product_replay"), UserId(10))
            .await
            .unwrap(),
        ApplyOutcome::Superseded { .. }
    ));
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn withdrawal_is_terminal_durable_and_database_constrained() {
    let pool = pool().await;
    let guild = GuildId(9_100_019);
    cleanup(&pool, guild).await;
    let requests = PostgresActivationRequestStore::new(pool.clone());
    let rulesets = PostgresRuleSetStore::new(pool.clone());
    let target = publish(&rulesets, guild, 1).await;
    let id = create_request(&requests, "withdraw_durable", &target, 1).await;

    let withdrawn = requests
        .withdraw(&id, UserId(10), "changed".to_string())
        .await
        .unwrap();
    assert_eq!(withdrawn.state, ActivationRequestState::Withdrawn);
    assert_eq!(
        requests
            .withdraw(&id, UserId(10), "again".to_string())
            .await
            .unwrap_err(),
        WithdrawError::InvalidState
    );
    assert_eq!(
        requests
            .claim_apply(&id, attempt_id("withdraw_claim"), 60)
            .await
            .unwrap(),
        ClaimOutcome::NotApproved
    );
    assert_eq!(
        requests.get(&id).await.unwrap().unwrap().state,
        ActivationRequestState::Withdrawn
    );

    let invalid_terminal = sqlx::query(
        "UPDATE activation_requests SET state = 'superseded', termination = NULL WHERE id = $1",
    )
    .bind(id.as_str())
    .execute(&pool)
    .await;
    assert!(invalid_terminal.is_err());
    cleanup(&pool, guild).await;
}
