mod authentication;
mod bindings;
mod database;
mod database_capability;
mod deployment_status;
mod digest;
mod envelope;
mod installation_authority;
mod product_action_digest;
mod product_decisions;
mod product_identity;
mod product_promotions;
mod product_retention;
mod runtime_convergence_readiness;
mod secret;
mod snapshot;

pub use authentication::{
    AuthenticationConfigError, AuthenticationReadinessErrorV1, PostgresAuthentication,
    PostgresAuthenticationConfig,
};
pub use database::ProductDatabaseFailureV1;
pub use deployment_status::{
    PostgresProductDeploymentOperationalStatusesV2, PostgresProductDeploymentStatuses,
    PostgresProductDeploymentStatusesConfig, ProductDeploymentOperationalStatusReadinessErrorV2,
    ProductDeploymentStatusConfigError, ProductDeploymentStatusReadinessErrorV1,
};
pub use digest::{
    digest_opaque_session_credential_v1, OpaqueSessionCredentialError, ProductSessionDigestV1,
};
pub use envelope::{
    build_snapshot_authenticated_data_v1, EncryptedSnapshotEnvelopeV1,
    SnapshotAuthenticatedDataError, SnapshotAuthenticatedDataInputV1, SnapshotAuthenticatedDataV1,
    SnapshotEnvelopeCipher, SnapshotEnvelopeCipherError, SnapshotEnvelopeKeyError,
    SnapshotEnvelopeKeyV1, SnapshotEnvelopeKeyringError, SnapshotEnvelopeKeyringV1,
    XChaCha20Poly1305SnapshotEnvelopeCipherV1, XCHACHA20_POLY1305_SNAPSHOT_NONCE_BYTES_V1,
    XCHACHA20_POLY1305_SNAPSHOT_SUITE_V1, XCHACHA20_POLY1305_SNAPSHOT_SUITE_VERSION_V1,
};
pub use installation_authority::{
    InstallationAuthorityReadinessErrorV1, InstallationAuthoritySourceConfigError,
    PostgresInstallationAuthoritySource, PostgresInstallationAuthoritySourceConfig,
};
pub use product_action_digest::{
    ProductActionDigestKeyError, ProductActionDigestKeyV1, ProductActionDigestKeyringError,
    ProductActionDigestKeyringV1,
};
pub use product_decisions::{
    PostgresProductDecisions, PostgresProductDecisionsConfig, ProductDecisionConfigError,
    ProductDecisionDatabasePoolsV1, ProductDecisionDigestKeyError, ProductDecisionDigestKeyV1,
    ProductDecisionDigestKeyringV1, ProductDecisionReadinessErrorV1,
};
pub use product_identity::{
    ConsumedOAuthFlowV1, CurrentProductPrincipalV1, IssuedProductSessionV1, OAuthFlowError,
    OAuthFlowIssueV1, PostgresProductIdentityConfig, PostgresProductIdentityStore,
    ProductIdentityConfigError, ProductIdentityDatabasePoolsV1, ProductIdentityError,
    ProductIdentityLifetimesV1, ProductIdentityReadinessErrorV1, ProductLogoutDispositionV1,
    ProductSessionRevocationReasonV1,
};
pub use product_promotions::{
    PostgresProductPromotions, PostgresProductPromotionsConfig, ProductPromotionConfigError,
    ProductPromotionReadinessErrorV1,
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
    AuthorizedSnapshotConfigError, AuthorizedSnapshotReadinessErrorV1,
    PostgresAuthorizedPromotionSnapshots, PostgresAuthorizedPromotionSnapshotsConfig,
};

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
