use std::collections::{BTreeMap, VecDeque};
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};

use authoring_promotion::{
    approval_payload_digest_v1, derive_promotion_identity_from_secret_v1,
    derive_promotion_identity_v1, plan_activation_link_v1, plan_approval_environment_v1,
    plan_pending_activation_v1, plan_ruleset_publication_v1, plan_start_promotion_v1,
    validate_exact_planned_record_v1, ApprovalPolicyV1, AuthenticatedPromotionContext,
    AuthoringSessionId, AutomationInstallationId, BindingRevision, CreatePromotionOutcomeV1,
    EnsurePendingActivationV1, IdempotencyKey, InMemoryPromotionStore, LinkPendingActivationV1,
    LinkedActivationTransitionV1, ManualPromotionClock, PendingActivationDispositionV1,
    PendingActivationPort, PendingActivationPortError, PendingActivationReceiptV1,
    PendingActivationTransitionV1, PolicyRevision, PrincipalId, PromotionError, PromotionId,
    PromotionRecordV1, PromotionRecordValidationError, PromotionService, PromotionStageV1,
    PromotionStore, PromotionStoreError, PublicationDispositionV1, PublicationPortOutcomeV1,
    PublicationRecordV1, PublishAuthoringRuleSetV1, PublishedAuthoringRuleSetV1,
    ResolveProductApprovalContextV1, ResolvedProductApprovalContextV1, ResumePromotionOutcomeV1,
    RuleSetPublicationPort, SessionGeneration, StartPromotionV1, TenantId,
};
use automation_ruleset::{
    InMemoryRuleSetStore, PublishOutcome, PublishRuleSetRequest, RuleSetStore, RuleSetStoreError,
};
use automation_ruleset_activation::{
    ActivationApprovalContextV1, ActivationLinkStateV1, ActivationRequest, ActivationRequestId,
    ApprovalBindingContextV1, ExpectedActiveBaselineV1, LinkProductActivation,
};
use chrono::{DateTime, TimeZone, Utc};
use design_harness::{
    BurstOutcome, DesignSession, LlmClient, LlmError, LlmResponse, Message, PreviewReadyArtifactV1,
    ResourceBindingMap, ToolCall, ToolDefinition,
};
use discord_model::{ChannelId, GuildId, UserId};
use futures::executor::block_on;
use resource_resolution::{approval_binding_fingerprint_v1, ResolvedApprovalBinding};
use serde_json::json;

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

async fn artifact(validated_preview: bool) -> PreviewReadyArtifactV1 {
    let mut bindings = ResourceBindingMap::default();
    bindings.channel_bindings.insert(
        serde_json::from_value(json!("community_hub")).unwrap(),
        "700".parse().unwrap(),
    );
    let mut session =
        DesignSession::with_intent_recipe(ScriptedClient::validated_preview(), bindings);
    let request = if validated_preview {
        "Create private study rooms in community_hub and prepare a validated preview"
    } else {
        "Create private study rooms in community_hub"
    };
    assert!(matches!(
        session.run_burst(request).await,
        BurstOutcome::Ready { .. }
    ));
    session.export_preview_ready_artifact().unwrap()
}

fn context(ruleset_key: &str) -> AuthenticatedPromotionContext {
    AuthenticatedPromotionContext {
        tenant_id: TenantId::parse("tenant-1").unwrap(),
        principal_id: PrincipalId::parse("principal-1").unwrap(),
        session_owner_id: PrincipalId::parse("principal-1").unwrap(),
        session_id: AuthoringSessionId::parse("session-1").unwrap(),
        session_generation: SessionGeneration::new(1).unwrap(),
        guild_id: GuildId(900),
        installation_id: AutomationInstallationId::parse("installation-1").unwrap(),
        ruleset_key: ruleset_key.parse().unwrap(),
        requester: UserId(100),
        binding_revision: BindingRevision::new(1).unwrap(),
        policy: ApprovalPolicyV1 {
            revision: PolicyRevision::new(1).unwrap(),
            required_approvals: NonZeroU32::new(1).unwrap(),
            ttl_seconds: NonZeroU64::new(3600).unwrap(),
        },
    }
}

fn fixed_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0)
        .single()
        .unwrap()
}

struct PublicationSpy {
    store: InMemoryRuleSetStore,
    calls: AtomicUsize,
    fail: AtomicBool,
    corrupt: AtomicBool,
    barrier: Option<Arc<Barrier>>,
}

impl Default for PublicationSpy {
    fn default() -> Self {
        Self {
            store: InMemoryRuleSetStore::default(),
            calls: AtomicUsize::new(0),
            fail: AtomicBool::new(false),
            corrupt: AtomicBool::new(false),
            barrier: None,
        }
    }
}

impl RuleSetPublicationPort for PublicationSpy {
    async fn publish_ruleset(
        &self,
        request: PublishAuthoringRuleSetV1,
    ) -> Result<PublicationPortOutcomeV1, RuleSetStoreError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(barrier) = &self.barrier {
            barrier.wait();
        }
        if self.fail.load(Ordering::SeqCst) {
            return Err(RuleSetStoreError::Backend("injected".to_string()));
        }
        let outcome = RuleSetStore::publish(
            &self.store,
            PublishRuleSetRequest {
                guild_id: request.guild_id,
                ruleset_key: request.ruleset_key,
                definition: request.definition,
                created_by: request.created_by,
            },
        )
        .await?;
        let outcome = match outcome {
            PublishOutcome::Created(artifact) => {
                PublicationPortOutcomeV1::Created(PublishedAuthoringRuleSetV1::from(artifact))
            }
            PublishOutcome::Reused(artifact) => {
                PublicationPortOutcomeV1::Reused(PublishedAuthoringRuleSetV1::from(artifact))
            }
        };
        if !self.corrupt.load(Ordering::SeqCst) {
            return Ok(outcome);
        }
        Ok(match outcome {
            PublicationPortOutcomeV1::Created(mut artifact) => {
                artifact.guild_id = GuildId(artifact.guild_id.0 + 1);
                PublicationPortOutcomeV1::Created(artifact)
            }
            PublicationPortOutcomeV1::Reused(mut artifact) => {
                artifact.guild_id = GuildId(artifact.guild_id.0 + 1);
                PublicationPortOutcomeV1::Reused(artifact)
            }
        })
    }
}

struct PendingSpy {
    calls: AtomicUsize,
    link_calls: AtomicUsize,
    fail_once: AtomicBool,
    fail_link_once: AtomicBool,
    indeterminate_after_link_once: AtomicBool,
    indeterminate_after_create_once: AtomicBool,
    corrupt: AtomicBool,
    expire_existing: AtomicBool,
    barrier: Option<Arc<Barrier>>,
    now: Mutex<DateTime<Utc>>,
    requests: Mutex<BTreeMap<ActivationRequestId, ActivationRequest>>,
}

