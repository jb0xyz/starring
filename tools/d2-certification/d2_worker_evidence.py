import hashlib
import pathlib
import re

from d2_certification import (
    CertificationError,
    canonical_json,
    load_json_file,
    require_owned_mode,
)
from d2_orchestrator_contract import fail, utc_now, write_atomic


SNAPSHOT_KIND = "starring.d2.worker-health-snapshot.v1"
AUTHORING_KIND = "starring.d2.worker-authoring-evidence.v1"
BROWSER_KIND = "starring.d2.browser-authoring-evidence.v1"
DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")
IDENTIFIER_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9:._-]{0,191}$")
TIMESTAMP_PATTERN = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?Z$"
)
HEALTH_FIELDS = {
    "schema_version",
    "status",
    "provider",
    "model",
    "reasoning_effort",
    "auth_mode",
    "codex_cli_version",
    "instance_id",
    "worker_source_sha256",
    "concurrency_limit",
    "queue_capacity",
    "request_timeout_ms",
    "active_requests",
    "queued_requests",
    "accepted_requests_total",
    "settled_requests_total",
}
SNAPSHOT_FIELDS = {
    "schema_version",
    "kind",
    "observed_at",
    "manifest_sha256",
    "checkpoint",
    "provider",
    "model",
    "reasoning_effort",
    "auth_mode",
    "codex_cli_version",
    "worker_instance_id",
    "worker_source_sha256",
    "concurrency_limit",
    "queue_capacity",
    "request_timeout_ms",
    "active_requests",
    "queued_requests",
    "accepted_requests_total",
    "settled_requests_total",
}
BROWSER_FIELDS = {
    "schema_version",
    "kind",
    "observed_at",
    "public_origin",
    "authoring_http_status",
    "authoring_session_id",
    "authoring_generation",
    "installation_id",
    "one_shot",
}
AUTHORING_FIELDS = {
    "schema_version",
    "kind",
    "observed_at",
    "manifest_sha256",
    "browser_evidence_sha256",
    "browser_observed_at",
    "provider",
    "model",
    "reasoning_effort",
    "auth_mode",
    "codex_cli_version",
    "worker_instance_id",
    "worker_source_sha256",
    "accepted_requests_before",
    "accepted_requests_after",
    "accepted_requests_delta",
    "settled_requests_before",
    "settled_requests_after",
    "settled_requests_delta",
    "active_requests_after",
    "queued_requests_after",
}


def _require_identifier(value, code):
    if not isinstance(value, str) or not IDENTIFIER_PATTERN.fullmatch(value):
        fail(code)
    return value


def _require_digest(value, code):
    if not isinstance(value, str) or not DIGEST_PATTERN.fullmatch(value):
        fail(code)
    return value


def _require_timestamp(value, code):
    if not isinstance(value, str) or not TIMESTAMP_PATTERN.fullmatch(value):
        fail(code)
    return value


def _require_counter(value, code):
    if type(value) is not int or value < 0 or value > 9223372036854775807:
        fail(code)
    return value


def validate_browser_authoring_evidence(value):
    if (
        not isinstance(value, dict)
        or set(value) != BROWSER_FIELDS
        or type(value["schema_version"]) is not int
        or value["schema_version"] != 1
        or value["kind"] != BROWSER_KIND
        or type(value["authoring_http_status"]) is not int
        or value["authoring_http_status"] not in {200, 201}
        or type(value["authoring_generation"]) is not int
        or value["authoring_generation"] <= 0
        or value["one_shot"] is not True
        or not isinstance(value["public_origin"], str)
        or not value["public_origin"].startswith("https://")
    ):
        fail("browser_authoring_evidence_invalid")
    _require_timestamp(value["observed_at"], "browser_authoring_evidence_invalid")
    _require_identifier(
        value["authoring_session_id"], "browser_authoring_evidence_invalid"
    )
    _require_identifier(value["installation_id"], "browser_authoring_evidence_invalid")
    return value


def validate_worker_health(manifest, health):
    authoring = manifest["authoring"]
    if (
        not isinstance(health, dict)
        or set(health) != HEALTH_FIELDS
        or type(health["schema_version"]) is not int
        or health["schema_version"] != 1
        or health["status"] != "ok"
        or any(
            health[field] != authoring[field]
            for field in ("provider", "model", "reasoning_effort", "auth_mode")
        )
        or health["worker_source_sha256"]
        != manifest["source_trees"]["codex_worker"]["sha256"]
        or health["concurrency_limit"] != 1
        or health["queue_capacity"] != 4
        or health["request_timeout_ms"] != 55000
    ):
        fail("worker_health_evidence_invalid")
    for field in ("provider", "model", "reasoning_effort", "auth_mode", "instance_id"):
        _require_identifier(health[field], "worker_health_evidence_invalid")
    if (
        not isinstance(health["codex_cli_version"], str)
        or not health["codex_cli_version"]
        or len(health["codex_cli_version"].encode("utf-8")) > 191
        or health["codex_cli_version"] != health["codex_cli_version"].strip()
    ):
        fail("worker_health_evidence_invalid")
    _require_digest(health["worker_source_sha256"], "worker_health_evidence_invalid")
    for field in (
        "concurrency_limit",
        "queue_capacity",
        "request_timeout_ms",
        "active_requests",
        "queued_requests",
        "accepted_requests_total",
        "settled_requests_total",
    ):
        _require_counter(health[field], "worker_health_evidence_invalid")
    return health


