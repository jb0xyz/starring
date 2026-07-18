use std::num::{NonZeroU32, NonZeroU64};

use authoring_promotion::{
    AutomationInstallationId, BindingRevision, EnsurePendingActivationV1, InMemoryPromotionStore,
    LinkPendingActivationV1, PendingActivationDispositionV1, PendingActivationPort,
    PendingActivationPortError, ProductActivationBridge, ProductApprovalEnvironmentError,
    ProductApprovalEnvironmentProvider, ProductApprovalEnvironmentV1,
    ResolveProductApprovalContextV1, TenantId,
};
use automation_ruleset::{
    InMemoryRuleSetStore, PublishOutcome, PublishRuleSetRequest, RuleSetKey, RuleSetStore,
};
use automation_ruleset_activation::{
    approval_policy_digest_v1, product_approval_context_digest_v1, ActivationDigest,
    ActivationLinkStateV1, ActivationPromotionId, ActivationRequestId, ActivationRequestStore,
    ActivationTarget, ApprovalPolicyBindingV1, ApproveError, CreateProductActivationRequest,
    InMemoryActivationRequestStore, LinkProductActivation, ManualActivationClock,
    ProductApprovalContextV1,
};
use automation_state::{
    ActionSpec, ActionTarget, InstanceRef, InteractionRule, InteractionRuleSet, RoleRef,
    TriggerSpec,
};
use chrono::{TimeZone, Utc};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, UserId};
use futures::executor::block_on;
use resource_resolution::{resource_binding_fingerprint_v2, ResourceBindingMap};

const GUILD: GuildId = GuildId(900);
const REQUESTER: UserId = UserId(100);

#[derive(Clone)]
struct FixedEnvironment {
    revision: NonZeroU64,
    bindings: ResourceBindingMap,
}

impl ProductApprovalEnvironmentProvider for FixedEnvironment {
    async fn load_fresh(
        &self,
        request: &ResolveProductApprovalContextV1,
    ) -> Result<ProductApprovalEnvironmentV1, ProductApprovalEnvironmentError> {
        assert_eq!(request.tenant_id.as_str(), "tenant-1");
        assert_eq!(request.installation_id.as_str(), "installation-1");
        Ok(ProductApprovalEnvironmentV1 {
            binding_revision: self.revision,
            bindings: self.bindings.clone(),
        })
    }
}

fn definition() -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "join".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "join".to_string(),
            },
            actions: vec![
                ActionSpec::DeferEphemeral,
                ActionSpec::GrantRole {
                    role: RoleRef::Instance {
                        instance: InstanceRef::Event,
                        alias: "member_role".to_string(),
                    },
                    target: ActionTarget::Actor,
                },
                ActionSpec::EditResponse {
                    content: "joined".to_string(),
                },
            ],
        }],
    }
}

fn bindings() -> ResourceBindingMap {
    let mut bindings = ResourceBindingMap::default();
    bindings
        .channel_bindings
        .insert(ResourceKey("community_hub".to_string()), ChannelId(700));
    bindings
}

async fn target(rulesets: &InMemoryRuleSetStore) -> ActivationTarget {
    let outcome = rulesets
        .publish(PublishRuleSetRequest {
            guild_id: GUILD,
            ruleset_key: RuleSetKey::parse("studyrooms").unwrap(),
            definition: definition(),
            created_by: REQUESTER,
        })
        .await
        .unwrap();
    let version = match outcome {
        PublishOutcome::Created(version) | PublishOutcome::Reused(version) => version,
    };
    ActivationTarget {
        guild_id: version.guild_id,
        ruleset_key: version.ruleset_key,
        version: version.version,
        content_hash: version.content_hash,
    }
}

fn resolution_request(
    target: &ActivationTarget,
    bindings: &ResourceBindingMap,
) -> ResolveProductApprovalContextV1 {
    ResolveProductApprovalContextV1 {
        tenant_id: TenantId::parse("tenant-1").unwrap(),
        installation_id: AutomationInstallationId::parse("installation-1").unwrap(),
        target: target.clone(),
        binding_revision: BindingRevision::new(3).unwrap(),
        context_fingerprint: resource_binding_fingerprint_v2(bindings),
        required_channel_bindings: vec!["community_hub".to_string()],
    }
}

