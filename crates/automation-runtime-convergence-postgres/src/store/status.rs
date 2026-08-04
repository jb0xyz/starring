use automation_runtime_convergence::RuntimeDeploymentPhaseV1;

use crate::error::database;
use crate::model::{RuntimeDeploymentScopeV1, RuntimeDeploymentStatusV1};
use crate::projection::{project_status, CurrentAuthorityOutcome, StatusProjectionEvidence};
use crate::row::{ServingLeaseRow, SERVING_LEASE_COLUMNS};
use crate::status_attestation::StatusAttestationEvidence;
use crate::{PostgresRuntimeConvergence, RuntimeConvergenceStoreError};

impl PostgresRuntimeConvergence {
    pub async fn status(
        &self,
        scope: &RuntimeDeploymentScopeV1,
    ) -> Result<RuntimeDeploymentStatusV1, RuntimeConvergenceStoreError> {
        let mut transaction = self.begin_status().await?;
        let persisted = Self::load_scoped_for_share(&mut transaction, scope).await?;
        let snapshot = persisted.deployment.snapshot();
        let terminal = matches!(
            snapshot.phase,
            RuntimeDeploymentPhaseV1::Cancelled { .. }
                | RuntimeDeploymentPhaseV1::Superseded { .. }
        );
        let authority = if terminal {
            CurrentAuthorityOutcome::NotEvaluated
        } else {
            match Self::assert_current_deployment_authority(&mut transaction, &persisted).await {
                Ok(()) => CurrentAuthorityOutcome::Exact,
                Err(RuntimeConvergenceStoreError::ActiveTargetMismatch) => {
                    CurrentAuthorityOutcome::ActiveMismatch
                }
                Err(RuntimeConvergenceStoreError::BindingAuthorityMismatch) => {
                    CurrentAuthorityOutcome::BindingMismatch
                }
                Err(RuntimeConvergenceStoreError::ProductAuthorityInactive) => {
                    CurrentAuthorityOutcome::LifecycleInactive
                }
                Err(RuntimeConvergenceStoreError::ScopeMismatch) => {
                    CurrentAuthorityOutcome::ScopeMismatch
                }
                Err(error) => return Err(error),
            }
        };
        let should_load_live = authority == CurrentAuthorityOutcome::Exact
            && matches!(snapshot.phase, RuntimeDeploymentPhaseV1::Live);
        let attestation = if should_load_live {
            match persisted.live_attestation_id.as_ref() {
                Some(attestation_id) => {
                    Self::load_attestation(&mut transaction, scope, attestation_id, false).await?
                }
                None => None,
            }
        } else {
            None
        }
        .map(StatusAttestationEvidence::from_legacy)
        .transpose()?;
        let serving = if should_load_live && attestation.is_some() {
            sqlx::query_as::<_, ServingLeaseRow>(&format!(
                "SELECT {SERVING_LEASE_COLUMNS} FROM public.runtime_serving_leases \
                 WHERE guild_id = $1 AND ruleset_key = $2 FOR SHARE"
            ))
            .bind(snapshot.target.guild_id.to_string())
            .bind(snapshot.target.ruleset_key.as_str())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(database)?
        } else {
            None
        };
        let observed_at = Self::database_now(&mut transaction).await?;
        transaction.commit().await.map_err(database)?;
        project_status(
            scope,
            observed_at,
            StatusProjectionEvidence {
                persisted,
                authority,
                attestation,
                serving,
            },
        )
    }
}
