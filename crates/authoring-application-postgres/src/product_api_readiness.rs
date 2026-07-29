use crate::database_capability::{verify_same_database_distinct_roles, ScopedDatabaseTopologyV1};
use crate::{
    AuthorizedSnapshotReadinessErrorV1, InstallationAuthorityReadinessErrorV1,
    PostgresAuthorizedPromotionSnapshots, PostgresInstallationAuthoritySource,
    PostgresProductControl, PostgresProductDeploymentOperationalStatusesV2,
    PostgresProductDeploymentStatuses, PostgresProductIdentityStore, PostgresProductPromotions,
    ProductDecisionReadinessErrorV1, ProductDeploymentOperationalStatusReadinessErrorV2,
    ProductDeploymentStatusReadinessErrorV1, ProductIdentityReadinessErrorV1,
    ProductPromotionReadinessErrorV1, SnapshotEnvelopeCipher,
};

#[derive(Debug, thiserror::Error)]
pub enum ProductApiReadinessErrorV1 {
    #[error("product API identity readiness failed")]
    Identity(#[source] ProductIdentityReadinessErrorV1),
    #[error("product API installation-authority readiness failed")]
    InstallationAuthority(#[source] InstallationAuthorityReadinessErrorV1),
    #[error("product API authorized-snapshot readiness failed")]
    AuthorizedSnapshot(#[source] AuthorizedSnapshotReadinessErrorV1),
    #[error("product API promotion readiness failed")]
    Promotion(#[source] ProductPromotionReadinessErrorV1),
    #[error("product API decision readiness failed")]
    Decision(#[source] ProductDecisionReadinessErrorV1),
    #[error("product API deployment-status readiness failed")]
    DeploymentStatus(#[source] ProductDeploymentStatusReadinessErrorV1),
    #[error("product API operational deployment-status readiness failed")]
    OperationalDeploymentStatus(#[source] ProductDeploymentOperationalStatusReadinessErrorV2),
    #[error("product API database topology is invalid")]
    TopologyMismatch,
}

pub struct PostgresProductApiReadiness<'a, G, C> {
    identity: &'a PostgresProductIdentityStore<G>,
    installation_authority: &'a PostgresInstallationAuthoritySource,
    authorized_snapshots: &'a PostgresAuthorizedPromotionSnapshots<C>,
    promotions: &'a PostgresProductPromotions,
    control: &'a PostgresProductControl,
    deployment_statuses: &'a PostgresProductDeploymentStatuses,
    operational_deployment_statuses: &'a PostgresProductDeploymentOperationalStatusesV2,
}

impl<'a, G, C> PostgresProductApiReadiness<'a, G, C> {
    pub fn new(
        identity: &'a PostgresProductIdentityStore<G>,
        installation_authority: &'a PostgresInstallationAuthoritySource,
        authorized_snapshots: &'a PostgresAuthorizedPromotionSnapshots<C>,
        promotions: &'a PostgresProductPromotions,
        control: &'a PostgresProductControl,
        deployment_statuses: &'a PostgresProductDeploymentStatuses,
        operational_deployment_statuses: &'a PostgresProductDeploymentOperationalStatusesV2,
    ) -> Self {
        Self {
            identity,
            installation_authority,
            authorized_snapshots,
            promotions,
            control,
            deployment_statuses,
            operational_deployment_statuses,
        }
    }
}

impl<G, C: SnapshotEnvelopeCipher> PostgresProductApiReadiness<'_, G, C> {
    pub async fn verify_readiness(&self) -> Result<(), ProductApiReadinessErrorV1> {
        let identity = self
            .identity
            .check_readiness()
            .await
            .map_err(ProductApiReadinessErrorV1::Identity)?;
        let installation_authority = self
            .installation_authority
            .check_readiness()
            .await
            .map_err(ProductApiReadinessErrorV1::InstallationAuthority)?;
        let authorized_snapshot = self
            .authorized_snapshots
            .check_readiness()
            .await
            .map_err(ProductApiReadinessErrorV1::AuthorizedSnapshot)?;
        let promotion = self
            .promotions
            .check_readiness()
            .await
            .map_err(ProductApiReadinessErrorV1::Promotion)?;
        let decisions = self
            .control
            .check_readiness()
            .await
            .map_err(ProductApiReadinessErrorV1::Decision)?;
        let deployment_status = self
            .deployment_statuses
            .check_readiness()
            .await
            .map_err(ProductApiReadinessErrorV1::DeploymentStatus)?;
        let operational_deployment_status = self
            .operational_deployment_statuses
            .check_readiness()
            .await
            .map_err(ProductApiReadinessErrorV1::OperationalDeploymentStatus)?;
        let topologies: [ScopedDatabaseTopologyV1; 14] = [
            identity[0].clone(),
            identity[1].clone(),
            identity[2].clone(),
            identity[3].clone(),
            installation_authority,
            authorized_snapshot,
            promotion,
            decisions[0].clone(),
            decisions[1].clone(),
            decisions[2].clone(),
            decisions[3].clone(),
            decisions[4].clone(),
            deployment_status,
            operational_deployment_status,
        ];
        verify_same_database_distinct_roles(&topologies)
            .map_err(|_| ProductApiReadinessErrorV1::TopologyMismatch)
    }
}
