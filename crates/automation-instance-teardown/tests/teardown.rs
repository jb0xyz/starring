use std::collections::{BTreeMap, VecDeque};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use automation_instance::{
    AutomationInstance, InMemoryInstanceStore, InstanceId, InstanceKind, InstanceMessageRef,
    InstanceResources, InstanceRuleSetVersion, InstanceStatus, InstanceStore, InstanceStoreError,
    InstanceTeardownClaimOutcomeV1, InstanceTeardownMarkOutcomeV1, InstanceTeardownStoreV1,
};
use automation_instance_teardown::{
    DeleteOutcome, DeleterError, DeleterErrorKind, DurableInstanceTeardownServiceV1,
    ExactInstanceRegistrationIdentityV1, ExactInstanceTeardownRequestV1, InstanceDeleter,
    InstanceResource, InstanceTeardownRecoveryObservationV1, InstanceTeardownService, Teardown,
    TeardownError, TeardownOutcome,
};
use discord_model::{ChannelId, GuildId, MessageId, RoleId, UserId};
use futures::executor::block_on;

const GUILD: GuildId = GuildId(7);

#[derive(Clone, Debug, PartialEq, Eq)]
enum DeleteCall {
    Message(ChannelId, MessageId),
    Channel(ChannelId),
    Role(RoleId),
}

#[derive(Default)]
struct StoreState {
    inner: InMemoryInstanceStore,
    get_calls: AtomicUsize,
    transition_calls: AtomicUsize,
    mark_calls: AtomicUsize,
    fail_mark: AtomicBool,
}

#[derive(Clone, Default)]
struct SharedStore(Arc<StoreState>);

impl SharedStore {
    fn get_calls(&self) -> usize {
        self.0.get_calls.load(Ordering::SeqCst)
    }

    fn transition_calls(&self) -> usize {
        self.0.transition_calls.load(Ordering::SeqCst)
    }

    fn mark_calls(&self) -> usize {
        self.0.mark_calls.load(Ordering::SeqCst)
    }

    fn set_fail_mark(&self, fail: bool) {
        self.0.fail_mark.store(fail, Ordering::SeqCst);
    }
}

impl InstanceStore for SharedStore {
    async fn register(&self, instance: AutomationInstance) -> Result<(), InstanceStoreError> {
        self.0.inner.register(instance).await
    }

    async fn get(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        self.0.get_calls.fetch_add(1, Ordering::SeqCst);
        self.0.inner.get(guild_id, instance_id).await
    }

    async fn list_by_guild(
        &self,
        guild_id: GuildId,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
        self.0.inner.list_by_guild(guild_id).await
    }

    async fn update_status(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
        status: InstanceStatus,
    ) -> Result<(), InstanceStoreError> {
        self.0
            .inner
            .update_status(guild_id, instance_id, status)
            .await
    }

    async fn transition_to_deleting(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<(), InstanceStoreError> {
        self.0.transition_calls.fetch_add(1, Ordering::SeqCst);
        self.0
            .inner
            .transition_to_deleting(guild_id, instance_id)
            .await
    }

    async fn mark_deleted(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<(), InstanceStoreError> {
        self.0.mark_calls.fetch_add(1, Ordering::SeqCst);
        if self.0.fail_mark.load(Ordering::SeqCst) {
            return Err(InstanceStoreError::Backend("mark failed".to_string()));
        }
        self.0.inner.mark_deleted(guild_id, instance_id).await
    }

    async fn list_deleting(
        &self,
        guild_id: GuildId,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
        self.0.inner.list_deleting(guild_id).await
    }
}

impl InstanceTeardownStoreV1 for SharedStore {
    async fn get_for_teardown_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        self.0.get_calls.fetch_add(1, Ordering::SeqCst);
        self.0
            .inner
            .get_for_teardown_v1(guild_id, instance_id)
            .await
    }

    async fn claim_deleting_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<InstanceTeardownClaimOutcomeV1, InstanceStoreError> {
        let outcome = self
            .0
            .inner
            .claim_deleting_v1(guild_id, instance_id)
            .await?;
        if outcome == InstanceTeardownClaimOutcomeV1::Claimed {
            self.0.transition_calls.fetch_add(1, Ordering::SeqCst);
        }
        Ok(outcome)
    }

    async fn mark_deleted_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<InstanceTeardownMarkOutcomeV1, InstanceStoreError> {
        self.0.mark_calls.fetch_add(1, Ordering::SeqCst);
        if self.0.fail_mark.load(Ordering::SeqCst) {
            return Err(InstanceStoreError::Backend("mark failed".to_string()));
        }
        self.0.inner.mark_deleted_v1(guild_id, instance_id).await
    }

    async fn list_retryable_v1(
        &self,
        guild_id: GuildId,
        limit: NonZeroUsize,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
        self.0.inner.list_retryable_v1(guild_id, limit).await
    }
}

#[derive(Default)]
struct DeleterState {
    calls: Mutex<Vec<DeleteCall>>,
    outcomes: Mutex<VecDeque<Result<DeleteOutcome, DeleterError>>>,
}

#[derive(Clone, Default)]
struct ScriptedDeleter(Arc<DeleterState>);

impl ScriptedDeleter {
    fn with_outcomes(outcomes: Vec<Result<DeleteOutcome, DeleterError>>) -> Self {
        Self(Arc::new(DeleterState {
            calls: Mutex::new(Vec::new()),
            outcomes: Mutex::new(outcomes.into()),
        }))
    }

