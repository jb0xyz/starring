use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

use automation_panel_installation::strict::{
    reconcile_declared_panels_strict, StrictDeclaredPanelV1, StrictDeleteOutcomeV1,
    StrictExternalPostResultV1, StrictObservedMessageV1, StrictPanelActionRowPayloadV1,
    StrictPanelActionV1, StrictPanelButtonPayloadV1, StrictPanelCleanupIntentV1,
    StrictPanelCleanupKindV1, StrictPanelInstallKindV1, StrictPanelInstallationStore,
    StrictPanelInstaller, StrictPanelJournalError, StrictPanelMessagePayloadV1,
    StrictPanelMessageRefV1, StrictPanelOperationJournal, StrictPanelOperationKeyV1,
    StrictPanelOperationStateV1, StrictPanelOperationV1, StrictPanelPostIntentV1,
    StrictPanelReconcileErrorV1, StrictPanelReconcileRequestV1, StrictPanelReportV1,
    MAX_STRICT_PANEL_RECORDS_PER_SLOT,
};
use automation_panel_installation::{
    spec_hash, InstallerError, PanelInstallation, PanelInstallationKey, PanelInstallationStore,
    PanelInstallationStoreError,
};
use automation_ruleset::{RuleSetKey, RuleSetVersionId};
use automation_state::{ButtonRoute, ButtonSpec, PanelSpec};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, MessageId};
use futures::executor::block_on;
use resource_resolution::ResourceBindingMap;

const GUILD: GuildId = GuildId(7);
const RENDER_REVISION: u32 = 1;

type SlotKey = (GuildId, RuleSetKey, String);

#[derive(Default)]
struct FakeStore {
    entries: Mutex<BTreeMap<SlotKey, PanelInstallation>>,
}

impl FakeStore {
    fn seed(&self, installation: PanelInstallation) {
        self.entries
            .lock()
            .unwrap()
            .insert(store_key(&installation), installation);
    }

    fn installation(&self, panel_key: &str) -> Option<PanelInstallation> {
        self.entries
            .lock()
            .unwrap()
            .get(&(GUILD, ruleset_key(), panel_key.to_string()))
            .cloned()
    }
}

impl PanelInstallationStore for FakeStore {
    async fn get(
        &self,
        key: &PanelInstallationKey,
    ) -> Result<Option<PanelInstallation>, PanelInstallationStoreError> {
        Ok(self
            .entries
            .lock()
            .unwrap()
            .get(&(key.guild_id, key.ruleset_key.clone(), key.panel_key.clone()))
            .cloned())
    }

    async fn upsert(
        &self,
        installation: PanelInstallation,
    ) -> Result<(), PanelInstallationStoreError> {
        self.entries
            .lock()
            .unwrap()
            .insert(store_key(&installation), installation);
        Ok(())
    }
}

impl StrictPanelInstallationStore for FakeStore {
    async fn list_slot(
        &self,
        guild_id: GuildId,
        ruleset_key: &RuleSetKey,
    ) -> Result<Vec<PanelInstallation>, PanelInstallationStoreError> {
        Ok(self
            .entries
            .lock()
            .unwrap()
            .values()
            .filter(|installation| {
                installation.guild_id == guild_id && &installation.ruleset_key == ruleset_key
            })
            .cloned()
            .collect())
    }

    async fn remove(&self, key: &PanelInstallationKey) -> Result<(), PanelInstallationStoreError> {
        self.entries.lock().unwrap().remove(&(
            key.guild_id,
            key.ruleset_key.clone(),
            key.panel_key.clone(),
        ));
        Ok(())
    }
}

#[derive(Default)]
struct FakeJournal {
    operations: Mutex<BTreeMap<StrictPanelOperationKeyV1, StrictPanelOperationV1>>,
}

impl FakeJournal {
    fn seed(&self, operation: StrictPanelOperationV1) {
        self.operations
            .lock()
            .unwrap()
            .insert(operation.key.clone(), operation);
    }

    fn operation(&self, panel_key: &str) -> Option<StrictPanelOperationV1> {
        self.operations
            .lock()
            .unwrap()
            .get(&operation_key(panel_key))
            .cloned()
    }

    fn is_empty(&self) -> bool {
        self.operations.lock().unwrap().is_empty()
    }
}

