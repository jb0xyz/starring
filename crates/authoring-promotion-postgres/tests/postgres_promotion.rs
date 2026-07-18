use std::collections::{BTreeMap, VecDeque};
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use authoring_promotion::{
    ApprovalPolicyV1, AuthenticatedPromotionContext, AuthoringSessionId, AutomationInstallationId,
    BindingRevision, CreatePromotionOutcomeV1, EnsurePendingActivationV1, IdempotencyKey,
    LinkPendingActivationV1, ManualPromotionClock, PendingActivationDispositionV1,
    PendingActivationPort, PendingActivationPortError, PendingActivationReceiptV1, PolicyRevision,
    PrincipalId, ProductActivationBridge, ProductApprovalEnvironmentError,
    ProductApprovalEnvironmentProvider, ProductApprovalEnvironmentV1, PromotionError,
    PromotionRecordV1, PromotionService, PromotionStageV1, PromotionStore, PromotionStoreError,
    PublicationDispositionV1, PublicationPortOutcomeV1, PublishAuthoringRuleSetV1,
    ResolveProductApprovalContextV1, ResolvedProductApprovalContextV1, ResumePromotionOutcomeV1,
    RuleSetPublicationPort, SessionGeneration, StartPromotionV1, TenantId, UtcPromotionClock,
};
use authoring_promotion_postgres::{PostgresPromotionStore, MIGRATOR};
use automation_ruleset::{
    InMemoryRuleSetStore, PublishOutcome, PublishRuleSetRequest, RuleSetStore, RuleSetStoreError,
};
use automation_ruleset_activation::{
    approval_policy_digest_v1, product_approval_context_digest_v1, ActivationApprovalContextV1,
    ActivationDigest, ActivationEnvironment, ActivationEnvironmentError,
    ActivationEnvironmentProvider, ActivationLinkStateV1, ActivationPromotionId, ActivationRequest,
    ActivationRequestId, ActivationRequestState, ActivationRequestStore, ActivationService,
    ActivationStoreError, ActivationTarget, ApplyAttemptId, ApplyOutcome, ApprovalBindingContextV1,
    ApprovalPolicyBindingV1, CreateProductActivationRequest, ExpectedActiveBaselineV1,
    LinkProductActivation, LinkProductError, ProductApprovalContextV1,
};
use automation_ruleset_activation_postgres::PostgresActivationRequestStore;
use automation_ruleset_postgres::PostgresRuleSetStore;
use automation_ruleset_readiness::GuildCapabilities;
use chrono::{DateTime, TimeZone, Utc};
use design_harness::{
    BurstOutcome, DesignSession, LlmClient, LlmError, LlmResponse, Message, PreviewReadyArtifactV1,
    ResourceBindingMap, ToolCall, ToolDefinition,
};
use discord_model::{ChannelId, GuildId, Permissions, UserId};
use resource_resolution::{approval_binding_fingerprint_v1, ResolvedApprovalBinding};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::Json;
use sqlx::PgPool;

#[derive(Clone)]
struct ScriptedClient {
    responses: Arc<Mutex<VecDeque<Result<LlmResponse, LlmError>>>>,
}

impl ScriptedClient {
    fn validated_preview() -> Self {
        let response = LlmResponse::ToolCalls(vec![ToolCall {
            id: "interpret".to_string(),
            name: "interpret_intent_core".to_string(),
            arguments: json!({
                "expected_revision": 0,
                "request_mode": "build",
                "automation_kind": "managed_private_study_room",
                "requested_outcome": "validated_preview",
                "hub_channel": "community_hub",
                "language": "en",
                "close_policy": "disabled",
                "other_unmapped_required_capabilities": [],
                "response": ""
            })
            .to_string(),
        }]);
        Self {
            responses: Arc::new(Mutex::new(vec![Ok(response)].into())),
        }
    }
}

impl LlmClient for ScriptedClient {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
    ) -> Result<LlmResponse, LlmError> {
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("scripted response")
    }
}

async fn artifact() -> PreviewReadyArtifactV1 {
    let bindings = product_bindings();
    let mut session =
        DesignSession::with_intent_recipe(ScriptedClient::validated_preview(), bindings);
    assert!(matches!(
        session
            .run_burst(
                "Create private study rooms in community_hub and prepare a validated preview"
            )
            .await,
        BurstOutcome::Ready { .. }
    ));
    session.export_preview_ready_artifact().unwrap()
}

