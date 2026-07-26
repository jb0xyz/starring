use sqlx::{Connection, PgPool};

use crate::connection::ExecutionConnectionGuardV1;
use crate::database::{
    configure_read_transaction, verify_database_authority, RuntimeExecutionDatabaseBindingRowV1,
    RuntimeExecutionDatabaseExpectationV1, RuntimeExecutionDatabaseTimeoutsV1,
};
use crate::error::map_readiness_query_error;
use crate::query::DATABASE_BINDING_QUERY;
use crate::RuntimeExecutionPersistenceErrorV1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeExecutionDatabaseIdentityObservationV1 {
    database_identity: String,
}

impl RuntimeExecutionDatabaseIdentityObservationV1 {
    pub fn database_identity(&self) -> &str {
        &self.database_identity
    }
}

pub async fn observe_runtime_execution_database_identity_v1(
    pool: &PgPool,
    expected_database_name: &str,
    expected_executor_role: &str,
) -> Result<RuntimeExecutionDatabaseIdentityObservationV1, RuntimeExecutionPersistenceErrorV1> {
    observe_runtime_execution_database_identity_with_timeouts_v1(
        pool,
        expected_database_name,
        expected_executor_role,
        RuntimeExecutionDatabaseTimeoutsV1::default(),
    )
    .await
}

pub async fn observe_runtime_execution_database_identity_with_timeouts_v1(
    pool: &PgPool,
    expected_database_name: &str,
    expected_executor_role: &str,
    timeouts: RuntimeExecutionDatabaseTimeoutsV1,
) -> Result<RuntimeExecutionDatabaseIdentityObservationV1, RuntimeExecutionPersistenceErrorV1> {
    let deadline = tokio::time::Instant::now() + timeouts.statement_timeout();
    let connection = tokio::time::timeout_at(deadline, pool.acquire())
        .await
        .map_err(|_| RuntimeExecutionPersistenceErrorV1::Timeout)?
        .map_err(map_readiness_query_error)?;
    let mut connection = ExecutionConnectionGuardV1::new(connection);
    let database_connection = connection
        .connection_mut()
        .ok_or(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt)?;
    let result = tokio::time::timeout_at(
        deadline,
        identify_on_connection(
            database_connection,
            expected_database_name,
            expected_executor_role,
            timeouts,
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

async fn identify_on_connection(
    connection: &mut sqlx::PgConnection,
    expected_database_name: &str,
    expected_executor_role: &str,
    timeouts: RuntimeExecutionDatabaseTimeoutsV1,
) -> Result<RuntimeExecutionDatabaseIdentityObservationV1, RuntimeExecutionPersistenceErrorV1> {
    let mut transaction = connection
        .begin()
        .await
        .map_err(map_readiness_query_error)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(map_readiness_query_error)?;
    configure_read_transaction(&mut transaction, timeouts).await?;
    let rows = sqlx::query_as::<_, RuntimeExecutionDatabaseBindingRowV1>(DATABASE_BINDING_QUERY)
        .fetch_all(&mut *transaction)
        .await
        .map_err(map_readiness_query_error)?;
    let [row] = rows.as_slice() else {
        return Err(RuntimeExecutionPersistenceErrorV1::PersistenceCorrupt);
    };
    let binding = decode_binding(row, expected_database_name, expected_executor_role)?;
    transaction
        .commit()
        .await
        .map_err(map_readiness_query_error)?;
    Ok(binding)
}

fn decode_binding(
    row: &RuntimeExecutionDatabaseBindingRowV1,
    expected_database_name: &str,
    expected_executor_role: &str,
) -> Result<RuntimeExecutionDatabaseIdentityObservationV1, RuntimeExecutionPersistenceErrorV1> {
    let expectation = RuntimeExecutionDatabaseExpectationV1::new(
        row.database_identity.clone(),
        expected_database_name,
        expected_executor_role,
    )?;
    verify_database_authority(
        &row.database_identity,
        &row.database_name,
        &row.executor_role,
        &expectation,
    )?;
    Ok(RuntimeExecutionDatabaseIdentityObservationV1 {
        database_identity: expectation.database_identity().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        database_identity: &str,
        database_name: &str,
        executor_role: &str,
    ) -> RuntimeExecutionDatabaseBindingRowV1 {
        RuntimeExecutionDatabaseBindingRowV1 {
            database_identity: database_identity.to_string(),
            database_name: database_name.to_string(),
            executor_role: executor_role.to_string(),
        }
    }

    #[test]
    fn identity_observation_requires_trusted_database_and_role() {
        let binding = decode_binding(
            &row(
                "01234567-89ab-cdef-8123-456789abcdef",
                "starring_runtime",
                "starring_runtime_execution",
            ),
            "starring_runtime",
            "starring_runtime_execution",
        )
        .unwrap();
        assert_eq!(
            binding.database_identity(),
            "01234567-89ab-cdef-8123-456789abcdef"
        );
        for invalid in [
            row(
                "00000000-0000-0000-0000-000000000000",
                "starring_runtime",
                "starring_runtime_execution",
            ),
            row(
                "01234567-89AB-cdef-8123-456789abcdef",
                "starring_runtime",
                "starring_runtime_execution",
            ),
            row(
                "01234567-89ab-cdef-8123-456789abcdef",
                "starring-runtime",
                "starring_runtime_execution",
            ),
            row(
                "01234567-89ab-cdef-8123-456789abcdef",
                "starring_runtime",
                "starring-runtime-execution",
            ),
        ] {
            assert_eq!(
                decode_binding(&invalid, "starring_runtime", "starring_runtime_execution",),
                Err(RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch)
            );
        }
        for (database_name, executor_role) in [
            ("starring_wrong", "starring_runtime_execution"),
            ("starring_runtime", "starring_wrong"),
        ] {
            assert_eq!(
                decode_binding(
                    &row(
                        "01234567-89ab-cdef-8123-456789abcdef",
                        "starring_runtime",
                        "starring_runtime_execution",
                    ),
                    database_name,
                    executor_role,
                ),
                Err(RuntimeExecutionPersistenceErrorV1::DatabaseAuthorityMismatch)
            );
        }
    }

    #[test]
    fn identity_query_is_function_only_and_bounded() {
        assert!(DATABASE_BINDING_QUERY.contains("starring_runtime_execution_database_identity_v1"));
        for forbidden in [
            "runtime_deployments",
            "runtime_attestations",
            "runtime_serving_leases",
            "INSERT ",
            "UPDATE ",
            "DELETE ",
        ] {
            assert!(!DATABASE_BINDING_QUERY.contains(forbidden));
        }
    }
}
