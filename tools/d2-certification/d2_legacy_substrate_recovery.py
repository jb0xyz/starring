import hashlib
import json
import os
import pathlib
import re
import shutil
import stat

from d2_certification import canonical_json, require_absolute_path
from d2_orchestrator_contract import (
    D2_RUNTIME_ROOT_PARENT,
    OWNER_ACCOUNT,
    PROTECTED_KEYCHAIN_SERVICES,
    PROTECTED_PORTS,
    RUN_ID_PATTERN,
    RunContext,
    append_journal,
    fail,
    keychain_inventory,
    load_discord_ownership_registry,
    save_state,
    standing_snapshot,
    utc_now,
    write_atomic,
)


LEGACY_RECOVERY_KIND = "starring.d2.legacy-substrate-recovery.v1"
LEGACY_RECOVERY_PHASES = {
    "preparing",
    "prepared",
    "substrate_starting",
    "substrate_started",
    "credentials_sealing",
    "candidate_starting",
    "candidate_started",
    "onboarding",
    "stopped",
    "cleaned",
}
SNOWFLAKE_PATTERN = re.compile(r"^[1-9][0-9]{0,19}$")
DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")


def strict_json_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            fail("legacy_manifest_invalid")
        value[key] = item
    return value


def require_owned_regular(path, mode, code, maximum_bytes):
    try:
        metadata = path.lstat()
    except OSError:
        fail(code)
    if (
        not stat.S_ISREG(metadata.st_mode)
        or path.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != mode
        or metadata.st_nlink != 1
        or metadata.st_size <= 0
        or metadata.st_size > maximum_bytes
    ):
        fail(code)
    return metadata


def load_strict_json(path, mode, code, maximum_bytes):
    require_owned_regular(path, mode, code, maximum_bytes)
    try:
        return json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=strict_json_object
        )
    except (OSError, UnicodeDecodeError, json.JSONDecodeError):
        fail(code)


def require_identity(value, code):
    if (
        not isinstance(value, str)
        or not value
        or len(value) > 192
        or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", value)
    ):
        fail(code)
    return value


def require_port(value):
    if type(value) is not int or value < 1024 or value > 65535:
        fail("legacy_manifest_port_invalid")
    return value


def require_snowflake(value):
    if (
        not isinstance(value, str)
        or not SNOWFLAKE_PATTERN.fullmatch(value)
        or int(value) > 18446744073709551615
    ):
        fail("legacy_manifest_discord_invalid")
    return value


