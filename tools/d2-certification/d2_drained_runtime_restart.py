import os
import pathlib
import plistlib
import re
import stat

from d2_certification import (
    canonical_json,
    fsync_directory,
    require_immutable_candidate,
    sha256_file,
)
from d2_orchestrator_composition import compose_plists
from d2_orchestrator_contract import (
    append_journal,
    fail,
    load_json,
    load_state,
    read_repaired_journal,
    standing_snapshot,
    utc_now,
    write_atomic,
)


TRANSPORT_INSTANCE_PATTERN = re.compile(r"^d2ti-[0-9a-f]{32}$")
PROCESS_INSTANCE_PATTERN = re.compile(r"^[0-9a-f]{32}$")
DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")
RECORDED_AT_PATTERN = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$"
)
RESTART_FILE_PATTERN = re.compile(r"^([0-9]{4})-(intent|complete)\.json$")
RESTART_OPERATION_PATTERN = re.compile(
    r"^d2:[0-9a-f]{16}:restart-drained-runtime:[0-9]{4}$"
)
RESTART_TEMP_FILE_PATTERN = re.compile(
    r"^(?:[0-9]{4}-(?:intent|complete)\.pending|"
    r"\.[0-9]{4}-(?:intent|complete)\.pending\.[1-9][0-9]*\.tmp)$"
)
RESTART_DEPENDENCIES = ("postgres", "api", "worker", "transport", "tunnel")


def service_plist_path(context, name):
    label = context.manifest["services"][name]["label"]
    path = context.plist_directory / f"{label}.plist"
    try:
        metadata = path.lstat()
    except OSError:
        fail("candidate_plist_absent")
    if not stat.S_ISREG(metadata.st_mode) or path.is_symlink():
        fail("candidate_plist_invalid")
    return path


def pinned_transport_instance_id(context):
    path = context.artifact_directory / "step-03-evidence.json"
    try:
        metadata = path.lstat()
    except OSError:
        fail("transport_instance_evidence_absent")
    if (
        not stat.S_ISREG(metadata.st_mode)
        or path.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o600
    ):
        fail("transport_instance_evidence_invalid")
    evidence = load_json(path, "transport_instance_evidence_invalid")
    instance_id = evidence.get("transport_instance_id") if isinstance(evidence, dict) else None
    if not isinstance(instance_id, str) or not TRANSPORT_INSTANCE_PATTERN.fullmatch(
        instance_id
    ):
        fail("transport_instance_evidence_invalid")
    return instance_id


def require_pinned_transport_snapshot(context, snapshot):
    if snapshot["instance_id"] != pinned_transport_instance_id(context):
        fail("transport_instance_changed")


def drained_runtime_restart_directory(context):
    return context.artifact_directory / "drained-runtime-restarts"


def drained_runtime_restart_temporary_directory(context):
    return context.artifact_directory / "drained-runtime-restart-temporary"


def ensure_drained_runtime_restart_directory(context, path, code):
    if not path.exists():
        try:
            path.mkdir(mode=0o700)
        except OSError:
            fail(code)
        fsync_directory(context.artifact_directory, f"{code}_parent")
    metadata = path.lstat()
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or path.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        fail(code)


def recover_drained_runtime_restart_temporary(context):
    directory = drained_runtime_restart_temporary_directory(context)
    if not directory.exists():
        return
    ensure_drained_runtime_restart_directory(
        context, directory, "drained_runtime_restart_temporary_directory_invalid"
    )
    try:
        entries = list(directory.iterdir())
    except OSError:
        fail("drained_runtime_restart_temporary_inventory_invalid")
    for entry in entries:
        try:
            metadata = entry.lstat()
        except OSError:
            fail("drained_runtime_restart_temporary_inventory_invalid")
        if (
            not RESTART_TEMP_FILE_PATTERN.fullmatch(entry.name)
            or not stat.S_ISREG(metadata.st_mode)
            or entry.is_symlink()
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
        ):
            fail("drained_runtime_restart_temporary_inventory_invalid")
        try:
            entry.unlink()
        except OSError:
            fail("drained_runtime_restart_temporary_cleanup_failed")
    if entries:
        fsync_directory(directory, "drained_runtime_restart_temporary_cleanup")


