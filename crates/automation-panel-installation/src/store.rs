use std::collections::BTreeMap;
use std::sync::Mutex;

use automation_ruleset::RuleSetKey;
use discord_model::GuildId;

use crate::model::{PanelInstallation, PanelInstallationKey};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelInstallationStoreError {
    Backend(String),
}

#[allow(async_fn_in_trait)]
pub trait PanelInstallationStore {
    async fn get(
        &self,
        key: &PanelInstallationKey,
    ) -> Result<Option<PanelInstallation>, PanelInstallationStoreError>;

    async fn upsert(
        &self,
        installation: PanelInstallation,
    ) -> Result<(), PanelInstallationStoreError>;
}

type LogicalKey = (GuildId, RuleSetKey, String);

#[derive(Default)]
pub struct InMemoryPanelInstallationStore {
    inner: Mutex<BTreeMap<LogicalKey, PanelInstallation>>,
}

impl InMemoryPanelInstallationStore {
    pub fn new() -> Self {
        Self::default()
    }
}

fn logical_key(key: &PanelInstallationKey) -> LogicalKey {
    (key.guild_id, key.ruleset_key.clone(), key.panel_key.clone())
}

fn installation_key(installation: &PanelInstallation) -> LogicalKey {
    (
        installation.guild_id,
        installation.ruleset_key.clone(),
        installation.panel_key.clone(),
    )
}

impl PanelInstallationStore for InMemoryPanelInstallationStore {
    async fn get(
        &self,
        key: &PanelInstallationKey,
    ) -> Result<Option<PanelInstallation>, PanelInstallationStoreError> {
        Ok(self.inner.lock().unwrap().get(&logical_key(key)).cloned())
    }

    async fn upsert(
        &self,
        installation: PanelInstallation,
    ) -> Result<(), PanelInstallationStoreError> {
        self.inner
            .lock()
            .unwrap()
            .insert(installation_key(&installation), installation);
        Ok(())
    }
}