impl PendingSpy {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            link_calls: AtomicUsize::new(0),
            fail_once: AtomicBool::new(false),
            fail_link_once: AtomicBool::new(false),
            indeterminate_after_link_once: AtomicBool::new(false),
            indeterminate_after_create_once: AtomicBool::new(false),
            corrupt: AtomicBool::new(false),
            expire_existing: AtomicBool::new(false),
            barrier: None,
            now: Mutex::new(fixed_now()),
            requests: Mutex::new(BTreeMap::new()),
        }
    }
}

impl PendingActivationPort for PendingSpy {
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
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(barrier) = &self.barrier {
            barrier.wait();
        }
        if self.fail_once.swap(false, Ordering::SeqCst) {
            return Err(PendingActivationPortError::Backend("injected".to_string()));
        }
        let mut requests = self.requests.lock().unwrap();
        if let Some(existing) = requests.get_mut(&input.create.id) {
            if self.expire_existing.load(Ordering::SeqCst) {
                existing.expire_if_due(existing.expires_at);
            }
            let exact = existing.target == input.create.target
                && existing.requester == input.create.requester
                && existing.required_approvals
                    == input.create.context.policy.required_approvals.get()
                && existing.expires_at - existing.created_at
                    == chrono::Duration::seconds(
                        i64::try_from(input.create.context.policy.ttl_seconds.get()).unwrap(),
                    )
                && existing.approval_context
                    == (ActivationApprovalContextV1::ProductAuthoring {
                        context: Box::new(input.create.context.clone()),
                    });
            if !exact {
                return Err(PendingActivationPortError::Conflict(
                    "request identity mismatch".to_string(),
                ));
            }
            return Ok(PendingActivationReceiptV1 {
                request: existing.clone(),
                disposition: PendingActivationDispositionV1::Reused,
            });
        }
        let mut request =
            ActivationRequest::create_product(input.create, *self.now.lock().unwrap()).unwrap();
        if self.corrupt.load(Ordering::SeqCst) {
            request.requester = UserId(request.requester.0 + 1);
        }
        requests.insert(request.id.clone(), request.clone());
        if self
            .indeterminate_after_create_once
            .swap(false, Ordering::SeqCst)
        {
            return Err(PendingActivationPortError::Indeterminate(
                "injected after create".to_string(),
            ));
        }
        Ok(PendingActivationReceiptV1 {
            request,
            disposition: PendingActivationDispositionV1::Created,
        })
    }

    async fn link_pending_activation(
        &self,
        input: LinkPendingActivationV1,
    ) -> Result<ActivationRequest, PendingActivationPortError> {
        self.link_calls.fetch_add(1, Ordering::SeqCst);
        if self.fail_link_once.swap(false, Ordering::SeqCst) {
            return Err(PendingActivationPortError::Backend(
                "injected before link".to_string(),
            ));
        }
        let mut requests = self.requests.lock().unwrap();
        let request = requests.get_mut(&input.request_id).ok_or_else(|| {
            PendingActivationPortError::Indeterminate("request disappeared".to_string())
        })?;
        let result = request.link_product_at(
            &input.link.promotion_id,
            &input.link.promotion_request_digest,
            &input.link.approval_context_digest,
            *self.now.lock().unwrap(),
        );
        if result.is_ok()
            && self
                .indeterminate_after_link_once
                .swap(false, Ordering::SeqCst)
        {
            return Err(PendingActivationPortError::Indeterminate(
                "injected after link".to_string(),
            ));
        }
        match result {
            Ok(_) | Err(automation_ruleset_activation::LinkDecisionError::Expired) => {
                Ok(request.clone())
            }
            Err(error) => Err(PendingActivationPortError::Conflict(error.to_string())),
        }
    }
}

async fn start_input(key: &str, ruleset_key: &str, validated: bool) -> StartPromotionV1 {
    StartPromotionV1 {
        idempotency_key: IdempotencyKey::parse(key).unwrap(),
        context: context(ruleset_key),
        artifact: artifact(validated).await,
    }
}

fn start_input_from_artifact(
    key: &str,
    ruleset_key: &str,
    artifact: &PreviewReadyArtifactV1,
) -> StartPromotionV1 {
    StartPromotionV1 {
        idempotency_key: IdempotencyKey::parse(key).unwrap(),
        context: context(ruleset_key),
        artifact: artifact.clone(),
    }
}

#[test]
fn pure_planner_preserves_identity_digests_and_exact_transitions() {
    block_on(async {
        let idempotency_key = IdempotencyKey::parse("planner-secret").unwrap();
        let context = context("studyrooms");
        let identity = derive_promotion_identity_v1(
            &context.tenant_id,
            &context.principal_id,
            &idempotency_key,
        )
        .unwrap();
        assert_eq!(
            derive_promotion_identity_from_secret_v1(
                &context.tenant_id,
                &context.principal_id,
                "planner-secret",
            )
            .unwrap(),
            identity
        );
        assert!(derive_promotion_identity_from_secret_v1(
            &context.tenant_id,
            &context.principal_id,
            "invalid secret",
        )
        .is_err());
        let plan = plan_start_promotion_v1(StartPromotionV1 {
            idempotency_key,
            context,
            artifact: artifact(true).await,
        })
        .unwrap();
        assert_eq!(identity.promotion_id, plan.promotion_id);
        assert_eq!(
            plan.promotion_id.as_str(),
            "490e8c1aa23981c65300756c82b5e001204c5c867a7c2920d57a0e1ecae7204f"
        );
        let encoded = serde_json::to_string(&plan).unwrap();
        assert!(!encoded.contains("planner-secret"));
        assert!(!format!("{plan:?}").contains("planner-secret"));
        let mut unknown = serde_json::to_value(&plan).unwrap();
        unknown
            .as_object_mut()
            .unwrap()
            .insert("unexpected".to_string(), json!(true));
        assert!(
            serde_json::from_value::<authoring_promotion::PreparedPromotionPlanV1>(unknown)
                .is_err()
        );

        let prepared = PromotionRecordV1::prepared(plan.materialize(fixed_now()).unwrap()).unwrap();
        plan.validate_prepared_record(&prepared).unwrap();
        let publication = PublicationSpy::default();
        let publication_plan = plan_ruleset_publication_v1(&prepared).unwrap();
        let publication_outcome = publication
            .publish_ruleset(publication_plan.request())
            .await
            .unwrap();
        let published = publication_plan
            .complete(&prepared, publication_outcome, fixed_now())
            .unwrap()
            .expected_record;
        validate_exact_planned_record_v1(&published, &published).unwrap();

        let activation = PendingSpy::new();
        let environment = plan_approval_environment_v1(&published).unwrap();
        let resolved = activation
            .resolve_product_approval_context(environment.request())
            .await
            .unwrap();
        let pending_plan = plan_pending_activation_v1(&published, resolved).unwrap();
        let receipt = activation
            .ensure_pending_activation(pending_plan.request())
            .await
            .unwrap();
        let PendingActivationTransitionV1::ActivationPending {
            expected_record: pending,
            ..
        } = pending_plan
            .complete(&published, &receipt, fixed_now())
            .unwrap()
        else {
            panic!("expected activation-pending transition")
        };
        let link_plan = plan_activation_link_v1(&pending).unwrap();
        let linked = activation
            .link_pending_activation(link_plan.request())
            .await
            .unwrap();
        assert!(matches!(
            link_plan.complete(&pending, &linked, fixed_now()).unwrap(),
            LinkedActivationTransitionV1::Linked { expected_record } if *expected_record == pending
        ));
    });
}

