import os
import pathlib
import plistlib

from d2_certification import canonical_json
from d2_orchestrator_contract import (
    API_DATABASE_ACCOUNTS,
    API_KEYRING_ACCOUNTS,
    REQUIRED_PROGRAMS,
    RUNTIME_DATABASE_ACCOUNTS,
    RUNTIME_KEYRING_ACCOUNTS,
    SCHEMA_VERSION,
    WORKER_ACCOUNTS,
    append_journal,
    external_keychain_inventory,
    fail,
    keychain_inventory,
    validate_identity,
    write_atomic,
)


RUNTIME_DATABASE_ROLES = (
    "starring_runtime_execution",
    "starring_runtime_exact_target",
    "starring_runtime_panel",
    "starring_runtime_serving",
    "starring_runtime_interaction",
)
API_DATABASE_ROLES = (
    "starring_identity_oauth",
    "starring_identity_issuer",
    "starring_identity_session",
    "starring_identity_security",
    "starring_installation_authority_reader",
    "starring_authorized_snapshot_reader",
    "starring_promotion_executor",
    "starring_decision_reader",
    "starring_decision_approval",
    "starring_decision_rejection",
    "starring_decision_apply",
    "starring_decision_cancellation",
    "starring_deployment_status_reader",
    "starring_operational_deployment_status_reader",
    "starring_authoring_session_writer",
)
D2A_SESSION_ISSUER_DATABASE_ROLES = (
    "starring_identity_oauth",
    "starring_identity_issuer",
    "starring_identity_security",
)


def environment_base():
    return {
        "HOME": str(pathlib.Path.home()),
        "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
    }


def keychain_reference(service, account):
    validate_identity(service, "keychain_reference_invalid")
    validate_identity(account, "keychain_reference_invalid")
    return f"keychain:{service}:{account}"