def write_drained_runtime_restart_record(context, sequence, kind, record):
    evidence_directory = drained_runtime_restart_directory(context)
    temporary_directory = drained_runtime_restart_temporary_directory(context)
    ensure_drained_runtime_restart_directory(
        context, evidence_directory, "drained_runtime_restart_evidence_directory_invalid"
    )
    ensure_drained_runtime_restart_directory(
        context, temporary_directory, "drained_runtime_restart_temporary_directory_invalid"
    )
    recover_drained_runtime_restart_temporary(context)
    if evidence_directory.stat().st_dev != temporary_directory.stat().st_dev:
        fail("drained_runtime_restart_temporary_filesystem_changed")
    destination = evidence_directory / f"{sequence:04d}-{kind}.json"
    if os.path.lexists(destination):
        fail("drained_runtime_restart_evidence_busy")
    pending = temporary_directory / f"{sequence:04d}-{kind}.pending"
    write_atomic(pending, canonical_json(record) + "\n")
    try:
        os.replace(pending, destination)
    except OSError:
        fail("drained_runtime_restart_evidence_publish_failed")
    fsync_directory(evidence_directory, "drained_runtime_restart_evidence_publish")
    fsync_directory(temporary_directory, "drained_runtime_restart_temporary_publish")


def launchd_service_identity(context, name):
    plist_path = service_plist_path(context, name)
    metadata = plist_path.lstat()
    if metadata.st_uid != os.getuid() or stat.S_IMODE(metadata.st_mode) != 0o600:
        fail("candidate_plist_invalid")
    try:
        actual_plist = plistlib.loads(plist_path.read_bytes())
    except (OSError, plistlib.InvalidFileException):
        fail("candidate_plist_invalid")
    expected_plist = compose_plists(context)[name]
    if actual_plist != expected_plist:
        fail("candidate_plist_changed")
    return {
        "label": context.manifest["services"][name]["label"],
        "plist_path": str(plist_path),
        "plist_sha256": sha256_file(plist_path),
        "program": expected_plist["ProgramArguments"][0],
        "program_arguments": expected_plist["ProgramArguments"],
    }


def drained_runtime_restart_identity(context):
    candidate = context.manifest["candidates"]["runtime"]
    candidate_path = pathlib.Path(candidate["path"])
    require_immutable_candidate(candidate_path, "runtime")
    candidate_sha256 = sha256_file(candidate_path)
    if candidate_sha256 != candidate["sha256"]:
        fail("runtime_candidate_changed")
    service_identity = launchd_service_identity(context, "runtime")
    if service_identity["program"] != str(candidate_path):
        fail("runtime_plist_changed")
    return {
        "label": service_identity["label"],
        "plist_path": service_identity["plist_path"],
        "plist_sha256": service_identity["plist_sha256"],
        "candidate_path": str(candidate_path),
        "runtime_sha256": candidate_sha256,
        "program_arguments": service_identity["program_arguments"],
    }


def runtime_job_observation(platform, identity):
    job = platform.launchd_job(identity["label"])
    if job is None:
        return {
            "loaded": False,
            "pid": None,
            "state": None,
            "last_exit_code": None,
            "plist_path": None,
            "program_arguments": None,
            "runs": None,
        }
    if (
        not isinstance(job, dict)
        or set(job)
        != {
            "pid",
            "program",
            "plist_path",
            "arguments",
            "runs",
            "state",
            "last_exit_code",
        }
        or job["program"] != identity["candidate_path"]
        or job["plist_path"] != identity["plist_path"]
        or job["arguments"] != identity["program_arguments"]
        or type(job["runs"]) is not int
        or job["runs"] <= 0
        or not isinstance(job["state"], str)
        or not job["state"]
        or job["last_exit_code"] is not None
        and type(job["last_exit_code"]) is not int
        or job["pid"] is not None
        and (type(job["pid"]) is not int or job["pid"] <= 0)
        or job["pid"] is not None
        and job["state"] != "running"
    ):
        fail("runtime_job_identity_changed")
    return {
        "loaded": True,
        "pid": job["pid"],
        "state": job["state"],
        "last_exit_code": job["last_exit_code"],
        "plist_path": job["plist_path"],
        "program_arguments": job["arguments"],
        "runs": job["runs"],
    }


def runtime_ready_status(context, platform):
    return platform.http_status(
        "http://127.0.0.1:"
        f"{context.manifest['services']['runtime']['port']}/health/ready"
    )


