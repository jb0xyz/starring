mod query;
mod row;

use std::future::Future;

use automation_runtime_controller::{
    RuntimeCertificationIntentReservationOutcomeV2, RuntimeCertificationReservationScopeLookupV2,
    RuntimeCertificationReservationScopeObservationV2, RuntimeReservedCertificationIntentV2,
};
use automation_runtime_worker::RuntimeCertificationReservationPortV2;
use sqlx::PgConnection;

use self::query::{
    OBSERVE_CERTIFICATION_RESERVATION_SCOPE_QUERY, RESERVE_CERTIFICATION_INTENT_QUERY,
};
use self::row::RuntimeCertificationReservationRowV2;
use crate::certification::MAX_RUNTIME_CERTIFICATION_SERVING_LEASE_DURATION;
use crate::connection::ExecutionConnectionGuardV1;
use crate::database::{
    begin_execution_locked_observation_transaction, begin_execution_mutation_transaction,
    verify_runtime_execution_binding_v1,
};
use crate::error::{map_mutation_commit_error, map_query_error, validate_millisecond_duration};
use crate::{PostgresRuntimeExecutionV1, RuntimeExecutionPersistenceErrorV1};

impl PostgresRuntimeExecutionV1 {
    async fn reserve_certification_intent_v2(
        &self,
        reservation: RuntimeReservedCertificationIntentV2,
    ) -> Result<RuntimeCertificationIntentReservationOutcomeV2, RuntimeExecutionPersistenceErrorV1>
    {
        let bindings = RuntimeCertificationReservationBindingsV2::from_reservation(&reservation)?;
        let deadline = tokio::time::Instant::now() + self.timeouts.statement_timeout();
        let connection = tokio::time::timeout_at(deadline, self.pool.acquire())
            .await
            .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)?
            .map_err(map_query_error)?;
        let mut connection = ExecutionConnectionGuardV1::new(connection);
        let database_connection = connection
            .connection_mut()
            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        let result = tokio::time::timeout_at(
            deadline,
            self.reserve_certification_intent_on_connection_v2(
                database_connection,
                reservation,
                bindings,
            ),
        )
        .await;
        match result {
            Ok(result) => {
                connection.release_to_pool();
                result
            }
            Err(_) => Err(RuntimeExecutionPersistenceErrorV1::Indeterminate),
        }
    }

    async fn reserve_certification_intent_on_connection_v2(
        &self,
        connection: &mut PgConnection,
        reservation: RuntimeReservedCertificationIntentV2,
        bindings: RuntimeCertificationReservationBindingsV2,
    ) -> Result<RuntimeCertificationIntentReservationOutcomeV2, RuntimeExecutionPersistenceErrorV1>
    {
        let mut transaction =
            begin_execution_mutation_transaction(connection, self.timeouts).await?;
        verify_runtime_execution_binding_v1(&mut transaction, &self.expectation).await?;
        let intent = reservation.canonical_intent().intent();
        let guard = &intent.guard;
        let target = &intent.target;
        let mut rows = sqlx::query_as::<_, RuntimeCertificationReservationRowV2>(
            RESERVE_CERTIFICATION_INTENT_QUERY,
        )
        .bind(bindings.action_id)
        .bind(intent.operation_id.as_str())
        .bind(guard.scope.tenant_id.as_str())
        .bind(guard.scope.installation_id.as_str())
        .bind(guard.scope.deployment_id.as_str())
        .bind(bindings.expected_revision)
        .bind(guard.controller_id.as_str())
        .bind(bindings.fencing_token)
        .bind(bindings.runtime_generation)
        .bind(bindings.convergence_attempt)
        .bind(target.guild_id.to_string())
        .bind(target.ruleset_key.as_str())
        .bind(bindings.target_version)
        .bind(target.content_hash.to_hex())
        .bind(bindings.binding_revision)
        .bind(target.binding_fingerprint.as_str())
        .bind(bindings.installation_authority_revision)
        .bind(intent.process_identity.process_instance_id.as_str())
        .bind(intent.gateway_owner_lease_id.gateway_shard_id.as_str())
        .bind(bindings.gateway_lease_epoch)
        .bind(bindings.gateway_owner_revision)
        .bind(intent.runtime_build_revision.as_str())
        .bind(intent.panel.certificate_id.as_str())
        .bind(intent.panel.report_digest.as_str())
        .bind(bindings.serving_lease_milliseconds)
        .bind(reservation.certification_intent_bytes())
        .bind(reservation.intent_fingerprint().as_str())
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_query_error)?;
        if rows.len() != 1 {
            return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
        }
        let outcome = rows
            .pop()
            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?
            .decode_reservation(reservation)?;
        transaction
            .commit()
            .await
            .map_err(map_mutation_commit_error)?;
        Ok(outcome)
    }

    async fn observe_certification_reservation_scope_v2(
        &self,
        lookup: RuntimeCertificationReservationScopeLookupV2,
    ) -> Result<RuntimeCertificationReservationScopeObservationV2, RuntimeExecutionPersistenceErrorV1>
    {
        let bindings = RuntimeCertificationReservationScopeBindingsV2::from_lookup(&lookup)?;
        let deadline = tokio::time::Instant::now() + self.timeouts.statement_timeout();
        let connection = tokio::time::timeout_at(deadline, self.pool.acquire())
            .await
            .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)?
            .map_err(map_query_error)?;
        let mut connection = ExecutionConnectionGuardV1::new(connection);
        let database_connection = connection
            .connection_mut()
            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
        let result = tokio::time::timeout_at(
            deadline,
            self.observe_certification_reservation_scope_on_connection_v2(
                database_connection,
                lookup,
                bindings,
            ),
        )
        .await;
        match result {
            Ok(result) => {
                connection.release_to_pool();
                result
            }
            Err(_) => Err(RuntimeExecutionPersistenceErrorV1::Timeout),
        }
    }

    async fn observe_certification_reservation_scope_on_connection_v2(
        &self,
        connection: &mut PgConnection,
        lookup: RuntimeCertificationReservationScopeLookupV2,
        bindings: RuntimeCertificationReservationScopeBindingsV2,
    ) -> Result<RuntimeCertificationReservationScopeObservationV2, RuntimeExecutionPersistenceErrorV1>
    {
        let mut transaction =
            begin_execution_locked_observation_transaction(connection, self.timeouts).await?;
        verify_runtime_execution_binding_v1(&mut transaction, &self.expectation).await?;
        let scope = lookup.operation_scope();
        let mut rows = sqlx::query_as::<_, RuntimeCertificationReservationRowV2>(
            OBSERVE_CERTIFICATION_RESERVATION_SCOPE_QUERY,
        )
        .bind(scope.scope().tenant_id.as_str())
        .bind(scope.scope().installation_id.as_str())
        .bind(scope.scope().deployment_id.as_str())
        .bind(bindings.expected_revision)
        .bind(bindings.convergence_attempt)
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_query_error)?;
        if rows.len() != 1 {
            return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
        }
        let observation = rows
            .pop()
            .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?
            .decode_observation(lookup)?;
        transaction.commit().await.map_err(map_query_error)?;
        Ok(observation)
    }
}