def api_environment(context):
    manifest = context.manifest
    service = manifest["keychain_services"]["api"]
    worker_service = manifest["keychain_services"]["worker"]
    environment = environment_base()
    environment.update(
        {
            "STARRING_API_BIND_PORT": str(manifest["services"]["api"]["port"]),
            "STARRING_API_PUBLIC_ORIGIN": manifest["public_origin"],
            "STARRING_API_OAUTH_RETURN_PATHS_JSON": '["/v1/me"]',
            "STARRING_API_OAUTH_DEFAULT_RETURN_PATH": "/v1/me",
            "STARRING_API_DATABASE_MAX_CONNECTIONS": "2",
            "STARRING_API_DATABASE_ACQUIRE_TIMEOUT_MILLISECONDS": "2000",
            "STARRING_API_DATABASE_IDLE_TIMEOUT_SECONDS": "120",
            "STARRING_API_DATABASE_MAX_LIFETIME_SECONDS": "900",
            "STARRING_API_DISCORD_APPLICATION_ID": manifest["discord"]["application_id"],
            "STARRING_API_DISCORD_BOT_USER_ID": manifest["discord"]["bot_user_id"],
            "STARRING_API_DISCORD_REQUEST_TIMEOUT_MILLISECONDS": "3000",
            "STARRING_API_DISCORD_WRITE_AUTHORITY_LIFETIME_MILLISECONDS": "3000",
            "STARRING_API_DISCORD_READ_AUTHORITY_LIFETIME_MILLISECONDS": "15000",
            "STARRING_API_AUTHORING_WORKER_URL": f"http://127.0.0.1:{manifest['services']['worker']['port']}",
            "STARRING_API_AUTHORING_WORKER_TOKEN_SECRET_REFERENCE": keychain_reference(
                worker_service, WORKER_ACCOUNTS[0]
            ),
            "STARRING_API_DISCORD_OAUTH_CLIENT_SECRET_REFERENCE": keychain_reference(
                manifest["external_keychain"]["discord_oauth_client_secret"]["service"],
                manifest["external_keychain"]["discord_oauth_client_secret"]["account"],
            ),
            "STARRING_API_DISCORD_BOT_TOKEN_REFERENCE": keychain_reference(
                manifest["external_keychain"]["discord_bot_token"]["service"],
                manifest["external_keychain"]["discord_bot_token"]["account"],
            ),
            "STARRING_API_PRODUCT_ACTION_KEYRING_SECRET_REFERENCE": keychain_reference(
                service, API_KEYRING_ACCOUNTS[0]
            ),
            "STARRING_API_SNAPSHOT_ENVELOPE_KEYRING_SECRET_REFERENCE": keychain_reference(
                service, API_KEYRING_ACCOUNTS[1]
            ),
        }
    )
    variable_by_account = {
        "database.oauth-flow-writer": "STARRING_API_OAUTH_FLOW_WRITER_DATABASE_SECRET_REFERENCE",
        "database.session-issuer": "STARRING_API_SESSION_ISSUER_DATABASE_SECRET_REFERENCE",
        "database.session-api": "STARRING_API_SESSION_API_DATABASE_SECRET_REFERENCE",
        "database.security-revoker": "STARRING_API_SECURITY_REVOKER_DATABASE_SECRET_REFERENCE",
        "database.installation-authority-reader": "STARRING_API_INSTALLATION_AUTHORITY_DATABASE_SECRET_REFERENCE",
        "database.authorized-snapshot-reader": "STARRING_API_AUTHORIZED_SNAPSHOT_DATABASE_SECRET_REFERENCE",
        "database.promotion-executor": "STARRING_API_PROMOTION_EXECUTOR_DATABASE_SECRET_REFERENCE",
        "database.decision-reader": "STARRING_API_DECISION_READER_DATABASE_SECRET_REFERENCE",
        "database.approval-executor": "STARRING_API_APPROVAL_EXECUTOR_DATABASE_SECRET_REFERENCE",
        "database.rejection-executor": "STARRING_API_REJECTION_EXECUTOR_DATABASE_SECRET_REFERENCE",
        "database.apply-executor": "STARRING_API_APPLY_EXECUTOR_DATABASE_SECRET_REFERENCE",
        "database.cancellation-executor": "STARRING_API_CANCELLATION_EXECUTOR_DATABASE_SECRET_REFERENCE",
        "database.deployment-status-reader": "STARRING_API_DEPLOYMENT_STATUS_DATABASE_SECRET_REFERENCE",
        "database.operational-deployment-status-reader": "STARRING_API_OPERATIONAL_STATUS_DATABASE_SECRET_REFERENCE",
        "database.authoring-session-writer": "STARRING_API_AUTHORING_SESSION_WRITER_DATABASE_SECRET_REFERENCE",
    }
    for account in API_DATABASE_ACCOUNTS:
        environment[variable_by_account[account]] = keychain_reference(service, account)
    return environment