def valid_runtime_process_identity(context, value):
    return (
        isinstance(value, dict)
        and set(value)
        == {
            "pid",
            "start_time_seconds",
            "start_time_microseconds",
            "uid",
            "path",
            "sha256",
            "size",
            "mode",
            "device",
            "inode",
            "links",
        }
        and type(value["pid"]) is int
        and value["pid"] > 0
        and type(value["start_time_seconds"]) is int
        and value["start_time_seconds"] > 0
        and type(value["start_time_microseconds"]) is int
        and 0 <= value["start_time_microseconds"] < 1_000_000
        and type(value["uid"]) is int
        and value["uid"] == os.getuid()
        and value["path"] == context.manifest["candidates"]["runtime"]["path"]
        and value["sha256"]
        == context.manifest["candidates"]["runtime"]["sha256"]
        and isinstance(value["sha256"], str)
        and DIGEST_PATTERN.fullmatch(value["sha256"])
        and type(value["size"]) is int
        and value["size"] > 0
        and type(value["mode"]) is int
        and value["mode"] > 0
        and value["mode"] & 0o111 != 0
        and value["mode"] & 0o222 == 0
        and type(value["device"]) is int
        and value["device"] >= 0
        and type(value["inode"]) is int
        and value["inode"] > 0
        and value["links"] == 1
    )


def valid_runtime_health_identity(value, expected_pid=None):
    return (
        isinstance(value, dict)
        and set(value)
        == {"schema_version", "os_pid", "process_instance_id"}
        and type(value["schema_version"]) is int
        and value["schema_version"] == 1
        and type(value["os_pid"]) is int
        and value["os_pid"] > 0
        and (expected_pid is None or value["os_pid"] == expected_pid)
        and isinstance(value["process_instance_id"], str)
        and PROCESS_INSTANCE_PATTERN.fullmatch(value["process_instance_id"])
    )


def require_bound_runtime_generation(
    context,
    platform,
    identity,
    expected_pid,
    expected_runs,
    code,
    expected_ready_status=200,
):
    if type(expected_ready_status) is int:
        allowed_ready_statuses = (expected_ready_status,)
    elif (
        isinstance(expected_ready_status, tuple)
        and expected_ready_status
        and all(type(status) is int for status in expected_ready_status)
    ):
        allowed_ready_statuses = expected_ready_status
    else:
        fail("runtime_ready_status_contract_invalid")
    before = runtime_job_observation(platform, identity)
    ready = runtime_ready_status(context, platform)
    process = platform.candidate_process_identity(
        expected_pid, pathlib.Path(identity["candidate_path"])
    )
    health = platform.runtime_process_identity(context)
    after = runtime_job_observation(platform, identity)
    if (
        before != after
        or before["pid"] != expected_pid
        or before["runs"] != expected_runs
        or before["state"] != "running"
        or ready not in allowed_ready_statuses
        or not valid_runtime_process_identity(context, process)
        or process["pid"] != expected_pid
        or not valid_runtime_health_identity(health, expected_pid)
    ):
        fail(code)
    return {
        "process_identity": process,
        "runtime_health": health,
    }


def require_drained_runtime(context, platform, identity, allow_absent=False):
    observation = runtime_job_observation(platform, identity)
    if observation["pid"] is not None or runtime_ready_status(context, platform) == 200:
        fail("runtime_drain_incomplete")
    if not observation["loaded"]:
        if allow_absent:
            return observation
        fail("runtime_drain_evidence_absent")
    if observation["loaded"] and (
        observation["state"] != "exited" or observation["last_exit_code"] != 0
    ):
        fail("runtime_drain_unsuccessful")
    return observation


def require_dependency_health(context, platform):
    manifest = context.manifest
    statuses = {
        "worker": platform.worker_health_status(context),
        "transport": platform.transport_health_status(context),
        "api": platform.http_status(
            f"http://127.0.0.1:{manifest['services']['api']['port']}/health/ready",
            host_header=manifest["public_origin"].removeprefix("https://"),
        ),
        "tunnel": platform.http_status(f"{manifest['public_origin']}/health/live"),
    }
    if any(status != 200 for status in statuses.values()):
        fail("candidate_health_unready")


