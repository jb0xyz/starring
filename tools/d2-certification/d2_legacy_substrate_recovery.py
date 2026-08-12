import hashlib
import json
import os
import pathlib
import re
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
from d2_orchestrator_platform import rename_exclusive


LEGACY_RECOVERY_KIND = "starring.d2.legacy-substrate-recovery.v1"
LEGACY_RECOVERY_PROGRESS_KIND = "starring.d2.legacy-substrate-recovery-progress.v1"
LEGACY_KEYCHAIN_BASELINE_KIND = (
    "starring.d2.legacy-substrate-keychain-baseline.v1"
)
LEGACY_SUBSTRATE_ALLOWLIST = frozenset(
    {
        (
            "d2-20260803t171032z-fd1232a7b31c",
            "/private/tmp/starring-d2-d2-20260803t171032z-fd1232a7b31c",
            "9d6b9d87866ecf284e2a037e0dcde472c089bf6536352aadaf1722088b934cec",
            "7669853998318333589",
            "f1ed6c8973daf2fd96f7e1b6572e3ad3616e6500d1373039812f4ed4011ca7a9",
            "3bede6a6e31548201055189114a90893298d83b39c34bab2936b22aac77ae021",
            "8d9d4b33acdbd40cc251f65b8429c88efe5c00cedf74ead722f02c9343286e2f",
        ),
        (
            "d2-20260803t202017z-3e7009e1458d",
            "/private/tmp/starring-d2-d2-20260803t202017z-3e7009e1458d",
            "2df44441e7a04efb6c606b41430ea6f8eaffd3b86c7e5e2434fe677011b9a226",
            "7669903128796247898",
            "42b261f9d9b6886668d56a2291514176df624ad366198ecebb31acaa0a7313a0",
            "fd38e2b0e704c14434d12ba4850b516600bf136b0212df48492c6831d74118c4",
            "de47b528b5b09264bd64cde2fde0bb1be9361953d6f10ed8a1410943a214c4c4",
        ),
    }
)
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


def path_metadata(path):
    try:
        return path.lstat()
    except FileNotFoundError:
        return None
    except OSError:
        fail("legacy_substrate_root_invalid")


def validate_runtime_root(context, root=None):
    selected_root = context.root if root is None else pathlib.Path(root)
    metadata = path_metadata(selected_root)
    if metadata is None:
        return None
    parent = selected_root.parent
    parent_metadata = path_metadata(parent)
    if (
        parent_metadata is None
        or not stat.S_ISDIR(parent_metadata.st_mode)
        or parent.is_symlink()
        or not stat.S_ISDIR(metadata.st_mode)
        or selected_root.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
        or metadata.st_dev != parent_metadata.st_dev
    ):
        fail("legacy_substrate_root_invalid")
    cluster_root = selected_root / "postgres"
    cluster = path_metadata(cluster_root)
    if cluster is not None:
        if (
            not stat.S_ISDIR(cluster.st_mode)
            or cluster_root.is_symlink()
            or cluster.st_uid != os.getuid()
            or stat.S_IMODE(cluster.st_mode) != 0o700
            or cluster.st_dev != metadata.st_dev
        ):
            fail("legacy_substrate_cluster_invalid")
    root_device = metadata.st_dev
    try:
        for directory, names, files in os.walk(selected_root, followlinks=False):
            for name in names + files:
                path = pathlib.Path(directory) / name
                item = path.lstat()
                if item.st_dev != root_device:
                    fail("legacy_substrate_mount_boundary_invalid")
    except OSError:
        fail("legacy_substrate_root_invalid")
    return metadata


