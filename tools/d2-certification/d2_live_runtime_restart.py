import contextlib
import datetime
import hashlib
import json
import os
import pathlib
import re
import stat
import time

from d2_certification import (
    LIVE_FRESH_LEASE_CHECKPOINT,
    canonical_json,
    fsync_directory,
    load_receipts_from_handle,
    open_locked_receipts,
    require_owned_mode,
)
from d2_drained_runtime_restart import (
    command_restart_drained_runtime,
    drained_runtime_restart_dependency_snapshot,
    drained_runtime_restart_identity,
    require_pinned_transport_snapshot,
    runtime_job_observation,
    runtime_ready_status,
    valid_drained_runtime_restart_dependencies,
    valid_drained_runtime_restart_identity,
)
from d2_orchestrator_contract import (
    append_journal,
    fail,
    load_json,
    load_state,
    standing_snapshot,
    utc_now,
    write_atomic,
)


RECORDED_AT_PATTERN = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$"
)
UTC_TIMESTAMP_PATTERN = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?Z$"
)
OPERATION_PATTERN = re.compile(
    r"^d2:[0-9a-f]{16}:certify-live-runtime-restart$"
)
TRANSPORT_INSTANCE_PATTERN = re.compile(r"^d2ti-[0-9a-f]{32}$")
RECEIPT_DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")
IDENTIFIER_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$")
PROCESS_INSTANCE_PATTERN = re.compile(r"^[0-9a-f]{32}$")
TEMPORARY_FILE_PATTERN = re.compile(
    r"^\.(?:intent|shutdown|awaiting-confirmation|complete|step-11-evidence)"
    r"\.json\.[1-9][0-9]*\.tmp$"
)
CONFIRMATION_KIND = "starring.d2.live-runtime-restart-confirmation.v1"
CONFIRMATION_MAXIMUM_BYTES = 16 * 1024
SERVING_LEASE_MAXIMUM_NANOSECONDS = 45 * 1_000_000_000
LIVE_EXIT_TIMEOUT_SECONDS = 30
LIVE_EXIT_STABILITY_SECONDS = 30


def monotonic_time():
    return time.monotonic()


def wait_interval(seconds):
    time.sleep(seconds)


def live_runtime_restart_directory(context):
    return context.artifact_directory / "live-runtime-restart"


def live_runtime_restart_intent_path(context):
    return live_runtime_restart_directory(context) / "intent.json"


def live_runtime_restart_complete_path(context):
    return live_runtime_restart_directory(context) / "complete.json"


def live_runtime_restart_shutdown_path(context):
    return live_runtime_restart_directory(context) / "shutdown.json"


def live_runtime_restart_awaiting_path(context):
    return live_runtime_restart_directory(context) / "awaiting-confirmation.json"


def live_runtime_restart_evidence_path(context):
    return live_runtime_restart_directory(context) / "step-11-evidence.json"


def ensure_live_runtime_restart_directory(context):
    directory = live_runtime_restart_directory(context)
    if not directory.exists():
        try:
            directory.mkdir(mode=0o700)
        except OSError:
            fail("live_runtime_restart_evidence_directory_invalid")
        fsync_directory(
            context.artifact_directory, "live_runtime_restart_directory_parent"
        )
    metadata = directory.lstat()
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or directory.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        fail("live_runtime_restart_evidence_directory_invalid")
    try:
        entries = list(directory.iterdir())
    except OSError:
        fail("live_runtime_restart_evidence_inventory_invalid")
    removed = False
    for entry in entries:
        try:
            entry_metadata = entry.lstat()
        except OSError:
            fail("live_runtime_restart_evidence_inventory_invalid")
        if TEMPORARY_FILE_PATTERN.fullmatch(entry.name):
            if (
                not stat.S_ISREG(entry_metadata.st_mode)
                or entry.is_symlink()
                or entry_metadata.st_uid != os.getuid()
                or stat.S_IMODE(entry_metadata.st_mode) != 0o600
            ):
                fail("live_runtime_restart_evidence_inventory_invalid")
            try:
                entry.unlink()
            except OSError:
                fail("live_runtime_restart_temporary_cleanup_failed")
            removed = True
            continue
        if entry.name not in {
            "intent.json",
            "shutdown.json",
            "awaiting-confirmation.json",
            "complete.json",
            "step-11-evidence.json",
        }:
            fail("live_runtime_restart_evidence_inventory_invalid")
        if (
            not stat.S_ISREG(entry_metadata.st_mode)
            or entry.is_symlink()
            or entry_metadata.st_uid != os.getuid()
            or stat.S_IMODE(entry_metadata.st_mode) != 0o600
        ):
            fail("live_runtime_restart_evidence_inventory_invalid")
    if removed:
        fsync_directory(directory, "live_runtime_restart_temporary_cleanup")
    return directory


