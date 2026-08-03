#!/usr/bin/env python3

import argparse
import dataclasses
import datetime
import fcntl
import hashlib
import json
import os
import pathlib
import re
import secrets
import stat
import sys
import urllib.parse
import uuid


SCHEMA_VERSION = 1
MAX_JSON_BYTES = 256 * 1024
MAX_STRING_BYTES = 4096
MAX_COLLECTION_ITEMS = 256
MAX_NESTING_DEPTH = 8
COMMIT_PATTERN = re.compile(r"^[0-9a-f]{40}$")
DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")
ID_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9:._-]{0,191}$")
SNOWFLAKE_PATTERN = re.compile(r"^[1-9][0-9]{0,19}$")
RUN_ID_PATTERN = re.compile(r"^d2-[0-9]{8}t[0-9]{6}z-[0-9a-f]{12}$")
MIGRATION_PATTERN = re.compile(r"^[0-9]{12}$")
UTC_TIMESTAMP_PATTERN = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")
TRANSPORT_INSTANCE_PATTERN = re.compile(r"^d2ti-[0-9a-f]{32}$")
ZERO_DIGEST = "0" * 64
FORBIDDEN_EVIDENCE_KEYS = {
    "access_token",
    "assistant_message",
    "authorization_code",
    "bot_token",
    "cookie",
    "csrf_token",
    "database_url",
    "full_user_transcript",
    "key_material",
    "material_digest",
    "oauth_code",
    "oauth_token",
    "password",
    "prompt",
    "refresh_token",
    "secret",
    "session_cookie",
    "session_credential",
    "user_message",
    "worker_token",
}
FORBIDDEN_VALUE_PATTERNS = (
    re.compile(r"postgres(?:ql)?://[^\s]+", re.IGNORECASE),
    re.compile(r"(?:Bearer|Bot)\s+[A-Za-z0-9._~-]+", re.IGNORECASE),
    re.compile(r"-----BEGIN " + r"AGE ENCRYPTED FILE-----"),
    re.compile(r"\bcf(?:at|ut)_[A-Za-z0-9_-]+\b"),
)
REQUIRED_CANDIDATES = {
    "api",
    "runtime",
    "codex_worker",
    "codex",
    "db_bootstrap",
    "sealed_provisioner",
    "certification_transport",
    "node",
    "cloudflared",
}
REQUIRED_PORTS = {
    "postgres",
    "api",
    "runtime",
    "worker",
    "transport_gateway",
    "transport_http",
}
CODEX_WORKER_SOURCE_FILES = (
    "admission-registry.mjs",
    "codex-runner.mjs",
    "metrics-log.mjs",
    "protocol.mjs",
    "request-timeline.mjs",
    "scheduler.mjs",
    "worker.mjs",
)
D2_TOOLCHAIN_SOURCE_FILES = (
    "d2_certification.py",
    "d2_orchestrator_composition.py",
    "d2_orchestrator_contract.py",
    "d2_orchestrator_platform.py",
    "d2_drained_runtime_restart.py",
    "isolated_orchestrator.py",
    "product_driver.js",
)
CERTIFICATION_TRANSPORT_SOURCE_FILES = (
    ".gitignore",
    "Cargo.lock",
    "Cargo.toml",
    "src/config.rs",
    "src/control.rs",
    "src/gateway.rs",
    "src/http_proxy.rs",
    "src/lib.rs",
    "src/main.rs",
    "src/state.rs",
)
AUTHORING_CONFIG = {
    "provider": "codex_chatgpt",
    "model": "gpt-5.6-luna",
    "reasoning_effort": "medium",
    "auth_mode": "chatgpt",
}
D2_PUBLIC_ORIGIN = "https://d2-api.starring.co.kr"
D2_CLOUDFLARE_TUNNEL_ID = "57c22e8a-0ec2-4f67-a882-2c355b0348df"
D2_API_PORT = 28080
D2_ORIGIN_SERVICE = f"http://127.0.0.1:{D2_API_PORT}"


class CertificationError(Exception):
    pass


@dataclasses.dataclass(frozen=True)
class StepSpec:
    code: str
    required: tuple[str, ...]


STEP_SPECS = {
    1: StepSpec(
        "isolated_target_created",
        (
            "database_system_identifier",
            "migration_count",
            "migration_head",
            "migration_ledger_sha256",
            "discord_resource_prefix",
        ),
    ),
    2: StepSpec(
        "prior_guild_ownership_absent",
        ("prior_runtime_owner_count", "prior_smoke_process_count"),
    ),
    3: StepSpec(
        "candidate_processes_started",
        (
            "api_sha256",
            "runtime_sha256",
            "codex_worker_sha256",
            "d2_toolchain_sha256",
            "certification_transport_sha256",
            "certification_transport_source_sha256",
            "api_build_revision",
            "runtime_build_revision",
            "api_ready_status",
            "runtime_ready_status",
            "worker_ready_status",
            "cloudflare_tunnel_id",
            "public_origin",
            "origin_service",
            "transport_instance_id",
            "transport_ready",
            "tunnel_ready",
        ),
    ),
    4: StepSpec(
        "oauth_authenticated",
        (
            "oauth_callback_status",
            "me_status",
            "principal_id",
            "installation_id",
            "guild_id",
            "authority_check_status",
        ),
    ),
    5: StepSpec(
        "one_shot_authoring_submitted",
        (
            "authoring_http_status",
            "authoring_session_id",
            "authoring_generation",
            "installation_id",
            "model",
            "provider",
            "reasoning_effort",
            "auth_mode",
            "one_shot",
        ),
    ),
    6: StepSpec(
        "encrypted_preview_ready",
        (
            "generation_encrypted",
            "projection_state",
            "generation",
            "payload_digest",
            "installation_id",
            "authoring_session_id",
        ),
    ),
    7: StepSpec(
        "product_decisions_applied",
        (
            "installation_id",
            "promotion_id",
            "preview_state",
            "approval_state",
            "apply_state",
        ),
    ),
    8: StepSpec(
        "runtime_live",
        (
            "pending_observed",
            "live_observed",
            "installation_id",
            "promotion_id",
            "deployment_id",
            "route_id",
            "attestation_id",
            "serving_lease_id",
        ),
    ),
    9: StepSpec(
        "create_and_join_executed",
        (
            "create_interaction_id",
            "join_interaction_id",
            "deployment_id",
            "route_id",
            "instance_id",
            "role_ids",
            "channel_ids",
            "panel_message_ids",
            "ephemeral_count",
        ),
    ),
    10: StepSpec(
        "duplicate_interaction_suppressed",
        (
            "interaction_id",
            "delivery_count",
            "external_effect_count",
            "receipt_state",
            "transport_duplicate_injections",
            "transport_duplicate_delivery_count",
            "transport_last_duplicate_interaction_id",
            "transport_instance_id",
        ),
    ),
    11: StepSpec(
        "runtime_restarted",
        (
            "old_pid",
            "new_pid",
            "runtime_sha256",
            "ready_after_restart",
            "checkpoint",
            "deployment_id",
            "route_id",
            "instance_id",
        ),
    ),
    12: StepSpec(
        "route_and_instance_reconstructed",
        (
            "route_reconstructed",
            "instance_reconstructed",
            "deployment_id",
            "route_id",
            "instance_id",
            "pinned_ruleset_digest",
        ),
    ),
    13: StepSpec(
        "indeterminate_effect_reconciled",
        (
            "effect_id",
            "interaction_id",
            "route_id",
            "injected_outcome",
            "reconciliation_state",
            "duplicate_external_effect_count",
            "unsafe_deletion_count",
            "transport_indeterminate_injections",
            "transport_last_audit_reason_sha256",
            "transport_last_upstream_status",
            "transport_instance_id",
        ),
    ),
    14: StepSpec(
        "target_replaced",
        (
            "replacement_target_id",
            "replacement_kind",
            "source_deployment_id",
            "source_route_id",
            "replacement_deployment_id",
            "replacement_route_id",
            "previous_target_drained",
            "replacement_live",
            "prior_route_absent",
        ),
    ),
    15: StepSpec(
        "gateway_disconnect_failed_closed",
        (
            "gateway_disconnected",
            "live_lost",
            "runtime_ready_status",
            "public_code",
            "route_id",
            "transport_gateway_partitioned",
            "transport_gateway_partition_events",
            "transport_instance_id",
        ),
    ),
    16: StepSpec(
        "test_resources_torn_down",
        (
            "teardown_started",
            "discord_resource_ids_deleted",
            "database_drop_requested",
            "services_stopped",
        ),
    ),
    17: StepSpec(
        "total_absence_confirmed",
        (
            "unresolved_operation_count",
            "unresolved_receipt_count",
            "unresolved_journal_count",
            "route_count",
            "instance_count",
            "role_count",
            "channel_count",
            "panel_count",
            "resource_prefix_match_count",
            "database_absent",
            "postgres_process_absent",
            "launchd_jobs_absent",
            "keychain_items_absent",
            "discord_guild_deleted",
        ),
    ),
}