def runtime_environment(context):
    manifest = context.manifest
    service = manifest["keychain_services"]["runtime"]
    environment = environment_base()
    environment.update(
        {
            "STARRING_RUNTIME_HEALTH_BIND_ADDRESS": f"127.0.0.1:{manifest['services']['runtime']['port']}",
            "STARRING_RUNTIME_DATABASE_MAX_CONNECTIONS": "2",
            "STARRING_RUNTIME_DATABASE_ACQUIRE_TIMEOUT_MILLISECONDS": "2000",
            "STARRING_RUNTIME_DATABASE_IDLE_TIMEOUT_SECONDS": "60",
            "STARRING_RUNTIME_DATABASE_MAX_LIFETIME_SECONDS": "600",
            "STARRING_RUNTIME_DATABASE_LOCK_TIMEOUT_MILLISECONDS": "1000",
            "STARRING_RUNTIME_DATABASE_STATEMENT_TIMEOUT_MILLISECONDS": "2000",
            "STARRING_RUNTIME_GATEWAY_COMMAND_CAPACITY": "8",
            "STARRING_RUNTIME_GATEWAY_LIFECYCLE_CAPACITY": "64",
            "STARRING_RUNTIME_GATEWAY_REJECTION_ACKNOWLEDGEMENT_CAPACITY": "64",
            "STARRING_RUNTIME_GATEWAY_GLOBAL_ADMISSION_CAPACITY": "256",
            "STARRING_RUNTIME_GATEWAY_OWNER_LEASE_MILLISECONDS": "60000",
            "STARRING_RUNTIME_GATEWAY_OWNER_RENEW_BEFORE_MILLISECONDS": "40000",
            "STARRING_RUNTIME_GATEWAY_OWNER_SAFETY_MARGIN_MILLISECONDS": "5000",
            "STARRING_RUNTIME_GATEWAY_DRAIN_TIMEOUT_SECONDS": "15",
            "STARRING_RUNTIME_INSTANCE_LOOKUP_TIMEOUT_MILLISECONDS": "500",
            "STARRING_RUNTIME_DISCORD_TRANSPORT_MODE": "loopback_proxy_v1",
            "STARRING_RUNTIME_DISCORD_GATEWAY_PROXY_URL": f"ws://127.0.0.1:{manifest['services']['transport']['gateway_port']}",
            "STARRING_RUNTIME_DISCORD_EFFECT_HTTP_PROXY_AUTHORITY": f"127.0.0.1:{manifest['services']['transport']['http_port']}",
            "STARRING_RUNTIME_DISCORD_BOT_TOKEN_SECRET_REFERENCE": keychain_reference(
                manifest["external_keychain"]["discord_bot_token"]["service"],
                manifest["external_keychain"]["discord_bot_token"]["account"],
            ),
            "STARRING_RUNTIME_INTERACTION_TOKEN_ENVELOPE_KEYRING_SECRET_REFERENCE": keychain_reference(
                service, RUNTIME_KEYRING_ACCOUNTS[0]
            ),
        }
    )
    variable_by_account = {
        "database.execution": "STARRING_RUNTIME_CONVERGENCE_DATABASE_URL_SECRET_REFERENCE",
        "database.exact-target": "STARRING_RUNTIME_EXACT_TARGET_DATABASE_URL_SECRET_REFERENCE",
        "database.panel": "STARRING_RUNTIME_PANEL_DATABASE_URL_SECRET_REFERENCE",
        "database.serving": "STARRING_RUNTIME_SERVING_DATABASE_URL_SECRET_REFERENCE",
        "database.interaction": "STARRING_RUNTIME_INTERACTION_DATABASE_URL_SECRET_REFERENCE",
    }
    for account in RUNTIME_DATABASE_ACCOUNTS:
        environment[variable_by_account[account]] = keychain_reference(service, account)
    return environment


def worker_environment(context):
    manifest = context.manifest
    service = manifest["keychain_services"]["worker"]
    environment = environment_base()
    environment.update(
        {
            "NODE_ENV": "production",
            "PATH": "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
            "STARRING_CODEX_PATH": manifest["candidates"]["codex"]["path"],
            "STARRING_CODEX_WORKER_CONCURRENCY": "1",
            "STARRING_CODEX_WORKER_KEYCHAIN_SERVICE": service,
            "STARRING_CODEX_WORKER_KEYCHAIN_ACCOUNT": WORKER_ACCOUNTS[0],
            "STARRING_CODEX_WORKER_MAX_QUEUE": "4",
            "STARRING_CODEX_WORKER_METRICS_LOG": str(context.log_directory / "worker-metrics.jsonl"),
            "STARRING_CODEX_WORKER_PORT": str(manifest["services"]["worker"]["port"]),
            "STARRING_CODEX_WORKER_TIMEOUT_MS": "55000",
        }
    )
    return environment


def launchd_plist(label, program_arguments, environment, log_path, working_directory):
    return {
        "Label": label,
        "ProgramArguments": [str(argument) for argument in program_arguments],
        "EnvironmentVariables": environment,
        "WorkingDirectory": str(working_directory),
        "RunAtLoad": False,
        "KeepAlive": {"SuccessfulExit": False},
        "ProcessType": "Standard",
        "ThrottleInterval": 30,
        "ExitTimeOut": 90,
        "Umask": 63,
        "StandardOutPath": str(log_path),
        "StandardErrorPath": str(log_path),
        "SoftResourceLimits": {"NumberOfFiles": 2048},
        "HardResourceLimits": {"NumberOfFiles": 4096},
    }