def load_lifecycle_journal(context):
    require_owned_regular(
        context.journal_path,
        0o600,
        "legacy_substrate_journal_invalid",
        8 * 1024 * 1024,
    )
    try:
        raw = context.journal_path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        fail("legacy_substrate_journal_invalid")
    rows = []
    for sequence, line in enumerate(raw.splitlines(), 1):
        try:
            row = json.loads(line, object_pairs_hook=strict_json_object)
        except (json.JSONDecodeError, UnicodeDecodeError):
            fail("legacy_substrate_journal_invalid")
        if (
            not isinstance(row, dict)
            or set(row)
            != {
                "schema_version",
                "sequence",
                "recorded_at",
                "manifest_sha256",
                "action",
                "status",
                "target",
            }
            or row.get("schema_version") != 1
            or row.get("sequence") != sequence
            or row.get("manifest_sha256") != context.digest
            or not isinstance(row.get("recorded_at"), str)
            or not row["recorded_at"]
            or len(row["recorded_at"]) > 64
        ):
            fail("legacy_substrate_journal_invalid")
        for field in ("action", "status", "target"):
            require_identity(row.get(field), "legacy_substrate_journal_invalid")
        rows.append(row)
    if not rows:
        fail("legacy_substrate_journal_invalid")
    return rows


def load_database_evidence(context):
    evidence = load_strict_json(
        context.artifact_directory / "database-evidence.json",
        0o600,
        "legacy_substrate_database_evidence_invalid",
        1024 * 1024,
    )
    required = {
        "database_system_identifier",
        "migration_count",
        "migration_head",
        "migration_ledger_sha256",
        "relation_count",
        "capability_function_count",
    }
    if (
        not isinstance(evidence, dict)
        or set(evidence) != required
        or not isinstance(evidence.get("database_system_identifier"), str)
        or not re.fullmatch(r"[1-9][0-9]*", evidence["database_system_identifier"])
        or type(evidence.get("migration_count")) is not int
        or evidence["migration_count"] <= 0
        or not isinstance(evidence.get("migration_head"), str)
        or not re.fullmatch(r"[0-9]{12}", evidence["migration_head"])
        or not isinstance(evidence.get("migration_ledger_sha256"), str)
        or not DIGEST_PATTERN.fullmatch(evidence["migration_ledger_sha256"])
        or type(evidence.get("relation_count")) is not int
        or evidence["relation_count"] <= 0
        or type(evidence.get("capability_function_count")) is not int
        or evidence["capability_function_count"] <= 0
    ):
        fail("legacy_substrate_database_evidence_invalid")
    return evidence


def historical_provenance(context):
    rows = load_lifecycle_journal(context)
    required_prefix = (
        ("prepare", "intent", "run"),
        ("root_create", "intent", "isolated_root"),
        ("root_create", "complete", "isolated_root"),
        ("initdb", "intent", "cluster"),
        ("initdb", "complete", "cluster"),
        ("postgres_configure", "intent", "cluster"),
        ("postgres_configure", "complete", "cluster"),
    )
    observed_prefix = tuple(
        (row["action"], row["status"], row["target"])
        for row in rows[: len(required_prefix)]
    )
    if observed_prefix != required_prefix or not any(
        row["action"] == "database_bootstrap"
        and row["status"] == "complete"
        and row["target"] == "database"
        for row in rows
    ):
        fail("legacy_substrate_provenance_invalid")
    historical_rows = [
        row for row in rows if row["action"] != "legacy_substrate_recovery"
    ]
    database = load_database_evidence(context)
    projection = {
        "manifest_sha256": context.digest,
        "journal": historical_rows,
        "database_evidence": database,
    }
    return {
        "database_system_identifier": database["database_system_identifier"],
        "journal_sha256": hashlib.sha256(
            canonical_json(historical_rows).encode("utf-8")
        ).hexdigest(),
        "database_evidence_sha256": hashlib.sha256(
            canonical_json(database).encode("utf-8")
        ).hexdigest(),
        "provenance_sha256": hashlib.sha256(
            canonical_json(projection).encode("utf-8")
        ).hexdigest(),
    }


def legacy_substrate_identity(context, historical):
    root = context.root
    try:
        canonical_root = root.resolve(strict=False)
    except OSError:
        fail("legacy_substrate_identity_not_allowlisted")
    if not root.is_absolute() or canonical_root != root:
        fail("legacy_substrate_identity_not_allowlisted")
    return (
        context.manifest["run_id"],
        str(root),
        context.digest,
        historical["database_system_identifier"],
        historical["journal_sha256"],
        historical["database_evidence_sha256"],
        historical["provenance_sha256"],
    )


