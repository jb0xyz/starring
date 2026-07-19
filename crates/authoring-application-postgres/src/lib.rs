mod authentication;
mod bindings;
mod database;
mod deployment_status;
mod digest;
mod envelope;
mod installation_authority;
mod product_decisions;
mod product_identity;
mod product_retention;
mod secret;
mod snapshot;

pub use authentication::{
    AuthenticationConfigError, PostgresAuthentication, PostgresAuthenticationConfig,
};
pub use database::ProductDatabaseFailureV1;
pub use deployment_status::PostgresProductDeploymentStatuses;
pub use digest::{
    digest_opaque_session_credential_v1, OpaqueSessionCredentialError, ProductSessionDigestV1,
};
pub use envelope::{
    build_snapshot_authenticated_data_v1, EncryptedSnapshotEnvelopeV1,
    SnapshotAuthenticatedDataError, SnapshotAuthenticatedDataInputV1, SnapshotAuthenticatedDataV1,
    SnapshotEnvelopeCipher, SnapshotEnvelopeCipherError,
};
pub use installation_authority::{
    InstallationAuthorityReadinessErrorV1, InstallationAuthoritySourceConfigError,
    PostgresInstallationAuthoritySource, PostgresInstallationAuthoritySourceConfig,
};
pub use product_decisions::{
    PostgresProductDecisions, PostgresProductDecisionsConfig, ProductDecisionConfigError,
    ProductDecisionDigestKeyError, ProductDecisionDigestKeyV1, ProductDecisionDigestKeyringV1,
    ProductDecisionReadinessErrorV1,
};
pub use product_identity::{
    ConsumedOAuthFlowV1, CurrentProductPrincipalV1, IssuedProductSessionV1, OAuthFlowError,
    OAuthFlowIssueV1, PostgresProductIdentityConfig, PostgresProductIdentityStore,
    ProductIdentityConfigError, ProductIdentityError, ProductIdentityLifetimesV1,
    ProductLogoutDispositionV1, ProductSessionRevocationReasonV1,
};
pub use product_retention::{
    PostgresProductActionRetention, PostgresProductActionRetentionConfig,
    PostgresProductIdentityRetention, PostgresProductIdentityRetentionConfig,
    ProductActionRetentionConfigError, ProductActionRetentionError, ProductActionRetentionReportV1,
    ProductIdentityRetentionConfigError, ProductIdentityRetentionError,
    ProductIdentityRetentionReportV1,
};
pub use secret::{
    OperatingSystemSecretGenerator, ProductSecretGenerator, ProductSecretGeneratorError,
    ProductSecretV1,
};
pub use snapshot::{
    AuthorizedSnapshotConfigError, PostgresAuthorizedPromotionSnapshots,
    PostgresAuthorizedPromotionSnapshotsConfig,
};

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
