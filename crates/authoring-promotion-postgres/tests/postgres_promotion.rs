use std::collections::{BTreeMap, VecDeque};
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use authoring_promotion::{
    ApprovalPolicyV1, AuthenticatedPromotionContext, AuthoringSessionId, AutomationInstallationId,
    BindingRevision, CreatePromotionOutcomeV1, EnsurePendingActivationV1, IdempotencyKey,
    ManualPromotionClock, PendingActivationDispositionV1, PendingActivationPort,
    PendingActivationPortError, PendingActivationReceiptV1, PolicyRevision, PrincipalId,
    PromotionError, PromotionRecordV1, PromotionService, PromotionStageV1, PromotionStore,
    PromotionStoreError, PublicationDispositionV1, PublicationPortOutcomeV1,
    PublishAuthoringRuleSetV1, ResumePromotionOutcomeV1, RuleSetPublicationPort, SessionGeneration,
    StartPromotionV1, TenantId,
};
use authoring_promotion_postgres::{PostgresPromotionStore, MIGRATOR};
use automation_ruleset::{InMemoryRuleSetStore, RuleSetStore, RuleSetStoreError};
use automation_ruleset_activation::{
    ActivationRequest, ActivationRequestId, CreateActivationRequest,
};
use chrono::{DateTime, TimeZone, Utc};
use design_harness::{
    BurstOutcome, DesignSession, LlmClient, LlmError, LlmResponse, Message, PreviewReadyArtifactV1,
    ResourceBindingMap, ToolCall, ToolDefinition,
};
use discord_model::{GuildId, UserId};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
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
    let mut bindings = ResourceBindingMap::default();
    bindings.channel_bindings.insert(
        serde_json::from_value(json!("community_hub")).unwrap(),
        "700".parse().unwrap(),
    );
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
    async fn ensure_pending_activation(
        &self,
        _request: EnsurePendingActivationV1,
    ) -> Result<PendingActivationReceiptV1, PendingActivationPortError> {
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
    async fn ensure_pending_activation(
        &self,
        input: EnsurePendingActivationV1,
    ) -> Result<PendingActivationReceiptV1, PendingActivationPortError> {
        if self.fail_once.swap(false, Ordering::SeqCst) {
            return Err(PendingActivationPortError::Backend("injected".to_string()));
        }
        let mut requests = self.requests.lock().unwrap();
        if let Some(existing) = requests.get(&input.id) {
            let exact = existing.target == input.target
                && existing.requester == input.requester
                && existing.required_approvals == input.required_approvals.get()
                && existing.expires_at - existing.created_at == input.ttl;
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
        let request = ActivationRequest::create(
            CreateActivationRequest {
                id: input.id,
                target: input.target,
                requester: input.requester,
                required_approvals: input.required_approvals.get(),
                ttl: input.ttl,
                observed_active: None,
            },
            fixed_now(),
        )
        .unwrap();
        requests.insert(request.id.clone(), request.clone());
        Ok(PendingActivationReceiptV1 {
            request,
            disposition: PendingActivationDispositionV1::Created,
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

async fn cleanup(pool: &PgPool, tenant: &str) {
    sqlx::query("DELETE FROM authoring_promotions WHERE tenant_id = $1")
        .bind(tenant)
        .execute(pool)
        .await
        .unwrap();
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