def require_allowlisted_legacy_substrate(context, historical):
    identity = legacy_substrate_identity(context, historical)
    if identity not in LEGACY_SUBSTRATE_ALLOWLIST:
        fail("legacy_substrate_identity_not_allowlisted")
    return identity


def quarantine_name(context):
    return f".{context.root.name}.legacy-recovery-{context.digest[:16]}"


def recovery_progress_path(context):
    return context.artifact_directory / "legacy-substrate-recovery-progress.json"


def quarantine_path(context):
    return context.root.parent / quarantine_name(context)


def load_recovery_progress(context):
    path = recovery_progress_path(context)
    if path_metadata(path) is None:
        return None
    progress = load_strict_json(
        path, 0o600, "legacy_substrate_recovery_progress_invalid", 64 * 1024
    )
    if (
        not isinstance(progress, dict)
        or set(progress)
        != {
            "schema_version",
            "kind",
            "manifest_sha256",
            "run_id",
            "provenance_sha256",
            "database_system_identifier",
            "root_device",
            "root_inode",
            "quarantine_name",
            "phase",
        }
        or progress.get("schema_version") != 1
        or progress.get("kind") != LEGACY_RECOVERY_PROGRESS_KIND
        or progress.get("manifest_sha256") != context.digest
        or progress.get("run_id") != context.manifest["run_id"]
        or not isinstance(progress.get("provenance_sha256"), str)
        or not DIGEST_PATTERN.fullmatch(progress["provenance_sha256"])
        or not isinstance(progress.get("database_system_identifier"), str)
        or not re.fullmatch(
            r"[1-9][0-9]*", progress["database_system_identifier"]
        )
        or type(progress.get("root_device")) is not int
        or progress["root_device"] < 0
        or type(progress.get("root_inode")) is not int
        or progress["root_inode"] <= 0
        or progress.get("quarantine_name") != quarantine_name(context)
        or progress.get("phase") not in {"planned", "quarantined", "deleted"}
    ):
        fail("legacy_substrate_recovery_progress_invalid")
    return progress


def save_recovery_progress(context, progress, phase):
    updated = {**progress, "phase": phase}
    write_atomic(
        recovery_progress_path(context), canonical_json(updated) + "\n"
    )
    return updated


def require_recovery_provenance(context, platform):
    historical = historical_provenance(context)
    require_allowlisted_legacy_substrate(context, historical)
    progress = load_recovery_progress(context)
    root = context.root
    quarantined = quarantine_path(context)
    root_metadata = path_metadata(root)
    quarantine_metadata = path_metadata(quarantined)
    if root_metadata is not None and quarantine_metadata is not None:
        fail("legacy_substrate_recovery_progress_invalid")
    selected = root if root_metadata is not None else quarantined
    selected_metadata = root_metadata or quarantine_metadata
    if selected_metadata is not None:
        validated = validate_runtime_root(context, selected)
        observed_identifier = platform.postgres_cluster_identity(
            selected / "postgres"
        )
        if observed_identifier != historical["database_system_identifier"]:
            fail("legacy_substrate_cluster_identity_mismatch")
        root_device = validated.st_dev
        root_inode = validated.st_ino
    elif progress is not None and progress["phase"] in {"quarantined", "deleted"}:
        observed_identifier = progress["database_system_identifier"]
        root_device = progress["root_device"]
        root_inode = progress["root_inode"]
    else:
        fail("legacy_substrate_provenance_invalid")
    if progress is not None and (
        progress["provenance_sha256"] != historical["provenance_sha256"]
        or progress["database_system_identifier"] != observed_identifier
        or progress["root_device"] != root_device
        or progress["root_inode"] != root_inode
    ):
        fail("legacy_substrate_recovery_progress_invalid")
    return {
        **historical,
        "root_device": root_device,
        "root_inode": root_inode,
    }


def ensure_recovery_progress(context, provenance):
    progress = load_recovery_progress(context)
    if progress is not None:
        return progress
    progress = {
        "schema_version": 1,
        "kind": LEGACY_RECOVERY_PROGRESS_KIND,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "provenance_sha256": provenance["provenance_sha256"],
        "database_system_identifier": provenance["database_system_identifier"],
        "root_device": provenance["root_device"],
        "root_inode": provenance["root_inode"],
        "quarantine_name": quarantine_name(context),
        "phase": "planned",
    }
    write_atomic(recovery_progress_path(context), canonical_json(progress) + "\n")
    return progress