    fn calls(&self) -> Vec<DeleteCall> {
        self.0.calls.lock().unwrap().clone()
    }

    fn replace_outcomes(&self, outcomes: Vec<Result<DeleteOutcome, DeleterError>>) {
        *self.0.outcomes.lock().unwrap() = outcomes.into();
    }

    fn next(&self) -> Result<DeleteOutcome, DeleterError> {
        self.0
            .outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(DeleteOutcome::Deleted))
    }
}

impl InstanceDeleter for ScriptedDeleter {
    async fn delete_message(
        &self,
        _: GuildId,
        channel: ChannelId,
        message: MessageId,
    ) -> Result<DeleteOutcome, DeleterError> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(DeleteCall::Message(channel, message));
        self.next()
    }

    async fn delete_channel(
        &self,
        _: GuildId,
        channel: ChannelId,
    ) -> Result<DeleteOutcome, DeleterError> {
        self.0
            .calls
            .lock()
            .unwrap()
            .push(DeleteCall::Channel(channel));
        self.next()
    }

    async fn delete_role(&self, _: GuildId, role: RoleId) -> Result<DeleteOutcome, DeleterError> {
        self.0.calls.lock().unwrap().push(DeleteCall::Role(role));
        self.next()
    }
}

fn instance_id() -> InstanceId {
    InstanceId::parse("room_001").unwrap()
}

fn resources() -> InstanceResources {
    InstanceResources {
        roles: BTreeMap::from([
            ("z_role".to_string(), RoleId(601)),
            ("a_role".to_string(), RoleId(600)),
        ]),
        channels: BTreeMap::from([
            ("z_channel".to_string(), ChannelId(501)),
            ("a_channel".to_string(), ChannelId(500)),
        ]),
        messages: BTreeMap::from([
            (
                "z_message".to_string(),
                InstanceMessageRef {
                    channel: ChannelId(99),
                    id: MessageId(401),
                },
            ),
            (
                "a_message".to_string(),
                InstanceMessageRef {
                    channel: ChannelId(500),
                    id: MessageId(400),
                },
            ),
        ]),
    }
}

fn instance_for_guild(guild_id: GuildId, status: InstanceStatus) -> AutomationInstance {
    AutomationInstance {
        id: instance_id(),
        guild_id,
        ruleset_key: "studyroom_demo".to_string(),
        ruleset_version: InstanceRuleSetVersion::new(1).unwrap(),
        kind: InstanceKind("study_room".to_string()),
        created_by: UserId(42),
        resources: resources(),
        status,
    }
}

fn register(store: &SharedStore, status: InstanceStatus) {
    register_for_guild(store, GUILD, status);
}

fn register_for_guild(store: &SharedStore, guild_id: GuildId, status: InstanceStatus) {
    block_on(store.register(instance_for_guild(guild_id, InstanceStatus::Active))).unwrap();
    if status == InstanceStatus::Deleting {
        block_on(store.transition_to_deleting(guild_id, &instance_id())).unwrap();
    }
    if status == InstanceStatus::Deleted {
        block_on(store.transition_to_deleting(guild_id, &instance_id())).unwrap();
        block_on(store.mark_deleted(guild_id, &instance_id())).unwrap();
    }
}