def worker_snapshot(manifest, manifest_sha256, checkpoint, health, observed_at=None):
    if checkpoint not in {"before", "after"}:
        fail("worker_checkpoint_invalid")
    validate_worker_health(manifest, health)
    return {
        "schema_version": 1,
        "kind": SNAPSHOT_KIND,
        "observed_at": observed_at or utc_now(),
        "manifest_sha256": manifest_sha256,
        "checkpoint": checkpoint,
        "provider": health["provider"],
        "model": health["model"],
        "reasoning_effort": health["reasoning_effort"],
        "auth_mode": health["auth_mode"],
        "codex_cli_version": health["codex_cli_version"],
        "worker_instance_id": health["instance_id"],
        "worker_source_sha256": health["worker_source_sha256"],
        "concurrency_limit": health["concurrency_limit"],
        "queue_capacity": health["queue_capacity"],
        "request_timeout_ms": health["request_timeout_ms"],
        "active_requests": health["active_requests"],
        "queued_requests": health["queued_requests"],
        "accepted_requests_total": health["accepted_requests_total"],
        "settled_requests_total": health["settled_requests_total"],
    }


def validate_worker_snapshot(manifest, manifest_sha256, checkpoint, value):
    if (
        not isinstance(value, dict)
        or set(value) != SNAPSHOT_FIELDS
        or type(value["schema_version"]) is not int
        or value["schema_version"] != 1
        or value["kind"] != SNAPSHOT_KIND
        or value["manifest_sha256"] != manifest_sha256
        or value["checkpoint"] != checkpoint
        or any(
            value[field] != manifest["authoring"][field]
            for field in ("provider", "model", "reasoning_effort", "auth_mode")
        )
        or value["worker_source_sha256"]
        != manifest["source_trees"]["codex_worker"]["sha256"]
        or value["concurrency_limit"] != 1
        or value["queue_capacity"] != 4
        or value["request_timeout_ms"] != 55000
    ):
        fail("worker_snapshot_invalid")
    _require_timestamp(value["observed_at"], "worker_snapshot_invalid")
    _require_digest(value["manifest_sha256"], "worker_snapshot_invalid")
    _require_digest(value["worker_source_sha256"], "worker_snapshot_invalid")
    for field in (
        "provider",
        "model",
        "reasoning_effort",
        "auth_mode",
        "worker_instance_id",
    ):
        _require_identifier(value[field], "worker_snapshot_invalid")
    if (
        not isinstance(value["codex_cli_version"], str)
        or not value["codex_cli_version"]
        or len(value["codex_cli_version"].encode("utf-8")) > 191
        or value["codex_cli_version"] != value["codex_cli_version"].strip()
    ):
        fail("worker_snapshot_invalid")
    for field in (
        "concurrency_limit",
        "queue_capacity",
        "request_timeout_ms",
        "active_requests",
        "queued_requests",
        "accepted_requests_total",
        "settled_requests_total",
    ):
        _require_counter(value[field], "worker_snapshot_invalid")
    return value


def load_private_json(path, label):
    try:
        require_owned_mode(path, 0o600, label)
        return load_json_file(path, label)
    except CertificationError as error:
        fail(str(error))


def snapshot_path(context, checkpoint):
    return context.artifact_directory / "worker-authoring" / f"{checkpoint}.json"


def authoring_path(context):
    return context.artifact_directory / "worker-authoring" / "evidence.json"


def browser_digest(browser):
    validate_browser_authoring_evidence(browser)
    return hashlib.sha256(canonical_json(browser).encode("utf-8")).hexdigest()