def metadata_matches(metadata, progress):
    return (
        metadata is not None
        and stat.S_ISDIR(metadata.st_mode)
        and metadata.st_dev == progress["root_device"]
        and metadata.st_ino == progress["root_inode"]
        and metadata.st_uid == os.getuid()
    )


def legacy_launchd_labels(context):
    return tuple(
        context.manifest["services"][name]["label"]
        for name in sorted(context.manifest["services"])
    )


def legacy_launchd_status(context, platform):
    loaded = [
        name
        for name, service in sorted(context.manifest["services"].items())
        if not platform.launchd_absent(service["label"])
    ]
    overrides_absent = platform.launchd_overrides_absent(
        legacy_launchd_labels(context)
    )
    return loaded, overrides_absent


def require_legacy_launchd_inert(context, platform):
    loaded, overrides_absent = legacy_launchd_status(context, platform)
    if loaded:
        fail("legacy_substrate_launchd_active")
    if not overrides_absent:
        fail("legacy_substrate_launchd_override_present")


def remove_tree_contents(descriptor, expected_device):
    try:
        entries = list(os.scandir(descriptor))
    except OSError:
        fail("legacy_substrate_root_delete_failed")
    for entry in entries:
        try:
            before = os.stat(
                entry.name, dir_fd=descriptor, follow_symlinks=False
            )
        except OSError:
            fail("legacy_substrate_root_swap_detected")
        if before.st_dev != expected_device:
            fail("legacy_substrate_mount_boundary_invalid")
        if stat.S_ISDIR(before.st_mode):
            try:
                child = os.open(
                    entry.name,
                    os.O_RDONLY
                    | getattr(os, "O_DIRECTORY", 0)
                    | getattr(os, "O_NOFOLLOW", 0),
                    dir_fd=descriptor,
                )
            except OSError:
                fail("legacy_substrate_root_swap_detected")
            try:
                opened = os.fstat(child)
                if (
                    opened.st_dev != before.st_dev
                    or opened.st_ino != before.st_ino
                ):
                    fail("legacy_substrate_root_swap_detected")
                remove_tree_contents(child, expected_device)
            finally:
                os.close(child)
            try:
                after = os.stat(
                    entry.name, dir_fd=descriptor, follow_symlinks=False
                )
                if after.st_dev != before.st_dev or after.st_ino != before.st_ino:
                    fail("legacy_substrate_root_swap_detected")
                os.rmdir(entry.name, dir_fd=descriptor)
            except OSError:
                fail("legacy_substrate_root_swap_detected")
        else:
            try:
                after = os.stat(
                    entry.name, dir_fd=descriptor, follow_symlinks=False
                )
                if after.st_dev != before.st_dev or after.st_ino != before.st_ino:
                    fail("legacy_substrate_root_swap_detected")
                os.unlink(entry.name, dir_fd=descriptor)
            except OSError:
                fail("legacy_substrate_root_swap_detected")


def remove_quarantined_root(context, progress):
    parent_flags = (
        os.O_RDONLY
        | getattr(os, "O_DIRECTORY", 0)
        | getattr(os, "O_NOFOLLOW", 0)
    )
    try:
        parent = os.open(context.root.parent, parent_flags)
    except OSError:
        fail("legacy_substrate_root_invalid")
    try:
        try:
            before = os.stat(
                progress["quarantine_name"],
                dir_fd=parent,
                follow_symlinks=False,
            )
        except OSError:
            fail("legacy_substrate_root_swap_detected")
        if not metadata_matches(before, progress):
            fail("legacy_substrate_root_swap_detected")
        try:
            root = os.open(
                progress["quarantine_name"],
                parent_flags,
                dir_fd=parent,
            )
        except OSError:
            fail("legacy_substrate_root_swap_detected")
        try:
            opened = os.fstat(root)
            if not metadata_matches(opened, progress):
                fail("legacy_substrate_root_swap_detected")
            remove_tree_contents(root, progress["root_device"])
        finally:
            os.close(root)
        try:
            after = os.stat(
                progress["quarantine_name"],
                dir_fd=parent,
                follow_symlinks=False,
            )
            if not metadata_matches(after, progress):
                fail("legacy_substrate_root_swap_detected")
            os.rmdir(progress["quarantine_name"], dir_fd=parent)
            os.fsync(parent)
        except OSError:
            fail("legacy_substrate_root_swap_detected")
    finally:
        os.close(parent)