def validate_legacy_manifest(manifest, manifest_path):
    required = {
        "schema_version",
        "run_id",
        "commit_sha",
        "created_at",
        "public_origin",
        "cloudflare",
        "authoring",
        "candidates",
        "source_trees",
        "discord",
        "database",
        "services",
        "keychain_services",
        "external_keychain",
        "protected_staging",
        "expected_steps",
        "human_boundaries",
    }
    if (
        not isinstance(manifest, dict)
        or set(manifest) != required
        or type(manifest.get("schema_version")) is not int
        or manifest.get("schema_version") != 1
    ):
        fail("legacy_manifest_invalid")
    run_id = manifest.get("run_id")
    if not isinstance(run_id, str) or not RUN_ID_PATTERN.fullmatch(run_id):
        fail("legacy_manifest_run_id_invalid")
    if manifest_path.parent.name != run_id:
        fail("legacy_manifest_run_directory_invalid")
    expected_root = D2_RUNTIME_ROOT_PARENT / f"starring-d2-{run_id}"
    database = manifest.get("database")
    if (
        not isinstance(database, dict)
        or set(database)
        != {"cluster_root", "socket_directory", "name", "port"}
        or database.get("cluster_root") != str(expected_root / "postgres")
        or database.get("socket_directory") != str(expected_root / "socket")
        or database.get("name") != "starring_runtime_staging"
    ):
        fail("legacy_manifest_database_invalid")
    database_port = require_port(database.get("port"))
    suffix = run_id.rsplit("-", 1)[1]
    expected_labels = {
        "api": f"local.starring.d2.{suffix}.api",
        "runtime": f"local.starring.d2.{suffix}.runtime",
        "transport": f"local.starring.d2.{suffix}.transport",
        "tunnel": f"local.starring.d2.{suffix}.tunnel",
        "worker": f"local.starring.d2.{suffix}.worker",
    }
    expected_service_fields = {
        "api": {"label", "port"},
        "runtime": {"label", "port"},
        "transport": {"label", "gateway_port", "http_port"},
        "tunnel": {"label"},
        "worker": {"label", "port"},
    }
    services = manifest.get("services")
    if not isinstance(services, dict) or set(services) != set(expected_labels):
        fail("legacy_manifest_services_invalid")
    ports = [database_port]
    for name, label in expected_labels.items():
        service = services.get(name)
        if (
            not isinstance(service, dict)
            or set(service) != expected_service_fields[name]
            or service.get("label") != label
        ):
            fail("legacy_manifest_services_invalid")
        for field in expected_service_fields[name] - {"label"}:
            ports.append(require_port(service.get(field)))
    if len(ports) != len(set(ports)) or set(ports).intersection(PROTECTED_PORTS):
        fail("legacy_manifest_port_invalid")
    expected_keychain = {
        "api": f"starring.d2.{suffix}.api",
        "runtime": f"starring.d2.{suffix}.runtime",
        "postgres": f"starring.d2.{suffix}.postgres",
        "worker": f"starring.d2.{suffix}.worker",
    }
    if manifest.get("keychain_services") != expected_keychain:
        fail("legacy_manifest_keychain_invalid")
    external = manifest.get("external_keychain")
    if (
        not isinstance(external, dict)
        or set(external)
        != {"discord_oauth_client_secret", "discord_bot_token", "tunnel_token"}
    ):
        fail("legacy_manifest_external_keychain_invalid")
    external_identities = []
    for identity in external.values():
        if not isinstance(identity, dict) or set(identity) != {"service", "account"}:
            fail("legacy_manifest_external_keychain_invalid")
        external_identities.append(
            (
                require_identity(
                    identity.get("service"), "legacy_manifest_external_keychain_invalid"
                ),
                require_identity(
                    identity.get("account"), "legacy_manifest_external_keychain_invalid"
                ),
            )
        )
    if (
        len(external_identities) != len(set(external_identities))
        or {service for service, _account in external_identities}.intersection(
            set(expected_keychain.values()) | PROTECTED_KEYCHAIN_SERVICES
        )
    ):
        fail("legacy_manifest_external_keychain_invalid")
    protected = manifest.get("protected_staging")
    protected_labels = (
        protected.get("launchd_labels") if isinstance(protected, dict) else None
    )
    if (
        not isinstance(protected, dict)
        or set(protected) != {"database", "launchd_labels", "mutation_allowed"}
        or protected.get("database") != "starring_runtime_staging@127.0.0.1:5432"
        or protected.get("mutation_allowed") is not False
        or not isinstance(protected_labels, list)
        or any(not isinstance(label, str) for label in protected_labels)
        or len(protected_labels) != len(set(protected_labels))
        or set(protected_labels).intersection(expected_labels.values())
    ):
        fail("legacy_manifest_protected_staging_invalid")
    for label in protected_labels:
        require_identity(label, "legacy_manifest_protected_staging_invalid")
    discord = manifest.get("discord")
    if (
        not isinstance(discord, dict)
        or set(discord)
        != {
            "guild_id",
            "hub_channel_id",
            "application_id",
            "bot_user_id",
            "actor_id",
            "resource_prefix",
            "disposable_guild_required",
        }
        or discord.get("disposable_guild_required") is not True
    ):
        fail("legacy_manifest_discord_invalid")
    for field in (
        "guild_id",
        "hub_channel_id",
        "application_id",
        "bot_user_id",
        "actor_id",
    ):
        require_snowflake(discord.get(field))
    require_identity(discord.get("resource_prefix"), "legacy_manifest_discord_invalid")
    return manifest


