use automation_panel_installation::strict::{
    validate_strict_panel_key_v1, validate_strict_panel_operation_v1, StrictPanelInstallationStore,
    StrictPanelJournalError, StrictPanelOperationJournal, StrictPanelOperationKeyV1,
    StrictPanelOperationStateV1, StrictPanelOperationV1, MAX_STRICT_PANEL_RECORDS_PER_SLOT,
};
use automation_panel_installation::{
    PanelInstallation, PanelInstallationKey, PanelInstallationStoreError,
};
use automation_ruleset::RuleSetKey;
use discord_model::GuildId;
use sqlx::types::Json;

use crate::store::{backend, begin_slot_write, bounded_message, PanelInstallationRow};
use crate::PostgresPanelInstallationStore;

const RECORD_FORMAT_VERSION: i16 = 1;

#[derive(sqlx::FromRow)]
struct StrictPanelOperationRow {
    record_format_version: i16,
    guild_id: String,
    ruleset_key: String,
    panel_key: String,
    state_tag: String,
    operation_payload: Json<StrictPanelOperationV1>,
}

fn journal_backend(error: impl std::fmt::Display) -> StrictPanelJournalError {
    StrictPanelJournalError(bounded_message(error))
}

fn validate_panel_key(panel_key: &str) -> Result<(), &'static str> {
    validate_strict_panel_key_v1(panel_key).map_err(|_| "invalid strict panel key")
}

fn state_tag(state: &StrictPanelOperationStateV1) -> &'static str {
    match state {
        StrictPanelOperationStateV1::PostDispatching { .. } => "post_dispatching",
        StrictPanelOperationStateV1::PostApplied { .. } => "post_applied",
        StrictPanelOperationStateV1::AmbiguousPost { .. } => "ambiguous_post",
        StrictPanelOperationStateV1::CleanupPending { .. } => "cleanup_pending",
    }
}

fn parse_operation_key(
    guild_id: &str,
    ruleset_key: &str,
    panel_key: &str,
) -> Result<StrictPanelOperationKeyV1, StrictPanelJournalError> {
    let guild_id = guild_id
        .parse::<GuildId>()
        .map_err(|_| journal_backend("invalid persisted journal guild id"))?;
    let ruleset_key = RuleSetKey::parse(ruleset_key)
        .map_err(|_| journal_backend("invalid persisted journal ruleset key"))?;
    validate_panel_key(panel_key).map_err(journal_backend)?;
    Ok(StrictPanelOperationKeyV1 {
        guild_id,
        ruleset_key,
        panel_key: panel_key.to_string(),
    })
}

fn validate_operation(operation: &StrictPanelOperationV1) -> Result<(), StrictPanelJournalError> {
    validate_strict_panel_operation_v1(operation)
        .map_err(|error| journal_backend(format!("invalid strict panel operation: {error:?}")))
}

impl TryFrom<StrictPanelOperationRow> for StrictPanelOperationV1 {
    type Error = StrictPanelJournalError;

    fn try_from(row: StrictPanelOperationRow) -> Result<Self, Self::Error> {
        if row.record_format_version != RECORD_FORMAT_VERSION {
            return Err(journal_backend("unsupported journal record format"));
        }
        let persisted_key = parse_operation_key(&row.guild_id, &row.ruleset_key, &row.panel_key)?;
        let operation = row.operation_payload.0;
        validate_operation(&operation)?;
        if operation.key != persisted_key {
            return Err(journal_backend("persisted journal key mismatch"));
        }
        if state_tag(&operation.state) != row.state_tag {
            return Err(journal_backend("persisted journal state mismatch"));
        }
        Ok(operation)
    }
}

