use std::collections::BTreeSet;
use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgSslMode};
use zeroize::Zeroizing;

use crate::ProvisionerErrorV1;

pub const DATABASE_NAME: &str = "starring_runtime_staging";
pub const ADMIN_DATABASE_NAME: &str = "postgres";
pub const OWNER_ROLE: &str = "starring_owner";
pub const CLUSTER_ADMIN_ROLE: &str = "starring_cluster_admin";
pub const PEER_SOCKET_DIRECTORY: &str = "/private/tmp/starring-bootstrap";
pub const PEER_SYSTEM_USER: &str = "jungbogeon";
pub const DATABASE_HOST: &str = "127.0.0.1";
pub const DATABASE_PORT: u16 = 5432;
pub const API_KEYCHAIN_SERVICE: &str = "starring-api.staging";
pub const RUNTIME_KEYCHAIN_SERVICE: &str = "starring.runtime.staging";
pub const ADMIN_KEYCHAIN_SERVICE: &str = "starring.postgres.staging";
pub const ADMIN_KEYCHAIN_ACCOUNT: &str = "database.cluster-admin";
pub const PRODUCT_ACTION_KEYRING_ACCOUNT: &str = "keyring.product-action";
pub const SNAPSHOT_ENVELOPE_KEYRING_ACCOUNT: &str = "keyring.snapshot-envelope";
pub const INTERACTION_TOKEN_ENVELOPE_KEYRING_ACCOUNT: &str = "interaction.token-envelope-keyring";
pub const AUTHORING_WRITER_IDENTITY: DatabaseIdentityV1 = DatabaseIdentityV1 {
    service: API_KEYCHAIN_SERVICE,
    account: "database.authoring-session-writer",
    role: "starring_authoring_session_writer",
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DatabaseIdentityV1 {
    pub service: &'static str,
    pub account: &'static str,
    pub role: &'static str,
}

pub const APPLICATION_DATABASE_IDENTITIES: [DatabaseIdentityV1; 20] = [
    DatabaseIdentityV1 {
        service: API_KEYCHAIN_SERVICE,
        account: "database.oauth-flow-writer",
        role: "starring_identity_oauth",
    },
    DatabaseIdentityV1 {
        service: API_KEYCHAIN_SERVICE,
        account: "database.session-issuer",
        role: "starring_identity_issuer",
    },
    DatabaseIdentityV1 {
        service: API_KEYCHAIN_SERVICE,
        account: "database.session-api",
        role: "starring_identity_session",
    },
    DatabaseIdentityV1 {
        service: API_KEYCHAIN_SERVICE,
        account: "database.security-revoker",
        role: "starring_identity_security",
    },
    DatabaseIdentityV1 {
        service: API_KEYCHAIN_SERVICE,
        account: "database.installation-authority-reader",
        role: "starring_installation_authority_reader",
    },
    DatabaseIdentityV1 {
        service: API_KEYCHAIN_SERVICE,
        account: "database.authorized-snapshot-reader",
        role: "starring_authorized_snapshot_reader",
    },
    DatabaseIdentityV1 {
        service: API_KEYCHAIN_SERVICE,
        account: "database.promotion-executor",
        role: "starring_promotion_executor",
    },
    DatabaseIdentityV1 {
        service: API_KEYCHAIN_SERVICE,
        account: "database.decision-reader",
        role: "starring_decision_reader",
    },
    DatabaseIdentityV1 {
        service: API_KEYCHAIN_SERVICE,
        account: "database.approval-executor",
        role: "starring_decision_approval",
    },
    DatabaseIdentityV1 {
        service: API_KEYCHAIN_SERVICE,
        account: "database.rejection-executor",
        role: "starring_decision_rejection",
    },
    DatabaseIdentityV1 {
        service: API_KEYCHAIN_SERVICE,
        account: "database.apply-executor",
        role: "starring_decision_apply",
    },
    DatabaseIdentityV1 {
        service: API_KEYCHAIN_SERVICE,
        account: "database.cancellation-executor",
        role: "starring_decision_cancellation",
    },
    DatabaseIdentityV1 {
        service: API_KEYCHAIN_SERVICE,
        account: "database.deployment-status-reader",
        role: "starring_deployment_status_reader",
    },
    DatabaseIdentityV1 {
        service: API_KEYCHAIN_SERVICE,
        account: "database.operational-deployment-status-reader",
        role: "starring_operational_deployment_status_reader",
    },
    AUTHORING_WRITER_IDENTITY,
    DatabaseIdentityV1 {
        service: RUNTIME_KEYCHAIN_SERVICE,
        account: "database.execution",
        role: "starring_runtime_execution",
    },
    DatabaseIdentityV1 {
        service: RUNTIME_KEYCHAIN_SERVICE,
        account: "database.exact-target",
        role: "starring_runtime_exact_target",
    },
    DatabaseIdentityV1 {
        service: RUNTIME_KEYCHAIN_SERVICE,
        account: "database.panel",
        role: "starring_runtime_panel",
    },
    DatabaseIdentityV1 {
        service: RUNTIME_KEYCHAIN_SERVICE,
        account: "database.serving",
        role: "starring_runtime_serving",
    },
    DatabaseIdentityV1 {
        service: RUNTIME_KEYCHAIN_SERVICE,
        account: "database.interaction",
        role: "starring_runtime_interaction",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeychainIdentityV1 {
    pub service: &'static str,
    pub account: &'static str,
}

pub const DISCORD_PREFLIGHT_IDENTITIES: [KeychainIdentityV1; 3] = [
    KeychainIdentityV1 {
        service: API_KEYCHAIN_SERVICE,
        account: "discord.oauth-client-secret",
    },
    KeychainIdentityV1 {
        service: API_KEYCHAIN_SERVICE,
        account: "discord.bot-token",
    },
    KeychainIdentityV1 {
        service: RUNTIME_KEYCHAIN_SERVICE,
        account: "discord.bot-token",
    },
];

pub const PRODUCT_ACTION_KEYRING_IDENTITY: KeychainIdentityV1 = KeychainIdentityV1 {
    service: API_KEYCHAIN_SERVICE,
    account: PRODUCT_ACTION_KEYRING_ACCOUNT,
};

pub const SNAPSHOT_ENVELOPE_KEYRING_IDENTITY: KeychainIdentityV1 = KeychainIdentityV1 {
    service: API_KEYCHAIN_SERVICE,
    account: SNAPSHOT_ENVELOPE_KEYRING_ACCOUNT,
};

pub const INTERACTION_TOKEN_ENVELOPE_KEYRING_IDENTITY: KeychainIdentityV1 = KeychainIdentityV1 {
    service: RUNTIME_KEYCHAIN_SERVICE,
    account: INTERACTION_TOKEN_ENVELOPE_KEYRING_ACCOUNT,
};

pub const ADMIN_KEYCHAIN_IDENTITY: KeychainIdentityV1 = KeychainIdentityV1 {
    service: ADMIN_KEYCHAIN_SERVICE,
    account: ADMIN_KEYCHAIN_ACCOUNT,
};

pub fn validate_identity_manifest() -> Result<(), ProvisionerErrorV1> {
    let roles = APPLICATION_DATABASE_IDENTITIES
        .iter()
        .map(|identity| identity.role)
        .collect::<BTreeSet<_>>();
    let keychain = APPLICATION_DATABASE_IDENTITIES
        .iter()
        .map(|identity| (identity.service, identity.account))
        .collect::<BTreeSet<_>>();
    if APPLICATION_DATABASE_IDENTITIES.len() != 20
        || roles.len() != 20
        || keychain.len() != 20
        || roles.contains(CLUSTER_ADMIN_ROLE)
        || roles.contains(OWNER_ROLE)
    {
        return Err(ProvisionerErrorV1::IdentityManifest);
    }
    Ok(())
}

pub fn peer_connect_options() -> PgConnectOptions {
    PgConnectOptions::new()
        .host(PEER_SOCKET_DIRECTORY)
        .port(DATABASE_PORT)
        .username(CLUSTER_ADMIN_ROLE)
        .database(DATABASE_NAME)
        .ssl_mode(PgSslMode::Disable)
        .application_name("starring-staging-provisioner")
}

pub fn database_url(role: &str, password: &str, database: &str) -> Zeroizing<String> {
    Zeroizing::new(format!(
        "postgresql://{role}:{password}@{DATABASE_HOST}:{DATABASE_PORT}/{database}?sslmode=disable"
    ))
}

pub fn exact_database_connect_options(
    value: &[u8],
    identity: DatabaseIdentityV1,
) -> Result<PgConnectOptions, ProvisionerErrorV1> {
    exact_connect_options(value, identity.role, DATABASE_NAME)
}

pub fn exact_admin_connect_options(value: &[u8]) -> Result<PgConnectOptions, ProvisionerErrorV1> {
    exact_connect_options(value, CLUSTER_ADMIN_ROLE, ADMIN_DATABASE_NAME)
}

pub fn exact_admin_target_connect_options(
    value: &[u8],
) -> Result<PgConnectOptions, ProvisionerErrorV1> {
    Ok(exact_admin_connect_options(value)?
        .database(DATABASE_NAME)
        .application_name("starring-staging-provisioner-final-target-verifier"))
}

fn exact_connect_options(
    value: &[u8],
    role: &str,
    database: &str,
) -> Result<PgConnectOptions, ProvisionerErrorV1> {
    let value = std::str::from_utf8(value).map_err(|_| ProvisionerErrorV1::DatabaseUrlShape)?;
    let prefix = format!("postgresql://{role}:");
    let suffix = format!("@{DATABASE_HOST}:{DATABASE_PORT}/{database}?sslmode=disable");
    let password = value
        .strip_prefix(&prefix)
        .and_then(|value| value.strip_suffix(&suffix))
        .ok_or(ProvisionerErrorV1::DatabaseUrlShape)?;
    if password.len() != 43
        || !password
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(ProvisionerErrorV1::DatabaseUrlShape);
    }
    let parsed =
        PgConnectOptions::from_str(value).map_err(|_| ProvisionerErrorV1::DatabaseUrlShape)?;
    Ok(parsed
        .host(DATABASE_HOST)
        .port(DATABASE_PORT)
        .username(role)
        .database(database)
        .ssl_mode(PgSslMode::Disable)
        .application_name("starring-staging-provisioner-final-verifier"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn password() -> &'static str {
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    }

    #[test]
    fn role_and_keychain_mapping_is_exact_and_distinct() {
        validate_identity_manifest().unwrap();
        assert_eq!(
            APPLICATION_DATABASE_IDENTITIES
                .iter()
                .filter(|identity| identity.service == API_KEYCHAIN_SERVICE)
                .count(),
            15
        );
        assert_eq!(
            APPLICATION_DATABASE_IDENTITIES
                .iter()
                .filter(|identity| identity.service == RUNTIME_KEYCHAIN_SERVICE)
                .count(),
            5
        );
        assert_eq!(
            APPLICATION_DATABASE_IDENTITIES[0],
            DatabaseIdentityV1 {
                service: API_KEYCHAIN_SERVICE,
                account: "database.oauth-flow-writer",
                role: "starring_identity_oauth",
            }
        );
        assert_eq!(
            APPLICATION_DATABASE_IDENTITIES[14],
            DatabaseIdentityV1 {
                service: API_KEYCHAIN_SERVICE,
                account: "database.authoring-session-writer",
                role: "starring_authoring_session_writer",
            }
        );
        assert_eq!(
            APPLICATION_DATABASE_IDENTITIES[19],
            DatabaseIdentityV1 {
                service: RUNTIME_KEYCHAIN_SERVICE,
                account: "database.interaction",
                role: "starring_runtime_interaction",
            }
        );
        assert_eq!(
            INTERACTION_TOKEN_ENVELOPE_KEYRING_IDENTITY,
            KeychainIdentityV1 {
                service: "starring.runtime.staging",
                account: "interaction.token-envelope-keyring",
            }
        );
    }

    #[test]
    fn final_url_parser_accepts_only_fixed_tcp_non_tls_identity() {
        let identity = APPLICATION_DATABASE_IDENTITIES[0];
        let valid = database_url(identity.role, password(), DATABASE_NAME);
        assert!(exact_database_connect_options(valid.as_bytes(), identity).is_ok());
        let wrong_database = database_url(identity.role, password(), "wrong");
        assert!(exact_database_connect_options(wrong_database.as_bytes(), identity).is_err());
        let required_ssl = valid.replace("sslmode=disable", "sslmode=require");
        assert!(exact_database_connect_options(required_ssl.as_bytes(), identity).is_err());
        let ipv6 = valid.replace("127.0.0.1", "[::1]");
        assert!(exact_database_connect_options(ipv6.as_bytes(), identity).is_err());
        let socket = format!(
            "postgresql://{}:{}@/{}?host=/private/tmp&sslmode=disable",
            identity.role,
            password(),
            DATABASE_NAME
        );
        assert!(exact_database_connect_options(socket.as_bytes(), identity).is_err());
        let unlisted = database_url("starring_unlisted", password(), DATABASE_NAME);
        assert!(exact_database_connect_options(unlisted.as_bytes(), identity).is_err());
    }
}
