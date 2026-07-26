mod contract;
mod row;

use automation_runtime_controller::{
    RuntimeObservePreviousServingV1, RuntimePreviousServingObservationReceiptV1,
};
use serde_json::Value;
use sqlx::types::Json;

use crate::error::database;
use crate::row::runtime_i64;
use crate::{PostgresRuntimeConvergence, RuntimeConvergenceStoreError};

use self::contract::OBSERVE_PREVIOUS_SERVING_QUERY;
use self::row::PreviousServingObservationRow;

impl PostgresRuntimeConvergence {
    pub(crate) async fn observe_previous_serving_capability(
        &self,
        request: RuntimeObservePreviousServingV1,
    ) -> Result<RuntimePreviousServingObservationReceiptV1, RuntimeConvergenceStoreError> {
        let previous_runtime = request
            .expected_previous_runtime
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|_| RuntimeConvergenceStoreError::InvalidInput("previous runtime"))?
            .map(Json::<Value>);
        let mut transaction = self.begin().await?;
        let rows =
            sqlx::query_as::<_, PreviousServingObservationRow>(OBSERVE_PREVIOUS_SERVING_QUERY)
                .bind(request.guard.scope.tenant_id.as_str())
                .bind(request.guard.scope.installation_id.as_str())
                .bind(request.guard.scope.deployment_id.as_str())
                .bind(runtime_i64(request.guard.expected_revision.get())?)
                .bind(request.guard.controller_id.as_str())
                .bind(runtime_i64(request.guard.fencing_token.get())?)
                .bind(i64::from(request.guard.convergence_attempt.get()))
                .bind(runtime_i64(request.guard.runtime_generation.get())?)
                .bind(request.expected_target.guild_id.to_string())
                .bind(request.expected_target.ruleset_key.as_str())
                .bind(i64::from(request.expected_target.version.get()))
                .bind(request.expected_target.content_hash.to_hex())
                .bind(runtime_i64(request.expected_target.binding_revision.get())?)
                .bind(request.expected_target.binding_fingerprint.as_str())
                .bind(previous_runtime)
                .fetch_all(&mut *transaction)
                .await
                .map_err(database)?;
        if rows.len() != 1 {
            transaction.commit().await.map_err(database)?;
            return if rows.is_empty() {
                Err(RuntimeConvergenceStoreError::ExecutionClaimStale)
            } else {
                Err(RuntimeConvergenceStoreError::InvalidPersistedState(
                    "runtime previous serving observation cardinality",
                ))
            };
        }
        let row =
            rows.into_iter()
                .next()
                .ok_or(RuntimeConvergenceStoreError::InvalidPersistedState(
                    "runtime previous serving observation cardinality",
                ))?;
        let receipt = row.decode(request)?;
        transaction.commit().await.map_err(database)?;
        Ok(receipt)
    }
}
