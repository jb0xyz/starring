use std::sync::Mutex;

use automation_panel_installation::{
    install_declared_panels, spec_hash, InMemoryPanelInstallationStore, InstallError,
    InstallerError, PanelAction, PanelEditOutcome, PanelInstallation, PanelInstallationKey,
    PanelInstallationStore, PanelInstallationStoreError, PanelInstaller, PanelPresence,
};
use automation_ruleset::{RuleSetKey, RuleSetVersionId};
use automation_state::{ButtonRoute, ButtonSpec, PanelSpec};
use desired_state::ResourceKey;
use discord_model::{ChannelId, GuildId, MessageId};
use futures::executor::block_on;
use resource_resolution::ResourceBindingMap;

const GUILD: GuildId = GuildId(7);
const REVISION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Call {
    Fetch(ChannelId, MessageId),
    Post(ChannelId),
    Edit(ChannelId, MessageId),
}

struct ScriptedInstaller {
    calls: Mutex<Vec<Call>>,
    fetch: Result<PanelPresence, InstallerError>,
    post: Result<MessageId, InstallerError>,
    edit: Result<PanelEditOutcome, InstallerError>,
}

impl ScriptedInstaller {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            fetch: Ok(PanelPresence::Present),
            post: Ok(MessageId(200)),
            edit: Ok(PanelEditOutcome::Updated),
        }
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
}

impl PanelInstaller for ScriptedInstaller {
    async fn fetch_message(
        &self,
        channel: ChannelId,
        message: MessageId,
    ) -> Result<PanelPresence, InstallerError> {
        self.calls
            .lock()
            .unwrap()
            .push(Call::Fetch(channel, message));
        self.fetch.clone()
    }

    async fn post_message(
        &self,
        channel: ChannelId,
        _guild: GuildId,
        _ruleset_key: &str,
        _spec: &PanelSpec,
    ) -> Result<MessageId, InstallerError> {
        self.calls.lock().unwrap().push(Call::Post(channel));
        self.post.clone()
    }

    async fn edit_message(
        &self,
        channel: ChannelId,
        message: MessageId,
        _guild: GuildId,
        _ruleset_key: &str,
        _spec: &PanelSpec,
    ) -> Result<PanelEditOutcome, InstallerError> {
        self.calls
            .lock()
            .unwrap()
            .push(Call::Edit(channel, message));
        self.edit.clone()
    }
}

struct FailingStore;

impl PanelInstallationStore for FailingStore {
    async fn get(
        &self,
        _key: &PanelInstallationKey,
    ) -> Result<Option<PanelInstallation>, PanelInstallationStoreError> {
        Err(PanelInstallationStoreError::Backend(
            "store failed".to_string(),
        ))
    }

    async fn upsert(
        &self,
        _installation: PanelInstallation,
    ) -> Result<(), PanelInstallationStoreError> {
        Err(PanelInstallationStoreError::Backend(
            "store failed".to_string(),
        ))
    }
}

fn ruleset_key() -> RuleSetKey {
    RuleSetKey::parse("studyroom").unwrap()
}

fn version(value: u32) -> RuleSetVersionId {
    RuleSetVersionId::new(value).unwrap()
}

fn panel(key: &str, channel: &str, content: &str) -> PanelSpec {
    PanelSpec {
        key: key.to_string(),
        channel: ResourceKey(channel.to_string()),
        content: content.to_string(),
        buttons: vec![ButtonSpec {
            label: "Join".to_string(),
            route: ButtonRoute::Static {
                key: "join".to_string(),
            },
        }],
    }
}

fn bindings(entries: &[(&str, u64)]) -> ResourceBindingMap {
    let mut bindings = ResourceBindingMap::default();
    for (key, channel) in entries {
        bindings
            .channel_bindings
            .insert(ResourceKey((*key).to_string()), ChannelId(*channel));
    }
    bindings
}

