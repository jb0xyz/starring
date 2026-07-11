use automation_panel_installation::{
    PanelInstallation, PanelInstallationKey, PanelInstallationStore, PanelInstallationStoreError,
};
use automation_ruleset::{RuleSetKey, RuleSetVersionId};
use discord_model::{ChannelId, GuildId, MessageId};
use sqlx::PgPool;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

pub struct PostgresPanelInstallationStore {
    pool: PgPool,
}

impl PostgresPanelInstallationStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct PanelInstallationRow {
    guild_id: String,
    ruleset_key: String,
    panel_key: String,
    installed_version: i64,
    channel_id: String,
    message_id: String,
    spec_hash: String,
}

fn backend(error: impl std::fmt::Display) -> PanelInstallationStoreError {
    PanelInstallationStoreError::Backend(error.to_string())
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

impl TryFrom<PanelInstallationRow> for PanelInstallation {
    type Error = PanelInstallationStoreError;

    fn try_from(row: PanelInstallationRow) -> Result<Self, Self::Error> {
        let guild_id = row
            .guild_id
            .parse::<GuildId>()
            .map_err(|_| backend(format!("invalid persisted guild_id: {}", row.guild_id)))?;
        let ruleset_key = RuleSetKey::parse(&row.ruleset_key)
            .map_err(|error| backend(format!("invalid persisted ruleset_key: {error:?}")))?;
        let version_value = u32::try_from(row.installed_version).map_err(|_| {
            backend(format!(
                "invalid persisted installed_version: {}",
                row.installed_version
            ))
        })?;
        let installed_version = RuleSetVersionId::new(version_value).map_err(|_| {
            backend(format!(
                "invalid persisted installed_version: {}",
                row.installed_version
            ))
        })?;
        let channel_id = row
            .channel_id
            .parse::<ChannelId>()
            .map_err(|_| backend(format!("invalid persisted channel_id: {}", row.channel_id)))?;
        let message_id = row
            .message_id
            .parse::<MessageId>()
            .map_err(|_| backend(format!("invalid persisted message_id: {}", row.message_id)))?;
        if !valid_hash(&row.spec_hash) {
            return Err(backend(format!(
                "invalid persisted spec_hash: {}",
                row.spec_hash
            )));
        }
        Ok(PanelInstallation {
            guild_id,
            ruleset_key,
            panel_key: row.panel_key,
            installed_version,
            channel_id,
            message_id,
            spec_hash: row.spec_hash,
        })
    }
}

impl PanelInstallationStore for PostgresPanelInstallationStore {
    async fn get(
        &self,
        key: &PanelInstallationKey,
    ) -> Result<Option<PanelInstallation>, PanelInstallationStoreError> {
        let row = sqlx::query_as::<_, PanelInstallationRow>(
            "SELECT guild_id, ruleset_key, panel_key, installed_version, channel_id, message_id, spec_hash \
             FROM ruleset_panel_installations \
             WHERE guild_id = $1 AND ruleset_key = $2 AND panel_key = $3",
        )
        .bind(key.guild_id.to_string())
        .bind(key.ruleset_key.as_str())
        .bind(&key.panel_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(PanelInstallation::try_from).transpose()
    }

    async fn upsert(
        &self,
        installation: PanelInstallation,
    ) -> Result<(), PanelInstallationStoreError> {
        sqlx::query(
            "INSERT INTO ruleset_panel_installations \
             (guild_id, ruleset_key, panel_key, installed_version, channel_id, message_id, spec_hash) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (guild_id, ruleset_key, panel_key) DO UPDATE SET \
             installed_version = EXCLUDED.installed_version, \
             channel_id = EXCLUDED.channel_id, \
             message_id = EXCLUDED.message_id, \
             spec_hash = EXCLUDED.spec_hash",
        )
        .bind(installation.guild_id.to_string())
        .bind(installation.ruleset_key.as_str())
        .bind(&installation.panel_key)
        .bind(i64::from(installation.installed_version.get()))
        .bind(installation.channel_id.to_string())
        .bind(installation.message_id.to_string())
        .bind(&installation.spec_hash)
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> PanelInstallationRow {
        PanelInstallationRow {
            guild_id: "7".to_string(),
            ruleset_key: "studyroom".to_string(),
            panel_key: "entry".to_string(),
            installed_version: 1,
            channel_id: "10".to_string(),
            message_id: "100".to_string(),
            spec_hash: "a".repeat(64),
        }
    }

    #[test]
    fn valid_row_converts() {
        let installation = PanelInstallation::try_from(row()).unwrap();
        assert_eq!(installation.guild_id, GuildId(7));
        assert_eq!(installation.ruleset_key.as_str(), "studyroom");
        assert_eq!(installation.installed_version, RuleSetVersionId::FIRST);
        assert_eq!(installation.channel_id, ChannelId(10));
        assert_eq!(installation.message_id, MessageId(100));
    }

    #[test]
    fn invalid_version_is_backend() {
        for value in [0, -1, i64::from(u32::MAX) + 1] {
            let mut invalid = row();
            invalid.installed_version = value;
            assert!(matches!(
                PanelInstallation::try_from(invalid),
                Err(PanelInstallationStoreError::Backend(_))
            ));
        }
    }

    #[test]
    fn invalid_ids_and_key_are_backend() {
        let mut invalid_guild = row();
        invalid_guild.guild_id = "bad".to_string();
        let mut invalid_key = row();
        invalid_key.ruleset_key = "bad key".to_string();
        let mut invalid_channel = row();
        invalid_channel.channel_id = "bad".to_string();
        let mut invalid_message = row();
        invalid_message.message_id = "bad".to_string();
        for invalid in [invalid_guild, invalid_key, invalid_channel, invalid_message] {
            assert!(matches!(
                PanelInstallation::try_from(invalid),
                Err(PanelInstallationStoreError::Backend(_))
            ));
        }
    }

    #[test]
    fn invalid_hash_is_backend() {
        for hash in ["a".repeat(63), "A".repeat(64), "g".repeat(64)] {
            let mut invalid = row();
            invalid.spec_hash = hash;
            assert!(matches!(
                PanelInstallation::try_from(invalid),
                Err(PanelInstallationStoreError::Backend(_))
            ));
        }
    }
}
