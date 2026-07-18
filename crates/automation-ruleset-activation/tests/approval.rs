use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_ruleset_activation::{
    approval_policy_digest_v1, product_approval_context_digest_v1, ActivationDigest,
    ActivationPromotionId, ActivationRequestId, ActivationRequestState, ActivationRequestStore,
    ActivationTarget, ApplyAttemptId, ApprovalBindingContextV1, ApprovalPolicyBindingV1,
    ApproveError, ClaimOutcome, CreateActivationRequest, CreateProductActivationRequest,
    ExpectedActiveBaselineV1, InMemoryActivationRequestStore, LinkProductActivation,
    LinkProductError, ManualActivationClock, ProductApprovalContextV1, RejectError,
};
use chrono::{Duration, TimeZone, Utc};
use discord_model::{GuildId, UserId};
use futures::executor::block_on;
use resource_resolution::approval_binding_fingerprint_v1;
use std::num::{NonZeroU32, NonZeroU64};

fn clock() -> ManualActivationClock {
    ManualActivationClock::new(Utc.with_ymd_and_hms(2026, 7, 12, 0, 0, 0).unwrap())
}

fn target() -> ActivationTarget {
    ActivationTarget {
        guild_id: GuildId(7),
        ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
        version: RuleSetVersionId::FIRST,
        content_hash: RuleSetContentHash::parse_hex(&"11".repeat(32)).unwrap(),
    }
}

fn create(
    store: &InMemoryActivationRequestStore<ManualActivationClock>,
    id: &str,
    requester: u64,
    required_approvals: u32,
) -> ActivationRequestId {
    let id = ActivationRequestId::parse(id).unwrap();
    block_on(store.create(CreateActivationRequest {
        id: id.clone(),
        target: target(),
        requester: UserId(requester),
        required_approvals,
        ttl: Duration::minutes(30),
        observed_active: None,
    }))
    .unwrap();
    id
}

fn digest(value: &str) -> ActivationDigest {
    ActivationDigest::parse(&value.repeat(64)).unwrap()
}