fn product_bindings() -> ResourceBindingMap {
    let mut bindings = ResourceBindingMap::default();
    bindings.channel_bindings.insert(
        serde_json::from_value(json!("community_hub")).unwrap(),
        "700".parse().unwrap(),
    );
    bindings
}

fn fixed_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0)
        .single()
        .unwrap()
}

fn context(tenant: &str) -> AuthenticatedPromotionContext {
    AuthenticatedPromotionContext {
        tenant_id: TenantId::parse(tenant).unwrap(),
        principal_id: PrincipalId::parse("postgres-principal").unwrap(),
        session_owner_id: PrincipalId::parse("postgres-principal").unwrap(),
        session_id: AuthoringSessionId::parse("postgres-session").unwrap(),
        session_generation: SessionGeneration::new(1).unwrap(),
        guild_id: GuildId(9_200_001),
        installation_id: AutomationInstallationId::parse("postgres-installation").unwrap(),
        ruleset_key: "studyrooms".parse().unwrap(),
        requester: UserId(100),
        binding_revision: BindingRevision::new(1).unwrap(),
        policy: ApprovalPolicyV1 {
            revision: PolicyRevision::new(1).unwrap(),
            required_approvals: NonZeroU32::new(1).unwrap(),
            ttl_seconds: NonZeroU64::new(3600).unwrap(),
        },
    }
}

fn input(key: &str, tenant: &str, artifact: &PreviewReadyArtifactV1) -> StartPromotionV1 {
    StartPromotionV1 {
        idempotency_key: IdempotencyKey::parse(key).unwrap(),
        context: context(tenant),
        artifact: artifact.clone(),
    }
}

struct UnusedPorts;

impl RuleSetPublicationPort for UnusedPorts {
    async fn publish_ruleset(
        &self,
        _request: PublishAuthoringRuleSetV1,
    ) -> Result<PublicationPortOutcomeV1, RuleSetStoreError> {
        panic!("publication must not be called")
    }
}

impl PendingActivationPort for UnusedPorts {
    async fn resolve_product_approval_context(
        &self,
        _request: ResolveProductApprovalContextV1,
    ) -> Result<ResolvedProductApprovalContextV1, PendingActivationPortError> {
        panic!("activation must not be called")
    }

    async fn ensure_pending_activation(
        &self,
        _request: EnsurePendingActivationV1,
    ) -> Result<PendingActivationReceiptV1, PendingActivationPortError> {
        panic!("activation must not be called")
    }

    async fn link_pending_activation(
        &self,
        _request: LinkPendingActivationV1,
    ) -> Result<ActivationRequest, PendingActivationPortError> {
        panic!("activation must not be called")
    }
}

struct PendingPort {
    fail_once: AtomicBool,
    requests: Mutex<BTreeMap<ActivationRequestId, ActivationRequest>>,
}

impl PendingPort {
    fn new() -> Self {
        Self {
            fail_once: AtomicBool::new(false),
            requests: Mutex::new(BTreeMap::new()),
        }
    }
}

impl PendingActivationPort for PendingPort {
    async fn resolve_product_approval_context(
        &self,
        input: ResolveProductApprovalContextV1,
    ) -> Result<ResolvedProductApprovalContextV1, PendingActivationPortError> {
        let revision = NonZeroU64::new(input.binding_revision.get()).unwrap();
        let required_bindings = input
            .required_channel_bindings
            .into_iter()
            .map(|key| ResolvedApprovalBinding::Channel {
                key: serde_json::from_value(json!(key)).unwrap(),
                id: ChannelId(700),
            })
            .collect::<Vec<_>>();
        Ok(ResolvedProductApprovalContextV1 {
            binding: ApprovalBindingContextV1 {
                revision,
                fingerprint: approval_binding_fingerprint_v1(
                    input.target.guild_id,
                    revision,
                    &required_bindings,
                )
                .unwrap(),
                required_bindings,
            },
            baseline: ExpectedActiveBaselineV1::Absent,
        })
    }