def recover_runtime_root(context, provenance, platform):
    require_legacy_launchd_inert(context, platform)
    progress = ensure_recovery_progress(context, provenance)
    root_metadata = path_metadata(context.root)
    quarantined = quarantine_path(context)
    quarantine_metadata = path_metadata(quarantined)
    if root_metadata is not None and quarantine_metadata is not None:
        fail("legacy_substrate_root_swap_detected")
    if root_metadata is not None:
        if progress["phase"] != "planned":
            fail("legacy_substrate_root_swap_detected")
        validated = validate_runtime_root(context)
        if not metadata_matches(validated, progress):
            fail("legacy_substrate_root_swap_detected")
        parent_flags = (
            os.O_RDONLY
            | getattr(os, "O_DIRECTORY", 0)
            | getattr(os, "O_NOFOLLOW", 0)
        )
        try:
            parent = os.open(context.root.parent, parent_flags)
        except OSError:
            fail("legacy_substrate_root_invalid")
        try:
            try:
                before = os.stat(
                    context.root.name, dir_fd=parent, follow_symlinks=False
                )
                if not metadata_matches(before, progress):
                    fail("legacy_substrate_root_swap_detected")
                rename_exclusive(
                    parent,
                    context.root.name,
                    parent,
                    progress["quarantine_name"],
                )
                after = os.stat(
                    progress["quarantine_name"],
                    dir_fd=parent,
                    follow_symlinks=False,
                )
                if not metadata_matches(after, progress):
                    fail("legacy_substrate_root_swap_detected")
                os.fsync(parent)
            except OSError:
                fail("legacy_substrate_root_swap_detected")
        finally:
            os.close(parent)
        cluster_root = quarantined / "postgres"
        require_legacy_launchd_inert(context, platform)
        if not platform.postgres_absent(cluster_root):
            fail("legacy_substrate_postgres_active")
        progress = save_recovery_progress(context, progress, "quarantined")
    elif quarantine_metadata is not None:
        if progress["phase"] not in {
            "planned",
            "quarantined",
        } or not metadata_matches(quarantine_metadata, progress):
            fail("legacy_substrate_root_swap_detected")
        cluster_root = quarantined / "postgres"
        require_legacy_launchd_inert(context, platform)
        if not platform.postgres_absent(cluster_root):
            fail("legacy_substrate_postgres_active")
        progress = save_recovery_progress(context, progress, "quarantined")
    elif progress["phase"] == "quarantined":
        return save_recovery_progress(context, progress, "deleted")
    elif progress["phase"] == "planned":
        fail("legacy_substrate_root_loss_unproven")
    if progress["phase"] == "quarantined":
        remove_quarantined_root(context, progress)
        progress = save_recovery_progress(context, progress, "deleted")
    if path_metadata(context.root) is not None or path_metadata(quarantined) is not None:
        fail("legacy_substrate_recovery_incomplete")
    return progress


def keychain_state(context, platform):
    inventory = keychain_inventory(context)
    identities = {}
    for service, account in inventory:
        identity = platform.keychain_item_identity(service, account)
        if identity is not None:
            if not isinstance(identity, str) or not DIGEST_PATTERN.fullmatch(identity):
                fail("legacy_substrate_keychain_identity_invalid")
            identities[(service, account)] = identity
    present = set(identities)
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
    return inventory, present, identities


def keychain_baseline_path(context):
    return context.artifact_directory / "legacy-substrate-keychain-baseline.json"


