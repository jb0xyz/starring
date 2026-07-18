use std::sync::Arc;

use automation_ruleset::{
    ExpectedActiveRuleSet, GuardedActivationOutcome, GuardedRuleSetActivation, PublishOutcome,
    PublishRuleSetRequest, RuleSetActivation, RuleSetContentHash, RuleSetHasher, RuleSetKey,
    RuleSetSchemaVersion, RuleSetStore, RuleSetStoreError, RuleSetVersion, RuleSetVersionId,
    RuleSetVersionIdentity, Sha256RuleSetHasher, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_state::InteractionRuleSet;
use discord_model::{GuildId, UserId};
use sqlx::{PgConnection, PgPool, Row};

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

async fn locked_active_identity(
    connection: &mut PgConnection,
    guild_id: &str,
    ruleset_key: &str,
) -> Result<Option<RuleSetVersionIdentity>, RuleSetStoreError> {
    let row = sqlx::query(
        "SELECT v.version, v.content_hash \
         FROM automation_ruleset_activations a \
         JOIN automation_ruleset_versions v \
           ON v.guild_id = a.guild_id AND v.ruleset_key = a.ruleset_key \
          AND v.version = a.active_version \
         WHERE a.guild_id = $1 AND a.ruleset_key = $2 \
         FOR UPDATE OF a",
    )
    .bind(guild_id)
    .bind(ruleset_key)
    .fetch_optional(connection)
    .await
    .map_err(backend)?;
    row.map(|row| {
        let version: i64 = row.try_get("version").map_err(backend)?;
        let content_hash: String = row.try_get("content_hash").map_err(backend)?;
        let version = u32::try_from(version)
            .ok()
            .and_then(|value| RuleSetVersionId::new(value).ok())
            .ok_or_else(|| backend("invalid active RuleSet version"))?;
        let content_hash = RuleSetContentHash::parse_hex(&content_hash)
            .ok_or_else(|| backend("invalid active RuleSet content hash"))?;
        Ok(RuleSetVersionIdentity {
            version,
            content_hash,
        })
    })
    .transpose()
}

async fn lock_ruleset_head(
    connection: &mut PgConnection,
    guild_id: &str,
    ruleset_key: &str,
) -> Result<(), RuleSetStoreError> {
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT next_version FROM automation_ruleset_heads \
         WHERE guild_id = $1 AND ruleset_key = $2 FOR UPDATE",
    )
    .bind(guild_id)
    .bind(ruleset_key)
    .fetch_optional(connection)
    .await
    .map_err(backend)?
    .is_some();
    if exists {
        Ok(())
    } else {
        Err(RuleSetStoreError::VersionNotFound)
    }
}

