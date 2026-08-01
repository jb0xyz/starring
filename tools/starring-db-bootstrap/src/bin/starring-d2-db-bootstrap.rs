use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use sha2::{Digest, Sha256};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};
use sqlx::{Connection, Row};
use starring_db_bootstrap::{
    bootstrap_staging_database_with_authentication, BootstrapAuthenticationV1,
    StagingAcknowledgementV1,
};

const DATABASE: &str = "starring_runtime_staging";
const ADMIN_DATABASE: &str = "postgres";
const ADMIN_ROLE: &str = "starring_cluster_admin";

struct ArgumentsV1 {
    run_id: String,
    cluster_root: PathBuf,
    socket_directory: PathBuf,
    port: u16,
}

struct ReportV1 {
    system_identifier: String,
    migration_count: usize,
    migration_head: i64,
    migration_ledger_sha256: String,
    relation_count: i64,
    capability_function_count: usize,
}

#[tokio::main]
async fn main() -> ExitCode {
    match execute().await {
        Ok(report) => {
            println!(
                "{{\"database_system_identifier\":\"{}\",\"migration_count\":{},\"migration_head\":\"{}\",\"migration_ledger_sha256\":\"{}\",\"relation_count\":{},\"capability_function_count\":{}}}",
                report.system_identifier,
                report.migration_count,
                report.migration_head,
                report.migration_ledger_sha256,
                report.relation_count,
                report.capability_function_count,
            );
            ExitCode::SUCCESS
        }
        Err(code) => {
            eprintln!("{code}");
            ExitCode::FAILURE
        }
    }
}

async fn execute() -> Result<ReportV1, &'static str> {
    let arguments = parse_arguments()?;
    validate_target(&arguments)?;
    let socket_directory = arguments
        .socket_directory
        .to_str()
        .ok_or("d2_database_target_invalid")?;
    let options = PgConnectOptions::new()
        .host(socket_directory)
        .port(arguments.port)
        .username(ADMIN_ROLE)
        .database(ADMIN_DATABASE)
        .ssl_mode(PgSslMode::Disable)
        .application_name("starring-d2-db-bootstrap");
    let system_identifier = probe_target(&options, &arguments).await?;
    let acknowledgement_value = format!(
        "starring-runtime-dedicated-staging-cluster-v2:{system_identifier}:{DATABASE}:cluster-wide-public-acl-reset:bidirectional-runtime-membership-revocation"
    );
    let acknowledgement =
        StagingAcknowledgementV1::parse(&system_identifier, &acknowledgement_value)
            .map_err(|_| "d2_database_acknowledgement_failed")?;
    let report = bootstrap_staging_database_with_authentication(
        options.clone(),
        BootstrapAuthenticationV1::TemporaryPeer,
        acknowledgement,
    )
    .await
    .map_err(|_| "d2_database_bootstrap_failed")?;
    let (migration_count, migration_head, migration_ledger_sha256) =
        read_migration_ledger(&options).await?;
    if migration_count != report.migration_count() {
        return Err("d2_database_migration_count_drift");
    }
    Ok(ReportV1 {
        system_identifier,
        migration_count,
        migration_head,
        migration_ledger_sha256,
        relation_count: report.relation_count(),
        capability_function_count: report.capability_function_count(),
    })
}

fn parse_arguments() -> Result<ArgumentsV1, &'static str> {
    let values = env::args().skip(1).collect::<Vec<_>>();
    let [run_flag, run_id, root_flag, cluster_root, socket_flag, socket_directory, port_flag, port] =
        values.as_slice()
    else {
        return Err("d2_database_arguments_invalid");
    };
    if run_flag != "--run-id"
        || root_flag != "--cluster-root"
        || socket_flag != "--socket-directory"
        || port_flag != "--port"
    {
        return Err("d2_database_arguments_invalid");
    }
    let port = port
        .parse::<u16>()
        .ok()
        .filter(|value| valid_port(*value))
        .ok_or("d2_database_arguments_invalid")?;
    Ok(ArgumentsV1 {
        run_id: run_id.to_owned(),
        cluster_root: PathBuf::from(cluster_root),
        socket_directory: PathBuf::from(socket_directory),
        port,
    })
}

fn validate_target(arguments: &ArgumentsV1) -> Result<(), &'static str> {
    if !valid_run_id(&arguments.run_id) {
        return Err("d2_database_run_id_invalid");
    }
    let root = PathBuf::from(format!("/private/tmp/starring-d2-{}", arguments.run_id));
    let expected_cluster = root.join("postgres");
    let expected_socket = root.join("socket");
    if arguments.cluster_root != expected_cluster || arguments.socket_directory != expected_socket {
        return Err("d2_database_target_invalid");
    }
    let canonical_cluster = arguments
        .cluster_root
        .canonicalize()
        .map_err(|_| "d2_database_target_invalid")?;
    let canonical_socket = arguments
        .socket_directory
        .canonicalize()
        .map_err(|_| "d2_database_target_invalid")?;
    if canonical_cluster != expected_cluster
        || canonical_socket != expected_socket
        || !canonical_cluster.join("PG_VERSION").is_file()
    {
        return Err("d2_database_target_invalid");
    }
    Ok(())
}