fn product_context(
    id: &ActivationRequestId,
    activation_target: &ActivationTarget,
    requester: UserId,
) -> ProductApprovalContextV1 {
    let policy_revision = NonZeroU64::new(7).unwrap();
    let required_approvals = NonZeroU32::new(1).unwrap();
    let ttl_seconds = NonZeroU64::new(1800).unwrap();
    let binding_revision = NonZeroU64::new(3).unwrap();
    let mut context = ProductApprovalContextV1 {
        promotion_id: ActivationPromotionId::parse(&"a".repeat(64)).unwrap(),
        promotion_request_digest: digest("b"),
        approval_payload_digest: digest("c"),
        approval_context_digest: digest("0"),
        binding: ApprovalBindingContextV1 {
            revision: binding_revision,
            required_bindings: vec![],
            fingerprint: approval_binding_fingerprint_v1(
                activation_target.guild_id,
                binding_revision,
                &[],
            )
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
        product_approval_context_digest_v1(id, activation_target, requester, &context);
    context
}

fn create_product(
    store: &InMemoryActivationRequestStore<ManualActivationClock>,
    id: &str,
) -> (ActivationRequestId, ProductApprovalContextV1) {
    let id = ActivationRequestId::parse(id).unwrap();
    let activation_target = target();
    let requester = UserId(10);
    let context = product_context(&id, &activation_target, requester);
    block_on(store.create_product(CreateProductActivationRequest {
        id: id.clone(),
        target: activation_target,
        requester,
        context: context.clone(),
    }))
    .unwrap();
    (id, context)
}

fn link(context: &ProductApprovalContextV1) -> LinkProductActivation {
    LinkProductActivation {
        promotion_id: context.promotion_id.clone(),
        promotion_request_digest: context.promotion_request_digest.clone(),
        approval_context_digest: context.approval_context_digest.clone(),
    }
}

#[test]
fn request_ids_validate_and_roundtrip() {
    let id = ActivationRequestId::parse("request_01-A").unwrap();
    assert_eq!(id.as_str(), "request_01-A");
    assert_eq!(serde_json::to_string(&id).unwrap(), r#""request_01-A""#);
    assert!(ActivationRequestId::parse("").is_err());
    assert!(ActivationRequestId::parse("bad id").is_err());
    assert!(serde_json::from_str::<ActivationRequestId>(r#""bad id""#).is_err());
}

#[test]
fn self_approval_is_rejected() {
    let store = InMemoryActivationRequestStore::with_clock(clock());
    let id = create(&store, "self", 10, 1);

    assert_eq!(
        block_on(store.approve(&id, UserId(10))).unwrap_err(),
        ApproveError::SelfApprovalForbidden
    );
    let request = block_on(store.get(&id)).unwrap().unwrap();
    assert_eq!(request.state, ActivationRequestState::Pending);
    assert!(request.approvals.is_empty());
}

#[test]
fn one_distinct_approval_reaches_quorum_one() {
    let store = InMemoryActivationRequestStore::with_clock(clock());
    let id = create(&store, "one", 10, 1);

    let request = block_on(store.approve(&id, UserId(20))).unwrap();
    assert_eq!(request.state, ActivationRequestState::Approved);
    assert_eq!(request.approvals.len(), 1);
}

#[test]
fn duplicate_approval_is_rejected() {
    let store = InMemoryActivationRequestStore::with_clock(clock());
    let id = create(&store, "duplicate", 10, 2);
    block_on(store.approve(&id, UserId(20))).unwrap();

    assert_eq!(
        block_on(store.approve(&id, UserId(20))).unwrap_err(),
        ApproveError::DuplicateApproval
    );
    assert_eq!(
        block_on(store.get(&id)).unwrap().unwrap().approvals.len(),
        1
    );
}

#[test]
fn quorum_two_requires_two_distinct_approvers() {
    let store = InMemoryActivationRequestStore::with_clock(clock());
    let id = create(&store, "two", 10, 2);

    assert_eq!(
        block_on(store.approve(&id, UserId(20))).unwrap().state,
        ActivationRequestState::Pending
    );
    let request = block_on(store.approve(&id, UserId(30))).unwrap();
    assert_eq!(request.state, ActivationRequestState::Approved);
    assert_eq!(request.approvals.len(), 2);
}

#[test]
fn one_rejection_is_terminal_and_keeps_approvals() {
    let store = InMemoryActivationRequestStore::with_clock(clock());
    let id = create(&store, "reject", 10, 2);
    block_on(store.approve(&id, UserId(20))).unwrap();

    let request = block_on(store.reject(&id, UserId(30), "unsafe".to_string())).unwrap();
    assert_eq!(request.state, ActivationRequestState::Rejected);
    assert_eq!(request.approvals.len(), 1);
    assert_eq!(request.rejection.unwrap().reason, "unsafe");
}

#[test]
fn approve_and_reject_after_approved_are_rejected() {
    let store = InMemoryActivationRequestStore::with_clock(clock());
    let id = create(&store, "terminal", 10, 1);
    block_on(store.approve(&id, UserId(20))).unwrap();

    assert_eq!(
        block_on(store.approve(&id, UserId(30))).unwrap_err(),
        ApproveError::NotPending
    );
    assert_eq!(
        block_on(store.reject(&id, UserId(30), "no".to_string())).unwrap_err(),
        RejectError::NotPending
    );
}

#[test]
fn expired_request_rejects_approval_and_persists_expired() {
    let clock = clock();
    let store = InMemoryActivationRequestStore::with_clock(clock.clone());
    let id = create(&store, "expired", 10, 1);
    clock.advance(Duration::minutes(31));

    assert_eq!(
        block_on(store.approve(&id, UserId(20))).unwrap_err(),
        ApproveError::Expired
    );
    assert_eq!(
        block_on(store.get(&id)).unwrap().unwrap().state,
        ActivationRequestState::Expired
    );
}

#[test]
fn stored_quorum_does_not_change_with_later_policy() {
    let store = InMemoryActivationRequestStore::with_clock(clock());
    let id = create(&store, "fixed_policy", 10, 2);

    let request = block_on(store.approve(&id, UserId(20))).unwrap();
    assert_eq!(request.required_approvals, 2);
    assert_eq!(request.state, ActivationRequestState::Pending);
}

#[test]
fn product_request_is_inert_until_exact_link_and_payload_bound_approval() {
    let store = InMemoryActivationRequestStore::with_clock(clock());
    let (id, context) = create_product(&store, "product_link");

    assert_eq!(
        block_on(store.approve(&id, UserId(20))).unwrap_err(),
        ApproveError::BoundApprovalRequired
    );
    assert_eq!(
        block_on(store.approve_bound(&id, UserId(20), &context.approval_payload_digest))
            .unwrap_err(),
        ApproveError::Unlinked
    );
    assert_eq!(
        block_on(store.reject(&id, UserId(20), "no".to_string())).unwrap_err(),
        RejectError::Unlinked
    );
    assert_eq!(
        block_on(store.claim_apply(&id, ApplyAttemptId::parse("unlinked_attempt").unwrap(), 60))
            .unwrap(),
        ClaimOutcome::Unlinked
    );
    let mut wrong = link(&context);
    wrong.approval_context_digest = digest("d");
    assert_eq!(
        block_on(store.link_product(&id, wrong)).unwrap_err(),
        LinkProductError::Conflict
    );

    let linked = block_on(store.link_product(&id, link(&context))).unwrap();
    assert!(matches!(
        linked.link_state,
        automation_ruleset_activation::ActivationLinkStateV1::Linked { .. }
    ));
    assert_eq!(
        block_on(store.link_product(&id, link(&context))).unwrap(),
        linked
    );
    assert_eq!(
        block_on(store.approve_bound(&id, UserId(20), &digest("d"))).unwrap_err(),
        ApproveError::PayloadMismatch
    );
    let approved =
        block_on(store.approve_bound(&id, UserId(20), &context.approval_payload_digest)).unwrap();
    assert_eq!(approved.state, ActivationRequestState::Approved);
    assert_eq!(
        approved.approvals[0].approval_payload_digest.as_ref(),
        Some(&context.approval_payload_digest)
    );
}

#[test]
fn expired_unlinked_product_request_cannot_be_linked() {
    let activation_clock = clock();
    let store = InMemoryActivationRequestStore::with_clock(activation_clock.clone());
    let (id, context) = create_product(&store, "product_expired");
    activation_clock.advance(Duration::minutes(31));

    assert_eq!(
        block_on(store.link_product(&id, link(&context))).unwrap_err(),
        LinkProductError::Expired
    );
    assert_eq!(
        block_on(store.get(&id)).unwrap().unwrap().state,
        ActivationRequestState::Expired
    );
}

#[test]
fn exact_product_link_replay_survives_expiry_and_persists_terminal_state() {
    let activation_clock = clock();
    let store = InMemoryActivationRequestStore::with_clock(activation_clock.clone());
    let (id, context) = create_product(&store, "product_link_replay_expired");
    let linked = block_on(store.link_product(&id, link(&context))).unwrap();
    activation_clock.advance(Duration::minutes(31));

    let replayed = block_on(store.link_product(&id, link(&context))).unwrap();

    assert_eq!(replayed.link_state, linked.link_state);
    assert_eq!(replayed.state, ActivationRequestState::Expired);
    assert_eq!(
        block_on(store.get(&id)).unwrap().unwrap().state,
        ActivationRequestState::Expired
    );
}

#[test]
fn product_approval_before_link_timestamp_fails_validation() {
    let store = InMemoryActivationRequestStore::with_clock(clock());
    let (id, context) = create_product(&store, "product_approval_timestamp");
    let linked = block_on(store.link_product(&id, link(&context))).unwrap();
    let mut approved =
        block_on(store.approve_bound(&id, UserId(20), &context.approval_payload_digest)).unwrap();
    let automation_ruleset_activation::ActivationLinkStateV1::Linked { linked_at } =
        linked.link_state
    else {
        panic!("expected linked product request");
    };
    approved.approvals[0].approved_at = linked_at - Duration::seconds(1);

    assert!(approved.validate().is_err());
}