def drained_runtime_restart_dependency_snapshot(context, platform):
    if not platform.postgres_running(context.cluster_root):
        fail("candidate_state_drift")
    postgres_pid = platform.postgres_pid(context.cluster_root)
    if type(postgres_pid) is not int or postgres_pid <= 0:
        fail("candidate_state_drift")
    snapshot = {
        "postgres": {
            "pid": postgres_pid,
            "cluster_root": str(context.cluster_root),
            "port": context.manifest["database"]["port"],
        }
    }
    for name in RESTART_DEPENDENCIES[1:]:
        identity = launchd_service_identity(context, name)
        job = platform.launchd_job(identity["label"])
        if (
            not isinstance(job, dict)
            or set(job)
            != {
                "pid",
                "program",
                "plist_path",
                "arguments",
                "runs",
                "state",
                "last_exit_code",
            }
            or type(job["pid"]) is not int
            or job["pid"] <= 0
            or job["program"] != identity["program"]
            or job["plist_path"] != identity["plist_path"]
            or job["arguments"] != identity["program_arguments"]
            or type(job["runs"]) is not int
            or job["runs"] <= 0
            or job["state"] != "running"
            or job["last_exit_code"] is not None
            and type(job["last_exit_code"]) is not int
        ):
            fail("candidate_state_drift")
        snapshot[name] = {"pid": job["pid"], "runs": job["runs"], **identity}
    require_dependency_health(context, platform)
    return snapshot


def valid_drained_runtime_restart_identity(context, value):
    expected = {
        "label": context.manifest["services"]["runtime"]["label"],
        "plist_path": str(service_plist_path(context, "runtime")),
        "candidate_path": context.manifest["candidates"]["runtime"]["path"],
    }
    return (
        isinstance(value, dict)
        and set(value)
        == {
            "label",
            "plist_path",
            "plist_sha256",
            "candidate_path",
            "runtime_sha256",
            "program_arguments",
        }
        and all(value[field] == expected[field] for field in expected)
        and isinstance(value["plist_sha256"], str)
        and re.fullmatch(r"[0-9a-f]{64}", value["plist_sha256"])
        and value["runtime_sha256"]
        == context.manifest["candidates"]["runtime"]["sha256"]
        and value["program_arguments"]
        == compose_plists(context)["runtime"]["ProgramArguments"]
    )


def valid_drained_runtime_restart_dependencies(context, value):
    if not isinstance(value, dict) or set(value) != set(RESTART_DEPENDENCIES):
        return False
    postgres = value["postgres"]
    if (
        not isinstance(postgres, dict)
        or set(postgres) != {"pid", "cluster_root", "port"}
        or type(postgres["pid"]) is not int
        or postgres["pid"] <= 0
        or postgres["cluster_root"] != str(context.cluster_root)
        or postgres["port"] != context.manifest["database"]["port"]
    ):
        return False
    expected_plists = compose_plists(context)
    for name in RESTART_DEPENDENCIES[1:]:
        dependency = value[name]
        expected = {
            "label": context.manifest["services"][name]["label"],
            "plist_path": str(service_plist_path(context, name)),
            "program": expected_plists[name]["ProgramArguments"][0],
            "program_arguments": expected_plists[name]["ProgramArguments"],
        }
        if (
            not isinstance(dependency, dict)
            or set(dependency)
            != {
                "pid",
                "runs",
                "label",
                "plist_path",
                "plist_sha256",
                "program",
                "program_arguments",
            }
            or type(dependency["pid"]) is not int
            or dependency["pid"] <= 0
            or type(dependency["runs"]) is not int
            or dependency["runs"] <= 0
            or any(dependency[field] != expected[field] for field in expected)
            or not isinstance(dependency["plist_sha256"], str)
            or not re.fullmatch(r"[0-9a-f]{64}", dependency["plist_sha256"])
        ):
            return False
    return True


def valid_runtime_drain_observation(value):
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
        and isinstance(value["plist_path"], str)
        and isinstance(value["program_arguments"], list)
        and type(value["runs"]) is int
        and value["runs"] > 0
    )