#[test]
fn product_bridge_rejects_link_without_activation_pending_journal() {
    block_on(async {
        let rulesets = InMemoryRuleSetStore::default();
        let target = target(&rulesets).await;
        let bindings = bindings();
        let environment = FixedEnvironment {
            revision: NonZeroU64::new(3).unwrap(),
            bindings: bindings.clone(),
        };
        let clock = ManualActivationClock::new(
            Utc.with_ymd_and_hms(2026, 7, 18, 12, 0, 0)
                .single()
                .unwrap(),
        );
        let requests = InMemoryActivationRequestStore::with_clock(clock);
        let promotions = InMemoryPromotionStore::default();
        let bridge = ProductActivationBridge::new(&rulesets, &environment, &requests, &promotions);
        let resolved = bridge
            .resolve_product_approval_context(resolution_request(&target, &bindings))
            .await
            .unwrap();
        assert!(matches!(
            resolved.baseline,
            automation_ruleset_activation::ExpectedActiveBaselineV1::Absent
        ));
        assert_eq!(resolved.binding.required_bindings.len(), 1);
        let request_id = ActivationRequestId::parse(&"1".repeat(64)).unwrap();
        let policy_revision = NonZeroU64::new(5).unwrap();
        let required_approvals = NonZeroU32::new(1).unwrap();
        let ttl_seconds = NonZeroU64::new(3600).unwrap();
        let mut context = ProductApprovalContextV1 {
            promotion_id: ActivationPromotionId::parse(&"2".repeat(64)).unwrap(),
            promotion_request_digest: ActivationDigest::parse(&"3".repeat(64)).unwrap(),
            approval_payload_digest: ActivationDigest::parse(&"4".repeat(64)).unwrap(),
            approval_context_digest: ActivationDigest::parse(&"0".repeat(64)).unwrap(),
            binding: resolved.binding,
            baseline: resolved.baseline,
            policy: ApprovalPolicyBindingV1 {
                revision: policy_revision,
                required_approvals,
                ttl_seconds,
                digest: approval_policy_digest_v1(policy_revision, required_approvals, ttl_seconds),
            },
        };
        context.approval_context_digest =
            product_approval_context_digest_v1(&request_id, &target, REQUESTER, &context);
        let create = CreateProductActivationRequest {
            id: request_id.clone(),
            target: target.clone(),
            requester: REQUESTER,
            context: context.clone(),
        };
        let created = bridge
            .ensure_pending_activation(EnsurePendingActivationV1 {
                create: create.clone(),
            })
            .await
            .unwrap();
        assert_eq!(created.disposition, PendingActivationDispositionV1::Created);
        assert_eq!(created.request.link_state, ActivationLinkStateV1::Unlinked);
        let replay = bridge
            .ensure_pending_activation(EnsurePendingActivationV1 { create })
            .await
            .unwrap();
        assert_eq!(replay.disposition, PendingActivationDispositionV1::Reused);
        assert_eq!(
            requests
                .approve(&request_id, UserId(200))
                .await
                .unwrap_err(),
            ApproveError::BoundApprovalRequired
        );
        let link = LinkProductActivation {
            promotion_id: context.promotion_id.clone(),
            promotion_request_digest: context.promotion_request_digest.clone(),
            approval_context_digest: context.approval_context_digest.clone(),
        };
        let error = bridge
            .link_pending_activation(LinkPendingActivationV1 {
                request_id: request_id.clone(),
                link,
            })
            .await
            .unwrap_err();
        assert!(matches!(error, PendingActivationPortError::Conflict(_)));
        assert_eq!(
            requests.get(&request_id).await.unwrap().unwrap().link_state,
            ActivationLinkStateV1::Unlinked
        );
        assert_eq!(
            requests
                .approve_bound(&request_id, UserId(200), &context.approval_payload_digest)
                .await
                .unwrap_err(),
            ApproveError::Unlinked
        );
        assert!(rulesets
            .active(GUILD, &target.ruleset_key)
            .await
            .unwrap()
            .is_none());
    });
}

#[test]
fn product_bridge_fails_closed_on_binding_authority_drift() {
    block_on(async {
        let rulesets = InMemoryRuleSetStore::default();
        let target = target(&rulesets).await;
        let sealed_bindings = bindings();
        for environment in [
            FixedEnvironment {
                revision: NonZeroU64::new(4).unwrap(),
                bindings: sealed_bindings.clone(),
            },
            FixedEnvironment {
                revision: NonZeroU64::new(3).unwrap(),
                bindings: ResourceBindingMap::default(),
            },
        ] {
            let requests = InMemoryActivationRequestStore::default();
            let promotions = InMemoryPromotionStore::default();
            let bridge =
                ProductActivationBridge::new(&rulesets, &environment, &requests, &promotions);
            assert!(matches!(
                bridge
                    .resolve_product_approval_context(resolution_request(&target, &sealed_bindings))
                    .await,
                Err(PendingActivationPortError::Conflict(_))
            ));
            assert!(requests
                .get(&ActivationRequestId::parse("missing").unwrap())
                .await
                .unwrap()
                .is_none());
        }
    });
}

#[test]
fn production_bridge_has_no_legacy_activation_create_call() {
    let source = include_str!("../src/bridge.rs");
    assert!(!source.contains(".create("));
    assert!(source.contains(".create_product("));
}