impl StrictPanelOperationJournal for FakeJournal {
    async fn list_slot(
        &self,
        guild_id: GuildId,
        ruleset_key: &RuleSetKey,
    ) -> Result<Vec<StrictPanelOperationV1>, StrictPanelJournalError> {
        Ok(self
            .operations
            .lock()
            .unwrap()
            .values()
            .filter(|operation| {
                operation.key.guild_id == guild_id && &operation.key.ruleset_key == ruleset_key
            })
            .cloned()
            .collect())
    }

    async fn put(&self, operation: StrictPanelOperationV1) -> Result<(), StrictPanelJournalError> {
        self.operations
            .lock()
            .unwrap()
            .insert(operation.key.clone(), operation);
        Ok(())
    }

    async fn remove(&self, key: &StrictPanelOperationKeyV1) -> Result<(), StrictPanelJournalError> {
        self.operations.lock().unwrap().remove(key);
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum InstallerCall {
    Observe(ChannelId, MessageId),
    Post(ChannelId, String),
    Delete(ChannelId, MessageId),
}

#[derive(Default)]
struct FakeInstallerState {
    messages: BTreeMap<(ChannelId, MessageId), StrictPanelMessagePayloadV1>,
    post_results: VecDeque<StrictExternalPostResultV1>,
    delete_results: VecDeque<StrictDeleteOutcomeV1>,
    observe_failures: BTreeSet<(ChannelId, MessageId)>,
    calls: Vec<InstallerCall>,
}

#[derive(Default)]
struct FakeInstaller {
    state: Mutex<FakeInstallerState>,
}

impl FakeInstaller {
    fn seed_message(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
        payload: StrictPanelMessagePayloadV1,
    ) {
        self.state
            .lock()
            .unwrap()
            .messages
            .insert((channel_id, message_id), payload);
    }

    fn script_post(&self, result: StrictExternalPostResultV1) {
        self.state.lock().unwrap().post_results.push_back(result);
    }

    fn script_delete(&self, outcome: StrictDeleteOutcomeV1) {
        self.state.lock().unwrap().delete_results.push_back(outcome);
    }

    fn has_message(&self, channel_id: ChannelId, message_id: MessageId) -> bool {
        self.state
            .lock()
            .unwrap()
            .messages
            .contains_key(&(channel_id, message_id))
    }

    fn calls(&self) -> Vec<InstallerCall> {
        self.state.lock().unwrap().calls.clone()
    }
}

impl StrictPanelInstaller for FakeInstaller {
    async fn observe_message(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
    ) -> Result<StrictObservedMessageV1, InstallerError> {
        let mut state = self.state.lock().unwrap();
        state
            .calls
            .push(InstallerCall::Observe(channel_id, message_id));
        if state.observe_failures.contains(&(channel_id, message_id)) {
            return Err(InstallerError::new("observe failed"));
        }
        Ok(match state.messages.get(&(channel_id, message_id)) {
            Some(payload) => StrictObservedMessageV1::Present(payload.clone()),
            None => StrictObservedMessageV1::Missing,
        })
    }

    async fn post_message(
        &self,
        channel_id: ChannelId,
        _guild_id: GuildId,
        _ruleset_key: &str,
        panel: &StrictDeclaredPanelV1,
    ) -> StrictExternalPostResultV1 {
        let mut state = self.state.lock().unwrap();
        state
            .calls
            .push(InstallerCall::Post(channel_id, panel.spec.key.clone()));
        let result = state
            .post_results
            .pop_front()
            .unwrap_or(StrictExternalPostResultV1::DefinitelyNotApplied);
        if let StrictExternalPostResultV1::Applied(message_id) = result {
            state
                .messages
                .insert((channel_id, message_id), panel.expected_payload.clone());
        }
        result
    }

    async fn delete_message(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
    ) -> StrictDeleteOutcomeV1 {
        let mut state = self.state.lock().unwrap();
        state
            .calls
            .push(InstallerCall::Delete(channel_id, message_id));
        let outcome = state.delete_results.pop_front().unwrap_or_else(|| {
            if state.messages.contains_key(&(channel_id, message_id)) {
                StrictDeleteOutcomeV1::Deleted
            } else {
                StrictDeleteOutcomeV1::AlreadyGone
            }
        });
        if matches!(
            outcome,
            StrictDeleteOutcomeV1::Deleted | StrictDeleteOutcomeV1::AlreadyGone
        ) {
            state.messages.remove(&(channel_id, message_id));
        }
        outcome
    }
}

fn ruleset_key() -> RuleSetKey {
    RuleSetKey::parse("studyroom").unwrap()
}

fn version(value: u32) -> RuleSetVersionId {
    RuleSetVersionId::new(value).unwrap()
}

fn panel(key: &str, channel: &str, content: &str) -> StrictDeclaredPanelV1 {
    StrictDeclaredPanelV1 {
        spec: PanelSpec {
            key: key.to_string(),
            channel: ResourceKey(channel.to_string()),
            content: content.to_string(),
            buttons: vec![ButtonSpec {
                label: "Join".to_string(),
                route: ButtonRoute::Static {
                    key: "join".to_string(),
                },
            }],
        },
        expected_payload: StrictPanelMessagePayloadV1 {
            content: content.to_string(),
            action_rows: vec![StrictPanelActionRowPayloadV1 {
                buttons: vec![StrictPanelButtonPayloadV1 {
                    label: "Join".to_string(),
                    custom_id: format!("starring:s:7:studyroom:{key}:join"),
                    style: "primary".to_string(),
                    disabled: false,
                }],
            }],
        },
    }
}

fn bindings(entries: &[(&str, u64)]) -> ResourceBindingMap {
    let mut bindings = ResourceBindingMap::default();
    for (key, channel_id) in entries {
        bindings
            .channel_bindings
            .insert(ResourceKey((*key).to_string()), ChannelId(*channel_id));
    }
    bindings
}

fn installation(
    panel: &StrictDeclaredPanelV1,
    ruleset_version: RuleSetVersionId,
    channel_id: ChannelId,
    message_id: MessageId,
) -> PanelInstallation {
    PanelInstallation {
        guild_id: GUILD,
        ruleset_key: ruleset_key(),
        panel_key: panel.spec.key.clone(),
        installed_version: ruleset_version,
        channel_id,
        message_id,
        spec_hash: spec_hash(RENDER_REVISION, &panel.spec),
    }
}

fn post_intent(
    panel: StrictDeclaredPanelV1,
    channel_id: ChannelId,
    install_kind: StrictPanelInstallKindV1,
) -> StrictPanelPostIntentV1 {
    StrictPanelPostIntentV1 {
        spec_hash: spec_hash(RENDER_REVISION, &panel.spec),
        panel,
        ruleset_version: version(1),
        channel_id,
        install_kind,
        previous_message: None,
    }
}

fn operation_key(panel_key: &str) -> StrictPanelOperationKeyV1 {
    StrictPanelOperationKeyV1 {
        guild_id: GUILD,
        ruleset_key: ruleset_key(),
        panel_key: panel_key.to_string(),
    }
}

fn store_key(installation: &PanelInstallation) -> SlotKey {
    (
        installation.guild_id,
        installation.ruleset_key.clone(),
        installation.panel_key.clone(),
    )
}

fn run(
    panels: &[StrictDeclaredPanelV1],
    bindings: &ResourceBindingMap,
    store: &FakeStore,
    installer: &FakeInstaller,
    journal: &FakeJournal,
) -> StrictPanelReportV1 {
    run_result(panels, bindings, store, installer, journal).unwrap()
}

fn run_result(
    panels: &[StrictDeclaredPanelV1],
    bindings: &ResourceBindingMap,
    store: &FakeStore,
    installer: &FakeInstaller,
    journal: &FakeJournal,
) -> Result<StrictPanelReportV1, StrictPanelReconcileErrorV1> {
    block_on(reconcile_declared_panels_strict(
        StrictPanelReconcileRequestV1 {
            guild_id: GUILD,
            ruleset_key: &ruleset_key(),
            ruleset_version: version(1),
            render_revision: RENDER_REVISION,
            panels,
            bindings,
        },
        store,
        installer,
        journal,
    ))
}

fn has_action(report: &StrictPanelReportV1, action: StrictPanelActionV1) -> bool {
    report
        .outcomes
        .iter()
        .any(|outcome| outcome.action == action)
}

#[test]
fn fresh_install_and_exact_recheck_are_eligible() {
    let store = FakeStore::default();
    let installer = FakeInstaller::default();
    let journal = FakeJournal::default();
    let declared = panel("entry", "hub", "hello");
    installer.script_post(StrictExternalPostResultV1::Applied(MessageId(100)));
    let first = run(
        std::slice::from_ref(&declared),
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
        &journal,
    );
    assert_eq!(first.declared_count, 1);
    assert_eq!(first.installed_count, 1);
    assert_eq!(first.unchanged_count, 0);
    assert!(first.is_eligible());
    assert!(journal.is_empty());
    assert_eq!(
        store.installation("entry").unwrap().message_id,
        MessageId(100)
    );
    let second = run(
        std::slice::from_ref(&declared),
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
        &journal,
    );
    assert_eq!(second.installed_count, 0);
    assert_eq!(second.unchanged_count, 1);
    assert!(second.is_eligible());
}

#[test]
fn observed_payload_mismatch_reposts_and_cleans_old_message() {
    let store = FakeStore::default();
    let installer = FakeInstaller::default();
    let journal = FakeJournal::default();
    let declared = panel("entry", "hub", "desired");
    store.seed(installation(
        &declared,
        version(1),
        ChannelId(10),
        MessageId(100),
    ));
    installer.seed_message(
        ChannelId(10),
        MessageId(100),
        panel("entry", "hub", "tampered").expected_payload,
    );
    installer.script_post(StrictExternalPostResultV1::Applied(MessageId(200)));
    let report = run(
        std::slice::from_ref(&declared),
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
        &journal,
    );
    assert_eq!(report.installed_count, 1);
    assert!(has_action(
        &report,
        StrictPanelActionV1::Installed(StrictPanelInstallKindV1::PayloadReplaced)
    ));
    assert!(has_action(
        &report,
        StrictPanelActionV1::CleanupCompleted(StrictPanelCleanupKindV1::PayloadReplaced)
    ));
    assert!(report.is_eligible());
    assert!(!installer.has_message(ChannelId(10), MessageId(100)));
    assert_eq!(
        store.installation("entry").unwrap().message_id,
        MessageId(200)
    );
}

#[test]
fn channel_move_stays_ineligible_until_old_message_cleanup_resumes() {
    let store = FakeStore::default();
    let installer = FakeInstaller::default();
    let journal = FakeJournal::default();
    let declared = panel("entry", "new_hub", "hello");
    store.seed(installation(
        &declared,
        version(1),
        ChannelId(10),
        MessageId(100),
    ));
    installer.seed_message(
        ChannelId(10),
        MessageId(100),
        declared.expected_payload.clone(),
    );
    installer.script_post(StrictExternalPostResultV1::Applied(MessageId(200)));
    installer.script_delete(StrictDeleteOutcomeV1::Ambiguous);
    let first = run(
        std::slice::from_ref(&declared),
        &bindings(&[("new_hub", 20)]),
        &store,
        &installer,
        &journal,
    );
    assert_eq!(first.installed_count, 0);
    assert_eq!(first.ambiguous_outcome_count, 1);
    assert_eq!(first.reposted_old_message_cleanup_pending_count, 1);
    assert!(!first.is_eligible());
    assert!(matches!(
        journal.operation("entry").unwrap().state,
        StrictPanelOperationStateV1::PostApplied { .. }
    ));
    assert_eq!(
        store.installation("entry").unwrap().message_id,
        MessageId(100)
    );
    let second = run(
        std::slice::from_ref(&declared),
        &bindings(&[("new_hub", 20)]),
        &store,
        &installer,
        &journal,
    );
    assert_eq!(second.installed_count, 1);
    assert_eq!(second.reposted_old_message_cleanup_pending_count, 0);
    assert!(has_action(
        &second,
        StrictPanelActionV1::CleanupCompleted(StrictPanelCleanupKindV1::ChannelMoved)
    ));
    assert!(second.is_eligible());
    assert!(journal.is_empty());
    assert!(!installer.has_message(ChannelId(10), MessageId(100)));
}

#[test]
fn removed_panel_stays_ineligible_until_stale_message_is_deleted() {
    let store = FakeStore::default();
    let installer = FakeInstaller::default();
    let journal = FakeJournal::default();
    let removed = panel("removed", "hub", "old");
    store.seed(installation(
        &removed,
        version(1),
        ChannelId(10),
        MessageId(100),
    ));
    installer.seed_message(ChannelId(10), MessageId(100), removed.expected_payload);
    installer.script_delete(StrictDeleteOutcomeV1::DefinitelyNotApplied);
    let first = run(&[], &bindings(&[("hub", 10)]), &store, &installer, &journal);
    assert_eq!(first.declared_count, 0);
    assert_eq!(first.stale_message_cleanup_pending_count, 1);
    assert!(!first.is_eligible());
    let second = run(&[], &bindings(&[("hub", 10)]), &store, &installer, &journal);
    assert_eq!(second.stale_message_cleanup_pending_count, 0);
    assert!(has_action(
        &second,
        StrictPanelActionV1::CleanupCompleted(StrictPanelCleanupKindV1::Removed)
    ));
    assert!(second.is_eligible());
    assert!(store.installation("removed").is_none());
}

#[test]
fn post_result_distinguishes_definite_failure_from_ambiguity() {
    let declared = panel("entry", "hub", "hello");
    let store = FakeStore::default();
    let installer = FakeInstaller::default();
    let journal = FakeJournal::default();
    installer.script_post(StrictExternalPostResultV1::DefinitelyNotApplied);
    let definite = run(
        std::slice::from_ref(&declared),
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
        &journal,
    );
    assert_eq!(definite.failed_count, 1);
    assert_eq!(definite.ambiguous_outcome_count, 0);
    assert_eq!(definite.orphan_message_cleanup_pending_count, 0);
    assert!(journal.is_empty());
    let store = FakeStore::default();
    let installer = FakeInstaller::default();
    let journal = FakeJournal::default();
    installer.script_post(StrictExternalPostResultV1::Ambiguous);
    let ambiguous = run(
        std::slice::from_ref(&declared),
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
        &journal,
    );
    assert_eq!(ambiguous.failed_count, 0);
    assert_eq!(ambiguous.ambiguous_outcome_count, 1);
    assert_eq!(ambiguous.orphan_message_cleanup_pending_count, 1);
    assert!(matches!(
        journal.operation("entry").unwrap().state,
        StrictPanelOperationStateV1::AmbiguousPost { .. }
    ));
    assert!(!ambiguous.is_eligible());
}

#[test]
fn applied_post_journal_resumes_after_crash_without_reposting() {
    let store = FakeStore::default();
    let installer = FakeInstaller::default();
    let journal = FakeJournal::default();
    let declared = panel("entry", "hub", "hello");
    let intent = post_intent(
        declared.clone(),
        ChannelId(10),
        StrictPanelInstallKindV1::Fresh,
    );
    journal.seed(StrictPanelOperationV1 {
        key: operation_key("entry"),
        state: StrictPanelOperationStateV1::PostApplied {
            intent,
            message_id: MessageId(500),
        },
    });
    installer.seed_message(
        ChannelId(10),
        MessageId(500),
        declared.expected_payload.clone(),
    );
    let report = run(
        std::slice::from_ref(&declared),
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
        &journal,
    );
    assert_eq!(report.installed_count, 1);
    assert!(report.is_eligible());
    assert!(journal.is_empty());
    assert_eq!(
        store.installation("entry").unwrap().message_id,
        MessageId(500)
    );
    assert!(!installer
        .calls()
        .iter()
        .any(|call| matches!(call, InstallerCall::Post(_, _))));
}

#[test]
fn crash_in_post_dispatch_is_persisted_as_unknown_ambiguous_post() {
    let store = FakeStore::default();
    let installer = FakeInstaller::default();
    let journal = FakeJournal::default();
    let declared = panel("entry", "hub", "hello");
    journal.seed(StrictPanelOperationV1 {
        key: operation_key("entry"),
        state: StrictPanelOperationStateV1::PostDispatching {
            intent: post_intent(
                declared.clone(),
                ChannelId(10),
                StrictPanelInstallKindV1::Fresh,
            ),
        },
    });
    let report = run(
        std::slice::from_ref(&declared),
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
        &journal,
    );
    assert_eq!(report.ambiguous_outcome_count, 1);
    assert_eq!(report.orphan_message_cleanup_pending_count, 1);
    assert!(matches!(
        journal.operation("entry").unwrap().state,
        StrictPanelOperationStateV1::AmbiguousPost { .. }
    ));
    assert!(!installer
        .calls()
        .iter()
        .any(|call| matches!(call, InstallerCall::Post(_, _))));
    assert!(!report.is_eligible());
}

#[test]
fn stale_applied_post_is_cleaned_as_orphan_before_new_post() {
    let store = FakeStore::default();
    let installer = FakeInstaller::default();
    let journal = FakeJournal::default();
    let old = panel("entry", "hub", "old");
    let desired = panel("entry", "hub", "new");
    journal.seed(StrictPanelOperationV1 {
        key: operation_key("entry"),
        state: StrictPanelOperationStateV1::PostApplied {
            intent: post_intent(old.clone(), ChannelId(10), StrictPanelInstallKindV1::Fresh),
            message_id: MessageId(500),
        },
    });
    installer.seed_message(ChannelId(10), MessageId(500), old.expected_payload);
    installer.script_post(StrictExternalPostResultV1::Applied(MessageId(600)));
    let report = run(
        std::slice::from_ref(&desired),
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
        &journal,
    );
    assert!(has_action(
        &report,
        StrictPanelActionV1::CleanupCompleted(StrictPanelCleanupKindV1::Orphan)
    ));
    assert_eq!(report.installed_count, 1);
    assert!(report.is_eligible());
    assert!(!installer.has_message(ChannelId(10), MessageId(500)));
    assert_eq!(
        store.installation("entry").unwrap().message_id,
        MessageId(600)
    );
}

#[test]
fn unresolved_binding_and_incomplete_accounting_are_ineligible() {
    let store = FakeStore::default();
    let installer = FakeInstaller::default();
    let journal = FakeJournal::default();
    let declared = panel("entry", "hub", "hello");
    let unresolved = run(
        std::slice::from_ref(&declared),
        &ResourceBindingMap::default(),
        &store,
        &installer,
        &journal,
    );
    assert_eq!(unresolved.skipped_unresolved_channel_count, 1);
    assert!(!unresolved.is_eligible());
    let mut report = StrictPanelReportV1 {
        declared_count: 1,
        ..StrictPanelReportV1::default()
    };
    assert!(!report.is_eligible());
    report.installed_count = 1;
    assert!(report.is_eligible());
    report.orphan_message_cleanup_pending_count = 1;
    assert!(!report.is_eligible());
}

#[test]
fn durable_operation_states_roundtrip_strictly() {
    let declared = panel("entry", "hub", "hello");
    let operation = StrictPanelOperationV1 {
        key: operation_key("entry"),
        state: StrictPanelOperationStateV1::PostApplied {
            intent: post_intent(declared, ChannelId(10), StrictPanelInstallKindV1::Fresh),
            message_id: MessageId(100),
        },
    };
    let encoded = serde_json::to_string(&operation).unwrap();
    assert_eq!(
        serde_json::from_str::<StrictPanelOperationV1>(&encoded).unwrap(),
        operation
    );
}

#[test]
fn declared_panel_count_is_rejected_before_external_work() {
    let store = FakeStore::default();
    let installer = FakeInstaller::default();
    let journal = FakeJournal::default();
    let panels = (0..=MAX_STRICT_PANEL_RECORDS_PER_SLOT)
        .map(|index| panel(&format!("panel_{index}"), "hub", "hello"))
        .collect::<Vec<_>>();
    let error = run_result(
        &panels,
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
        &journal,
    )
    .unwrap_err();
    assert_eq!(
        error,
        StrictPanelReconcileErrorV1::TooManyDeclaredPanels {
            count: MAX_STRICT_PANEL_RECORDS_PER_SLOT + 1
        }
    );
    assert!(installer.calls().is_empty());
    assert!(journal.is_empty());
}

#[test]
fn oversized_panel_identity_and_payload_are_rejected_before_external_work() {
    let store = FakeStore::default();
    let installer = FakeInstaller::default();
    let journal = FakeJournal::default();
    let oversized_key = panel(&"k".repeat(129), "hub", "hello");
    assert!(matches!(
        run_result(
            &[oversized_key],
            &bindings(&[("hub", 10)]),
            &store,
            &installer,
            &journal,
        ),
        Err(StrictPanelReconcileErrorV1::InvalidOperation { .. })
    ));
    let oversized_payload = panel("entry", "hub", &"x".repeat(250_000));
    assert!(matches!(
        run_result(
            &[oversized_payload],
            &bindings(&[("hub", 10)]),
            &store,
            &installer,
            &journal,
        ),
        Err(StrictPanelReconcileErrorV1::InvalidOperation { .. })
    ));
    assert!(installer.calls().is_empty());
    assert!(journal.is_empty());
}

#[test]
fn retained_cleanup_keys_block_new_posts_before_slot_overflow() {
    let store = FakeStore::default();
    let installer = FakeInstaller::default();
    let journal = FakeJournal::default();
    for index in 0..MAX_STRICT_PANEL_RECORDS_PER_SLOT {
        let key = format!("old_{index}");
        let declared = panel(&key, "hub", "old");
        journal.seed(StrictPanelOperationV1 {
            key: operation_key(&key),
            state: StrictPanelOperationStateV1::AmbiguousPost {
                intent: post_intent(declared, ChannelId(10), StrictPanelInstallKindV1::Fresh),
            },
        });
    }
    let desired = panel("new", "hub", "new");
    let error = run_result(
        &[desired],
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
        &journal,
    )
    .unwrap_err();
    assert_eq!(
        error,
        StrictPanelReconcileErrorV1::SlotCapacityExceeded {
            count: MAX_STRICT_PANEL_RECORDS_PER_SLOT + 1
        }
    );
    assert!(!installer
        .calls()
        .iter()
        .any(|call| matches!(call, InstallerCall::Post(_, _))));
}

#[test]
fn invalid_cleanup_disposition_fails_before_discord_delete() {
    let store = FakeStore::default();
    let installer = FakeInstaller::default();
    let journal = FakeJournal::default();
    journal.seed(StrictPanelOperationV1 {
        key: operation_key("old"),
        state: StrictPanelOperationStateV1::CleanupPending {
            intent: StrictPanelCleanupIntentV1 {
                message: StrictPanelMessageRefV1 {
                    channel_id: ChannelId(10),
                    message_id: MessageId(100),
                },
                kind: StrictPanelCleanupKindV1::Orphan,
                remove_installation: true,
            },
        },
    });
    assert!(matches!(
        run_result(
            &[],
            &ResourceBindingMap::default(),
            &store,
            &installer,
            &journal,
        ),
        Err(StrictPanelReconcileErrorV1::InvalidOperation { .. })
    ));
    assert!(!installer
        .calls()
        .iter()
        .any(|call| matches!(call, InstallerCall::Delete(_, _))));
}

#[test]
fn invalid_post_disposition_fails_before_discord_delete() {
    let store = FakeStore::default();
    let installer = FakeInstaller::default();
    let journal = FakeJournal::default();
    let declared = panel("entry", "hub", "hello");
    let mut intent = post_intent(
        declared.clone(),
        ChannelId(10),
        StrictPanelInstallKindV1::Fresh,
    );
    intent.previous_message = Some(
        automation_panel_installation::strict::StrictPanelPreviousMessageV1 {
            message: StrictPanelMessageRefV1 {
                channel_id: ChannelId(20),
                message_id: MessageId(200),
            },
            cleanup_kind: StrictPanelCleanupKindV1::PayloadReplaced,
        },
    );
    journal.seed(StrictPanelOperationV1 {
        key: operation_key("entry"),
        state: StrictPanelOperationStateV1::PostApplied {
            intent,
            message_id: MessageId(300),
        },
    });
    assert!(matches!(
        run_result(
            &[declared],
            &bindings(&[("hub", 10)]),
            &store,
            &installer,
            &journal,
        ),
        Err(StrictPanelReconcileErrorV1::InvalidOperation { .. })
    ));
    assert!(installer.calls().is_empty());
}

#[test]
fn missing_installation_never_authorizes_deleting_a_present_message() {
    let store = FakeStore::default();
    let installer = FakeInstaller::default();
    let journal = FakeJournal::default();
    let payload = panel("old", "hub", "old").expected_payload;
    installer.seed_message(ChannelId(10), MessageId(100), payload);
    journal.seed(StrictPanelOperationV1 {
        key: operation_key("old"),
        state: StrictPanelOperationStateV1::CleanupPending {
            intent: StrictPanelCleanupIntentV1 {
                message: StrictPanelMessageRefV1 {
                    channel_id: ChannelId(10),
                    message_id: MessageId(100),
                },
                kind: StrictPanelCleanupKindV1::Removed,
                remove_installation: true,
            },
        },
    });
    assert!(matches!(
        run_result(
            &[],
            &ResourceBindingMap::default(),
            &store,
            &installer,
            &journal,
        ),
        Err(StrictPanelReconcileErrorV1::InvalidJournalState(_))
    ));
    assert!(installer.has_message(ChannelId(10), MessageId(100)));
    assert!(!installer
        .calls()
        .iter()
        .any(|call| matches!(call, InstallerCall::Delete(_, _))));
}