def validate_drained_runtime_restart_record(context, record, sequence, kind):
    common = {
        "schema_version",
        "manifest_sha256",
        "recorded_at",
        "sequence",
        "operation_id",
        "runtime_identity",
        "old_pid",
        "drain_observation",
        "transport_instance_id",
        "dependencies",
        "standing_snapshot",
    }
    expected = (
        common
        if kind == "intent"
        else common
        | {
            "new_pid",
            "new_runs",
            "new_process_identity",
            "new_runtime_health",
            "ready_after_restart",
        }
    )
    if (
        not isinstance(record, dict)
        or set(record) != expected
        or record["schema_version"] != 1
        or record["manifest_sha256"] != context.digest
        or record["sequence"] != sequence
        or not isinstance(record["recorded_at"], str)
        or not RECORDED_AT_PATTERN.fullmatch(record["recorded_at"])
        or not isinstance(record["operation_id"], str)
        or not RESTART_OPERATION_PATTERN.fullmatch(record["operation_id"])
        or record["operation_id"]
        != f"d2:{context.digest[:16]}:restart-drained-runtime:{sequence:04d}"
        or not valid_drained_runtime_restart_identity(context, record["runtime_identity"])
        or record["old_pid"] is not None
        or not valid_runtime_drain_observation(record["drain_observation"])
        or record["drain_observation"]["plist_path"]
        != record["runtime_identity"]["plist_path"]
        or record["drain_observation"]["program_arguments"]
        != record["runtime_identity"]["program_arguments"]
        or not isinstance(record["transport_instance_id"], str)
        or not TRANSPORT_INSTANCE_PATTERN.fullmatch(record["transport_instance_id"])
        or not valid_drained_runtime_restart_dependencies(context, record["dependencies"])
        or not isinstance(record["standing_snapshot"], dict)
    ):
        fail("drained_runtime_restart_evidence_invalid")
    if kind == "complete" and (
        type(record["new_pid"]) is not int
        or record["new_pid"] <= 0
        or type(record["new_runs"]) is not int
        or record["new_runs"] <= 0
        or not valid_runtime_process_identity(
            context, record["new_process_identity"]
        )
        or record["new_process_identity"]["pid"] != record["new_pid"]
        or not valid_runtime_health_identity(
            record["new_runtime_health"], record["new_pid"]
        )
        or record["ready_after_restart"] is not True
    ):
        fail("drained_runtime_restart_evidence_invalid")


def drained_runtime_restart_inventory(context):
    directory = drained_runtime_restart_directory(context)
    if not directory.exists():
        return [], None
    ensure_drained_runtime_restart_directory(
        context, directory, "drained_runtime_restart_evidence_directory_invalid"
    )
    records = {}
    try:
        entries = list(directory.iterdir())
    except OSError:
        fail("drained_runtime_restart_evidence_inventory_invalid")
    for entry in entries:
        match = RESTART_FILE_PATTERN.fullmatch(entry.name)
        try:
            metadata = entry.lstat()
        except OSError:
            fail("drained_runtime_restart_evidence_inventory_invalid")
        if (
            match is None
            or not stat.S_ISREG(metadata.st_mode)
            or entry.is_symlink()
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
        ):
            fail("drained_runtime_restart_evidence_inventory_invalid")
        sequence = int(match.group(1))
        kind = match.group(2)
        if sequence == 0:
            fail("drained_runtime_restart_evidence_inventory_invalid")
        record = records.setdefault(sequence, {})
        if kind in record:
            fail("drained_runtime_restart_evidence_inventory_invalid")
        value = load_json(entry, "drained_runtime_restart_evidence_invalid")
        validate_drained_runtime_restart_record(context, value, sequence, kind)
        record[kind] = value
    ordered = []
    for expected_sequence, sequence in enumerate(sorted(records), 1):
        if sequence != expected_sequence:
            fail("drained_runtime_restart_evidence_inventory_invalid")
        record = records[sequence]
        if "intent" not in record:
            fail("drained_runtime_restart_evidence_inventory_invalid")
        complete = record.get("complete")
        if complete is not None:
            for field in (
                "schema_version",
                "manifest_sha256",
                "sequence",
                "operation_id",
                "runtime_identity",
                "old_pid",
                "drain_observation",
                "transport_instance_id",
                "dependencies",
                "standing_snapshot",
            ):
                if complete[field] != record["intent"][field]:
                    fail("drained_runtime_restart_evidence_invalid")
        ordered.append({"sequence": sequence, **record})
    pending = [record for record in ordered if "complete" not in record]
    if len(pending) > 1 or pending and pending[0] is not ordered[-1]:
        fail("drained_runtime_restart_evidence_inventory_invalid")
    return ordered, pending[0] if pending else None


def drained_runtime_restart_journal_contains(context, status, operation_id):
    found = False
    for receipt in read_repaired_journal(context):
        if (
            receipt["action"] == "drained_runtime_restart"
            and receipt["status"] == status
            and receipt["target"] == operation_id.replace(":", "_")
        ):
            if found:
                fail("journal_invalid")
            found = True
    return found


