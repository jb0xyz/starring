#!/usr/bin/env python3

"""Run and verify the non-release D2 automated maintenance lane.

The controller never handles a product session credential.  It validates the
isolated D2 run identity and D2A-only direct-onboarding evidence, delegates
ephemeral authentication to the isolated session issuer, accepts only redacted
D2A evidence, and seals that evidence in a separate result root.  D2A output is
deliberately ineligible for D3 release binding.
"""

import argparse
import datetime
import hashlib
import json
import os
import pathlib
import re
import selectors
import signal
import stat
import subprocess
import sys
import time
import uuid


SCHEMA_VERSION = 1
AUTOMATED_CLASS = "automated_maintenance_v1"
COMMERCIAL_CLASS = "commercial_human_v1"
D2_PUBLIC_ORIGIN = "https://d2-api.starring.co.kr"
D2_HUMAN_BOUNDARIES = (
    "create_disposable_discord_guild",
    "complete_discord_oauth",
    "confirm_product_preview",
    "execute_real_discord_interactions",
    "confirm_replacement_preview",
    "delete_disposable_discord_guild",
)
JS_SAFE_INTEGER = 9_007_199_254_740_991
MAX_SCENARIO_MESSAGE_BYTES = 16 * 1024
COMMERCIAL_MANIFEST_FIELDS = {
    "schema_version",
    "certification_class",
    "run_id",
    "created_at",
    "commit_sha",
    "authoring",
    "public_origin",
    "cloudflare",
    "candidates",
    "source_trees",
    "database",
    "discord",
    "services",
    "keychain_services",
    "external_keychain",
    "protected_staging",
    "human_boundaries",
    "expected_steps",
}
CANDIDATE_KEYS = {
    "api",
    "certification_transport",
    "cloudflared",
    "codex",
    "codex_worker",
    "db_bootstrap",
    "node",
    "runtime",
    "sealed_provisioner",
}
RUN_ID = re.compile(r"^d2-[0-9]{8}t[0-9]{6}z-[0-9a-f]{12}$")
DIGEST = re.compile(r"^[0-9a-f]{64}$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")
SNOWFLAKE = re.compile(r"^[1-9][0-9]{0,19}$")
UTC_TIMESTAMP = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?Z$"
)
ALLOWED_OPERATIONS = frozenset({"auth-smoke", "one-shot"})
ALLOWED_EVIDENCE_KINDS = {
    "auth-smoke": "starring.d2a.authentication-evidence.v1",
    "one-shot": "starring.d2a.one-shot-product-evidence.v1",
}
UNCOVERED_RELEASE_BOUNDARIES = [
    "discord_oauth_consent_and_code_exchange",
    "real_discord_gateway_interactions",
    "discord_web_confirmation_surface",
    "human_preview_approval",
    "disposable_guild_deletion",
]
BASE_EVIDENCE_FIELDS = {
    "schema_version",
    "kind",
    "certification_class",
    "operation",
    "observed_at",
    "run_id",
    "manifest_sha256",
    "public_origin",
    "principal_id",
    "guild_id",
    "installation_id",
    "direct_auth_used",
    "release_eligible",
    "uncovered_release_boundaries",
    "logout_status",
    "post_logout_me_status",
}
AUTH_EVIDENCE_FIELDS = BASE_EVIDENCE_FIELDS | {
    "me_status",
    "authority_check_status",
}
ONE_SHOT_EVIDENCE_FIELDS = BASE_EVIDENCE_FIELDS | {
    "scenario_sha256",
    "authoring_http_status",
    "promotion_http_status",
    "preview_http_status",
    "approval_http_status",
    "apply_http_status",
    "authoring_session_id",
    "authoring_generation",
    "promotion_id",
    "candidate_ruleset_hash",
    "target_content_hash",
    "payload_digest",
    "preview_state",
    "approval_state",
    "apply_state",
    "apply_attempts",
    "runtime_drain_observed",
    "runtime_pending_observed",
    "apply_resumed_after_conflict",
    "apply_status_observations",
    "summary",
    "live_observed_at",
    "deployment_http_status",
    "operational_http_status",
    "live_attempts",
    "pending_observed",
    "live_observed",
    "product_state",
    "operational_state",
    "runtime_phase",
    "serving_state",
    "deployment_observed_at",
    "deployment_attestation_revision",
    "deployment_last_heartbeat_at",
    "deployment_lease_expires_at",
    "decision_observed_at",
    "runtime_observed_at",
    "current_attempt",
    "attestation_revision",
    "convergence_attempt",
    "process_instance_id",
    "last_heartbeat_at",
    "lease_expires_at",
}
TAINT_FIELDS = {
    "schema_version",
    "kind",
    "run_id",
    "manifest_sha256",
    "certification_class",
    "direct_auth_used",
    "release_eligible",
    "issuer_sha256",
    "issuer_source_sha256",
    "runner_sha256",
    "product_driver_sha256",
    "scenario_sha256",
}
DIRECT_ONBOARDING_EVIDENCE_NAME = "d2a-onboarding-evidence.json"
DIRECT_ONBOARDING_EVIDENCE_KIND = (
    "starring.d2a.direct-onboarding-evidence.v1"
)
DIRECT_ONBOARDING_EVIDENCE_FIELDS = {
    "schema_version",
    "kind",
    "certification_class",
    "operation",
    "observed_at",
    "run_id",
    "manifest_sha256",
    "principal_id",
    "guild_id",
    "discord_application_id",
    "hub_channel_id",
    "binding_key",
    "installation_id",
    "outcome",
    "provisioner_sha256",
    "issuer_sha256",
    "issuer_source_sha256",
    "discord_hub_preflight",
    "direct_auth_used",
    "session_revoked",
    "release_eligible",
}
DIRECT_ONBOARDING_BINDING_FIELDS = {
    "run_id",
    "manifest_sha256",
    "principal_id",
    "guild_id",
    "discord_application_id",
    "hub_channel_id",
    "installation_id",
    "provisioner_sha256",
    "issuer_sha256",
    "issuer_source_sha256",
}
FINAL_RECORD_FIELDS = {
    "schema_version",
    "kind",
    "certification_class",
    "status",
    "release_eligible",
    "direct_auth_used",
    "run_id",
    "source_commit",
    "d2_manifest_sha256",
    "candidate_api_sha256",
    "candidate_node_sha256",
    "candidate_sealed_provisioner_sha256",
    "controller_sha256",
    "issuer_sha256",
    "issuer_source_sha256",
    "runner_sha256",
    "product_driver_sha256",
    "trusted_scenario_sha256",
    "d2a_taint_sha256",
    "direct_onboarding_evidence_sha256",
    "scenario_sha256",
    "expected_summary",
    "scenario_session_id_prefix",
    "authoring_session_id",
    "installation_id",
    "principal_id",
    "guild_id",
    "discord_application_id",
    "hub_channel_id",
    "public_origin",
    "operation",
    "evidence_sha256",
    "completed_at",
    "uncovered_release_boundaries",
}
COMMERCIAL_ONBOARDING_ARTIFACTS = (
    pathlib.Path("orchestrator/onboarding-evidence.json"),
    pathlib.Path("orchestrator/coordinator-sources/step-04-onboarding.json"),
)
AUTHORING_SESSION_PREFIX = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,110}$")
AUTHORING_SESSION_SUFFIX = re.compile(r"^[0-9a-f]{16}$")
PROCESS_INSTANCE_ID = re.compile(r"^[0-9a-f]{32}$")
SERVING_LEASE_MAXIMUM_NANOSECONDS = 45 * 1_000_000_000
MAX_JSON_BYTES = 1024 * 1024
ISSUER_TIMEOUT_SECONDS = 15 * 60
PROCESS_GROUP_GRACE_SECONDS = 2
FORBIDDEN_KEYS = {
    "authorization",
    "cookie",
    "csrf",
    "csrf_token",
    "database_url",
    "password",
    "secret",
    "session",
    "session_cookie",
    "session_credential",
    "token",
}
FORBIDDEN_VALUES = (
    re.compile(r"postgres(?:ql)?://", re.IGNORECASE),
    re.compile(r"(?:Bearer|Bot)\s+[A-Za-z0-9._~-]+", re.IGNORECASE),
    re.compile(r"__Host-starring_(?:session|csrf)=", re.IGNORECASE),
)