def step_11_prerequisites(context, receipts):
    live = receipts[7]["evidence"]
    route = receipts[8]["evidence"]
    return {
        "receipt_chain_head_sha256": receipts[-1]["receipt_sha256"],
        "transport_instance_id": receipts[2]["evidence"][
            "transport_instance_id"
        ],
        "deployment_identity": {
            "deployment_id": route["deployment_id"],
            "route_id": route["route_id"],
            "instance_id": route["instance_id"],
        },
        "canonical_scope": {
            "installation_id": live["installation_id"],
            "promotion_id": live["promotion_id"],
            "public_origin": context.manifest["cloudflare"]["public_origin"],
        },
        "prior_live_witness": {
            "deployment_id": live["deployment_id"],
            "route_id": live["route_id"],
            "attestation_id": live["attestation_id"],
            "serving_lease_id": live["serving_lease_id"],
        },
    }


@contextlib.contextmanager
def locked_step_11_prerequisites(context):
    receipts_path = context.manifest_path.with_name("receipts.jsonl")
    require_owned_mode(receipts_path, 0o600, "receipts")
    with open_locked_receipts(receipts_path, False) as handle:
        receipts = load_receipts_from_handle(
            handle, context.manifest, context.digest
        )
        if len(receipts) != 10:
            fail("live_runtime_restart_prerequisites_invalid")
        yield step_11_prerequisites(context, receipts)


def valid_deployment_identity(value):
    return (
        isinstance(value, dict)
        and set(value) == {"deployment_id", "route_id", "instance_id"}
        and all(
            isinstance(value[field], str)
            and IDENTIFIER_PATTERN.fullmatch(value[field])
            for field in value
        )
    )


def valid_canonical_scope(value):
    return (
        isinstance(value, dict)
        and set(value)
        == {"installation_id", "promotion_id", "public_origin"}
        and isinstance(value["installation_id"], str)
        and IDENTIFIER_PATTERN.fullmatch(value["installation_id"])
        and isinstance(value["promotion_id"], str)
        and RECEIPT_DIGEST_PATTERN.fullmatch(value["promotion_id"])
        and isinstance(value["public_origin"], str)
        and 1 <= len(value["public_origin"]) <= 2048
        and value["public_origin"].startswith("https://")
    )


def valid_prior_live_witness(value):
    return (
        isinstance(value, dict)
        and set(value)
        == {
            "deployment_id",
            "route_id",
            "attestation_id",
            "serving_lease_id",
        }
        and all(
            isinstance(value[field], str)
            and IDENTIFIER_PATTERN.fullmatch(value[field])
            for field in value
        )
    )


def validate_live_runtime_restart_intent(context, intent, prerequisites):
    if (
        not isinstance(intent, dict)
        or set(intent)
        != {
            "schema_version",
            "manifest_sha256",
            "recorded_at",
            "operation_id",
            "receipt_chain_head_sha256",
            "checkpoint",
            "deployment_identity",
            "canonical_scope",
            "prior_live_witness",
            "runtime_identity",
            "old_pid",
            "old_process_instance_id",
            "old_runs",
            "transport_instance_id",
            "dependencies",
            "standing_snapshot",
        }
        or type(intent["schema_version"]) is not int
        or intent["schema_version"] != 1
        or intent["manifest_sha256"] != context.digest
        or not isinstance(intent["recorded_at"], str)
        or not RECORDED_AT_PATTERN.fullmatch(intent["recorded_at"])
        or not isinstance(intent["operation_id"], str)
        or not OPERATION_PATTERN.fullmatch(intent["operation_id"])
        or intent["operation_id"]
        != f"d2:{context.digest[:16]}:certify-live-runtime-restart"
        or intent["receipt_chain_head_sha256"]
        != prerequisites["receipt_chain_head_sha256"]
        or not RECEIPT_DIGEST_PATTERN.fullmatch(
            intent["receipt_chain_head_sha256"]
        )
        or intent["checkpoint"] != LIVE_FRESH_LEASE_CHECKPOINT
        or intent["deployment_identity"] != prerequisites["deployment_identity"]
        or not valid_deployment_identity(intent["deployment_identity"])
        or intent["canonical_scope"] != prerequisites["canonical_scope"]
        or not valid_canonical_scope(intent["canonical_scope"])
        or intent["prior_live_witness"] != prerequisites["prior_live_witness"]
        or not valid_prior_live_witness(intent["prior_live_witness"])
        or not valid_drained_runtime_restart_identity(
            context, intent["runtime_identity"]
        )
        or type(intent["old_pid"]) is not int
        or intent["old_pid"] <= 0
        or not isinstance(intent["old_process_instance_id"], str)
        or not PROCESS_INSTANCE_PATTERN.fullmatch(
            intent["old_process_instance_id"]
        )
        or type(intent["old_runs"]) is not int
        or intent["old_runs"] <= 0
        or not isinstance(intent["transport_instance_id"], str)
        or not TRANSPORT_INSTANCE_PATTERN.fullmatch(
            intent["transport_instance_id"]
        )
        or intent["transport_instance_id"]
        != prerequisites["transport_instance_id"]
        or not valid_drained_runtime_restart_dependencies(
            context, intent["dependencies"]
        )
        or not isinstance(intent["standing_snapshot"], dict)
    ):
        fail("live_runtime_restart_intent_invalid")