#[test]
fn exact_candidate_reaches_pending_without_changing_the_active_pointer() {
    block_on(async {
        let promotions = InMemoryPromotionStore::default();
        let publication = PublicationSpy::default();
        let activation = PendingSpy::new();
        let service = PromotionService::new(
            &promotions,
            &publication,
            &activation,
            ManualPromotionClock::new(fixed_now()),
        );
        let sealed_artifact = artifact(true).await;

        let created = service
            .start(start_input_from_artifact(
                "request-1",
                "studyrooms",
                &sealed_artifact,
            ))
            .await
            .unwrap();
        let CreatePromotionOutcomeV1::Created(prepared) = created else {
            panic!("expected created promotion")
        };
        assert_eq!(prepared.stage, PromotionStageV1::Prepared);
        let outcome = service
            .resume_to_activation_pending(&prepared.id)
            .await
            .unwrap();
        let ResumePromotionOutcomeV1::Advanced(record) = outcome else {
            panic!("expected an advanced promotion")
        };
        let PromotionStageV1::ActivationPending {
            publication: published,
            activation: linked,
        } = &record.stage
        else {
            panic!("expected activation pending")
        };
        assert_eq!(published.disposition, PublicationDispositionV1::Created);
        assert_eq!(published.registry_created_by, UserId(100));
        assert_eq!(linked.requester, UserId(100));
        let activation_request = activation
            .requests
            .lock()
            .unwrap()
            .get(&linked.request_id)
            .cloned()
            .unwrap();
        assert!(matches!(
            activation_request.approval_context,
            ActivationApprovalContextV1::ProductAuthoring { .. }
        ));
        assert!(matches!(
            activation_request.link_state,
            ActivationLinkStateV1::Linked { .. }
        ));
        let approval_payload = record.product_approval_payload().unwrap();
        let approval_payload_digest = approval_payload_digest_v1(&approval_payload).unwrap();
        assert_eq!(
            approval_payload_digest,
            linked.approval_context.approval_payload_digest
        );
        assert_eq!(
            approval_payload_digest.as_str(),
            "759d0f7a037eab7c077c63054f8f83b5f2a7853d32e415895395540e5c6bcd6f"
        );
        let mut changed_payload = approval_payload.clone();
        changed_payload.preview.summary.actions += 1;
        assert_ne!(
            approval_payload_digest_v1(&changed_payload).unwrap(),
            approval_payload_digest
        );
        assert_eq!(
            record.intent.idempotency_scope_digest.as_str(),
            "554d460bd901e90fa6fc6be49c267bfea2408c8af7fd892fd124721b03f66849"
        );
        assert_eq!(
            record.request_digest.as_str(),
            "6a0290f572f1afbab146f218fd14e318096093830047f57634f12eccef9af7ee"
        );
        assert_eq!(
            linked.request_id.as_str(),
            "d4f2051e24b836c59a67e2d35e001195d22dc436ebb4919bdb0a6c9585a5e210"
        );
        assert_ne!(
            record.intent.evidence.candidate_ruleset_hash.as_str(),
            published.content_hash.to_hex()
        );
        assert!(RuleSetStore::active(
            &publication.store,
            GuildId(900),
            &record.intent.authority.ruleset_key,
        )
        .await
        .unwrap()
        .is_none());
        assert_eq!(publication.calls.load(Ordering::SeqCst), 1);
        assert_eq!(activation.calls.load(Ordering::SeqCst), 1);

        assert!(matches!(
            service
                .start(start_input_from_artifact(
                    "request-1",
                    "studyrooms",
                    &sealed_artifact,
                ))
                .await
                .unwrap(),
            CreatePromotionOutcomeV1::ExactReplay(existing) if existing == record
        ));
        assert_eq!(publication.calls.load(Ordering::SeqCst), 1);
        assert_eq!(activation.calls.load(Ordering::SeqCst), 1);

        assert!(matches!(
            service.resume_to_activation_pending(&record.id).await.unwrap(),
            ResumePromotionOutcomeV1::AlreadyActivationPending(existing) if existing == record
        ));
        assert_eq!(publication.calls.load(Ordering::SeqCst), 1);
        assert_eq!(activation.calls.load(Ordering::SeqCst), 1);

        let encoded = serde_json::to_string(&record).unwrap();
        let restored = serde_json::from_str::<PromotionRecordV1>(&encoded).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored, record);

        let mut legacy_state_name = serde_json::to_value(&record).unwrap();
        let activation = legacy_state_name["stage"]["activation"]
            .as_object_mut()
            .unwrap();
        let state = activation.remove("request_state_at_journal").unwrap();
        activation.insert("request_state_at_link".to_string(), state);
        let legacy_restored =
            serde_json::from_value::<PromotionRecordV1>(legacy_state_name).unwrap();
        legacy_restored.validate().unwrap();
        assert_eq!(legacy_restored, record);
        let canonical = serde_json::to_value(&legacy_restored).unwrap();
        assert!(canonical["stage"]["activation"]
            .get("request_state_at_journal")
            .is_some());
        assert!(canonical["stage"]["activation"]
            .get("request_state_at_link")
            .is_none());

        let mut changed_evidence = serde_json::to_value(&record).unwrap();
        changed_evidence["intent"]["evidence"]["candidate_ruleset_hash"] = json!("00".repeat(32));
        let changed_evidence =
            serde_json::from_value::<PromotionRecordV1>(changed_evidence).unwrap();
        assert_eq!(
            changed_evidence.validate().unwrap_err(),
            PromotionRecordValidationError::Identity
        );

        let mut changed_activation = serde_json::to_value(&record).unwrap();
        changed_activation["stage"]["activation"]["request_id"] = json!("11".repeat(32));
        let changed_activation =
            serde_json::from_value::<PromotionRecordV1>(changed_activation).unwrap();
        assert_eq!(
            changed_activation.validate().unwrap_err(),
            PromotionRecordValidationError::Activation
        );

        let mut unknown_preview_field = serde_json::to_value(&record).unwrap();
        unknown_preview_field["intent"]["preview"]["summary"]["unexpected"] = json!(1);
        assert!(serde_json::from_value::<PromotionRecordV1>(unknown_preview_field).is_err());

        let mut changed_creator = serde_json::to_value(&record).unwrap();
        changed_creator["stage"]["publication"]["registry_created_by"] = json!("101");
        let changed_creator = serde_json::from_value::<PromotionRecordV1>(changed_creator).unwrap();
        assert_eq!(
            changed_creator.validate().unwrap_err(),
            PromotionRecordValidationError::Publication
        );

        let mut changed_activation_time = serde_json::to_value(&record).unwrap();
        changed_activation_time["stage"]["activation"]["created_at"] =
            json!("2026-07-17T12:00:00Z");
        changed_activation_time["stage"]["activation"]["expires_at"] =
            json!("2026-07-17T13:00:00Z");
        let changed_activation_time =
            serde_json::from_value::<PromotionRecordV1>(changed_activation_time).unwrap();
        assert_eq!(
            changed_activation_time.validate().unwrap_err(),
            PromotionRecordValidationError::Activation
        );

        let mut relabeled_expired = serde_json::to_value(&record).unwrap();
        relabeled_expired["stage"]["state"] = json!("expired");
        let relabeled_expired =
            serde_json::from_value::<PromotionRecordV1>(relabeled_expired).unwrap();
        assert_eq!(
            relabeled_expired.validate().unwrap_err(),
            PromotionRecordValidationError::Activation
        );

        let debug = format!("{record:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains(&record.intent.definition.panels[0].content));
        assert!(!encoded.contains("request-1"));

        let publish_input = PublishAuthoringRuleSetV1 {
            guild_id: record.intent.authority.guild_id,
            ruleset_key: record.intent.authority.ruleset_key.clone(),
            definition: record.intent.definition.clone(),
            created_by: record.intent.authority.requester,
        };
        let publish_input_debug = format!("{publish_input:?}");
        assert!(publish_input_debug.contains("<redacted>"));
        assert!(!publish_input_debug.contains(&record.intent.definition.panels[0].content));
        let published_version = RuleSetStore::get_version(
            &publication.store,
            record.intent.authority.guild_id,
            &record.intent.authority.ruleset_key,
            published.version,
        )
        .await
        .unwrap()
        .unwrap();
        let publish_output_debug = format!(
            "{:?}",
            PublicationPortOutcomeV1::Created(PublishedAuthoringRuleSetV1::from(published_version))
        );
        assert!(publish_output_debug.contains("<redacted>"));
        assert!(!publish_output_debug.contains(&record.intent.definition.panels[0].content));
    });
}

