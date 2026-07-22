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
use sqlx::PgPool;

use crate::contract::{INSTANCE_REGISTER_QUERY, PINNED_READ_QUERY, ROUTE_READ_QUERY};
use crate::database::{
    begin_interaction_transaction, verify_runtime_interaction_database_with_timeouts_v1,
};
use crate::error::{map_mutation_commit_error, map_mutation_error, map_query_error};
use crate::row::{
    InstanceRowV1, PinnedInstanceRowErrorV1, PinnedInstanceRowOutcomeV1, PinnedInstanceRowV1,
};
use crate::{
    RuntimeInteractionDatabaseExpectationV1, RuntimeInteractionDatabaseReadinessV1,
    RuntimeInteractionDatabaseTimeoutsV1, RuntimeInteractionPersistenceErrorV1,
};

#[derive(Clone)]
pub struct PostgresRuntimeInteractionV1 {
    pool: PgPool,
    expectation: RuntimeInteractionDatabaseExpectationV1,
    timeouts: RuntimeInteractionDatabaseTimeoutsV1,
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
        let initial_readiness =
            verify_runtime_interaction_database_with_timeouts_v1(&pool, &expectation, timeouts)
                .await?;
        Ok(Self {
            pool,
            expectation,
            timeouts,
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
}

impl InstanceRouteReaderV1 for PostgresRuntimeInteractionV1 {
    async fn read_instance_route_v1(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        let mut transaction = begin_interaction_transaction(&self.pool, self.timeouts)
            .await
            .map_err(instance_backend)?;
        let rows = sqlx::query_as::<_, InstanceRowV1>(ROUTE_READ_QUERY)
            .bind(guild_id.to_string())
            .bind(instance_id.as_str())
            .fetch_all(&mut *transaction)
            .await
            .map_err(|error| instance_backend(map_query_error(&error)))?;
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
            .map_err(|error| instance_backend(map_query_error(&error)))?;
        Ok(instance)
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