def fail(message):
    raise CertificationError(message)


def canonical_json(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def sha256_bytes(value):
    return hashlib.sha256(value).hexdigest()


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while True:
            chunk = handle.read(1024 * 1024)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def source_tree_digest(root, files, label):
    root = require_absolute_path(str(root), f"{label}_root")
    try:
        metadata = root.lstat()
    except OSError as error:
        fail(f"{label}_root_unavailable:{error.__class__.__name__}")
    if not stat.S_ISDIR(metadata.st_mode) or root.is_symlink():
        fail(f"{label}_root_invalid")
    if metadata.st_uid != os.getuid() or stat.S_IMODE(metadata.st_mode) & 0o022:
        fail(f"{label}_root_ownership_invalid")
    digest = hashlib.sha256()
    for name in files:
        path = root / name
        file_metadata = require_regular_file(path, f"{label}_{name}")
        if file_metadata.st_uid != os.getuid() or stat.S_IMODE(file_metadata.st_mode) & 0o022:
            fail(f"{label}_{name}_writable")
        content = path.read_bytes()
        encoded_name = name.encode("utf-8")
        digest.update(str(len(encoded_name)).encode("ascii"))
        digest.update(b":")
        digest.update(encoded_name)
        digest.update(b":")
        digest.update(str(len(content)).encode("ascii"))
        digest.update(b":")
        digest.update(content)
    return digest.hexdigest()


def validate_codex_worker_inventory(root):
    try:
        observed = {
            path.name
            for path in root.iterdir()
            if path.name.endswith(".mjs") and not path.name.endswith(".test.mjs")
        }
    except OSError as error:
        fail(f"codex_worker_inventory_unavailable:{error.__class__.__name__}")
    if observed != set(CODEX_WORKER_SOURCE_FILES):
        fail("codex_worker_inventory_invalid")


def validate_certification_transport_inventory(root):
    try:
        observed = set()
        for path in root.iterdir():
            if path.name == "target":
                continue
            if path.name == "src" and path.is_dir() and not path.is_symlink():
                observed.update(
                    child.relative_to(root).as_posix() for child in path.iterdir()
                )
            else:
                observed.add(path.relative_to(root).as_posix())
    except OSError as error:
        fail(
            f"certification_transport_inventory_unavailable:{error.__class__.__name__}"
        )
    if observed != set(CERTIFICATION_TRANSPORT_SOURCE_FILES):
        fail("certification_transport_inventory_invalid")


def source_tree_manifest(root, files, label):
    return {
        "root": str(root),
        "files": list(files),
        "sha256": source_tree_digest(root, files, label),
    }


def require_regular_file(path, label, allow_empty=False):
    try:
        metadata = path.lstat()
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    if not stat.S_ISREG(metadata.st_mode) or path.is_symlink():
        fail(f"{label}_not_regular")
    if not allow_empty and metadata.st_size == 0:
        fail(f"{label}_empty")
    return metadata


def require_owned_mode(path, mode, label, directory=False):
    try:
        metadata = path.lstat()
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    expected_kind = stat.S_ISDIR if directory else stat.S_ISREG
    if (
        not expected_kind(metadata.st_mode)
        or path.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != mode
    ):
        fail(f"{label}_ownership_invalid")
    return metadata


def require_absolute_path(raw, label):
    path = pathlib.Path(raw)
    if not path.is_absolute():
        fail(f"{label}_not_absolute")
    if ".." in path.parts:
        fail(f"{label}_parent_component")
    return path


def parse_named_values(raw_values, required, label, value_parser):
    parsed = {}
    for raw in raw_values:
        name, separator, value = raw.partition("=")
        if not separator or name not in required or name in parsed:
            fail(f"{label}_invalid")
        parsed[name] = value_parser(value, name)
    if set(parsed) != required:
        fail(f"{label}_incomplete")
    return parsed


def parse_candidate(raw, name):
    path = require_absolute_path(raw, f"candidate_{name}")
    require_immutable_candidate(path, name)
    return path


def require_immutable_candidate(path, name):
    metadata = require_regular_file(path, f"candidate_{name}")
    if metadata.st_uid != os.getuid():
        fail(f"candidate_{name}_owner_invalid")
    if stat.S_IMODE(metadata.st_mode) & 0o222:
        fail(f"candidate_{name}_writable")
    try:
        parent = path.parent.lstat()
        resolved = path.resolve(strict=True)
    except OSError as error:
        fail(f"candidate_{name}_unavailable:{error.__class__.__name__}")
    if (
        not stat.S_ISDIR(parent.st_mode)
        or path.parent.is_symlink()
        or parent.st_uid != metadata.st_uid
        or stat.S_IMODE(parent.st_mode) & 0o222
    ):
        fail(f"candidate_{name}_directory_mutable")
    if resolved != path:
        fail(f"candidate_{name}_path_not_canonical")
    return metadata


def parse_port(raw, name):
    try:
        port = int(raw)
    except ValueError:
        fail(f"port_{name}_invalid")
    if port < 1024 or port > 65535:
        fail(f"port_{name}_invalid")
    return port


def validate_public_origin(raw):
    if not isinstance(raw, str):
        fail("public_origin_invalid")
    parsed = urllib.parse.urlsplit(raw)
    hostname = parsed.hostname
    if hostname and hostname.endswith("."):
        hostname = hostname[:-1]
    if (
        parsed.scheme != "https"
        or not hostname
        or hostname.endswith(".")
        or ":" in hostname
        or parsed.username is not None
        or parsed.password is not None
        or parsed.path not in ("", "/")
        or parsed.query
        or parsed.fragment
    ):
        fail("public_origin_invalid")
    try:
        port = parsed.port
    except ValueError:
        fail("public_origin_invalid")
    normalized = f"https://{hostname}"
    if port is not None and port != 443:
        normalized = f"{normalized}:{port}"
    return normalized


def validate_cloudflare_tunnel_id(raw):
    if not isinstance(raw, str):
        fail("cloudflare_tunnel_id_invalid")
    try:
        parsed = uuid.UUID(raw)
    except (ValueError, AttributeError):
        fail("cloudflare_tunnel_id_invalid")
    if str(parsed) != raw or parsed.version != 4:
        fail("cloudflare_tunnel_id_invalid")
    return raw


def validate_cloudflare_route_binding(tunnel_id, public_origin, api_port):
    if (
        tunnel_id != D2_CLOUDFLARE_TUNNEL_ID
        or public_origin != D2_PUBLIC_ORIGIN
        or api_port != D2_API_PORT
    ):
        fail("cloudflare_route_binding_invalid")
    return {
        "tunnel_id": tunnel_id,
        "public_origin": public_origin,
        "origin_service": D2_ORIGIN_SERVICE,
    }


def validate_commit(raw):
    if not isinstance(raw, str) or not COMMIT_PATTERN.fullmatch(raw):
        fail("commit_invalid")
    return raw


def validate_snowflake(raw, label):
    if (
        not isinstance(raw, str)
        or not SNOWFLAKE_PATTERN.fullmatch(raw)
        or int(raw) > 18446744073709551615
    ):
        fail(f"{label}_invalid")
    return raw


def validate_keychain_identity(raw, label):
    if not isinstance(raw, str):
        fail(f"{label}_invalid")
    service, separator, account = raw.partition(":")
    identity_pattern = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,191}$")
    if (
        not separator
        or ":" in account
        or not identity_pattern.fullmatch(service)
        or not identity_pattern.fullmatch(account)
    ):
        fail(f"{label}_invalid")
    return {"service": service, "account": account}


