use automation_instance::{
    AutomationInstance, InstanceId, InstanceRegistrarV1, InstanceRouteReaderV1, InstanceStatus,
    InstanceStoreError,
};
use automation_ruleset::RuleSetStoreError;
use automation_ruleset_dispatch::{
    PinnedInstanceResolverErrorV1, PinnedInstanceResolverV1, ResolvedPinnedInstanceV1,
};
use discord_model::GuildId;
use serde_json::Value;
use sqlx::types::Json;
use sqlx::{PgConnection, PgPool};

use crate::contract::{INSTANCE_REGISTER_QUERY, PINNED_READ_QUERY, ROUTE_READ_QUERY};
use crate::database::{
    begin_interaction_transaction, begin_interaction_transaction_on_connection,
    verify_runtime_interaction_database_with_timeouts_v1,
};
use crate::error::{map_mutation_commit_error, map_mutation_error, map_query_error};
use crate::route_connection::RouteConnectionGuardV1;
use crate::row::{
    InstanceRowV1, PinnedInstanceRowErrorV1, PinnedInstanceRowOutcomeV1, PinnedInstanceRowV1,
};
use crate::{
    RuntimeInteractionDatabaseExpectationV1, RuntimeInteractionDatabaseReadinessV1,
    RuntimeInteractionDatabaseTimeoutsV1, RuntimeInteractionPersistenceErrorV1,
    RuntimeInteractionRouteTimeoutV1,
};

#[derive(Clone)]
pub struct PostgresRuntimeInteractionV1 {
    pool: PgPool,
    expectation: RuntimeInteractionDatabaseExpectationV1,
    timeouts: RuntimeInteractionDatabaseTimeoutsV1,
    route_database_timeouts: RuntimeInteractionDatabaseTimeoutsV1,
    route_timeout: RuntimeInteractionRouteTimeoutV1,
    initial_readiness: RuntimeInteractionDatabaseReadinessV1,
}

#[derive(sqlx::FromRow)]
struct InstanceRegistrationOutcomeRowV1 {
    outcome: String,
}

impl PostgresRuntimeInteractionV1 {
    pub async fn connect_verified(
        pool: PgPool,
        expectation: RuntimeInteractionDatabaseExpectationV1,
        timeouts: RuntimeInteractionDatabaseTimeoutsV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        Self::connect_verified_with_route_timeout(
            pool,
            expectation,
            timeouts,
            RuntimeInteractionRouteTimeoutV1::default(),
        )
        .await
    }

    pub async fn connect_verified_with_route_timeout(
        pool: PgPool,
        expectation: RuntimeInteractionDatabaseExpectationV1,
        timeouts: RuntimeInteractionDatabaseTimeoutsV1,
        route_timeout: RuntimeInteractionRouteTimeoutV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        let route_database_timeouts = route_timeout.database_timeouts(timeouts)?;
        let initial_readiness =
            verify_runtime_interaction_database_with_timeouts_v1(&pool, &expectation, timeouts)
                .await?;
        Ok(Self {
            pool,
            expectation,
            timeouts,
            route_database_timeouts,
            route_timeout,
            initial_readiness,
        })
    }

    pub async fn connect_verified_default(
        pool: PgPool,
        expectation: RuntimeInteractionDatabaseExpectationV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        Self::connect_verified(
            pool,
            expectation,
            RuntimeInteractionDatabaseTimeoutsV1::default(),
        )
        .await
    }

    pub fn initial_readiness(&self) -> &RuntimeInteractionDatabaseReadinessV1 {
        &self.initial_readiness
    }

    pub fn route_timeout(&self) -> RuntimeInteractionRouteTimeoutV1 {
        self.route_timeout
    }

    pub fn route_database_timeouts(&self) -> RuntimeInteractionDatabaseTimeoutsV1 {
        self.route_database_timeouts
    }

    pub async fn verify_database_v1(
        &self,
    ) -> Result<RuntimeInteractionDatabaseReadinessV1, RuntimeInteractionPersistenceErrorV1> {
        verify_runtime_interaction_database_with_timeouts_v1(
            &self.pool,
            &self.expectation,
            self.timeouts,
        )
        .await
    }

    async fn read_instance_route_operation_v1(
        &self,
        connection: &mut PgConnection,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, RuntimeInteractionPersistenceErrorV1> {
        let mut transaction =
            begin_interaction_transaction_on_connection(connection, self.route_database_timeouts)
                .await?;
        let rows = sqlx::query_as::<_, InstanceRowV1>(ROUTE_READ_QUERY)
            .bind(guild_id.to_string())
            .bind(instance_id.as_str())
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| map_query_error(&error))?;
        let instance = match rows.len() {
            0 => None,
            1 => Some(
                rows.into_iter()
                    .next()
                    .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?
                    .decode(guild_id, instance_id)?,
            ),
            _ => {
                return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
            }
        };
        transaction
            .commit()
            .await
            .map_err(|error| map_query_error(&error))?;
        Ok(instance)
    }
}

