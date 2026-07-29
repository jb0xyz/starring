mod command;
mod keychain;
mod postgres;

use thiserror::Error;

pub use command::{AuthorityAdvanceCommandV1, AuthorityAdvanceCommandValuesV1};
pub use postgres::{advance_authority, AuthorityAdvanceOutcomeV1, AuthorityAdvanceReportV1};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum AuthorityOperatorErrorV1 {
    #[error("unsupported_platform")]
    UnsupportedPlatform,
    #[error("postgres_environment_not_allowed")]
    PostgresEnvironment,
    #[error("command_line_arguments_not_allowed")]
    CommandLineArguments,
    #[error("staging_authority_acknowledgement_failed")]
    Acknowledgement,
    #[error("keychain_read_failed")]
    KeychainRead,
    #[error("keychain_timeout")]
    KeychainTimeout,
    #[error("database_url_shape_failed")]
    DatabaseUrlShape,
    #[error("database_connection_failed")]
    DatabaseConnection,
    #[error("cluster_contract_failed")]
    ClusterContract,
    #[error("authority_precondition_failed")]
    AuthorityPrecondition,
    #[error("authority_input_conflicts")]
    AuthorityConflict,
    #[error("authority_database_mutation_failed")]
    DatabaseMutation,
    #[error("authority_database_busy")]
    DatabaseBusy,
    #[error("authority_commit_indeterminate")]
    CommitIndeterminate,
    #[error("authority_post_verification_failed")]
    Verification,
}

impl AuthorityOperatorErrorV1 {
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedPlatform => "unsupported_platform",
            Self::PostgresEnvironment => "postgres_environment_not_allowed",
            Self::CommandLineArguments => "command_line_arguments_not_allowed",
            Self::Acknowledgement => "staging_authority_acknowledgement_failed",
            Self::KeychainRead => "keychain_read_failed",
            Self::KeychainTimeout => "keychain_timeout",
            Self::DatabaseUrlShape => "database_url_shape_failed",
            Self::DatabaseConnection => "database_connection_failed",
            Self::ClusterContract => "cluster_contract_failed",
            Self::AuthorityPrecondition => "authority_precondition_failed",
            Self::AuthorityConflict => "authority_input_conflicts",
            Self::DatabaseMutation => "authority_database_mutation_failed",
            Self::DatabaseBusy => "authority_database_busy",
            Self::CommitIndeterminate => "authority_commit_indeterminate",
            Self::Verification => "authority_post_verification_failed",
        }
    }
}

pub fn postgres_environment_is_present() -> bool {
    std::env::vars_os().any(|(name, _)| name.to_string_lossy().starts_with("PG"))
}
