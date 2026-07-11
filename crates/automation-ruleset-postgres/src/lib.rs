use std::sync::Arc;

use automation_ruleset::{
    PublishOutcome, PublishRuleSetRequest, RuleSetActivation, RuleSetContentHash, RuleSetHasher,
    RuleSetKey, RuleSetSchemaVersion, RuleSetStore, RuleSetStoreError, RuleSetVersion,
    RuleSetVersionId, Sha256RuleSetHasher,
};
use automation_state::InteractionRuleSet;
use discord_model::{GuildId, UserId};
use sqlx::PgPool;

const VERSION_COLUMNS: &str =
    "guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by";

pub struct PostgresRuleSetStore<H: RuleSetHasher = Sha256RuleSetHasher> {
    pool: PgPool,
    hasher: Arc<H>,
}

impl PostgresRuleSetStore<Sha256RuleSetHasher> {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            hasher: Arc::new(Sha256RuleSetHasher),
        }
    }
}

impl<H: RuleSetHasher> PostgresRuleSetStore<H> {
    pub fn with_hasher(pool: PgPool, hasher: H) -> Self {
        Self {
            pool,
            hasher: Arc::new(hasher),
        }
    }
}

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

fn backend(error: impl std::fmt::Display) -> RuleSetStoreError {
    RuleSetStoreError::Backend(error.to_string())
}

#[derive(sqlx::FromRow)]
struct RuleSetVersionRow {
    guild_id: String,
    ruleset_key: String,
    version: i64,
    schema_version: i64,
    definition: sqlx::types::Json<InteractionRuleSet>,
    content_hash: String,
    created_by: String,
}

impl TryFrom<RuleSetVersionRow> for RuleSetVersion {
    type Error = RuleSetStoreError;

    fn try_from(row: RuleSetVersionRow) -> Result<Self, Self::Error> {
        let guild_id = row
            .guild_id
            .parse::<GuildId>()
            .map_err(|_| backend(format!("invalid persisted guild_id: {}", row.guild_id)))?;
        let ruleset_key = RuleSetKey::parse(&row.ruleset_key)
            .map_err(|error| backend(format!("invalid persisted ruleset_key: {error:?}")))?;
        let version = u32::try_from(row.version)
            .ok()
            .and_then(|value| RuleSetVersionId::new(value).ok())
            .ok_or_else(|| backend(format!("invalid persisted version: {}", row.version)))?;
        let schema_version = u32::try_from(row.schema_version)
            .ok()
            .and_then(|value| RuleSetSchemaVersion::new(value).ok())
            .ok_or_else(|| {
                backend(format!(
                    "invalid persisted schema_version: {}",
                    row.schema_version
                ))
            })?;
        let content_hash = RuleSetContentHash::parse_hex(&row.content_hash).ok_or_else(|| {
            backend(format!(
                "invalid persisted content_hash: {}",
                row.content_hash
            ))
        })?;
        let created_by = row
            .created_by
            .parse::<UserId>()
            .map_err(|_| backend(format!("invalid persisted created_by: {}", row.created_by)))?;
        Ok(RuleSetVersion {
            guild_id,
            ruleset_key,
            version,
            schema_version,
            definition: row.definition.0,
            content_hash,
            created_by,
        })
    }
}

impl<H: RuleSetHasher> RuleSetStore for PostgresRuleSetStore<H> {
    async fn publish(
        &self,
        _request: PublishRuleSetRequest,
    ) -> Result<PublishOutcome, RuleSetStoreError> {
        let _ = &self.hasher;
        unimplemented!()
    }

    async fn get_version(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
        let row = sqlx::query_as::<_, RuleSetVersionRow>(&format!(
            "SELECT {VERSION_COLUMNS} FROM automation_ruleset_versions \
             WHERE guild_id = $1 AND ruleset_key = $2 AND version = $3"
        ))
        .bind(guild_id.to_string())
        .bind(key.as_str())
        .bind(i64::from(version.get()))
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(RuleSetVersion::try_from).transpose()
    }

