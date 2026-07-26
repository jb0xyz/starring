use std::time::Duration;

use automation_runtime_controller::RuntimeExecutionGuardV1;
use automation_runtime_convergence_postgres::RuntimeExactTargetV1;
use sqlx::PgPool;

use crate::{
    verify_runtime_panel_database_with_timeouts_v1, PostgresFencedStrictPanelStoreV1,
    RuntimePanelDatabaseExpectationV1, RuntimePanelDatabaseReadinessV1,
    RuntimePanelDatabaseTimeoutsV1, RuntimePanelPersistenceErrorV1, RuntimePanelSessionIdV1,
};

#[derive(Clone)]
pub struct PostgresRuntimePanelV1 {
    pool: PgPool,
    expectation: RuntimePanelDatabaseExpectationV1,
    timeouts: RuntimePanelDatabaseTimeoutsV1,
    initial_readiness: RuntimePanelDatabaseReadinessV1,
}

impl PostgresRuntimePanelV1 {
    pub async fn connect_verified(
        pool: PgPool,
        expectation: RuntimePanelDatabaseExpectationV1,
        timeouts: RuntimePanelDatabaseTimeoutsV1,
    ) -> Result<Self, RuntimePanelPersistenceErrorV1> {
        let initial_readiness =
            verify_runtime_panel_database_with_timeouts_v1(&pool, &expectation, timeouts).await?;
        Ok(Self {
            pool,
            expectation,
            timeouts,
            initial_readiness,
        })
    }

    pub async fn connect_verified_default(
        pool: PgPool,
        expectation: RuntimePanelDatabaseExpectationV1,
    ) -> Result<Self, RuntimePanelPersistenceErrorV1> {
        Self::connect_verified(pool, expectation, RuntimePanelDatabaseTimeoutsV1::default()).await
    }

    pub fn initial_readiness(&self) -> &RuntimePanelDatabaseReadinessV1 {
        &self.initial_readiness
    }

    pub async fn verify_database_v1(
        &self,
    ) -> Result<RuntimePanelDatabaseReadinessV1, RuntimePanelPersistenceErrorV1> {
        verify_runtime_panel_database_with_timeouts_v1(&self.pool, &self.expectation, self.timeouts)
            .await
    }

    pub async fn claim(
        &self,
        guard: RuntimeExecutionGuardV1,
        exact_target: RuntimeExactTargetV1,
        side_effect_headroom: Duration,
    ) -> Result<PostgresFencedStrictPanelStoreV1, RuntimePanelPersistenceErrorV1> {
        PostgresFencedStrictPanelStoreV1::claim_verified_with_timeouts(
            self.pool.clone(),
            self.expectation.clone(),
            guard,
            exact_target,
            side_effect_headroom,
            self.timeouts,
        )
        .await
    }

    pub async fn claim_with_session_id(
        &self,
        guard: RuntimeExecutionGuardV1,
        exact_target: RuntimeExactTargetV1,
        side_effect_headroom: Duration,
        session_id: &RuntimePanelSessionIdV1,
    ) -> Result<PostgresFencedStrictPanelStoreV1, RuntimePanelPersistenceErrorV1> {
        PostgresFencedStrictPanelStoreV1::claim_verified_with_session_id_and_timeouts(
            self.pool.clone(),
            self.expectation.clone(),
            guard,
            exact_target,
            side_effect_headroom,
            session_id,
            self.timeouts,
        )
        .await
    }
}