def validate_run_id(raw):
    if not isinstance(raw, str) or not RUN_ID_PATTERN.fullmatch(raw):
        fail("run_id_invalid")
    return raw


def validate_utc_timestamp(raw):
    if not isinstance(raw, str) or not UTC_TIMESTAMP_PATTERN.fullmatch(raw):
        return False
    try:
        datetime.datetime.strptime(raw, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError:
        return False
    return True


def generate_run_id(now=None):
    instant = now or datetime.datetime.now(datetime.timezone.utc)
    timestamp = instant.strftime("%Y%m%dt%H%M%Sz")
    return f"d2-{timestamp}-{secrets.token_hex(6)}"


def write_new_file(path, value, mode=0o600):
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    descriptor = os.open(path, flags, mode)
    try:
        os.fchmod(descriptor, mode)
        payload = value.encode("utf-8")
        written = 0
        while written < len(payload):
            count = os.write(descriptor, payload[written:])
            if count <= 0:
                fail("file_write_failed")
            written += count
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def fsync_directory(path, label):
    try:
        metadata = path.lstat()
    except OSError as error:
        fail(f"{label}_unavailable:{error.__class__.__name__}")
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or path.is_symlink()
        or metadata.st_uid != os.getuid()
    ):
        fail(f"{label}_invalid")
    flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        flags |= os.O_DIRECTORY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        fail(f"{label}_open_failed:{error.__class__.__name__}")
    try:
        opened = os.fstat(descriptor)
        if (
            not stat.S_ISDIR(opened.st_mode)
            or opened.st_uid != os.getuid()
            or (opened.st_dev, opened.st_ino) != (metadata.st_dev, metadata.st_ino)
        ):
            fail(f"{label}_identity_changed")
        os.fsync(descriptor)
    except OSError as error:
        fail(f"{label}_fsync_failed:{error.__class__.__name__}")
    finally:
        os.close(descriptor)


def manifest_digest(manifest):
    return sha256_bytes(canonical_json(manifest).encode("utf-8"))