    async fn list_versions(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Vec<RuleSetVersion>, RuleSetStoreError> {
        let rows = sqlx::query_as::<_, RuleSetVersionRow>(&format!(
            "SELECT {VERSION_COLUMNS} FROM automation_ruleset_versions \
             WHERE guild_id = $1 AND ruleset_key = $2 ORDER BY version"
        ))
        .bind(guild_id.to_string())
        .bind(key.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter().map(RuleSetVersion::try_from).collect()
    }

    async fn activate(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<RuleSetActivation, RuleSetStoreError> {
        let row = sqlx::query(
            "INSERT INTO automation_ruleset_activations (guild_id, ruleset_key, active_version) \
             SELECT guild_id, ruleset_key, version FROM automation_ruleset_versions \
             WHERE guild_id = $1 AND ruleset_key = $2 AND version = $3 \
             ON CONFLICT (guild_id, ruleset_key) DO UPDATE SET active_version = EXCLUDED.active_version \
             RETURNING active_version",
        )
        .bind(guild_id.to_string())
        .bind(key.as_str())
        .bind(i64::from(version.get()))
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        match row {
            Some(_) => Ok(RuleSetActivation {
                guild_id,
                ruleset_key: key.clone(),
                active_version: version,
            }),
            None => Err(RuleSetStoreError::VersionNotFound),
        }
    }

    async fn active(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
        let row = sqlx::query_as::<_, RuleSetVersionRow>(&format!(
            "SELECT {} FROM automation_ruleset_versions v \
             JOIN automation_ruleset_activations a \
               ON a.guild_id = v.guild_id AND a.ruleset_key = v.ruleset_key \
              AND a.active_version = v.version \
             WHERE v.guild_id = $1 AND v.ruleset_key = $2",
            VERSION_COLUMNS
                .split(", ")
                .map(|c| format!("v.{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .bind(guild_id.to_string())
        .bind(key.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(RuleSetVersion::try_from).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automation_ruleset::{content_hash, CURRENT_RULESET_SCHEMA_VERSION};
    use automation_state::{
        ActionSpec, ActionTarget, InstanceRef, InteractionRule, RoleRef, TriggerSpec,
    };

    fn definition() -> InteractionRuleSet {
        InteractionRuleSet {
            version: 1,
            panels: vec![],
            modals: vec![],
            rules: vec![InteractionRule {
                key: "r".to_string(),
                trigger: TriggerSpec::InstanceAction {
                    action: "join".to_string(),
                },
                actions: vec![ActionSpec::GrantRole {
                    role: RoleRef::Instance {
                        instance: InstanceRef::Event,
                        alias: "member_role".to_string(),
                    },
                    target: ActionTarget::Actor,
                }],
            }],
        }
    }

    fn row() -> RuleSetVersionRow {
        RuleSetVersionRow {
            guild_id: "7".to_string(),
            ruleset_key: "studyroom".to_string(),
            version: 1,
            schema_version: 1,
            definition: sqlx::types::Json(definition()),
            content_hash: content_hash(CURRENT_RULESET_SCHEMA_VERSION, &definition())
                .unwrap()
                .to_hex(),
            created_by: "3".to_string(),
        }
    }

    #[test]
    fn valid_row_converts() {
        let version = RuleSetVersion::try_from(row()).unwrap();
        assert_eq!(version.guild_id, GuildId(7));
        assert_eq!(version.version, RuleSetVersionId::FIRST);
        assert_eq!(version.created_by, UserId(3));
    }

    #[test]
    fn invalid_persisted_values_are_backend() {
        let mut bad = row();
        bad.version = 0;
        assert!(matches!(
            RuleSetVersion::try_from(bad),
            Err(RuleSetStoreError::Backend(_))
        ));
        let mut bad = row();
        bad.version = 5_000_000_000;
        assert!(matches!(
            RuleSetVersion::try_from(bad),
            Err(RuleSetStoreError::Backend(_))
        ));
        let mut bad = row();
        bad.content_hash = "nothex".to_string();
        assert!(matches!(
            RuleSetVersion::try_from(bad),
            Err(RuleSetStoreError::Backend(_))
        ));
        let mut bad = row();
        bad.ruleset_key = "bad key".to_string();
        assert!(matches!(
            RuleSetVersion::try_from(bad),
            Err(RuleSetStoreError::Backend(_))
        ));
    }
}