impl StrictPanelInstallationStore for PostgresPanelInstallationStore {
    async fn list_slot(
        &self,
        guild_id: GuildId,
        ruleset_key: &RuleSetKey,
    ) -> Result<Vec<PanelInstallation>, PanelInstallationStoreError> {
        let limit = i64::try_from(MAX_STRICT_PANEL_RECORDS_PER_SLOT + 1)
            .map_err(|_| backend("panel slot query limit overflow"))?;
        let rows = sqlx::query_as::<_, PanelInstallationRow>(
            "SELECT guild_id, ruleset_key, panel_key, installed_version, channel_id, message_id, spec_hash \
             FROM public.ruleset_panel_installations \
             WHERE guild_id = $1 AND ruleset_key = $2 \
             ORDER BY panel_key ASC \
             LIMIT $3",
        )
        .bind(guild_id.to_string())
        .bind(ruleset_key.as_str())
        .bind(limit)
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        if rows.len() > MAX_STRICT_PANEL_RECORDS_PER_SLOT {
            return Err(backend("panel installation slot capacity exceeded"));
        }
        rows.into_iter()
            .map(PanelInstallation::try_from)
            .map(|result| {
                let installation = result?;
                if installation.guild_id != guild_id || &installation.ruleset_key != ruleset_key {
                    return Err(backend("persisted panel slot identity mismatch"));
                }
                Ok(installation)
            })
            .collect()
    }

    async fn remove(&self, key: &PanelInstallationKey) -> Result<(), PanelInstallationStoreError> {
        validate_panel_key(&key.panel_key).map_err(backend)?;
        let guild_id = key.guild_id.to_string();
        let ruleset_key = key.ruleset_key.as_str();
        let mut transaction = begin_slot_write(self.pool(), &guild_id, ruleset_key)
            .await
            .map_err(backend)?;
        sqlx::query(
            "DELETE FROM public.ruleset_panel_installations \
             WHERE guild_id = $1 AND ruleset_key = $2 AND panel_key = $3",
        )
        .bind(guild_id)
        .bind(ruleset_key)
        .bind(&key.panel_key)
        .execute(&mut *transaction)
        .await
        .map_err(backend)?;
        transaction.commit().await.map_err(backend)?;
        Ok(())
    }
}

impl StrictPanelOperationJournal for PostgresPanelInstallationStore {
    async fn list_slot(
        &self,
        guild_id: GuildId,
        ruleset_key: &RuleSetKey,
    ) -> Result<Vec<StrictPanelOperationV1>, StrictPanelJournalError> {
        let limit = i64::try_from(MAX_STRICT_PANEL_RECORDS_PER_SLOT + 1)
            .map_err(|_| journal_backend("journal slot query limit overflow"))?;
        let rows = sqlx::query_as::<_, StrictPanelOperationRow>(
            "SELECT record_format_version, guild_id, ruleset_key, panel_key, state_tag, operation_payload \
             FROM public.strict_panel_operation_journal \
             WHERE guild_id = $1 AND ruleset_key = $2 \
             ORDER BY panel_key ASC \
             LIMIT $3",
        )
        .bind(guild_id.to_string())
        .bind(ruleset_key.as_str())
        .bind(limit)
        .fetch_all(self.pool())
        .await
        .map_err(journal_backend)?;
        if rows.len() > MAX_STRICT_PANEL_RECORDS_PER_SLOT {
            return Err(journal_backend("journal slot capacity exceeded"));
        }
        rows.into_iter()
            .map(StrictPanelOperationV1::try_from)
            .map(|result| {
                let operation = result?;
                if operation.key.guild_id != guild_id || &operation.key.ruleset_key != ruleset_key {
                    return Err(journal_backend("persisted journal slot identity mismatch"));
                }
                Ok(operation)
            })
            .collect()
    }

