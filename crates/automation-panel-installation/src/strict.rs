use std::collections::{BTreeMap, BTreeSet};

use automation_ruleset::{RuleSetKey, RuleSetVersionId};
use automation_state::PanelSpec;
use discord_model::{ChannelId, GuildId, MessageId};
use resource_resolution::ResourceBindingMap;
use serde::{Deserialize, Serialize};

use crate::{
    spec_hash, InstallerError, PanelInstallation, PanelInstallationKey, PanelInstallationStore,
    PanelInstallationStoreError,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrictPanelButtonPayloadV1 {
    pub label: String,
    pub custom_id: String,
    pub style: String,
    pub disabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrictPanelActionRowPayloadV1 {
    pub buttons: Vec<StrictPanelButtonPayloadV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrictPanelMessagePayloadV1 {
    pub content: String,
    pub action_rows: Vec<StrictPanelActionRowPayloadV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrictDeclaredPanelV1 {
    pub spec: PanelSpec,
    pub expected_payload: StrictPanelMessagePayloadV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StrictObservedMessageV1 {
    Missing,
    Present(StrictPanelMessagePayloadV1),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrictExternalPostResultV1 {
    Applied(MessageId),
    DefinitelyNotApplied,
    Ambiguous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrictDeleteOutcomeV1 {
    Deleted,
    AlreadyGone,
    DefinitelyNotApplied,
    Ambiguous,
}

#[allow(async_fn_in_trait)]
pub trait StrictPanelInstaller {
    async fn observe_message(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
    ) -> Result<StrictObservedMessageV1, InstallerError>;

    async fn post_message(
        &self,
        channel_id: ChannelId,
        guild_id: GuildId,
        ruleset_key: &str,
        panel: &StrictDeclaredPanelV1,
    ) -> StrictExternalPostResultV1;

    async fn delete_message(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
    ) -> StrictDeleteOutcomeV1;
}

#[allow(async_fn_in_trait)]
pub trait StrictPanelInstallationStore: PanelInstallationStore {
    async fn list_slot(
        &self,
        guild_id: GuildId,
        ruleset_key: &RuleSetKey,
    ) -> Result<Vec<PanelInstallation>, PanelInstallationStoreError>;

    async fn remove(&self, key: &PanelInstallationKey) -> Result<(), PanelInstallationStoreError>;
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrictPanelOperationKeyV1 {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub panel_key: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrictPanelInstallKindV1 {
    Fresh,
    MissingMessage,
    ChannelMoved,
    PayloadReplaced,
    MetadataUpdated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrictPanelCleanupKindV1 {
    Removed,
    ChannelMoved,
    PayloadReplaced,
    Orphan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrictPanelMessageRefV1 {
    pub channel_id: ChannelId,
    pub message_id: MessageId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrictPanelPreviousMessageV1 {
    pub message: StrictPanelMessageRefV1,
    pub cleanup_kind: StrictPanelCleanupKindV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrictPanelPostIntentV1 {
    pub panel: StrictDeclaredPanelV1,
    pub ruleset_version: RuleSetVersionId,
    pub channel_id: ChannelId,
    pub spec_hash: String,
    pub install_kind: StrictPanelInstallKindV1,
    pub previous_message: Option<StrictPanelPreviousMessageV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrictPanelCleanupIntentV1 {
    pub message: StrictPanelMessageRefV1,
    pub kind: StrictPanelCleanupKindV1,
    pub remove_installation: bool,
}

#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum StrictPanelOperationStateV1 {
    PostDispatching {
        intent: StrictPanelPostIntentV1,
    },
    PostApplied {
        intent: StrictPanelPostIntentV1,
        message_id: MessageId,
    },
    AmbiguousPost {
        intent: StrictPanelPostIntentV1,
    },
    CleanupPending {
        intent: StrictPanelCleanupIntentV1,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrictPanelOperationV1 {
    pub key: StrictPanelOperationKeyV1,
    pub state: StrictPanelOperationStateV1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrictPanelJournalError(pub String);

#[allow(async_fn_in_trait)]
pub trait StrictPanelOperationJournal {
    async fn list_slot(
        &self,
        guild_id: GuildId,
        ruleset_key: &RuleSetKey,
    ) -> Result<Vec<StrictPanelOperationV1>, StrictPanelJournalError>;

    async fn put(&self, operation: StrictPanelOperationV1) -> Result<(), StrictPanelJournalError>;

    async fn remove(&self, key: &StrictPanelOperationKeyV1) -> Result<(), StrictPanelJournalError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StrictPanelActionV1 {
    Installed(StrictPanelInstallKindV1),
    Unchanged,
    CleanupCompleted(StrictPanelCleanupKindV1),
    CleanupPending(StrictPanelCleanupKindV1),
    PostDefinitelyNotApplied,
    AmbiguousPost,
    PostedMessageMissing,
    PostedPayloadMismatch,
    SkippedTransient,
    SkippedUnresolvedChannel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrictPanelOutcomeV1 {
    pub panel_key: String,
    pub action: StrictPanelActionV1,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StrictPanelReportV1 {
    pub outcomes: Vec<StrictPanelOutcomeV1>,
    pub declared_count: u32,
    pub installed_count: u32,
    pub unchanged_count: u32,
    pub skipped_transient_count: u32,
    pub skipped_unresolved_channel_count: u32,
    pub failed_count: u32,
    pub ambiguous_outcome_count: u32,
    pub stale_message_cleanup_pending_count: u32,
    pub orphan_message_cleanup_pending_count: u32,
    pub reposted_old_message_cleanup_pending_count: u32,
}

impl StrictPanelReportV1 {
    pub fn is_eligible(&self) -> bool {
        self.installed_count.checked_add(self.unchanged_count) == Some(self.declared_count)
            && self.skipped_transient_count == 0
            && self.skipped_unresolved_channel_count == 0
            && self.failed_count == 0
            && self.ambiguous_outcome_count == 0
            && self.stale_message_cleanup_pending_count == 0
            && self.orphan_message_cleanup_pending_count == 0
            && self.reposted_old_message_cleanup_pending_count == 0
    }

    fn increment(value: &mut u32) -> Result<(), StrictPanelReconcileErrorV1> {
        *value = (*value)
            .checked_add(1)
            .ok_or(StrictPanelReconcileErrorV1::CountOverflow)?;
        Ok(())
    }

    fn push(&mut self, panel_key: &str, action: StrictPanelActionV1) {
        self.outcomes.push(StrictPanelOutcomeV1 {
            panel_key: panel_key.to_string(),
            action,
        });
    }

    fn installed(
        &mut self,
        panel_key: &str,
        kind: StrictPanelInstallKindV1,
    ) -> Result<(), StrictPanelReconcileErrorV1> {
        Self::increment(&mut self.installed_count)?;
        self.push(panel_key, StrictPanelActionV1::Installed(kind));
        Ok(())
    }

    fn unchanged(&mut self, panel_key: &str) -> Result<(), StrictPanelReconcileErrorV1> {
        Self::increment(&mut self.unchanged_count)?;
        self.push(panel_key, StrictPanelActionV1::Unchanged);
        Ok(())
    }

    fn unresolved(&mut self, panel_key: &str) -> Result<(), StrictPanelReconcileErrorV1> {
        Self::increment(&mut self.skipped_unresolved_channel_count)?;
        self.push(panel_key, StrictPanelActionV1::SkippedUnresolvedChannel);
        Ok(())
    }

    fn transient(&mut self, panel_key: &str) -> Result<(), StrictPanelReconcileErrorV1> {
        Self::increment(&mut self.skipped_transient_count)?;
        self.push(panel_key, StrictPanelActionV1::SkippedTransient);
        Ok(())
    }

    fn failed(
        &mut self,
        panel_key: &str,
        action: StrictPanelActionV1,
    ) -> Result<(), StrictPanelReconcileErrorV1> {
        Self::increment(&mut self.failed_count)?;
        self.push(panel_key, action);
        Ok(())
    }

    fn ambiguous_post(&mut self, panel_key: &str) -> Result<(), StrictPanelReconcileErrorV1> {
        Self::increment(&mut self.ambiguous_outcome_count)?;
        Self::increment(&mut self.orphan_message_cleanup_pending_count)?;
        self.push(panel_key, StrictPanelActionV1::AmbiguousPost);
        Ok(())
    }

    fn cleanup_pending(
        &mut self,
        panel_key: &str,
        kind: StrictPanelCleanupKindV1,
        ambiguous: bool,
    ) -> Result<(), StrictPanelReconcileErrorV1> {
        match kind {
            StrictPanelCleanupKindV1::Removed => {
                Self::increment(&mut self.stale_message_cleanup_pending_count)?
            }
            StrictPanelCleanupKindV1::ChannelMoved | StrictPanelCleanupKindV1::PayloadReplaced => {
                Self::increment(&mut self.reposted_old_message_cleanup_pending_count)?
            }
            StrictPanelCleanupKindV1::Orphan => {
                Self::increment(&mut self.orphan_message_cleanup_pending_count)?
            }
        }
        if ambiguous {
            Self::increment(&mut self.ambiguous_outcome_count)?;
        }
        self.push(panel_key, StrictPanelActionV1::CleanupPending(kind));
        Ok(())
    }

    fn cleanup_completed(&mut self, panel_key: &str, kind: StrictPanelCleanupKindV1) {
        self.push(panel_key, StrictPanelActionV1::CleanupCompleted(kind));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StrictPanelReconcileErrorV1 {
    Store(PanelInstallationStoreError),
    Journal(StrictPanelJournalError),
    DuplicateDeclaredPanel(String),
    DuplicateStoredPanel(String),
    DuplicateJournalOperation(String),
    StoredPanelOutsideSlot,
    JournalOperationOutsideSlot,
    InvalidJournalState(String),
    CountOverflow,
}

pub struct StrictPanelReconcileRequestV1<'a> {
    pub guild_id: GuildId,
    pub ruleset_key: &'a RuleSetKey,
    pub ruleset_version: RuleSetVersionId,
    pub render_revision: u32,
    pub panels: &'a [StrictDeclaredPanelV1],
    pub bindings: &'a ResourceBindingMap,
}

pub async fn reconcile_declared_panels_strict<S, I, J>(
    request: StrictPanelReconcileRequestV1<'_>,
    store: &S,
    installer: &I,
    journal: &J,
) -> Result<StrictPanelReportV1, StrictPanelReconcileErrorV1>
where
    S: StrictPanelInstallationStore,
    I: StrictPanelInstaller,
    J: StrictPanelOperationJournal,
{
    StrictPanelReconciler::load(request, store, installer, journal)
        .await?
        .run()
        .await
}

struct StrictPanelReconciler<'a, S, I, J> {
    request: StrictPanelReconcileRequestV1<'a>,
    store: &'a S,
    installer: &'a I,
    journal: &'a J,
    report: StrictPanelReportV1,
    installations: BTreeMap<String, PanelInstallation>,
    operations: BTreeMap<String, StrictPanelOperationV1>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OperationDisposition {
    Continue,
    Counted,
    Blocked,
}

impl<'a, S, I, J> StrictPanelReconciler<'a, S, I, J>
where
    S: StrictPanelInstallationStore,
    I: StrictPanelInstaller,
    J: StrictPanelOperationJournal,
{
    async fn load(
        request: StrictPanelReconcileRequestV1<'a>,
        store: &'a S,
        installer: &'a I,
        journal: &'a J,
    ) -> Result<Self, StrictPanelReconcileErrorV1> {
        let declared_count = u32::try_from(request.panels.len())
            .map_err(|_| StrictPanelReconcileErrorV1::CountOverflow)?;
        let mut declared = BTreeSet::new();
        for panel in request.panels {
            if !declared.insert(panel.spec.key.clone()) {
                return Err(StrictPanelReconcileErrorV1::DuplicateDeclaredPanel(
                    panel.spec.key.clone(),
                ));
            }
        }
        let mut installations = BTreeMap::new();
        for installation in store
            .list_slot(request.guild_id, request.ruleset_key)
            .await
            .map_err(StrictPanelReconcileErrorV1::Store)?
        {
            if installation.guild_id != request.guild_id
                || &installation.ruleset_key != request.ruleset_key
            {
                return Err(StrictPanelReconcileErrorV1::StoredPanelOutsideSlot);
            }
            let panel_key = installation.panel_key.clone();
            if installations
                .insert(panel_key.clone(), installation)
                .is_some()
            {
                return Err(StrictPanelReconcileErrorV1::DuplicateStoredPanel(panel_key));
            }
        }
        let mut operations = BTreeMap::new();
        for operation in journal
            .list_slot(request.guild_id, request.ruleset_key)
            .await
            .map_err(StrictPanelReconcileErrorV1::Journal)?
        {
            if operation.key.guild_id != request.guild_id
                || &operation.key.ruleset_key != request.ruleset_key
            {
                return Err(StrictPanelReconcileErrorV1::JournalOperationOutsideSlot);
            }
            let panel_key = operation.key.panel_key.clone();
            if operations.insert(panel_key.clone(), operation).is_some() {
                return Err(StrictPanelReconcileErrorV1::DuplicateJournalOperation(
                    panel_key,
                ));
            }
        }
        Ok(Self {
            request,
            store,
            installer,
            journal,
            report: StrictPanelReportV1 {
                declared_count,
                ..StrictPanelReportV1::default()
            },
            installations,
            operations,
        })
    }

    async fn run(mut self) -> Result<StrictPanelReportV1, StrictPanelReconcileErrorV1> {
        let declared_keys = self
            .request
            .panels
            .iter()
            .map(|panel| panel.spec.key.clone())
            .collect::<BTreeSet<_>>();
        let removed_keys = self
            .installations
            .keys()
            .chain(self.operations.keys())
            .filter(|key| !declared_keys.contains(*key))
            .cloned()
            .collect::<BTreeSet<_>>();
        for panel_key in removed_keys {
            self.reconcile_removed(&panel_key).await?;
        }
        let panels = self.request.panels.to_vec();
        for panel in panels {
            self.reconcile_declared(panel).await?;
        }
        Ok(self.report)
    }

    async fn reconcile_removed(
        &mut self,
        panel_key: &str,
    ) -> Result<(), StrictPanelReconcileErrorV1> {
        if let Some(operation) = self.operations.remove(panel_key) {
            match operation.state.clone() {
                StrictPanelOperationStateV1::PostDispatching { intent } => {
                    self.record_ambiguous_post(operation.key, intent).await?;
                    if self.installations.contains_key(panel_key) {
                        self.report.cleanup_pending(
                            panel_key,
                            StrictPanelCleanupKindV1::Removed,
                            false,
                        )?;
                    }
                    return Ok(());
                }
                StrictPanelOperationStateV1::AmbiguousPost { .. } => {
                    self.report.ambiguous_post(panel_key)?;
                    if self.installations.contains_key(panel_key) {
                        self.report.cleanup_pending(
                            panel_key,
                            StrictPanelCleanupKindV1::Removed,
                            false,
                        )?;
                    }
                    return Ok(());
                }
                StrictPanelOperationStateV1::PostApplied { intent, message_id } => {
                    let cleanup = StrictPanelCleanupIntentV1 {
                        message: StrictPanelMessageRefV1 {
                            channel_id: intent.channel_id,
                            message_id,
                        },
                        kind: StrictPanelCleanupKindV1::Orphan,
                        remove_installation: false,
                    };
                    let cleanup_operation = StrictPanelOperationV1 {
                        key: operation.key,
                        state: StrictPanelOperationStateV1::CleanupPending { intent: cleanup },
                    };
                    self.put_operation(cleanup_operation.clone()).await?;
                    if !self.drive_cleanup(cleanup_operation).await? {
                        if self.installations.contains_key(panel_key) {
                            self.report.cleanup_pending(
                                panel_key,
                                StrictPanelCleanupKindV1::Removed,
                                false,
                            )?;
                        }
                        return Ok(());
                    }
                }
                StrictPanelOperationStateV1::CleanupPending { .. } => {
                    let removes_installation = cleanup_intent(&operation)?.remove_installation;
                    if !self.drive_cleanup(operation).await? {
                        if !removes_installation && self.installations.contains_key(panel_key) {
                            self.report.cleanup_pending(
                                panel_key,
                                StrictPanelCleanupKindV1::Removed,
                                false,
                            )?;
                        }
                        return Ok(());
                    }
                }
            }
        }
        let Some(installation) = self.installations.get(panel_key).cloned() else {
            return Ok(());
        };
        let operation = StrictPanelOperationV1 {
            key: self.operation_key(panel_key),
            state: StrictPanelOperationStateV1::CleanupPending {
                intent: StrictPanelCleanupIntentV1 {
                    message: message_ref(&installation),
                    kind: StrictPanelCleanupKindV1::Removed,
                    remove_installation: true,
                },
            },
        };
        self.put_operation(operation.clone()).await?;
        self.drive_cleanup(operation).await?;
        Ok(())
    }

    async fn reconcile_declared(
        &mut self,
        panel: StrictDeclaredPanelV1,
    ) -> Result<(), StrictPanelReconcileErrorV1> {
        let panel_key = panel.spec.key.clone();
        let Some(channel_id) = self
            .request
            .bindings
            .channel_bindings
            .get(&panel.spec.channel)
            .copied()
        else {
            self.report.unresolved(&panel_key)?;
            return Ok(());
        };
        let desired_hash = spec_hash(self.request.render_revision, &panel.spec);
        if let Some(operation) = self.operations.remove(&panel_key) {
            match self
                .resume_operation(operation, &panel, channel_id, &desired_hash)
                .await?
            {
                OperationDisposition::Continue => {}
                OperationDisposition::Counted | OperationDisposition::Blocked => return Ok(()),
            }
        }
        let Some(installation) = self.installations.get(&panel_key).cloned() else {
            return self
                .start_post(
                    panel,
                    channel_id,
                    desired_hash,
                    StrictPanelInstallKindV1::Fresh,
                    None,
                )
                .await;
        };
        if installation.channel_id != channel_id {
            return self
                .start_post(
                    panel,
                    channel_id,
                    desired_hash,
                    StrictPanelInstallKindV1::ChannelMoved,
                    Some(StrictPanelPreviousMessageV1 {
                        message: message_ref(&installation),
                        cleanup_kind: StrictPanelCleanupKindV1::ChannelMoved,
                    }),
                )
                .await;
        }
        match self
            .installer
            .observe_message(installation.channel_id, installation.message_id)
            .await
        {
            Err(_) => self.report.transient(&panel_key),
            Ok(StrictObservedMessageV1::Missing) => {
                self.start_post(
                    panel,
                    channel_id,
                    desired_hash,
                    StrictPanelInstallKindV1::MissingMessage,
                    None,
                )
                .await
            }
            Ok(StrictObservedMessageV1::Present(observed))
                if observed != panel.expected_payload =>
            {
                self.start_post(
                    panel,
                    channel_id,
                    desired_hash,
                    StrictPanelInstallKindV1::PayloadReplaced,
                    Some(StrictPanelPreviousMessageV1 {
                        message: message_ref(&installation),
                        cleanup_kind: StrictPanelCleanupKindV1::PayloadReplaced,
                    }),
                )
                .await
            }
            Ok(StrictObservedMessageV1::Present(_))
                if installation.installed_version != self.request.ruleset_version
                    || installation.spec_hash != desired_hash =>
            {
                let updated = PanelInstallation {
                    installed_version: self.request.ruleset_version,
                    spec_hash: desired_hash,
                    ..installation
                };
                self.store
                    .upsert(updated.clone())
                    .await
                    .map_err(StrictPanelReconcileErrorV1::Store)?;
                self.installations.insert(panel_key.clone(), updated);
                self.report
                    .installed(&panel_key, StrictPanelInstallKindV1::MetadataUpdated)
            }
            Ok(StrictObservedMessageV1::Present(_)) => self.report.unchanged(&panel_key),
        }
    }

    async fn resume_operation(
        &mut self,
        operation: StrictPanelOperationV1,
        panel: &StrictDeclaredPanelV1,
        channel_id: ChannelId,
        desired_hash: &str,
    ) -> Result<OperationDisposition, StrictPanelReconcileErrorV1> {
        match operation.state.clone() {
            StrictPanelOperationStateV1::PostDispatching { intent } => {
                self.record_ambiguous_post(operation.key, intent).await?;
                Ok(OperationDisposition::Blocked)
            }
            StrictPanelOperationStateV1::AmbiguousPost { .. } => {
                self.report.ambiguous_post(&operation.key.panel_key)?;
                Ok(OperationDisposition::Blocked)
            }
            StrictPanelOperationStateV1::PostApplied { intent, message_id } => {
                if post_matches(
                    &intent,
                    panel,
                    self.request.ruleset_version,
                    channel_id,
                    desired_hash,
                ) {
                    self.complete_applied_post(operation.key, intent, message_id)
                        .await?;
                    Ok(OperationDisposition::Counted)
                } else {
                    let cleanup = StrictPanelOperationV1 {
                        key: operation.key,
                        state: StrictPanelOperationStateV1::CleanupPending {
                            intent: StrictPanelCleanupIntentV1 {
                                message: StrictPanelMessageRefV1 {
                                    channel_id: intent.channel_id,
                                    message_id,
                                },
                                kind: StrictPanelCleanupKindV1::Orphan,
                                remove_installation: false,
                            },
                        },
                    };
                    self.put_operation(cleanup.clone()).await?;
                    if self.drive_cleanup(cleanup).await? {
                        Ok(OperationDisposition::Continue)
                    } else {
                        Ok(OperationDisposition::Blocked)
                    }
                }
            }
            StrictPanelOperationStateV1::CleanupPending { .. } => {
                if self.drive_cleanup(operation).await? {
                    Ok(OperationDisposition::Continue)
                } else {
                    Ok(OperationDisposition::Blocked)
                }
            }
        }
    }

    async fn start_post(
        &mut self,
        panel: StrictDeclaredPanelV1,
        channel_id: ChannelId,
        desired_hash: String,
        install_kind: StrictPanelInstallKindV1,
        previous_message: Option<StrictPanelPreviousMessageV1>,
    ) -> Result<(), StrictPanelReconcileErrorV1> {
        let panel_key = panel.spec.key.clone();
        let key = self.operation_key(&panel_key);
        let intent = StrictPanelPostIntentV1 {
            panel,
            ruleset_version: self.request.ruleset_version,
            channel_id,
            spec_hash: desired_hash,
            install_kind,
            previous_message,
        };
        self.put_operation(StrictPanelOperationV1 {
            key: key.clone(),
            state: StrictPanelOperationStateV1::PostDispatching {
                intent: intent.clone(),
            },
        })
        .await?;
        match self
            .installer
            .post_message(
                channel_id,
                self.request.guild_id,
                self.request.ruleset_key.as_str(),
                &intent.panel,
            )
            .await
        {
            StrictExternalPostResultV1::Applied(message_id) => {
                self.put_operation(StrictPanelOperationV1 {
                    key: key.clone(),
                    state: StrictPanelOperationStateV1::PostApplied {
                        intent: intent.clone(),
                        message_id,
                    },
                })
                .await?;
                self.complete_applied_post(key, intent, message_id).await
            }
            StrictExternalPostResultV1::DefinitelyNotApplied => {
                self.remove_operation(&key).await?;
                self.report
                    .failed(&panel_key, StrictPanelActionV1::PostDefinitelyNotApplied)
            }
            StrictExternalPostResultV1::Ambiguous => self.record_ambiguous_post(key, intent).await,
        }
    }

    async fn complete_applied_post(
        &mut self,
        key: StrictPanelOperationKeyV1,
        intent: StrictPanelPostIntentV1,
        message_id: MessageId,
    ) -> Result<(), StrictPanelReconcileErrorV1> {
        let panel_key = key.panel_key.clone();
        match self
            .installer
            .observe_message(intent.channel_id, message_id)
            .await
        {
            Err(_) => {
                self.report.transient(&panel_key)?;
                self.report
                    .cleanup_pending(&panel_key, StrictPanelCleanupKindV1::Orphan, false)
            }
            Ok(StrictObservedMessageV1::Missing) => {
                self.remove_operation(&key).await?;
                self.report
                    .failed(&panel_key, StrictPanelActionV1::PostedMessageMissing)
            }
            Ok(StrictObservedMessageV1::Present(observed))
                if observed != intent.panel.expected_payload =>
            {
                let cleanup = StrictPanelOperationV1 {
                    key,
                    state: StrictPanelOperationStateV1::CleanupPending {
                        intent: StrictPanelCleanupIntentV1 {
                            message: StrictPanelMessageRefV1 {
                                channel_id: intent.channel_id,
                                message_id,
                            },
                            kind: StrictPanelCleanupKindV1::Orphan,
                            remove_installation: false,
                        },
                    },
                };
                self.put_operation(cleanup.clone()).await?;
                self.report
                    .failed(&panel_key, StrictPanelActionV1::PostedPayloadMismatch)?;
                self.drive_cleanup(cleanup).await?;
                Ok(())
            }
            Ok(StrictObservedMessageV1::Present(_)) => {
                if intent.previous_message.as_ref().is_some_and(|previous| {
                    previous.message
                        == (StrictPanelMessageRefV1 {
                            channel_id: intent.channel_id,
                            message_id,
                        })
                }) {
                    return Err(StrictPanelReconcileErrorV1::InvalidJournalState(panel_key));
                }
                let installation = PanelInstallation {
                    guild_id: key.guild_id,
                    ruleset_key: key.ruleset_key.clone(),
                    panel_key: panel_key.clone(),
                    installed_version: intent.ruleset_version,
                    channel_id: intent.channel_id,
                    message_id,
                    spec_hash: intent.spec_hash.clone(),
                };
                self.store
                    .upsert(installation.clone())
                    .await
                    .map_err(StrictPanelReconcileErrorV1::Store)?;
                self.installations.insert(panel_key.clone(), installation);
                self.report.installed(&panel_key, intent.install_kind)?;
                if let Some(previous) = intent.previous_message {
                    let cleanup = StrictPanelOperationV1 {
                        key,
                        state: StrictPanelOperationStateV1::CleanupPending {
                            intent: StrictPanelCleanupIntentV1 {
                                message: previous.message,
                                kind: previous.cleanup_kind,
                                remove_installation: false,
                            },
                        },
                    };
                    self.put_operation(cleanup.clone()).await?;
                    self.drive_cleanup(cleanup).await?;
                    Ok(())
                } else {
                    self.remove_operation(&key).await
                }
            }
        }
    }

    async fn record_ambiguous_post(
        &mut self,
        key: StrictPanelOperationKeyV1,
        intent: StrictPanelPostIntentV1,
    ) -> Result<(), StrictPanelReconcileErrorV1> {
        let panel_key = key.panel_key.clone();
        self.put_operation(StrictPanelOperationV1 {
            key,
            state: StrictPanelOperationStateV1::AmbiguousPost { intent },
        })
        .await?;
        self.report.ambiguous_post(&panel_key)
    }

    async fn drive_cleanup(
        &mut self,
        operation: StrictPanelOperationV1,
    ) -> Result<bool, StrictPanelReconcileErrorV1> {
        let key = operation.key.clone();
        let intent = cleanup_intent(&operation)?.clone();
        match self
            .installer
            .delete_message(intent.message.channel_id, intent.message.message_id)
            .await
        {
            StrictDeleteOutcomeV1::Deleted | StrictDeleteOutcomeV1::AlreadyGone => {
                if intent.remove_installation {
                    if self
                        .installations
                        .get(&key.panel_key)
                        .is_some_and(|installation| message_ref(installation) != intent.message)
                    {
                        return Err(StrictPanelReconcileErrorV1::InvalidJournalState(
                            key.panel_key,
                        ));
                    }
                    self.store
                        .remove(&installation_key(&key))
                        .await
                        .map_err(StrictPanelReconcileErrorV1::Store)?;
                    self.installations.remove(&key.panel_key);
                }
                self.remove_operation(&key).await?;
                self.report.cleanup_completed(&key.panel_key, intent.kind);
                Ok(true)
            }
            StrictDeleteOutcomeV1::DefinitelyNotApplied => {
                self.report
                    .cleanup_pending(&key.panel_key, intent.kind, false)?;
                Ok(false)
            }
            StrictDeleteOutcomeV1::Ambiguous => {
                self.report
                    .cleanup_pending(&key.panel_key, intent.kind, true)?;
                Ok(false)
            }
        }
    }

    async fn put_operation(
        &self,
        operation: StrictPanelOperationV1,
    ) -> Result<(), StrictPanelReconcileErrorV1> {
        self.journal
            .put(operation)
            .await
            .map_err(StrictPanelReconcileErrorV1::Journal)
    }

    async fn remove_operation(
        &self,
        key: &StrictPanelOperationKeyV1,
    ) -> Result<(), StrictPanelReconcileErrorV1> {
        self.journal
            .remove(key)
            .await
            .map_err(StrictPanelReconcileErrorV1::Journal)
    }

    fn operation_key(&self, panel_key: &str) -> StrictPanelOperationKeyV1 {
        StrictPanelOperationKeyV1 {
            guild_id: self.request.guild_id,
            ruleset_key: self.request.ruleset_key.clone(),
            panel_key: panel_key.to_string(),
        }
    }
}

fn message_ref(installation: &PanelInstallation) -> StrictPanelMessageRefV1 {
    StrictPanelMessageRefV1 {
        channel_id: installation.channel_id,
        message_id: installation.message_id,
    }
}

fn installation_key(key: &StrictPanelOperationKeyV1) -> PanelInstallationKey {
    PanelInstallationKey {
        guild_id: key.guild_id,
        ruleset_key: key.ruleset_key.clone(),
        panel_key: key.panel_key.clone(),
    }
}

fn cleanup_intent(
    operation: &StrictPanelOperationV1,
) -> Result<&StrictPanelCleanupIntentV1, StrictPanelReconcileErrorV1> {
    match &operation.state {
        StrictPanelOperationStateV1::CleanupPending { intent } => Ok(intent),
        _ => Err(StrictPanelReconcileErrorV1::InvalidJournalState(
            operation.key.panel_key.clone(),
        )),
    }
}

fn post_matches(
    intent: &StrictPanelPostIntentV1,
    panel: &StrictDeclaredPanelV1,
    ruleset_version: RuleSetVersionId,
    channel_id: ChannelId,
    desired_hash: &str,
) -> bool {
    &intent.panel == panel
        && intent.ruleset_version == ruleset_version
        && intent.channel_id == channel_id
        && intent.spec_hash == desired_hash
}