def load_legacy_context(raw_manifest):
    manifest_path = require_absolute_path(raw_manifest, "manifest")
    try:
        run_metadata = manifest_path.parent.lstat()
    except OSError:
        fail("legacy_manifest_run_directory_invalid")
    if (
        not stat.S_ISDIR(run_metadata.st_mode)
        or manifest_path.parent.is_symlink()
        or run_metadata.st_uid != os.getuid()
        or stat.S_IMODE(run_metadata.st_mode) != 0o700
    ):
        fail("legacy_manifest_run_directory_invalid")
    manifest = load_strict_json(
        manifest_path, 0o600, "legacy_manifest_invalid", 1024 * 1024
    )
    digest_path = manifest_path.with_name("manifest.sha256")
    require_owned_regular(
        digest_path, 0o600, "legacy_manifest_digest_invalid", 1024
    )
    try:
        digest = digest_path.read_text(encoding="ascii").strip()
    except (OSError, UnicodeDecodeError):
        fail("legacy_manifest_digest_invalid")
    calculated = hashlib.sha256(canonical_json(manifest).encode("utf-8")).hexdigest()
    if not DIGEST_PATTERN.fullmatch(digest) or digest != calculated:
        fail("legacy_manifest_digest_invalid")
    validate_legacy_manifest(manifest, manifest_path)
    context = RunContext(manifest_path, manifest, digest)
    artifact = context.artifact_directory
    try:
        artifact_metadata = artifact.lstat()
    except OSError:
        fail("legacy_orchestrator_artifact_invalid")
    if (
        not stat.S_ISDIR(artifact_metadata.st_mode)
        or artifact.is_symlink()
        or artifact_metadata.st_uid != os.getuid()
        or stat.S_IMODE(artifact_metadata.st_mode) != 0o700
    ):
        fail("legacy_orchestrator_artifact_invalid")
    state = load_strict_json(
        context.state_path, 0o600, "legacy_orchestrator_state_invalid", 1024 * 1024
    )
    if (
        not isinstance(state, dict)
        or set(state)
        != {
            "schema_version",
            "manifest_sha256",
            "run_id",
            "phase",
            "updated_at",
            "standing_snapshot",
        }
        or type(state.get("schema_version")) is not int
        or state.get("schema_version") != 1
        or state.get("manifest_sha256") != digest
        or state.get("run_id") != manifest["run_id"]
        or state.get("phase") not in LEGACY_RECOVERY_PHASES
        or not valid_standing_snapshot(
            manifest, state.get("standing_snapshot")
        )
    ):
        fail("legacy_orchestrator_state_invalid")
    return context, state


def valid_standing_snapshot(manifest, snapshot):
    labels = manifest["protected_staging"]["launchd_labels"]
    if (
        not isinstance(snapshot, dict)
        or set(snapshot) != {"launchd_loaded", "plist_sha256", "port_occupied"}
        or not isinstance(snapshot.get("launchd_loaded"), dict)
        or set(snapshot["launchd_loaded"]) != set(labels)
        or any(type(value) is not bool for value in snapshot["launchd_loaded"].values())
        or not isinstance(snapshot.get("plist_sha256"), dict)
        or set(snapshot["plist_sha256"]) != set(labels)
        or any(
            value is not None
            and (not isinstance(value, str) or not DIGEST_PATTERN.fullmatch(value))
            for value in snapshot["plist_sha256"].values()
        )
        or not isinstance(snapshot.get("port_occupied"), dict)
        or set(snapshot["port_occupied"])
        != {str(port) for port in PROTECTED_PORTS}
        or any(type(value) is not bool for value in snapshot["port_occupied"].values())
    ):
        return False
    return True


def require_registry_unowned(context):
    registry = load_discord_ownership_registry()
    manifest = context.manifest
    discord = manifest["discord"]
    for owner in registry["owners"]:
        if any(
            owner[field] == value
            for field, value in (
                ("run_id", manifest["run_id"]),
                ("manifest_sha256", context.digest),
                ("manifest_path", str(context.manifest_path)),
                ("guild_id", discord["guild_id"]),
                ("application_id", discord["application_id"]),
            )
        ):
            fail("legacy_substrate_registry_conflict")
    return hashlib.sha256(canonical_json(registry).encode("utf-8")).hexdigest()


def validate_runtime_root(context):
    if not context.root.exists() and not context.root.is_symlink():
        return
    try:
        metadata = context.root.lstat()
    except OSError:
        fail("legacy_substrate_root_invalid")
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or context.root.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        fail("legacy_substrate_root_invalid")
    if context.cluster_root.exists() or context.cluster_root.is_symlink():
        cluster = context.cluster_root.lstat()
        if (
            not stat.S_ISDIR(cluster.st_mode)
            or context.cluster_root.is_symlink()
            or cluster.st_uid != os.getuid()
            or stat.S_IMODE(cluster.st_mode) != 0o700
        ):
            fail("legacy_substrate_cluster_invalid")
    root_device = metadata.st_dev
    try:
        for directory, names, files in os.walk(context.root, followlinks=False):
            for name in names + files:
                path = pathlib.Path(directory) / name
                item = path.lstat()
                if item.st_dev != root_device:
                    fail("legacy_substrate_mount_boundary_invalid")
    except OSError:
        fail("legacy_substrate_root_invalid")


def keychain_state(context, platform):
    inventory = keychain_inventory(context)
    present = {
        (service, account)
        for service, account in inventory
        if platform.keychain_present(service, account)
    }
    for service in sorted({service for service, _account in inventory}):
        service_present = {
            account for item_service, account in present if item_service == service
        }
        if service_present and (
            OWNER_ACCOUNT not in service_present
            or not platform.keychain_owner_matches(
                service, context.manifest["run_id"]
            )
        ):
            fail("legacy_substrate_keychain_ownership_invalid")
    return inventory, present