fn expected_order() -> Vec<DeleteCall> {
    vec![
        DeleteCall::Message(ChannelId(500), MessageId(400)),
        DeleteCall::Message(ChannelId(99), MessageId(401)),
        DeleteCall::Channel(ChannelId(500)),
        DeleteCall::Channel(ChannelId(501)),
        DeleteCall::Role(RoleId(600)),
        DeleteCall::Role(RoleId(601)),
    ]
}

fn exact_request() -> ExactInstanceTeardownRequestV1 {
    ExactInstanceTeardownRequestV1::new(GUILD, instance_id(), resources())
}

#[test]
fn active_instance_deletes_in_order_and_preserves_footprint() {
    let store = SharedStore::default();
    let deleter = ScriptedDeleter::default();
    register(&store, InstanceStatus::Active);
    let service = Teardown::new(store.clone(), deleter.clone());

    let outcome = block_on(service.teardown(GUILD, instance_id())).unwrap();

    assert_eq!(outcome, TeardownOutcome::Completed);
    assert_eq!(deleter.calls(), expected_order());
    let stored = block_on(store.get(GUILD, &instance_id())).unwrap().unwrap();
    assert_eq!(stored.status, InstanceStatus::Deleted);
    assert_eq!(stored.resources, resources());
    assert_eq!(store.transition_calls(), 1);
    assert_eq!(store.mark_calls(), 1);
}

#[test]
fn already_gone_resources_are_successful() {
    let store = SharedStore::default();
    let deleter = ScriptedDeleter::with_outcomes(vec![
        Ok(DeleteOutcome::AlreadyGone),
        Ok(DeleteOutcome::Deleted),
        Ok(DeleteOutcome::AlreadyGone),
        Ok(DeleteOutcome::Deleted),
        Ok(DeleteOutcome::AlreadyGone),
        Ok(DeleteOutcome::Deleted),
    ]);
    register(&store, InstanceStatus::Active);
    let service = Teardown::new(store.clone(), deleter);

    assert_eq!(
        block_on(service.teardown(GUILD, instance_id())).unwrap(),
        TeardownOutcome::Completed
    );
    assert_eq!(
        block_on(store.get(GUILD, &instance_id()))
            .unwrap()
            .unwrap()
            .status,
        InstanceStatus::Deleted
    );
}

#[test]
fn forbidden_channel_stops_before_roles_and_leaves_deleting() {
    let store = SharedStore::default();
    let forbidden = DeleterError {
        kind: DeleterErrorKind::Forbidden,
        message: "forbidden".to_string(),
    };
    let deleter = ScriptedDeleter::with_outcomes(vec![
        Ok(DeleteOutcome::Deleted),
        Ok(DeleteOutcome::Deleted),
        Err(forbidden.clone()),
    ]);
    register(&store, InstanceStatus::Active);
    let service = Teardown::new(store.clone(), deleter.clone());

    let error = block_on(service.teardown(GUILD, instance_id())).unwrap_err();

    assert_eq!(
        error,
        TeardownError::DeleteFailed {
            resource: InstanceResource::Channel {
                alias: "a_channel".to_string(),
                id: ChannelId(500),
            },
            source: forbidden,
        }
    );
    assert_eq!(deleter.calls().len(), 3);
    assert_eq!(
        block_on(store.get(GUILD, &instance_id()))
            .unwrap()
            .unwrap()
            .status,
        InstanceStatus::Deleting
    );
    assert_eq!(store.mark_calls(), 0);
}

#[test]
fn deleting_instance_resumes_with_preserved_footprint() {
    let store = SharedStore::default();
    let deleter = ScriptedDeleter::with_outcomes(vec![Ok(DeleteOutcome::AlreadyGone); 6]);
    register(&store, InstanceStatus::Deleting);
    let transition_calls = store.transition_calls();
    let service = Teardown::new(store.clone(), deleter.clone());

    assert_eq!(
        block_on(service.teardown(GUILD, instance_id())).unwrap(),
        TeardownOutcome::ResumedAndCompleted
    );
    assert_eq!(deleter.calls(), expected_order());
    assert_eq!(store.transition_calls(), transition_calls);
    assert_eq!(
        block_on(store.get(GUILD, &instance_id()))
            .unwrap()
            .unwrap()
            .resources,
        resources()
    );
}