fn installation(
    spec: &PanelSpec,
    installed_version: RuleSetVersionId,
    channel_id: ChannelId,
    message_id: MessageId,
    revision: u32,
) -> PanelInstallation {
    PanelInstallation {
        guild_id: GUILD,
        ruleset_key: ruleset_key(),
        panel_key: spec.key.clone(),
        installed_version,
        channel_id,
        message_id,
        spec_hash: spec_hash(revision, spec),
    }
}

fn key(panel_key: &str) -> PanelInstallationKey {
    PanelInstallationKey {
        guild_id: GUILD,
        ruleset_key: ruleset_key(),
        panel_key: panel_key.to_string(),
    }
}

fn run<S: PanelInstallationStore, I: PanelInstaller>(
    panels: &[PanelSpec],
    ruleset_version: RuleSetVersionId,
    revision: u32,
    bindings: &ResourceBindingMap,
    store: &S,
    installer: &I,
) -> Result<automation_panel_installation::InstallReport, InstallError> {
    block_on(install_declared_panels(
        GUILD,
        &ruleset_key(),
        ruleset_version,
        revision,
        panels,
        bindings,
        store,
        installer,
    ))
}

#[test]
fn fresh_install_posts_and_persists() {
    let store = InMemoryPanelInstallationStore::new();
    let installer = ScriptedInstaller::new();
    let spec = panel("panel", "hub", "content");
    let report = run(
        std::slice::from_ref(&spec),
        version(1),
        REVISION,
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
    )
    .unwrap();
    assert_eq!(report.outcomes[0].action, PanelAction::Posted);
    assert_eq!(installer.calls(), vec![Call::Post(ChannelId(10))]);
    let stored = block_on(store.get(&key("panel"))).unwrap().unwrap();
    assert_eq!(stored.message_id, MessageId(200));
    assert_eq!(stored.installed_version, version(1));
}

#[test]
fn matching_record_is_noop_without_mutation() {
    let store = InMemoryPanelInstallationStore::new();
    let installer = ScriptedInstaller::new();
    let spec = panel("panel", "hub", "content");
    block_on(store.upsert(installation(
        &spec,
        version(1),
        ChannelId(10),
        MessageId(100),
        REVISION,
    )))
    .unwrap();
    let report = run(
        std::slice::from_ref(&spec),
        version(1),
        REVISION,
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
    )
    .unwrap();
    assert_eq!(report.outcomes[0].action, PanelAction::NoOp);
    assert_eq!(
        installer.calls(),
        vec![Call::Fetch(ChannelId(10), MessageId(100))]
    );
}

#[test]
fn version_change_updates_only_persistence() {
    let store = InMemoryPanelInstallationStore::new();
    let installer = ScriptedInstaller::new();
    let spec = panel("panel", "hub", "content");
    block_on(store.upsert(installation(
        &spec,
        version(1),
        ChannelId(10),
        MessageId(100),
        REVISION,
    )))
    .unwrap();
    let report = run(
        std::slice::from_ref(&spec),
        version(2),
        REVISION,
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
    )
    .unwrap();
    assert_eq!(report.outcomes[0].action, PanelAction::PersistenceUpdated);
    assert_eq!(
        installer.calls(),
        vec![Call::Fetch(ChannelId(10), MessageId(100))]
    );
    assert_eq!(
        block_on(store.get(&key("panel")))
            .unwrap()
            .unwrap()
            .installed_version,
        version(2)
    );
}

#[test]
fn changed_hash_edits_existing_message() {
    let store = InMemoryPanelInstallationStore::new();
    let installer = ScriptedInstaller::new();
    let old = panel("panel", "hub", "old");
    let desired = panel("panel", "hub", "new");
    block_on(store.upsert(installation(
        &old,
        version(1),
        ChannelId(10),
        MessageId(100),
        REVISION,
    )))
    .unwrap();
    let report = run(
        std::slice::from_ref(&desired),
        version(2),
        REVISION,
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
    )
    .unwrap();
    assert_eq!(report.outcomes[0].action, PanelAction::Edited);
    assert_eq!(
        installer.calls(),
        vec![
            Call::Fetch(ChannelId(10), MessageId(100)),
            Call::Edit(ChannelId(10), MessageId(100)),
        ]
    );
    let stored = block_on(store.get(&key("panel"))).unwrap().unwrap();
    assert_eq!(stored.message_id, MessageId(100));
    assert_eq!(stored.spec_hash, spec_hash(REVISION, &desired));
}