def valid_shutdown_observation(intent, value):
    return (
        isinstance(value, dict)
        and set(value)
        == {
            "loaded",
            "pid",
            "state",
            "last_exit_code",
            "plist_path",
            "program_arguments",
            "runs",
        }
        and value["loaded"] is True
        and value["pid"] is None
        and value["state"] == "exited"
        and value["last_exit_code"] == 0
        and value["plist_path"] == intent["runtime_identity"]["plist_path"]
        and value["program_arguments"]
        == intent["runtime_identity"]["program_arguments"]
        and value["runs"] == intent["old_runs"]
    )


def validate_live_runtime_restart_shutdown(shutdown, intent):
    if (
        not isinstance(shutdown, dict)
        or set(shutdown)
        != set(intent) | {"shutdown_observation", "stability_seconds"}
        or any(
            shutdown[field] != value
            for field, value in intent.items()
            if field != "recorded_at"
        )
        or not isinstance(shutdown["recorded_at"], str)
        or not RECORDED_AT_PATTERN.fullmatch(shutdown["recorded_at"])
        or shutdown["stability_seconds"] != LIVE_EXIT_STABILITY_SECONDS
        or not valid_shutdown_observation(
            intent, shutdown["shutdown_observation"]
        )
    ):
        fail("live_runtime_restart_shutdown_invalid")


def validate_live_runtime_restart_awaiting(awaiting, intent, shutdown):
    inherited = set(intent)
    if (
        not isinstance(awaiting, dict)
        or set(awaiting)
        != inherited
        | {
            "new_pid",
            "process_instance_id",
            "new_runs",
            "ready_after_restart",
            "drained_restart_operation_id",
            "shutdown_boundary",
        }
        or any(
            awaiting[field] != value
            for field, value in intent.items()
            if field != "recorded_at"
        )
        or not isinstance(awaiting["recorded_at"], str)
        or not RECORDED_AT_PATTERN.fullmatch(awaiting["recorded_at"])
        or type(awaiting["new_pid"]) is not int
        or awaiting["new_pid"] <= 0
        or awaiting["new_pid"] == intent["old_pid"]
        or not isinstance(awaiting["process_instance_id"], str)
        or not PROCESS_INSTANCE_PATTERN.fullmatch(
            awaiting["process_instance_id"]
        )
        or awaiting["process_instance_id"]
        == intent["old_process_instance_id"]
        or type(awaiting["new_runs"]) is not int
        or awaiting["new_runs"] <= 0
        or awaiting["ready_after_restart"] is not True
        or not isinstance(awaiting["drained_restart_operation_id"], str)
        or not re.fullmatch(
            r"d2:[0-9a-f]{16}:restart-drained-runtime:[0-9]{4}",
            awaiting["drained_restart_operation_id"],
        )
        or awaiting["shutdown_boundary"] != shutdown["recorded_at"]
    ):
        fail("live_runtime_restart_awaiting_invalid")


def utc_timestamp_key(value):
    if not isinstance(value, str) or not UTC_TIMESTAMP_PATTERN.fullmatch(value):
        raise ValueError("timestamp_invalid")
    whole, separator, fraction_with_suffix = value[:-1].partition(".")
    try:
        parsed = datetime.datetime.strptime(whole, "%Y-%m-%dT%H:%M:%S")
    except ValueError:
        raise ValueError("timestamp_invalid") from None
    fraction = fraction_with_suffix if separator else ""
    nanoseconds = int(fraction.ljust(9, "0")) if fraction else 0
    epoch = datetime.datetime(1970, 1, 1)
    seconds = int((parsed - epoch).total_seconds())
    return seconds, nanoseconds


def current_utc_timestamp_key():
    return divmod(time.time_ns(), 1_000_000_000)


def valid_canonical_confirmation_shape(value):
    if (
        not isinstance(value, dict)
        or set(value)
        != {
            "schema_version",
            "kind",
            "checkpoint",
            "operation_id",
            "installation_id",
            "promotion_id",
            "public_origin",
            "shutdown_boundary",
            "observed_at",
            "product_state",
            "operational_state",
            "runtime_phase",
            "serving_state",
            "attestation_revision",
            "process_instance_id",
            "last_heartbeat_at",
            "lease_expires_at",
        }
        or type(value["schema_version"]) is not int
        or value["schema_version"] != 1
        or value["kind"] != CONFIRMATION_KIND
        or value["checkpoint"] != LIVE_FRESH_LEASE_CHECKPOINT
        or not isinstance(value["operation_id"], str)
        or not OPERATION_PATTERN.fullmatch(value["operation_id"])
        or not isinstance(value["installation_id"], str)
        or not IDENTIFIER_PATTERN.fullmatch(value["installation_id"])
        or not isinstance(value["promotion_id"], str)
        or not RECEIPT_DIGEST_PATTERN.fullmatch(value["promotion_id"])
        or not isinstance(value["public_origin"], str)
        or not value["public_origin"].startswith("https://")
        or len(value["public_origin"]) > 2048
        or value["product_state"] != "live"
        or value["operational_state"] != "live"
        or value["runtime_phase"] != "live"
        or value["serving_state"] != "fresh"
        or type(value["attestation_revision"]) is not int
        or value["attestation_revision"] <= 0
        or not isinstance(value["process_instance_id"], str)
        or not PROCESS_INSTANCE_PATTERN.fullmatch(
            value["process_instance_id"]
        )
    ):
        return False
    try:
        boundary = utc_timestamp_key(value["shutdown_boundary"])
        observed = utc_timestamp_key(value["observed_at"])
        heartbeat = utc_timestamp_key(value["last_heartbeat_at"])
        expires = utc_timestamp_key(value["lease_expires_at"])
    except ValueError:
        return False
    heartbeat_value = heartbeat[0] * 1_000_000_000 + heartbeat[1]
    observed_value = observed[0] * 1_000_000_000 + observed[1]
    expires_value = expires[0] * 1_000_000_000 + expires[1]
    return (
        boundary < heartbeat <= observed < expires
        and expires_value - heartbeat_value
        <= SERVING_LEASE_MAXIMUM_NANOSECONDS
        and observed_value - heartbeat_value
        <= SERVING_LEASE_MAXIMUM_NANOSECONDS
    )