struct BlockingDeleter {
    started: Arc<(Mutex<bool>, Condvar)>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

impl InstanceDeleter for BlockingDeleter {
    async fn delete_message(
        &self,
        _: GuildId,
        _: ChannelId,
        _: MessageId,
    ) -> Result<DeleteOutcome, DeleterError> {
        let (started, started_ready) = &*self.started;
        *started.lock().unwrap() = true;
        started_ready.notify_all();
        let (release, release_ready) = &*self.release;
        let mut released = release.lock().unwrap();
        while !*released {
            released = release_ready.wait(released).unwrap();
        }
        Ok(DeleteOutcome::Deleted)
    }

    async fn delete_channel(
        &self,
        _: GuildId,
        _: ChannelId,
    ) -> Result<DeleteOutcome, DeleterError> {
        Ok(DeleteOutcome::Deleted)
    }

    async fn delete_role(&self, _: GuildId, _: RoleId) -> Result<DeleteOutcome, DeleterError> {
        Ok(DeleteOutcome::Deleted)
    }
}

#[test]
fn identical_guild_and_instance_returns_in_progress_without_store_read() {
    let store = SharedStore::default();
    register(&store, InstanceStatus::Active);
    let started = Arc::new((Mutex::new(false), Condvar::new()));
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let service = Arc::new(Teardown::new(
        store.clone(),
        BlockingDeleter {
            started: started.clone(),
            release: release.clone(),
        },
    ));
    let first_service = service.clone();
    let first =
        std::thread::spawn(move || block_on(first_service.teardown(GUILD, instance_id())).unwrap());
    let (started_lock, started_ready) = &*started;
    let mut has_started = started_lock.lock().unwrap();
    while !*has_started {
        has_started = started_ready.wait(has_started).unwrap();
    }
    drop(has_started);
    let reads = store.get_calls();

    assert_eq!(
        block_on(service.teardown(GUILD, instance_id())).unwrap(),
        TeardownOutcome::InProgress
    );
    assert_eq!(store.get_calls(), reads);
    let (release_lock, release_ready) = &*release;
    *release_lock.lock().unwrap() = true;
    release_ready.notify_all();
    assert_eq!(first.join().unwrap(), TeardownOutcome::Completed);
}

struct GuildBlockingDeleter {
    started: mpsc::Sender<GuildId>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

impl InstanceDeleter for GuildBlockingDeleter {
    async fn delete_message(
        &self,
        guild: GuildId,
        _: ChannelId,
        _: MessageId,
    ) -> Result<DeleteOutcome, DeleterError> {
        self.started.send(guild).unwrap();
        let (release, release_ready) = &*self.release;
        let mut released = release.lock().unwrap();
        while !*released {
            released = release_ready.wait(released).unwrap();
        }
        Ok(DeleteOutcome::Deleted)
    }

    async fn delete_channel(
        &self,
        _: GuildId,
        _: ChannelId,
    ) -> Result<DeleteOutcome, DeleterError> {
        Ok(DeleteOutcome::Deleted)
    }

