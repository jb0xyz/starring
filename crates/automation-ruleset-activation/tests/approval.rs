use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
use automation_ruleset_activation::{
    ActivationRequestId, ActivationRequestState, ActivationRequestStore, ActivationTarget,
    ApproveError, CreateActivationRequest, InMemoryActivationRequestStore, ManualActivationClock,
    RejectError,
};
use chrono::{Duration, TimeZone, Utc};
use discord_model::{GuildId, UserId};
use futures::executor::block_on;

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