impl RuntimeCertificationReservationPortV2 for PostgresRuntimeExecutionV1 {
    type Error = RuntimeExecutionPersistenceErrorV1;

    fn reserve_certification_intent(
        &self,
        reservation: RuntimeReservedCertificationIntentV2,
    ) -> impl Future<Output = Result<RuntimeCertificationIntentReservationOutcomeV2, Self::Error>> + Send
    {
        self.reserve_certification_intent_v2(reservation)
    }

    fn observe_certification_reservation_scope(
        &self,
        lookup: RuntimeCertificationReservationScopeLookupV2,
    ) -> impl Future<Output = Result<RuntimeCertificationReservationScopeObservationV2, Self::Error>>
           + Send {
        self.observe_certification_reservation_scope_v2(lookup)
    }
}

#[derive(Clone, Copy)]
struct RuntimeCertificationReservationBindingsV2 {
    action_id: i64,
    expected_revision: i64,
    fencing_token: i64,
    runtime_generation: i64,
    convergence_attempt: i64,
    target_version: i64,
    binding_revision: i64,
    installation_authority_revision: i64,
    gateway_lease_epoch: i64,
    gateway_owner_revision: i64,
    serving_lease_milliseconds: i64,
}

impl RuntimeCertificationReservationBindingsV2 {
    fn from_reservation(
        reservation: &RuntimeReservedCertificationIntentV2,
    ) -> Result<Self, RuntimeExecutionPersistenceErrorV1> {
        let intent = reservation.canonical_intent().intent();
        let guard = &intent.guard;
        if reservation.operation_scope().scope() != &guard.scope
            || reservation.operation_scope().deployment_revision() != guard.expected_revision
            || reservation.operation_scope().convergence_attempt() != guard.convergence_attempt
        {
            return Err(RuntimeExecutionPersistenceErrorV1::InvalidInput);
        }
        Ok(Self {
            action_id: positive_i64(intent.action_id.get())?,
            expected_revision: positive_i64(guard.expected_revision.get())?,
            fencing_token: positive_i64(guard.fencing_token.get())?,
            runtime_generation: positive_i64(guard.runtime_generation.get())?,
            convergence_attempt: i64::from(guard.convergence_attempt.get()),
            target_version: i64::from(intent.target.version.get()),
            binding_revision: positive_i64(intent.target.binding_revision.get())?,
            installation_authority_revision: positive_i64(
                intent.binding_pin.installation_authority_revision.get(),
            )?,
            gateway_lease_epoch: positive_i64(intent.gateway_owner_lease_id.lease_epoch.get())?,
            gateway_owner_revision: positive_i64(intent.observed_owner_revision.get())?,
            serving_lease_milliseconds: validate_millisecond_duration(
                intent.serving_lease_for,
                MAX_RUNTIME_CERTIFICATION_SERVING_LEASE_DURATION,
            )?,
        })
    }
}

#[derive(Clone, Copy)]
struct RuntimeCertificationReservationScopeBindingsV2 {
    expected_revision: i64,
    convergence_attempt: i64,
}

impl RuntimeCertificationReservationScopeBindingsV2 {
    fn from_lookup(
        lookup: &RuntimeCertificationReservationScopeLookupV2,
    ) -> Result<Self, RuntimeExecutionPersistenceErrorV1> {
        Ok(Self {
            expected_revision: positive_i64(lookup.operation_scope().deployment_revision().get())?,
            convergence_attempt: i64::from(lookup.operation_scope().convergence_attempt().get()),
        })
    }
}

fn positive_i64(value: u64) -> Result<i64, RuntimeExecutionPersistenceErrorV1> {
    let value =
        i64::try_from(value).map_err(|_| RuntimeExecutionPersistenceErrorV1::InvalidInput)?;
    if value > 0 {
        Ok(value)
    } else {
        Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_postgres_integer_binding_is_checked() {
        let maximum = u64::try_from(i64::MAX).unwrap();
        assert_eq!(positive_i64(1), Ok(1));
        assert_eq!(positive_i64(maximum), Ok(i64::MAX));
        assert_eq!(
            positive_i64(0),
            Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
        );
        assert_eq!(
            positive_i64(maximum.checked_add(1).unwrap()),
            Err(RuntimeExecutionPersistenceErrorV1::InvalidInput)
        );
    }
}