def canonical_confirmation_digest(confirmation):
    return hashlib.sha256(canonical_json(confirmation).encode("utf-8")).hexdigest()


def validate_live_runtime_restart_completion(completion, awaiting):
    if (
        not isinstance(completion, dict)
        or set(completion)
        != set(awaiting)
        | {"canonical_confirmation", "canonical_confirmation_sha256"}
        or any(
            completion[field] != value
            for field, value in awaiting.items()
            if field != "recorded_at"
        )
        or not isinstance(completion["recorded_at"], str)
        or not RECORDED_AT_PATTERN.fullmatch(completion["recorded_at"])
        or not valid_canonical_confirmation_shape(
            completion["canonical_confirmation"]
        )
        or not isinstance(completion["canonical_confirmation_sha256"], str)
        or not RECEIPT_DIGEST_PATTERN.fullmatch(
            completion["canonical_confirmation_sha256"]
        )
        or completion["canonical_confirmation_sha256"]
        != canonical_confirmation_digest(
            completion["canonical_confirmation"]
        )
        or completion["canonical_confirmation"]["operation_id"]
        != awaiting["operation_id"]
        or completion["canonical_confirmation"]["installation_id"]
        != awaiting["canonical_scope"]["installation_id"]
        or completion["canonical_confirmation"]["promotion_id"]
        != awaiting["canonical_scope"]["promotion_id"]
        or completion["canonical_confirmation"]["public_origin"]
        != awaiting["canonical_scope"]["public_origin"]
        or completion["canonical_confirmation"]["shutdown_boundary"]
        != awaiting["shutdown_boundary"]
        or completion["canonical_confirmation"]["process_instance_id"]
        != awaiting["process_instance_id"]
    ):
        fail("live_runtime_restart_completion_invalid")


def load_live_runtime_restart_records(context, prerequisites):
    directory = live_runtime_restart_directory(context)
    if not directory.exists():
        return None, None, None, None
    ensure_live_runtime_restart_directory(context)
    intent_path = live_runtime_restart_intent_path(context)
    shutdown_path = live_runtime_restart_shutdown_path(context)
    awaiting_path = live_runtime_restart_awaiting_path(context)
    complete_path = live_runtime_restart_complete_path(context)
    evidence_path = live_runtime_restart_evidence_path(context)
    if (
        shutdown_path.exists()
        or awaiting_path.exists()
        or complete_path.exists()
        or evidence_path.exists()
    ) and not intent_path.exists():
        fail("live_runtime_restart_evidence_inventory_invalid")
    if not intent_path.exists():
        return None, None, None, None
    intent = load_json(intent_path, "live_runtime_restart_intent_invalid")
    validate_live_runtime_restart_intent(context, intent, prerequisites)
    if not shutdown_path.exists():
        if (
            awaiting_path.exists()
            or complete_path.exists()
            or evidence_path.exists()
        ):
            fail("live_runtime_restart_evidence_inventory_invalid")
        return intent, None, None, None
    shutdown = load_json(
        shutdown_path, "live_runtime_restart_shutdown_invalid"
    )
    validate_live_runtime_restart_shutdown(shutdown, intent)
    if not awaiting_path.exists():
        if complete_path.exists() or evidence_path.exists():
            fail("live_runtime_restart_evidence_inventory_invalid")
        return intent, shutdown, None, None
    awaiting = load_json(
        awaiting_path, "live_runtime_restart_awaiting_invalid"
    )
    validate_live_runtime_restart_awaiting(awaiting, intent, shutdown)
    if not complete_path.exists():
        if evidence_path.exists():
            fail("live_runtime_restart_evidence_inventory_invalid")
        return intent, shutdown, awaiting, None
    completion = load_json(
        complete_path, "live_runtime_restart_completion_invalid"
    )
    validate_live_runtime_restart_completion(completion, awaiting)
    return intent, shutdown, awaiting, completion