def build_manifest(arguments):
    commit = validate_commit(arguments.commit)
    run_id = validate_run_id(arguments.run_id or generate_run_id())
    guild_id = validate_snowflake(arguments.discord_guild_id, "discord_guild_id")
    hub_channel_id = validate_snowflake(
        arguments.discord_hub_channel_id, "discord_hub_channel_id"
    )
    application_id = validate_snowflake(
        arguments.discord_application_id, "discord_application_id"
    )
    bot_user_id = validate_snowflake(
        arguments.discord_bot_user_id, "discord_bot_user_id"
    )
    actor_id = validate_snowflake(arguments.discord_actor_id, "discord_actor_id")
    if hub_channel_id in {guild_id, application_id, bot_user_id, actor_id}:
        fail("discord_hub_channel_identity_invalid")
    candidates = parse_named_values(
        arguments.candidate, REQUIRED_CANDIDATES, "candidates", parse_candidate
    )
    ports = parse_named_values(arguments.port, REQUIRED_PORTS, "ports", parse_port)
    if len(set(ports.values())) != len(ports):
        fail("ports_not_unique")
    origin = validate_public_origin(arguments.public_origin)
    tunnel_id = validate_cloudflare_tunnel_id(arguments.cloudflare_tunnel_id)
    cloudflare = validate_cloudflare_route_binding(tunnel_id, origin, ports["api"])
    discord_oauth_identity = validate_keychain_identity(
        arguments.discord_oauth_keychain, "discord_oauth_keychain"
    )
    discord_bot_identity = validate_keychain_identity(
        arguments.discord_bot_keychain, "discord_bot_keychain"
    )
    tunnel_token_identity = validate_keychain_identity(
        arguments.tunnel_token_keychain, "tunnel_token_keychain"
    )
    suffix = run_id.rsplit("-", 1)[1]
    prefix = f"starring-d2-{run_id[3:11]}-{suffix}"
    root = pathlib.Path(f"/private/tmp/starring-d2-{run_id}")
    candidate_manifest = {
        name: {"path": str(path), "sha256": sha256_file(path)}
        for name, path in sorted(candidates.items())
    }
    worker_root = candidates["codex_worker"].parent
    if candidates["codex_worker"].name != "worker.mjs":
        fail("candidate_codex_worker_entrypoint_invalid")
    validate_codex_worker_inventory(worker_root)
    toolchain_root = pathlib.Path(__file__).resolve().parent
    transport_root = toolchain_root.parent / "d2-certification-transport"
    validate_certification_transport_inventory(transport_root)
    return {
        "schema_version": SCHEMA_VERSION,
        "run_id": run_id,
        "created_at": datetime.datetime.now(datetime.timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z"),
        "commit_sha": commit,
        "authoring": dict(AUTHORING_CONFIG),
        "public_origin": origin,
        "cloudflare": cloudflare,
        "candidates": candidate_manifest,
        "source_trees": {
            "codex_worker": source_tree_manifest(
                worker_root, CODEX_WORKER_SOURCE_FILES, "codex_worker_tree"
            ),
            "d2_toolchain": source_tree_manifest(
                toolchain_root, D2_TOOLCHAIN_SOURCE_FILES, "d2_toolchain_tree"
            ),
            "certification_transport": source_tree_manifest(
                transport_root,
                CERTIFICATION_TRANSPORT_SOURCE_FILES,
                "certification_transport_tree",
            ),
        },
        "database": {
            "name": "starring_runtime_staging",
            "cluster_root": str(root / "postgres"),
            "socket_directory": str(root / "socket"),
            "port": ports["postgres"],
        },
        "discord": {
            "guild_id": guild_id,
            "hub_channel_id": hub_channel_id,
            "application_id": application_id,
            "bot_user_id": bot_user_id,
            "actor_id": actor_id,
            "resource_prefix": prefix,
            "disposable_guild_required": True,
        },
        "services": {
            "api": {
                "label": f"local.starring.d2.{suffix}.api",
                "port": ports["api"],
            },
            "runtime": {
                "label": f"local.starring.d2.{suffix}.runtime",
                "port": ports["runtime"],
            },
            "worker": {
                "label": f"local.starring.d2.{suffix}.worker",
                "port": ports["worker"],
            },
            "transport": {
                "label": f"local.starring.d2.{suffix}.transport",
                "gateway_port": ports["transport_gateway"],
                "http_port": ports["transport_http"],
            },
            "tunnel": {"label": f"local.starring.d2.{suffix}.tunnel"},
        },
        "keychain_services": {
            "api": f"starring.d2.{suffix}.api",
            "runtime": f"starring.d2.{suffix}.runtime",
            "postgres": f"starring.d2.{suffix}.postgres",
            "worker": f"starring.d2.{suffix}.worker",
        },
        "external_keychain": {
            "discord_oauth_client_secret": discord_oauth_identity,
            "discord_bot_token": discord_bot_identity,
            "tunnel_token": tunnel_token_identity,
        },
        "protected_staging": {
            "database": "starring_runtime_staging@127.0.0.1:5432",
            "launchd_labels": [
                "local.starring.api.staging",
                "local.starring.codex-worker",
                "local.starring.runtime.staging",
                "local.cloudflared.starring",
            ],
            "mutation_allowed": False,
        },
        "human_boundaries": [
            "create_disposable_discord_guild",
            "complete_discord_oauth",
            "execute_real_discord_interactions",
            "delete_disposable_discord_guild",
        ],
        "expected_steps": [
            {"step": number, "code": specification.code}
            for number, specification in STEP_SPECS.items()
        ],
    }


def command_prepare(arguments):
    output_root = require_absolute_path(arguments.output_root, "output_root")
    if not output_root.exists():
        output_root.mkdir(mode=0o700, parents=True)
        fsync_directory(output_root.parent, "output_root_parent")
    require_owned_mode(output_root, 0o700, "output_root", directory=True)
    manifest = build_manifest(arguments)
    run_directory = output_root / manifest["run_id"]
    run_directory.mkdir(mode=0o700)
    require_owned_mode(run_directory, 0o700, "run_directory", directory=True)
    fsync_directory(output_root, "output_root")
    manifest_path = run_directory / "manifest.json"
    digest_path = run_directory / "manifest.sha256"
    receipts_path = run_directory / "receipts.jsonl"
    payload = canonical_json(manifest) + "\n"
    write_new_file(manifest_path, payload)
    write_new_file(digest_path, manifest_digest(manifest) + "\n")
    write_new_file(receipts_path, "")
    fsync_directory(run_directory, "run_directory")
    print(
        canonical_json(
            {
                "run_id": manifest["run_id"],
                "manifest": str(manifest_path),
                "receipts": str(receipts_path),
                "resource_prefix": manifest["discord"]["resource_prefix"],
            }
        )
    )


def load_json_file(path, label, allow_empty=False):
    require_regular_file(path, label, allow_empty=allow_empty)
    raw = path.read_bytes()
    if len(raw) > MAX_JSON_BYTES:
        fail(f"{label}_too_large")
    try:
        return json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError):
        fail(f"{label}_invalid_json")