    async fn ensure_pending_activation(
        &self,
        input: EnsurePendingActivationV1,
    ) -> Result<PendingActivationReceiptV1, PendingActivationPortError> {
        if self.fail_once.swap(false, Ordering::SeqCst) {
            return Err(PendingActivationPortError::Backend("injected".to_string()));
        }
        let mut requests = self.requests.lock().unwrap();
        if let Some(existing) = requests.get(&input.create.id) {
            let exact = existing.target == input.create.target
                && existing.requester == input.create.requester
                && existing.required_approvals
                    == input.create.context.policy.required_approvals.get()
                && existing.approval_context
                    == (ActivationApprovalContextV1::ProductAuthoring {
                        context: Box::new(input.create.context.clone()),
                    });
            if !exact {
                return Err(PendingActivationPortError::Conflict(
                    "request mismatch".to_string(),
                ));
            }
            return Ok(PendingActivationReceiptV1 {
                request: existing.clone(),
                disposition: PendingActivationDispositionV1::Reused,
            });
        }
        let request = ActivationRequest::create_product(input.create, fixed_now()).unwrap();
        requests.insert(request.id.clone(), request.clone());
        Ok(PendingActivationReceiptV1 {
            request,
            disposition: PendingActivationDispositionV1::Created,
        })
    }

    async fn link_pending_activation(
        &self,
        input: LinkPendingActivationV1,
    ) -> Result<ActivationRequest, PendingActivationPortError> {
        let mut requests = self.requests.lock().unwrap();
        let request = requests.get_mut(&input.request_id).ok_or_else(|| {
            PendingActivationPortError::Indeterminate("request disappeared".to_string())
        })?;
        match request.link_product_at(
            &input.link.promotion_id,
            &input.link.promotion_request_digest,
            &input.link.approval_context_digest,
            fixed_now(),
        ) {
            Ok(_) | Err(automation_ruleset_activation::LinkDecisionError::Expired) => {
                Ok(request.clone())
            }
            Err(error) => Err(PendingActivationPortError::Conflict(error.to_string())),
        }
    }
}

#[derive(Clone)]
struct ReadyProductEnvironment {
    revision: NonZeroU64,
    bindings: ResourceBindingMap,
}

impl ProductApprovalEnvironmentProvider for ReadyProductEnvironment {
    async fn load_fresh(
        &self,
        request: &ResolveProductApprovalContextV1,
    ) -> Result<ProductApprovalEnvironmentV1, ProductApprovalEnvironmentError> {
        assert_eq!(request.tenant_id.as_str(), "postgres-end-to-end");
        assert_eq!(request.installation_id.as_str(), "postgres-installation");
        Ok(ProductApprovalEnvironmentV1 {
            binding_revision: self.revision,
            bindings: self.bindings.clone(),
        })
    }
}

impl ActivationEnvironmentProvider for ReadyProductEnvironment {
    async fn load_fresh(
        &self,
        _target: &automation_ruleset_activation::ActivationTarget,
    ) -> Result<ActivationEnvironment, ActivationEnvironmentError> {
        Ok(ActivationEnvironment {
            binding_revision: Some(self.revision),
            bindings: self.bindings.clone(),
            guild_capabilities: GuildCapabilities {
                base_permissions: Permissions::ADMINISTRATOR,
            },
            role_permissions: BTreeMap::new(),
        })
    }
}

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

fn is_check_violation(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(database) if database.code().as_deref() == Some("23514")
    )
}

async fn cleanup(pool: &PgPool, tenant: &str) {
    sqlx::query("DELETE FROM authoring_promotions WHERE tenant_id = $1")
        .bind(tenant)
        .execute(pool)
        .await
        .unwrap();
}

async fn cleanup_product(pool: &PgPool, tenant: &str, guild_id: GuildId) {
    cleanup(pool, tenant).await;
    let guild_id = guild_id.to_string();
    sqlx::query("DELETE FROM activation_requests WHERE guild_id = $1")
        .bind(&guild_id)
        .execute(pool)
        .await
        .unwrap();
    for table in [
        "automation_ruleset_activations",
        "automation_ruleset_versions",
        "automation_ruleset_heads",
    ] {
        sqlx::query(&format!("DELETE FROM {table} WHERE guild_id = $1"))
            .bind(&guild_id)
            .execute(pool)
            .await
            .unwrap();
    }
}