impl InstanceRouteReaderV1 for PostgresRuntimeInteractionV1 {
    async fn read_instance_route_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        let deadline = tokio::time::Instant::now() + self.route_timeout.duration();
        let connection = tokio::time::timeout_at(deadline, self.pool.acquire())
            .await
            .map_err(|_| InstanceStoreError::TimedOut)?
            .map_err(|error| instance_route_error(map_query_error(&error)))?;
        let mut connection = RouteConnectionGuardV1::new(connection);
        let Some(route_connection) = connection.connection_mut() else {
            return Err(instance_corrupt());
        };
        let result = tokio::time::timeout_at(
            deadline,
            self.read_instance_route_operation_v1(route_connection, guild_id, instance_id),
        )
        .await;
        match result {
            Ok(result) => {
                connection.release_to_pool();
                result.map_err(instance_route_error)
            }
            Err(_) => Err(InstanceStoreError::TimedOut),
        }
    }
}

impl InstanceRegistrarV1 for PostgresRuntimeInteractionV1 {
    async fn register_instance_v1(
        &self,
        instance: AutomationInstance,
    ) -> Result<(), InstanceStoreError> {
        if instance.status != InstanceStatus::Active {
            return Err(instance_backend(
                RuntimeInteractionPersistenceErrorV1::InvalidInput,
            ));
        }
        let resources = serde_json::to_value(&instance.resources)
            .map_err(|_| instance_backend(RuntimeInteractionPersistenceErrorV1::InvalidInput))?;
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts)
            .await
            .map_err(instance_backend)?;
        let rows = sqlx::query_as::<_, InstanceRegistrationOutcomeRowV1>(INSTANCE_REGISTER_QUERY)
            .bind(instance.guild_id.to_string())
            .bind(instance.id.as_str())
            .bind(&instance.ruleset_key)
            .bind(i64::from(instance.ruleset_version.get()))
            .bind(&instance.kind.0)
            .bind(instance.created_by.to_string())
            .bind(Json::<Value>(resources))
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| instance_backend(map_mutation_error(&error)))?;
        let [row] = rows.as_slice() else {
            return Err(instance_corrupt());
        };
        match row.outcome.as_str() {
            "created" | "exact_replay" => {}
            "conflict" => return Err(InstanceStoreError::DuplicateInstance),
            _ => return Err(instance_corrupt()),
        }
        transaction
            .commit()
            .await
            .map_err(|error| instance_backend(map_mutation_commit_error(&error)))?;
        Ok(())
    }
}

impl PinnedInstanceResolverV1 for PostgresRuntimeInteractionV1 {
    async fn resolve_pinned_instance_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<ResolvedPinnedInstanceV1, PinnedInstanceResolverErrorV1> {
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts)
            .await
            .map_err(pinned_instance_backend)?;
        let rows = sqlx::query_as::<_, PinnedInstanceRowV1>(PINNED_READ_QUERY)
            .bind(guild_id.to_string())
            .bind(instance_id.as_str())
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| pinned_instance_backend(map_query_error(&error)))?;
        let row = match rows.len() {
            0 => return Err(PinnedInstanceResolverErrorV1::InstanceNotFound),
            1 => rows
                .into_iter()
                .next()
                .ok_or_else(pinned_instance_corrupt)?,
            _ => return Err(pinned_instance_corrupt()),
        };
        let outcome = row
            .decode(guild_id, instance_id)
            .map_err(|error| match error {
                PinnedInstanceRowErrorV1::Instance => pinned_instance_corrupt(),
                PinnedInstanceRowErrorV1::Artifact => pinned_artifact_corrupt(),
            })?;
        transaction
            .commit()
            .await
            .map_err(|error| pinned_instance_backend(map_query_error(&error)))?;
        match outcome {
            PinnedInstanceRowOutcomeV1::Resolved(resolved) => Ok(*resolved),
            PinnedInstanceRowOutcomeV1::Inactive(status) => {
                Err(PinnedInstanceResolverErrorV1::InstanceInactive(status))
            }
            PinnedInstanceRowOutcomeV1::PinnedVersionMissing => {
                Err(PinnedInstanceResolverErrorV1::PinnedVersionMissing)
            }
        }
    }
}

fn instance_backend(error: RuntimeInteractionPersistenceErrorV1) -> InstanceStoreError {
    InstanceStoreError::Backend(error.code().to_string())
}

fn instance_route_error(error: RuntimeInteractionPersistenceErrorV1) -> InstanceStoreError {
    match error {
        RuntimeInteractionPersistenceErrorV1::Timeout => InstanceStoreError::TimedOut,
        error => instance_backend(error),
    }
}

fn instance_corrupt() -> InstanceStoreError {
    instance_backend(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn pinned_instance_backend(
    error: RuntimeInteractionPersistenceErrorV1,
) -> PinnedInstanceResolverErrorV1 {
    PinnedInstanceResolverErrorV1::InstanceLookup(instance_backend(error))
}

fn pinned_instance_corrupt() -> PinnedInstanceResolverErrorV1 {
    pinned_instance_backend(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

fn pinned_artifact_corrupt() -> PinnedInstanceResolverErrorV1 {
    PinnedInstanceResolverErrorV1::VersionLookup(RuleSetStoreError::Backend(
        RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt
            .code()
            .to_string(),
    ))
}
