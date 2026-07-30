use std::num::NonZeroUsize;

use automation_instance::{
    AutomationInstance, InstanceId, InstanceRegistrarV1, InstanceRouteReaderV1, InstanceStatus,
    InstanceStoreError, InstanceTeardownClaimOutcomeV1, InstanceTeardownMarkOutcomeV1,
    InstanceTeardownRetryScanCursorV2, InstanceTeardownRetryScanPageV2,
    InstanceTeardownRetryScannerV2, InstanceTeardownStoreV1, MAX_INSTANCE_TEARDOWN_RETRY_BATCH_V1,
    MAX_INSTANCE_TEARDOWN_RETRY_SCAN_BATCH_V2,
};
use automation_ruleset::RuleSetStoreError;
use automation_ruleset_dispatch::{
    PinnedInstanceResolverErrorV1, PinnedInstanceResolverV1, ResolvedPinnedInstanceV1,
};
use discord_model::GuildId;
use serde_json::Value;
use sqlx::types::Json;
use sqlx::{PgConnection, PgPool};

use crate::contract::{
    INSTANCE_REGISTER_QUERY, INSTANCE_TEARDOWN_CLAIM_QUERY, INSTANCE_TEARDOWN_GET_QUERY,
    INSTANCE_TEARDOWN_MARK_QUERY, INSTANCE_TEARDOWN_RETRY_QUERY,
    INSTANCE_TEARDOWN_RETRY_SCAN_QUERY, PINNED_READ_QUERY, ROUTE_READ_QUERY,
};
use crate::database::{
    begin_interaction_transaction, begin_interaction_transaction_on_connection,
    verify_runtime_interaction_binding_v1, verify_runtime_interaction_database_with_timeouts_v1,
};
use crate::error::{map_mutation_commit_error, map_mutation_error, map_query_error};
use crate::route_connection::RouteConnectionGuardV1;
use crate::row::{
    InstanceRowV1, InstanceTeardownRetryScanRowV2, PinnedInstanceRowErrorV1,
    PinnedInstanceRowOutcomeV1, PinnedInstanceRowV1,
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
struct InstanceOutcomeRowV1 {
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
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation).await?;
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
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation)
            .await
            .map_err(instance_backend)?;
        let rows = sqlx::query_as::<_, InstanceOutcomeRowV1>(INSTANCE_REGISTER_QUERY)
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

impl InstanceTeardownStoreV1 for PostgresRuntimeInteractionV1 {
    async fn get_for_teardown_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts)
            .await
            .map_err(instance_route_error)?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation)
            .await
            .map_err(instance_route_error)?;
        let rows = sqlx::query_as::<_, InstanceRowV1>(INSTANCE_TEARDOWN_GET_QUERY)
            .bind(guild_id.to_string())
            .bind(instance_id.as_str())
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| instance_route_error(map_query_error(&error)))?;
        let instance = match rows.len() {
            0 => None,
            1 => Some(
                rows.into_iter()
                    .next()
                    .ok_or_else(instance_corrupt)?
                    .decode(guild_id, instance_id)
                    .map_err(instance_backend)?,
            ),
            _ => return Err(instance_corrupt()),
        };
        transaction
            .commit()
            .await
            .map_err(|error| instance_route_error(map_query_error(&error)))?;
        Ok(instance)
    }

    async fn claim_deleting_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<InstanceTeardownClaimOutcomeV1, InstanceStoreError> {
        let outcome = self
            .teardown_mutation_outcome_v1(INSTANCE_TEARDOWN_CLAIM_QUERY, guild_id, instance_id)
            .await?;
        match outcome.as_str() {
            "claimed" => Ok(InstanceTeardownClaimOutcomeV1::Claimed),
            "already_deleting" => Ok(InstanceTeardownClaimOutcomeV1::AlreadyDeleting),
            "already_deleted" => Ok(InstanceTeardownClaimOutcomeV1::AlreadyDeleted),
            "not_found" => Err(InstanceStoreError::NotFound),
            _ => Err(instance_corrupt()),
        }
    }

    async fn mark_deleted_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<InstanceTeardownMarkOutcomeV1, InstanceStoreError> {
        let outcome = self
            .teardown_mutation_outcome_v1(INSTANCE_TEARDOWN_MARK_QUERY, guild_id, instance_id)
            .await?;
        match outcome.as_str() {
            "marked_deleted" => Ok(InstanceTeardownMarkOutcomeV1::MarkedDeleted),
            "already_deleted" => Ok(InstanceTeardownMarkOutcomeV1::AlreadyDeleted),
            "not_found" => Err(InstanceStoreError::NotFound),
            "conflict" => Err(instance_backend(
                RuntimeInteractionPersistenceErrorV1::Conflict,
            )),
            _ => Err(instance_corrupt()),
        }
    }

    async fn list_retryable_v1(
        &self,
        guild_id: GuildId,
        limit: NonZeroUsize,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
        if limit.get() > MAX_INSTANCE_TEARDOWN_RETRY_BATCH_V1 {
            return Err(instance_backend(
                RuntimeInteractionPersistenceErrorV1::InvalidInput,
            ));
        }
        let limit = i64::try_from(limit.get())
            .map_err(|_| instance_backend(RuntimeInteractionPersistenceErrorV1::InvalidInput))?;
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts)
            .await
            .map_err(instance_route_error)?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation)
            .await
            .map_err(instance_route_error)?;
        let rows = sqlx::query_as::<_, InstanceRowV1>(INSTANCE_TEARDOWN_RETRY_QUERY)
            .bind(guild_id.to_string())
            .bind(limit)
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| instance_route_error(map_query_error(&error)))?;
        if rows.len() > usize::try_from(limit).map_err(|_| instance_corrupt())? {
            return Err(instance_corrupt());
        }
        let instances = rows
            .into_iter()
            .map(|row| row.decode_retryable(guild_id).map_err(instance_backend))
            .collect::<Result<Vec<_>, _>>()?;
        if instances.windows(2).any(|pair| pair[0].id >= pair[1].id) {
            return Err(instance_corrupt());
        }
        transaction
            .commit()
            .await
            .map_err(|error| instance_route_error(map_query_error(&error)))?;
        Ok(instances)
    }
}