#[test]
fn exact_idempotent_replay_succeeds_and_changed_payload_conflicts() {
    block_on(async {
        let promotions = InMemoryPromotionStore::default();
        let publication = PublicationSpy::default();
        let activation = PendingSpy::new();
        let service = PromotionService::new(
            &promotions,
            &publication,
            &activation,
            ManualPromotionClock::new(fixed_now()),
        );
        let sealed_artifact = artifact(true).await;
        let first = service
            .start(start_input_from_artifact(
                "same-key",
                "studyrooms",
                &sealed_artifact,
            ))
            .await
            .unwrap();
        let CreatePromotionOutcomeV1::Created(first) = first else {
            panic!("expected created")
        };
        let replay = service
            .start(start_input_from_artifact(
                "same-key",
                "studyrooms",
                &sealed_artifact,
            ))
            .await
            .unwrap();
        assert!(matches!(
            replay,
            CreatePromotionOutcomeV1::ExactReplay(existing) if existing == first
        ));
        let mut changed_session =
            start_input_from_artifact("same-key", "studyrooms", &sealed_artifact);
        changed_session.context.session_id = AuthoringSessionId::parse("session-2").unwrap();
        let mut changed_requester =
            start_input_from_artifact("same-key", "studyrooms", &sealed_artifact);
        changed_requester.context.requester = UserId(101);
        let mut changed_generation =
            start_input_from_artifact("same-key", "studyrooms", &sealed_artifact);
        changed_generation.context.session_generation = SessionGeneration::new(2).unwrap();
        let mut changed_guild =
            start_input_from_artifact("same-key", "studyrooms", &sealed_artifact);
        changed_guild.context.guild_id = GuildId(901);
        let mut changed_installation =
            start_input_from_artifact("same-key", "studyrooms", &sealed_artifact);
        changed_installation.context.installation_id =
            AutomationInstallationId::parse("installation-2").unwrap();
        let mut changed_binding =
            start_input_from_artifact("same-key", "studyrooms", &sealed_artifact);
        changed_binding.context.binding_revision = BindingRevision::new(2).unwrap();
        let mut changed_policy =
            start_input_from_artifact("same-key", "studyrooms", &sealed_artifact);
        changed_policy.context.policy.revision = PolicyRevision::new(2).unwrap();
        let mut changed_quorum =
            start_input_from_artifact("same-key", "studyrooms", &sealed_artifact);
        changed_quorum.context.policy.required_approvals = NonZeroU32::new(2).unwrap();
        let mut changed_ttl = start_input_from_artifact("same-key", "studyrooms", &sealed_artifact);
        changed_ttl.context.policy.ttl_seconds = NonZeroU64::new(7200).unwrap();
        let conflicting = [
            start_input_from_artifact("same-key", "different-target", &sealed_artifact),
            changed_session,
            changed_requester,
            changed_generation,
            changed_guild,
            changed_installation,
            changed_binding,
            changed_policy,
            changed_quorum,
            changed_ttl,
        ];
        for input in conflicting {
            assert_eq!(
                service.start(input).await.unwrap_err(),
                PromotionError::Store(PromotionStoreError::IdempotencyConflict)
            );
        }
        assert_eq!(publication.calls.load(Ordering::SeqCst), 0);
        assert_eq!(activation.calls.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn tenant_principal_and_raw_key_partition_the_idempotency_scope() {
    block_on(async {
        let promotions = InMemoryPromotionStore::default();
        let publication = PublicationSpy::default();
        let activation = PendingSpy::new();
        let service = PromotionService::new(
            &promotions,
            &publication,
            &activation,
            ManualPromotionClock::new(fixed_now()),
        );
        let sealed_artifact = artifact(true).await;
        let base = start_input_from_artifact("partition-key", "studyrooms", &sealed_artifact);
        let mut changed_tenant =
            start_input_from_artifact("partition-key", "studyrooms", &sealed_artifact);
        changed_tenant.context.tenant_id = TenantId::parse("tenant-2").unwrap();
        let mut changed_principal =
            start_input_from_artifact("partition-key", "studyrooms", &sealed_artifact);
        changed_principal.context.principal_id = PrincipalId::parse("principal-2").unwrap();
        changed_principal.context.session_owner_id = PrincipalId::parse("principal-2").unwrap();
        let changed_key =
            start_input_from_artifact("partition-key-2", "studyrooms", &sealed_artifact);
        let mut ids = Vec::new();
        for input in [base, changed_tenant, changed_principal, changed_key] {
            let CreatePromotionOutcomeV1::Created(record) = service.start(input).await.unwrap()
            else {
                panic!("expected separate promotion")
            };
            ids.push(record.id.as_str().to_string());
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 4);
        assert_eq!(publication.calls.load(Ordering::SeqCst), 0);
        assert_eq!(activation.calls.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn stale_publication_and_activation_cas_cannot_overwrite_the_winner() {
    block_on(async {
        let promotions = InMemoryPromotionStore::default();
        let publication = PublicationSpy::default();
        let activation = PendingSpy::new();
        let clock = ManualPromotionClock::new(fixed_now());
        let service = PromotionService::new(&promotions, &publication, &activation, clock.clone());
        let CreatePromotionOutcomeV1::Created(prepared) = service
            .start(start_input("cas-winner", "studyrooms", true).await)
            .await
            .unwrap()
        else {
            panic!("expected created")
        };
        let outcome = publication
            .publish_ruleset(PublishAuthoringRuleSetV1 {
                guild_id: prepared.intent.authority.guild_id,
                ruleset_key: prepared.intent.authority.ruleset_key.clone(),
                definition: prepared.intent.definition.clone(),
                created_by: prepared.intent.authority.requester,
            })
            .await
            .unwrap();
        let (disposition, artifact) = match outcome {
            PublicationPortOutcomeV1::Created(artifact) => {
                (PublicationDispositionV1::Created, artifact)
            }
            PublicationPortOutcomeV1::Reused(artifact) => {
                (PublicationDispositionV1::Reused, artifact)
            }
        };
        let publication_record = PublicationRecordV1 {
            version: artifact.version,
            schema_version: artifact.schema_version,
            content_hash: artifact.content_hash,
            disposition,
            registry_created_by: artifact.created_by,
        };
        let published = promotions
            .mark_published(
                &prepared.id,
                prepared.revision,
                publication_record.clone(),
                fixed_now(),
            )
            .await
            .unwrap();
        assert_eq!(
            promotions
                .mark_published(
                    &prepared.id,
                    prepared.revision,
                    publication_record,
                    fixed_now(),
                )
                .await
                .unwrap_err(),
            PromotionStoreError::RevisionConflict {
                current: published.revision
            }
        );
        let ResumePromotionOutcomeV1::Advanced(final_record) = service
            .resume_to_activation_pending(&prepared.id)
            .await
            .unwrap()
        else {
            panic!("expected advanced")
        };
        let PromotionStageV1::ActivationPending {
            activation: link, ..
        } = &final_record.stage
        else {
            panic!("expected activation pending")
        };
        assert_eq!(
            promotions
                .mark_activation_pending(
                    &prepared.id,
                    published.revision,
                    link.clone(),
                    fixed_now(),
                )
                .await
                .unwrap_err(),
            PromotionStoreError::RevisionConflict {
                current: final_record.revision
            }
        );
        assert_eq!(
            promotions.get(&prepared.id).await.unwrap().unwrap(),
            final_record
        );
    });
}

#[test]
fn concurrent_resumes_converge_on_one_version_one_request_and_one_record() {
    let promotions = InMemoryPromotionStore::default();
    let publication = PublicationSpy {
        barrier: Some(Arc::new(Barrier::new(2))),
        ..PublicationSpy::default()
    };
    let mut activation = PendingSpy::new();
    activation.barrier = Some(Arc::new(Barrier::new(2)));
    let clock = ManualPromotionClock::new(fixed_now());
    let prepared = block_on(async {
        let service = PromotionService::new(&promotions, &publication, &activation, clock.clone());
        let CreatePromotionOutcomeV1::Created(prepared) = service
            .start(start_input("concurrent-resume", "studyrooms", true).await)
            .await
            .unwrap()
        else {
            panic!("expected created")
        };
        prepared
    });
    let (first, second) = std::thread::scope(|scope| {
        let first_promotions = &promotions;
        let first_publication = &publication;
        let first_activation = &activation;
        let first_clock = clock.clone();
        let first_id = prepared.id.clone();
        let first = scope.spawn(move || {
            block_on(
                PromotionService::new(
                    first_promotions,
                    first_publication,
                    first_activation,
                    first_clock,
                )
                .resume_to_activation_pending(&first_id),
            )
            .unwrap()
        });
        let second_promotions = &promotions;
        let second_publication = &publication;
        let second_activation = &activation;
        let second_clock = clock.clone();
        let second_id = prepared.id.clone();
        let second = scope.spawn(move || {
            block_on(
                PromotionService::new(
                    second_promotions,
                    second_publication,
                    second_activation,
                    second_clock,
                )
                .resume_to_activation_pending(&second_id),
            )
            .unwrap()
        });
        (first.join().unwrap(), second.join().unwrap())
    });
    assert!(matches!(
        (&first, &second),
        (
            ResumePromotionOutcomeV1::Advanced(_),
            ResumePromotionOutcomeV1::AlreadyActivationPending(_)
        ) | (
            ResumePromotionOutcomeV1::AlreadyActivationPending(_),
            ResumePromotionOutcomeV1::Advanced(_)
        ) | (
            ResumePromotionOutcomeV1::Advanced(_),
            ResumePromotionOutcomeV1::Advanced(_)
        )
    ));
    let final_record = |outcome| match outcome {
        ResumePromotionOutcomeV1::Advanced(record)
        | ResumePromotionOutcomeV1::AlreadyActivationPending(record) => record,
        ResumePromotionOutcomeV1::TerminalExpired(_) => panic!("unexpected expiry"),
    };
    let first = final_record(first);
    let second = final_record(second);
    assert_eq!(first, second);
    assert_eq!(publication.calls.load(Ordering::SeqCst), 2);
    assert_eq!(activation.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        block_on(promotions.get(&prepared.id)).unwrap().unwrap(),
        first
    );
    assert_eq!(activation.requests.lock().unwrap().len(), 1);
    assert_eq!(
        block_on(RuleSetStore::list_versions(
            &publication.store,
            prepared.intent.authority.guild_id,
            &prepared.intent.authority.ruleset_key,
        ))
        .unwrap()
        .len(),
        1
    );
    assert!(block_on(RuleSetStore::active(
        &publication.store,
        prepared.intent.authority.guild_id,
        &prepared.intent.authority.ruleset_key,
    ))
    .unwrap()
    .is_none());
}

struct PanicStore;

impl PromotionStore for PanicStore {
    async fn create_prepared(
        &self,
        _promotion: authoring_promotion::NewPromotionV1,
    ) -> Result<CreatePromotionOutcomeV1, PromotionStoreError> {
        panic!("store must not be called")
    }

    async fn get(
        &self,
        _promotion_id: &PromotionId,
    ) -> Result<Option<PromotionRecordV1>, PromotionStoreError> {
        panic!("store must not be called")
    }

    async fn mark_published(
        &self,
        _promotion_id: &PromotionId,
        _expected_revision: authoring_promotion::PromotionRevision,
        _publication: authoring_promotion::PublicationRecordV1,
        _updated_at: DateTime<Utc>,
    ) -> Result<PromotionRecordV1, PromotionStoreError> {
        panic!("store must not be called")
    }

    async fn mark_activation_pending(
        &self,
        _promotion_id: &PromotionId,
        _expected_revision: authoring_promotion::PromotionRevision,
        _activation: authoring_promotion::PendingActivationLinkV1,
        _updated_at: DateTime<Utc>,
    ) -> Result<PromotionRecordV1, PromotionStoreError> {
        panic!("store must not be called")
    }

    async fn mark_expired(
        &self,
        _promotion_id: &PromotionId,
        _expected_revision: authoring_promotion::PromotionRevision,
        _activation: authoring_promotion::PendingActivationLinkV1,
        _updated_at: DateTime<Utc>,
    ) -> Result<PromotionRecordV1, PromotionStoreError> {
        panic!("store must not be called")
    }
}

#[test]
fn working_draft_is_rejected_before_any_store_or_external_capability() {
    block_on(async {
        let publication = PublicationSpy::default();
        let activation = PendingSpy::new();
        let service = PromotionService::new(
            &PanicStore,
            &publication,
            &activation,
            ManualPromotionClock::new(fixed_now()),
        );
        assert_eq!(
            service
                .start(start_input("working", "studyrooms", false).await)
                .await
                .unwrap_err(),
            PromotionError::ValidatedPreviewRequired
        );
        assert_eq!(publication.calls.load(Ordering::SeqCst), 0);
        assert_eq!(activation.calls.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn session_owner_mismatch_is_rejected_before_any_side_effect() {
    block_on(async {
        let publication = PublicationSpy::default();
        let activation = PendingSpy::new();
        let service = PromotionService::new(
            &PanicStore,
            &publication,
            &activation,
            ManualPromotionClock::new(fixed_now()),
        );
        let mut input = start_input("wrong-owner", "studyrooms", true).await;
        input.context.session_owner_id = PrincipalId::parse("principal-2").unwrap();
        assert_eq!(
            service.start(input).await.unwrap_err(),
            PromotionError::SessionOwnerMismatch
        );
        assert_eq!(publication.calls.load(Ordering::SeqCst), 0);
        assert_eq!(activation.calls.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn invalid_approval_ttl_extremes_are_rejected_before_any_side_effect() {
    block_on(async {
        let publication = PublicationSpy::default();
        let activation = PendingSpy::new();
        let service = PromotionService::new(
            &PanicStore,
            &publication,
            &activation,
            ManualPromotionClock::new(fixed_now()),
        );
        let sealed_artifact = artifact(true).await;
        for (key, ttl_seconds) in [
            ("ttl-u64-overflow", u64::MAX),
            ("ttl-duration-overflow", i64::MAX as u64),
            ("ttl-timestamp-overflow", 9_000_000_000_000),
        ] {
            let mut input = start_input_from_artifact(key, "studyrooms", &sealed_artifact);
            input.context.policy.ttl_seconds = NonZeroU64::new(ttl_seconds).unwrap();
            assert_eq!(
                service.start(input).await.unwrap_err(),
                PromotionError::InvalidPolicy
            );
        }
        assert_eq!(publication.calls.load(Ordering::SeqCst), 0);
        assert_eq!(activation.calls.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn publication_failure_or_mismatch_stays_prepared_without_activation() {
    block_on(async {
        for mismatch in [false, true] {
            let promotions = InMemoryPromotionStore::default();
            let publication = PublicationSpy::default();
            publication.fail.store(!mismatch, Ordering::SeqCst);
            publication.corrupt.store(mismatch, Ordering::SeqCst);
            let activation = PendingSpy::new();
            let service = PromotionService::new(
                &promotions,
                &publication,
                &activation,
                ManualPromotionClock::new(fixed_now()),
            );
            let CreatePromotionOutcomeV1::Created(prepared) = service
                .start(
                    start_input(
                        if mismatch { "mismatch" } else { "failure" },
                        "studyrooms",
                        true,
                    )
                    .await,
                )
                .await
                .unwrap()
            else {
                panic!("expected created")
            };
            let error = service
                .resume_to_activation_pending(&prepared.id)
                .await
                .unwrap_err();
            if mismatch {
                assert_eq!(error, PromotionError::PublicationMismatch);
            } else {
                assert!(matches!(error, PromotionError::RuleSet(_)));
            }
            assert_eq!(
                promotions.get(&prepared.id).await.unwrap().unwrap().stage,
                PromotionStageV1::Prepared
            );
            assert_eq!(activation.calls.load(Ordering::SeqCst), 0);
        }
    });
}

struct FailOnceTransitionStore {
    inner: InMemoryPromotionStore,
    fail_publish_once: AtomicBool,
    fail_activation_once: AtomicBool,
}

impl FailOnceTransitionStore {
    fn publication() -> Self {
        Self {
            inner: InMemoryPromotionStore::default(),
            fail_publish_once: AtomicBool::new(true),
            fail_activation_once: AtomicBool::new(false),
        }
    }

    fn activation() -> Self {
        Self {
            inner: InMemoryPromotionStore::default(),
            fail_publish_once: AtomicBool::new(false),
            fail_activation_once: AtomicBool::new(true),
        }
    }
}

impl PromotionStore for FailOnceTransitionStore {
    async fn create_prepared(
        &self,
        promotion: authoring_promotion::NewPromotionV1,
    ) -> Result<CreatePromotionOutcomeV1, PromotionStoreError> {
        self.inner.create_prepared(promotion).await
    }

    async fn get(
        &self,
        promotion_id: &PromotionId,
    ) -> Result<Option<PromotionRecordV1>, PromotionStoreError> {
        self.inner.get(promotion_id).await
    }

    async fn mark_published(
        &self,
        promotion_id: &PromotionId,
        expected_revision: authoring_promotion::PromotionRevision,
        publication: authoring_promotion::PublicationRecordV1,
        updated_at: DateTime<Utc>,
    ) -> Result<PromotionRecordV1, PromotionStoreError> {
        if self.fail_publish_once.swap(false, Ordering::SeqCst) {
            return Err(PromotionStoreError::Backend("injected".to_string()));
        }
        self.inner
            .mark_published(promotion_id, expected_revision, publication, updated_at)
            .await
    }

    async fn mark_activation_pending(
        &self,
        promotion_id: &PromotionId,
        expected_revision: authoring_promotion::PromotionRevision,
        activation: authoring_promotion::PendingActivationLinkV1,
        updated_at: DateTime<Utc>,
    ) -> Result<PromotionRecordV1, PromotionStoreError> {
        if self.fail_activation_once.swap(false, Ordering::SeqCst) {
            return Err(PromotionStoreError::Backend("injected".to_string()));
        }
        self.inner
            .mark_activation_pending(promotion_id, expected_revision, activation, updated_at)
            .await
    }

    async fn mark_expired(
        &self,
        promotion_id: &PromotionId,
        expected_revision: authoring_promotion::PromotionRevision,
        activation: authoring_promotion::PendingActivationLinkV1,
        updated_at: DateTime<Utc>,
    ) -> Result<PromotionRecordV1, PromotionStoreError> {
        self.inner
            .mark_expired(promotion_id, expected_revision, activation, updated_at)
            .await
    }
}

#[test]
fn crash_after_registry_publish_resumes_by_reusing_the_exact_inactive_artifact() {
    block_on(async {
        let promotions = FailOnceTransitionStore::publication();
        let publication = PublicationSpy::default();
        let activation = PendingSpy::new();
        let service = PromotionService::new(
            &promotions,
            &publication,
            &activation,
            ManualPromotionClock::new(fixed_now()),
        );
        let CreatePromotionOutcomeV1::Created(prepared) = service
            .start(start_input("crash-publish", "studyrooms", true).await)
            .await
            .unwrap()
        else {
            panic!("expected created")
        };
        assert!(matches!(
            service.resume_to_activation_pending(&prepared.id).await,
            Err(PromotionError::Store(PromotionStoreError::Backend(_)))
        ));
        assert_eq!(
            promotions.get(&prepared.id).await.unwrap().unwrap().stage,
            PromotionStageV1::Prepared
        );
        let ResumePromotionOutcomeV1::Advanced(record) = service
            .resume_to_activation_pending(&prepared.id)
            .await
            .unwrap()
        else {
            panic!("expected advanced")
        };
        let PromotionStageV1::ActivationPending {
            publication: published,
            ..
        } = record.stage
        else {
            panic!("expected pending")
        };
        assert_eq!(published.disposition, PublicationDispositionV1::Reused);
        assert_eq!(published.registry_created_by, UserId(100));
        assert_eq!(publication.calls.load(Ordering::SeqCst), 2);
        assert_eq!(activation.calls.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn activation_request_failure_leaves_an_inactive_published_artifact_and_resumes() {
    block_on(async {
        let promotions = InMemoryPromotionStore::default();
        let publication = PublicationSpy::default();
        let activation = PendingSpy::new();
        activation.fail_once.store(true, Ordering::SeqCst);
        let service = PromotionService::new(
            &promotions,
            &publication,
            &activation,
            ManualPromotionClock::new(fixed_now()),
        );
        let CreatePromotionOutcomeV1::Created(prepared) = service
            .start(start_input("activation-retry", "studyrooms", true).await)
            .await
            .unwrap()
        else {
            panic!("expected created")
        };
        assert!(matches!(
            service.resume_to_activation_pending(&prepared.id).await,
            Err(PromotionError::PendingActivation(
                PendingActivationPortError::Backend(_)
            ))
        ));
        assert!(matches!(
            promotions.get(&prepared.id).await.unwrap().unwrap().stage,
            PromotionStageV1::Published { .. }
        ));
        assert!(RuleSetStore::active(
            &publication.store,
            GuildId(900),
            &prepared.intent.authority.ruleset_key,
        )
        .await
        .unwrap()
        .is_none());
        assert!(matches!(
            service
                .resume_to_activation_pending(&prepared.id)
                .await
                .unwrap(),
            ResumePromotionOutcomeV1::Advanced(_)
        ));
        assert_eq!(publication.calls.load(Ordering::SeqCst), 1);
        assert_eq!(activation.calls.load(Ordering::SeqCst), 2);
    });
}

#[test]
fn indeterminate_activation_creation_resumes_with_the_single_exact_request() {
    block_on(async {
        let promotions = InMemoryPromotionStore::default();
        let publication = PublicationSpy::default();
        let activation = PendingSpy::new();
        activation
            .indeterminate_after_create_once
            .store(true, Ordering::SeqCst);
        let service = PromotionService::new(
            &promotions,
            &publication,
            &activation,
            ManualPromotionClock::new(fixed_now()),
        );
        let CreatePromotionOutcomeV1::Created(prepared) = service
            .start(start_input("activation-indeterminate", "studyrooms", true).await)
            .await
            .unwrap()
        else {
            panic!("expected created")
        };
        assert!(matches!(
            service.resume_to_activation_pending(&prepared.id).await,
            Err(PromotionError::PendingActivation(
                PendingActivationPortError::Indeterminate(_)
            ))
        ));
        assert!(matches!(
            promotions.get(&prepared.id).await.unwrap().unwrap().stage,
            PromotionStageV1::Published { .. }
        ));
        let ResumePromotionOutcomeV1::Advanced(record) = service
            .resume_to_activation_pending(&prepared.id)
            .await
            .unwrap()
        else {
            panic!("expected advanced")
        };
        let PromotionStageV1::ActivationPending {
            activation: link, ..
        } = record.stage
        else {
            panic!("expected activation pending")
        };
        assert_eq!(link.disposition, PendingActivationDispositionV1::Reused);
        assert_eq!(activation.requests.lock().unwrap().len(), 1);
        assert_eq!(publication.calls.load(Ordering::SeqCst), 1);
        assert_eq!(activation.calls.load(Ordering::SeqCst), 2);
    });
}

#[test]
fn crash_after_pending_request_creation_resumes_by_linking_the_exact_request() {
    block_on(async {
        let promotions = FailOnceTransitionStore::activation();
        let publication = PublicationSpy::default();
        let activation = PendingSpy::new();
        let service = PromotionService::new(
            &promotions,
            &publication,
            &activation,
            ManualPromotionClock::new(fixed_now()),
        );
        let CreatePromotionOutcomeV1::Created(prepared) = service
            .start(start_input("crash-activation", "studyrooms", true).await)
            .await
            .unwrap()
        else {
            panic!("expected created")
        };
        assert!(matches!(
            service.resume_to_activation_pending(&prepared.id).await,
            Err(PromotionError::Store(PromotionStoreError::Backend(_)))
        ));
        assert!(matches!(
            promotions.get(&prepared.id).await.unwrap().unwrap().stage,
            PromotionStageV1::Published { .. }
        ));
        let ResumePromotionOutcomeV1::Advanced(record) = service
            .resume_to_activation_pending(&prepared.id)
            .await
            .unwrap()
        else {
            panic!("expected advanced")
        };
        let PromotionStageV1::ActivationPending {
            activation: link, ..
        } = record.stage
        else {
            panic!("expected activation pending")
        };
        assert_eq!(link.disposition, PendingActivationDispositionV1::Reused);
        assert_eq!(publication.calls.load(Ordering::SeqCst), 1);
        assert_eq!(activation.calls.load(Ordering::SeqCst), 2);
    });
}

#[test]
fn linked_request_without_activation_pending_journal_is_not_laundered() {
    block_on(async {
        let promotions = FailOnceTransitionStore::activation();
        let publication = PublicationSpy::default();
        let activation = PendingSpy::new();
        let service = PromotionService::new(
            &promotions,
            &publication,
            &activation,
            ManualPromotionClock::new(fixed_now()),
        );
        let CreatePromotionOutcomeV1::Created(prepared) = service
            .start(start_input("out-of-order-link", "studyrooms", true).await)
            .await
            .unwrap()
        else {
            panic!("expected created")
        };
        assert!(matches!(
            service.resume_to_activation_pending(&prepared.id).await,
            Err(PromotionError::Store(PromotionStoreError::Backend(_)))
        ));
        assert!(matches!(
            promotions.get(&prepared.id).await.unwrap().unwrap().stage,
            PromotionStageV1::Published { .. }
        ));
        {
            let mut requests = activation.requests.lock().unwrap();
            let request = requests.values_mut().next().unwrap();
            let ActivationApprovalContextV1::ProductAuthoring { context } =
                &request.approval_context
            else {
                panic!("expected product context")
            };
            let link = LinkProductActivation {
                promotion_id: context.promotion_id.clone(),
                promotion_request_digest: context.promotion_request_digest.clone(),
                approval_context_digest: context.approval_context_digest.clone(),
            };
            request
                .link_product_at(
                    &link.promotion_id,
                    &link.promotion_request_digest,
                    &link.approval_context_digest,
                    fixed_now(),
                )
                .unwrap();
        }
        assert_eq!(
            service
                .resume_to_activation_pending(&prepared.id)
                .await
                .unwrap_err(),
            PromotionError::ConcurrentTransitionLimit
        );
        assert!(matches!(
            promotions.get(&prepared.id).await.unwrap().unwrap().stage,
            PromotionStageV1::Published { .. }
        ));
        assert_eq!(activation.link_calls.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn crash_after_promotion_journal_repairs_the_exact_unlinked_request() {
    block_on(async {
        for indeterminate_after_link in [false, true] {
            let promotions = InMemoryPromotionStore::default();
            let publication = PublicationSpy::default();
            let activation = PendingSpy::new();
            if indeterminate_after_link {
                activation
                    .indeterminate_after_link_once
                    .store(true, Ordering::SeqCst);
            } else {
                activation.fail_link_once.store(true, Ordering::SeqCst);
            }
            let service = PromotionService::new(
                &promotions,
                &publication,
                &activation,
                ManualPromotionClock::new(fixed_now()),
            );
            let key = if indeterminate_after_link {
                "journal-link-indeterminate"
            } else {
                "journal-link-crash"
            };
            let CreatePromotionOutcomeV1::Created(prepared) = service
                .start(start_input(key, "studyrooms", true).await)
                .await
                .unwrap()
            else {
                panic!("expected created")
            };
            assert!(matches!(
                service.resume_to_activation_pending(&prepared.id).await,
                Err(PromotionError::PendingActivation(
                    PendingActivationPortError::Backend(_)
                        | PendingActivationPortError::Indeterminate(_)
                ))
            ));
            let journaled = promotions.get(&prepared.id).await.unwrap().unwrap();
            let PromotionStageV1::ActivationPending {
                activation: link, ..
            } = &journaled.stage
            else {
                panic!("expected journaled activation")
            };
            let request = activation
                .requests
                .lock()
                .unwrap()
                .get(&link.request_id)
                .cloned()
                .unwrap();
            assert_eq!(
                matches!(request.link_state, ActivationLinkStateV1::Linked { .. }),
                indeterminate_after_link
            );
            assert!(matches!(
                service
                    .resume_to_activation_pending(&prepared.id)
                    .await
                    .unwrap(),
                ResumePromotionOutcomeV1::AlreadyActivationPending(existing)
                    if existing == journaled
            ));
            let repaired = activation
                .requests
                .lock()
                .unwrap()
                .get(&link.request_id)
                .cloned()
                .unwrap();
            assert!(matches!(
                repaired.link_state,
                ActivationLinkStateV1::Linked { .. }
            ));
            assert_eq!(activation.requests.lock().unwrap().len(), 1);
            assert_eq!(activation.link_calls.load(Ordering::SeqCst), 2);
        }
    });
}

#[test]
fn unlinked_request_expiring_after_journal_becomes_revision_four_terminal() {
    block_on(async {
        let promotions = InMemoryPromotionStore::default();
        let publication = PublicationSpy::default();
        let activation = PendingSpy::new();
        activation.fail_link_once.store(true, Ordering::SeqCst);
        let promotion_clock = ManualPromotionClock::new(fixed_now());
        let service = PromotionService::new(
            &promotions,
            &publication,
            &activation,
            promotion_clock.clone(),
        );
        let CreatePromotionOutcomeV1::Created(prepared) = service
            .start(start_input("journal-expiry", "studyrooms", true).await)
            .await
            .unwrap()
        else {
            panic!("expected created")
        };
        assert!(service
            .resume_to_activation_pending(&prepared.id)
            .await
            .is_err());
        let journaled = promotions.get(&prepared.id).await.unwrap().unwrap();
        assert_eq!(journaled.revision.get(), 3);
        *activation.now.lock().unwrap() = fixed_now() + chrono::Duration::seconds(3600);
        promotion_clock.advance(chrono::Duration::seconds(3600));
        let ResumePromotionOutcomeV1::TerminalExpired(expired) = service
            .resume_to_activation_pending(&prepared.id)
            .await
            .unwrap()
        else {
            panic!("expected terminal expiry")
        };
        assert_eq!(expired.revision.get(), 4);
        assert!(matches!(expired.stage, PromotionStageV1::Expired { .. }));
        expired.validate().unwrap();
    });
}

#[test]
fn expired_request_found_after_a_crash_is_journaled_terminally() {
    block_on(async {
        let promotions = FailOnceTransitionStore::activation();
        let publication = PublicationSpy::default();
        let activation = PendingSpy::new();
        let clock = ManualPromotionClock::new(fixed_now());
        let service = PromotionService::new(&promotions, &publication, &activation, clock.clone());
        let CreatePromotionOutcomeV1::Created(prepared) = service
            .start(start_input("expired-activation", "studyrooms", true).await)
            .await
            .unwrap()
        else {
            panic!("expected created")
        };
        assert!(matches!(
            service.resume_to_activation_pending(&prepared.id).await,
            Err(PromotionError::Store(PromotionStoreError::Backend(_)))
        ));
        activation.expire_existing.store(true, Ordering::SeqCst);
        clock.advance(chrono::Duration::seconds(3600));
        let ResumePromotionOutcomeV1::TerminalExpired(record) = service
            .resume_to_activation_pending(&prepared.id)
            .await
            .unwrap()
        else {
            panic!("expected terminal expiry")
        };
        assert!(matches!(record.stage, PromotionStageV1::Expired { .. }));
        assert!(matches!(
            service.resume_to_activation_pending(&record.id).await.unwrap(),
            ResumePromotionOutcomeV1::TerminalExpired(existing) if existing == record
        ));
        assert_eq!(publication.calls.load(Ordering::SeqCst), 1);
        assert_eq!(activation.calls.load(Ordering::SeqCst), 2);
    });
}

#[test]
fn mismatched_pending_request_is_not_linked() {
    block_on(async {
        let promotions = InMemoryPromotionStore::default();
        let publication = PublicationSpy::default();
        let activation = PendingSpy::new();
        activation.corrupt.store(true, Ordering::SeqCst);
        let service = PromotionService::new(
            &promotions,
            &publication,
            &activation,
            ManualPromotionClock::new(fixed_now()),
        );
        let CreatePromotionOutcomeV1::Created(prepared) = service
            .start(start_input("activation-mismatch", "studyrooms", true).await)
            .await
            .unwrap()
        else {
            panic!("expected created")
        };
        assert_eq!(
            service
                .resume_to_activation_pending(&prepared.id)
                .await
                .unwrap_err(),
            PromotionError::PendingActivationMismatch
        );
        assert!(matches!(
            promotions.get(&prepared.id).await.unwrap().unwrap().stage,
            PromotionStageV1::Published { .. }
        ));
    });
}