class D2AError(Exception):
    pass


def fail(code):
    raise D2AError(code)


def process_group_exists(process_group):
    try:
        os.killpg(process_group, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def terminate_process_group(process, process_group):
    for signal_value in (signal.SIGTERM, signal.SIGKILL):
        if not process_group_exists(process_group):
            break
        try:
            os.killpg(process_group, signal_value)
        except ProcessLookupError:
            break
        deadline = time.monotonic() + PROCESS_GROUP_GRACE_SECONDS
        while process_group_exists(process_group) and time.monotonic() < deadline:
            try:
                process.wait(timeout=0.05)
            except subprocess.TimeoutExpired:
                pass
            time.sleep(0.01)
    try:
        process.wait(timeout=PROCESS_GROUP_GRACE_SECONDS)
    except subprocess.TimeoutExpired:
        pass
    return not process_group_exists(process_group)


def supervise_issuer(command):
    try:
        process = subprocess.Popen(
            command,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env={"HOME": str(pathlib.Path.home()), "PATH": "/usr/bin:/bin"},
            start_new_session=True,
            close_fds=True,
            bufsize=0,
        )
    except OSError:
        fail("issuer_execution_failed")
    process_group = process.pid
    stdout_stream = process.stdout
    stderr_stream = process.stderr
    streams = {stdout_stream: bytearray(), stderr_stream: bytearray()}
    exceeded = {stdout_stream: False, stderr_stream: False}
    selector = selectors.DefaultSelector()
    for stream in streams:
        os.set_blocking(stream.fileno(), False)
        selector.register(stream, selectors.EVENT_READ)
    deadline = time.monotonic() + ISSUER_TIMEOUT_SECONDS
    timed_out = False
    output_exceeded = False
    try:
        while selector.get_map():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                timed_out = True
                break
            events = selector.select(min(0.25, remaining))
            if not events and process.poll() is not None:
                events = [(key, selectors.EVENT_READ) for key in selector.get_map().values()]
            for key, _mask in events:
                stream = key.fileobj
                try:
                    chunk = os.read(stream.fileno(), 64 * 1024)
                except BlockingIOError:
                    continue
                except OSError:
                    chunk = b""
                if not chunk:
                    try:
                        selector.unregister(stream)
                    except Exception:
                        pass
                    continue
                retained = max(0, MAX_JSON_BYTES + 1 - len(streams[stream]))
                streams[stream].extend(chunk[:retained])
                if len(chunk) > retained or len(streams[stream]) > MAX_JSON_BYTES:
                    exceeded[stream] = True
                    output_exceeded = True
                    break
            if output_exceeded:
                if not terminate_process_group(process, process_group):
                    fail("issuer_process_group_active")
                break
            if process.poll() is not None and process_group_exists(process_group):
                if not terminate_process_group(process, process_group):
                    fail("issuer_process_group_active")
                timed_out = True
                break
        if process.poll() is None:
            try:
                process.wait(timeout=max(0.1, deadline - time.monotonic()))
            except subprocess.TimeoutExpired:
                timed_out = True
        if timed_out or process.returncode != 0 or process_group_exists(process_group):
            if not terminate_process_group(process, process_group):
                fail("issuer_process_group_active")
    except BaseException:
        terminate_process_group(process, process_group)
        raise
    finally:
        selector.close()
        for stream in streams:
            try:
                stream.close()
            except OSError:
                pass
    result = subprocess.CompletedProcess(
        command,
        process.returncode if process.returncode is not None else -signal.SIGKILL,
        bytes(streams[stdout_stream]),
        bytes(streams[stderr_stream]),
    )
    result.timed_out = timed_out
    result.output_exceeded = output_exceeded or exceeded[stdout_stream] or exceeded[stderr_stream]
    return result


def canonical_json(value):
    return json.dumps(
        value,
        ensure_ascii=False,
        allow_nan=False,
        separators=(",", ":"),
        sort_keys=True,
    )


def strict_json_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate_key")
        result[key] = value
    return result


def reject_json_constant(_value):
    raise ValueError("non_finite_number")


def sha256_bytes(value):
    return hashlib.sha256(value).hexdigest()


def sha256_file(path, label, maximum_bytes):
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError:
        fail(f"{label}_unavailable")
    try:
        before = os.fstat(descriptor)
        if (
            not stat.S_ISREG(before.st_mode)
            or before.st_uid != os.getuid()
            or before.st_nlink != 1
            or before.st_size < 1
            or before.st_size > maximum_bytes
        ):
            fail(f"{label}_invalid")
        digest = hashlib.sha256()
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            digest.update(chunk)
        after = os.fstat(descriptor)
        if (
            (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)
            != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
        ):
            fail(f"{label}_changed")
        return digest.hexdigest()
    finally:
        os.close(descriptor)


def issuer_source_sha256(tool_root):
    source_root = tool_root / "session-issuer"
    entries = ("Cargo.toml", "Cargo.lock", "src/lib.rs", "src/main.rs")
    digest = hashlib.sha256(b"starring.d2a.session-issuer-source.v1\0")
    for name in entries:
        encoded_name = name.encode("utf-8")
        path = source_root / name
        require_owned(path, "issuer_source", 0o644)
        flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
        try:
            descriptor = os.open(path, flags)
        except OSError:
            fail("issuer_source_unavailable")
        try:
            before = os.fstat(descriptor)
            if (
                not stat.S_ISREG(before.st_mode)
                or before.st_uid != os.getuid()
                or before.st_nlink != 1
                or before.st_size < 1
                or before.st_size > 4 * 1024 * 1024
            ):
                fail("issuer_source_invalid")
            digest.update(len(encoded_name).to_bytes(8, "big"))
            digest.update(encoded_name)
            digest.update(before.st_size.to_bytes(8, "big"))
            while True:
                chunk = os.read(descriptor, 1024 * 1024)
                if not chunk:
                    break
                digest.update(chunk)
            after = os.fstat(descriptor)
            if (
                (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)
                != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
            ):
                fail("issuer_source_changed")
        finally:
            os.close(descriptor)
    return digest.hexdigest()


def utc_now():
    return (
        datetime.datetime.now(datetime.timezone.utc)
        .isoformat()
        .replace("+00:00", "Z")
    )


def require_absolute(value, label):
    path = pathlib.Path(value)
    if not path.is_absolute() or path != pathlib.Path(os.path.normpath(path)):
        fail(f"{label}_path_invalid")
    return path


def require_owned(path, label, mode, directory=False):
    try:
        metadata = path.lstat()
    except OSError:
        fail(f"{label}_unavailable")
    expected_type = stat.S_ISDIR if directory else stat.S_ISREG
    if (
        path.is_symlink()
        or not expected_type(metadata.st_mode)
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != mode
        or (not directory and metadata.st_nlink != 1)
    ):
        fail(f"{label}_invalid")
    return metadata


def read_owned_payload(path, label, mode=0o600, maximum=MAX_JSON_BYTES):
    expected = require_owned(path, label, mode)
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError:
        fail(f"{label}_unavailable")
    try:
        before = os.fstat(descriptor)
        if (
            not stat.S_ISREG(before.st_mode)
            or before.st_uid != os.getuid()
            or stat.S_IMODE(before.st_mode) != mode
            or before.st_nlink != 1
            or (before.st_dev, before.st_ino)
            != (expected.st_dev, expected.st_ino)
            or before.st_size < 1
            or before.st_size > maximum
        ):
            fail(f"{label}_invalid")
        raw = bytearray()
        while len(raw) <= maximum:
            chunk = os.read(descriptor, min(65536, maximum + 1 - len(raw)))
            if not chunk:
                break
            raw.extend(chunk)
        after = os.fstat(descriptor)
        if (
            not raw
            or len(raw) > maximum
            or (
                before.st_dev,
                before.st_ino,
                before.st_size,
                before.st_mtime_ns,
                before.st_ctime_ns,
                before.st_nlink,
            )
            != (
                after.st_dev,
                after.st_ino,
                after.st_size,
                after.st_mtime_ns,
                after.st_ctime_ns,
                after.st_nlink,
            )
        ):
            fail(f"{label}_changed")
        return bytes(raw)
    finally:
        os.close(descriptor)


def load_json_payload(path, label, mode=0o600, maximum=MAX_JSON_BYTES):
    raw = read_owned_payload(path, label, mode, maximum)
    try:
        value = json.loads(
            raw,
            object_pairs_hook=strict_json_object,
            parse_constant=reject_json_constant,
        )
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
        fail(f"{label}_json_invalid")
    return value, raw


def load_json(path, label, mode=0o600):
    return load_json_payload(path, label, mode)[0]


def reject_commercial_onboarding_artifacts(run_directory):
    for relative in COMMERCIAL_ONBOARDING_ARTIFACTS:
        path = run_directory / relative
        try:
            path.lstat()
        except FileNotFoundError:
            continue
        except OSError:
            fail("commercial_onboarding_artifact_invalid")
        fail("commercial_onboarding_artifact_present")


def validate_direct_onboarding_evidence(value, binding):
    if (
        not isinstance(binding, dict)
        or set(binding) != DIRECT_ONBOARDING_BINDING_FIELDS
        or not isinstance(value, dict)
        or set(value) != DIRECT_ONBOARDING_EVIDENCE_FIELDS
        or type(value.get("schema_version")) is not int
        or value.get("schema_version") != SCHEMA_VERSION
        or value.get("kind") != DIRECT_ONBOARDING_EVIDENCE_KIND
        or value.get("certification_class") != AUTOMATED_CLASS
        or value.get("operation") != "direct-onboard"
        or not isinstance(value.get("observed_at"), str)
        or not UTC_TIMESTAMP.fullmatch(value["observed_at"])
        or any(value.get(key) != expected for key, expected in binding.items())
        or value.get("binding_key") != "community_hub"
        or value.get("outcome") not in {"fresh", "exact_replay"}
        or value.get("discord_hub_preflight") is not True
        or value.get("direct_auth_used") is not True
        or value.get("session_revoked") is not True
        or value.get("release_eligible") is not False
        or not isinstance(value.get("run_id"), str)
        or not RUN_ID.fullmatch(value["run_id"])
        or not isinstance(value.get("principal_id"), str)
        or not re.fullmatch(r"discord:[1-9][0-9]{0,19}", value["principal_id"])
        or any(
            not isinstance(value.get(field), str)
            or not SNOWFLAKE.fullmatch(value[field])
            for field in (
                "guild_id",
                "discord_application_id",
                "hub_channel_id",
            )
        )
        or not isinstance(value.get("installation_id"), str)
        or not value["installation_id"].startswith("installation:starring-d2-")
        or any(
            not isinstance(value.get(field), str)
            or not DIGEST.fullmatch(value[field])
            for field in (
                "manifest_sha256",
                "provisioner_sha256",
                "issuer_sha256",
                "issuer_source_sha256",
            )
        )
    ):
        fail("direct_onboarding_evidence_invalid")
    try:
        utc_nanoseconds(value["observed_at"])
    except D2AError:
        fail("direct_onboarding_evidence_invalid")
    return value


def load_direct_onboarding_evidence(run_directory, binding):
    reject_commercial_onboarding_artifacts(run_directory)
    path = run_directory / DIRECT_ONBOARDING_EVIDENCE_NAME
    value, payload = load_json_payload(
        path,
        "direct_onboarding_evidence",
        mode=0o600,
        maximum=64 * 1024,
    )
    validate_direct_onboarding_evidence(value, binding)
    if payload != (canonical_json(value) + "\n").encode("utf-8"):
        fail("direct_onboarding_evidence_not_canonical")
    return path, value, payload, sha256_bytes(payload)


def direct_onboarding_binding(
    manifest,
    manifest_sha256,
    installation_id,
    issuer_sha256,
    issuer_source_digest,
):
    discord = manifest["discord"]
    return {
        "run_id": manifest["run_id"],
        "manifest_sha256": manifest_sha256,
        "principal_id": f"discord:{discord['actor_id']}",
        "guild_id": discord["guild_id"],
        "discord_application_id": discord["application_id"],
        "hub_channel_id": discord["hub_channel_id"],
        "installation_id": installation_id,
        "provisioner_sha256": manifest["candidates"]["sealed_provisioner"][
            "sha256"
        ],
        "issuer_sha256": issuer_sha256,
        "issuer_source_sha256": issuer_source_digest,
    }


def load_manifest(path):
    path = require_absolute(path, "manifest")
    require_owned(path.parent, "run_directory", 0o700, directory=True)
    manifest = load_json(path, "manifest")
    if not isinstance(manifest, dict):
        fail("manifest_invalid")
    if set(manifest) != COMMERCIAL_MANIFEST_FIELDS:
        fail("manifest_fields_invalid")
    run_id = manifest.get("run_id")
    if not isinstance(run_id, str) or not RUN_ID.fullmatch(run_id):
        fail("manifest_run_id_invalid")
    if manifest.get("certification_class") != COMMERCIAL_CLASS:
        fail("manifest_certification_class_invalid")
    if manifest.get("public_origin") != D2_PUBLIC_ORIGIN:
        fail("manifest_public_origin_invalid")
    commit_sha = manifest.get("commit_sha")
    if not isinstance(commit_sha, str) or not COMMIT.fullmatch(commit_sha):
        fail("manifest_commit_invalid")
    candidates = manifest.get("candidates")
    if not isinstance(candidates, dict) or set(candidates) != CANDIDATE_KEYS:
        fail("manifest_candidates_invalid")
    for name, candidate in candidates.items():
        if (
            not isinstance(candidate, dict)
            or set(candidate) != {"path", "sha256"}
            or not isinstance(candidate.get("path"), str)
            or not pathlib.Path(candidate["path"]).is_absolute()
            or not DIGEST.fullmatch(candidate.get("sha256", ""))
        ):
            fail(f"manifest_{name}_candidate_invalid")
    database = manifest.get("database")
    suffix = run_id.rsplit("-", 1)[1]
    expected_root = pathlib.Path("/private/tmp") / f"starring-d2-{run_id}"
    if not isinstance(database, dict) or database != {
        "name": "starring_runtime_staging",
        "cluster_root": str(expected_root / "postgres"),
        "socket_directory": str(expected_root / "socket"),
        "port": database.get("port"),
    }:
        fail("manifest_database_invalid")
    if type(database["port"]) is not int or not 1024 <= database["port"] <= 65535:
        fail("manifest_database_invalid")
    keychain = manifest.get("keychain_services")
    if not isinstance(keychain, dict) or keychain.get("api") != f"starring.d2.{suffix}.api":
        fail("manifest_keychain_invalid")
    discord = manifest.get("discord")
    if (
        not isinstance(discord, dict)
        or discord.get("disposable_guild_required") is not True
        or not isinstance(discord.get("actor_id"), str)
        or not SNOWFLAKE.fullmatch(discord["actor_id"])
        or not isinstance(discord.get("guild_id"), str)
        or not SNOWFLAKE.fullmatch(discord["guild_id"])
        or not isinstance(discord.get("application_id"), str)
        or not SNOWFLAKE.fullmatch(discord["application_id"])
        or discord.get("bot_user_id") != discord["application_id"]
        or not isinstance(discord.get("hub_channel_id"), str)
        or not SNOWFLAKE.fullmatch(discord["hub_channel_id"])
        or not isinstance(discord.get("resource_prefix"), str)
        or discord["resource_prefix"] != f"starring-d2-{run_id[3:11]}-{suffix}"
    ):
        fail("manifest_discord_invalid")
    if len(
        {
            discord["guild_id"],
            discord["hub_channel_id"],
            discord["application_id"],
            discord["actor_id"],
        }
    ) != 4:
        fail("manifest_discord_invalid")
    if manifest.get("human_boundaries") != list(D2_HUMAN_BOUNDARIES):
        fail("manifest_human_boundaries_invalid")
    digest_path = path.with_name("manifest.sha256")
    require_owned(digest_path, "manifest_digest", 0o600)
    digest = digest_path.read_text(encoding="utf-8").strip()
    observed = sha256_bytes(canonical_json(manifest).encode("utf-8"))
    if not DIGEST.fullmatch(digest) or digest != observed:
        fail("manifest_digest_mismatch")
    reject_commercial_onboarding_artifacts(path.parent)
    state_path = path.parent / "orchestrator" / "state.json"
    state = load_json(state_path, "orchestrator_state")
    if state.get("phase") != "candidate_started":
        fail("candidate_not_running")
    expected_installation = f"installation:{discord.get('resource_prefix', '')}"
    return path, manifest, digest, expected_installation


def load_scenario(path):
    scenario = load_json(path, "scenario", mode=0o644)
    if (
        not isinstance(scenario, dict)
        or set(scenario)
        != {
            "schema_version",
            "kind",
            "session_id_prefix",
            "message",
            "expected_generation",
            "expected_summary",
        }
        or type(scenario.get("schema_version")) is not int
        or scenario.get("schema_version") != 1
        or scenario.get("kind") != "starring.d2a.product-scenario.v1"
        or not isinstance(scenario.get("session_id_prefix"), str)
        or not AUTHORING_SESSION_PREFIX.fullmatch(scenario["session_id_prefix"])
        or not isinstance(scenario.get("message"), str)
        or not scenario["message"]
        or len(scenario["message"].encode("utf-8")) > MAX_SCENARIO_MESSAGE_BYTES
        or scenario.get("expected_generation") != 0
    ):
        fail("scenario_invalid")
    summary = scenario.get("expected_summary")
    if (
        not isinstance(summary, dict)
        or set(summary)
        != {
            "panels",
            "modals",
            "rules",
            "actions",
            "target_version",
            "required_approvals",
        }
        or any(
            type(summary[field]) is not int
            or not 0 <= summary[field] <= JS_SAFE_INTEGER
            for field in ("panels", "modals", "rules", "actions")
        )
        or type(summary["target_version"]) is not int
        or not 1 <= summary["target_version"] <= JS_SAFE_INTEGER
        or summary["required_approvals"] != 1
    ):
        fail("scenario_summary_invalid")
    return scenario


def validate_taint(value, binding):
    if (
        not isinstance(value, dict)
        or set(value) != TAINT_FIELDS
        or type(value.get("schema_version")) is not int
        or value.get("schema_version") != SCHEMA_VERSION
        or value.get("kind") != "starring.d2a.run-taint.v1"
        or value.get("certification_class") != AUTOMATED_CLASS
        or value.get("direct_auth_used") is not True
        or value.get("release_eligible") is not False
        or any(value.get(key) != expected for key, expected in binding.items())
        or any(
            not isinstance(value.get(field), str)
            or not DIGEST.fullmatch(value[field])
            for field in (
                "manifest_sha256",
                "issuer_sha256",
                "issuer_source_sha256",
                "runner_sha256",
                "product_driver_sha256",
                "scenario_sha256",
            )
        )
    ):
        fail("d2a_taint_invalid")
    return value


def build_taint_marker(
    run_id,
    manifest_sha256,
    issuer_sha256,
    issuer_source_digest,
    runner_sha256,
    product_driver_sha256,
    scenario_sha256,
):
    marker = {
        "schema_version": SCHEMA_VERSION,
        "kind": "starring.d2a.run-taint.v1",
        "run_id": run_id,
        "manifest_sha256": manifest_sha256,
        "certification_class": AUTOMATED_CLASS,
        "direct_auth_used": True,
        "release_eligible": False,
        "issuer_sha256": issuer_sha256,
        "issuer_source_sha256": issuer_source_digest,
        "runner_sha256": runner_sha256,
        "product_driver_sha256": product_driver_sha256,
        "scenario_sha256": scenario_sha256,
    }
    validate_taint(
        marker,
        {
            "run_id": run_id,
            "manifest_sha256": manifest_sha256,
            "issuer_sha256": issuer_sha256,
            "issuer_source_sha256": issuer_source_digest,
            "runner_sha256": runner_sha256,
            "product_driver_sha256": product_driver_sha256,
            "scenario_sha256": scenario_sha256,
        },
    )
    # Field insertion order intentionally matches the Rust D2aTaintMarker struct.
    # The issuer requires byte-exact replay so an early bootstrap marker cannot be
    # replaced or reinterpreted after candidate start.
    payload = json.dumps(
        marker,
        ensure_ascii=False,
        allow_nan=False,
        separators=(",", ":"),
    ).encode("utf-8") + b"\n"
    return marker, payload


def valid_authoring_session_id(value, prefix):
    if (
        not isinstance(value, str)
        or not isinstance(prefix, str)
        or not AUTHORING_SESSION_PREFIX.fullmatch(prefix)
        or not value.startswith(prefix + "-")
        or len(value) > 128
    ):
        return False
    return bool(AUTHORING_SESSION_SUFFIX.fullmatch(value[len(prefix) + 1 :]))


def utc_nanoseconds(value):
    if not isinstance(value, str) or not UTC_TIMESTAMP.fullmatch(value):
        fail("runner_live_timestamp_invalid")
    whole, _, fraction = value[:-1].partition(".")
    try:
        seconds = int(
            datetime.datetime.strptime(whole, "%Y-%m-%dT%H:%M:%S")
            .replace(tzinfo=datetime.timezone.utc)
            .timestamp()
        )
    except ValueError:
        fail("runner_live_timestamp_invalid")
    return seconds * 1_000_000_000 + int(fraction.ljust(9, "0") or "0")


def validate_public_evidence(
    value, operation, binding, scenario_session_id_prefix=None
):
    if not isinstance(value, dict):
        fail("runner_evidence_invalid")
    expected_fields = (
        AUTH_EVIDENCE_FIELDS if operation == "auth-smoke" else ONE_SHOT_EVIDENCE_FIELDS
    )
    if set(value) != expected_fields:
        fail("runner_evidence_fields_invalid")
    if (
        type(value.get("schema_version")) is not int
        or value.get("schema_version") != SCHEMA_VERSION
    ):
        fail("runner_evidence_invalid")
    if value.get("kind") != ALLOWED_EVIDENCE_KINDS[operation]:
        fail("runner_evidence_kind_invalid")
    if (
        value.get("certification_class") != AUTOMATED_CLASS
        or value.get("operation") != operation
        or value.get("direct_auth_used") is not True
        or value.get("release_eligible") is not False
        or value.get("uncovered_release_boundaries") != UNCOVERED_RELEASE_BOUNDARIES
        or not isinstance(value.get("observed_at"), str)
        or not UTC_TIMESTAMP.fullmatch(value["observed_at"])
        or any(value.get(key) != expected for key, expected in binding.items())
        or value.get("logout_status") != 204
        or value.get("post_logout_me_status") != 401
    ):
        fail("runner_release_boundary_invalid")
    if operation == "auth-smoke":
        if value.get("me_status") != 200 or value.get("authority_check_status") != 204:
            fail("runner_authentication_result_invalid")
    else:
        summary = value.get("summary")
        if (
            not isinstance(summary, dict)
            or set(summary)
            != {
                "panels",
                "modals",
                "rules",
                "actions",
                "target_version",
                "required_approvals",
            }
            or any(
                type(summary[field]) is not int
                or not 0 <= summary[field] <= JS_SAFE_INTEGER
                for field in ("panels", "modals", "rules", "actions")
            )
            or type(summary["target_version"]) is not int
            or not 1 <= summary["target_version"] <= JS_SAFE_INTEGER
            or summary["required_approvals"] != 1
            or value.get("authoring_http_status") not in {200, 201}
            or value.get("promotion_http_status") not in {200, 201}
            or value.get("preview_http_status") != 200
            or value.get("approval_http_status") not in {200, 201}
            or value.get("apply_http_status") not in {200, 201, 202}
            or not valid_authoring_session_id(
                value.get("authoring_session_id"), scenario_session_id_prefix
            )
            or type(value.get("authoring_generation")) is not int
            or not 1 <= value["authoring_generation"] <= JS_SAFE_INTEGER
            or any(
                not isinstance(value.get(field), str)
                or not DIGEST.fullmatch(value[field])
                for field in (
                    "scenario_sha256",
                    "promotion_id",
                    "candidate_ruleset_hash",
                    "target_content_hash",
                    "payload_digest",
                )
            )
            or value.get("preview_state") != "pending_approval"
            or value.get("approval_state") != "approved"
            or value.get("apply_state") not in {"runtime_pending", "live"}
            or type(value.get("apply_attempts")) is not int
            or not 1 <= value["apply_attempts"] <= 180
            or type(value.get("runtime_pending_observed")) is not bool
            or type(value.get("runtime_drain_observed")) is not bool
            or type(value.get("apply_resumed_after_conflict")) is not bool
            or type(value.get("apply_status_observations")) is not int
            or not 0 <= value["apply_status_observations"] <= 180
            or value.get("deployment_http_status") != 200
            or value.get("operational_http_status") != 200
            or type(value.get("live_attempts")) is not int
            or not 1 <= value["live_attempts"] <= 180
            or type(value.get("pending_observed")) is not bool
            or value.get("live_observed") is not True
            or value.get("product_state") != "live"
            or value.get("operational_state") != "live"
            or value.get("runtime_phase") != "live"
            or value.get("serving_state") != "fresh"
            or type(value.get("deployment_attestation_revision")) is not int
            or not 1 <= value["deployment_attestation_revision"] <= JS_SAFE_INTEGER
            or type(value.get("current_attempt")) is not int
            or not 1 <= value["current_attempt"] <= JS_SAFE_INTEGER
            or type(value.get("attestation_revision")) is not int
            or not 1 <= value["attestation_revision"] <= JS_SAFE_INTEGER
            or type(value.get("convergence_attempt")) is not int
            or not 1 <= value["convergence_attempt"] <= JS_SAFE_INTEGER
            or value["deployment_attestation_revision"]
            != value["attestation_revision"]
            or value["current_attempt"] != value["convergence_attempt"]
            or not isinstance(value.get("process_instance_id"), str)
            or not PROCESS_INSTANCE_ID.fullmatch(value["process_instance_id"])
        ):
            fail("runner_one_shot_result_invalid")
        live_observed_at = utc_nanoseconds(value.get("live_observed_at"))
        deployment_observed_at = utc_nanoseconds(value.get("deployment_observed_at"))
        deployment_last_heartbeat_at = utc_nanoseconds(
            value.get("deployment_last_heartbeat_at")
        )
        deployment_lease_expires_at = utc_nanoseconds(
            value.get("deployment_lease_expires_at")
        )
        decision_observed_at = utc_nanoseconds(value.get("decision_observed_at"))
        runtime_observed_at = utc_nanoseconds(value.get("runtime_observed_at"))
        last_heartbeat_at = utc_nanoseconds(value.get("last_heartbeat_at"))
        lease_expires_at = utc_nanoseconds(value.get("lease_expires_at"))
        evidence_observed_at = utc_nanoseconds(value.get("observed_at"))
        if not (
            deployment_last_heartbeat_at <= deployment_observed_at
            < deployment_lease_expires_at
            and deployment_observed_at <= decision_observed_at
            <= runtime_observed_at
            <= live_observed_at
            <= evidence_observed_at
            and deployment_last_heartbeat_at <= last_heartbeat_at
            and deployment_lease_expires_at <= lease_expires_at
            and last_heartbeat_at <= runtime_observed_at < lease_expires_at
            and live_observed_at < lease_expires_at
            and 0
            < deployment_lease_expires_at - deployment_last_heartbeat_at
            <= SERVING_LEASE_MAXIMUM_NANOSECONDS
            and 0
            < lease_expires_at - last_heartbeat_at
            <= SERVING_LEASE_MAXIMUM_NANOSECONDS
        ):
            fail("runner_live_projection_invalid")

    def visit(node):
        if isinstance(node, dict):
            for key, child in node.items():
                if not isinstance(key, str) or not key:
                    fail("runner_evidence_key_invalid")
                lowered = key.casefold().replace("-", "_")
                if lowered in FORBIDDEN_KEYS or any(
                    fragment in lowered
                    for fragment in ("password", "credential", "cookie", "token", "secret")
                ):
                    fail("runner_evidence_contains_secret_field")
                visit(child)
        elif isinstance(node, list):
            if len(node) > 512:
                fail("runner_evidence_collection_too_large")
            for child in node:
                visit(child)
        elif isinstance(node, str):
            if len(node.encode("utf-8")) > 64 * 1024 or any(
                pattern.search(node) for pattern in FORBIDDEN_VALUES
            ):
                fail("runner_evidence_contains_secret_value")
        elif node is not None and type(node) not in {bool, int, float}:
            fail("runner_evidence_value_invalid")
    visit(value)
    return value


def write_new(path, payload, mode=0o600):
    descriptor = os.open(
        path,
        os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
        mode,
    )
    try:
        with os.fdopen(descriptor, "wb", closefd=False) as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
    finally:
        os.close(descriptor)


def default_output_root():
    return pathlib.Path.home() / "Library" / "Application Support" / "Starring" / "d2a-runs"


def create_result_directory(root, run_id):
    root = require_absolute(root, "output_root")
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    require_owned(root, "output_root", 0o700, directory=True)
    timestamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    result = root / f"d2a-{timestamp}-{run_id.rsplit('-', 1)[1]}-{uuid.uuid4().hex[:8]}"
    result.mkdir(mode=0o700)
    require_owned(result, "result_directory", 0o700, directory=True)
    return result


def execute(arguments):
    manifest_path, manifest, manifest_digest, installation_id = load_manifest(
        arguments.manifest
    )
    operation = arguments.operation
    if operation not in ALLOWED_OPERATIONS:
        fail("operation_invalid")
    tool_root = pathlib.Path(__file__).resolve().parent
    expected_issuer = (
        tool_root
        / "session-issuer"
        / "target"
        / "release"
        / "starring-d2-session-issuer"
    )
    expected_runner = tool_root / "headless_product_runner.mjs"
    expected_scenario = tool_root / "scenarios" / "study-room.v1.json"
    product_driver = tool_root.parent / "d2-certification" / "product_driver.js"
    scenario = None
    scenario_value = None
    if operation == "one-shot":
        scenario = require_absolute(
            arguments.scenario or str(expected_scenario), "scenario"
        )
        if scenario != expected_scenario:
            fail("scenario_path_invalid")
        scenario_value = load_scenario(scenario)
    elif arguments.scenario:
        fail("scenario_unexpected")
    issuer = require_absolute(arguments.issuer, "issuer")
    if issuer != expected_issuer:
        fail("issuer_path_invalid")
    expected_node = require_absolute(manifest["candidates"]["node"]["path"], "manifest_node")
    node = require_absolute(arguments.node or str(expected_node), "node")
    if node != expected_node:
        fail("node_candidate_mismatch")
    runner = require_absolute(arguments.runner, "runner")
    if runner != expected_runner:
        fail("runner_path_invalid")
    for path, label, mode in (
        (issuer, "issuer", 0o755),
        (node, "node", 0o555),
        (runner, "runner", 0o644),
        (product_driver, "product_driver", 0o644),
        (expected_scenario, "trusted_scenario", 0o644),
    ):
        metadata = path.lstat() if path.exists() else None
        if (
            metadata is None
            or path.is_symlink()
            or not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or metadata.st_nlink != 1
            or stat.S_IMODE(metadata.st_mode) != mode
            or not os.access(
                path,
                os.X_OK if label in {"issuer", "node"} else os.R_OK,
            )
        ):
            fail(f"{label}_invalid")
    if sha256_file(node, "node", 512 * 1024 * 1024) != manifest["candidates"]["node"]["sha256"]:
        fail("node_candidate_digest_mismatch")
    issuer_sha = sha256_file(issuer, "issuer", 512 * 1024 * 1024)
    issuer_source_sha = issuer_source_sha256(tool_root)
    node_sha = sha256_file(node, "node", 512 * 1024 * 1024)
    runner_sha = sha256_file(runner, "runner", 4 * 1024 * 1024)
    product_driver_sha = sha256_file(
        product_driver, "product_driver", 4 * 1024 * 1024
    )
    trusted_scenario_sha = sha256_file(
        expected_scenario, "trusted_scenario", 49_152
    )
    controller_sha = sha256_file(pathlib.Path(__file__), "controller", 4 * 1024 * 1024)
    onboarding_binding = direct_onboarding_binding(
        manifest,
        manifest_digest,
        installation_id,
        issuer_sha,
        issuer_source_sha,
    )
    (
        _onboarding_path,
        onboarding_evidence,
        onboarding_payload,
        onboarding_sha,
    ) = load_direct_onboarding_evidence(
        manifest_path.parent,
        onboarding_binding,
    )
    command = [
        str(issuer),
        "--manifest",
        str(manifest_path),
        "--operation",
        operation,
    ]
    if scenario is not None:
        command.extend(("--scenario", str(scenario)))
    command.extend(("--", str(node), str(runner)))
    completed = supervise_issuer(command)
    if getattr(completed, "timed_out", False):
        fail("issuer_timeout")
    if getattr(completed, "output_exceeded", False):
        fail("issuer_output_invalid")
    if completed.returncode != 0:
        # Never relay child diagnostics: a future dependency error could contain
        # a URL or credential.  The issuer's stable exit code is sufficient.
        fail(f"issuer_failed:{completed.returncode}")
    if completed.stderr or not completed.stdout or len(completed.stdout) > MAX_JSON_BYTES:
        fail("issuer_output_invalid")
    if (
        sha256_file(issuer, "issuer", 512 * 1024 * 1024) != issuer_sha
        or sha256_file(node, "node", 512 * 1024 * 1024) != node_sha
        or issuer_source_sha256(tool_root) != issuer_source_sha
        or sha256_file(runner, "runner", 4 * 1024 * 1024) != runner_sha
        or sha256_file(product_driver, "product_driver", 4 * 1024 * 1024)
        != product_driver_sha
        or sha256_file(expected_scenario, "trusted_scenario", 49_152)
        != trusted_scenario_sha
        or sha256_file(pathlib.Path(__file__), "controller", 4 * 1024 * 1024)
        != controller_sha
    ):
        fail("tool_identity_changed")
    (
        _onboarding_path_after,
        onboarding_evidence_after,
        onboarding_payload_after,
        onboarding_sha_after,
    ) = load_direct_onboarding_evidence(
        manifest_path.parent,
        onboarding_binding,
    )
    if (
        onboarding_evidence_after != onboarding_evidence
        or onboarding_payload_after != onboarding_payload
        or onboarding_sha_after != onboarding_sha
    ):
        fail("direct_onboarding_evidence_changed")
    try:
        evidence = json.loads(
            completed.stdout,
            object_pairs_hook=strict_json_object,
            parse_constant=reject_json_constant,
        )
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
        fail("issuer_output_invalid")
    evidence_binding = {
        "run_id": manifest["run_id"],
        "manifest_sha256": manifest_digest,
        "public_origin": manifest["public_origin"],
        "principal_id": f"discord:{manifest['discord']['actor_id']}",
        "guild_id": manifest["discord"]["guild_id"],
        "installation_id": installation_id,
    }
    if scenario is not None:
        evidence_binding.update(
            {
                "scenario_sha256": trusted_scenario_sha,
                "summary": scenario_value["expected_summary"],
            }
        )
    validate_public_evidence(
        evidence,
        operation,
        evidence_binding,
        scenario_value["session_id_prefix"] if scenario_value is not None else None,
    )
    if utc_nanoseconds(onboarding_evidence["observed_at"]) > utc_nanoseconds(
        evidence["observed_at"]
    ):
        fail("direct_onboarding_timestamp_invalid")
    taint_path = manifest_path.parent / "d2a-taint.json"
    taint = load_json(taint_path, "d2a_taint")
    validate_taint(
        taint,
        {
            "run_id": manifest["run_id"],
            "manifest_sha256": manifest_digest,
            "issuer_sha256": issuer_sha,
            "issuer_source_sha256": issuer_source_sha,
            "runner_sha256": runner_sha,
            "product_driver_sha256": product_driver_sha,
            "scenario_sha256": trusted_scenario_sha,
        },
    )
    result_directory = create_result_directory(arguments.output_root, manifest["run_id"])
    taint_payload = taint_path.read_bytes()
    write_new(result_directory / "d2a-taint.json", taint_payload)
    taint_sha = sha256_bytes(taint_payload)
    onboarding_copy_path = result_directory / DIRECT_ONBOARDING_EVIDENCE_NAME
    write_new(onboarding_copy_path, onboarding_payload)
    (
        _copied_onboarding_path,
        copied_onboarding_evidence,
        copied_onboarding_payload,
        copied_onboarding_sha,
    ) = load_direct_onboarding_evidence(result_directory, onboarding_binding)
    if (
        copied_onboarding_evidence != onboarding_evidence
        or copied_onboarding_payload != onboarding_payload
        or copied_onboarding_sha != onboarding_sha
    ):
        fail("direct_onboarding_copy_mismatch")
    evidence_payload = (canonical_json(evidence) + "\n").encode("utf-8")
    evidence_path = result_directory / "evidence.json"
    write_new(evidence_path, evidence_payload)
    evidence_sha = sha256_bytes(canonical_json(evidence).encode("utf-8"))
    scenario_sha = trusted_scenario_sha if scenario is not None else None
    completed_at = utc_now()
    if utc_nanoseconds(completed_at) < utc_nanoseconds(evidence["observed_at"]):
        completed_at = evidence["observed_at"]
    record = {
        "schema_version": SCHEMA_VERSION,
        "kind": "starring.d2a.automated-final-record.v1",
        "certification_class": AUTOMATED_CLASS,
        "status": "passed",
        "release_eligible": False,
        "direct_auth_used": True,
        "run_id": manifest["run_id"],
        "source_commit": manifest["commit_sha"],
        "d2_manifest_sha256": manifest_digest,
        "candidate_api_sha256": manifest["candidates"]["api"]["sha256"],
        "candidate_node_sha256": node_sha,
        "candidate_sealed_provisioner_sha256": manifest["candidates"][
            "sealed_provisioner"
        ]["sha256"],
        "controller_sha256": controller_sha,
        "issuer_sha256": issuer_sha,
        "issuer_source_sha256": issuer_source_sha,
        "runner_sha256": runner_sha,
        "product_driver_sha256": product_driver_sha,
        "trusted_scenario_sha256": trusted_scenario_sha,
        "d2a_taint_sha256": taint_sha,
        "direct_onboarding_evidence_sha256": onboarding_sha,
        "scenario_sha256": scenario_sha,
        "expected_summary": (
            scenario_value["expected_summary"] if scenario_value is not None else None
        ),
        "scenario_session_id_prefix": (
            scenario_value["session_id_prefix"] if scenario_value is not None else None
        ),
        "authoring_session_id": (
            evidence["authoring_session_id"] if scenario_value is not None else None
        ),
        "installation_id": installation_id,
        "principal_id": evidence_binding["principal_id"],
        "guild_id": evidence_binding["guild_id"],
        "discord_application_id": manifest["discord"]["application_id"],
        "hub_channel_id": manifest["discord"]["hub_channel_id"],
        "public_origin": evidence_binding["public_origin"],
        "operation": operation,
        "evidence_sha256": evidence_sha,
        "completed_at": completed_at,
        "uncovered_release_boundaries": UNCOVERED_RELEASE_BOUNDARIES,
    }
    if set(record) != FINAL_RECORD_FIELDS:
        fail("record_fields_invalid")
    record_payload = (canonical_json(record) + "\n").encode("utf-8")
    record_path = result_directory / "final.json"
    write_new(record_path, record_payload)
    print(
        canonical_json(
            {
                "status": "passed",
                "release_eligible": False,
                "result": str(record_path),
                "evidence_sha256": evidence_sha,
            }
        )
    )


def verify(arguments):
    record_path = require_absolute(arguments.record, "record")
    require_owned(record_path.parent, "result_directory", 0o700, directory=True)
    record = load_json(record_path, "record")
    if (
        not isinstance(record, dict)
        or set(record) != FINAL_RECORD_FIELDS
        or type(record.get("schema_version")) is not int
        or record["schema_version"] != SCHEMA_VERSION
        or record["kind"] != "starring.d2a.automated-final-record.v1"
        or record["certification_class"] != AUTOMATED_CLASS
        or record["status"] != "passed"
        or record["release_eligible"] is not False
        or record["direct_auth_used"] is not True
        or record["operation"] not in ALLOWED_OPERATIONS
        or record["uncovered_release_boundaries"] != UNCOVERED_RELEASE_BOUNDARIES
        or not DIGEST.fullmatch(record.get("d2_manifest_sha256", ""))
        or not DIGEST.fullmatch(record.get("candidate_api_sha256", ""))
        or not DIGEST.fullmatch(record.get("candidate_node_sha256", ""))
        or not DIGEST.fullmatch(
            record.get("candidate_sealed_provisioner_sha256", "")
        )
        or not DIGEST.fullmatch(record.get("controller_sha256", ""))
        or not DIGEST.fullmatch(record.get("issuer_sha256", ""))
        or not DIGEST.fullmatch(record.get("issuer_source_sha256", ""))
        or not DIGEST.fullmatch(record.get("runner_sha256", ""))
        or not DIGEST.fullmatch(record.get("product_driver_sha256", ""))
        or not DIGEST.fullmatch(record.get("trusted_scenario_sha256", ""))
        or not DIGEST.fullmatch(record.get("d2a_taint_sha256", ""))
        or not DIGEST.fullmatch(
            record.get("direct_onboarding_evidence_sha256", "")
        )
        or (
            record["operation"] == "one-shot"
            and not DIGEST.fullmatch(record.get("scenario_sha256") or "")
        )
        or (
            record["operation"] == "one-shot"
            and not valid_authoring_session_id(
                record.get("authoring_session_id"),
                record.get("scenario_session_id_prefix"),
            )
        )
        or (record["operation"] == "auth-smoke" and record.get("scenario_sha256") is not None)
        or (record["operation"] == "auth-smoke" and record.get("expected_summary") is not None)
        or (
            record["operation"] == "auth-smoke"
            and record.get("scenario_session_id_prefix") is not None
        )
        or (
            record["operation"] == "auth-smoke"
            and record.get("authoring_session_id") is not None
        )
        or not DIGEST.fullmatch(record.get("evidence_sha256", ""))
        or not isinstance(record.get("run_id"), str)
        or not RUN_ID.fullmatch(record["run_id"])
        or not isinstance(record.get("source_commit"), str)
        or not COMMIT.fullmatch(record["source_commit"])
        or not isinstance(record.get("principal_id"), str)
        or not re.fullmatch(r"discord:[1-9][0-9]{0,19}", record["principal_id"])
        or not isinstance(record.get("guild_id"), str)
        or not SNOWFLAKE.fullmatch(record["guild_id"])
        or not isinstance(record.get("discord_application_id"), str)
        or not SNOWFLAKE.fullmatch(record["discord_application_id"])
        or not isinstance(record.get("hub_channel_id"), str)
        or not SNOWFLAKE.fullmatch(record["hub_channel_id"])
        or not isinstance(record.get("installation_id"), str)
        or not record["installation_id"].startswith("installation:starring-d2-")
        or record.get("public_origin") != D2_PUBLIC_ORIGIN
        or not isinstance(record.get("completed_at"), str)
        or not UTC_TIMESTAMP.fullmatch(record["completed_at"])
    ):
        fail("record_invalid")
    (
        _onboarding_path,
        onboarding_evidence,
        _onboarding_payload,
        onboarding_sha,
    ) = load_direct_onboarding_evidence(
        record_path.parent,
        {
            "run_id": record["run_id"],
            "manifest_sha256": record["d2_manifest_sha256"],
            "principal_id": record["principal_id"],
            "guild_id": record["guild_id"],
            "discord_application_id": record["discord_application_id"],
            "hub_channel_id": record["hub_channel_id"],
            "installation_id": record["installation_id"],
            "provisioner_sha256": record[
                "candidate_sealed_provisioner_sha256"
            ],
            "issuer_sha256": record["issuer_sha256"],
            "issuer_source_sha256": record["issuer_source_sha256"],
        },
    )
    if onboarding_sha != record["direct_onboarding_evidence_sha256"]:
        fail("direct_onboarding_evidence_digest_mismatch")
    taint_path = record_path.with_name("d2a-taint.json")
    taint = load_json(taint_path, "d2a_taint")
    validate_taint(
        taint,
        {
            "run_id": record["run_id"],
            "manifest_sha256": record["d2_manifest_sha256"],
            "issuer_sha256": record["issuer_sha256"],
            "issuer_source_sha256": record["issuer_source_sha256"],
            "runner_sha256": record["runner_sha256"],
            "product_driver_sha256": record["product_driver_sha256"],
            "scenario_sha256": record["trusted_scenario_sha256"],
        },
    )
    if sha256_bytes(taint_path.read_bytes()) != record["d2a_taint_sha256"]:
        fail("d2a_taint_digest_mismatch")
    evidence_path = record_path.with_name("evidence.json")
    evidence = load_json(evidence_path, "evidence")
    validate_public_evidence(
        evidence,
        record["operation"],
        {
            "run_id": record["run_id"],
            "manifest_sha256": record["d2_manifest_sha256"],
            "public_origin": record["public_origin"],
            "principal_id": record["principal_id"],
            "guild_id": record["guild_id"],
            "installation_id": record["installation_id"],
            **(
                {
                    "scenario_sha256": record["scenario_sha256"],
                    "authoring_session_id": record["authoring_session_id"],
                    "summary": record["expected_summary"],
                }
                if record["operation"] == "one-shot"
                else {}
            ),
        },
        record["scenario_session_id_prefix"]
        if record["operation"] == "one-shot"
        else None,
    )
    if evidence.get("scenario_sha256") != record["scenario_sha256"]:
        fail("scenario_digest_mismatch")
    evidence_observed_at = utc_nanoseconds(evidence.get("observed_at"))
    onboarding_observed_at = utc_nanoseconds(onboarding_evidence.get("observed_at"))
    completed_at = utc_nanoseconds(record.get("completed_at"))
    if not (
        onboarding_observed_at <= evidence_observed_at
        and evidence_observed_at
        <= completed_at
        <= evidence_observed_at + 300_000_000_000
    ):
        fail("record_timestamp_invalid")
    observed = sha256_bytes(canonical_json(evidence).encode("utf-8"))
    if observed != record["evidence_sha256"]:
        fail("evidence_digest_mismatch")
    print(canonical_json({"status": "verified", "release_eligible": False}))


def parser():
    root = pathlib.Path(__file__).resolve().parent
    result = argparse.ArgumentParser()
    commands = result.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run")
    run.add_argument("--manifest", required=True)
    run.add_argument("--operation", required=True, choices=sorted(ALLOWED_OPERATIONS))
    run.add_argument("--scenario")
    run.add_argument(
        "--issuer",
        default=str(root / "session-issuer" / "target" / "release" / "starring-d2-session-issuer"),
    )
    run.add_argument("--node")
    run.add_argument("--runner", default=str(root / "headless_product_runner.mjs"))
    run.add_argument("--output-root", default=str(default_output_root()))
    check = commands.add_parser("verify")
    check.add_argument("--record", required=True)
    return result


def main():
    arguments = parser().parse_args()
    try:
        if arguments.command == "run":
            execute(arguments)
        else:
            verify(arguments)
    except D2AError as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1) from None


if __name__ == "__main__":
    main()