async fn create_prepared(
    store: &PostgresPromotionStore,
    tenant: &str,
    key: &str,
    sealed: &PreviewReadyArtifactV1,
) -> PromotionRecordV1 {
    let service = PromotionService::new(
        store,
        &UnusedPorts,
        &UnusedPorts,
        ManualPromotionClock::new(fixed_now()),
    );
    let CreatePromotionOutcomeV1::Created(record) =
        service.start(input(key, tenant, sealed)).await.unwrap()
    else {
        panic!("expected created promotion")
    };
    record
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn database_rejects_direct_product_link_without_promotion_journal() {
    let tenant = "postgres-direct-link-gate";
    let guild_id = GuildId(9_200_002);
    let promotion_id = ActivationPromotionId::parse(&"f".repeat(64)).unwrap();
    let pool = pool().await;
    cleanup_product(&pool, tenant, guild_id).await;
    sqlx::query("DELETE FROM authoring_promotions WHERE id = $1")
        .bind(promotion_id.as_str())
        .execute(&pool)
        .await
        .unwrap();
    let rulesets = PostgresRuleSetStore::new(pool.clone());
    let published = rulesets
        .publish(PublishRuleSetRequest {
            guild_id,
            ruleset_key: "direct-link-gate".parse().unwrap(),
            definition: artifact().await.ruleset().clone(),
            created_by: UserId(100),
        })
        .await
        .unwrap();
    let published = match published {
        PublishOutcome::Created(published) | PublishOutcome::Reused(published) => published,
    };
    let target = ActivationTarget {
        guild_id,
        ruleset_key: published.ruleset_key,
        version: published.version,
        content_hash: published.content_hash,
    };
    let request_id = ActivationRequestId::parse("direct_product_without_journal").unwrap();
    let binding_revision = NonZeroU64::new(1).unwrap();
    let policy_revision = NonZeroU64::new(1).unwrap();
    let required_approvals = NonZeroU32::new(1).unwrap();
    let ttl_seconds = NonZeroU64::new(3600).unwrap();
    let mut approval_context = ProductApprovalContextV1 {
        promotion_id,
        promotion_request_digest: ActivationDigest::parse(&"d".repeat(64)).unwrap(),
        approval_payload_digest: ActivationDigest::parse(&"e".repeat(64)).unwrap(),
        approval_context_digest: ActivationDigest::parse(&"0".repeat(64)).unwrap(),
        binding: ApprovalBindingContextV1 {
            revision: binding_revision,
            required_bindings: Vec::new(),
            fingerprint: approval_binding_fingerprint_v1(guild_id, binding_revision, &[]).unwrap(),
        },
        baseline: ExpectedActiveBaselineV1::Absent,
        policy: ApprovalPolicyBindingV1 {
            revision: policy_revision,
            required_approvals,
            ttl_seconds,
            digest: approval_policy_digest_v1(policy_revision, required_approvals, ttl_seconds),
        },
    };
    approval_context.approval_context_digest =
        product_approval_context_digest_v1(&request_id, &target, UserId(100), &approval_context);
    let requests = PostgresActivationRequestStore::new(pool.clone());
    let created = requests
        .create_product(CreateProductActivationRequest {
            id: request_id.clone(),
            target: target.clone(),
            requester: UserId(100),
            context: approval_context.clone(),
        })
        .await
        .unwrap();
    assert_eq!(created.link_state, ActivationLinkStateV1::Unlinked);
    let authority_rewrite = sqlx::query(
        "UPDATE activation_requests SET authority_kind = 'legacy_manual', \
         link_state_name = 'not_required', approval_context = '{\"authority\":\"legacy_manual\"}', \
         link_state = '{\"state\":\"not_required\"}', promotion_id = NULL, \
         promotion_request_digest = NULL, approval_payload_digest = NULL, \
         approval_context_digest = NULL, linked_at = NULL WHERE id = $1",
    )
    .bind(request_id.as_str())
    .execute(&pool)
    .await;
    assert!(is_check_violation(&authority_rewrite.unwrap_err()));
    let error = requests
        .link_product(
            &request_id,
            LinkProductActivation {
                promotion_id: approval_context.promotion_id.clone(),
                promotion_request_digest: approval_context.promotion_request_digest.clone(),
                approval_context_digest: approval_context.approval_context_digest.clone(),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        LinkProductError::Store(ActivationStoreError::Backend(_))
    ));
    assert_eq!(
        requests.get(&request_id).await.unwrap().unwrap().link_state,
        ActivationLinkStateV1::Unlinked
    );

    let raw_request_id = ActivationRequestId::parse("direct_linked_insert").unwrap();
    let mut raw_context = approval_context;
    raw_context.promotion_id = ActivationPromotionId::parse(&"a".repeat(64)).unwrap();
    raw_context.promotion_request_digest = ActivationDigest::parse(&"b".repeat(64)).unwrap();
    raw_context.approval_payload_digest = ActivationDigest::parse(&"c".repeat(64)).unwrap();
    raw_context.approval_context_digest =
        product_approval_context_digest_v1(&raw_request_id, &target, UserId(100), &raw_context);
    let raw_created_at = Utc::now();
    let raw_linked_at = raw_created_at + chrono::Duration::milliseconds(1);
    let raw_expires_at = raw_created_at + chrono::Duration::seconds(3600);
    let raw_link_state = ActivationLinkStateV1::Linked {
        linked_at: raw_linked_at,
    };
    let raw_approval_context = ActivationApprovalContextV1::ProductAuthoring {
        context: Box::new(raw_context.clone()),
    };
    let raw_insert = sqlx::query(
        "INSERT INTO activation_requests \
         (id, guild_id, ruleset_key, target_version, target_content_hash, requester_id, \
          required_approvals, state, created_at, expires_at, authority_kind, link_state_name, \
          approval_context, link_state, promotion_id, promotion_request_digest, \
          approval_payload_digest, approval_context_digest, linked_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', $8, $9, 'product_authoring', \
          'linked', $10, $11, $12, $13, $14, $15, $16)",
    )
    .bind(raw_request_id.as_str())
    .bind(guild_id.to_string())
    .bind(target.ruleset_key.as_str())
    .bind(i64::from(target.version.get()))
    .bind(target.content_hash.to_hex())
    .bind(UserId(100).to_string())
    .bind(i32::try_from(required_approvals.get()).unwrap())
    .bind(raw_created_at)
    .bind(raw_expires_at)
    .bind(Json(raw_approval_context))
    .bind(Json(raw_link_state))
    .bind(raw_context.promotion_id.as_str())
    .bind(raw_context.promotion_request_digest.as_str())
    .bind(raw_context.approval_payload_digest.as_str())
    .bind(raw_context.approval_context_digest.as_str())
    .bind(raw_linked_at)
    .execute(&pool)
    .await;
    assert!(is_check_violation(&raw_insert.unwrap_err()));
    assert!(requests.get(&raw_request_id).await.unwrap().is_none());
    cleanup_product(&pool, tenant, guild_id).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn create_reconnect_exact_replay_and_conflict_are_durable() {
    let tenant = "postgres-reconnect";
    let sealed = artifact().await;
    let first_pool = pool().await;
    cleanup(&first_pool, tenant).await;
    let first_store = PostgresPromotionStore::new(first_pool.clone());
    let created = create_prepared(&first_store, tenant, "reconnect-key", &sealed).await;
    drop(first_store);
    first_pool.close().await;

    let second_pool = pool().await;
    let second_store = PostgresPromotionStore::new(second_pool.clone());
    assert_eq!(
        second_store.get(&created.id).await.unwrap().unwrap(),
        created
    );
    let service = PromotionService::new(
        &second_store,
        &UnusedPorts,
        &UnusedPorts,
        ManualPromotionClock::new(fixed_now()),
    );
    assert!(matches!(
        service
            .start(input("reconnect-key", tenant, &sealed))
            .await
            .unwrap(),
        CreatePromotionOutcomeV1::ExactReplay(existing) if existing == created
    ));
    let mut conflict = input("reconnect-key", tenant, &sealed);
    conflict.context.requester = UserId(101);
    assert_eq!(
        service.start(conflict).await.unwrap_err(),
        PromotionError::Store(PromotionStoreError::IdempotencyConflict)
    );
    cleanup(&second_pool, tenant).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn concurrent_create_and_publication_cas_choose_one_winner() {
    let tenant = "postgres-concurrency";
    let sealed = artifact().await;
    let pool = pool().await;
    cleanup(&pool, tenant).await;
    let first_store = PostgresPromotionStore::new(pool.clone());
    let second_store = PostgresPromotionStore::new(pool.clone());
    let first_service = PromotionService::new(
        &first_store,
        &UnusedPorts,
        &UnusedPorts,
        ManualPromotionClock::new(fixed_now()),
    );
    let second_service = PromotionService::new(
        &second_store,
        &UnusedPorts,
        &UnusedPorts,
        ManualPromotionClock::new(fixed_now()),
    );
    let (first, second) = tokio::join!(
        first_service.start(input("concurrent-key", tenant, &sealed)),
        second_service.start(input("concurrent-key", tenant, &sealed))
    );
    let outcomes = [first.unwrap(), second.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, CreatePromotionOutcomeV1::Created(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, CreatePromotionOutcomeV1::ExactReplay(_)))
            .count(),
        1
    );
    let prepared = match &outcomes[0] {
        CreatePromotionOutcomeV1::Created(record)
        | CreatePromotionOutcomeV1::ExactReplay(record) => record.clone(),
    };
    let publication = authoring_promotion::PublicationRecordV1 {
        version: automation_ruleset::RuleSetVersionId::FIRST,
        schema_version: prepared.intent.registry_schema_version,
        content_hash: prepared.intent.expected_registry_content_hash,
        disposition: PublicationDispositionV1::Created,
        registry_created_by: prepared.intent.authority.requester,
    };
    let (first, second) = tokio::join!(
        first_store.mark_published(
            &prepared.id,
            prepared.revision,
            publication.clone(),
            fixed_now()
        ),
        second_store.mark_published(&prepared.id, prepared.revision, publication, fixed_now())
    );
    assert_ne!(first.is_ok(), second.is_ok());
    let loser = if first.is_err() { first } else { second };
    assert_eq!(
        loser.unwrap_err(),
        PromotionStoreError::RevisionConflict {
            current: authoring_promotion::PromotionRevision::new(2).unwrap()
        }
    );
    let stored = first_store.get(&prepared.id).await.unwrap().unwrap();
    assert!(matches!(stored.stage, PromotionStageV1::Published { .. }));
    cleanup(&pool, tenant).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn crash_resume_reuses_publication_and_survives_reconnect() {
    let tenant = "postgres-crash-resume";
    let sealed = artifact().await;
    let first_pool = pool().await;
    cleanup(&first_pool, tenant).await;
    let first_store = PostgresPromotionStore::new(first_pool.clone());
    let prepared = create_prepared(&first_store, tenant, "crash-key", &sealed).await;
    let registry = InMemoryRuleSetStore::default();
    let pending = PendingPort::new();
    registry
        .publish(automation_ruleset::PublishRuleSetRequest {
            guild_id: prepared.intent.authority.guild_id,
            ruleset_key: prepared.intent.authority.ruleset_key.clone(),
            definition: prepared.intent.definition.clone(),
            created_by: prepared.intent.authority.requester,
        })
        .await
        .unwrap();
    pending.fail_once.store(true, Ordering::SeqCst);
    let first_service = PromotionService::new(
        &first_store,
        &registry,
        &pending,
        ManualPromotionClock::new(fixed_now()),
    );
    assert!(matches!(
        first_service
            .resume_to_activation_pending(&prepared.id)
            .await,
        Err(PromotionError::PendingActivation(
            PendingActivationPortError::Backend(_)
        ))
    ));
    assert!(matches!(
        first_store.get(&prepared.id).await.unwrap().unwrap().stage,
        PromotionStageV1::Published { .. }
    ));
    drop(first_service);
    drop(first_store);
    first_pool.close().await;

    let second_pool = pool().await;
    let second_store = PostgresPromotionStore::new(second_pool.clone());
    let second_service = PromotionService::new(
        &second_store,
        &registry,
        &pending,
        ManualPromotionClock::new(fixed_now()),
    );
    let ResumePromotionOutcomeV1::Advanced(final_record) = second_service
        .resume_to_activation_pending(&prepared.id)
        .await
        .unwrap()
    else {
        panic!("expected activation pending")
    };
    assert!(matches!(
        final_record.stage,
        PromotionStageV1::ActivationPending { .. }
    ));
    assert_eq!(pending.requests.lock().unwrap().len(), 1);
    assert_eq!(
        registry
            .list_versions(
                prepared.intent.authority.guild_id,
                &prepared.intent.authority.ruleset_key,
            )
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(registry
        .active(
            prepared.intent.authority.guild_id,
            &prepared.intent.authority.ruleset_key,
        )
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        second_store.get(&prepared.id).await.unwrap().unwrap(),
        final_record
    );
    cleanup(&second_pool, tenant).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn sealed_authoring_reaches_bound_approval_and_guarded_deployment() {
    let tenant = "postgres-end-to-end";
    let guild_id = context(tenant).guild_id;
    let sealed = artifact().await;
    let pool = pool().await;
    cleanup_product(&pool, tenant, guild_id).await;
    let promotions = PostgresPromotionStore::new(pool.clone());
    let rulesets = PostgresRuleSetStore::new(pool.clone());
    let requests = PostgresActivationRequestStore::new(pool.clone());
    let environment = ReadyProductEnvironment {
        revision: NonZeroU64::new(1).unwrap(),
        bindings: product_bindings(),
    };
    let bridge = ProductActivationBridge::new(&rulesets, &environment, &requests, &promotions);
    let promotion_service =
        PromotionService::new(&promotions, &rulesets, &bridge, UtcPromotionClock);
    let CreatePromotionOutcomeV1::Created(prepared) = promotion_service
        .start(input("end-to-end-key", tenant, &sealed))
        .await
        .unwrap()
    else {
        panic!("expected prepared promotion")
    };
    let ResumePromotionOutcomeV1::Advanced(journaled) = promotion_service
        .resume_to_activation_pending(&prepared.id)
        .await
        .unwrap()
    else {
        panic!("expected linked product activation")
    };
    let PromotionStageV1::ActivationPending {
        publication,
        activation,
    } = &journaled.stage
    else {
        panic!("expected activation pending")
    };
    let pending = requests.get(&activation.request_id).await.unwrap().unwrap();
    assert_eq!(pending.state, ActivationRequestState::Pending);
    assert!(matches!(
        pending.link_state,
        automation_ruleset_activation::ActivationLinkStateV1::Linked { .. }
    ));
    assert!(matches!(
        pending.approval_context,
        ActivationApprovalContextV1::ProductAuthoring { .. }
    ));
    let changed_request_digest = "a".repeat(64);
    let identity_rewrite = sqlx::query(
        "UPDATE activation_requests SET promotion_request_digest = $2, \
         approval_context = jsonb_set(approval_context, \
         '{context,promotion_request_digest}', to_jsonb($2::TEXT)) WHERE id = $1",
    )
    .bind(activation.request_id.as_str())
    .bind(changed_request_digest)
    .execute(&pool)
    .await;
    assert!(is_check_violation(&identity_rewrite.unwrap_err()));
    let approved = requests
        .approve_bound(
            &activation.request_id,
            UserId(200),
            &activation.approval_context.approval_payload_digest,
        )
        .await
        .unwrap();
    assert_eq!(approved.state, ActivationRequestState::Approved);
    let activation_service = ActivationService::new(&requests, &rulesets, &environment);
    let outcome = activation_service
        .apply(
            &activation.request_id,
            ApplyAttemptId::parse("authoring_end_to_end_apply").unwrap(),
            UserId(300),
        )
        .await
        .unwrap();
    assert_eq!(outcome, ApplyOutcome::Activated);
    let active = rulesets
        .active(guild_id, &journaled.intent.authority.ruleset_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active.version, publication.version);
    assert_eq!(active.content_hash, publication.content_hash);
    assert_eq!(
        requests
            .get(&activation.request_id)
            .await
            .unwrap()
            .unwrap()
            .state,
        ActivationRequestState::Applied
    );
    cleanup_product(&pool, tenant, guild_id).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn corrupted_json_and_shadow_identity_fail_closed() {
    let tenant = "postgres-corruption";
    let sealed = artifact().await;
    let pool = pool().await;
    cleanup(&pool, tenant).await;
    let store = PostgresPromotionStore::new(pool.clone());
    let prepared = create_prepared(&store, tenant, "corruption-key", &sealed).await;
    sqlx::query(
        "UPDATE authoring_promotions SET record = jsonb_set(record, '{unexpected}', '1') \
         WHERE id = $1",
    )
    .bind(prepared.id.as_str())
    .execute(&pool)
    .await
    .unwrap();
    assert!(matches!(
        store.get(&prepared.id).await,
        Err(PromotionStoreError::Backend(_))
    ));
    let shadow_tamper =
        sqlx::query("UPDATE authoring_promotions SET request_digest = $2 WHERE id = $1")
            .bind(prepared.id.as_str())
            .bind("11".repeat(32))
            .execute(&pool)
            .await;
    assert!(shadow_tamper.is_err());
    cleanup(&pool, tenant).await;
}