def validate_keychain_baseline(context, provenance, baseline):
    inventory = tuple(keychain_inventory(context))
    if (
        not isinstance(baseline, dict)
        or set(baseline)
        != {
            "schema_version",
            "kind",
            "manifest_sha256",
            "run_id",
            "provenance_sha256",
            "inventory",
        }
        or baseline.get("schema_version") != 1
        or baseline.get("kind") != LEGACY_KEYCHAIN_BASELINE_KIND
        or baseline.get("manifest_sha256") != context.digest
        or baseline.get("run_id") != context.manifest["run_id"]
        or baseline.get("provenance_sha256")
        != provenance["provenance_sha256"]
        or not isinstance(baseline.get("inventory"), list)
        or len(baseline["inventory"]) != len(inventory)
    ):
        fail("legacy_substrate_keychain_baseline_invalid")
    observed_inventory = []
    identities = {}
    for entry in baseline["inventory"]:
        if (
            not isinstance(entry, dict)
            or set(entry) != {"service", "account", "identity_sha256"}
            or not isinstance(entry.get("service"), str)
            or not isinstance(entry.get("account"), str)
            or (
                entry.get("identity_sha256") is not None
                and (
                    not isinstance(entry["identity_sha256"], str)
                    or not DIGEST_PATTERN.fullmatch(entry["identity_sha256"])
                )
            )
        ):
            fail("legacy_substrate_keychain_baseline_invalid")
        identity = (entry["service"], entry["account"])
        if identity in identities:
            fail("legacy_substrate_keychain_baseline_invalid")
        observed_inventory.append(identity)
        identities[identity] = entry["identity_sha256"]
    if tuple(observed_inventory) != inventory:
        fail("legacy_substrate_keychain_baseline_invalid")
    for service in {service for service, _account in inventory}:
        service_identities = {
            account: identities[(service, account)]
            for item_service, account in inventory
            if item_service == service
        }
        if any(value is not None for value in service_identities.values()) and (
            service_identities.get(OWNER_ACCOUNT) is None
        ):
            fail("legacy_substrate_keychain_baseline_invalid")
    return identities


def load_keychain_baseline(context, provenance):
    return validate_keychain_baseline(
        context,
        provenance,
        load_strict_json(
            keychain_baseline_path(context),
            0o600,
            "legacy_substrate_keychain_baseline_invalid",
            256 * 1024,
        ),
    )


def load_or_create_keychain_baseline(context, platform, provenance):
    path = keychain_baseline_path(context)
    if path_metadata(path) is None:
        inventory, _present, observed = keychain_state(context, platform)
        baseline = {
            "schema_version": 1,
            "kind": LEGACY_KEYCHAIN_BASELINE_KIND,
            "manifest_sha256": context.digest,
            "run_id": context.manifest["run_id"],
            "provenance_sha256": provenance["provenance_sha256"],
            "inventory": [
                {
                    "service": service,
                    "account": account,
                    "identity_sha256": observed.get((service, account)),
                }
                for service, account in inventory
            ],
        }
        validate_keychain_baseline(context, provenance, baseline)
        write_atomic(path, canonical_json(baseline) + "\n")
    return load_keychain_baseline(context, provenance)


def validate_keychain_baseline_replay(context, platform, baseline):
    inventory, _present, current_present = keychain_state(context, platform)
    current = {
        identity: current_present.get(identity) for identity in inventory
    }
    for identity, original in baseline.items():
        observed = current[identity]
        if original is None:
            if observed is not None:
                fail("legacy_substrate_keychain_identity_drift")
        elif observed not in {None, original}:
            fail("legacy_substrate_keychain_identity_drift")
    return current


def recover_keychain_items(context, platform, provenance):
    baseline = load_or_create_keychain_baseline(
        context, platform, provenance
    )
    services = sorted({service for service, _account in baseline})
    for service in services:
        accounts = sorted(
            (
                account
                for (item_service, account), original in baseline.items()
                if item_service == service and original is not None
            ),
            key=lambda account: account == OWNER_ACCOUNT,
        )
        for account in accounts:
            current = validate_keychain_baseline_replay(
                context, platform, baseline
            )
            target = (service, account)
            if current[target] is None:
                continue
            if not platform.keychain_owner_matches(
                service, context.manifest["run_id"]
            ):
                fail("legacy_substrate_keychain_ownership_invalid")
            platform.keychain_delete_exact(
                service, account, baseline[target]
            )
            if platform.keychain_item_identity(service, account) is not None:
                fail("legacy_substrate_keychain_delete_unconfirmed")
    current = validate_keychain_baseline_replay(context, platform, baseline)
    if any(identity is not None for identity in current.values()):
        fail("legacy_substrate_keychain_delete_unconfirmed")