    async fn delete_role(&self, _: GuildId, _: RoleId) -> Result<DeleteOutcome, DeleterError> {
        Ok(DeleteOutcome::Deleted)
    }
}

#[test]
fn same_instance_id_in_different_guilds_executes_concurrently() {
    let other_guild = GuildId(8);
    let store = SharedStore::default();
    register_for_guild(&store, GUILD, InstanceStatus::Active);
    register_for_guild(&store, other_guild, InstanceStatus::Active);
    let (started_tx, started_rx) = mpsc::channel();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let service = Arc::new(Teardown::new(
        store.clone(),
        GuildBlockingDeleter {
            started: started_tx,
            release: release.clone(),
        },
    ));
    let first_service = service.clone();
    let first =
        std::thread::spawn(move || block_on(first_service.teardown(GUILD, instance_id())).unwrap());
    let first_started = started_rx.recv_timeout(Duration::from_secs(1));
    let second_service = service.clone();
    let second = std::thread::spawn(move || {
        block_on(second_service.teardown(other_guild, instance_id())).unwrap()
    });
    let second_started = started_rx.recv_timeout(Duration::from_secs(1));
    let (release_lock, release_ready) = &*release;
    *release_lock.lock().unwrap() = true;
    release_ready.notify_all();
    let first_outcome = first.join().unwrap();
    let second_outcome = second.join().unwrap();

    assert_eq!(first_started, Ok(GUILD));
    assert_eq!(second_started, Ok(other_guild));
    assert_eq!(first_outcome, TeardownOutcome::Completed);
    assert_eq!(second_outcome, TeardownOutcome::Completed);
    assert_eq!(store.transition_calls(), 2);
    assert_eq!(store.mark_calls(), 2);
}

#[test]
fn deleted_instance_is_already_deleted_without_deletes() {
    let store = SharedStore::default();
    let deleter = ScriptedDeleter::default();
    register(&store, InstanceStatus::Deleted);
    let service = Teardown::new(store, deleter.clone());

    assert_eq!(
        block_on(service.teardown(GUILD, instance_id())).unwrap(),
        TeardownOutcome::AlreadyDeleted
    );
    assert!(deleter.calls().is_empty());
}

#[test]
fn mark_failure_keeps_deleting_and_resume_completes() {
    let store = SharedStore::default();
    let deleter = ScriptedDeleter::default();
    register(&store, InstanceStatus::Active);
    store.set_fail_mark(true);
    let service = Teardown::new(store.clone(), deleter.clone());

    assert!(matches!(
        block_on(service.teardown(GUILD, instance_id())),
        Err(TeardownError::Store(InstanceStoreError::Backend(_)))
    ));
    assert_eq!(
        block_on(store.get(GUILD, &instance_id()))
            .unwrap()
            .unwrap()
            .status,
        InstanceStatus::Deleting
    );
    store.set_fail_mark(false);
    deleter.replace_outcomes(vec![Ok(DeleteOutcome::AlreadyGone); 6]);
    assert_eq!(
        block_on(service.teardown(GUILD, instance_id())).unwrap(),
        TeardownOutcome::ResumedAndCompleted
    );
}

#[test]
fn uncertain_delete_error_is_not_already_gone() {
    let store = SharedStore::default();
    let source = DeleterError {
        kind: DeleterErrorKind::Unknown,
        message: "uncertain".to_string(),
    };
    let deleter = ScriptedDeleter::with_outcomes(vec![Err(source.clone())]);
    register(&store, InstanceStatus::Active);
    let service = Teardown::new(store.clone(), deleter);

    assert_eq!(
        block_on(service.teardown(GUILD, instance_id())).unwrap_err(),
        TeardownError::DeleteFailed {
            resource: InstanceResource::Message {
                alias: "a_message".to_string(),
                channel: ChannelId(500),
                id: MessageId(400),
            },
            source,
        }
    );
    assert_eq!(
        block_on(store.get(GUILD, &instance_id()))
            .unwrap()
            .unwrap()
            .status,
        InstanceStatus::Deleting
    );
    assert_eq!(store.mark_calls(), 0);
}

#[test]
fn exact_teardown_rejects_manifest_drift_before_claim_or_delete() {
    let store = SharedStore::default();
    let deleter = ScriptedDeleter::default();
    register(&store, InstanceStatus::Active);
    let mut drifted = resources();
    drifted.roles.insert("a_role".to_string(), RoleId(999));
    let request = ExactInstanceTeardownRequestV1::new(GUILD, instance_id(), drifted);
    let service = Teardown::new(store.clone(), deleter.clone());

    assert_eq!(
        block_on(service.teardown_exact_v1(&request)),
        Err(TeardownError::ManifestDrift)
    );
    assert!(deleter.calls().is_empty());
    assert_eq!(store.transition_calls(), 0);
    assert_eq!(store.mark_calls(), 0);
    assert_eq!(
        block_on(store.get(GUILD, &instance_id()))
            .unwrap()
            .unwrap()
            .status,
        InstanceStatus::Active
    );
}

#[test]
fn exact_teardown_observation_distinguishes_durable_states() {
    let active_store = SharedStore::default();
    register(&active_store, InstanceStatus::Active);
    let active = Teardown::new(active_store, ScriptedDeleter::default());
    assert_eq!(
        block_on(active.observe_teardown_exact_v1(&exact_request())).unwrap(),
        InstanceTeardownRecoveryObservationV1::ProvenNotStarted
    );

    let deleting_store = SharedStore::default();
    register(&deleting_store, InstanceStatus::Deleting);
    let deleting = Teardown::new(deleting_store, ScriptedDeleter::default());
    assert_eq!(
        block_on(deleting.observe_teardown_exact_v1(&exact_request())).unwrap(),
        InstanceTeardownRecoveryObservationV1::DurableRetryPending
    );

    let deleted_store = SharedStore::default();
    register(&deleted_store, InstanceStatus::Deleted);
    let deleted = Teardown::new(deleted_store, ScriptedDeleter::default());
    assert_eq!(
        block_on(deleted.observe_teardown_exact_v1(&exact_request())).unwrap(),
        InstanceTeardownRecoveryObservationV1::ProvenSucceeded
    );
}

#[test]
fn indeterminate_delete_retries_only_the_exact_manifest_and_converges() {
    let store = SharedStore::default();
    let network = DeleterError {
        kind: DeleterErrorKind::Network,
        message: "timeout".to_string(),
    };
    let deleter =
        ScriptedDeleter::with_outcomes(vec![Ok(DeleteOutcome::Deleted), Err(network.clone())]);
    register(&store, InstanceStatus::Active);
    let service = Teardown::new(store.clone(), deleter.clone());
    let request = exact_request();

    assert_eq!(
        block_on(service.teardown_exact_v1(&request)),
        Err(TeardownError::DeleteFailed {
            resource: InstanceResource::Message {
                alias: "z_message".to_string(),
                channel: ChannelId(99),
                id: MessageId(401),
            },
            source: network,
        })
    );
    assert_eq!(
        block_on(service.observe_teardown_exact_v1(&request)).unwrap(),
        InstanceTeardownRecoveryObservationV1::DurableRetryPending
    );
    deleter.replace_outcomes(vec![Ok(DeleteOutcome::AlreadyGone); 6]);
    assert_eq!(
        block_on(service.teardown_exact_v1(&request)).unwrap(),
        TeardownOutcome::ResumedAndCompleted
    );
    assert_eq!(
        deleter.calls(),
        [
            DeleteCall::Message(ChannelId(500), MessageId(400)),
            DeleteCall::Message(ChannelId(99), MessageId(401)),
        ]
        .into_iter()
        .chain(expected_order())
        .collect::<Vec<_>>()
    );
    assert_eq!(
        block_on(service.observe_teardown_exact_v1(&request)).unwrap(),
        InstanceTeardownRecoveryObservationV1::ProvenSucceeded
    );
}

#[derive(Clone)]
struct ClaimDriftStore(Arc<Mutex<AutomationInstance>>);

impl InstanceTeardownStoreV1 for ClaimDriftStore {
    async fn get_for_teardown_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        let instance = self.0.lock().unwrap().clone();
        Ok((instance.guild_id == guild_id && &instance.id == instance_id).then_some(instance))
    }