def validate_manifest(manifest):
    if not isinstance(manifest, dict) or manifest.get("schema_version") != SCHEMA_VERSION:
        fail("manifest_schema_invalid")
    validate_run_id(manifest.get("run_id", ""))
    validate_commit(manifest.get("commit_sha", ""))
    if manifest.get("authoring") != AUTHORING_CONFIG:
        fail("manifest_authoring_invalid")
    if validate_public_origin(manifest.get("public_origin", "")) != manifest.get(
        "public_origin"
    ):
        fail("manifest_public_origin_invalid")
    cloudflare = manifest.get("cloudflare")
    if not isinstance(cloudflare, dict) or set(cloudflare) != {
        "tunnel_id",
        "public_origin",
        "origin_service",
    }:
        fail("manifest_cloudflare_invalid")
    tunnel_id = validate_cloudflare_tunnel_id(cloudflare.get("tunnel_id"))
    expected_steps = [
        {"step": number, "code": specification.code}
        for number, specification in STEP_SPECS.items()
    ]
    if manifest.get("expected_steps") != expected_steps:
        fail("manifest_steps_invalid")
    candidates = manifest.get("candidates")
    if not isinstance(candidates, dict) or set(candidates) != REQUIRED_CANDIDATES:
        fail("manifest_candidates_invalid")
    for name, candidate in candidates.items():
        if not isinstance(candidate, dict) or set(candidate) != {"path", "sha256"}:
            fail(f"manifest_candidate_{name}_invalid")
        if (
            not isinstance(candidate["path"], str)
            or not isinstance(candidate["sha256"], str)
            or not DIGEST_PATTERN.fullmatch(candidate["sha256"])
        ):
            fail(f"manifest_candidate_{name}_invalid")
    source_trees = manifest.get("source_trees")
    expected_files = {
        "codex_worker": CODEX_WORKER_SOURCE_FILES,
        "d2_toolchain": D2_TOOLCHAIN_SOURCE_FILES,
        "certification_transport": CERTIFICATION_TRANSPORT_SOURCE_FILES,
    }
    if not isinstance(source_trees, dict) or set(source_trees) != set(expected_files):
        fail("manifest_source_trees_invalid")
    for name, files in expected_files.items():
        tree = source_trees[name]
        if (
            not isinstance(tree, dict)
            or set(tree) != {"root", "files", "sha256"}
            or tree.get("files") != list(files)
            or not isinstance(tree.get("root"), str)
            or not isinstance(tree.get("sha256"), str)
            or not DIGEST_PATTERN.fullmatch(tree["sha256"])
        ):
            fail("manifest_source_trees_invalid")
        require_absolute_path(tree["root"], f"manifest_source_tree_{name}")
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
        fail("manifest_discord_invalid")
    validate_snowflake(discord.get("guild_id", ""), "manifest_discord_guild")
    validate_snowflake(
        discord.get("hub_channel_id", ""), "manifest_discord_hub_channel"
    )
    validate_snowflake(
        discord.get("application_id", ""), "manifest_discord_application"
    )
    validate_snowflake(discord.get("bot_user_id", ""), "manifest_discord_bot")
    validate_snowflake(discord.get("actor_id", ""), "manifest_discord_actor")
    if discord["hub_channel_id"] in {
        discord["guild_id"],
        discord["application_id"],
        discord["bot_user_id"],
        discord["actor_id"],
    }:
        fail("manifest_discord_hub_channel_invalid")
    if not isinstance(discord.get("resource_prefix"), str) or not ID_PATTERN.fullmatch(
        discord["resource_prefix"]
    ):
        fail("manifest_discord_invalid")
    run_id = manifest["run_id"]
    suffix = run_id.rsplit("-", 1)[1]
    expected_root = pathlib.Path(f"/private/tmp/starring-d2-{run_id}")
    database = manifest.get("database")
    if database != {
        "name": "starring_runtime_staging",
        "cluster_root": str(expected_root / "postgres"),
        "socket_directory": str(expected_root / "socket"),
        "port": database.get("port") if isinstance(database, dict) else None,
    }:
        fail("manifest_database_invalid")
    parse_port(str(database["port"]), "postgres")
    services = manifest.get("services")
    expected_labels = {
        "api": f"local.starring.d2.{suffix}.api",
        "runtime": f"local.starring.d2.{suffix}.runtime",
        "worker": f"local.starring.d2.{suffix}.worker",
        "transport": f"local.starring.d2.{suffix}.transport",
        "tunnel": f"local.starring.d2.{suffix}.tunnel",
    }
    if not isinstance(services, dict) or set(services) != set(expected_labels):
        fail("manifest_services_invalid")
    observed_ports = [database["port"]]
    for name, label in expected_labels.items():
        expected = {"label": label}
        if name == "transport":
            if (
                not isinstance(services[name], dict)
                or set(services[name]) != {"label", "gateway_port", "http_port"}
            ):
                fail("manifest_services_invalid")
            for port_name in ("gateway_port", "http_port"):
                parse_port(str(services[name][port_name]), f"transport_{port_name}")
                expected[port_name] = services[name][port_name]
                observed_ports.append(services[name][port_name])
        elif name != "tunnel":
            if not isinstance(services[name], dict) or "port" not in services[name]:
                fail("manifest_services_invalid")
            parse_port(str(services[name]["port"]), name)
            expected["port"] = services[name]["port"]
            observed_ports.append(services[name]["port"])
        if services[name] != expected:
            fail("manifest_services_invalid")
    if len(set(observed_ports)) != 6:
        fail("manifest_ports_invalid")
    expected_cloudflare = validate_cloudflare_route_binding(
        tunnel_id, manifest["public_origin"], services["api"]["port"]
    )
    if cloudflare != expected_cloudflare:
        fail("manifest_cloudflare_invalid")
    expected_keychain = {
        "api": f"starring.d2.{suffix}.api",
        "runtime": f"starring.d2.{suffix}.runtime",
        "postgres": f"starring.d2.{suffix}.postgres",
        "worker": f"starring.d2.{suffix}.worker",
    }
    if manifest.get("keychain_services") != expected_keychain:
        fail("manifest_keychain_invalid")
    external = manifest.get("external_keychain")
    if not isinstance(external, dict) or set(external) != {
        "discord_oauth_client_secret",
        "discord_bot_token",
        "tunnel_token",
    }:
        fail("manifest_external_keychain_invalid")
    observed_external = set()
    for identity in external.values():
        if not isinstance(identity, dict) or set(identity) != {"service", "account"}:
            fail("manifest_external_keychain_invalid")
        normalized = validate_keychain_identity(
            f"{identity['service']}:{identity['account']}", "manifest_external_keychain"
        )
        observed_external.add((normalized["service"], normalized["account"]))
    if len(observed_external) != 3:
        fail("manifest_external_keychain_invalid")
    protected = manifest.get("protected_staging")
    protected_labels = protected.get("launchd_labels") if isinstance(protected, dict) else None
    if (
        not isinstance(protected, dict)
        or protected.get("mutation_allowed") is not False
        or not isinstance(protected_labels, list)
        or not all(isinstance(label, str) for label in protected_labels)
        or set(protected_labels)
        != {
            "local.starring.api.staging",
            "local.starring.codex-worker",
            "local.starring.runtime.staging",
            "local.cloudflared.starring",
        }
    ):
        fail("manifest_protected_staging_invalid")
    return manifest


def validate_candidate_files(manifest):
    for name, candidate in manifest["candidates"].items():
        path = require_absolute_path(candidate["path"], f"candidate_{name}")
        require_immutable_candidate(path, name)
        if sha256_file(path) != candidate["sha256"]:
            fail(f"candidate_{name}_digest_mismatch")
    worker_path = pathlib.Path(manifest["candidates"]["codex_worker"]["path"])
    validate_codex_worker_inventory(worker_path.parent)
    expected_roots = {
        "codex_worker": worker_path.parent,
        "d2_toolchain": pathlib.Path(__file__).resolve().parent,
        "certification_transport": pathlib.Path(__file__).resolve().parent.parent
        / "d2-certification-transport",
    }
    expected_files = {
        "codex_worker": CODEX_WORKER_SOURCE_FILES,
        "d2_toolchain": D2_TOOLCHAIN_SOURCE_FILES,
        "certification_transport": CERTIFICATION_TRANSPORT_SOURCE_FILES,
    }
    validate_certification_transport_inventory(
        expected_roots["certification_transport"]
    )
    for name in sorted(expected_roots):
        tree = manifest["source_trees"][name]
        if pathlib.Path(tree["root"]) != expected_roots[name]:
            fail(f"source_tree_{name}_root_mismatch")
        observed = source_tree_digest(
            expected_roots[name], expected_files[name], f"source_tree_{name}"
        )
        if observed != tree["sha256"]:
            fail(f"source_tree_{name}_digest_mismatch")


def load_verified_manifest(path):
    manifest_path = require_absolute_path(path, "manifest")
    require_owned_mode(manifest_path.parent, 0o700, "run_directory", directory=True)
    require_owned_mode(manifest_path, 0o600, "manifest")
    manifest = validate_manifest(load_json_file(manifest_path, "manifest"))
    digest_path = manifest_path.with_name("manifest.sha256")
    require_owned_mode(digest_path, 0o600, "manifest_digest")
    require_regular_file(digest_path, "manifest_digest")
    digest = digest_path.read_text(encoding="utf-8").strip()
    if not DIGEST_PATTERN.fullmatch(digest) or digest != manifest_digest(manifest):
        fail("manifest_digest_mismatch")
    validate_candidate_files(manifest)
    return manifest_path, manifest, digest


def validate_json_safety(value, key_path=(), depth=0):
    if depth > MAX_NESTING_DEPTH:
        fail("evidence_nesting_too_deep")
    if isinstance(value, dict):
        if len(value) > MAX_COLLECTION_ITEMS:
            fail("evidence_collection_too_large")
        for key, nested in value.items():
            if not isinstance(key, str) or not key:
                fail("evidence_key_invalid")
            normalized = key.lower()
            if normalized in FORBIDDEN_EVIDENCE_KEYS or any(
                component in normalized
                for component in ("token", "secret", "password", "cookie")
            ):
                fail(f"evidence_forbidden_key:{'.'.join((*key_path, key))}")
            validate_json_safety(nested, (*key_path, key), depth + 1)
        return
    if isinstance(value, list):
        if len(value) > MAX_COLLECTION_ITEMS:
            fail("evidence_collection_too_large")
        for index, nested in enumerate(value):
            validate_json_safety(nested, (*key_path, str(index)), depth + 1)
        return
    if isinstance(value, str):
        if len(value.encode("utf-8")) > MAX_STRING_BYTES:
            fail("evidence_string_too_large")
        for pattern in FORBIDDEN_VALUE_PATTERNS:
            if pattern.search(value):
                fail("evidence_forbidden_value")
        return
    if value is None or isinstance(value, (bool, int)):
        return
    fail("evidence_value_type_invalid")