def require_keychain_baseline_absent(context, platform, provenance):
    baseline = load_keychain_baseline(context, provenance)
    current = validate_keychain_baseline_replay(context, platform, baseline)
    if any(identity is not None for identity in current.values()):
        fail("legacy_substrate_keychain_delete_unconfirmed")


def legacy_substrate_status(context, state, platform):
    validate_runtime_root(context)
    progress = load_recovery_progress(context)
    quarantined = quarantine_path(context)
    quarantine_metadata = path_metadata(quarantined)
    if quarantine_metadata is not None:
        if progress is None or not metadata_matches(quarantine_metadata, progress):
            fail("legacy_substrate_recovery_progress_invalid")
        if path_metadata(context.root) is not None:
            fail("legacy_substrate_root_swap_detected")
        validate_runtime_root(context, quarantined)
    registry_sha256 = require_registry_unowned(context)
    inventory, present, _identities = keychain_state(context, platform)
    current_snapshot = standing_snapshot(context, platform)
    loaded, overrides_absent = legacy_launchd_status(context, platform)
    cluster_root = (
        quarantined / "postgres"
        if quarantine_metadata is not None
        else context.cluster_root
    )
    postgres_running = path_metadata(cluster_root) is not None and not platform.postgres_absent(
        cluster_root
    )
    return {
        "status": "observed",
        "phase": state["phase"],
        "run_id": context.manifest["run_id"],
        "manifest_sha256": context.digest,
        "runtime_root_present": context.root.exists(),
        "quarantine_present": quarantine_metadata is not None,
        "postgres_running": postgres_running,
        "loaded_services": loaded,
        "launchd_overrides_absent": overrides_absent,
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
    if not status["launchd_overrides_absent"]:
        fail("legacy_substrate_launchd_override_present")
    return status


def recovery_evidence(
    context, state, observed_status, registry_unchanged, provenance
):
    root_absent = (
        path_metadata(context.root) is None
        and path_metadata(quarantine_path(context)) is None
    )
    return {
        "schema_version": 1,
        "kind": LEGACY_RECOVERY_KIND,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "observed_at": utc_now(),
        "recovered_from_phase": state["phase"],
        "database_absent": root_absent,
        "postgres_process_absent": not observed_status["postgres_running"],
        "launchd_jobs_absent": (
            not observed_status["loaded_services"]
            and observed_status["launchd_overrides_absent"]
        ),
        "keychain_items_absent": observed_status["keychain_items_present"] == 0,
        "isolated_root_absent": root_absent,
        "quarantine_absent": root_absent,
        "protected_staging_unchanged": observed_status[
            "protected_staging_unchanged"
        ],
        "discord_ownership_registry_unchanged": registry_unchanged,
        "provenance_sha256": provenance["provenance_sha256"],
        "database_system_identifier": provenance["database_system_identifier"],
        "root_device": provenance["root_device"],
        "root_inode": provenance["root_inode"],
        "keychain_identity_boundary": (
            "persistent-reference-baseline-reobserved-before-each-delete"
        ),
    }


def load_recovery_evidence(context, provenance):
    evidence = load_strict_json(
        context.artifact_directory / "legacy-substrate-recovery.json",
        0o600,
        "legacy_substrate_recovery_evidence_invalid",
        256 * 1024,
    )
    required = {
        "schema_version",
        "kind",
        "manifest_sha256",
        "run_id",
        "observed_at",
        "recovered_from_phase",
        "database_absent",
        "postgres_process_absent",
        "launchd_jobs_absent",
        "keychain_items_absent",
        "isolated_root_absent",
        "quarantine_absent",
        "protected_staging_unchanged",
        "discord_ownership_registry_unchanged",
        "provenance_sha256",
        "database_system_identifier",
        "root_device",
        "root_inode",
        "keychain_identity_boundary",
    }
    if (
        not isinstance(evidence, dict)
        or set(evidence) != required
        or evidence.get("schema_version") != 1
        or evidence.get("kind") != LEGACY_RECOVERY_KIND
        or evidence.get("manifest_sha256") != context.digest
        or evidence.get("run_id") != context.manifest["run_id"]
        or not isinstance(evidence.get("observed_at"), str)
        or not evidence["observed_at"]
        or evidence.get("recovered_from_phase") not in LEGACY_RECOVERY_PHASES - {"cleaned"}
        or evidence.get("provenance_sha256") != provenance["provenance_sha256"]
        or evidence.get("database_system_identifier")
        != provenance["database_system_identifier"]
        or evidence.get("root_device") != provenance["root_device"]
        or evidence.get("root_inode") != provenance["root_inode"]
        or evidence.get("keychain_identity_boundary")
        != "persistent-reference-baseline-reobserved-before-each-delete"
        or not all(
            evidence.get(field) is True
            for field in (
                "database_absent",
                "postgres_process_absent",
                "launchd_jobs_absent",
                "keychain_items_absent",
                "isolated_root_absent",
                "quarantine_absent",
                "protected_staging_unchanged",
                "discord_ownership_registry_unchanged",
            )
        )
    ):
        fail("legacy_substrate_recovery_evidence_invalid")
    return evidence


def ensure_recovery_journal_complete(context):
    rows = load_lifecycle_journal(context)
    for row in rows:
        if row["action"] == "legacy_substrate_recovery" and (
            row["target"] != "run"
            or row["status"] not in {"intent", "failed", "complete"}
        ):
            fail("legacy_substrate_recovery_journal_incomplete")
    recovery_rows = [
        (index, row)
        for index, row in enumerate(rows)
        if row["action"] == "legacy_substrate_recovery"
        and row["target"] == "run"
    ]
    intents = [index for index, row in recovery_rows if row["status"] == "intent"]
    if not intents:
        fail("legacy_substrate_recovery_journal_incomplete")
    last_intent = intents[-1]
    if (
        recovery_rows[-1][0] <= last_intent
        or recovery_rows[-1][1]["status"] != "complete"
    ):
        append_journal(context, "legacy_substrate_recovery", "complete", "run")
        rows = load_lifecycle_journal(context)
        if rows[-1]["action"] != "legacy_substrate_recovery" or rows[-1][
            "status"
        ] != "complete":
            fail("legacy_substrate_recovery_journal_incomplete")


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
            or initial["quarantine_present"]
            or initial["keychain_items_present"] != 0
        ):
            fail("legacy_substrate_cleaned_state_drift")
        provenance = require_recovery_provenance(context, platform)
        evidence = load_recovery_evidence(context, provenance)
        require_keychain_baseline_absent(context, platform, provenance)
        progress = load_recovery_progress(context)
        if progress is None:
            fail("legacy_substrate_recovery_progress_invalid")
        if progress["phase"] == "quarantined":
            progress = save_recovery_progress(context, progress, "deleted")
        if progress["phase"] != "deleted":
            fail("legacy_substrate_recovery_progress_invalid")
        ensure_recovery_journal_complete(context)
        return {
            "status": "already_recovered",
            "phase": "cleaned",
            **evidence,
        }
    provenance = require_recovery_provenance(context, platform)
    ensure_recovery_progress(context, provenance)
    append_journal(context, "legacy_substrate_recovery", "intent", "run")
    try:
        recover_keychain_items(context, platform, provenance)
        require_inert(context, state, platform)
        recover_runtime_root(context, provenance, platform)
    except BaseException:
        append_journal(context, "legacy_substrate_recovery", "failed", "run")
        raise
    final = require_inert(context, state, platform)
    if (
        final["runtime_root_present"]
        or final["quarantine_present"]
        or final["keychain_items_present"] != 0
    ):
        append_journal(context, "legacy_substrate_recovery", "failed", "run")
        fail("legacy_substrate_recovery_incomplete")
    evidence = recovery_evidence(
        context,
        state,
        final,
        initial["registry_sha256"] == final["registry_sha256"],
        provenance,
    )
    if not all(
        evidence[field]
        for field in (
            "database_absent",
            "postgres_process_absent",
            "launchd_jobs_absent",
            "keychain_items_absent",
            "isolated_root_absent",
            "quarantine_absent",
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
