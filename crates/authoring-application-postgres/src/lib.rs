mod authentication;
mod bindings;
mod digest;
mod envelope;
mod snapshot;

pub use authentication::{
    AuthenticationConfigError, PostgresAuthentication, PostgresAuthenticationConfig,
};
pub use digest::{
    digest_opaque_session_credential_v1, OpaqueSessionCredentialError, ProductSessionDigestV1,
};
pub use envelope::{
    build_snapshot_authenticated_data_v1, EncryptedSnapshotEnvelopeV1,
    SnapshotAuthenticatedDataError, SnapshotAuthenticatedDataInputV1, SnapshotAuthenticatedDataV1,
    SnapshotEnvelopeCipher, SnapshotEnvelopeCipherError,
};
pub use snapshot::{
    AuthorizedSnapshotConfigError, PostgresAuthorizedPromotionSnapshots,
    PostgresAuthorizedPromotionSnapshotsConfig,
};

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