def write_live_runtime_restart_record(context, name, record):
    ensure_live_runtime_restart_directory(context)
    path = live_runtime_restart_directory(context) / f"{name}.json"
    if os.path.lexists(path):
        fail("live_runtime_restart_evidence_busy")
    write_atomic(path, canonical_json(record) + "\n")
    return path


def strict_json_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate_key")
        result[key] = value
    return result


def read_canonical_confirmation(path):
    candidate = pathlib.Path(path)
    if not candidate.is_absolute():
        fail("live_runtime_restart_confirmation_path_invalid")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(candidate, flags)
    except OSError:
        fail("live_runtime_restart_confirmation_file_invalid")
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_size <= 0
            or metadata.st_size > CONFIRMATION_MAXIMUM_BYTES
        ):
            fail("live_runtime_restart_confirmation_file_invalid")
        raw = bytearray()
        while len(raw) <= CONFIRMATION_MAXIMUM_BYTES:
            limit = min(
                4096, CONFIRMATION_MAXIMUM_BYTES + 1 - len(raw)
            )
            chunk = os.read(descriptor, limit)
            if not chunk:
                break
            raw.extend(chunk)
        if len(raw) != metadata.st_size or len(raw) > CONFIRMATION_MAXIMUM_BYTES:
            fail("live_runtime_restart_confirmation_file_invalid")
    finally:
        os.close(descriptor)
    try:
        confirmation = json.loads(
            raw.decode("utf-8"), object_pairs_hook=strict_json_object
        )
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
        fail("live_runtime_restart_confirmation_invalid")
    if not valid_canonical_confirmation_shape(confirmation):
        fail("live_runtime_restart_confirmation_invalid")
    return confirmation


def require_bound_canonical_confirmation(path, awaiting):
    confirmation = read_canonical_confirmation(path)
    scope = awaiting["canonical_scope"]
    if (
        confirmation["installation_id"] != scope["installation_id"]
        or confirmation["promotion_id"] != scope["promotion_id"]
        or confirmation["public_origin"] != scope["public_origin"]
        or confirmation["operation_id"] != awaiting["operation_id"]
        or confirmation["shutdown_boundary"] != awaiting["shutdown_boundary"]
        or confirmation["process_instance_id"]
        != awaiting["process_instance_id"]
    ):
        fail("live_runtime_restart_confirmation_scope_mismatch")
    try:
        now = current_utc_timestamp_key()
        observed = utc_timestamp_key(confirmation["observed_at"])
        expires = utc_timestamp_key(confirmation["lease_expires_at"])
    except ValueError:
        fail("live_runtime_restart_confirmation_invalid")
    if now < observed or now >= expires:
        fail("live_runtime_restart_confirmation_expired")
    return confirmation


def live_runtime_restart_journal_contains(context, status, operation_id):
    if not context.journal_path.exists():
        return False
    metadata = context.journal_path.lstat()
    if (
        not stat.S_ISREG(metadata.st_mode)
        or context.journal_path.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o600
        or metadata.st_size > 8 * 1024 * 1024
    ):
        fail("journal_invalid")
    found = False
    try:
        lines = context.journal_path.read_text(encoding="utf-8").splitlines()
    except (OSError, UnicodeDecodeError):
        fail("journal_invalid")
    for expected_sequence, line in enumerate(lines, 1):
        try:
            receipt = json.loads(line)
        except json.JSONDecodeError:
            fail("journal_invalid")
        if (
            not isinstance(receipt, dict)
            or set(receipt)
            != {
                "schema_version",
                "sequence",
                "recorded_at",
                "manifest_sha256",
                "action",
                "status",
                "target",
            }
            or receipt["schema_version"] != 1
            or receipt["sequence"] != expected_sequence
            or receipt["manifest_sha256"] != context.digest
        ):
            fail("journal_invalid")
        if (
            receipt["action"] == "live_runtime_restart"
            and receipt["status"] == status
            and receipt["target"] == operation_id.replace(":", "_")
        ):
            if found:
                fail("journal_invalid")
            found = True
    return found


def ensure_live_runtime_restart_journal(context, status, operation_id):
    if not live_runtime_restart_journal_contains(context, status, operation_id):
        append_journal(
            context,
            "live_runtime_restart",
            status,
            operation_id.replace(":", "_"),
        )


def require_fresh_live_runtime(context, platform, identity):
    before = runtime_job_observation(platform, identity)
    ready = runtime_ready_status(context, platform)
    process = platform.runtime_process_identity(context)
    after = runtime_job_observation(platform, identity)
    if (
        before != after
        or not before["loaded"]
        or type(before["pid"]) is not int
        or before["pid"] <= 0
        or before["state"] != "running"
        or ready != 200
        or not isinstance(process, dict)
        or process.get("os_pid") != before["pid"]
        or not isinstance(process.get("process_instance_id"), str)
        or not PROCESS_INSTANCE_PATTERN.fullmatch(
            process["process_instance_id"]
        )
    ):
        fail("live_runtime_restart_precondition_unready")
    return before, process["process_instance_id"]


