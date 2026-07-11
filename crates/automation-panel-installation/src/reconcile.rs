use automation_ruleset::{RuleSetKey, RuleSetVersionId};
use automation_state::PanelSpec;
use discord_model::{ChannelId, GuildId, MessageId};
use resource_resolution::ResourceBindingMap;

use crate::hash::spec_hash;
use crate::installer::{PanelEditOutcome, PanelInstaller, PanelPresence};
use crate::model::{PanelInstallation, PanelInstallationKey};
use crate::store::{PanelInstallationStore, PanelInstallationStoreError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelAction {
    Posted,
    Reposted,
    RepostedNewChannel,
    Edited,
    PersistenceUpdated,
    NoOp,
    SkippedTransient,
    SkippedUnresolvedChannel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelOutcome {
    pub panel_key: String,
    pub action: PanelAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallReport {
    pub outcomes: Vec<PanelOutcome>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallError {
    Store(PanelInstallationStoreError),
}

#[allow(clippy::too_many_arguments)]
pub async fn install_declared_panels<S, I>(
    guild_id: GuildId,
    ruleset_key: &RuleSetKey,
    ruleset_version: RuleSetVersionId,
    render_revision: u32,
    panels: &[PanelSpec],
    bindings: &ResourceBindingMap,
    store: &S,
    installer: &I,
) -> Result<InstallReport, InstallError>
where
    S: PanelInstallationStore,
    I: PanelInstaller,
{
    let mut outcomes = Vec::with_capacity(panels.len());
    for panel in panels {
        let Some(channel_id) = bindings.channel_bindings.get(&panel.channel).copied() else {
            outcomes.push(PanelOutcome {
                panel_key: panel.key.clone(),
                action: PanelAction::SkippedUnresolvedChannel,
            });
            continue;
        };
        let key = PanelInstallationKey {
            guild_id,
            ruleset_key: ruleset_key.clone(),
            panel_key: panel.key.clone(),
        };
        let desired_hash = spec_hash(render_revision, panel);
        let record = store.get(&key).await.map_err(InstallError::Store)?;
        let action = match record {
            None => {
                let Some(message_id) =
                    post_message(installer, channel_id, guild_id, ruleset_key, panel).await
                else {
                    outcomes.push(transient(panel));
                    continue;
                };
                store
                    .upsert(installation(
                        guild_id,
                        ruleset_key,
                        panel,
                        ruleset_version,
                        channel_id,
                        message_id,
                        desired_hash,
                    ))
                    .await
                    .map_err(InstallError::Store)?;
                PanelAction::Posted
            }
            Some(record) if record.channel_id != channel_id => {
                let Some(message_id) =
                    post_message(installer, channel_id, guild_id, ruleset_key, panel).await
                else {
                    outcomes.push(transient(panel));
                    continue;
                };
                store
                    .upsert(installation(
                        guild_id,
                        ruleset_key,
                        panel,
                        ruleset_version,
                        channel_id,
                        message_id,
                        desired_hash,
                    ))
                    .await
                    .map_err(InstallError::Store)?;
                PanelAction::RepostedNewChannel
            }
            Some(record) => {
                let presence = match installer
                    .fetch_message(record.channel_id, record.message_id)
                    .await
                {
                    Ok(presence) => presence,
                    Err(_) => {
                        outcomes.push(transient(panel));
                        continue;
                    }
                };
                match presence {
                    PanelPresence::Gone => {
                        let Some(message_id) =
                            post_message(installer, channel_id, guild_id, ruleset_key, panel).await
                        else {
                            outcomes.push(transient(panel));
                            continue;
                        };
                        store
                            .upsert(installation(
                                guild_id,
                                ruleset_key,
                                panel,
                                ruleset_version,
                                channel_id,
                                message_id,
                                desired_hash,
                            ))
                            .await
                            .map_err(InstallError::Store)?;
                        PanelAction::Reposted
                    }
                    PanelPresence::Present if record.spec_hash != desired_hash => {
                        let edit = match installer
                            .edit_message(
                                record.channel_id,
                                record.message_id,
                                guild_id,
                                ruleset_key.as_str(),
                                panel,
                            )
                            .await
                        {
                            Ok(edit) => edit,
                            Err(_) => {
                                outcomes.push(transient(panel));
                                continue;
                            }
                        };
                        match edit {
                            PanelEditOutcome::Updated => {
                                store
                                    .upsert(installation(
                                        guild_id,
                                        ruleset_key,
                                        panel,
                                        ruleset_version,
                                        channel_id,
                                        record.message_id,
                                        desired_hash,
                                    ))
                                    .await
                                    .map_err(InstallError::Store)?;
                                PanelAction::Edited
                            }
                            PanelEditOutcome::Gone => {
                                let Some(message_id) = post_message(
                                    installer,
                                    channel_id,
                                    guild_id,
                                    ruleset_key,
                                    panel,
                                )
                                .await
                                else {
                                    outcomes.push(transient(panel));
                                    continue;
                                };
                                store
                                    .upsert(installation(
                                        guild_id,
                                        ruleset_key,
                                        panel,
                                        ruleset_version,
                                        channel_id,
                                        message_id,
                                        desired_hash,
                                    ))
                                    .await
                                    .map_err(InstallError::Store)?;
                                PanelAction::Reposted
                            }
                        }
                    }
                    PanelPresence::Present if record.installed_version != ruleset_version => {
                        store
                            .upsert(PanelInstallation {
                                installed_version: ruleset_version,
                                ..record
                            })
                            .await
                            .map_err(InstallError::Store)?;
                        PanelAction::PersistenceUpdated
                    }
                    PanelPresence::Present => PanelAction::NoOp,
                }
            }
        };
        outcomes.push(PanelOutcome {
            panel_key: panel.key.clone(),
            action,
        });
    }
    Ok(InstallReport { outcomes })
}

async fn post_message<I: PanelInstaller>(
    installer: &I,
    channel_id: ChannelId,
    guild_id: GuildId,
    ruleset_key: &RuleSetKey,
    panel: &PanelSpec,
) -> Option<MessageId> {
    installer
        .post_message(channel_id, guild_id, ruleset_key.as_str(), panel)
        .await
        .ok()
}

fn installation(
    guild_id: GuildId,
    ruleset_key: &RuleSetKey,
    panel: &PanelSpec,
    installed_version: RuleSetVersionId,
    channel_id: ChannelId,
    message_id: MessageId,
    spec_hash: String,
) -> PanelInstallation {
    PanelInstallation {
        guild_id,
        ruleset_key: ruleset_key.clone(),
        panel_key: panel.key.clone(),
        installed_version,
        channel_id,
        message_id,
        spec_hash,
    }
}

fn transient(panel: &PanelSpec) -> PanelOutcome {
    PanelOutcome {
        panel_key: panel.key.clone(),
        action: PanelAction::SkippedTransient,
    }
}