#[test]
fn gone_message_is_reposted() {
    let store = InMemoryPanelInstallationStore::new();
    let mut installer = ScriptedInstaller::new();
    installer.fetch = Ok(PanelPresence::Gone);
    let spec = panel("panel", "hub", "content");
    block_on(store.upsert(installation(
        &spec,
        version(1),
        ChannelId(10),
        MessageId(100),
        REVISION,
    )))
    .unwrap();
    let report = run(
        std::slice::from_ref(&spec),
        version(1),
        REVISION,
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
    )
    .unwrap();
    assert_eq!(report.outcomes[0].action, PanelAction::Reposted);
    assert_eq!(
        installer.calls(),
        vec![
            Call::Fetch(ChannelId(10), MessageId(100)),
            Call::Post(ChannelId(10)),
        ]
    );
    assert_eq!(
        block_on(store.get(&key("panel")))
            .unwrap()
            .unwrap()
            .message_id,
        MessageId(200)
    );
}

#[test]
fn gone_during_edit_is_reposted() {
    let store = InMemoryPanelInstallationStore::new();
    let mut installer = ScriptedInstaller::new();
    installer.edit = Ok(PanelEditOutcome::Gone);
    let old = panel("panel", "hub", "old");
    let desired = panel("panel", "hub", "new");
    block_on(store.upsert(installation(
        &old,
        version(1),
        ChannelId(10),
        MessageId(100),
        REVISION,
    )))
    .unwrap();
    let report = run(
        std::slice::from_ref(&desired),
        version(2),
        REVISION,
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
    )
    .unwrap();
    assert_eq!(report.outcomes[0].action, PanelAction::Reposted);
    assert_eq!(
        installer.calls(),
        vec![
            Call::Fetch(ChannelId(10), MessageId(100)),
            Call::Edit(ChannelId(10), MessageId(100)),
            Call::Post(ChannelId(10)),
        ]
    );
}

#[test]
fn fetch_error_is_transient_and_keeps_record() {
    let store = InMemoryPanelInstallationStore::new();
    let mut installer = ScriptedInstaller::new();
    installer.fetch = Err(InstallerError::new("network"));
    let spec = panel("panel", "hub", "content");
    let original = installation(&spec, version(1), ChannelId(10), MessageId(100), REVISION);
    block_on(store.upsert(original.clone())).unwrap();
    let report = run(
        std::slice::from_ref(&spec),
        version(2),
        REVISION,
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
    )
    .unwrap();
    assert_eq!(report.outcomes[0].action, PanelAction::SkippedTransient);
    assert_eq!(
        installer.calls(),
        vec![Call::Fetch(ChannelId(10), MessageId(100))]
    );
    assert_eq!(block_on(store.get(&key("panel"))).unwrap(), Some(original));
}

#[test]
fn edit_error_is_transient_and_keeps_record() {
    let store = InMemoryPanelInstallationStore::new();
    let mut installer = ScriptedInstaller::new();
    installer.edit = Err(InstallerError::new("forbidden"));
    let old = panel("panel", "hub", "old");
    let desired = panel("panel", "hub", "new");
    let original = installation(&old, version(1), ChannelId(10), MessageId(100), REVISION);
    block_on(store.upsert(original.clone())).unwrap();
    let report = run(
        std::slice::from_ref(&desired),
        version(2),
        REVISION,
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
    )
    .unwrap();
    assert_eq!(report.outcomes[0].action, PanelAction::SkippedTransient);
    assert_eq!(
        installer.calls(),
        vec![
            Call::Fetch(ChannelId(10), MessageId(100)),
            Call::Edit(ChannelId(10), MessageId(100)),
        ]
    );
    assert_eq!(block_on(store.get(&key("panel"))).unwrap(), Some(original));
}