def ensure_drained_runtime_restart_journal(context, status, operation_id):
    if not drained_runtime_restart_journal_contains(context, status, operation_id):
        append_journal(
            context, "drained_runtime_restart", status, operation_id.replace(":", "_")
        )


def drained_runtime_restart_result(status, intent, new_pid):
    return {
        "status": status,
        "phase": "candidate_started",
        "operation_id": intent["operation_id"],
        "old_pid": intent["old_pid"],
        "new_pid": new_pid,
        "runtime_sha256": intent["runtime_identity"]["runtime_sha256"],
        "transport_instance_id": intent["transport_instance_id"],
        "ready_after_restart": True,
    }


def require_drained_runtime_restart_final_observation(
    context, platform, state, intent, expected_pid
):
    identity = drained_runtime_restart_identity(context)
    dependencies = drained_runtime_restart_dependency_snapshot(context, platform)
    transport_snapshot = platform.transport_control(context, "snapshot")
    require_pinned_transport_snapshot(context, transport_snapshot)
    if (
        identity != intent["runtime_identity"]
        or dependencies != intent["dependencies"]
        or transport_snapshot["instance_id"] != intent["transport_instance_id"]
        or standing_snapshot(context, platform) != state["standing_snapshot"]
    ):
        fail("drained_runtime_restart_postcondition_changed")
    before_ready = runtime_job_observation(platform, identity)
    ready = runtime_ready_status(context, platform)
    after_ready = runtime_job_observation(platform, identity)
    if (
        before_ready["pid"] != expected_pid
        or before_ready["state"] != "running"
        or ready != 200
        or after_ready != before_ready
    ):
        fail("drained_runtime_restart_final_observation_changed")
    return after_ready


def complete_drained_runtime_restart(context, platform, state, intent, current_job):
    identity = drained_runtime_restart_identity(context)
    if identity != intent["runtime_identity"]:
        fail("drained_runtime_restart_identity_changed")
    dependencies = drained_runtime_restart_dependency_snapshot(context, platform)
    if dependencies != intent["dependencies"]:
        fail("drained_runtime_restart_dependency_changed")
    transport_snapshot = platform.transport_control(context, "snapshot")
    require_pinned_transport_snapshot(context, transport_snapshot)
    if transport_snapshot["instance_id"] != intent["transport_instance_id"]:
        fail("transport_instance_changed")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    status = platform.wait_for_status(
        lambda: runtime_ready_status(context, platform),
        200,
    )
    if status != 200:
        fail("drained_runtime_restart_health_unready")
    expected_pid = current_job["pid"] if current_job is not None else None
    if type(expected_pid) is not int or expected_pid <= 0:
        fail("drained_runtime_restart_pid_unavailable")
    final_job = require_drained_runtime_restart_final_observation(
        context, platform, state, intent, expected_pid
    )
    new_pid = final_job["pid"]
    generation = require_bound_runtime_generation(
        context,
        platform,
        identity,
        new_pid,
        final_job["runs"],
        "drained_runtime_restart_process_identity_changed",
    )
    completion = {
        **intent,
        "recorded_at": utc_now(),
        "new_pid": new_pid,
        "new_runs": final_job["runs"],
        "new_process_identity": generation["process_identity"],
        "new_runtime_health": generation["runtime_health"],
        "ready_after_restart": True,
    }
    write_drained_runtime_restart_record(
        context,
        intent["sequence"],
        "complete",
        completion,
    )
    ensure_drained_runtime_restart_journal(context, "complete", intent["operation_id"])
    return drained_runtime_restart_result("drained_runtime_restarted", intent, new_pid)