fn valid_run_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 32
        && value.starts_with("d2-")
        && bytes[3..11].iter().all(u8::is_ascii_digit)
        && bytes[11] == b't'
        && bytes[12..18].iter().all(u8::is_ascii_digit)
        && bytes[18] == b'z'
        && bytes[19] == b'-'
        && bytes[20..].iter().all(u8::is_ascii_hexdigit)
        && bytes[20..].iter().all(|byte| !byte.is_ascii_uppercase())
}

fn valid_port(value: u16) -> bool {
    value >= 1024 && !matches!(value, 5432 | 18080 | 18181 | 19091)
}

async fn probe_target(
    options: &PgConnectOptions,
    arguments: &ArgumentsV1,
) -> Result<String, &'static str> {
    let mut connection = PgConnection::connect_with(options)
        .await
        .map_err(|_| "d2_database_probe_connection_failed")?;
    let row = sqlx::query(
        "SELECT current_setting('data_directory'), current_setting('port')::INTEGER, pg_catalog.inet_client_addr() IS NULL, current_user, control.system_identifier::TEXT, current_setting('server_version_num')::INTEGER, current_setting('data_checksums') FROM pg_catalog.pg_control_system() AS control",
    )
    .fetch_one(&mut connection)
    .await
    .map_err(|_| "d2_database_probe_failed")?;
    let data_directory: String = row.try_get(0).map_err(|_| "d2_database_probe_failed")?;
    let port: i32 = row.try_get(1).map_err(|_| "d2_database_probe_failed")?;
    let unix_socket: bool = row.try_get(2).map_err(|_| "d2_database_probe_failed")?;
    let user: String = row.try_get(3).map_err(|_| "d2_database_probe_failed")?;
    let system_identifier: String = row.try_get(4).map_err(|_| "d2_database_probe_failed")?;
    let version: i32 = row.try_get(5).map_err(|_| "d2_database_probe_failed")?;
    let checksums: String = row.try_get(6).map_err(|_| "d2_database_probe_failed")?;
    let expected_cluster = arguments
        .cluster_root
        .to_str()
        .ok_or("d2_database_target_invalid")?;
    if data_directory != expected_cluster
        || port != i32::from(arguments.port)
        || !unix_socket
        || user != ADMIN_ROLE
        || !(160000..170000).contains(&version)
        || checksums != "on"
        || system_identifier.is_empty()
        || !system_identifier.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err("d2_database_probe_contract_failed");
    }
    Ok(system_identifier)
}

async fn read_migration_ledger(
    admin_options: &PgConnectOptions,
) -> Result<(usize, i64, String), &'static str> {
    let target_options = admin_options.clone().database(DATABASE);
    let mut connection = PgConnection::connect_with(&target_options)
        .await
        .map_err(|_| "d2_database_ledger_connection_failed")?;
    let rows = sqlx::query(
        "SELECT version, success, checksum FROM public._sqlx_migrations ORDER BY version",
    )
    .fetch_all(&mut connection)
    .await
    .map_err(|_| "d2_database_ledger_failed")?;
    if rows.is_empty() {
        return Err("d2_database_ledger_empty");
    }
    let mut digest = Sha256::new();
    let mut head = 0_i64;
    for row in &rows {
        let version: i64 = row.try_get(0).map_err(|_| "d2_database_ledger_failed")?;
        let success: bool = row.try_get(1).map_err(|_| "d2_database_ledger_failed")?;
        let checksum: Vec<u8> = row.try_get(2).map_err(|_| "d2_database_ledger_failed")?;
        if version <= head || !success || checksum.is_empty() {
            return Err("d2_database_ledger_contract_failed");
        }
        digest.update(version.to_be_bytes());
        digest.update([u8::from(success)]);
        digest.update((checksum.len() as u64).to_be_bytes());
        digest.update(&checksum);
        head = version;
    }
    Ok((rows.len(), head, format!("{:x}", digest.finalize())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_id_contract_is_exact() {
        assert!(valid_run_id("d2-20260801t120000z-0123456789ab"));
        assert!(!valid_run_id("d2-20260801t120000z-0123456789AB"));
        assert!(!valid_run_id("d2-20260801t120000z-0123456789a"));
        assert!(!valid_run_id("other-20260801t120000z-0123456789ab"));
    }

    #[test]
    fn protected_ports_are_rejected_by_argument_contract() {
        for port in [5432_u16, 18080, 18181, 19091] {
            assert!(!valid_port(port));
        }
        assert!(valid_port(55433));
        assert!(!valid_port(1023));
    }
}