#[test]
fn channel_change_reposts_without_fetching_old_message() {
    let store = InMemoryPanelInstallationStore::new();
    let installer = ScriptedInstaller::new();
    let spec = panel("panel", "hub", "content");
    block_on(store.upsert(installation(
        &spec,
        version(1),
        ChannelId(10),
        MessageId(100),
        REVISION,
    )))
    .unwrap();
    let report = run(
        std::slice::from_ref(&spec),
        version(2),
        REVISION,
        &bindings(&[("hub", 20)]),
        &store,
        &installer,
    )
    .unwrap();
    assert_eq!(report.outcomes[0].action, PanelAction::RepostedNewChannel);
    assert_eq!(installer.calls(), vec![Call::Post(ChannelId(20))]);
    let stored = block_on(store.get(&key("panel"))).unwrap().unwrap();
    assert_eq!(stored.channel_id, ChannelId(20));
    assert_eq!(stored.message_id, MessageId(200));
}

#[test]
fn store_error_is_fatal() {
    let installer = ScriptedInstaller::new();
    let spec = panel("panel", "hub", "content");
    assert_eq!(
        run(
            &[spec],
            version(1),
            REVISION,
            &bindings(&[("hub", 10)]),
            &FailingStore,
            &installer,
        )
        .unwrap_err(),
        InstallError::Store(PanelInstallationStoreError::Backend(
            "store failed".to_string()
        ))
    );
}

#[test]
fn render_revision_bump_edits() {
    let store = InMemoryPanelInstallationStore::new();
    let installer = ScriptedInstaller::new();
    let spec = panel("panel", "hub", "content");
    block_on(store.upsert(installation(
        &spec,
        version(1),
        ChannelId(10),
        MessageId(100),
        REVISION,
    )))
    .unwrap();
    let report = run(
        std::slice::from_ref(&spec),
        version(1),
        REVISION + 1,
        &bindings(&[("hub", 10)]),
        &store,
        &installer,
    )
    .unwrap();
    assert_eq!(report.outcomes[0].action, PanelAction::Edited);
    assert_eq!(
        installer.calls(),
        vec![
            Call::Fetch(ChannelId(10), MessageId(100)),
            Call::Edit(ChannelId(10), MessageId(100)),
        ]
    );
}

#[test]
fn unresolved_channel_is_skipped() {
    let store = InMemoryPanelInstallationStore::new();
    let installer = ScriptedInstaller::new();
    let spec = panel("panel", "missing", "content");
    let report = run(
        &[spec],
        version(1),
        REVISION,
        &ResourceBindingMap::default(),
        &store,
        &installer,
    )
    .unwrap();
    assert_eq!(
        report.outcomes[0].action,
        PanelAction::SkippedUnresolvedChannel
    );
    assert!(installer.calls().is_empty());
}

#[test]
fn transient_failure_does_not_stop_later_panels() {
    let store = InMemoryPanelInstallationStore::new();
    let mut installer = ScriptedInstaller::new();
    installer.fetch = Err(InstallerError::new("network"));
    let first = panel("first", "first_channel", "content");
    let second = panel("second", "second_channel", "content");
    block_on(store.upsert(installation(
        &first,
        version(1),
        ChannelId(10),
        MessageId(100),
        REVISION,
    )))
    .unwrap();
    let report = run(
        &[first, second],
        version(1),
        REVISION,
        &bindings(&[("first_channel", 10), ("second_channel", 20)]),
        &store,
        &installer,
    )
    .unwrap();
    assert_eq!(
        report
            .outcomes
            .iter()
            .map(|outcome| outcome.action.clone())
            .collect::<Vec<_>>(),
        vec![PanelAction::SkippedTransient, PanelAction::Posted]
    );
    assert_eq!(
        installer.calls(),
        vec![
            Call::Fetch(ChannelId(10), MessageId(100)),
            Call::Post(ChannelId(20)),
        ]
    );
}