fn activation(
    guild_id: GuildId,
    ruleset_key: RuleSetKey,
    active_version: RuleSetVersionId,
) -> RuleSetActivation {
    RuleSetActivation {
        guild_id,
        ruleset_key,
        active_version,
    }
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
        request: PublishRuleSetRequest,
    ) -> Result<PublishOutcome, RuleSetStoreError> {
        automation_core::validate_structural(&request.definition)
            .map_err(RuleSetStoreError::InvalidDefinition)?;
        let schema_version = CURRENT_RULESET_SCHEMA_VERSION;
        let content_hash = self
            .hasher
            .hash(schema_version, &request.definition)
            .map_err(|error| match error {
                automation_ruleset::RuleSetHashError::Serialization(message) => {
                    RuleSetStoreError::Canonicalization(message)
                }
            })?;
        let guild = request.guild_id.to_string();
        let key = request.ruleset_key.as_str();
        let hash_hex = content_hash.to_hex();

        let mut tx = self.pool.begin().await.map_err(backend)?;

        sqlx::query(
            "INSERT INTO automation_ruleset_heads (guild_id, ruleset_key, next_version) \
             VALUES ($1, $2, 1) ON CONFLICT (guild_id, ruleset_key) DO NOTHING",
        )
        .bind(&guild)
        .bind(key)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;

        let next_version: i64 = sqlx::query_scalar(
            "SELECT next_version FROM automation_ruleset_heads \
             WHERE guild_id = $1 AND ruleset_key = $2 FOR UPDATE",
        )
        .bind(&guild)
        .bind(key)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;

        let existing = sqlx::query_as::<_, RuleSetVersionRow>(&format!(
            "SELECT {VERSION_COLUMNS} FROM automation_ruleset_versions \
             WHERE guild_id = $1 AND ruleset_key = $2 AND content_hash = $3"
        ))
        .bind(&guild)
        .bind(key)
        .bind(&hash_hex)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;

        if let Some(row) = existing {
            let existing_version = RuleSetVersion::try_from(row)?;
            if existing_version.schema_version == schema_version
                && existing_version.definition == request.definition
            {
                tx.commit().await.map_err(backend)?;
                return Ok(PublishOutcome::Reused(existing_version));
            }
            tx.rollback().await.map_err(backend)?;
            return Err(RuleSetStoreError::HashCollision);
        }

        let version = match u32::try_from(next_version)
            .ok()
            .and_then(|value| RuleSetVersionId::new(value).ok())
        {
            Some(version) => version,
            None => {
                tx.rollback().await.map_err(backend)?;
                return Err(RuleSetStoreError::VersionOverflow);
            }
        };

        sqlx::query(&format!(
            "INSERT INTO automation_ruleset_versions ({VERSION_COLUMNS}) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)"
        ))
        .bind(&guild)
        .bind(key)
        .bind(i64::from(version.get()))
        .bind(i64::from(schema_version.get()))
        .bind(sqlx::types::Json(&request.definition))
        .bind(&hash_hex)
        .bind(request.created_by.to_string())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;

        sqlx::query(
            "UPDATE automation_ruleset_heads SET next_version = next_version + 1 \
             WHERE guild_id = $1 AND ruleset_key = $2",
        )
        .bind(&guild)
        .bind(key)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;

        tx.commit().await.map_err(backend)?;

        Ok(PublishOutcome::Created(RuleSetVersion {
            guild_id: request.guild_id,
            ruleset_key: request.ruleset_key,
            version,
            schema_version,
            definition: request.definition,
            content_hash,
            created_by: request.created_by,
        }))
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
        let guild = guild_id.to_string();
        let mut tx = self.pool.begin().await.map_err(backend)?;
        lock_ruleset_head(&mut tx, &guild, key.as_str()).await?;
        let row = sqlx::query(
            "INSERT INTO automation_ruleset_activations (guild_id, ruleset_key, active_version) \
             SELECT guild_id, ruleset_key, version FROM automation_ruleset_versions \
             WHERE guild_id = $1 AND ruleset_key = $2 AND version = $3 \
             ON CONFLICT (guild_id, ruleset_key) DO UPDATE SET active_version = EXCLUDED.active_version \
             RETURNING active_version",
        )
        .bind(&guild)
        .bind(key.as_str())
        .bind(i64::from(version.get()))
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        match row {
            Some(_) => {
                tx.commit().await.map_err(backend)?;
                Ok(RuleSetActivation {
                    guild_id,
                    ruleset_key: key.clone(),
                    active_version: version,
                })
            }
            None => {
                tx.rollback().await.map_err(backend)?;
                Err(RuleSetStoreError::VersionNotFound)
            }
        }
    }

    async fn activate_guarded(
        &self,
        request: GuardedRuleSetActivation,
    ) -> Result<GuardedActivationOutcome, RuleSetStoreError> {
        let guild = request.guild_id.to_string();
        let key = request.ruleset_key.as_str();
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let target = sqlx::query_as::<_, RuleSetVersionRow>(&format!(
            "SELECT {VERSION_COLUMNS} FROM automation_ruleset_versions \
             WHERE guild_id = $1 AND ruleset_key = $2 AND version = $3"
        ))
        .bind(&guild)
        .bind(key)
        .bind(i64::from(request.target.version.get()))
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?
        .ok_or(RuleSetStoreError::VersionNotFound)
        .and_then(RuleSetVersion::try_from)?;
        if target.content_hash != request.target.content_hash {
            tx.rollback().await.map_err(backend)?;
            return Err(RuleSetStoreError::TargetHashMismatch);
        }
        match lock_ruleset_head(&mut tx, &guild, key).await {
            Ok(()) => {}
            Err(RuleSetStoreError::VersionNotFound) => {
                tx.rollback().await.map_err(backend)?;
                return Err(backend("published RuleSet target has no head row"));
            }
            Err(error) => return Err(error),
        }

        let mut observed = locked_active_identity(&mut tx, &guild, key).await?;
        let result_activation = activation(
            request.guild_id,
            request.ruleset_key.clone(),
            request.target.version,
        );
        if observed.as_ref() == Some(&request.target) {
            tx.commit().await.map_err(backend)?;
            return Ok(GuardedActivationOutcome::AlreadyTarget(result_activation));
        }
        let baseline_matches = match &request.expected_active {
            ExpectedActiveRuleSet::Absent => observed.is_none(),
            ExpectedActiveRuleSet::Exact { identity } => observed.as_ref() == Some(identity),
        };
        if !baseline_matches {
            tx.commit().await.map_err(backend)?;
            return Ok(GuardedActivationOutcome::BaselineMismatch {
                observed_active: observed,
            });
        }

        if observed.is_some() {
            let updated = sqlx::query(
                "UPDATE automation_ruleset_activations SET active_version = $3 \
                 WHERE guild_id = $1 AND ruleset_key = $2",
            )
            .bind(&guild)
            .bind(key)
            .bind(i64::from(request.target.version.get()))
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
            if updated.rows_affected() != 1 {
                tx.rollback().await.map_err(backend)?;
                return Err(backend("guarded activation update lost its locked row"));
            }
        } else {
            let inserted = sqlx::query(
                "INSERT INTO automation_ruleset_activations \
                 (guild_id, ruleset_key, active_version) VALUES ($1, $2, $3) \
                 ON CONFLICT (guild_id, ruleset_key) DO NOTHING",
            )
            .bind(&guild)
            .bind(key)
            .bind(i64::from(request.target.version.get()))
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
            if inserted.rows_affected() == 0 {
                observed = locked_active_identity(&mut tx, &guild, key).await?;
                if observed.is_none() {
                    tx.rollback().await.map_err(backend)?;
                    return Err(backend(
                        "guarded activation insert conflicted without an active pointer",
                    ));
                }
                tx.commit().await.map_err(backend)?;
                if observed.as_ref() == Some(&request.target) {
                    return Ok(GuardedActivationOutcome::AlreadyTarget(result_activation));
                }
                return Ok(GuardedActivationOutcome::BaselineMismatch {
                    observed_active: observed,
                });
            }
        }
        tx.commit().await.map_err(backend)?;
        Ok(GuardedActivationOutcome::Activated(result_activation))
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