def validate_authoring_evidence(context, browser, value):
    if (
        not isinstance(value, dict)
        or set(value) != AUTHORING_FIELDS
        or type(value["schema_version"]) is not int
        or value["schema_version"] != 1
        or value["kind"] != AUTHORING_KIND
        or value["manifest_sha256"] != context.digest
        or value["browser_evidence_sha256"] != browser_digest(browser)
        or value["browser_observed_at"] != browser["observed_at"]
        or any(
            value[field] != context.manifest["authoring"][field]
            for field in ("provider", "model", "reasoning_effort", "auth_mode")
        )
        or value["worker_source_sha256"]
        != context.manifest["source_trees"]["codex_worker"]["sha256"]
        or value["accepted_requests_delta"] != 1
        or value["settled_requests_delta"] != 1
        or value["accepted_requests_after"]
        != value["accepted_requests_before"] + 1
        or value["settled_requests_after"] != value["settled_requests_before"] + 1
        or value["active_requests_after"] != 0
        or value["queued_requests_after"] != 0
    ):
        fail("worker_authoring_evidence_invalid")
    _require_timestamp(value["observed_at"], "worker_authoring_evidence_invalid")
    _require_timestamp(
        value["browser_observed_at"], "worker_authoring_evidence_invalid"
    )
    _require_digest(value["manifest_sha256"], "worker_authoring_evidence_invalid")
    _require_digest(
        value["browser_evidence_sha256"], "worker_authoring_evidence_invalid"
    )
    _require_digest(
        value["worker_source_sha256"], "worker_authoring_evidence_invalid"
    )
    for field in (
        "provider",
        "model",
        "reasoning_effort",
        "auth_mode",
        "worker_instance_id",
    ):
        _require_identifier(value[field], "worker_authoring_evidence_invalid")
    for field in (
        "accepted_requests_before",
        "accepted_requests_after",
        "accepted_requests_delta",
        "settled_requests_before",
        "settled_requests_after",
        "settled_requests_delta",
        "active_requests_after",
        "queued_requests_after",
    ):
        _require_counter(value[field], "worker_authoring_evidence_invalid")
    if (
        not isinstance(value["codex_cli_version"], str)
        or not value["codex_cli_version"]
        or len(value["codex_cli_version"].encode("utf-8")) > 191
    ):
        fail("worker_authoring_evidence_invalid")
    return value


def capture_worker_authoring_checkpoint(context, health, checkpoint, browser=None):
    if checkpoint not in {"before", "after"}:
        fail("worker_checkpoint_invalid")
    if checkpoint == "after" and browser is None:
        fail("browser_authoring_evidence_required")
    if checkpoint == "after":
        browser = validate_browser_authoring_evidence(browser)
    path = snapshot_path(context, checkpoint)
    current = worker_snapshot(context.manifest, context.digest, checkpoint, health)
    if checkpoint == "before" and (
        current["active_requests"] != 0
        or current["queued_requests"] != 0
        or current["accepted_requests_total"] != current["settled_requests_total"]
    ):
        fail("worker_before_not_idle")
    replayed = path.exists()
    if replayed:
        recorded = load_private_json(path, f"worker_{checkpoint}_snapshot")
        validate_worker_snapshot(context.manifest, context.digest, checkpoint, recorded)
        if checkpoint == "before" and {
            key: value for key, value in current.items() if key != "observed_at"
        } != {key: value for key, value in recorded.items() if key != "observed_at"}:
            fail("worker_checkpoint_replay_drift")
        current = recorded
    else:
        write_atomic(path, canonical_json(current) + "\n")
        current = load_private_json(path, f"worker_{checkpoint}_snapshot")
        validate_worker_snapshot(context.manifest, context.digest, checkpoint, current)
    if checkpoint == "before":
        return {
            "status": "exact_replay" if replayed else "recorded",
            "checkpoint": checkpoint,
            "evidence": str(path),
        }
    before_path = snapshot_path(context, "before")
    before = load_private_json(before_path, "worker_before_snapshot")
    validate_worker_snapshot(context.manifest, context.digest, "before", before)
    if any(
        before[field] != current[field]
        for field in (
            "provider",
            "model",
            "reasoning_effort",
            "auth_mode",
            "codex_cli_version",
            "worker_instance_id",
            "worker_source_sha256",
            "concurrency_limit",
            "queue_capacity",
            "request_timeout_ms",
        )
    ):
        fail("worker_identity_changed")
    evidence = {
        "schema_version": 1,
        "kind": AUTHORING_KIND,
        "observed_at": current["observed_at"],
        "manifest_sha256": context.digest,
        "browser_evidence_sha256": browser_digest(browser),
        "browser_observed_at": browser["observed_at"],
        "provider": current["provider"],
        "model": current["model"],
        "reasoning_effort": current["reasoning_effort"],
        "auth_mode": current["auth_mode"],
        "codex_cli_version": current["codex_cli_version"],
        "worker_instance_id": current["worker_instance_id"],
        "worker_source_sha256": current["worker_source_sha256"],
        "accepted_requests_before": before["accepted_requests_total"],
        "accepted_requests_after": current["accepted_requests_total"],
        "accepted_requests_delta": current["accepted_requests_total"]
        - before["accepted_requests_total"],
        "settled_requests_before": before["settled_requests_total"],
        "settled_requests_after": current["settled_requests_total"],
        "settled_requests_delta": current["settled_requests_total"]
        - before["settled_requests_total"],
        "active_requests_after": current["active_requests"],
        "queued_requests_after": current["queued_requests"],
    }
    validate_authoring_evidence(context, browser, evidence)
    final_path = authoring_path(context)
    if final_path.exists():
        recorded = load_private_json(final_path, "worker_authoring_evidence")
        validate_authoring_evidence(context, browser, recorded)
        if recorded != evidence:
            fail("worker_authoring_evidence_replay_drift")
        status = "exact_replay"
    else:
        write_atomic(final_path, canonical_json(evidence) + "\n")
        recorded = load_private_json(final_path, "worker_authoring_evidence")
        validate_authoring_evidence(context, browser, recorded)
        status = "recorded"
    return {
        "status": status,
        "checkpoint": checkpoint,
        "evidence": str(final_path),
    }