def tunnel_environment(context):
    identity = context.manifest["external_keychain"]["tunnel_token"]
    environment = environment_base()
    environment.update(
        {
            "STARRING_D2_CLOUDFLARED_PATH": context.manifest["candidates"]["cloudflared"]["path"],
            "STARRING_D2_CLOUDFLARE_TUNNEL_ID": context.manifest["cloudflare"]["tunnel_id"],
            "STARRING_D2_CLOUDFLARE_ORIGIN_SERVICE": context.manifest["cloudflare"]["origin_service"],
            "STARRING_D2_TUNNEL_KEYCHAIN_SERVICE": identity["service"],
            "STARRING_D2_TUNNEL_KEYCHAIN_ACCOUNT": identity["account"],
        }
    )
    return environment


def tunnel_runner_path(context):
    return context.artifact_directory / "run-tunnel.zsh"


def write_tunnel_runner(context):
    payload = "\n".join(
        (
            "#!/bin/zsh",
            "set -euo pipefail",
            "token=\"$(/usr/bin/security find-generic-password -a \"$STARRING_D2_TUNNEL_KEYCHAIN_ACCOUNT\" -s \"$STARRING_D2_TUNNEL_KEYCHAIN_SERVICE\" -w)\"",
            "export TUNNEL_TOKEN=\"$token\"",
            "unset token",
            "exec \"$STARRING_D2_CLOUDFLARED_PATH\" tunnel --no-autoupdate --loglevel warn --transport-loglevel warn run --url \"$STARRING_D2_CLOUDFLARE_ORIGIN_SERVICE\" \"$STARRING_D2_CLOUDFLARE_TUNNEL_ID\"",
            "",
        )
    )
    append_journal(context, "tunnel_runner_write", "intent", "tunnel")
    write_atomic(tunnel_runner_path(context), payload, mode=0o700)
    append_journal(context, "tunnel_runner_write", "complete", "tunnel")


def compose_plists(context):
    manifest = context.manifest
    candidates = manifest["candidates"]
    repo_root = pathlib.Path(candidates["codex_worker"]["path"]).parents[2]
    return {
        "api": launchd_plist(
            manifest["services"]["api"]["label"],
            [candidates["api"]["path"]],
            api_environment(context),
            context.log_directory / "api.log",
            pathlib.Path.home(),
        ),
        "runtime": launchd_plist(
            manifest["services"]["runtime"]["label"],
            [candidates["runtime"]["path"]],
            runtime_environment(context),
            context.log_directory / "runtime.log",
            repo_root,
        ),
        "worker": launchd_plist(
            manifest["services"]["worker"]["label"],
            [candidates["node"]["path"], candidates["codex_worker"]["path"]],
            worker_environment(context),
            context.log_directory / "worker.log",
            repo_root,
        ),
        "transport": launchd_plist(
            manifest["services"]["transport"]["label"],
            [
                candidates["certification_transport"]["path"],
                "--root",
                context.root,
                "--run-id",
                manifest["run_id"],
                "--guild-id",
                manifest["discord"]["guild_id"],
                "--hub-channel-id",
                manifest["discord"]["hub_channel_id"],
                "--actor-id",
                manifest["discord"]["actor_id"],
                "--bot-user-id",
                manifest["discord"]["bot_user_id"],
                "--gateway-listen",
                f"127.0.0.1:{manifest['services']['transport']['gateway_port']}",
                "--http-listen",
                f"127.0.0.1:{manifest['services']['transport']['http_port']}",
            ],
            environment_base(),
            context.log_directory / "transport.log",
            context.root,
        ),
        "tunnel": launchd_plist(
            manifest["services"]["tunnel"]["label"],
            ["/bin/zsh", tunnel_runner_path(context)],
            tunnel_environment(context),
            context.log_directory / "tunnel.log",
            pathlib.Path.home(),
        ),
    }


def write_plists(context, platform):
    context.plist_directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    write_tunnel_runner(context)
    for name, value in compose_plists(context).items():
        label = context.manifest["services"][name]["label"]
        append_journal(context, "plist_write", "intent", label)
        path = context.plist_directory / f"{label}.plist"
        write_atomic(path, plistlib.dumps(value, fmt=plistlib.FMT_XML, sort_keys=True))
        result = platform.run([REQUIRED_PROGRAMS["plutil"], "-lint", path], timeout=5)
        if result.returncode != 0:
            fail("generated_plist_invalid")
        append_journal(context, "plist_write", "complete", label)


