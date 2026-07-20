use automation_runtime_convergence::{
    FencingToken, RecoverBlockedRequestV1, RuntimeDeploymentError,
};

use crate::error::database;
use crate::model::{
    ClaimExecutionReceiptV1, RecoverBlockedDeploymentV1, RuntimeConvergenceAttemptV1,
};
use crate::{PostgresRuntimeConvergence, RuntimeConvergenceStoreError};

use super::{attempt, DeploymentExecutionProjection};

impl PostgresRuntimeConvergence {
    pub async fn recover_blocked_for_operator(
        &self,
        request: RecoverBlockedDeploymentV1,
    ) -> Result<ClaimExecutionReceiptV1, RuntimeConvergenceStoreError> {
        let RecoverBlockedDeploymentV1 {
            scope,
            expected_revision,
            expected_failure_id,
            expected_failure_attempt,
            controller_id,
            lease_for,
        } = request;
        let lease_duration = self.bounded_lease_duration(
            lease_for,
            self.config.maximum_controller_lease,
            "controller lease duration",
        )?;
        let mut transaction = self.begin().await?;
        let persisted = Self::load_scoped_for_update(&mut transaction, &scope).await?;
        Self::assert_current_deployment_authority(&mut transaction, &persisted).await?;
        let now = Self::mutation_now(&mut transaction).await?;
        if persisted.deployment.revision() != expected_revision {
            attempt::validate_blocked_recovery_replay(
                &persisted,
                &expected_failure_id,
                expected_failure_attempt,
            )?;
            let replayed_claim = expected_revision
                .next()
                .is_ok_and(|revision| revision == persisted.deployment.revision())
                && persisted
                    .deployment
                    .controller_lease()
                    .is_some_and(|lease| {
                        lease.controller_id == controller_id && lease.expires_at > now
                    });
            if !replayed_claim {
                return Err(RuntimeConvergenceStoreError::RevisionConflict);
            }
            let lease = persisted.deployment.controller_lease().ok_or(
                RuntimeConvergenceStoreError::InvalidPersistedState(
                    "replayed operator recovery lease",
                ),
            )?;
            let convergence_attempt = persisted.exact_convergence_attempt()?.started().ok_or(
                RuntimeConvergenceStoreError::InvalidPersistedState(
                    "replayed operator recovery attempt",
                ),
            )?;
            let receipt = ClaimExecutionReceiptV1 {
                snapshot: persisted.deployment.snapshot(),
                controller_id: lease.controller_id.clone(),
                fencing_token: lease.fencing_token,
                convergence_attempt,
                acquired_at: lease.acquired_at,
                expires_at: lease.expires_at,
            };
            transaction.commit().await.map_err(database)?;
            return Ok(receipt);
        }
        let convergence_attempt = attempt::next_blocked_recovery_attempt(
            &persisted,
            &expected_failure_id,
            expected_failure_attempt,
        )?;
        let previous_revision = persisted.deployment.revision();
        let next_fencing_value = persisted
            .deployment
            .snapshot()
            .last_fencing_token
            .map(|token| token.get().checked_add(1))
            .unwrap_or(Some(1))
            .ok_or(RuntimeDeploymentError::FencingTokenNotMonotonic)?;
        let fencing_token = FencingToken::new(next_fencing_value)
            .map_err(|_| RuntimeDeploymentError::FencingTokenNotMonotonic)?;
        let expires_at = now.checked_add_signed(lease_duration).ok_or(
            RuntimeConvergenceStoreError::InvalidInput("controller lease expiry overflow"),
        )?;
        let mut persisted = persisted;
        persisted
            .deployment
            .recover_blocked(RecoverBlockedRequestV1 {
                expected_revision: previous_revision,
                expected_failure_id,
                controller_id: controller_id.clone(),
                fencing_token,
                now,
                expires_at,
            })?;
        Self::persist_deployment(
            &mut transaction,
            &scope,
            previous_revision.get(),
            &persisted.deployment,
            DeploymentExecutionProjection {
                live_attestation_id: persisted.live_attestation_id.as_ref().map(|id| id.as_str()),
                convergence_attempt: RuntimeConvergenceAttemptV1::from(convergence_attempt),
                last_failure_attempt: persisted.last_failure_attempt,
            },
            now,
        )
        .await?;
        transaction.commit().await.map_err(database)?;
        Ok(ClaimExecutionReceiptV1 {
            snapshot: persisted.deployment.snapshot(),
            controller_id,
            fencing_token,
            convergence_attempt,
            acquired_at: now,
            expires_at,
        })
    }
}