def require_fields(evidence, specification):
    if not isinstance(evidence, dict):
        fail("evidence_not_object")
    missing = [field for field in specification.required if field not in evidence]
    if missing:
        fail(f"evidence_missing_fields:{','.join(missing)}")


def require_true(evidence, *fields):
    for field in fields:
        if evidence[field] is not True:
            fail(f"step_contract_failed:{field}")


def require_zero(evidence, *fields):
    for field in fields:
        if type(evidence[field]) is not int or evidence[field] != 0:
            fail(f"step_contract_failed:{field}")


def require_positive_integer(evidence, *fields):
    for field in fields:
        if type(evidence[field]) is not int or evidence[field] <= 0:
            fail(f"step_contract_failed:{field}")


def require_identifier(evidence, *fields):
    for field in fields:
        if not isinstance(evidence[field], str) or not ID_PATTERN.fullmatch(evidence[field]):
            fail(f"step_contract_failed:{field}")


def require_digest(evidence, *fields):
    for field in fields:
        if not isinstance(evidence[field], str) or not DIGEST_PATTERN.fullmatch(evidence[field]):
            fail(f"step_contract_failed:{field}")


def validate_step_contract(step, evidence, manifest, prior_receipts):
    specification = STEP_SPECS[step]
    require_fields(evidence, specification)
    validate_json_safety(evidence)
    if set(evidence) != set(specification.required):
        fail("evidence_fields_invalid")
    if step == 1:
        if (
            not isinstance(evidence["database_system_identifier"], str)
            or not evidence["database_system_identifier"].isdigit()
        ):
            fail("step_contract_failed:database_system_identifier")
        require_positive_integer(evidence, "migration_count")
        if not MIGRATION_PATTERN.fullmatch(str(evidence["migration_head"])):
            fail("step_contract_failed:migration_head")
        require_digest(evidence, "migration_ledger_sha256")
        if evidence["discord_resource_prefix"] != manifest["discord"]["resource_prefix"]:
            fail("step_contract_failed:discord_resource_prefix")
    elif step == 2:
        require_zero(evidence, "prior_runtime_owner_count", "prior_smoke_process_count")
    elif step == 3:
        for name in ("api", "runtime"):
            field = f"{name}_sha256"
            require_digest(evidence, field)
            if evidence[field] != manifest["candidates"][name]["sha256"]:
                fail(f"step_contract_failed:{field}")
        require_digest(evidence, "codex_worker_sha256", "d2_toolchain_sha256")
        if (
            evidence["codex_worker_sha256"]
            != manifest["source_trees"]["codex_worker"]["sha256"]
            or evidence["d2_toolchain_sha256"]
            != manifest["source_trees"]["d2_toolchain"]["sha256"]
        ):
            fail("step_contract_failed:source_tree_sha256")
        require_digest(
            evidence,
            "certification_transport_sha256",
            "certification_transport_source_sha256",
        )
        if (
            evidence["certification_transport_sha256"]
            != manifest["candidates"]["certification_transport"]["sha256"]
            or evidence["certification_transport_source_sha256"]
            != manifest["source_trees"]["certification_transport"]["sha256"]
        ):
            fail("step_contract_failed:certification_transport_sha256")
        for field in ("api_build_revision", "runtime_build_revision"):
            if evidence[field] != manifest["commit_sha"]:
                fail(f"step_contract_failed:{field}")
        for field in ("api_ready_status", "runtime_ready_status", "worker_ready_status"):
            if evidence[field] != 200:
                fail(f"step_contract_failed:{field}")
        require_true(evidence, "transport_ready", "tunnel_ready")
        if not isinstance(
            evidence["transport_instance_id"], str
        ) or not TRANSPORT_INSTANCE_PATTERN.fullmatch(
            evidence["transport_instance_id"]
        ):
            fail("step_contract_failed:transport_instance_id")
        if (
            evidence["cloudflare_tunnel_id"]
            != manifest["cloudflare"]["tunnel_id"]
            or evidence["public_origin"] != manifest["cloudflare"]["public_origin"]
            or evidence["origin_service"]
            != manifest["cloudflare"]["origin_service"]
        ):
            fail("step_contract_failed:cloudflare_route_binding")
    elif step == 4:
        if evidence["oauth_callback_status"] != 303 or evidence["me_status"] != 200:
            fail("step_contract_failed:oauth_status")
        if evidence["authority_check_status"] != 204:
            fail("step_contract_failed:authority_check_status")
        require_identifier(evidence, "principal_id", "installation_id", "guild_id")
        if evidence["guild_id"] != manifest["discord"]["guild_id"]:
            fail("step_contract_failed:guild_id")
        if evidence["principal_id"] != f"discord:{manifest['discord']['actor_id']}":
            fail("step_contract_failed:principal_id")
    elif step == 5:
        if evidence["authoring_http_status"] not in (200, 201):
            fail("step_contract_failed:authoring_http_status")
        require_identifier(evidence, "authoring_session_id", "installation_id")
        if evidence["installation_id"] != prior_receipts[3]["evidence"]["installation_id"]:
            fail("step_contract_failed:installation_id")
        require_positive_integer(evidence, "authoring_generation")
        require_true(evidence, "one_shot")
        if any(
            evidence[field] != manifest["authoring"][field]
            for field in ("provider", "model", "reasoning_effort", "auth_mode")
        ):
            fail("step_contract_failed:authoring_model")
    elif step == 6:
        require_true(evidence, "generation_encrypted")
        if evidence["projection_state"] != "preview_ready":
            fail("step_contract_failed:projection_state")
        require_positive_integer(evidence, "generation")
        require_digest(evidence, "payload_digest")
        require_identifier(evidence, "installation_id", "authoring_session_id")
        if (
            evidence["generation"]
            != prior_receipts[4]["evidence"]["authoring_generation"]
            or evidence["installation_id"]
            != prior_receipts[4]["evidence"]["installation_id"]
            or evidence["authoring_session_id"]
            != prior_receipts[4]["evidence"]["authoring_session_id"]
        ):
            fail("step_contract_failed:generation")
    elif step == 7:
        require_identifier(evidence, "installation_id", "promotion_id")
        if evidence["installation_id"] != prior_receipts[5]["evidence"]["installation_id"]:
            fail("step_contract_failed:installation_id")
        if (
            evidence["preview_state"] != "pending_approval"
            or evidence["approval_state"] != "approved"
            or evidence["apply_state"] != "runtime_pending"
        ):
            fail("step_contract_failed:product_decision_state")
    elif step == 8:
        require_true(evidence, "pending_observed", "live_observed")
        require_identifier(
            evidence,
            "installation_id",
            "promotion_id",
            "deployment_id",
            "route_id",
            "attestation_id",
            "serving_lease_id",
        )
        if (
            evidence["installation_id"]
            != prior_receipts[6]["evidence"]["installation_id"]
            or evidence["promotion_id"]
            != prior_receipts[6]["evidence"]["promotion_id"]
        ):
            fail("step_contract_failed:deployment_identity")
    elif step == 9:
        require_identifier(
            evidence,
            "create_interaction_id",
            "join_interaction_id",
            "deployment_id",
            "route_id",
            "instance_id",
        )
        if (
            evidence["deployment_id"]
            != prior_receipts[7]["evidence"]["deployment_id"]
            or evidence["route_id"] != prior_receipts[7]["evidence"]["route_id"]
        ):
            fail("step_contract_failed:interaction_target_identity")
        for field in ("role_ids", "channel_ids", "panel_message_ids"):
            values = evidence[field]
            if not isinstance(values, list) or not values:
                fail(f"step_contract_failed:{field}")
            for value in values:
                if not isinstance(value, str) or not ID_PATTERN.fullmatch(value):
                    fail(f"step_contract_failed:{field}")
        require_positive_integer(evidence, "ephemeral_count")
    elif step == 10:
        require_identifier(evidence, "interaction_id")
        if (
            type(evidence["delivery_count"]) is not int
            or evidence["delivery_count"] < 2
            or type(evidence["external_effect_count"]) is not int
            or evidence["external_effect_count"] != 1
        ):
            fail("step_contract_failed:duplicate_effect")
        if evidence["receipt_state"] != "completed":
            fail("step_contract_failed:receipt_state")
        if evidence["interaction_id"] != prior_receipts[8]["evidence"]["join_interaction_id"]:
            fail("step_contract_failed:duplicate_interaction_id")
        if (
            evidence["transport_duplicate_injections"] != 1
            or evidence["transport_duplicate_delivery_count"] != 2
            or evidence["transport_last_duplicate_interaction_id"]
            != evidence["interaction_id"]
        ):
            fail("step_contract_failed:transport_duplicate_evidence")
        if (
            evidence["transport_instance_id"]
            != prior_receipts[2]["evidence"]["transport_instance_id"]
        ):
            fail("step_contract_failed:transport_instance_id")
    elif step == 11:
        require_positive_integer(evidence, "old_pid", "new_pid")
        if evidence["old_pid"] == evidence["new_pid"]:
            fail("step_contract_failed:pid_rotation")
        if evidence["runtime_sha256"] != manifest["candidates"]["runtime"]["sha256"]:
            fail("step_contract_failed:runtime_sha256")
        require_true(evidence, "ready_after_restart")
        require_identifier(
            evidence, "checkpoint", "deployment_id", "route_id", "instance_id"
        )
        if any(
            evidence[field] != prior_receipts[8]["evidence"][field]
            for field in ("deployment_id", "route_id", "instance_id")
        ):
            fail("step_contract_failed:restart_identity")
    elif step == 12:
        require_true(evidence, "route_reconstructed", "instance_reconstructed")
        require_identifier(evidence, "deployment_id", "route_id", "instance_id")
        require_digest(evidence, "pinned_ruleset_digest")
        if any(
            evidence[field] != prior_receipts[10]["evidence"][field]
            for field in ("deployment_id", "route_id", "instance_id")
        ):
            fail("step_contract_failed:reconstruction_identity")
    elif step == 13:
        require_identifier(evidence, "effect_id", "interaction_id", "route_id")
        if evidence["route_id"] != prior_receipts[11]["evidence"]["route_id"]:
            fail("step_contract_failed:route_id")
        if evidence["interaction_id"] in {
            prior_receipts[8]["evidence"]["create_interaction_id"],
            prior_receipts[8]["evidence"]["join_interaction_id"],
        }:
            fail("step_contract_failed:injection_interaction_id")
        if evidence["injected_outcome"] != "indeterminate":
            fail("step_contract_failed:injected_outcome")
        if evidence["reconciliation_state"] not in (
            "known_success",
            "known_failure",
            "compensated",
        ):
            fail("step_contract_failed:reconciliation_state")
        require_zero(evidence, "duplicate_external_effect_count", "unsafe_deletion_count")
        if evidence["transport_indeterminate_injections"] != 1:
            fail("step_contract_failed:transport_indeterminate_injections")
        require_digest(evidence, "transport_last_audit_reason_sha256")
        if (
            type(evidence["transport_last_upstream_status"]) is not int
            or not 200 <= evidence["transport_last_upstream_status"] < 300
        ):
            fail("step_contract_failed:transport_last_upstream_status")
        if (
            evidence["transport_instance_id"]
            != prior_receipts[2]["evidence"]["transport_instance_id"]
        ):
            fail("step_contract_failed:transport_instance_id")
    elif step == 14:
        require_identifier(
            evidence,
            "replacement_target_id",
            "source_deployment_id",
            "source_route_id",
            "replacement_deployment_id",
            "replacement_route_id",
        )
        if (
            evidence["source_deployment_id"]
            != prior_receipts[11]["evidence"]["deployment_id"]
            or evidence["source_route_id"]
            != prior_receipts[11]["evidence"]["route_id"]
            or evidence["replacement_target_id"]
            != evidence["replacement_deployment_id"]
            or evidence["replacement_deployment_id"]
            == evidence["source_deployment_id"]
            or evidence["replacement_route_id"] == evidence["source_route_id"]
        ):
            fail("step_contract_failed:replacement_identity")
        if evidence["replacement_kind"] not in ("update", "rollback"):
            fail("step_contract_failed:replacement_kind")
        require_true(
            evidence, "previous_target_drained", "replacement_live", "prior_route_absent"
        )
    elif step == 15:
        require_true(evidence, "gateway_disconnected", "live_lost")
        if evidence["runtime_ready_status"] != 503:
            fail("step_contract_failed:runtime_ready_status")
        require_identifier(evidence, "public_code", "route_id")
        if evidence["route_id"] != prior_receipts[13]["evidence"]["replacement_route_id"]:
            fail("step_contract_failed:route_id")
        require_true(evidence, "transport_gateway_partitioned")
        require_positive_integer(evidence, "transport_gateway_partition_events")
        if (
            evidence["transport_instance_id"]
            != prior_receipts[2]["evidence"]["transport_instance_id"]
        ):
            fail("step_contract_failed:transport_instance_id")
    elif step == 16:
        require_true(
            evidence,
            "teardown_started",
            "database_drop_requested",
            "services_stopped",
        )
        deleted = evidence["discord_resource_ids_deleted"]
        if not isinstance(deleted, list) or not deleted:
            fail("step_contract_failed:discord_resource_ids_deleted")
        for value in deleted:
            if not isinstance(value, str) or not ID_PATTERN.fullmatch(value):
                fail("step_contract_failed:discord_resource_ids_deleted")
        created = set(
            prior_receipts[8]["evidence"]["role_ids"]
            + prior_receipts[8]["evidence"]["channel_ids"]
            + prior_receipts[8]["evidence"]["panel_message_ids"]
        )
        if len(deleted) != len(set(deleted)) or set(deleted) != created:
            fail("step_contract_failed:discord_resource_ids_deleted")
    elif step == 17:
        require_zero(
            evidence,
            "unresolved_operation_count",
            "unresolved_receipt_count",
            "unresolved_journal_count",
            "route_count",
            "instance_count",
            "role_count",
            "channel_count",
            "panel_count",
            "resource_prefix_match_count",
        )
        require_true(
            evidence,
            "database_absent",
            "postgres_process_absent",
            "launchd_jobs_absent",
            "keychain_items_absent",
            "discord_guild_deleted",
        )


