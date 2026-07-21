use automation_panel_installation::strict::{
    validate_strict_panel_operation_v1, StrictPanelOperationStateV1, StrictPanelOperationV1,
};
use automation_panel_installation::PanelInstallation;
use automation_ruleset::RuleSetVersionId;
use discord_model::{ChannelId, GuildId, MessageId};
use sqlx::types::Json;

use crate::RuntimePanelPersistenceErrorV1;

pub(crate) const RECORD_FORMAT_VERSION: i16 = 1;

#[derive(sqlx::FromRow)]
pub(crate) struct RuntimePanelSnapshotRowV1 {
    pub(crate) record_kind: String,
    pub(crate) record_revision: i64,
    pub(crate) record_format_version: Option<i16>,
    pub(crate) guild_id: String,
    pub(crate) ruleset_key: String,
    pub(crate) panel_key: String,
    pub(crate) installed_version: Option<i64>,
    pub(crate) channel_id: Option<String>,
    pub(crate) message_id: Option<String>,
    pub(crate) spec_hash: Option<String>,
    pub(crate) state_tag: Option<String>,
    pub(crate) operation_payload: Option<Json<StrictPanelOperationV1>>,
}

pub(crate) enum DecodedSnapshotRecordV1 {
    Installation {
        revision: u64,
        value: PanelInstallation,
    },
    Journal {
        revision: u64,
        value: StrictPanelOperationV1,
    },
}

impl RuntimePanelSnapshotRowV1 {
    pub(crate) fn decode(
        self,
        expected_guild_id: GuildId,
        expected_ruleset_key: &automation_ruleset::RuleSetKey,
    ) -> Result<DecodedSnapshotRecordV1, RuntimePanelPersistenceErrorV1> {
        let invalid = || RuntimePanelPersistenceErrorV1::PersistenceCorrupt;
        let revision = u64::try_from(self.record_revision)
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(invalid)?;
        let guild_id = self.guild_id.parse::<GuildId>().map_err(|_| invalid())?;
        let ruleset_key =
            automation_ruleset::RuleSetKey::parse(&self.ruleset_key).map_err(|_| invalid())?;
        if guild_id != expected_guild_id || &ruleset_key != expected_ruleset_key {
            return Err(invalid());
        }
        automation_panel_installation::strict::validate_strict_panel_key_v1(&self.panel_key)
            .map_err(|_| invalid())?;
        match self.record_kind.as_str() {
            "installation" => {
                if self.record_format_version.is_some()
                    || self.state_tag.is_some()
                    || self.operation_payload.is_some()
                {
                    return Err(invalid());
                }
                let installed_version = self
                    .installed_version
                    .and_then(|value| u32::try_from(value).ok())
                    .and_then(|value| RuleSetVersionId::new(value).ok())
                    .ok_or_else(invalid)?;
                let channel_id = self
                    .channel_id
                    .ok_or_else(invalid)?
                    .parse::<ChannelId>()
                    .map_err(|_| invalid())?;
                let message_id = self
                    .message_id
                    .ok_or_else(invalid)?
                    .parse::<MessageId>()
                    .map_err(|_| invalid())?;
                let spec_hash = self.spec_hash.ok_or_else(invalid)?;
                if !valid_hash(&spec_hash) {
                    return Err(invalid());
                }
                Ok(DecodedSnapshotRecordV1::Installation {
                    revision,
                    value: PanelInstallation {
                        guild_id,
                        ruleset_key,
                        panel_key: self.panel_key,
                        installed_version,
                        channel_id,
                        message_id,
                        spec_hash,
                    },
                })
            }
            "journal" => {
                if self.record_format_version != Some(RECORD_FORMAT_VERSION)
                    || self.installed_version.is_some()
                    || self.channel_id.is_some()
                    || self.message_id.is_some()
                    || self.spec_hash.is_some()
                {
                    return Err(invalid());
                }
                let operation = self.operation_payload.ok_or_else(invalid)?.0;
                validate_strict_panel_operation_v1(&operation).map_err(|_| invalid())?;
                if operation.key.guild_id != guild_id
                    || operation.key.ruleset_key != ruleset_key
                    || operation.key.panel_key != self.panel_key
                    || self.state_tag.as_deref() != Some(state_tag(&operation.state))
                {
                    return Err(invalid());
                }
                Ok(DecodedSnapshotRecordV1::Journal {
                    revision,
                    value: operation,
                })
            }
            _ => Err(invalid()),
        }
    }
}

pub(crate) fn state_tag(state: &StrictPanelOperationStateV1) -> &'static str {
    match state {
        StrictPanelOperationStateV1::PostDispatching { .. } => "post_dispatching",
        StrictPanelOperationStateV1::PostApplied { .. } => "post_applied",
        StrictPanelOperationStateV1::AmbiguousPost { .. } => "ambiguous_post",
        StrictPanelOperationStateV1::CleanupPending { .. } => "cleanup_pending",
    }
}

pub(crate) fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