def command_restart_drained_runtime(context, platform, expected_initial_runs):
    if type(expected_initial_runs) is not int or expected_initial_runs <= 0:
        fail("drained_runtime_restart_generation_invalid")
    state = load_state(context, {"candidate_started"})
    recover_drained_runtime_restart_temporary(context)
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    identity = drained_runtime_restart_identity(context)
    dependencies = drained_runtime_restart_dependency_snapshot(context, platform)
    transport_snapshot = platform.transport_control(context, "snapshot")
    require_pinned_transport_snapshot(context, transport_snapshot)
    current_job = runtime_job_observation(platform, identity)
    records, pending = drained_runtime_restart_inventory(context)
    if pending is None and records:
        latest = records[-1]["complete"]
        if current_job["pid"] == latest["new_pid"]:
            if (
                current_job["runs"] != latest["new_runs"]
                or
                identity != latest["runtime_identity"]
                or dependencies != latest["dependencies"]
                or transport_snapshot["instance_id"]
                != latest["transport_instance_id"]
                or state["standing_snapshot"] != latest["standing_snapshot"]
            ):
                fail("drained_runtime_restart_replay_drift")
            ready = platform.wait_for_status(
                lambda: runtime_ready_status(context, platform),
                200,
            )
            if ready != 200:
                fail("drained_runtime_restart_health_unready")
            require_drained_runtime_restart_final_observation(
                context, platform, state, latest, latest["new_pid"]
            )
            generation = require_bound_runtime_generation(
                context,
                platform,
                identity,
                latest["new_pid"],
                latest["new_runs"],
                "drained_runtime_restart_replay_drift",
            )
            if (
                generation["process_identity"]
                != latest["new_process_identity"]
                or generation["runtime_health"]
                != latest["new_runtime_health"]
            ):
                fail("drained_runtime_restart_replay_drift")
            ensure_drained_runtime_restart_journal(
                context, "complete", latest["operation_id"]
            )
            return drained_runtime_restart_result("exact_replay", latest, latest["new_pid"])
        if current_job["pid"] is not None:
            fail("drained_runtime_restart_unjournaled_pid")
        fail("drained_runtime_restart_sequence_exhausted")
    if pending is None:
        drain_observation = require_drained_runtime(context, platform, identity)
        expected_runs = (
            records[-1]["complete"]["new_runs"]
            if records
            else expected_initial_runs
        )
        if drain_observation["runs"] != expected_runs:
            fail("drained_runtime_restart_generation_unjournaled")
        sequence = len(records) + 1
        if sequence > 9999:
            fail("drained_runtime_restart_evidence_capacity_exhausted")
        operation_id = f"d2:{context.digest[:16]}:restart-drained-runtime:{sequence:04d}"
        intent = {
            "schema_version": 1,
            "manifest_sha256": context.digest,
            "recorded_at": utc_now(),
            "sequence": sequence,
            "operation_id": operation_id,
            "runtime_identity": identity,
            "old_pid": None,
            "drain_observation": drain_observation,
            "transport_instance_id": transport_snapshot["instance_id"],
            "dependencies": dependencies,
            "standing_snapshot": state["standing_snapshot"],
        }
        write_drained_runtime_restart_record(context, sequence, "intent", intent)
        ensure_drained_runtime_restart_journal(context, "intent", operation_id)
    else:
        intent = pending["intent"]
        if (
            identity != intent["runtime_identity"]
            or dependencies != intent["dependencies"]
            or transport_snapshot["instance_id"] != intent["transport_instance_id"]
            or state["standing_snapshot"] != intent["standing_snapshot"]
        ):
            fail("drained_runtime_restart_recovery_drift")
        ensure_drained_runtime_restart_journal(context, "intent", intent["operation_id"])
        if current_job["pid"] is not None and current_job["pid"] != intent["old_pid"]:
            return complete_drained_runtime_restart(
                context, platform, state, intent, current_job
            )
        require_drained_runtime(context, platform, identity, allow_absent=True)
    pre_bootout = require_drained_runtime(
        context, platform, identity, allow_absent=pending is not None
    )
    if pre_bootout != intent["drain_observation"] and not (
        pending is not None
        and intent["drain_observation"]["loaded"] is True
        and pre_bootout["loaded"] is False
    ):
        fail("runtime_drain_observation_changed")
    if drained_runtime_restart_identity(context) != intent["runtime_identity"]:
        fail("drained_runtime_restart_identity_changed")
    if pre_bootout["loaded"]:
        platform.launchd_bootout(identity["label"])
    if (
        runtime_job_observation(platform, identity)["loaded"]
        or runtime_ready_status(context, platform) == 200
    ):
        fail("drained_runtime_restart_bootout_unconfirmed")
    if drained_runtime_restart_dependency_snapshot(context, platform) != intent["dependencies"]:
        fail("drained_runtime_restart_dependency_changed")
    platform.launchd_start(identity["label"], pathlib.Path(identity["plist_path"]))
    started_job = runtime_job_observation(platform, identity)
    if not started_job["loaded"] or started_job["pid"] is None:
        fail("drained_runtime_restart_start_unconfirmed")
    return complete_drained_runtime_restart(context, platform, state, intent, started_job)