def require_clean_runtime_exit_observation(
    context, platform, identity, intent
):
    observation = runtime_job_observation(platform, identity)
    if (
        observation["pid"] is not None
        or not observation["loaded"]
        or observation["state"] != "exited"
        or observation["last_exit_code"] != 0
        or observation["runs"] != intent["old_runs"]
        or runtime_ready_status(context, platform) == 200
    ):
        fail("live_runtime_restart_shutdown_unsuccessful")
    return observation


def wait_for_clean_runtime_exit(
    context, platform, identity, intent, shutdown_deadline=None
):
    deadline = (
        monotonic_time() + LIVE_EXIT_TIMEOUT_SECONDS
        if shutdown_deadline is None
        else shutdown_deadline
    )
    while True:
        observation = runtime_job_observation(platform, identity)
        if observation["pid"] is None:
            if monotonic_time() > deadline:
                fail("live_runtime_restart_shutdown_timeout")
            require_clean_runtime_exit_observation(
                context, platform, identity, intent
            )
            break
        if observation["pid"] != intent["old_pid"]:
            fail("live_runtime_restart_unjournaled_pid")
        if monotonic_time() >= deadline:
            fail("live_runtime_restart_shutdown_timeout")
        wait_interval(0.1)
    stability_deadline = monotonic_time() + LIVE_EXIT_STABILITY_SECONDS
    while True:
        stable = require_clean_runtime_exit_observation(
            context, platform, identity, intent
        )
        if monotonic_time() >= stability_deadline:
            return stable
        wait_interval(0.25)


def require_live_runtime_restart_scope(
    context, platform, state, intent, prerequisites
):
    identity = drained_runtime_restart_identity(context)
    dependencies = drained_runtime_restart_dependency_snapshot(context, platform)
    transport = platform.transport_control(context, "snapshot")
    require_pinned_transport_snapshot(context, transport)
    if (
        prerequisites["receipt_chain_head_sha256"]
        != intent["receipt_chain_head_sha256"]
        or prerequisites["deployment_identity"] != intent["deployment_identity"]
        or identity != intent["runtime_identity"]
        or dependencies != intent["dependencies"]
        or transport["instance_id"] != intent["transport_instance_id"]
        or transport["instance_id"] != prerequisites["transport_instance_id"]
        or standing_snapshot(context, platform) != state["standing_snapshot"]
    ):
        fail("live_runtime_restart_scope_changed")
    return identity


def step_11_evidence(completion):
    identity = completion["deployment_identity"]
    confirmation = completion["canonical_confirmation"]
    return {
        "old_pid": completion["old_pid"],
        "new_pid": completion["new_pid"],
        "runtime_sha256": completion["runtime_identity"]["runtime_sha256"],
        "ready_after_restart": True,
        "process_identity_joined": True,
        "process_instance_id": completion["process_instance_id"],
        "checkpoint": LIVE_FRESH_LEASE_CHECKPOINT,
        "deployment_id": identity["deployment_id"],
        "route_id": identity["route_id"],
        "instance_id": identity["instance_id"],
        "canonical_confirmation_sha256": completion[
            "canonical_confirmation_sha256"
        ],
        "operation_id": completion["operation_id"],
        "shutdown_boundary": completion["shutdown_boundary"],
        "installation_id": confirmation["installation_id"],
        "promotion_id": confirmation["promotion_id"],
        "attestation_revision": confirmation["attestation_revision"],
        "public_origin": confirmation["public_origin"],
    }


def publish_step_11_evidence(context, completion):
    evidence = step_11_evidence(completion)
    path = live_runtime_restart_evidence_path(context)
    if path.exists():
        existing = load_json(
            path, "live_runtime_restart_step_11_evidence_invalid"
        )
        if existing != evidence:
            fail("live_runtime_restart_step_11_evidence_changed")
    else:
        write_live_runtime_restart_record(context, "step-11-evidence", evidence)
    return evidence, path


def require_completed_live_runtime_restart(
    context, platform, state, completion, prerequisites
):
    identity = require_live_runtime_restart_scope(
        context, platform, state, completion, prerequisites
    )
    final, process_instance_id = require_bound_running_runtime_identity(
        context,
        platform,
        identity,
        completion["new_pid"],
        completion["new_runs"],
        "live_runtime_restart_replay_drift",
    )
    if process_instance_id != completion["process_instance_id"]:
        fail("live_runtime_restart_replay_drift")
    require_live_runtime_restart_scope(
        context, platform, state, completion, prerequisites
    )
    if runtime_job_observation(platform, identity) != final:
        fail("live_runtime_restart_replay_drift")
    return final


def require_stable_running_runtime(
    context, platform, identity, expected_pid, expected_runs, code
):
    before = runtime_job_observation(platform, identity)
    ready = runtime_ready_status(context, platform)
    after = runtime_job_observation(platform, identity)
    if (
        before != after
        or before["pid"] != expected_pid
        or before["runs"] != expected_runs
        or before["state"] != "running"
        or ready != 200
    ):
        fail(code)
    return after