    async fn claim_deleting_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<InstanceTeardownClaimOutcomeV1, InstanceStoreError> {
        let mut instance = self.0.lock().unwrap();
        if instance.guild_id != guild_id || &instance.id != instance_id {
            return Err(InstanceStoreError::NotFound);
        }
        instance.status = InstanceStatus::Deleting;
        instance.kind = InstanceKind("drifted".to_string());
        Ok(InstanceTeardownClaimOutcomeV1::Claimed)
    }

    async fn mark_deleted_v1(
        &self,
        _guild_id: GuildId,
        _instance_id: &InstanceId,
    ) -> Result<InstanceTeardownMarkOutcomeV1, InstanceStoreError> {
        Ok(InstanceTeardownMarkOutcomeV1::MarkedDeleted)
    }

    async fn list_retryable_v1(
        &self,
        _guild_id: GuildId,
        _limit: NonZeroUsize,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
        Ok(Vec::new())
    }
}

#[test]
fn identity_complete_teardown_rejects_same_manifest_replacement_after_claim_before_first_delete() {
    let instance = instance_for_guild(GUILD, InstanceStatus::Active);
    let request = ExactInstanceTeardownRequestV1::new_exact_v1(
        instance.guild_id,
        instance.id.clone(),
        instance.resources.clone(),
        ExactInstanceRegistrationIdentityV1::new(
            instance.ruleset_key.clone(),
            instance.ruleset_version,
            instance.kind.clone(),
            instance.created_by,
        ),
    );
    let store = ClaimDriftStore(Arc::new(Mutex::new(instance)));
    let deleter = ScriptedDeleter::default();
    let service = Teardown::new(store, deleter.clone());

    assert_eq!(
        block_on(service.teardown_exact_v1(&request)),
        Err(TeardownError::ManifestDrift)
    );
    assert!(deleter.calls().is_empty());
}