    async fn put(&self, operation: StrictPanelOperationV1) -> Result<(), StrictPanelJournalError> {
        validate_operation(&operation)?;
        let tag = state_tag(&operation.state);
        let guild_id = operation.key.guild_id.to_string();
        let ruleset_key = operation.key.ruleset_key.as_str();
        let mut transaction = begin_slot_write(self.pool(), &guild_id, ruleset_key)
            .await
            .map_err(journal_backend)?;
        let (exists, count) = sqlx::query_as::<_, (bool, i64)>(
            "SELECT \
             EXISTS(SELECT 1 FROM public.strict_panel_operation_journal \
                    WHERE guild_id = $1 AND ruleset_key = $2 AND panel_key = $3), \
             COUNT(*) \
             FROM public.strict_panel_operation_journal \
             WHERE guild_id = $1 AND ruleset_key = $2",
        )
        .bind(&guild_id)
        .bind(ruleset_key)
        .bind(&operation.key.panel_key)
        .fetch_one(&mut *transaction)
        .await
        .map_err(journal_backend)?;
        if !exists && count >= MAX_STRICT_PANEL_RECORDS_PER_SLOT as i64 {
            return Err(journal_backend("journal slot capacity exceeded"));
        }
        sqlx::query(
            "INSERT INTO public.strict_panel_operation_journal \
             (record_format_version, guild_id, ruleset_key, panel_key, state_tag, operation_payload) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (guild_id, ruleset_key, panel_key) DO UPDATE SET \
             record_format_version = EXCLUDED.record_format_version, \
             state_tag = EXCLUDED.state_tag, \
             operation_payload = EXCLUDED.operation_payload, \
             updated_at = CURRENT_TIMESTAMP",
        )
        .bind(RECORD_FORMAT_VERSION)
        .bind(guild_id)
        .bind(ruleset_key)
        .bind(&operation.key.panel_key)
        .bind(tag)
        .bind(Json(&operation))
        .execute(&mut *transaction)
        .await
        .map_err(journal_backend)?;
        transaction.commit().await.map_err(journal_backend)?;
        Ok(())
    }

    async fn remove(&self, key: &StrictPanelOperationKeyV1) -> Result<(), StrictPanelJournalError> {
        validate_panel_key(&key.panel_key).map_err(journal_backend)?;
        let guild_id = key.guild_id.to_string();
        let ruleset_key = key.ruleset_key.as_str();
        let mut transaction = begin_slot_write(self.pool(), &guild_id, ruleset_key)
            .await
            .map_err(journal_backend)?;
        sqlx::query(
            "DELETE FROM public.strict_panel_operation_journal \
             WHERE guild_id = $1 AND ruleset_key = $2 AND panel_key = $3",
        )
        .bind(guild_id)
        .bind(ruleset_key)
        .bind(&key.panel_key)
        .execute(&mut *transaction)
        .await
        .map_err(journal_backend)?;
        transaction.commit().await.map_err(journal_backend)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use automation_panel_installation::strict::{
        StrictPanelCleanupIntentV1, StrictPanelCleanupKindV1, StrictPanelMessageRefV1,
    };
    use discord_model::{ChannelId, MessageId};

    use super::*;

    fn cleanup_operation(panel_key: &str) -> StrictPanelOperationV1 {
        StrictPanelOperationV1 {
            key: StrictPanelOperationKeyV1 {
                guild_id: GuildId(7),
                ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
                panel_key: panel_key.to_string(),
            },
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
        }
    }

    fn row(operation: StrictPanelOperationV1) -> StrictPanelOperationRow {
        StrictPanelOperationRow {
            record_format_version: RECORD_FORMAT_VERSION,
            guild_id: operation.key.guild_id.to_string(),
            ruleset_key: operation.key.ruleset_key.as_str().to_string(),
            panel_key: operation.key.panel_key.clone(),
            state_tag: state_tag(&operation.state).to_string(),
            operation_payload: Json(operation),
        }
    }

    #[test]
    fn journal_row_rejects_key_and_state_mismatch() {
        let operation = cleanup_operation("entry");
        let mut key_mismatch = row(operation.clone());
        key_mismatch.panel_key = "other".to_string();
        assert!(StrictPanelOperationV1::try_from(key_mismatch).is_err());
        let mut state_mismatch = row(operation);
        state_mismatch.state_tag = "post_applied".to_string();
        assert!(StrictPanelOperationV1::try_from(state_mismatch).is_err());
    }

    #[test]
    fn journal_row_rejects_unknown_format() {
        let mut unknown_format = row(cleanup_operation("entry"));
        unknown_format.record_format_version = 2;
        assert!(StrictPanelOperationV1::try_from(unknown_format).is_err());
    }

    #[test]
    fn backend_messages_are_bounded_on_character_boundaries() {
        let message = "한".repeat(1_000);
        let bounded = bounded_message(message);
        assert!(bounded.len() <= 512);
        assert!(bounded.ends_with("..."));
    }
}