def write_keychain_plan(context):
    plan = {
        "schema_version": SCHEMA_VERSION,
        "manifest_sha256": context.digest,
        "owned_identities": [
            {"service": service, "account": account}
            for service, account in keychain_inventory(context)
        ],
        "external_read_only_identities": [
            {"service": service, "account": account}
            for service, account in external_keychain_inventory(context)
        ],
        "secret_values_present": False,
    }
    write_atomic(
        context.artifact_directory / "keychain-plan.json", canonical_json(plan) + "\n"
    )


def configure_postgres(context):
    configuration = "\n".join(
        (
            f"port = {context.manifest['database']['port']}",
            f"unix_socket_directories = '{context.socket_directory}'",
            f"include = '{context.root / 'postgres-network.conf'}'",
            "password_encryption = 'scram-sha-256'",
            "ssl = off",
            "max_connections = 64",
            "shared_buffers = '128MB'",
            "fsync = on",
            "full_page_writes = on",
            "logging_collector = on",
            f"log_directory = '{context.log_directory}'",
            "log_filename = 'postgresql.log'",
            "log_connections = off",
            "log_disconnections = off",
            "log_statement = 'none'",
            "",
        )
    )
    append_journal(context, "postgres_configure", "intent", "cluster")
    with (context.cluster_root / "postgresql.conf").open("a", encoding="utf-8") as handle:
        handle.write(configuration)
        handle.flush()
        os.fsync(handle.fileno())
    configure_postgres_bootstrap_network(context)
    append_journal(context, "postgres_configure", "complete", "cluster")


def configure_postgres_bootstrap_network(context):
    network = "listen_addresses = ''\n"
    hba = "\n".join(
        (
            "local postgres,starring_runtime_staging starring_cluster_admin peer map=starring_bootstrap",
            "host all all 0.0.0.0/0 reject",
            "host all all ::0/0 reject",
            "local all all reject",
            "host replication all 0.0.0.0/0 reject",
            "host replication all ::0/0 reject",
            "local replication all reject",
            "",
        )
    )
    write_atomic(context.root / "postgres-network.conf", network)
    write_atomic(context.cluster_root / "pg_hba.conf", hba)
    write_atomic(
        context.cluster_root / "pg_ident.conf",
        "starring_bootstrap jungbogeon starring_cluster_admin\n",
    )


def configure_postgres_sealed_network(context):
    network = "listen_addresses = '127.0.0.1'\n"
    runtime_roles = ",".join(RUNTIME_DATABASE_ROLES)
    api_roles = ",".join(API_DATABASE_ROLES)
    session_issuer_roles = ",".join(D2A_SESSION_ISSUER_DATABASE_ROLES)
    hba = "\n".join(
        (
            f"hostnossl starring_runtime_staging {runtime_roles} 127.0.0.1/32 scram-sha-256",
            f"host all {runtime_roles} 0.0.0.0/0 reject",
            f"host all {runtime_roles} ::0/0 reject",
            f"local all {runtime_roles} reject",
            f"hostnossl starring_runtime_staging {api_roles} 127.0.0.1/32 scram-sha-256",
            f"host all {api_roles} 0.0.0.0/0 reject",
            f"host all {api_roles} ::0/0 reject",
            f"local starring_runtime_staging {session_issuer_roles} scram-sha-256",
            f"local all {api_roles} reject",
            "hostnossl postgres,starring_runtime_staging starring_cluster_admin 127.0.0.1/32 scram-sha-256",
            "host all all 0.0.0.0/0 reject",
            "host all all ::0/0 reject",
            "local all all reject",
            "host replication all 0.0.0.0/0 reject",
            "host replication all ::0/0 reject",
            "local replication all reject",
            "",
        )
    )
    write_atomic(context.root / "postgres-network.conf", network)
    write_atomic(context.cluster_root / "pg_hba.conf", hba)