def load_receipts_from_handle(handle, manifest, digest):
    handle.seek(0)
    receipts = []
    total = 0
    for raw in handle:
        total += len(raw)
        if total > MAX_JSON_BYTES * len(STEP_SPECS):
            fail("receipts_too_large")
        try:
            receipt = json.loads(raw)
        except (UnicodeDecodeError, json.JSONDecodeError):
            fail("receipt_invalid_json")
        expected_step = len(receipts) + 1
        expected_previous = (
            ZERO_DIGEST if not receipts else receipts[-1]["receipt_sha256"]
        )
        if (
            not isinstance(receipt, dict)
            or set(receipt)
            != {
                "schema_version",
                "run_id",
                "manifest_sha256",
                "step",
                "code",
                "observed_at",
                "previous_sha256",
                "receipt_sha256",
                "evidence",
            }
            or receipt.get("schema_version") != SCHEMA_VERSION
            or receipt.get("run_id") != manifest["run_id"]
            or receipt.get("manifest_sha256") != digest
            or receipt.get("step") != expected_step
            or receipt.get("code") != STEP_SPECS[expected_step].code
            or not validate_utc_timestamp(receipt.get("observed_at"))
            or receipt.get("previous_sha256") != expected_previous
            or not isinstance(receipt.get("receipt_sha256"), str)
            or not DIGEST_PATTERN.fullmatch(receipt["receipt_sha256"])
        ):
            fail("receipt_sequence_invalid")
        hash_payload = dict(receipt)
        receipt_sha256 = hash_payload.pop("receipt_sha256")
        if sha256_bytes(canonical_json(hash_payload).encode("utf-8")) != receipt_sha256:
            fail("receipt_chain_invalid")
        validate_step_contract(expected_step, receipt.get("evidence"), manifest, receipts)
        receipts.append(receipt)
    return receipts