impl InstanceTeardownRetryScannerV2 for PostgresRuntimeInteractionV1 {
    async fn scan_retryable_v2(
        &self,
        cursor: &InstanceTeardownRetryScanCursorV2,
        limit: NonZeroUsize,
    ) -> Result<InstanceTeardownRetryScanPageV2, InstanceStoreError> {
        if limit.get() > MAX_INSTANCE_TEARDOWN_RETRY_SCAN_BATCH_V2 {
            return Err(instance_backend(
                RuntimeInteractionPersistenceErrorV1::InvalidInput,
            ));
        }
        let limit_i64 = i64::try_from(limit.get())
            .map_err(|_| instance_backend(RuntimeInteractionPersistenceErrorV1::InvalidInput))?;
        let after_guild_id = cursor
            .after()
            .map(|key| key.guild_id().to_string())
            .unwrap_or_default();
        let after_instance_id = cursor
            .after()
            .map(|key| key.instance_id().as_str())
            .unwrap_or_default();
        let through_guild_id = cursor
            .through()
            .map(|key| key.guild_id().to_string())
            .unwrap_or_default();
        let through_instance_id = cursor
            .through()
            .map(|key| key.instance_id().as_str())
            .unwrap_or_default();
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts)
            .await
            .map_err(instance_route_error)?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation)
            .await
            .map_err(instance_route_error)?;
        let rows =
            sqlx::query_as::<_, InstanceTeardownRetryScanRowV2>(INSTANCE_TEARDOWN_RETRY_SCAN_QUERY)
                .bind(after_guild_id)
                .bind(after_instance_id)
                .bind(through_guild_id)
                .bind(through_instance_id)
                .bind(limit_i64)
                .fetch_all(&mut *transaction)
                .await
                .map_err(|error| instance_route_error(map_query_error(&error)))?;
        if rows.len() > limit.get() {
            return Err(instance_corrupt());
        }
        let decoded = rows
            .into_iter()
            .map(InstanceTeardownRetryScanRowV2::decode)
            .collect::<Result<Vec<_>, _>>()
            .map_err(instance_backend)?;
        let through = decoded
            .first()
            .map(|(_, through)| through.clone())
            .or_else(|| cursor.through().cloned());
        if decoded.iter().any(|(_, row_through)| {
            through
                .as_ref()
                .is_none_or(|through| row_through != through)
        }) || cursor
            .through()
            .is_some_and(|expected| through.as_ref() != Some(expected))
        {
            return Err(instance_corrupt());
        }
        let keys = decoded.into_iter().map(|(key, _)| key).collect::<Vec<_>>();
        if keys.iter().any(|key| {
            cursor
                .after()
                .is_some_and(|after| key.cmp_c_v2(after).is_le())
        }) {
            return Err(instance_corrupt());
        }
        let page = InstanceTeardownRetryScanPageV2::new(keys, through, limit)
            .ok_or_else(instance_corrupt)?;
        transaction
            .commit()
            .await
            .map_err(|error| instance_route_error(map_query_error(&error)))?;
        Ok(page)
    }
}

impl PostgresRuntimeInteractionV1 {
    async fn teardown_mutation_outcome_v1(
        &self,
        query: &str,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<String, InstanceStoreError> {
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts)
            .await
            .map_err(instance_backend)?;
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation)
            .await
            .map_err(instance_backend)?;
        let rows = sqlx::query_as::<_, InstanceOutcomeRowV1>(query)
            .bind(guild_id.to_string())
            .bind(instance_id.as_str())
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| instance_backend(map_mutation_error(&error)))?;
        let [row] = rows.as_slice() else {
            return Err(instance_corrupt());
        };
        let outcome = row.outcome.clone();
        transaction
            .commit()
            .await
            .map_err(|error| instance_backend(map_mutation_commit_error(&error)))?;
        Ok(outcome)
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
        verify_runtime_interaction_binding_v1(&mut transaction, &self.expectation)
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