def require_bound_running_runtime_identity(
    context, platform, identity, expected_pid, expected_runs, code
):
    before = require_stable_running_runtime(
        context,
        platform,
        identity,
        expected_pid,
        expected_runs,
        code,
    )
    process = platform.runtime_process_identity(context)
    after = require_stable_running_runtime(
        context,
        platform,
        identity,
        expected_pid,
        expected_runs,
        code,
    )
    if (
        before != after
        or not isinstance(process, dict)
        or process.get("os_pid") != expected_pid
        or not isinstance(process.get("process_instance_id"), str)
        or not PROCESS_INSTANCE_PATTERN.fullmatch(
            process["process_instance_id"]
        )
    ):
        fail(code)
    return after, process["process_instance_id"]


def live_runtime_restart_result(status, completion, evidence, evidence_path):
    return {
        "status": status,
        "phase": "candidate_started",
        "operation_id": completion["operation_id"],
        "old_pid": evidence["old_pid"],
        "new_pid": evidence["new_pid"],
        "runtime_sha256": evidence["runtime_sha256"],
        "ready_after_restart": True,
        "process_identity_joined": True,
        "process_instance_id": evidence["process_instance_id"],
        "checkpoint": evidence["checkpoint"],
        "deployment_id": evidence["deployment_id"],
        "route_id": evidence["route_id"],
        "instance_id": evidence["instance_id"],
        "canonical_confirmation_sha256": evidence[
            "canonical_confirmation_sha256"
        ],
        "evidence_path": str(evidence_path),
    }


def awaiting_canonical_confirmation_result(awaiting):
    return {
        "status": "awaiting_canonical_confirmation",
        "phase": "candidate_started",
        "operation_id": awaiting["operation_id"],
        "old_pid": awaiting["old_pid"],
        "new_pid": awaiting["new_pid"],
        "process_instance_id": awaiting["process_instance_id"],
        "runtime_sha256": awaiting["runtime_identity"]["runtime_sha256"],
        "ready_after_restart": True,
        "checkpoint": awaiting["checkpoint"],
        "installation_id": awaiting["canonical_scope"]["installation_id"],
        "promotion_id": awaiting["canonical_scope"]["promotion_id"],
        "public_origin": awaiting["canonical_scope"]["public_origin"],
        "shutdown_boundary": awaiting["shutdown_boundary"],
        "confirmation_required": True,
    }


