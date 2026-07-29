mod crypto;
mod final_verify;
mod identity;
mod keychain;
mod keyring;
mod postgres;

use thiserror::Error;

use crypto::GeneratedSecretsV1;
use keychain::KeychainClientV1;
use keyring::validate_keyring_pair;
use postgres::StagingPostgresSessionV1;

pub use final_verify::{verify_final, FinalVerificationReportV1};
pub use identity::{
    ADMIN_DATABASE_NAME, ADMIN_KEYCHAIN_ACCOUNT, ADMIN_KEYCHAIN_SERVICE,
    APPLICATION_DATABASE_IDENTITIES, CLUSTER_ADMIN_ROLE, DATABASE_HOST, DATABASE_NAME,
    DATABASE_PORT, OWNER_ROLE, PEER_SOCKET_DIRECTORY,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum ProvisionerErrorV1 {
    #[error("unsupported_platform")]
    UnsupportedPlatform,
    #[error("postgres_environment_not_allowed")]
    PostgresEnvironment,
    #[error("command_line_arguments_not_allowed")]
    CommandLineArguments,
    #[error("staging_acknowledgement_failed")]
    StagingAcknowledgement,
    #[error("identity_manifest_failed")]
    IdentityManifest,
    #[error("cryptographic_random_failed")]
    Random,
    #[error("scram_generation_failed")]
    Scram,
    #[error("discord_keychain_preflight_failed")]
    DiscordPreflight,
    #[error("keychain_read_failed")]
    KeychainRead,
    #[error("keychain_write_failed")]
    KeychainWrite,
    #[error("keychain_timeout")]
    KeychainTimeout,
    #[error("keychain_rollback_failed")]
    KeychainRollback,
    #[error("database_connection_failed")]
    DatabaseConnection,
    #[error("cluster_contract_failed")]
    ClusterContract,
    #[error("peer_contract_failed")]
    PeerContract,
    #[error("database_quiescence_failed")]
    DatabaseQuiescence,
    #[error("role_quarantine_failed")]
    RoleQuarantine,
    #[error("database_mutation_failed")]
    DatabaseMutation,
    #[error("database_verification_failed")]
    DatabaseVerification,
    #[error("database_commit_indeterminate")]
    DatabaseCommitIndeterminate,
    #[error("database_url_shape_failed")]
    DatabaseUrlShape,
    #[error("final_hba_contract_failed")]
    FinalHbaContract,
    #[error("final_role_contract_failed")]
    FinalRoleContract,
    #[error("final_connection_contract_failed")]
    FinalConnectionContract,
    #[error("keyring_contract_failed")]
    KeyringContract,
}

impl ProvisionerErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedPlatform => "unsupported_platform",
            Self::PostgresEnvironment => "postgres_environment_not_allowed",
            Self::CommandLineArguments => "command_line_arguments_not_allowed",
            Self::StagingAcknowledgement => "staging_acknowledgement_failed",
            Self::IdentityManifest => "identity_manifest_failed",
            Self::Random => "cryptographic_random_failed",
            Self::Scram => "scram_generation_failed",
            Self::DiscordPreflight => "discord_keychain_preflight_failed",
            Self::KeychainRead => "keychain_read_failed",
            Self::KeychainWrite => "keychain_write_failed",
            Self::KeychainTimeout => "keychain_timeout",
            Self::KeychainRollback => "keychain_rollback_failed",
            Self::DatabaseConnection => "database_connection_failed",
            Self::ClusterContract => "cluster_contract_failed",
            Self::PeerContract => "peer_contract_failed",
            Self::DatabaseQuiescence => "database_quiescence_failed",
            Self::RoleQuarantine => "role_quarantine_failed",
            Self::DatabaseMutation => "database_mutation_failed",
            Self::DatabaseVerification => "database_verification_failed",
            Self::DatabaseCommitIndeterminate => "database_commit_indeterminate",
            Self::DatabaseUrlShape => "database_url_shape_failed",
            Self::FinalHbaContract => "final_hba_contract_failed",
            Self::FinalRoleContract => "final_role_contract_failed",
            Self::FinalConnectionContract => "final_connection_contract_failed",
            Self::KeyringContract => "keyring_contract_failed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagingAcknowledgementV1 {
    system_identifier: String,
}

