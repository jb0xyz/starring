use automation_instance::{
    AutomationInstance, InstanceId, InstanceKind, InstanceResources, InstanceStatus, InstanceStore,
    InstanceStoreError,
};
use discord_model::{GuildId, UserId};
use sqlx::PgPool;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

pub struct PostgresInstanceStore {
    pool: PgPool,
}

impl PostgresInstanceStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct AutomationInstanceRow {
    guild_id: String,
    instance_id: String,
    ruleset_key: String,
    kind: String,
    created_by: String,
    status: String,
    resources: sqlx::types::Json<InstanceResources>,
}

fn status_str(status: InstanceStatus) -> &'static str {
    match status {
        InstanceStatus::Active => "active",
        InstanceStatus::Disabled => "disabled",
        InstanceStatus::Deleted => "deleted",
    }
}

fn backend(error: impl std::fmt::Display) -> InstanceStoreError {
    InstanceStoreError::Backend(error.to_string())
}

impl TryFrom<AutomationInstanceRow> for AutomationInstance {
    type Error = InstanceStoreError;

    fn try_from(row: AutomationInstanceRow) -> Result<Self, Self::Error> {
        let guild_id = row
            .guild_id
            .parse::<GuildId>()
            .map_err(|_| backend(format!("invalid persisted guild_id: {}", row.guild_id)))?;
        let id = InstanceId::parse(&row.instance_id)
            .map_err(|error| backend(format!("invalid persisted instance_id: {error:?}")))?;
        let created_by = row
            .created_by
            .parse::<UserId>()
            .map_err(|_| backend(format!("invalid persisted created_by: {}", row.created_by)))?;
        let status = match row.status.as_str() {
            "active" => InstanceStatus::Active,
            "disabled" => InstanceStatus::Disabled,
            "deleted" => InstanceStatus::Deleted,
            other => return Err(backend(format!("invalid persisted status: {other}"))),
        };
        Ok(AutomationInstance {
            id,
            guild_id,
            ruleset_key: row.ruleset_key,
            kind: InstanceKind(row.kind),
            created_by,
            resources: row.resources.0,
            status,
        })
    }
}

impl InstanceStore for PostgresInstanceStore {
    async fn register(&self, instance: AutomationInstance) -> Result<(), InstanceStoreError> {
        let result = sqlx::query(
            "INSERT INTO automation_instances (guild_id, instance_id, ruleset_key, kind, created_by, status, resources) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (guild_id, instance_id) DO NOTHING",
        )
        .bind(instance.guild_id.to_string())
        .bind(instance.id.as_str())
        .bind(&instance.ruleset_key)
        .bind(&instance.kind.0)
        .bind(instance.created_by.to_string())
        .bind(status_str(instance.status))
        .bind(sqlx::types::Json(&instance.resources))
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        if result.rows_affected() == 0 {
            return Err(InstanceStoreError::DuplicateInstance);
        }
        Ok(())
    }

    async fn get(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        let row = sqlx::query_as::<_, AutomationInstanceRow>(
            "SELECT guild_id, instance_id, ruleset_key, kind, created_by, status, resources \
             FROM automation_instances WHERE guild_id = $1 AND instance_id = $2",
        )
        .bind(guild_id.to_string())
        .bind(instance_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(AutomationInstance::try_from).transpose()
    }

    async fn list_by_guild(
        &self,
        guild_id: GuildId,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
        let rows = sqlx::query_as::<_, AutomationInstanceRow>(
            "SELECT guild_id, instance_id, ruleset_key, kind, created_by, status, resources \
             FROM automation_instances WHERE guild_id = $1 ORDER BY instance_id",
        )
        .bind(guild_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter().map(AutomationInstance::try_from).collect()
    }

    async fn update_status(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
        status: InstanceStatus,
    ) -> Result<(), InstanceStoreError> {
        let result = sqlx::query(
            "UPDATE automation_instances SET status = $3 WHERE guild_id = $1 AND instance_id = $2",
        )
        .bind(guild_id.to_string())
        .bind(instance_id.as_str())
        .bind(status_str(status))
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        if result.rows_affected() == 0 {
            return Err(InstanceStoreError::NotFound);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(instance_id: &str, status: &str) -> AutomationInstanceRow {
        AutomationInstanceRow {
            guild_id: "7".to_string(),
            instance_id: instance_id.to_string(),
            ruleset_key: "studyroom_demo".to_string(),
            kind: "study_room".to_string(),
            created_by: "3".to_string(),
            status: status.to_string(),
            resources: sqlx::types::Json(InstanceResources::default()),
        }
    }

    #[test]
    fn valid_row_converts() {
        let instance = AutomationInstance::try_from(row("room1", "active")).unwrap();
        assert_eq!(instance.guild_id, GuildId(7));
        assert_eq!(instance.id.as_str(), "room1");
        assert_eq!(instance.status, InstanceStatus::Active);
        assert_eq!(instance.created_by, UserId(3));
        assert_eq!(instance.kind, InstanceKind("study_room".to_string()));
    }

    #[test]
    fn invalid_persisted_instance_id_is_backend() {
        assert!(matches!(
            AutomationInstance::try_from(row("bad id", "active")),
            Err(InstanceStoreError::Backend(_))
        ));
    }

    #[test]
    fn invalid_persisted_status_is_backend() {
        assert!(matches!(
            AutomationInstance::try_from(row("room1", "weird")),
            Err(InstanceStoreError::Backend(_))
        ));
    }
}