def certify_live_runtime_restart(
    context, platform, prerequisites, confirmation_path=None
):
    state = load_state(context, {"candidate_started"})
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    intent, shutdown, awaiting, completion = load_live_runtime_restart_records(
        context, prerequisites
    )
    if (
        completion is None
        and awaiting is None
        and confirmation_path is not None
    ):
        fail("live_runtime_restart_confirmation_premature")
    if completion is not None:
        require_completed_live_runtime_restart(
            context, platform, state, completion, prerequisites
        )
        if confirmation_path is not None:
            confirmation = read_canonical_confirmation(confirmation_path)
            if (
                canonical_confirmation_digest(confirmation)
                != completion["canonical_confirmation_sha256"]
            ):
                fail("live_runtime_restart_confirmation_changed")
        ensure_live_runtime_restart_journal(
            context, "complete", completion["operation_id"]
        )
        evidence, evidence_path = publish_step_11_evidence(context, completion)
        return live_runtime_restart_result(
            "exact_replay", completion, evidence, evidence_path
        )
    if intent is None:
        identity = drained_runtime_restart_identity(context)
        dependencies = drained_runtime_restart_dependency_snapshot(
            context, platform
        )
        transport = platform.transport_control(context, "snapshot")
        require_pinned_transport_snapshot(context, transport)
        if transport["instance_id"] != prerequisites["transport_instance_id"]:
            fail("live_runtime_restart_transport_changed")
        old_job, old_process_instance_id = require_fresh_live_runtime(
            context, platform, identity
        )
        if standing_snapshot(context, platform) != state["standing_snapshot"]:
            fail("protected_staging_state_changed")
        operation_id = (
            f"d2:{context.digest[:16]}:certify-live-runtime-restart"
        )
        intent = {
            "schema_version": 1,
            "manifest_sha256": context.digest,
            "recorded_at": utc_now(),
            "operation_id": operation_id,
            "receipt_chain_head_sha256": prerequisites[
                "receipt_chain_head_sha256"
            ],
            "checkpoint": LIVE_FRESH_LEASE_CHECKPOINT,
            "deployment_identity": prerequisites["deployment_identity"],
            "canonical_scope": prerequisites["canonical_scope"],
            "prior_live_witness": prerequisites["prior_live_witness"],
            "runtime_identity": identity,
            "old_pid": old_job["pid"],
            "old_process_instance_id": old_process_instance_id,
            "old_runs": old_job["runs"],
            "transport_instance_id": transport["instance_id"],
            "dependencies": dependencies,
            "standing_snapshot": state["standing_snapshot"],
        }
        write_live_runtime_restart_record(context, "intent", intent)
    ensure_live_runtime_restart_journal(
        context, "intent", intent["operation_id"]
    )
    identity = require_live_runtime_restart_scope(
        context, platform, state, intent, prerequisites
    )
    if shutdown is None:
        current = runtime_job_observation(platform, identity)
        if current["pid"] == intent["old_pid"]:
            if (
                not current["loaded"]
                or current["state"] != "running"
                or current["runs"] != intent["old_runs"]
            ):
                fail("live_runtime_restart_precondition_changed")
            stable_old_job, stable_old_process_instance_id = (
                require_bound_running_runtime_identity(
                    context,
                    platform,
                    identity,
                    intent["old_pid"],
                    intent["old_runs"],
                    "live_runtime_restart_precondition_changed",
                )
            )
            if (
                stable_old_job != current
                or stable_old_process_instance_id
                != intent["old_process_instance_id"]
            ):
                fail("live_runtime_restart_precondition_changed")
            shutdown_deadline = monotonic_time() + LIVE_EXIT_TIMEOUT_SECONDS
            platform.launchd_signal(identity["label"], "SIGTERM")
        elif current["pid"] is not None or not current["loaded"]:
            fail("live_runtime_restart_shutdown_unjournaled_state")
        else:
            shutdown_deadline = monotonic_time() + LIVE_EXIT_TIMEOUT_SECONDS
        stable = wait_for_clean_runtime_exit(
            context,
            platform,
            identity,
            intent,
            shutdown_deadline,
        )
        shutdown = {
            **intent,
            "recorded_at": utc_now(),
            "shutdown_observation": stable,
            "stability_seconds": LIVE_EXIT_STABILITY_SECONDS,
        }
        write_live_runtime_restart_record(context, "shutdown", shutdown)
    ensure_live_runtime_restart_journal(
        context, "shutdown_stable", intent["operation_id"]
    )
    if awaiting is None:
        drained = command_restart_drained_runtime(context, platform)
        if (
            drained.get("status")
            not in {"drained_runtime_restarted", "exact_replay"}
            or drained.get("old_pid") is not None
            or type(drained.get("new_pid")) is not int
            or drained["new_pid"] <= 0
            or drained["new_pid"] == intent["old_pid"]
            or drained.get("runtime_sha256")
            != intent["runtime_identity"]["runtime_sha256"]
            or drained.get("transport_instance_id")
            != intent["transport_instance_id"]
            or drained.get("ready_after_restart") is not True
        ):
            fail("live_runtime_restart_drained_result_invalid")
        observed_job = runtime_job_observation(platform, identity)
        final_job, process_instance_id = require_bound_running_runtime_identity(
            context,
            platform,
            identity,
            drained["new_pid"],
            observed_job["runs"],
            "live_runtime_restart_final_observation_invalid",
        )
        require_live_runtime_restart_scope(
            context, platform, state, intent, prerequisites
        )
        awaiting = {
            **intent,
            "recorded_at": utc_now(),
            "new_pid": final_job["pid"],
            "process_instance_id": process_instance_id,
            "new_runs": final_job["runs"],
            "ready_after_restart": True,
            "drained_restart_operation_id": drained["operation_id"],
            "shutdown_boundary": shutdown["recorded_at"],
        }
        if runtime_job_observation(platform, identity) != final_job:
            fail("live_runtime_restart_final_observation_changed")
        write_live_runtime_restart_record(
            context, "awaiting-confirmation", awaiting
        )
    ensure_live_runtime_restart_journal(
        context,
        "awaiting_canonical_confirmation",
        intent["operation_id"],
    )
    final_job, process_instance_id = require_bound_running_runtime_identity(
        context,
        platform,
        identity,
        awaiting["new_pid"],
        awaiting["new_runs"],
        "live_runtime_restart_confirmation_process_drift",
    )
    if process_instance_id != awaiting["process_instance_id"]:
        fail("live_runtime_restart_confirmation_process_drift")
    require_live_runtime_restart_scope(
        context, platform, state, awaiting, prerequisites
    )
    if confirmation_path is None:
        if runtime_job_observation(platform, identity) != final_job:
            fail("live_runtime_restart_confirmation_process_drift")
        return awaiting_canonical_confirmation_result(awaiting)
    confirmation = require_bound_canonical_confirmation(
        confirmation_path, awaiting
    )
    completion = {
        **awaiting,
        "recorded_at": utc_now(),
        "canonical_confirmation": confirmation,
        "canonical_confirmation_sha256": canonical_confirmation_digest(
            confirmation
        ),
    }
    if runtime_job_observation(platform, identity) != final_job:
        fail("live_runtime_restart_final_observation_changed")
    write_live_runtime_restart_record(context, "complete", completion)
    ensure_live_runtime_restart_journal(
        context, "complete", intent["operation_id"]
    )
    evidence, evidence_path = publish_step_11_evidence(context, completion)
    return live_runtime_restart_result(
        "live_runtime_restart_certified", completion, evidence, evidence_path
    )


def command_certify_live_runtime_restart(
    context, platform, confirmation_path=None
):
    with locked_step_11_prerequisites(context) as prerequisites:
        return certify_live_runtime_restart(
            context, platform, prerequisites, confirmation_path
        )