impl StagingAcknowledgementV1 {
    pub fn parse(
        system_identifier: &str,
        acknowledgement: &str,
    ) -> Result<Self, ProvisionerErrorV1> {
        if system_identifier.is_empty()
            || system_identifier.len() > 20
            || system_identifier.starts_with('0')
            || !system_identifier.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(ProvisionerErrorV1::StagingAcknowledgement);
        }
        let expected = format!(
            "starring-runtime-dedicated-staging-cluster-v2:{system_identifier}:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
        );
        if acknowledgement != expected {
            return Err(ProvisionerErrorV1::StagingAcknowledgement);
        }
        Ok(Self {
            system_identifier: system_identifier.to_owned(),
        })
    }

    pub fn system_identifier(&self) -> &str {
        &self.system_identifier
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProvisioningReportV1 {
    product_action_key_id: String,
    snapshot_envelope_key_id: String,
}

impl ProvisioningReportV1 {
    pub fn product_action_key_id(&self) -> &str {
        &self.product_action_key_id
    }

    pub fn snapshot_envelope_key_id(&self) -> &str {
        &self.snapshot_envelope_key_id
    }
}

pub async fn provision_staging(
    acknowledgement: StagingAcknowledgementV1,
) -> Result<ProvisioningReportV1, ProvisionerErrorV1> {
    let keychain = KeychainClientV1::new()?;
    keychain.preflight_discord()?;
    let database = StagingPostgresSessionV1::connect_and_preflight(&acknowledgement).await?;
    let secrets = GeneratedSecretsV1::generate()?;
    validate_keyring_pair(
        secrets.product_action_keyring_payload(),
        secrets.snapshot_envelope_keyring_payload(),
    )?;
    let keychain_items = secrets.keychain_items();
    let keychain_update = keychain.begin_update(&keychain_items)?;
    match database.apply_verifiers(&secrets).await {
        Ok(()) => keychain_update.commit(),
        Err(ProvisionerErrorV1::DatabaseCommitIndeterminate) => {
            keychain_update.commit();
            return Err(ProvisionerErrorV1::DatabaseCommitIndeterminate);
        }
        Err(error) => {
            keychain_update.rollback()?;
            return Err(error);
        }
    }
    Ok(ProvisioningReportV1 {
        product_action_key_id: secrets.product_action_key_id().to_owned(),
        snapshot_envelope_key_id: secrets.snapshot_envelope_key_id().to_owned(),
    })
}

pub fn postgres_environment_is_present() -> bool {
    std::env::vars_os().any(|(name, _)| name.to_string_lossy().starts_with("PG"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acknowledgement_requires_independent_exact_v2_contract() {
        let system_identifier = "7663763942264209752";
        let acknowledgement = format!(
            "starring-runtime-dedicated-staging-cluster-v2:{system_identifier}:starring_runtime_staging:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
        );
        assert_eq!(
            StagingAcknowledgementV1::parse(system_identifier, &acknowledgement)
                .unwrap()
                .system_identifier(),
            system_identifier
        );
        assert!(StagingAcknowledgementV1::parse("7663763942264209753", &acknowledgement).is_err());
        assert!(StagingAcknowledgementV1::parse(
            system_identifier,
            &acknowledgement.replace("cluster-wide", "database-only")
        )
        .is_err());
        assert!(StagingAcknowledgementV1::parse("0", &acknowledgement).is_err());
    }

    #[test]
    fn errors_render_only_stable_secret_free_codes() {
        for error in [
            ProvisionerErrorV1::KeychainRead,
            ProvisionerErrorV1::KeychainWrite,
            ProvisionerErrorV1::DatabaseMutation,
            ProvisionerErrorV1::DatabaseCommitIndeterminate,
        ] {
            assert_eq!(error.to_string(), error.code());
            assert!(!error.to_string().contains("postgresql://"));
            assert!(!error.to_string().contains("SCRAM-SHA-256"));
        }
    }
}