def legacy_substrate_status(context, state, platform):
    validate_runtime_root(context)
    registry_sha256 = require_registry_unowned(context)
    inventory, present = keychain_state(context, platform)
    current_snapshot = standing_snapshot(context, platform)
    loaded = [
        name
        for name, service in sorted(context.manifest["services"].items())
        if platform.launchd_loaded(service["label"])
    ]
    postgres_running = context.cluster_root.exists() and platform.postgres_running(
        context.cluster_root
    )
    return {
        "status": "observed",
        "phase": state["phase"],
        "run_id": context.manifest["run_id"],
        "manifest_sha256": context.digest,
        "runtime_root_present": context.root.exists(),
        "postgres_running": postgres_running,
        "loaded_services": loaded,
        "keychain_items_present": len(present),
        "keychain_items_expected": len(inventory),
        "protected_staging_unchanged": current_snapshot
        == state["standing_snapshot"],
        "registry_owned": False,
        "registry_sha256": registry_sha256,
    }


def require_inert(context, state, platform):
    status = legacy_substrate_status(context, state, platform)
    if not status["protected_staging_unchanged"]:
        fail("legacy_substrate_protected_staging_drift")
    if status["postgres_running"]:
        fail("legacy_substrate_postgres_active")
    if status["loaded_services"]:
        fail("legacy_substrate_launchd_active")
    return status


def recovery_evidence(context, state, observed_status, registry_unchanged):
    return {
        "schema_version": 1,
        "kind": LEGACY_RECOVERY_KIND,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "observed_at": utc_now(),
        "recovered_from_phase": state["phase"],
        "database_absent": not context.root.exists(),
        "postgres_process_absent": not observed_status["postgres_running"],
        "launchd_jobs_absent": not observed_status["loaded_services"],
        "keychain_items_absent": observed_status["keychain_items_present"] == 0,
        "isolated_root_absent": not context.root.exists(),
        "protected_staging_unchanged": observed_status[
            "protected_staging_unchanged"
        ],
        "discord_ownership_registry_unchanged": registry_unchanged,
    }


def command_status(context, state, platform):
    return legacy_substrate_status(context, state, platform)


def command_recover(
    context,
    state,
    platform,
    confirmed_run_id,
    confirmed_manifest_sha256,
):
    if confirmed_run_id != context.manifest["run_id"]:
        fail("legacy_substrate_run_confirmation_mismatch")
    if confirmed_manifest_sha256 != context.digest:
        fail("legacy_substrate_digest_confirmation_mismatch")
    initial = require_inert(context, state, platform)
    if state["phase"] == "cleaned":
        if (
            initial["runtime_root_present"]
            or initial["keychain_items_present"] != 0
        ):
            fail("legacy_substrate_cleaned_state_drift")
        return {
            "status": "already_recovered",
            "phase": "cleaned",
            **recovery_evidence(context, state, initial, True),
        }
    append_journal(context, "legacy_substrate_recovery", "intent", "run")
    try:
        inventory, present = keychain_state(context, platform)
        services = sorted({service for service, _account in inventory})
        for service in services:
            accounts = sorted(
                (
                    account
                    for item_service, account in present
                    if item_service == service
                ),
                key=lambda account: account == OWNER_ACCOUNT,
            )
            for account in accounts:
                platform.keychain_delete(service, account)
        if context.root.exists() or context.root.is_symlink():
            validate_runtime_root(context)
            shutil.rmtree(context.root)
    except BaseException:
        append_journal(context, "legacy_substrate_recovery", "failed", "run")
        raise
    final = require_inert(context, state, platform)
    if final["runtime_root_present"] or final["keychain_items_present"] != 0:
        append_journal(context, "legacy_substrate_recovery", "failed", "run")
        fail("legacy_substrate_recovery_incomplete")
    evidence = recovery_evidence(
        context,
        state,
        final,
        initial["registry_sha256"] == final["registry_sha256"],
    )
    if not all(
        evidence[field]
        for field in (
            "database_absent",
            "postgres_process_absent",
            "launchd_jobs_absent",
            "keychain_items_absent",
            "isolated_root_absent",
            "protected_staging_unchanged",
            "discord_ownership_registry_unchanged",
        )
    ):
        append_journal(context, "legacy_substrate_recovery", "failed", "run")
        fail("legacy_substrate_recovery_incomplete")
    write_atomic(
        context.artifact_directory / "legacy-substrate-recovery.json",
        canonical_json(evidence) + "\n",
    )
    save_state(context, "cleaned", state["standing_snapshot"])
    append_journal(context, "legacy_substrate_recovery", "complete", "run")
    return {"status": "recovered", "phase": "cleaned", **evidence}