def open_locked_receipts(path, write):
    flags = os.O_RDWR if write else os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        fail(f"receipts_unavailable:{error.__class__.__name__}")
    handle = os.fdopen(descriptor, "r+b" if write else "rb")
    fcntl.flock(handle.fileno(), fcntl.LOCK_EX if write else fcntl.LOCK_SH)
    metadata = os.fstat(handle.fileno())
    if not stat.S_ISREG(metadata.st_mode):
        handle.close()
        fail("receipts_not_regular")
    return handle


def command_record(arguments):
    manifest_path, manifest, digest = load_verified_manifest(arguments.manifest)
    if arguments.step not in STEP_SPECS:
        fail("step_invalid")
    evidence_path = require_absolute_path(arguments.evidence, "evidence")
    require_owned_mode(evidence_path, 0o600, "evidence")
    evidence = load_json_file(evidence_path, "evidence")
    receipts_path = manifest_path.with_name("receipts.jsonl")
    require_owned_mode(receipts_path, 0o600, "receipts")
    with open_locked_receipts(receipts_path, True) as handle:
        receipts = load_receipts_from_handle(handle, manifest, digest)
        expected_step = len(receipts) + 1
        if arguments.step != expected_step:
            fail(f"step_out_of_order:expected_{expected_step}")
        validate_step_contract(arguments.step, evidence, manifest, receipts)
        receipt = {
            "schema_version": SCHEMA_VERSION,
            "run_id": manifest["run_id"],
            "manifest_sha256": digest,
            "step": arguments.step,
            "code": STEP_SPECS[arguments.step].code,
            "observed_at": datetime.datetime.now(datetime.timezone.utc)
            .replace(microsecond=0)
            .isoformat()
            .replace("+00:00", "Z"),
            "previous_sha256": (
                ZERO_DIGEST if not receipts else receipts[-1]["receipt_sha256"]
            ),
            "evidence": evidence,
        }
        receipt["receipt_sha256"] = sha256_bytes(
            canonical_json(receipt).encode("utf-8")
        )
        payload = (canonical_json(receipt) + "\n").encode("utf-8")
        if len(payload) > MAX_JSON_BYTES:
            fail("receipt_too_large")
        handle.seek(0, os.SEEK_END)
        handle.write(payload)
        handle.flush()
        os.fsync(handle.fileno())
    print(canonical_json({"step": arguments.step, "code": STEP_SPECS[arguments.step].code}))


def command_verify(arguments):
    manifest_path, manifest, digest = load_verified_manifest(arguments.manifest)
    receipts_path = manifest_path.with_name("receipts.jsonl")
    require_owned_mode(receipts_path, 0o600, "receipts")
    with open_locked_receipts(receipts_path, False) as handle:
        receipts = load_receipts_from_handle(handle, manifest, digest)
    if len(receipts) != len(STEP_SPECS):
        fail(f"certification_incomplete:{len(receipts)}_of_{len(STEP_SPECS)}")
    summary = {
        "schema_version": SCHEMA_VERSION,
        "run_id": manifest["run_id"],
        "commit_sha": manifest["commit_sha"],
        "manifest_sha256": digest,
        "steps": len(receipts),
        "status": "passed",
        "resource_prefix": manifest["discord"]["resource_prefix"],
        "receipt_chain_head_sha256": receipts[-1]["receipt_sha256"],
    }
    print(canonical_json(summary))


def parser():
    root = argparse.ArgumentParser(prog="d2-certification")
    commands = root.add_subparsers(dest="command", required=True)
    prepare = commands.add_parser("prepare")
    prepare.add_argument("--output-root", required=True)
    prepare.add_argument("--commit", required=True)
    prepare.add_argument("--discord-guild-id", required=True)
    prepare.add_argument("--discord-hub-channel-id", required=True)
    prepare.add_argument("--discord-application-id", required=True)
    prepare.add_argument("--discord-bot-user-id", required=True)
    prepare.add_argument("--discord-actor-id", required=True)
    prepare.add_argument("--discord-oauth-keychain", required=True)
    prepare.add_argument("--discord-bot-keychain", required=True)
    prepare.add_argument("--tunnel-token-keychain", required=True)
    prepare.add_argument("--cloudflare-tunnel-id", required=True)
    prepare.add_argument("--public-origin", required=True)
    prepare.add_argument("--candidate", action="append", default=[], required=True)
    prepare.add_argument("--port", action="append", default=[], required=True)
    prepare.add_argument("--run-id")
    prepare.set_defaults(handler=command_prepare)
    record = commands.add_parser("record")
    record.add_argument("--manifest", required=True)
    record.add_argument("--step", type=int, required=True)
    record.add_argument("--evidence", required=True)
    record.set_defaults(handler=command_record)
    verify = commands.add_parser("verify")
    verify.add_argument("--manifest", required=True)
    verify.set_defaults(handler=command_verify)
    return root


def main(argv=None):
    try:
        arguments = parser().parse_args(argv)
        arguments.handler(arguments)
        return 0
    except CertificationError as error:
        print(str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
