#!/usr/bin/env python3

import argparse
import hashlib
import os
import pathlib
import plistlib
import re
import signal
import stat
import sys
import unicodedata

from d2_certification import (
    CertificationError,
    STEP_SPECS,
    canonical_json,
    isolated_runtime_root,
    load_json_file,
    require_absolute_path,
    require_owned_mode,
    validate_snowflake,
    validate_step_contract,
    validate_utc_timestamp,
)
from d2_orchestrator_composition import (
    compose_plists,
    configure_postgres,
    configure_postgres_bootstrap_network,
    configure_postgres_sealed_network,
    write_keychain_plan,
    write_plists,
)
from d2_orchestrator_contract import (
    GLOBAL_LOCK_PATH,
    OWNER_ACCOUNT,
    PROTECTED_PORTS,
    STANDING_DISCORD_IDENTITIES,
    STANDING_PUBLIC_ORIGIN,
    OrchestratorError,
    append_journal,
    claim_discord_ownership,
    external_keychain_inventory,
    fail,
    global_operation_lock,
    keychain_inventory,
    load_json,
    load_context,
    load_state,
    owner_identities,
    release_discord_ownership,
    require_discord_ownership_available,
    require_discord_ownership_claimed,
    require_discord_ownership_released,
    save_state,
    standing_snapshot,
    utc_now,
    validate_candidate_programs,
    validate_dedicated_discord_identity,
    validate_ports,
    validate_programs,
    write_atomic,
)
from d2_orchestrator_platform import Platform, rename_exclusive
from d2_drained_runtime_restart import (
    command_restart_drained_runtime as run_restart_drained_runtime,
    drained_runtime_restart_directory,
    drained_runtime_restart_identity,
    drained_runtime_restart_inventory,
    drained_runtime_restart_temporary_directory,
    require_bound_runtime_generation,
)
from d2_finalization import (
    abort_teardown_evidence_path,
    abort_teardown_progress_path,
    abort_teardown_tombstone_path,
    certified_teardown_binding,
    command_finalize_run as run_finalize_run,
    command_finalize_total_absence as run_finalize_total_absence,
    freeze_intent_path,
    require_certified_teardown_snapshot,
    require_certification_eligible_teardown,
    validate_runtime_freeze_binding,
)
from d2_live_runtime_restart import (
    committed_live_runtime_restart_chain,
    command_certify_live_runtime_restart as run_certify_live_runtime_restart,
    live_runtime_restart_complete_path,
    live_runtime_restart_directory,
    live_runtime_restart_intent_path,
)
from d2_legacy_substrate_recovery import (
    command_recover as command_recover_legacy_substrate,
    command_status as command_legacy_substrate_status,
    load_legacy_context,
    load_lifecycle_journal,
)
from d2_source_contract import (
    CANDIDATE_KIND,
    publish_bootstrap_source,
    publish_candidate_source,
    publish_onboarding_source,
    read_private_source,
    source_path,
)
from d2_worker_evidence import capture_worker_authoring_checkpoint


SERVICE_START_ORDER = ("transport", "worker", "api", "runtime", "tunnel")
SERVICE_STOP_ORDER = tuple(reversed(SERVICE_START_ORDER))
TRANSPORT_INSTANCE_PATTERN = re.compile(r"^d2ti-[0-9a-f]{32}$")
DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")
DISCORD_RESOURCE_KIND_ORDER = {"message": 0, "channel": 1, "role": 2}
DISCORD_RESOURCE_UNKNOWN_CODES = {
    "role": {10011},
    "channel": {10003},
    "message": {10003, 10008},
}
DISCORD_RESOURCE_SUCCESS_STATUS = {"role": 204, "channel": 200, "message": 204}
DISCORD_TEARDOWN_PROGRESS_KIND = "starring.d2.discord-resource-teardown-progress.v1"
DISCORD_TEARDOWN_EVIDENCE_KIND = "starring.d2.discord-resource-teardown.v1"
DISCORD_TEARDOWN_ABORT_KIND = "starring.d2.discord-resource-teardown-abort.v1"
CLEANUP_ROOT_PROGRESS_KIND = "starring.d2.cleanup-root-progress.v1"
CLEANUP_ROOT_IDENTITY_KIND = "starring.d2.cleanup-root-identity.v1"
CLEANUP_KEYCHAIN_BASELINE_KIND = "starring.d2.cleanup-keychain-baseline.v1"
CANDIDATE_START_TRANSITION_KIND = "starring.d2.candidate-start-transition.v1"
CANDIDATE_START_RETIREMENT_KIND = "starring.d2.candidate-start-retirement.v1"
CANDIDATE_START_RETIREMENT_REASONS = {
    "state_drift",
    "transition_invalid",
    "candidate_service_drift",
    "candidate_health_drift",
    "protected_staging_drift",
    "candidate_identity_drift",
    "candidate_source_drift",
    "explicit_stop",
    "explicit_cleanup",
}
RECONCILIATION_DISCORD_OBSERVATION_KIND = (
    "starring.d2.discord-reconciliation-role-observation.v1"
)
TRANSPORT_EVIDENCE_KINDS = {
    "interaction": "starring.d2.transport-resource-evidence.v1",
    "duplicate": "starring.d2.transport-duplicate-evidence.v1",
    "reconciliation": "starring.d2.transport-indeterminate-evidence.v1",
    "gateway-loss": "starring.d2.transport-gateway-loss-evidence.v1",
    "gateway-healed": "starring.d2.transport-gateway-healed-evidence.v1",
}


def command_dry_run(context, platform):
    validate_programs(platform)
    validate_candidate_programs(context, platform)
    validate_dedicated_discord_identity(context)
    require_discord_ownership_available(context)
    validate_ports(context, platform, require_available=True)
    if context.root.exists():
        fail("isolated_root_busy")
    for service in context.manifest["services"].values():
        if platform.launchd_loaded(service["label"]):
            fail("isolated_launchd_label_busy")
    for service, account in keychain_inventory(context):
        if platform.keychain_present(service, account):
            fail("isolated_keychain_identity_busy")
    for service, account in external_keychain_inventory(context):
        if not platform.keychain_present(service, account):
            fail("external_keychain_identity_absent")
    snapshot = standing_snapshot(context, platform)
    return {
        "status": "ready",
        "manifest_sha256": context.digest,
        "standing_snapshot": snapshot,
        "standing_mutation_allowed": False,
    }


def command_prepare(context, platform):
    if context.state_path.exists():
        state = load_state(context)
        if state["phase"] in {"prepared", "substrate_started", "stopped"}:
            require_discord_ownership_claimed(context)
            root_identity = load_cleanup_root_identity(context)
            root_metadata = cleanup_path_metadata(
                context.root, "cleanup_root_invalid"
            )
            if (
                root_identity is None
                or not cleanup_root_identity_matches(
                    root_metadata, root_identity
                )
                or not (context.cluster_root / "PG_VERSION").is_file()
                or any(
                    not platform.keychain_owner_matches(
                        service, context.manifest["run_id"]
                    )
                    for service, _account in owner_identities(context)
                )
                or (
                    state["phase"] == "substrate_started"
                    and not platform.postgres_running(context.cluster_root)
                )
            ):
                fail("prepared_state_drift")
            return {"status": "already_prepared", "phase": state["phase"]}
        if state["phase"] == "preparing":
            fail("orchestrator_recovery_required")
        fail("orchestrator_already_cleaned")
    preflight = command_dry_run(context, platform)
    context.artifact_directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    save_state(context, "preparing", preflight["standing_snapshot"])
    try:
        claim_discord_ownership(context)
        append_journal(context, "discord_ownership", "complete", "identity")
        append_journal(context, "prepare", "intent", "run")
        append_journal(context, "root_create", "intent", "isolated_root")
        context.root.mkdir(mode=0o700)
        record_cleanup_root_identity(context)
        context.socket_directory.mkdir(mode=0o700)
        context.log_directory.mkdir(mode=0o700)
        append_journal(context, "root_create", "complete", "isolated_root")
        append_journal(context, "initdb", "intent", "cluster")
        platform.initdb(context.cluster_root)
        if not (context.cluster_root / "PG_VERSION").is_file():
            fail("postgres_cluster_incomplete")
        append_journal(context, "initdb", "complete", "cluster")
        configure_postgres(context)
        write_plists(context, platform)
        write_keychain_plan(context)
        for service, account in owner_identities(context):
            append_journal(context, "keychain_owner_create", "intent", service)
            platform.keychain_write_new(
                service, account, context.manifest["run_id"].encode("ascii")
            )
            append_journal(context, "keychain_owner_create", "complete", service)
        state = save_state(context, "prepared", preflight["standing_snapshot"])
        append_journal(context, "prepare", "complete", "run")
        return {"status": "prepared", "phase": state["phase"]}
    except BaseException:
        try:
            cleanup(context, platform, preflight["standing_snapshot"], from_failure=True)
        except BaseException:
            append_journal(context, "prepare_cleanup", "failed", "run")
        raise


def managed_keychain_inventory(context):
    return tuple(
        (service, account)
        for service, account in keychain_inventory(context)
        if account != OWNER_ACCOUNT
    )


def managed_keychain_presence(context, platform):
    inventory = managed_keychain_inventory(context)
    present = sum(
        1 for service, account in inventory if platform.keychain_present(service, account)
    )
    return present, len(inventory)


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


def candidate_health(context, platform, wait):
    manifest = context.manifest
    def observe(probe):
        return platform.wait_for_status(probe, 200) if wait else probe()

    worker_status = observe(lambda: platform.worker_health_status(context))
    transport_status = observe(lambda: platform.transport_health_status(context))
    api_status = observe(
        lambda: platform.http_status(
            f"http://127.0.0.1:{manifest['services']['api']['port']}/health/ready",
            host_header=manifest["public_origin"].removeprefix("https://"),
        )
    )
    runtime_status = observe(
        lambda: platform.http_status(
            f"http://127.0.0.1:{manifest['services']['runtime']['port']}/health/ready"
        )
    )
    tunnel_status = observe(
        lambda: platform.http_status(f"{manifest['public_origin']}/health/live")
    )
    return {
        "worker": worker_status,
        "transport": transport_status,
        "api": api_status,
        "runtime": runtime_status,
        "tunnel": tunnel_status,
    }


def require_started_dependency(context, platform, name):
    if name == "transport":
        status = platform.wait_for_status(
            lambda: platform.transport_health_status(context), 200
        )
    elif name == "worker":
        status = platform.wait_for_status(
            lambda: platform.worker_health_status(context), 200
        )
    else:
        return
    if status != 200:
        fail("candidate_health_unready")


def rollback_candidate_services(context, platform):
    failures = []
    for name in SERVICE_STOP_ORDER:
        label = context.manifest["services"][name]["label"]
        try:
            platform.launchd_bootout(label)
        except BaseException:
            failures.append(name)
    if failures:
        fail("candidate_service_rollback_incomplete")


def recover_interrupted_start(context, platform, state):
    rollback_candidate_services(context, platform)
    platform.postgres_stop(context.cluster_root)
    if platform.postgres_running(context.cluster_root):
        fail("interrupted_start_recovery_failed")
    save_state(context, "stopped", state["standing_snapshot"])
    append_journal(context, "interrupted_start", "recovered", "run")
    return load_state(context, {"stopped"})


def write_database_evidence(context, database_evidence):
    write_atomic(
        context.artifact_directory / "database-evidence.json",
        canonical_json(database_evidence) + "\n",
    )
    step_one_evidence = {
        "database_system_identifier": database_evidence[
            "database_system_identifier"
        ],
        "migration_count": database_evidence["migration_count"],
        "migration_head": database_evidence["migration_head"],
        "migration_ledger_sha256": database_evidence["migration_ledger_sha256"],
        "discord_resource_prefix": context.manifest["discord"]["resource_prefix"],
    }
    write_atomic(
        context.artifact_directory / "step-01-evidence.json",
        canonical_json(step_one_evidence) + "\n",
    )
    return publish_bootstrap_source(context, step_one_evidence, utc_now())


def candidate_plist_identity(context, name):
    path = service_plist_path(context, name)
    flags = os.O_RDONLY
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    if not hasattr(os, "O_NOFOLLOW"):
        fail(f"candidate_{name}_plist_nofollow_unavailable")
    flags |= os.O_NOFOLLOW
    if hasattr(os, "O_NONBLOCK"):
        flags |= os.O_NONBLOCK
    try:
        descriptor = os.open(path, flags)
    except OSError:
        fail(f"candidate_{name}_plist_unavailable")
    try:
        before = os.fstat(descriptor)
        mode = stat.S_IMODE(before.st_mode)
        if (
            not stat.S_ISREG(before.st_mode)
            or before.st_uid != os.getuid()
            or before.st_nlink != 1
            or before.st_size <= 0
            or before.st_size > 256 * 1024
            or mode != 0o600
        ):
            fail(f"candidate_{name}_plist_identity_invalid")
        raw = bytearray()
        while len(raw) <= 256 * 1024:
            chunk = os.read(descriptor, 64 * 1024)
            if not chunk:
                break
            raw.extend(chunk)
        after = os.fstat(descriptor)
        try:
            named = os.stat(path, follow_symlinks=False)
        except OSError:
            fail(f"candidate_{name}_plist_path_changed")
    finally:
        os.close(descriptor)
    metadata = (
        before.st_dev,
        before.st_ino,
        before.st_mode,
        before.st_uid,
        before.st_nlink,
        before.st_size,
        before.st_mtime_ns,
        before.st_ctime_ns,
    )
    if (
        len(raw) != before.st_size
        or metadata
        != (
            after.st_dev,
            after.st_ino,
            after.st_mode,
            after.st_uid,
            after.st_nlink,
            after.st_size,
            after.st_mtime_ns,
            after.st_ctime_ns,
        )
        or metadata
        != (
            named.st_dev,
            named.st_ino,
            named.st_mode,
            named.st_uid,
            named.st_nlink,
            named.st_size,
            named.st_mtime_ns,
            named.st_ctime_ns,
        )
    ):
        fail(f"candidate_{name}_plist_changed_during_observation")
    expected = plistlib.dumps(
        compose_plists(context)[name], fmt=plistlib.FMT_XML, sort_keys=True
    )
    if bytes(raw) != expected:
        fail(f"candidate_{name}_plist_content_mismatch")
    return {
        "path": str(path),
        "sha256": hashlib.sha256(raw).hexdigest(),
        "size": len(raw),
        "mode": mode,
        "uid": before.st_uid,
        "device": before.st_dev,
        "inode": before.st_ino,
        "links": before.st_nlink,
    }


def candidate_ready_status(context, platform, name):
    service = context.manifest["services"][name]
    host_header = None
    if name == "api":
        host_header = context.manifest["public_origin"].removeprefix("https://")
    return platform.http_status(
        f"http://127.0.0.1:{service['port']}/health/ready",
        host_header=host_header,
    )


def observe_candidate_process(context, platform, name):
    manifest = context.manifest
    service = manifest["services"][name]
    candidate = manifest["candidates"][name]
    expected_plist = str(service_plist_path(context, name))
    expected_arguments = [candidate["path"]]
    first_plist = candidate_plist_identity(context, name)
    first_job = platform.launchd_job(service["label"])
    if (
        not isinstance(first_job, dict)
        or set(first_job)
        != {
            "pid",
            "program",
            "plist_path",
            "arguments",
            "runs",
            "state",
            "last_exit_code",
        }
        or type(first_job["pid"]) is not int
        or first_job["pid"] <= 0
        or first_job["program"] != candidate["path"]
        or first_job["plist_path"] != expected_plist
        or first_job["arguments"] != expected_arguments
        or type(first_job["runs"]) is not int
        or first_job["runs"] <= 0
        or first_job["state"] != "running"
        or first_job["last_exit_code"] is not None
    ):
        fail(f"candidate_{name}_launchd_identity_invalid")
    first_process = platform.candidate_process_identity(
        first_job["pid"], pathlib.Path(candidate["path"])
    )
    ready_status = candidate_ready_status(context, platform, name)
    if ready_status != 200:
        fail(f"candidate_{name}_health_identity_unready")
    health_identity = None
    if name == "runtime":
        health_identity = platform.runtime_process_identity(context)
        if (
            not isinstance(health_identity, dict)
            or health_identity.get("os_pid") != first_job["pid"]
        ):
            fail("candidate_runtime_health_identity_mismatch")
    second_process = platform.candidate_process_identity(
        first_job["pid"], pathlib.Path(candidate["path"])
    )
    second_job = platform.launchd_job(service["label"])
    second_plist = candidate_plist_identity(context, name)
    if first_process != second_process:
        fail(f"candidate_{name}_process_identity_drift")
    if first_job != second_job:
        fail(f"candidate_{name}_launchd_identity_drift")
    if first_plist != second_plist:
        fail(f"candidate_{name}_plist_identity_drift")
    if first_process["sha256"] != candidate["sha256"]:
        fail(f"candidate_{name}_process_digest_mismatch")
    evidence = {
        "launchd": {
            "pid": first_job["pid"],
            "program": first_job["program"],
            "plist_path": first_job["plist_path"],
            "arguments": first_job["arguments"],
            "runs": first_job["runs"],
            "state": first_job["state"],
        },
        "process": first_process,
        "plist": first_plist,
    }
    if health_identity is not None:
        evidence["runtime_health"] = health_identity
    return evidence, ready_status


def revalidate_candidate_process(
    context, platform, name, evidence, expected_ready_status=200
):
    revalidate_candidate_process_identity(
        context, platform, name, evidence
    )
    if type(expected_ready_status) is int:
        allowed_ready_statuses = (expected_ready_status,)
    elif (
        isinstance(expected_ready_status, tuple)
        and expected_ready_status
        and all(type(status) is int for status in expected_ready_status)
    ):
        allowed_ready_statuses = expected_ready_status
    else:
        fail("candidate_ready_status_contract_invalid")
    if candidate_ready_status(
        context, platform, name
    ) not in allowed_ready_statuses:
        fail(f"candidate_{name}_health_final_unready")
    if name == "runtime":
        health = platform.runtime_process_identity(context)
        if health != evidence["runtime_health"]:
            fail("candidate_runtime_health_final_identity_drift")


def revalidate_candidate_process_identity(context, platform, name, evidence):
    service = context.manifest["services"][name]
    job = platform.launchd_job(service["label"])
    expected_job = {
        **evidence["launchd"],
        "last_exit_code": None,
    }
    if job != expected_job:
        fail(f"candidate_{name}_launchd_final_identity_drift")
    process = platform.candidate_process_identity(
        job["pid"], pathlib.Path(context.manifest["candidates"][name]["path"])
    )
    if process != evidence["process"]:
        fail(f"candidate_{name}_process_final_identity_drift")
    if candidate_plist_identity(context, name) != evidence["plist"]:
        fail(f"candidate_{name}_plist_final_identity_drift")
    return job


def build_candidate_evidence(context, statuses, platform):
    manifest = context.manifest
    transport_snapshot = platform.transport_control(context, "snapshot")
    observations = {
        name: observe_candidate_process(context, platform, name)
        for name in ("api", "runtime")
    }
    process_identities = {
        "schema_version": 1,
        "api": observations["api"][0],
        "runtime": observations["runtime"][0],
    }
    if (
        process_identities["api"]["launchd"]["pid"]
        == process_identities["runtime"]["launchd"]["pid"]
    ):
        fail("candidate_process_pid_collision")
    for name in ("api", "runtime"):
        revalidate_candidate_process(
            context, platform, name, process_identities[name]
        )
    return {
        "api_sha256": manifest["candidates"]["api"]["sha256"],
        "runtime_sha256": manifest["candidates"]["runtime"]["sha256"],
        "codex_worker_sha256": manifest["source_trees"]["codex_worker"]["sha256"],
        "d2_toolchain_sha256": manifest["source_trees"]["d2_toolchain"]["sha256"],
        "certification_transport_sha256": manifest["candidates"][
            "certification_transport"
        ]["sha256"],
        "certification_transport_source_sha256": manifest["source_trees"][
            "certification_transport"
        ]["sha256"],
        "api_build_revision": manifest["commit_sha"],
        "runtime_build_revision": manifest["commit_sha"],
        "api_ready_status": observations["api"][1],
        "runtime_ready_status": observations["runtime"][1],
        "worker_ready_status": statuses["worker"],
        "cloudflare_tunnel_id": manifest["cloudflare"]["tunnel_id"],
        "public_origin": manifest["cloudflare"]["public_origin"],
        "origin_service": manifest["cloudflare"]["origin_service"],
        "transport_instance_id": transport_snapshot["instance_id"],
        "transport_ready": statuses["transport"] == 200,
        "tunnel_ready": statuses["tunnel"] == 200,
        "process_identities": process_identities,
    }


def candidate_start_transition_path(context):
    return context.artifact_directory / "candidate-start-transition.json"


def candidate_start_source_path(context):
    return source_path(context, 3, "candidate")


def candidate_start_retirement_path(context):
    return context.artifact_directory / "candidate-start-retirement.json"


def candidate_start_commitment_present(context):
    return (
        os.path.lexists(candidate_start_transition_path(context))
        or os.path.lexists(candidate_start_source_path(context))
        or os.path.lexists(candidate_start_retirement_path(context))
    )


def digest_json(value):
    return hashlib.sha256(canonical_json(value).encode("utf-8")).hexdigest()


def load_candidate_start_retirement(context):
    path = candidate_start_retirement_path(context)
    try:
        require_owned_mode(path, 0o600, "candidate_start_retirement")
    except CertificationError as error:
        fail(str(error))
    value = load_json(path, "candidate_start_retirement_invalid")
    if (
        not isinstance(value, dict)
        or set(value)
        != {
            "schema_version",
            "kind",
            "manifest_sha256",
            "run_id",
            "observed_at",
            "transition_sha256",
            "reason",
        }
        or type(value["schema_version"]) is not int
        or value["schema_version"] != 1
        or value["kind"] != CANDIDATE_START_RETIREMENT_KIND
        or value["manifest_sha256"] != context.digest
        or value["run_id"] != context.manifest["run_id"]
        or not validate_utc_timestamp(value["observed_at"])
        or not isinstance(value["transition_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(value["transition_sha256"])
        or not isinstance(value["reason"], str)
        or value["reason"] not in CANDIDATE_START_RETIREMENT_REASONS
    ):
        fail("candidate_start_retirement_invalid")
    return value


def persist_candidate_start_retirement(context, transition, reason):
    if reason not in CANDIDATE_START_RETIREMENT_REASONS:
        fail("candidate_start_retirement_reason_invalid")
    path = candidate_start_retirement_path(context)
    if os.path.lexists(path):
        return load_candidate_start_retirement(context)
    value = {
        "schema_version": 1,
        "kind": CANDIDATE_START_RETIREMENT_KIND,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "observed_at": utc_now(),
        "transition_sha256": (
            digest_json(transition) if transition is not None else "0" * 64
        ),
        "reason": reason,
    }
    write_atomic(path, canonical_json(value) + "\n")
    if load_candidate_start_retirement(context) != value:
        fail("candidate_start_retirement_replay_drift")
    return value


def retire_candidate_start(context, transition, reason):
    persist_candidate_start_retirement(context, transition, reason)
    fail("candidate_start_transition_retirement_required")


def persist_candidate_abort_retirement(context, state, reason):
    if not candidate_start_commitment_present(context):
        return
    transition = None
    if os.path.lexists(candidate_start_transition_path(context)):
        try:
            transition, _ = load_candidate_start_transition(
                context, state["standing_snapshot"]
            )
        except OrchestratorError:
            transition = None
    persist_candidate_start_retirement(context, transition, reason)


def require_candidate_start_not_retired(
    context, allow_abort_teardown=False
):
    if os.path.lexists(candidate_start_retirement_path(context)):
        load_candidate_start_retirement(context)
        fail("candidate_start_transition_retirement_required")
    if (
        not allow_abort_teardown
        and os.path.lexists(abort_teardown_tombstone_path(context))
    ):
        fail("candidate_start_transition_retirement_required")


def finalization_freeze_committed(context):
    if not os.path.lexists(freeze_intent_path(context)):
        return False
    certified_teardown_binding(context)
    return True


def require_finalization_not_started(context):
    if finalization_freeze_committed(context):
        fail("orchestrator_phase_invalid")


def command_restart_drained_runtime(context, platform):
    require_candidate_start_not_retired(context)
    state = load_state(context, {"candidate_started"})
    require_finalization_not_started(context)
    _transition, evidence, _source = require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_transport_identity(context, platform, state)
    try:
        result = run_restart_drained_runtime(
            context,
            platform,
            evidence["process_identities"]["runtime"]["launchd"]["runs"],
        )
    except OrchestratorError as error:
        require_committed_candidate_processes(
            context, platform, state, ("api",)
        )
        require_committed_transport_identity(context, platform, state)
        if str(error) in {
            "drained_runtime_restart_generation_unjournaled",
            "drained_runtime_restart_process_identity_changed",
            "drained_runtime_restart_replay_drift",
            "drained_runtime_restart_sequence_exhausted",
            "drained_runtime_restart_unjournaled_pid",
            "transport_instance_changed",
        }:
            retire_candidate_start(
                context, _transition, "candidate_identity_drift"
            )
        raise
    except BaseException:
        require_committed_candidate_processes(
            context, platform, state, ("api",)
        )
        require_committed_transport_identity(context, platform, state)
        raise
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_transport_identity(context, platform, state)
    return result


def command_certify_live_runtime_restart(
    context, platform, confirmation_path=None
):
    require_candidate_start_not_retired(context)
    require_finalization_not_started(context)
    state = load_state(context, {"candidate_started"})
    if not os.path.lexists(live_runtime_restart_intent_path(context)):
        transition, _evidence, _source = require_initial_candidate_commitment(
            context, platform, state
        )
    else:
        transition, _evidence, _source = require_committed_candidate_processes(
            context, platform, state, ("api",)
        )
    require_committed_transport_identity(context, platform, state)
    try:
        result = run_certify_live_runtime_restart(
            context, platform, confirmation_path
        )
    except OrchestratorError as error:
        require_committed_candidate_processes(
            context, platform, state, ("api",)
        )
        require_committed_transport_identity(context, platform, state)
        if str(error) in {
            "transport_instance_changed",
            "live_runtime_restart_transport_changed",
        }:
            retire_candidate_start(
                context, transition, "candidate_identity_drift"
            )
        raise
    except BaseException:
        require_committed_candidate_processes(
            context, platform, state, ("api",)
        )
        require_committed_transport_identity(context, platform, state)
        raise
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_transport_identity(context, platform, state)
    return result


def command_finalize_run(context, platform, teardown_boundary):
    require_candidate_start_not_retired(context)
    if not os.path.lexists(freeze_intent_path(context)):
        state = load_state(context, {"candidate_started"})
        require_committed_candidate_processes(
            context, platform, state, ("api",)
        )
        require_committed_runtime_generation(context, platform, state)
        require_committed_transport_identity(context, platform, state)

    def committed_identity_boundary(
        boundary_context, boundary_platform, action, runtime_binding
    ):
        if boundary_context is not context or boundary_platform is not platform:
            fail("finalization_identity_boundary_invalid")
        if action not in {"capture", "suspend", "checkpoint"}:
            fail("finalization_identity_boundary_invalid")
        boundary_state = load_state(boundary_context, {"candidate_started"})
        transition, _evidence, _source = require_committed_candidate_processes(
            boundary_context,
            boundary_platform,
            boundary_state,
            ("api",),
        )
        require_committed_transport_identity(
            boundary_context, boundary_platform, boundary_state
        )
        if action == "capture":
            if runtime_binding is not None:
                fail("finalization_identity_boundary_invalid")
            require_committed_runtime_generation(
                boundary_context, boundary_platform, boundary_state
            )
            captured, _ready = observe_candidate_process(
                boundary_context, boundary_platform, "runtime"
            )
            require_committed_runtime_generation(
                boundary_context, boundary_platform, boundary_state
            )
            revalidate_candidate_process(
                boundary_context,
                boundary_platform,
                "runtime",
                captured,
            )
            return captured
        try:
            committed_transition, _committed_evidence, _committed_source = (
                require_committed_runtime_freeze_binding(
                    boundary_context, boundary_state, runtime_binding
                )
            )
            if committed_transition != transition:
                fail("candidate_runtime_freeze_binding_drift")
            revalidate_candidate_process_identity(
                boundary_context,
                boundary_platform,
                "runtime",
                runtime_binding,
            )
        except OrchestratorError:
            retire_candidate_start(
                boundary_context, transition, "candidate_identity_drift"
            )
        runtime_path = pathlib.Path(
            boundary_context.manifest["candidates"]["runtime"]["path"]
        )
        runtime_pid = runtime_binding["launchd"]["pid"]
        runtime_process = runtime_binding["process"]
        try:
            if action == "suspend":
                boundary_platform.candidate_process_suspend(
                    runtime_pid, runtime_path, runtime_process
                )
            if not boundary_platform.candidate_process_stopped(
                runtime_pid, runtime_path, runtime_process
            ):
                fail("candidate_runtime_freeze_suspend_incomplete")
            revalidate_candidate_process_identity(
                boundary_context,
                boundary_platform,
                "runtime",
                runtime_binding,
            )
        except OrchestratorError:
            retire_candidate_start(
                boundary_context, transition, "candidate_identity_drift"
            )

    def certified_cleanup_boundary(boundary_context, boundary_platform):
        if boundary_context is not context or boundary_platform is not platform:
            fail("certified_cleanup_boundary_invalid")
        return command_cleanup_internal(
            boundary_context, boundary_platform, retire_committed=False
        )

    return run_finalize_run(
        context,
        platform,
        certified_cleanup_boundary,
        teardown_boundary,
        committed_identity_boundary,
    )


def command_finalize_total_absence(
    context, platform, prefix_scan_evidence_path, guild_deletion_evidence_path
):
    require_candidate_start_not_retired(context)
    return run_finalize_total_absence(
        context,
        platform,
        prefix_scan_evidence_path,
        guild_deletion_evidence_path,
    )


def load_candidate_start_transition(context, snapshot):
    path = candidate_start_transition_path(context)
    try:
        require_owned_mode(path, 0o600, "candidate_start_transition")
    except CertificationError as error:
        fail(str(error))
    transition = load_json(path, "candidate_start_transition_invalid")
    if (
        not isinstance(transition, dict)
        or set(transition)
        != {
            "schema_version",
            "kind",
            "manifest_sha256",
            "run_id",
            "observed_at",
            "evidence_sha256",
            "standing_snapshot_sha256",
        }
        or type(transition["schema_version"]) is not int
        or transition["schema_version"] != 1
        or transition["kind"] != CANDIDATE_START_TRANSITION_KIND
        or transition["manifest_sha256"] != context.digest
        or transition["run_id"] != context.manifest["run_id"]
        or not validate_utc_timestamp(transition["observed_at"])
        or not isinstance(transition["evidence_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(transition["evidence_sha256"])
        or not isinstance(transition["standing_snapshot_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(transition["standing_snapshot_sha256"])
        or transition["standing_snapshot_sha256"] != digest_json(snapshot)
    ):
        fail("candidate_start_transition_invalid")
    evidence = load_step_evidence(context, 3)
    try:
        validate_step_contract(3, evidence, context.manifest, [])
    except CertificationError as error:
        fail(f"candidate_start_transition_evidence_invalid:{error}")
    if transition["evidence_sha256"] != digest_json(evidence):
        fail("candidate_start_transition_evidence_drift")
    return transition, evidence


def stage_candidate_start_transition(context, evidence, snapshot):
    if candidate_start_commitment_present(context):
        fail("candidate_start_transition_reentry_invalid")
    write_atomic(
        context.artifact_directory / "step-03-evidence.json",
        canonical_json(evidence) + "\n",
    )
    transition = {
        "schema_version": 1,
        "kind": CANDIDATE_START_TRANSITION_KIND,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "observed_at": utc_now(),
        "evidence_sha256": digest_json(evidence),
        "standing_snapshot_sha256": digest_json(snapshot),
    }
    write_atomic(
        candidate_start_transition_path(context),
        canonical_json(transition) + "\n",
    )
    recorded, recorded_evidence = load_candidate_start_transition(context, snapshot)
    if recorded != transition or recorded_evidence != evidence:
        fail("candidate_start_transition_replay_drift")
    return transition


def load_step_evidence(context, step):
    path = context.artifact_directory / f"step-{step:02d}-evidence.json"
    try:
        require_owned_mode(path, 0o600, f"step_{step:02d}_evidence")
    except CertificationError as error:
        fail(str(error))
    evidence = load_json(path, f"step_{step:02d}_evidence_invalid")
    if not isinstance(evidence, dict) or set(evidence) != set(
        STEP_SPECS[step].required
    ):
        fail(f"step_{step:02d}_evidence_invalid")
    return evidence


def candidate_start_result(bootstrap_source, candidate_source, status):
    return {
        "status": status,
        "phase": "candidate_started",
        "candidate_services_loaded": True,
        "database_schema_ready": True,
        "credentials_sealed": True,
        "coordinator_sources": {
            "1": str(bootstrap_source),
            "3": str(candidate_source),
        },
    }


def require_committed_candidate_identity(
    context, platform, state, transition, evidence
):
    if not platform.postgres_running(context.cluster_root) or any(
        not platform.launchd_loaded(context.manifest["services"][name]["label"])
        for name in SERVICE_START_ORDER
    ):
        retire_candidate_start(context, transition, "candidate_service_drift")
    statuses = candidate_health(context, platform, wait=True)
    if any(status != 200 for status in statuses.values()):
        retire_candidate_start(context, transition, "candidate_health_drift")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        retire_candidate_start(context, transition, "protected_staging_drift")
    try:
        for name in ("api", "runtime"):
            revalidate_candidate_process(
                context,
                platform,
                name,
                evidence["process_identities"][name],
            )
        transport_snapshot = platform.transport_control(context, "snapshot")
    except OrchestratorError:
        retire_candidate_start(context, transition, "candidate_identity_drift")
    if transport_snapshot["instance_id"] != evidence["transport_instance_id"]:
        retire_candidate_start(context, transition, "candidate_identity_drift")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        retire_candidate_start(context, transition, "protected_staging_drift")


def publish_committed_candidate_source(context, transition, evidence):
    candidate_path = candidate_start_source_path(context)
    source_present = os.path.lexists(candidate_path)
    try:
        candidate_path = publish_candidate_source(
            context, evidence, transition["observed_at"]
        )
        source = read_private_source(context, candidate_path, 3, CANDIDATE_KIND)
    except OrchestratorError:
        if source_present or os.path.lexists(candidate_path):
            retire_candidate_start(
                context, transition, "candidate_source_drift"
            )
        raise
    if (
        source["observed_at"] != transition["observed_at"]
        or source["evidence"] != evidence
    ):
        retire_candidate_start(context, transition, "candidate_source_drift")
    return candidate_path


def read_committed_candidate_source(context, transition, evidence):
    candidate_path = candidate_start_source_path(context)
    if not os.path.lexists(candidate_path):
        retire_candidate_start(context, transition, "candidate_source_drift")
    try:
        source = read_private_source(context, candidate_path, 3, CANDIDATE_KIND)
    except OrchestratorError:
        retire_candidate_start(context, transition, "candidate_source_drift")
    if (
        source["observed_at"] != transition["observed_at"]
        or source["evidence"] != evidence
    ):
        retire_candidate_start(context, transition, "candidate_source_drift")
    return candidate_path


def load_committed_candidate_artifacts(context, state):
    if not os.path.lexists(candidate_start_transition_path(context)):
        retire_candidate_start(context, None, "transition_invalid")
    try:
        transition, evidence = load_candidate_start_transition(
            context, state["standing_snapshot"]
        )
    except OrchestratorError:
        retire_candidate_start(context, None, "transition_invalid")
    candidate_source = read_committed_candidate_source(
        context, transition, evidence
    )
    return transition, evidence, candidate_source


def require_committed_candidate_processes(
    context, platform, state, names
):
    if not names or any(name not in {"api", "runtime"} for name in names):
        fail("candidate_process_selection_invalid")
    transition, evidence, candidate_source = load_committed_candidate_artifacts(
        context, state
    )
    try:
        for name in names:
            revalidate_candidate_process(
                context,
                platform,
                name,
                evidence["process_identities"][name],
            )
    except OrchestratorError:
        retire_candidate_start(
            context, transition, "candidate_identity_drift"
        )
    return transition, evidence, candidate_source


def require_committed_runtime_generation(
    context, platform, state, expected_ready_status=200
):
    transition, evidence, candidate_source = load_committed_candidate_artifacts(
        context, state
    )
    try:
        records, pending = drained_runtime_restart_inventory(context)
        live_chain_before = committed_live_runtime_restart_chain(context)
    except OrchestratorError:
        retire_candidate_start(
            context, transition, "candidate_identity_drift"
        )
    if pending is not None or live_chain_before["status"] in {
        "pending",
        "complete_unpublished",
    }:
        fail("candidate_restart_protocol_pending")
    if not records:
        try:
            revalidate_candidate_process(
                context,
                platform,
                "runtime",
                evidence["process_identities"]["runtime"],
                expected_ready_status,
            )
        except OrchestratorError:
            retire_candidate_start(
                context, transition, "candidate_identity_drift"
            )
        try:
            live_chain_after = committed_live_runtime_restart_chain(context)
        except OrchestratorError:
            retire_candidate_start(
                context, transition, "candidate_identity_drift"
            )
        if live_chain_after != live_chain_before:
            retire_candidate_start(
                context, transition, "candidate_identity_drift"
            )
        return transition, evidence, candidate_source
    if len(records) != 1 or "complete" not in records[0]:
        retire_candidate_start(
            context, transition, "candidate_identity_drift"
        )
    completion = records[0]["complete"]
    try:
        generation = require_bound_runtime_generation(
            context,
            platform,
            drained_runtime_restart_identity(context),
            completion["new_pid"],
            completion["new_runs"],
            "candidate_runtime_generation_drift",
            expected_ready_status,
        )
    except OrchestratorError:
        retire_candidate_start(
            context, transition, "candidate_identity_drift"
        )
    if (
        generation["process_identity"]
        != completion["new_process_identity"]
        or generation["runtime_health"]
        != completion["new_runtime_health"]
    ):
        retire_candidate_start(
            context, transition, "candidate_identity_drift"
        )
    try:
        live_chain_after = committed_live_runtime_restart_chain(context)
    except OrchestratorError:
        retire_candidate_start(
            context, transition, "candidate_identity_drift"
        )
    if live_chain_after != live_chain_before:
        retire_candidate_start(
            context, transition, "candidate_identity_drift"
        )
    return transition, evidence, candidate_source


def require_committed_runtime_freeze_binding(context, state, binding):
    validate_runtime_freeze_binding(context, binding)
    transition, evidence, candidate_source = load_committed_candidate_artifacts(
        context, state
    )
    records, pending = drained_runtime_restart_inventory(context)
    live_chain = committed_live_runtime_restart_chain(context)
    if pending is not None or live_chain["status"] in {
        "pending",
        "complete_unpublished",
    }:
        fail("candidate_restart_protocol_pending")
    if not records:
        if binding != evidence["process_identities"]["runtime"]:
            fail("candidate_runtime_freeze_binding_drift")
        return transition, evidence, candidate_source
    if len(records) != 1 or "complete" not in records[0]:
        fail("candidate_runtime_freeze_binding_drift")
    completion = records[0]["complete"]
    launchd = binding["launchd"]
    if (
        binding["process"] != completion["new_process_identity"]
        or binding["runtime_health"] != completion["new_runtime_health"]
        or launchd["pid"] != completion["new_pid"]
        or launchd["runs"] != completion["new_runs"]
    ):
        fail("candidate_runtime_freeze_binding_drift")
    return transition, evidence, candidate_source


def require_committed_transport_snapshot(context, state, snapshot):
    transition, evidence, candidate_source = load_committed_candidate_artifacts(
        context, state
    )
    if (
        not isinstance(snapshot, dict)
        or snapshot.get("instance_id")
        != evidence["transport_instance_id"]
    ):
        retire_candidate_start(
            context, transition, "candidate_identity_drift"
        )
    return transition, evidence, candidate_source


def require_committed_transport_identity(context, platform, state):
    snapshot = platform.transport_control(context, "snapshot")
    require_committed_transport_snapshot(context, state, snapshot)
    return snapshot


def require_initial_candidate_commitment(context, platform, state):
    transition, evidence, candidate_source = load_committed_candidate_artifacts(
        context, state
    )
    require_committed_candidate_identity(
        context, platform, state, transition, evidence
    )
    require_committed_candidate_identity(
        context, platform, state, transition, evidence
    )
    return transition, evidence, candidate_source


def candidate_restart_protocol_committed(context):
    live_chain = committed_live_runtime_restart_chain(context)
    if live_chain["status"] != "absent":
        return live_chain["status"]
    records, _pending = drained_runtime_restart_inventory(context)
    return "drained" if records else None


def recover_candidate_start_transition(context, platform, state):
    require_candidate_start_not_retired(context)
    if state["phase"] != "candidate_starting":
        retire_candidate_start(context, None, "state_drift")
    if not os.path.lexists(candidate_start_transition_path(context)):
        retire_candidate_start(context, None, "transition_invalid")
    try:
        transition, evidence = load_candidate_start_transition(
            context, state["standing_snapshot"]
        )
    except OrchestratorError:
        retire_candidate_start(context, None, "transition_invalid")
    require_committed_candidate_identity(
        context, platform, state, transition, evidence
    )
    candidate_path = publish_committed_candidate_source(
        context, transition, evidence
    )
    require_committed_candidate_identity(
        context, platform, state, transition, evidence
    )
    bootstrap_source = publish_bootstrap_source(
        context, load_step_evidence(context, 1), utc_now()
    )
    append_journal(
        context,
        "candidate_start_transition",
        "complete",
        transition["evidence_sha256"],
    )
    append_journal(context, "postgres_start", "complete", "cluster")
    require_committed_candidate_identity(
        context, platform, state, transition, evidence
    )
    save_state(context, "candidate_started", state["standing_snapshot"])
    require_committed_candidate_identity(
        context, platform, state, transition, evidence
    )
    return candidate_start_result(
        bootstrap_source, candidate_path, "candidate_start_recovered"
    )


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


def command_start(context, platform):
    state = load_state(context)
    require_candidate_start_not_retired(context)
    require_finalization_not_started(context)
    if state["phase"] in {"cleaned", "onboarding"}:
        fail("orchestrator_phase_invalid")
    if state["phase"] == "candidate_started":
        try:
            restart_protocol_committed = candidate_restart_protocol_committed(
                context
            )
        except OrchestratorError:
            transition, _evidence, _source = (
                load_committed_candidate_artifacts(context, state)
            )
            retire_candidate_start(
                context, transition, "candidate_identity_drift"
            )
        if restart_protocol_committed is not None:
            require_committed_candidate_processes(
                context, platform, state, ("api",)
            )
            if restart_protocol_committed != "pending":
                require_committed_runtime_generation(context, platform, state)
            fail("orchestrator_phase_invalid")
        _transition, _evidence, candidate_source = (
            require_initial_candidate_commitment(
                context, platform, state
            )
        )
        bootstrap_source = publish_bootstrap_source(
            context, load_step_evidence(context, 1), utc_now()
        )
        return {
            "status": "already_started",
            "phase": "candidate_started",
            "coordinator_sources": {
                "1": str(bootstrap_source),
                "3": str(candidate_source),
            },
        }
    if candidate_start_commitment_present(context):
        return recover_candidate_start_transition(context, platform, state)
    if state["phase"] in {
        "substrate_starting",
        "substrate_started",
        "credentials_sealing",
        "candidate_starting",
    }:
        state = recover_interrupted_start(context, platform, state)
    if state["phase"] not in {"prepared", "stopped"}:
        fail("orchestrator_phase_invalid")
    if platform.postgres_running(context.cluster_root):
        fail("postgres_state_drift")
    rollback_candidate_services(context, platform)
    validate_ports(context, platform, require_available=True)
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    save_state(context, "substrate_starting", state["standing_snapshot"])
    append_journal(context, "postgres_start", "intent", "cluster")
    try:
        configure_postgres_bootstrap_network(context)
        platform.postgres_start(context.cluster_root, context.postgres_log)
        if not platform.postgres_running(context.cluster_root):
            fail("postgres_start_unconfirmed")
        if not platform.port_available(context.manifest["database"]["port"]):
            fail("bootstrap_tcp_exposure_detected")
        append_journal(context, "database_bootstrap", "intent", "database")
        database_evidence = platform.bootstrap_database(context)
        bootstrap_source = write_database_evidence(context, database_evidence)
        append_journal(context, "database_bootstrap", "complete", "database")
        save_state(context, "credentials_sealing", state["standing_snapshot"])
        present, total = managed_keychain_presence(context, platform)
        if present != total:
            provisioning = platform.provision_credentials(context)
            if present == 0 and provisioning["outcome"] != "fresh":
                fail("sealed_provisioning_outcome_invalid")
            if present != 0:
                fail("partial_credentials_not_quarantined")
        configure_postgres_sealed_network(context)
        platform.postgres_stop(context.cluster_root)
        platform.postgres_start(context.cluster_root, context.postgres_log)
        if not platform.postgres_loopback_accepting(context):
            fail("sealed_postgres_unready")
        replay = platform.provision_credentials(context)
        if replay["outcome"] != "exact_replay":
            fail("sealed_replay_required")
        save_state(context, "candidate_starting", state["standing_snapshot"])
        for name in SERVICE_START_ORDER:
            label = context.manifest["services"][name]["label"]
            append_journal(context, "launchd_start", "intent", label)
            platform.launchd_start(label, service_plist_path(context, name))
            append_journal(context, "launchd_start", "complete", label)
            require_started_dependency(context, platform, name)
        statuses = candidate_health(context, platform, wait=True)
        if any(status != 200 for status in statuses.values()):
            fail("candidate_health_unready")
        if standing_snapshot(context, platform) != state["standing_snapshot"]:
            fail("protected_staging_state_changed")
        candidate_evidence = build_candidate_evidence(context, statuses, platform)
        if standing_snapshot(context, platform) != state["standing_snapshot"]:
            fail("protected_staging_state_changed")
        transition = stage_candidate_start_transition(
            context, candidate_evidence, state["standing_snapshot"]
        )
        require_committed_candidate_identity(
            context, platform, state, transition, candidate_evidence
        )
        candidate_source = publish_committed_candidate_source(
            context, transition, candidate_evidence
        )
        require_committed_candidate_identity(
            context, platform, state, transition, candidate_evidence
        )
        append_journal(
            context,
            "candidate_start_transition",
            "complete",
            transition["evidence_sha256"],
        )
        append_journal(context, "postgres_start", "complete", "cluster")
        require_committed_candidate_identity(
            context, platform, state, transition, candidate_evidence
        )
        save_state(context, "candidate_started", state["standing_snapshot"])
        require_committed_candidate_identity(
            context, platform, state, transition, candidate_evidence
        )
        return candidate_start_result(
            bootstrap_source, candidate_source, "candidate_started"
        )
    except BaseException:
        if candidate_start_commitment_present(context):
            raise
        try:
            rollback_candidate_services(context, platform)
            platform.postgres_stop(context.cluster_root)
            save_state(context, "stopped", state["standing_snapshot"])
            append_journal(context, "candidate_start", "rolled_back", "run")
        except BaseException:
            append_journal(context, "candidate_start", "rollback_failed", "run")
        raise


def command_stop(context, platform):
    state = load_state(
        context,
        {
            "prepared",
            "substrate_starting",
            "substrate_started",
            "credentials_sealing",
            "candidate_starting",
            "candidate_started",
            "onboarding",
            "stopped",
        },
    )
    persist_candidate_abort_retirement(context, state, "explicit_stop")
    failures = []
    for name in SERVICE_STOP_ORDER:
        label = context.manifest["services"][name]["label"]
        append_journal(context, "launchd_bootout", "intent", label)
        try:
            platform.launchd_bootout(label)
            append_journal(context, "launchd_bootout", "complete", label)
        except BaseException:
            failures.append(name)
            append_journal(context, "launchd_bootout", "failed", label)
    append_journal(context, "postgres_stop", "intent", "cluster")
    try:
        platform.postgres_stop(context.cluster_root)
    except BaseException:
        failures.append("postgres")
    try:
        if any(
            not platform.launchd_absent(service["label"])
            for service in context.manifest["services"].values()
        ):
            failures.append("launchd_absence")
    except BaseException:
        failures.append("launchd_observation")
    try:
        if not cleanup_postgres_absent(context, platform):
            failures.append("postgres_absence")
    except BaseException:
        failures.append("postgres_observation")
    if failures:
        fail("candidate_stop_incomplete")
    append_journal(context, "postgres_stop", "complete", "cluster")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    save_state(context, "stopped", state["standing_snapshot"])
    return {"status": "stopped", "phase": "stopped"}


def command_onboard(context, platform, principal_id, display_name):
    require_candidate_start_not_retired(context)
    require_finalization_not_started(context)
    state = load_state(context, {"candidate_started", "onboarding"})
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_runtime_generation(context, platform, state)
    require_committed_transport_identity(context, platform, state)
    if not principal_id.startswith("discord:"):
        fail("onboarding_principal_invalid")
    validate_snowflake(principal_id.removeprefix("discord:"), "onboarding_principal")
    if principal_id != f"discord:{context.manifest['discord']['actor_id']}":
        fail("onboarding_principal_invalid")
    if (
        not display_name
        or len(display_name.encode("utf-8")) > 512
        or len(display_name) > 128
        or display_name != display_name.strip()
        or any(unicodedata.category(character).startswith("C") for character in display_name)
    ):
        fail("onboarding_display_name_invalid")
    if not platform.postgres_running(context.cluster_root) or any(
        not platform.launchd_loaded(context.manifest["services"][name]["label"])
        for name in SERVICE_START_ORDER
    ):
        fail("candidate_state_drift")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    installation_id = (
        f"installation:{context.manifest['discord']['resource_prefix']}"
    )
    save_state(context, "onboarding", state["standing_snapshot"])
    append_journal(context, "installation_onboard", "intent", "installation")
    try:
        evidence = platform.onboard_installation(
            context, principal_id, display_name, installation_id
        )
        require_committed_candidate_processes(
            context, platform, state, ("api",)
        )
        require_committed_runtime_generation(context, platform, state)
        require_committed_transport_identity(context, platform, state)
        output = {
            "outcome": evidence["outcome"],
            "installation_id": evidence["installation_id"],
            "principal_id": evidence["principal_id"],
            "guild_id": context.manifest["discord"]["guild_id"],
            "discord_application_id": context.manifest["discord"]["application_id"],
            "binding_key": evidence["binding_key"],
            "hub_channel_id": evidence["hub_channel_id"],
        }
        write_atomic(
            context.artifact_directory / "onboarding-evidence.json",
            canonical_json(output) + "\n",
        )
        coordinator_source = publish_onboarding_source(
            context, output, utc_now()
        )
        require_committed_candidate_processes(
            context, platform, state, ("api",)
        )
        require_committed_runtime_generation(context, platform, state)
        require_committed_transport_identity(context, platform, state)
        append_journal(context, "installation_onboard", "complete", "installation")
        save_state(context, "candidate_started", state["standing_snapshot"])
        return {
            "status": "onboarded",
            **output,
            "coordinator_source": str(coordinator_source),
        }
    except BaseException:
        save_state(context, "candidate_started", state["standing_snapshot"])
        append_journal(context, "installation_onboard", "failed", "installation")
        raise


TRANSPORT_OPERATIONS = {
    "snapshot": "snapshot",
    "arm-next-duplicate": "arm_next_duplicate",
    "disarm-duplicate": "disarm_duplicate",
    "arm-next-indeterminate": "arm_next_create_role_indeterminate",
    "disarm-indeterminate": "disarm_indeterminate",
    "partition-gateway": "partition_gateway",
    "heal-gateway": "heal_gateway",
}
TRANSPORT_CONTROL_FILE_PATTERN = re.compile(
    r"^([0-9]{4})-([a-z-]+)-(intent|complete)\.json$"
)
TRANSPORT_OPERATION_ID_PATTERN = re.compile(r"^[a-z][a-z0-9_.:-]{7,95}$")
TRANSPORT_RECORDED_AT_PATTERN = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$"
)
EVIDENCE_RECORDED_AT_PATTERN = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?Z$"
)


def transport_control_directory(context):
    return context.artifact_directory / "transport-controls"


def transport_control_inventory(context):
    directory = transport_control_directory(context)
    if not directory.exists():
        return [], None
    metadata = directory.lstat()
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or directory.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        fail("transport_evidence_directory_invalid")
    records = {}
    try:
        entries = list(directory.iterdir())
    except OSError:
        fail("transport_evidence_inventory_invalid")
    for entry in entries:
        match = TRANSPORT_CONTROL_FILE_PATTERN.fullmatch(entry.name)
        try:
            entry_metadata = entry.lstat()
        except OSError:
            fail("transport_evidence_inventory_invalid")
        if (
            match is None
            or not stat.S_ISREG(entry_metadata.st_mode)
            or entry.is_symlink()
            or entry_metadata.st_uid != os.getuid()
            or stat.S_IMODE(entry_metadata.st_mode) != 0o600
        ):
            fail("transport_evidence_inventory_invalid")
        sequence = int(match.group(1))
        operation = match.group(2)
        kind = match.group(3)
        if sequence == 0 or operation not in TRANSPORT_OPERATIONS:
            fail("transport_evidence_inventory_invalid")
        record = records.setdefault(sequence, {"operation": operation})
        if record["operation"] != operation or kind in record:
            fail("transport_evidence_inventory_invalid")
        record[kind] = load_json(entry, "transport_evidence_invalid")
    ordered = []
    for expected_sequence, sequence in enumerate(sorted(records), 1):
        if sequence != expected_sequence:
            fail("transport_evidence_inventory_invalid")
        record = records[sequence]
        if "intent" not in record:
            fail("transport_evidence_inventory_invalid")
        intent = record["intent"]
        expected_intent = {
            "schema_version",
            "manifest_sha256",
            "recorded_at",
            "sequence",
            "operation",
            "command",
            "operation_id",
        }
        if (
            not isinstance(intent, dict)
            or set(intent) != expected_intent
            or intent["schema_version"] != 1
            or intent["manifest_sha256"] != context.digest
            or intent["sequence"] != sequence
            or intent["operation"] != record["operation"]
            or intent["command"] != TRANSPORT_OPERATIONS[record["operation"]]
            or intent["operation_id"]
            != f"d2:{context.digest[:16]}:{sequence:04d}:{record['operation']}"
            or not isinstance(intent["recorded_at"], str)
            or not TRANSPORT_RECORDED_AT_PATTERN.fullmatch(intent["recorded_at"])
            or not isinstance(intent["operation_id"], str)
            or not TRANSPORT_OPERATION_ID_PATTERN.fullmatch(intent["operation_id"])
        ):
            fail("transport_evidence_invalid")
        complete = record.get("complete")
        if complete is not None:
            if (
                not isinstance(complete, dict)
                or set(complete)
                != {
                    "schema_version",
                    "manifest_sha256",
                    "recorded_at",
                    "sequence",
                    "operation",
                    "command",
                    "operation_id",
                    "response",
                    "snapshot",
                }
                or complete["schema_version"] != 1
                or complete["manifest_sha256"] != context.digest
                or complete["sequence"] != sequence
                or complete["operation"] != intent["operation"]
                or complete["command"] != intent["command"]
                or complete["operation_id"] != intent["operation_id"]
                or not isinstance(complete["recorded_at"], str)
                or not TRANSPORT_RECORDED_AT_PATTERN.fullmatch(
                    complete["recorded_at"]
                )
                or complete["response"] is not None
                and not isinstance(complete["response"], dict)
                or not isinstance(complete["snapshot"], dict)
            ):
                fail("transport_evidence_invalid")
        ordered.append({"sequence": sequence, **record})
    pending = [record for record in ordered if "complete" not in record]
    if len(pending) > 1 or pending and pending[0] is not ordered[-1]:
        fail("transport_evidence_inventory_invalid")
    return ordered, pending[0] if pending else None


def transport_operation_postcondition(operation, operation_id, response, snapshot):
    gateway = snapshot["gateway"]
    effect = snapshot["effect_http"]
    if operation == "snapshot":
        return
    if operation == "arm-next-duplicate":
        if response["disposition"] == "busy":
            fail("transport_operation_busy")
        if not (
            gateway["armed_duplicate_operation_id"] == operation_id
            or gateway["claimed_duplicate_operation_id"] == operation_id
            or gateway["last_duplicate_operation_id"] == operation_id
        ):
            fail("transport_operation_not_applied")
        return
    if operation == "arm-next-indeterminate":
        if response["disposition"] == "busy":
            fail("transport_operation_busy")
        if not (
            effect["armed_indeterminate_operation_id"] == operation_id
            or effect["claimed_indeterminate_operation_id"] == operation_id
            or effect["last_indeterminate_operation_id"] == operation_id
        ):
            fail("transport_operation_not_applied")
        return
    expected = {
        "disarm-duplicate": not gateway["duplicate_armed"]
        and not gateway["duplicate_claimed"],
        "disarm-indeterminate": not effect["indeterminate_armed"]
        and not effect["indeterminate_claimed"],
        "partition-gateway": gateway["partitioned"],
        "heal-gateway": not gateway["partitioned"],
    }[operation]
    if not expected:
        fail("transport_operation_not_applied")


def validate_transport_control_history(context, records):
    validator = Platform()
    pinned_instance_id = pinned_transport_instance_id(context)
    for record in records:
        complete = record.get("complete")
        if complete is None:
            continue
        response = complete["response"]
        operation = complete["operation"]
        if operation == "snapshot":
            if response is not None:
                fail("transport_evidence_invalid")
        elif operation in {"arm-next-duplicate", "arm-next-indeterminate"}:
            if (
                not isinstance(response, dict)
                or set(response) != {"changed", "disposition"}
                or type(response["changed"]) is not bool
                or response["disposition"] not in {"armed", "replayed"}
                or (response["disposition"] == "armed") != response["changed"]
            ):
                fail("transport_evidence_invalid")
        elif (
            not isinstance(response, dict)
            or set(response) != {"changed"}
            or type(response["changed"]) is not bool
        ):
            fail("transport_evidence_invalid")
        snapshot = complete["snapshot"]
        if (
            not validator._transport_snapshot_valid(context, snapshot)
            or snapshot["instance_id"] != pinned_instance_id
        ):
            fail("transport_evidence_invalid")
        transport_operation_postcondition(
            operation, complete["operation_id"], response, snapshot
        )


def gateway_control_completion_bindings(context, expected_operations):
    records, pending = transport_control_inventory(context)
    validate_transport_control_history(context, records)
    if pending is not None:
        fail("transport_gateway_operation_pending")
    gateway_records = [
        record
        for record in records
        if record["operation"] in {"partition-gateway", "heal-gateway"}
    ]
    if [record["operation"] for record in gateway_records] != list(
        expected_operations
    ):
        fail("transport_gateway_operation_history_invalid")
    bindings = []
    for record in gateway_records:
        complete = record.get("complete")
        if complete is None:
            fail("transport_gateway_operation_incomplete")
        bindings.append(
            {
                "operation_id": complete["operation_id"],
                "completion_sha256": hashlib.sha256(
                    canonical_json(complete).encode("utf-8")
                ).hexdigest(),
                "snapshot": complete["snapshot"],
            }
        )
    return bindings


def command_transport_control(context, platform, operation):
    require_candidate_start_not_retired(context)
    require_finalization_not_started(context)
    state = load_state(context, {"candidate_started"})
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_runtime_generation(
        context, platform, state, expected_ready_status=(200, 503)
    )
    if not platform.postgres_running(context.cluster_root) or any(
        not platform.launchd_loaded(context.manifest["services"][name]["label"])
        for name in SERVICE_START_ORDER
    ):
        fail("candidate_state_drift")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    command = TRANSPORT_OPERATIONS.get(operation)
    if command is None:
        fail("transport_operation_invalid")
    pre_snapshot = platform.transport_control(context, "snapshot")
    require_committed_transport_snapshot(context, state, pre_snapshot)
    records, pending = transport_control_inventory(context)
    validate_transport_control_history(context, records)
    if pending is not None:
        intent = pending["intent"]
        if intent["operation"] != operation:
            fail("transport_operation_pending")
        sequence = intent["sequence"]
        operation_id = intent["operation_id"]
    else:
        if operation == "arm-next-duplicate" and (
            pre_snapshot["gateway"]["duplicate_armed"]
            or pre_snapshot["gateway"]["duplicate_claimed"]
        ):
            fail("transport_operation_busy")
        if operation == "arm-next-indeterminate" and (
            pre_snapshot["effect_http"]["indeterminate_armed"]
            or pre_snapshot["effect_http"]["indeterminate_claimed"]
        ):
            fail("transport_operation_busy")
        sequence = len(records) + 1
        if sequence > 9999:
            fail("transport_evidence_capacity_exhausted")
        operation_id = f"d2:{context.digest[:16]}:{sequence:04d}:{operation}"
        intent = {
            "schema_version": 1,
            "manifest_sha256": context.digest,
            "recorded_at": utc_now(),
            "sequence": sequence,
            "operation": operation,
            "command": command,
            "operation_id": operation_id,
        }
        intent_path = transport_control_directory(context) / (
            f"{sequence:04d}-{operation}-intent.json"
        )
        write_atomic(intent_path, canonical_json(intent) + "\n")
        append_journal(
            context, "transport_control", "intent", operation_id.replace(":", "_")
        )
    response = None
    if operation != "snapshot":
        fields = (
            {"operation_id": operation_id}
            if operation in {"arm-next-duplicate", "arm-next-indeterminate"}
            else {}
        )
        response = platform.transport_control(context, command, fields)
    snapshot = platform.transport_control(context, "snapshot")
    require_committed_transport_snapshot(context, state, snapshot)
    transport_operation_postcondition(operation, operation_id, response, snapshot)
    evidence = {
        "schema_version": 1,
        "manifest_sha256": context.digest,
        "recorded_at": utc_now(),
        "sequence": sequence,
        "operation": operation,
        "command": command,
        "operation_id": operation_id,
        "response": response,
        "snapshot": snapshot,
    }
    evidence_path = transport_control_directory(context) / (
        f"{sequence:04d}-{operation}-complete.json"
    )
    write_atomic(evidence_path, canonical_json(evidence) + "\n")
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_runtime_generation(
        context, platform, state, expected_ready_status=(200, 503)
    )
    require_committed_transport_identity(context, platform, state)
    append_journal(
        context, "transport_control", "complete", operation_id.replace(":", "_")
    )
    return {
        "status": "controlled",
        "operation": operation,
        "operation_id": operation_id,
        "response": response,
        "evidence": str(evidence_path),
        "snapshot": snapshot,
    }


def require_candidate_certification_boundary(
    context, platform, allow_abort_teardown=False
):
    require_candidate_start_not_retired(context, allow_abort_teardown)
    require_finalization_not_started(context)
    state = load_state(context, {"candidate_started"})
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_runtime_generation(context, platform, state)
    if not platform.postgres_running(context.cluster_root) or any(
        not platform.launchd_loaded(context.manifest["services"][name]["label"])
        for name in SERVICE_START_ORDER
    ):
        fail("candidate_state_drift")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    statuses = candidate_health(context, platform, wait=False)
    if statuses != {
        "worker": 200,
        "transport": 200,
        "api": 200,
        "runtime": 200,
        "tunnel": 200,
    }:
        fail("candidate_health_unready")
    snapshot = platform.transport_control(context, "snapshot")
    require_committed_transport_snapshot(context, state, snapshot)
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_runtime_generation(context, platform, state)
    require_committed_transport_identity(context, platform, state)
    return state, snapshot


def require_frozen_discord_teardown_boundary(context, platform):
    require_candidate_start_not_retired(context)
    state = load_state(context, {"candidate_started"})
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    if not platform.postgres_running(context.cluster_root):
        fail("finalization_freeze_state_drift")
    required = ("transport", "worker", "api")
    stopped = ("runtime", "tunnel")
    if any(
        not platform.launchd_loaded(context.manifest["services"][name]["label"])
        for name in required
    ) or any(
        platform.launchd_loaded(context.manifest["services"][name]["label"])
        for name in stopped
    ):
        fail("finalization_freeze_state_drift")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    snapshot = platform.transport_control(context, "snapshot")
    require_committed_transport_snapshot(context, state, snapshot)
    require_certified_teardown_snapshot(context, snapshot)
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_transport_identity(context, platform, state)
    return state, snapshot


def command_resource_inventory(context, platform):
    _state, snapshot = require_candidate_certification_boundary(context, platform)
    inventory = platform.transport_control(context, "resource_inventory")
    if inventory["instance_id"] != snapshot["instance_id"]:
        fail("transport_instance_changed")
    return {
        "status": "observed",
        "phase": "candidate_started",
        "manifest_sha256": context.digest,
        "transport_instance_id": inventory["instance_id"],
        "inventory_digest_sha256": inventory["digest_sha256"],
        "created_count": len(inventory["created"]),
        "deleted_count": len(inventory["deleted"]),
        "active_count": len(inventory["active"]),
        "resource_inventory": inventory,
    }


def require_gateway_loss_certification_boundary(context, platform):
    require_candidate_start_not_retired(context)
    require_finalization_not_started(context)
    state = load_state(context, {"candidate_started"})
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_runtime_generation(
        context, platform, state, expected_ready_status=(200, 503)
    )
    if not platform.postgres_running(context.cluster_root) or any(
        not platform.launchd_loaded(context.manifest["services"][name]["label"])
        for name in SERVICE_START_ORDER
    ):
        fail("candidate_state_drift")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    snapshot = platform.transport_control(context, "snapshot")
    require_committed_transport_snapshot(context, state, snapshot)
    runtime_status = platform.http_status(
        "http://127.0.0.1:"
        f"{context.manifest['services']['runtime']['port']}/health/ready"
    )
    if runtime_status != 503:
        fail("gateway_loss_runtime_readiness_invalid")
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_runtime_generation(
        context, platform, state, expected_ready_status=(200, 503)
    )
    require_committed_transport_identity(context, platform, state)
    return snapshot, runtime_status


def require_gateway_healed_certification_boundary(context, platform):
    _state, snapshot = require_candidate_certification_boundary(context, platform)
    runtime_status = platform.http_status(
        "http://127.0.0.1:"
        f"{context.manifest['services']['runtime']['port']}/health/ready"
    )
    if runtime_status != 200:
        fail("gateway_healed_runtime_readiness_invalid")
    return snapshot, runtime_status


def transport_evidence_path(context, checkpoint):
    return context.artifact_directory / "transport-evidence" / f"{checkpoint}.json"


def validate_transport_evidence_payload(context, checkpoint, evidence):
    common = {"schema_version", "kind", "observed_at"}
    fields = {
        "interaction": {
            "role_ids",
            "channel_ids",
            "panel_message_ids",
            "inventory_digest_sha256",
            "transport_instance_id",
        },
        "duplicate": {
            "interaction_id",
            "delivery_count",
            "transport_duplicate_injections",
            "transport_duplicate_delivery_count",
            "transport_last_duplicate_interaction_id",
            "role_ids",
            "channel_ids",
            "panel_message_ids",
            "inventory_digest_sha256",
            "transport_instance_id",
        },
        "reconciliation": {
            "injected_outcome",
            "transport_indeterminate_injections",
            "transport_last_audit_reason_sha256",
            "transport_last_upstream_status",
            "transport_instance_id",
        },
        "gateway-loss": {
            "gateway_disconnected",
            "runtime_ready_status",
            "transport_gateway_partitioned",
            "transport_gateway_partition_events",
            "transport_instance_id",
            "partition_operation_id",
            "partition_completion_sha256",
        },
        "gateway-healed": {
            "gateway_connected",
            "runtime_ready_status",
            "transport_gateway_partitioned",
            "transport_gateway_partition_events",
            "transport_duplicate_armed",
            "transport_duplicate_claimed",
            "transport_indeterminate_armed",
            "transport_indeterminate_claimed",
            "transport_instance_id",
            "partition_operation_id",
            "partition_completion_sha256",
            "heal_operation_id",
            "heal_completion_sha256",
        },
    }
    if checkpoint not in TRANSPORT_EVIDENCE_KINDS:
        fail("transport_evidence_checkpoint_invalid")
    if (
        not isinstance(evidence, dict)
        or set(evidence) != common | fields[checkpoint]
        or evidence["schema_version"] != 1
        or evidence["kind"] != TRANSPORT_EVIDENCE_KINDS[checkpoint]
        or not isinstance(evidence["observed_at"], str)
        or not TRANSPORT_RECORDED_AT_PATTERN.fullmatch(evidence["observed_at"])
        or evidence["transport_instance_id"] != pinned_transport_instance_id(context)
    ):
        fail("transport_evidence_invalid")
    if checkpoint == "interaction":
        _require_transport_inventory_projection(evidence)
        for field in ("role_ids", "channel_ids", "panel_message_ids"):
            values = evidence[field]
            if (
                not isinstance(values, list)
                or not values
                or len(values) > 128
                or values != sorted(values)
                or len(values) != len(set(values))
            ):
                fail("transport_interaction_inventory_invalid")
            for value in values:
                validate_snowflake(value, f"transport_{field}")
    elif checkpoint == "duplicate":
        _require_transport_inventory_projection(evidence)
        validate_snowflake(evidence["interaction_id"], "transport_interaction_id")
        validate_snowflake(
            evidence["transport_last_duplicate_interaction_id"],
            "transport_last_duplicate_interaction_id",
        )
        if (
            evidence["interaction_id"]
            != evidence["transport_last_duplicate_interaction_id"]
            or type(evidence["delivery_count"]) is not int
            or evidence["delivery_count"] != 2
            or type(evidence["transport_duplicate_injections"]) is not int
            or evidence["transport_duplicate_injections"] != 1
            or type(evidence["transport_duplicate_delivery_count"]) is not int
            or evidence["transport_duplicate_delivery_count"] != 2
        ):
            fail("transport_duplicate_evidence_invalid")
    elif checkpoint == "reconciliation":
        if (
            evidence["injected_outcome"] != "indeterminate"
            or type(evidence["transport_indeterminate_injections"]) is not int
            or evidence["transport_indeterminate_injections"] != 1
            or not isinstance(
                evidence["transport_last_audit_reason_sha256"], str
            )
            or not DIGEST_PATTERN.fullmatch(
                evidence["transport_last_audit_reason_sha256"]
            )
            or type(evidence["transport_last_upstream_status"]) is not int
            or not 200 <= evidence["transport_last_upstream_status"] <= 299
        ):
            fail("transport_reconciliation_evidence_invalid")
    elif checkpoint == "gateway-loss":
        if (
            evidence["gateway_disconnected"] is not True
            or type(evidence["runtime_ready_status"]) is not int
            or evidence["runtime_ready_status"] != 503
            or evidence["transport_gateway_partitioned"] is not True
            or type(evidence["transport_gateway_partition_events"]) is not int
            or evidence["transport_gateway_partition_events"] != 1
            or not isinstance(evidence["partition_operation_id"], str)
            or not TRANSPORT_OPERATION_ID_PATTERN.fullmatch(
                evidence["partition_operation_id"]
            )
            or not evidence["partition_operation_id"].endswith(
                ":partition-gateway"
            )
            or not isinstance(evidence["partition_completion_sha256"], str)
            or not DIGEST_PATTERN.fullmatch(
                evidence["partition_completion_sha256"]
            )
        ):
            fail("transport_gateway_loss_evidence_invalid")
    elif (
        evidence["gateway_connected"] is not True
        or type(evidence["runtime_ready_status"]) is not int
        or evidence["runtime_ready_status"] != 200
        or evidence["transport_gateway_partitioned"] is not False
        or type(evidence["transport_gateway_partition_events"]) is not int
        or evidence["transport_gateway_partition_events"] != 1
        or not isinstance(evidence["partition_operation_id"], str)
        or not TRANSPORT_OPERATION_ID_PATTERN.fullmatch(
            evidence["partition_operation_id"]
        )
        or not evidence["partition_operation_id"].endswith(
            ":partition-gateway"
        )
        or not isinstance(evidence["partition_completion_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(evidence["partition_completion_sha256"])
        or not isinstance(evidence["heal_operation_id"], str)
        or not TRANSPORT_OPERATION_ID_PATTERN.fullmatch(
            evidence["heal_operation_id"]
        )
        or not evidence["heal_operation_id"].endswith(":heal-gateway")
        or evidence["heal_operation_id"] == evidence["partition_operation_id"]
        or not isinstance(evidence["heal_completion_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(evidence["heal_completion_sha256"])
        or any(
            type(evidence[field]) is not bool or evidence[field]
            for field in (
                "transport_duplicate_armed",
                "transport_duplicate_claimed",
                "transport_indeterminate_armed",
                "transport_indeterminate_claimed",
            )
        )
    ):
        fail("transport_gateway_healed_evidence_invalid")
    return evidence


def interaction_transport_evidence(context, platform, snapshot):
    inventory = platform.transport_control(context, "resource_inventory")
    if (
        inventory["instance_id"] != snapshot["instance_id"]
        or inventory["deleted"] != []
        or inventory["active"] != inventory["created"]
    ):
        fail("transport_interaction_inventory_invalid")
    values = {
        "role_ids": sorted(
            resource["resource_id"]
            for resource in inventory["active"]
            if resource["kind"] == "role"
        ),
        "channel_ids": sorted(
            resource["resource_id"]
            for resource in inventory["active"]
            if resource["kind"] == "channel"
        ),
        "panel_message_ids": sorted(
            resource["resource_id"]
            for resource in inventory["active"]
            if resource["kind"] == "message"
        ),
    }
    return {
        "schema_version": 1,
        "kind": TRANSPORT_EVIDENCE_KINDS["interaction"],
        "observed_at": utc_now(),
        **values,
        "inventory_digest_sha256": inventory["digest_sha256"],
        "transport_instance_id": inventory["instance_id"],
    }


def _require_transport_inventory_projection(evidence):
    digest = evidence.get("inventory_digest_sha256")
    if not isinstance(digest, str) or not DIGEST_PATTERN.fullmatch(digest):
        fail("transport_inventory_projection_invalid")
    for field in ("role_ids", "channel_ids", "panel_message_ids"):
        values = evidence.get(field)
        if (
            not isinstance(values, list)
            or not values
            or values != sorted(values)
            or len(values) != len(set(values))
        ):
            fail("transport_inventory_projection_invalid")
        for value in values:
            validate_snowflake(value, f"transport_{field}")
    resource_ids = (
        evidence["role_ids"]
        + evidence["channel_ids"]
        + evidence["panel_message_ids"]
    )
    if len(resource_ids) != len(set(resource_ids)):
        fail("transport_inventory_projection_invalid")


def duplicate_transport_evidence(context, platform, snapshot):
    gateway = snapshot["gateway"]
    interaction_id = gateway["last_duplicate_interaction_id"]
    inventory = platform.transport_control(context, "resource_inventory")
    if (
        inventory["instance_id"] != snapshot["instance_id"]
        or inventory["deleted"] != []
        or inventory["active"] != inventory["created"]
    ):
        fail("transport_duplicate_inventory_invalid")
    return {
        "schema_version": 1,
        "kind": TRANSPORT_EVIDENCE_KINDS["duplicate"],
        "observed_at": utc_now(),
        "interaction_id": interaction_id,
        "delivery_count": gateway["duplicate_delivery_count"],
        "transport_duplicate_injections": gateway["duplicate_injections"],
        "transport_duplicate_delivery_count": gateway[
            "duplicate_delivery_count"
        ],
        "transport_last_duplicate_interaction_id": interaction_id,
        "role_ids": sorted(
            resource["resource_id"]
            for resource in inventory["active"]
            if resource["kind"] == "role"
        ),
        "channel_ids": sorted(
            resource["resource_id"]
            for resource in inventory["active"]
            if resource["kind"] == "channel"
        ),
        "panel_message_ids": sorted(
            resource["resource_id"]
            for resource in inventory["active"]
            if resource["kind"] == "message"
        ),
        "inventory_digest_sha256": inventory["digest_sha256"],
        "transport_instance_id": snapshot["instance_id"],
    }


def reconciliation_transport_evidence(snapshot):
    effect = snapshot["effect_http"]
    return {
        "schema_version": 1,
        "kind": TRANSPORT_EVIDENCE_KINDS["reconciliation"],
        "observed_at": utc_now(),
        "injected_outcome": "indeterminate",
        "transport_indeterminate_injections": effect["indeterminate_injections"],
        "transport_last_audit_reason_sha256": effect[
            "last_indeterminate_audit_reason_sha256"
        ],
        "transport_last_upstream_status": effect[
            "last_indeterminate_upstream_status"
        ],
        "transport_instance_id": snapshot["instance_id"],
    }


def gateway_loss_transport_evidence(snapshot, runtime_status, partition_binding):
    gateway = snapshot["gateway"]
    return {
        "schema_version": 1,
        "kind": TRANSPORT_EVIDENCE_KINDS["gateway-loss"],
        "observed_at": utc_now(),
        "gateway_disconnected": gateway["partitioned"],
        "runtime_ready_status": runtime_status,
        "transport_gateway_partitioned": gateway["partitioned"],
        "transport_gateway_partition_events": gateway["partition_events"],
        "transport_instance_id": snapshot["instance_id"],
        "partition_operation_id": partition_binding["operation_id"],
        "partition_completion_sha256": partition_binding[
            "completion_sha256"
        ],
    }


def gateway_healed_transport_evidence(
    snapshot, runtime_status, partition_binding, heal_binding
):
    gateway = snapshot["gateway"]
    effect = snapshot["effect_http"]
    return {
        "schema_version": 1,
        "kind": TRANSPORT_EVIDENCE_KINDS["gateway-healed"],
        "observed_at": utc_now(),
        "gateway_connected": not gateway["partitioned"],
        "runtime_ready_status": runtime_status,
        "transport_gateway_partitioned": gateway["partitioned"],
        "transport_gateway_partition_events": gateway["partition_events"],
        "transport_duplicate_armed": gateway["duplicate_armed"],
        "transport_duplicate_claimed": gateway["duplicate_claimed"],
        "transport_indeterminate_armed": effect["indeterminate_armed"],
        "transport_indeterminate_claimed": effect["indeterminate_claimed"],
        "transport_instance_id": snapshot["instance_id"],
        "partition_operation_id": partition_binding["operation_id"],
        "partition_completion_sha256": partition_binding[
            "completion_sha256"
        ],
        "heal_operation_id": heal_binding["operation_id"],
        "heal_completion_sha256": heal_binding["completion_sha256"],
    }


def command_transport_evidence(context, platform, checkpoint):
    require_candidate_start_not_retired(context)
    if checkpoint not in TRANSPORT_EVIDENCE_KINDS:
        fail("transport_evidence_checkpoint_invalid")
    if checkpoint == "gateway-loss":
        snapshot, runtime_status = require_gateway_loss_certification_boundary(
            context, platform
        )
        bindings = gateway_control_completion_bindings(
            context, ("partition-gateway",)
        )
        partition_binding = bindings[0]
        if (
            partition_binding["snapshot"]["instance_id"]
            != snapshot["instance_id"]
            or partition_binding["snapshot"]["gateway"]["partitioned"]
            is not True
            or partition_binding["snapshot"]["gateway"]["partition_events"]
            != 1
        ):
            fail("transport_gateway_partition_binding_invalid")
        current = gateway_loss_transport_evidence(
            snapshot, runtime_status, partition_binding
        )
    elif checkpoint == "gateway-healed":
        snapshot, runtime_status = require_gateway_healed_certification_boundary(
            context, platform
        )
        loss_path = transport_evidence_path(context, "gateway-loss")
        if not loss_path.exists():
            fail("transport_gateway_loss_evidence_missing")
        loss = load_private_json(loss_path, "transport_evidence_gateway_loss")
        validate_transport_evidence_payload(context, "gateway-loss", loss)
        bindings = gateway_control_completion_bindings(
            context, ("partition-gateway", "heal-gateway")
        )
        partition_binding, heal_binding = bindings
        if (
            loss["partition_operation_id"] != partition_binding["operation_id"]
            or loss["partition_completion_sha256"]
            != partition_binding["completion_sha256"]
            or partition_binding["snapshot"]["instance_id"]
            != snapshot["instance_id"]
            or partition_binding["snapshot"]["gateway"]["partitioned"]
            is not True
            or partition_binding["snapshot"]["gateway"]["partition_events"]
            != 1
            or heal_binding["snapshot"]["instance_id"] != snapshot["instance_id"]
            or heal_binding["snapshot"]["gateway"]["partitioned"] is not False
            or heal_binding["snapshot"]["gateway"]["partition_events"] != 1
        ):
            fail("transport_gateway_heal_binding_invalid")
        current = gateway_healed_transport_evidence(
            snapshot,
            runtime_status,
            partition_binding,
            heal_binding,
        )
    else:
        _state, snapshot = require_candidate_certification_boundary(
            context, platform
        )
        if checkpoint == "interaction":
            current = interaction_transport_evidence(context, platform, snapshot)
        elif checkpoint == "duplicate":
            current = duplicate_transport_evidence(context, platform, snapshot)
        else:
            current = reconciliation_transport_evidence(snapshot)
    validate_transport_evidence_payload(context, checkpoint, current)
    path = transport_evidence_path(context, checkpoint)
    if path.exists():
        recorded = load_private_json(path, f"transport_evidence_{checkpoint}")
        validate_transport_evidence_payload(context, checkpoint, recorded)
        current_semantics = {
            key: value for key, value in current.items() if key != "observed_at"
        }
        recorded_semantics = {
            key: value for key, value in recorded.items() if key != "observed_at"
        }
        if current_semantics != recorded_semantics:
            fail("transport_evidence_replay_drift")
        status = "exact_replay"
        evidence = recorded
    else:
        append_journal(context, "transport_evidence", "intent", checkpoint)
        write_atomic(path, canonical_json(current) + "\n")
        evidence = load_private_json(path, f"transport_evidence_{checkpoint}")
        validate_transport_evidence_payload(context, checkpoint, evidence)
        append_journal(context, "transport_evidence", "complete", checkpoint)
        status = "recorded"
    return {
        "status": status,
        "phase": "candidate_started",
        "checkpoint": checkpoint,
        "kind": evidence["kind"],
        "transport_instance_id": evidence["transport_instance_id"],
        "evidence": str(path),
    }


def command_worker_authoring_evidence(
    context, platform, checkpoint, browser_evidence_path=None
):
    if checkpoint == "before" and browser_evidence_path is not None:
        fail("worker_browser_evidence_unexpected")
    if checkpoint == "after" and browser_evidence_path is None:
        fail("worker_browser_evidence_required")
    require_candidate_certification_boundary(context, platform)
    browser = None
    if browser_evidence_path is not None:
        path = require_absolute_path(
            browser_evidence_path, "worker_browser_evidence"
        )
        browser = load_private_json(path, "worker_browser_evidence")
    health = platform.worker_health_snapshot(context)
    return capture_worker_authoring_checkpoint(
        context, health, checkpoint, browser
    )


def reconciliation_discord_observation_path(context):
    return (
        context.artifact_directory
        / "discord-evidence"
        / "reconciliation-role.json"
    )


def validate_reconciliation_database_source(value):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "effect_identity",
        "interaction_id",
        "route_identity",
        "reconciliation_state",
        "duplicate_external_effect_count",
        "unsafe_deletion_count",
        "output_role_id",
    }
    if (
        not isinstance(value, dict)
        or set(value) != fields
        or value["schema_version"] != 1
        or value["kind"] != "starring.d2.db-reconciliation-evidence.v1"
        or not isinstance(value["observed_at"], str)
        or not EVIDENCE_RECORDED_AT_PATTERN.fullmatch(value["observed_at"])
        or value["reconciliation_state"] != "known_success"
        or value["duplicate_external_effect_count"] != 0
        or value["unsafe_deletion_count"] != 0
        or type(value["duplicate_external_effect_count"]) is not int
        or type(value["unsafe_deletion_count"]) is not int
        or not isinstance(value["effect_identity"], dict)
        or not isinstance(value["route_identity"], dict)
    ):
        fail("reconciliation_database_evidence_invalid")
    validate_snowflake(value["interaction_id"], "reconciliation_interaction_id")
    validate_snowflake(value["output_role_id"], "reconciliation_output_role_id")
    if value["effect_identity"].get("interaction_id") != value["interaction_id"]:
        fail("reconciliation_database_evidence_invalid")
    return value


def validate_reconciliation_discord_observation(context, value, inventory, role_id):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "transport_instance_id",
        "inventory_digest_sha256",
        "resource_kind",
        "resource_id",
        "channel_id",
        "http_status",
        "discord_code",
        "exists",
    }
    if (
        not isinstance(value, dict)
        or set(value) != fields
        or value["schema_version"] != 1
        or value["kind"] != RECONCILIATION_DISCORD_OBSERVATION_KIND
        or not isinstance(value["observed_at"], str)
        or not EVIDENCE_RECORDED_AT_PATTERN.fullmatch(value["observed_at"])
        or value["transport_instance_id"] != inventory["instance_id"]
        or value["inventory_digest_sha256"] != inventory["digest_sha256"]
        or value["resource_kind"] != "role"
        or value["resource_id"] != role_id
        or value["channel_id"] is not None
        or value["http_status"] != 200
        or value["discord_code"] is not None
        or value["exists"] is not True
    ):
        fail("reconciliation_discord_observation_invalid")
    require_pinned_transport_snapshot(
        context, {"instance_id": value["transport_instance_id"]}
    )
    return value


def current_reconciliation_discord_observation(
    context, platform, database, inventory
):
    role_id = database["output_role_id"]
    resource = {"kind": "role", "resource_id": role_id}
    if resource not in inventory["active"]:
        fail("reconciliation_output_role_not_active")
    observed = platform.discord_observe_resource(context, resource, inventory)
    if (
        not isinstance(observed, dict)
        or set(observed)
        != {
            "schema_version",
            "kind",
            "transport_instance_id",
            "inventory_digest_sha256",
            "resource_kind",
            "resource_id",
            "channel_id",
            "http_status",
            "discord_code",
            "exists",
        }
        or observed["schema_version"] != 1
        or observed["kind"]
        != "starring.d2.discord-resource-observation.v1"
    ):
        fail("reconciliation_discord_observation_invalid")
    evidence = {
        **observed,
        "kind": RECONCILIATION_DISCORD_OBSERVATION_KIND,
        "observed_at": utc_now(),
    }
    return validate_reconciliation_discord_observation(
        context, evidence, inventory, role_id
    )


def command_reconciliation_discord_observation(
    context, platform, database_evidence_path
):
    _state, snapshot = require_candidate_certification_boundary(context, platform)
    database_path = require_absolute_path(
        database_evidence_path, "reconciliation_database_evidence"
    )
    database = validate_reconciliation_database_source(
        load_private_json(database_path, "reconciliation_database_evidence")
    )
    inventory = platform.transport_control(context, "resource_inventory")
    if inventory["instance_id"] != snapshot["instance_id"]:
        fail("transport_instance_changed")
    current = current_reconciliation_discord_observation(
        context, platform, database, inventory
    )
    path = reconciliation_discord_observation_path(context)
    if path.exists():
        recorded = validate_reconciliation_discord_observation(
            context,
            load_private_json(path, "reconciliation_discord_observation"),
            inventory,
            database["output_role_id"],
        )
        if {
            key: value for key, value in current.items() if key != "observed_at"
        } != {
            key: value for key, value in recorded.items() if key != "observed_at"
        }:
            fail("reconciliation_discord_observation_replay_drift")
        status = "exact_replay"
    else:
        append_journal(
            context, "reconciliation_discord_observation", "intent", "role"
        )
        write_atomic(path, canonical_json(current) + "\n")
        recorded = validate_reconciliation_discord_observation(
            context,
            load_private_json(path, "reconciliation_discord_observation"),
            inventory,
            database["output_role_id"],
        )
        append_journal(
            context, "reconciliation_discord_observation", "complete", "role"
        )
        status = "recorded"
    return {
        "status": status,
        "phase": "candidate_started",
        "kind": recorded["kind"],
        "transport_instance_id": recorded["transport_instance_id"],
        "resource_id": recorded["resource_id"],
        "evidence": str(path),
    }


def discord_resource_identity_key(resource):
    return (
        resource["kind"],
        resource["resource_id"],
        resource.get("channel_id"),
    )


def discord_resource_teardown_key(resource):
    return (
        DISCORD_RESOURCE_KIND_ORDER[resource["kind"]],
        resource.get("channel_id", ""),
        resource["resource_id"],
    )


def discord_resource_union_sha256(resources):
    return hashlib.sha256(canonical_json(resources).encode("utf-8")).hexdigest()


def discord_teardown_progress_path(context, frozen=False):
    if frozen:
        return context.artifact_directory / "discord-resource-teardown-progress.json"
    return abort_teardown_progress_path(context)


def discord_teardown_evidence_path(context, frozen=False):
    if frozen:
        return context.artifact_directory / "discord-resource-teardown-evidence.json"
    return abort_teardown_evidence_path(context)


def load_private_json(path, label):
    require_owned_mode(path, 0o600, label)
    return load_json_file(path, label)


def validate_abort_teardown_tombstone(context, value, inventory):
    if (
        not isinstance(value, dict)
        or set(value)
        != {
            "schema_version",
            "kind",
            "manifest_sha256",
            "run_id",
            "recorded_at",
            "transport_instance_id",
            "source_inventory_digest_sha256",
            "certification_permanently_disqualified",
        }
        or value["schema_version"] != 1
        or value["kind"] != DISCORD_TEARDOWN_ABORT_KIND
        or value["manifest_sha256"] != context.digest
        or value["run_id"] != context.manifest["run_id"]
        or not isinstance(value["recorded_at"], str)
        or not TRANSPORT_RECORDED_AT_PATTERN.fullmatch(value["recorded_at"])
        or value["transport_instance_id"] != inventory["instance_id"]
        or not isinstance(value["source_inventory_digest_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(value["source_inventory_digest_sha256"])
        or value["certification_permanently_disqualified"] is not True
    ):
        fail("discord_resource_teardown_abort_invalid")
    return value


def ensure_abort_teardown_tombstone(context, inventory):
    path = abort_teardown_tombstone_path(context)
    if path.exists():
        return validate_abort_teardown_tombstone(
            context,
            load_private_json(path, "discord_resource_teardown_abort"),
            inventory,
        )
    value = {
        "schema_version": 1,
        "kind": DISCORD_TEARDOWN_ABORT_KIND,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "recorded_at": utc_now(),
        "transport_instance_id": inventory["instance_id"],
        "source_inventory_digest_sha256": inventory["digest_sha256"],
        "certification_permanently_disqualified": True,
    }
    validate_abort_teardown_tombstone(context, value, inventory)
    write_atomic(path, canonical_json(value) + "\n")
    return value


def discord_teardown_record(resource, disposition, http_status=None, discord_code=None):
    return {
        "resource_kind": resource["kind"],
        "resource_id": resource["resource_id"],
        "channel_id": resource.get("channel_id"),
        "disposition": disposition,
        "http_status": http_status,
        "discord_code": discord_code,
    }


def discord_teardown_record_resource(record):
    resource = {
        "kind": record["resource_kind"],
        "resource_id": record["resource_id"],
    }
    if record["resource_kind"] == "message":
        resource["channel_id"] = record["channel_id"]
    return resource


def validate_discord_teardown_record(record, resources):
    if not isinstance(record, dict) or set(record) != {
        "resource_kind",
        "resource_id",
        "channel_id",
        "disposition",
        "http_status",
        "discord_code",
    }:
        fail("discord_resource_teardown_progress_invalid")
    kind = record["resource_kind"]
    if kind not in DISCORD_RESOURCE_KIND_ORDER:
        fail("discord_resource_teardown_progress_invalid")
    resource = discord_teardown_record_resource(record)
    if resource not in resources or (
        kind == "message" and not isinstance(record["channel_id"], str)
    ) or (kind != "message" and record["channel_id"] is not None):
        fail("discord_resource_teardown_progress_invalid")
    disposition = record["disposition"]
    if disposition in {"preexisting_deleted", "reconciled_deleted"}:
        if record["http_status"] is not None or record["discord_code"] is not None:
            fail("discord_resource_teardown_progress_invalid")
    elif disposition == "deleted":
        if (
            record["http_status"] != DISCORD_RESOURCE_SUCCESS_STATUS[kind]
            or record["discord_code"] is not None
        ):
            fail("discord_resource_teardown_progress_invalid")
    elif disposition == "already_absent":
        if (
            record["http_status"] != 404
            or record["discord_code"] not in DISCORD_RESOURCE_UNKNOWN_CODES[kind]
        ):
            fail("discord_resource_teardown_progress_invalid")
    else:
        fail("discord_resource_teardown_progress_invalid")
    return resource


def validate_discord_teardown_progress(context, progress, inventory):
    if not isinstance(progress, dict) or set(progress) != {
        "schema_version",
        "kind",
        "manifest_sha256",
        "run_id",
        "transport_instance_id",
        "source_inventory_digest_sha256",
        "resource_union_sha256",
        "created_resources",
        "deletions",
    }:
        fail("discord_resource_teardown_progress_invalid")
    resources = inventory["created"]
    if (
        progress["schema_version"] != 1
        or progress["kind"] != DISCORD_TEARDOWN_PROGRESS_KIND
        or progress["manifest_sha256"] != context.digest
        or progress["run_id"] != context.manifest["run_id"]
        or progress["transport_instance_id"] != inventory["instance_id"]
        or not isinstance(progress["source_inventory_digest_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(
            progress["source_inventory_digest_sha256"]
        )
        or progress["resource_union_sha256"]
        != discord_resource_union_sha256(resources)
        or progress["created_resources"] != resources
        or not isinstance(progress["deletions"], list)
    ):
        fail("discord_resource_teardown_progress_invalid")
    deleted = {
        discord_resource_identity_key(resource) for resource in inventory["deleted"]
    }
    observed = []
    for record in progress["deletions"]:
        resource = validate_discord_teardown_record(record, resources)
        key = discord_resource_identity_key(resource)
        if key not in deleted:
            fail("discord_resource_teardown_progress_mismatch")
        observed.append(key)
    expected_order = [
        discord_resource_identity_key(resource)
        for resource in sorted(
            (discord_teardown_record_resource(record) for record in progress["deletions"]),
            key=discord_resource_teardown_key,
        )
    ]
    if observed != expected_order or len(observed) != len(set(observed)):
        fail("discord_resource_teardown_progress_invalid")
    return progress


def new_discord_teardown_progress(context, inventory):
    deleted = {
        discord_resource_identity_key(resource) for resource in inventory["deleted"]
    }
    resources = inventory["created"]
    deletions = [
        discord_teardown_record(resource, "preexisting_deleted")
        for resource in sorted(resources, key=discord_resource_teardown_key)
        if discord_resource_identity_key(resource) in deleted
    ]
    return {
        "schema_version": 1,
        "kind": DISCORD_TEARDOWN_PROGRESS_KIND,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "transport_instance_id": inventory["instance_id"],
        "source_inventory_digest_sha256": inventory["digest_sha256"],
        "resource_union_sha256": discord_resource_union_sha256(resources),
        "created_resources": resources,
        "deletions": deletions,
    }


def write_discord_teardown_progress(context, progress, frozen=False):
    write_atomic(
        discord_teardown_progress_path(context, frozen), canonical_json(progress) + "\n"
    )


def reconcile_discord_teardown_progress(context, progress, inventory, frozen=False):
    completed = {
        discord_resource_identity_key(discord_teardown_record_resource(record))
        for record in progress["deletions"]
    }
    added = False
    for resource in sorted(inventory["deleted"], key=discord_resource_teardown_key):
        key = discord_resource_identity_key(resource)
        if key not in completed:
            progress["deletions"].append(
                discord_teardown_record(resource, "reconciled_deleted")
            )
            completed.add(key)
            added = True
    if added:
        progress["deletions"].sort(
            key=lambda record: discord_resource_teardown_key(
                discord_teardown_record_resource(record)
            )
        )
        write_discord_teardown_progress(context, progress, frozen)
    return progress


def normalize_proxy_deletion(inventory, resource, evidence):
    if not isinstance(evidence, dict) or set(evidence) != {
        "schema_version",
        "kind",
        "transport_instance_id",
        "inventory_digest_sha256",
        "resource_kind",
        "resource_id",
        "channel_id",
        "http_status",
        "discord_code",
        "deleted",
    }:
        fail("discord_resource_proxy_evidence_invalid")
    expected_status = DISCORD_RESOURCE_SUCCESS_STATUS[resource["kind"]]
    if (
        evidence["schema_version"] != 1
        or evidence["kind"]
        != "starring.d2.discord-resource-proxy-deletion.v1"
        or evidence["transport_instance_id"] != inventory["instance_id"]
        or evidence["inventory_digest_sha256"] != inventory["digest_sha256"]
        or evidence["resource_kind"] != resource["kind"]
        or evidence["resource_id"] != resource["resource_id"]
        or evidence["channel_id"] != resource.get("channel_id")
        or type(evidence["deleted"]) is not bool
    ):
        fail("discord_resource_proxy_evidence_invalid")
    if evidence["http_status"] == expected_status:
        if evidence["discord_code"] is not None or evidence["deleted"] is not True:
            fail("discord_resource_proxy_evidence_invalid")
        disposition = "deleted"
    elif evidence["http_status"] == 404:
        if (
            evidence["discord_code"]
            not in DISCORD_RESOURCE_UNKNOWN_CODES[resource["kind"]]
            or evidence["deleted"] is not False
        ):
            fail("discord_resource_proxy_evidence_invalid")
        disposition = "already_absent"
    else:
        fail("discord_resource_proxy_evidence_invalid")
    return discord_teardown_record(
        resource,
        disposition,
        evidence["http_status"],
        evidence["discord_code"],
    )


def normalize_direct_observation(inventory, resource, evidence):
    if not isinstance(evidence, dict) or set(evidence) != {
        "schema_version",
        "kind",
        "transport_instance_id",
        "inventory_digest_sha256",
        "resource_kind",
        "resource_id",
        "channel_id",
        "http_status",
        "discord_code",
        "exists",
    }:
        fail("discord_resource_observation_evidence_invalid")
    kind = resource["kind"]
    absent_status = (
        evidence["http_status"] == 200
        and kind == "role"
        and evidence["discord_code"] is None
    ) or (
        evidence["http_status"] == 404
        and evidence["discord_code"] in DISCORD_RESOURCE_UNKNOWN_CODES[kind]
    )
    if (
        evidence["schema_version"] != 1
        or evidence["kind"] != "starring.d2.discord-resource-observation.v1"
        or evidence["transport_instance_id"] != inventory["instance_id"]
        or evidence["inventory_digest_sha256"] != inventory["digest_sha256"]
        or evidence["resource_kind"] != kind
        or evidence["resource_id"] != resource["resource_id"]
        or evidence["channel_id"] != resource.get("channel_id")
        or evidence["exists"] is not False
        or not absent_status
    ):
        fail("discord_resource_absence_unconfirmed")
    return {
        "resource_kind": kind,
        "resource_id": resource["resource_id"],
        "channel_id": resource.get("channel_id"),
        "http_status": evidence["http_status"],
        "discord_code": evidence["discord_code"],
        "exists": False,
    }


def observe_absent_discord_resources(context, platform, inventory):
    observations = []
    for resource in sorted(inventory["created"], key=discord_resource_teardown_key):
        evidence = platform.discord_observe_resource(context, resource, inventory)
        observations.append(
            normalize_direct_observation(inventory, resource, evidence)
        )
    return observations


def discord_resource_id_lists(resources):
    return {
        "resource_ids": sorted(resource["resource_id"] for resource in resources),
        "message_ids": sorted(
            resource["resource_id"]
            for resource in resources
            if resource["kind"] == "message"
        ),
        "channel_ids": sorted(
            resource["resource_id"]
            for resource in resources
            if resource["kind"] == "channel"
        ),
        "role_ids": sorted(
            resource["resource_id"]
            for resource in resources
            if resource["kind"] == "role"
        ),
    }


def validate_discord_teardown_evidence(
    context, evidence, inventory, certification_binding=None
):
    required = {
        "schema_version",
        "kind",
        "manifest_sha256",
        "run_id",
        "recorded_at",
        "transport_instance_id",
        "source_inventory_digest_sha256",
        "final_inventory_digest_sha256",
        "resource_union_sha256",
        "created_resources",
        "deleted_resources",
        "active_resources",
        "resource_ids",
        "message_ids",
        "channel_ids",
        "role_ids",
        "proxy_deletions",
        "direct_observations",
        "all_resources_absent",
    }
    if certification_binding is not None:
        required.update(certification_binding)
    resources = inventory["created"]
    identifiers = discord_resource_id_lists(resources)
    if (
        not isinstance(evidence, dict)
        or set(evidence) != required
        or evidence["schema_version"] != 1
        or evidence["kind"] != DISCORD_TEARDOWN_EVIDENCE_KIND
        or evidence["manifest_sha256"] != context.digest
        or evidence["run_id"] != context.manifest["run_id"]
        or not isinstance(evidence["recorded_at"], str)
        or not TRANSPORT_RECORDED_AT_PATTERN.fullmatch(evidence["recorded_at"])
        or evidence["transport_instance_id"] != inventory["instance_id"]
        or not isinstance(evidence["source_inventory_digest_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(
            evidence["source_inventory_digest_sha256"]
        )
        or evidence["final_inventory_digest_sha256"]
        != inventory["digest_sha256"]
        or evidence["resource_union_sha256"]
        != discord_resource_union_sha256(resources)
        or evidence["created_resources"] != resources
        or evidence["deleted_resources"] != resources
        or evidence["active_resources"] != []
        or any(evidence[name] != value for name, value in identifiers.items())
        or evidence["all_resources_absent"] is not True
        or not isinstance(evidence["proxy_deletions"], list)
        or not isinstance(evidence["direct_observations"], list)
        or inventory["deleted"] != resources
        or inventory["active"] != []
    ):
        fail("discord_resource_teardown_evidence_invalid")
    if certification_binding is not None:
        if any(
            evidence[field] != value
            for field, value in certification_binding.items()
        ) or evidence["source_inventory_digest_sha256"] != (
            certification_binding["freeze_resource_inventory_digest_sha256"]
        ):
            fail("discord_resource_teardown_evidence_invalid")
    progress_view = {
        "schema_version": 1,
        "kind": DISCORD_TEARDOWN_PROGRESS_KIND,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "transport_instance_id": inventory["instance_id"],
        "source_inventory_digest_sha256": evidence[
            "source_inventory_digest_sha256"
        ],
        "resource_union_sha256": evidence["resource_union_sha256"],
        "created_resources": resources,
        "deletions": evidence["proxy_deletions"],
    }
    validate_discord_teardown_progress(context, progress_view, inventory)
    expected_resources = sorted(resources, key=discord_resource_teardown_key)
    observations = evidence["direct_observations"]
    if len(observations) != len(expected_resources):
        fail("discord_resource_teardown_evidence_invalid")
    for resource, observation in zip(expected_resources, observations):
        normalized = {
            "schema_version": 1,
            "kind": "starring.d2.discord-resource-observation.v1",
            "transport_instance_id": inventory["instance_id"],
            "inventory_digest_sha256": inventory["digest_sha256"],
            **observation,
        }
        normalize_direct_observation(inventory, resource, normalized)
    return evidence


def command_teardown_discord_resources(context, platform, frozen=False):
    if frozen:
        require_certification_eligible_teardown(context)
        boundary = require_frozen_discord_teardown_boundary
    else:
        def boundary(boundary_context, boundary_platform):
            return require_candidate_certification_boundary(
                boundary_context,
                boundary_platform,
                allow_abort_teardown=True,
            )
    _state, snapshot = boundary(context, platform)
    inventory = platform.transport_control(context, "resource_inventory")
    if inventory["instance_id"] != snapshot["instance_id"]:
        fail("transport_instance_changed")
    certification_binding = certified_teardown_binding(context) if frozen else None
    if not frozen:
        ensure_abort_teardown_tombstone(context, inventory)
    evidence_path = discord_teardown_evidence_path(context, frozen)
    if evidence_path.exists():
        evidence = load_private_json(
            evidence_path, "discord_resource_teardown_evidence"
        )
        validate_discord_teardown_evidence(
            context, evidence, inventory, certification_binding
        )
        observe_absent_discord_resources(context, platform, inventory)
        final_inventory = platform.transport_control(context, "resource_inventory")
        if final_inventory["digest_sha256"] != inventory["digest_sha256"]:
            fail("discord_resource_teardown_replay_drift")
        boundary(context, platform)
        return {
            "status": "exact_replay",
            "phase": "candidate_started",
            "transport_instance_id": inventory["instance_id"],
            "inventory_digest_sha256": inventory["digest_sha256"],
            "resource_count": len(inventory["created"]),
            "all_resources_absent": True,
            "evidence": str(evidence_path),
        }
    progress_path = discord_teardown_progress_path(context, frozen)
    if progress_path.exists():
        progress = load_private_json(
            progress_path, "discord_resource_teardown_progress"
        )
        validate_discord_teardown_progress(context, progress, inventory)
        if certification_binding is not None and progress[
            "source_inventory_digest_sha256"
        ] != certification_binding["freeze_resource_inventory_digest_sha256"]:
            fail("discord_resource_teardown_progress_invalid")
    else:
        if certification_binding is not None and inventory["digest_sha256"] != (
            certification_binding["freeze_resource_inventory_digest_sha256"]
        ):
            fail("discord_teardown_live_inventory_drift")
        progress = new_discord_teardown_progress(context, inventory)
        append_journal(context, "discord_resource_teardown", "intent", "resources")
        write_discord_teardown_progress(context, progress, frozen)
    progress = reconcile_discord_teardown_progress(
        context, progress, inventory, frozen
    )
    completed = {
        discord_resource_identity_key(discord_teardown_record_resource(record))
        for record in progress["deletions"]
    }
    for resource in sorted(inventory["created"], key=discord_resource_teardown_key):
        key = discord_resource_identity_key(resource)
        if key in completed:
            continue
        current = platform.transport_control(context, "resource_inventory")
        if (
            current["instance_id"] != inventory["instance_id"]
            or current["created"] != inventory["created"]
        ):
            fail("discord_resource_teardown_inventory_drift")
        if resource not in current["active"]:
            progress["deletions"].append(
                discord_teardown_record(resource, "reconciled_deleted")
            )
        else:
            deletion = platform.discord_delete_resource_through_transport(
                context, resource, current
            )
            record = normalize_proxy_deletion(current, resource, deletion)
            refreshed = platform.transport_control(context, "resource_inventory")
            if (
                refreshed["instance_id"] != inventory["instance_id"]
                or refreshed["created"] != inventory["created"]
                or resource in refreshed["active"]
                or resource not in refreshed["deleted"]
            ):
                fail("discord_resource_lifecycle_not_deleted")
            progress["deletions"].append(record)
        progress["deletions"].sort(
            key=lambda value: discord_resource_teardown_key(
                discord_teardown_record_resource(value)
            )
        )
        write_discord_teardown_progress(context, progress, frozen)
        completed.add(key)
    final_inventory = platform.transport_control(context, "resource_inventory")
    if (
        final_inventory["instance_id"] != inventory["instance_id"]
        or final_inventory["created"] != inventory["created"]
        or final_inventory["deleted"] != inventory["created"]
        or final_inventory["active"] != []
    ):
        fail("discord_resource_teardown_incomplete")
    validate_discord_teardown_progress(context, progress, final_inventory)
    if len(progress["deletions"]) != len(final_inventory["created"]):
        fail("discord_resource_teardown_incomplete")
    observations = observe_absent_discord_resources(
        context, platform, final_inventory
    )
    _state, final_snapshot = boundary(context, platform)
    confirmed_inventory = platform.transport_control(context, "resource_inventory")
    if (
        final_snapshot["instance_id"] != final_inventory["instance_id"]
        or confirmed_inventory["digest_sha256"]
        != final_inventory["digest_sha256"]
    ):
        fail("discord_resource_teardown_final_drift")
    evidence = {
        "schema_version": 1,
        "kind": DISCORD_TEARDOWN_EVIDENCE_KIND,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "recorded_at": utc_now(),
        "transport_instance_id": final_inventory["instance_id"],
        "source_inventory_digest_sha256": progress[
            "source_inventory_digest_sha256"
        ],
        "final_inventory_digest_sha256": final_inventory["digest_sha256"],
        "resource_union_sha256": progress["resource_union_sha256"],
        "created_resources": final_inventory["created"],
        "deleted_resources": final_inventory["deleted"],
        "active_resources": final_inventory["active"],
        **discord_resource_id_lists(final_inventory["created"]),
        "proxy_deletions": progress["deletions"],
        "direct_observations": observations,
        "all_resources_absent": True,
        **(certification_binding or {}),
    }
    validate_discord_teardown_evidence(
        context, evidence, final_inventory, certification_binding
    )
    write_atomic(evidence_path, canonical_json(evidence) + "\n")
    append_journal(context, "discord_resource_teardown", "complete", "resources")
    return {
        "status": "torn_down",
        "phase": "candidate_started",
        "transport_instance_id": final_inventory["instance_id"],
        "inventory_digest_sha256": final_inventory["digest_sha256"],
        "resource_count": len(final_inventory["created"]),
        "all_resources_absent": True,
        "evidence": str(evidence_path),
    }


def cleanup_root_quarantine_name(context):
    return f".{context.root.name}.cleanup-{context.digest[:16]}"


def cleanup_root_quarantine_path(context):
    return context.root.parent / cleanup_root_quarantine_name(context)


def cleanup_root_progress_path(context):
    return context.artifact_directory / "cleanup-root-progress.json"


def cleanup_root_identity_path(context):
    return context.artifact_directory / "cleanup-root-identity.json"


def cleanup_path_metadata(path, code):
    try:
        return path.lstat()
    except FileNotFoundError:
        return None
    except OSError:
        fail(code)


def validate_cleanup_root_directory(context, root):
    expected = isolated_runtime_root(context.manifest["run_id"])
    if context.root != expected or context.root.parent != pathlib.Path("/private/tmp"):
        fail("cleanup_root_guard_failed")
    metadata = cleanup_path_metadata(root, "cleanup_root_invalid")
    if metadata is None:
        return None
    parent = cleanup_path_metadata(root.parent, "cleanup_root_invalid")
    if parent is None:
        fail("cleanup_root_invalid")
    if (
        not stat.S_ISDIR(parent.st_mode)
        or root.parent.is_symlink()
        or not stat.S_ISDIR(metadata.st_mode)
        or root.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
        or metadata.st_dev != parent.st_dev
    ):
        fail("cleanup_root_invalid")
    cluster_root = root / "postgres"
    cluster = cleanup_path_metadata(cluster_root, "cleanup_cluster_invalid")
    if cluster is not None and (
        not stat.S_ISDIR(cluster.st_mode)
        or cluster_root.is_symlink()
        or cluster.st_uid != os.getuid()
        or stat.S_IMODE(cluster.st_mode) != 0o700
        or cluster.st_dev != metadata.st_dev
    ):
        fail("cleanup_cluster_invalid")
    try:
        for directory, names, files in os.walk(root, followlinks=False):
            for name in names + files:
                item = (pathlib.Path(directory) / name).lstat()
                if item.st_dev != metadata.st_dev:
                    fail("cleanup_mount_boundary_invalid")
    except OSError:
        fail("cleanup_root_invalid")
    return metadata


def validate_cleanup_root_identity(context, identity):
    if (
        not isinstance(identity, dict)
        or set(identity)
        != {
            "schema_version",
            "kind",
            "manifest_sha256",
            "run_id",
            "root_path",
            "root_device",
            "root_inode",
            "parent_device",
            "owner_uid",
        }
        or identity.get("schema_version") != 1
        or identity.get("kind") != CLEANUP_ROOT_IDENTITY_KIND
        or identity.get("manifest_sha256") != context.digest
        or identity.get("run_id") != context.manifest["run_id"]
        or identity.get("root_path") != str(context.root)
        or type(identity.get("root_device")) is not int
        or identity["root_device"] < 0
        or type(identity.get("root_inode")) is not int
        or identity["root_inode"] <= 0
        or type(identity.get("parent_device")) is not int
        or identity["parent_device"] < 0
        or identity["root_device"] != identity["parent_device"]
        or identity.get("owner_uid") != os.getuid()
    ):
        fail("cleanup_root_identity_invalid")
    return identity


def load_cleanup_root_identity(context):
    path = cleanup_root_identity_path(context)
    metadata = cleanup_path_metadata(path, "cleanup_root_identity_invalid")
    if metadata is None:
        return None
    require_owned_mode(path, 0o600, "cleanup_root_identity")
    return validate_cleanup_root_identity(
        context, load_json(path, "cleanup_root_identity_invalid")
    )


def record_cleanup_root_identity(context):
    if cleanup_path_metadata(
        cleanup_root_identity_path(context), "cleanup_root_identity_invalid"
    ) is not None:
        fail("cleanup_root_identity_busy")
    metadata = validate_cleanup_root_directory(context, context.root)
    parent = cleanup_path_metadata(context.root.parent, "cleanup_root_invalid")
    if metadata is None or parent is None:
        fail("cleanup_root_identity_invalid")
    identity = {
        "schema_version": 1,
        "kind": CLEANUP_ROOT_IDENTITY_KIND,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "root_path": str(context.root),
        "root_device": metadata.st_dev,
        "root_inode": metadata.st_ino,
        "parent_device": parent.st_dev,
        "owner_uid": os.getuid(),
    }
    validate_cleanup_root_identity(context, identity)
    write_atomic(
        cleanup_root_identity_path(context), canonical_json(identity) + "\n"
    )
    return identity


def cleanup_root_identity_matches(metadata, identity):
    return (
        metadata is not None
        and stat.S_ISDIR(metadata.st_mode)
        and metadata.st_uid == identity["owner_uid"]
        and metadata.st_dev == identity["root_device"]
        and metadata.st_ino == identity["root_inode"]
    )


def load_cleanup_root_progress(context, identity=None):
    path = cleanup_root_progress_path(context)
    metadata = cleanup_path_metadata(path, "cleanup_root_progress_invalid")
    if metadata is None:
        return None
    if identity is None:
        identity = load_cleanup_root_identity(context)
    if identity is None:
        fail("cleanup_root_progress_invalid")
    require_owned_mode(path, 0o600, "cleanup_root_progress")
    progress = load_json(path, "cleanup_root_progress_invalid")
    if (
        not isinstance(progress, dict)
        or set(progress)
        != {
            "schema_version",
            "kind",
            "manifest_sha256",
            "run_id",
            "root_device",
            "root_inode",
            "quarantine_name",
            "phase",
        }
        or progress.get("schema_version") != 1
        or progress.get("kind") != CLEANUP_ROOT_PROGRESS_KIND
        or progress.get("manifest_sha256") != context.digest
        or progress.get("run_id") != context.manifest["run_id"]
        or type(progress.get("root_device")) is not int
        or progress["root_device"] < 0
        or type(progress.get("root_inode")) is not int
        or progress["root_inode"] <= 0
        or progress.get("quarantine_name") != cleanup_root_quarantine_name(context)
        or progress.get("phase") not in {"planned", "quarantined", "deleted"}
        or progress.get("root_device") != identity["root_device"]
        or progress.get("root_inode") != identity["root_inode"]
    ):
        fail("cleanup_root_progress_invalid")
    return progress


def save_cleanup_root_progress(context, progress, phase):
    updated = {**progress, "phase": phase}
    write_atomic(
        cleanup_root_progress_path(context), canonical_json(updated) + "\n"
    )
    return updated


def cleanup_root_metadata_matches(metadata, progress):
    return (
        metadata is not None
        and stat.S_ISDIR(metadata.st_mode)
        and metadata.st_uid == os.getuid()
        and metadata.st_dev == progress["root_device"]
        and metadata.st_ino == progress["root_inode"]
    )


def remove_cleanup_tree_contents(descriptor, expected_device):
    try:
        entries = list(os.scandir(descriptor))
    except OSError:
        fail("cleanup_root_delete_failed")
    for entry in entries:
        try:
            before = os.stat(
                entry.name, dir_fd=descriptor, follow_symlinks=False
            )
        except OSError:
            fail("cleanup_root_swap_detected")
        if before.st_dev != expected_device:
            fail("cleanup_mount_boundary_invalid")
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
                fail("cleanup_root_swap_detected")
            try:
                opened = os.fstat(child)
                if (
                    opened.st_dev != before.st_dev
                    or opened.st_ino != before.st_ino
                ):
                    fail("cleanup_root_swap_detected")
                remove_cleanup_tree_contents(child, expected_device)
            finally:
                os.close(child)
            try:
                after = os.stat(
                    entry.name, dir_fd=descriptor, follow_symlinks=False
                )
                if after.st_dev != before.st_dev or after.st_ino != before.st_ino:
                    fail("cleanup_root_swap_detected")
                os.rmdir(entry.name, dir_fd=descriptor)
            except OSError:
                fail("cleanup_root_swap_detected")
        else:
            try:
                after = os.stat(
                    entry.name, dir_fd=descriptor, follow_symlinks=False
                )
                if after.st_dev != before.st_dev or after.st_ino != before.st_ino:
                    fail("cleanup_root_swap_detected")
                os.unlink(entry.name, dir_fd=descriptor)
            except OSError:
                fail("cleanup_root_swap_detected")


def remove_cleanup_quarantine(context, progress):
    flags = (
        os.O_RDONLY
        | getattr(os, "O_DIRECTORY", 0)
        | getattr(os, "O_NOFOLLOW", 0)
    )
    try:
        parent = os.open(context.root.parent, flags)
    except OSError:
        fail("cleanup_root_invalid")
    try:
        try:
            before = os.stat(
                progress["quarantine_name"],
                dir_fd=parent,
                follow_symlinks=False,
            )
        except OSError:
            fail("cleanup_root_swap_detected")
        if not cleanup_root_metadata_matches(before, progress):
            fail("cleanup_root_swap_detected")
        try:
            root = os.open(
                progress["quarantine_name"], flags, dir_fd=parent
            )
        except OSError:
            fail("cleanup_root_swap_detected")
        try:
            opened = os.fstat(root)
            if not cleanup_root_metadata_matches(opened, progress):
                fail("cleanup_root_swap_detected")
            remove_cleanup_tree_contents(root, progress["root_device"])
        finally:
            os.close(root)
        try:
            after = os.stat(
                progress["quarantine_name"],
                dir_fd=parent,
                follow_symlinks=False,
            )
            if not cleanup_root_metadata_matches(after, progress):
                fail("cleanup_root_swap_detected")
            os.rmdir(progress["quarantine_name"], dir_fd=parent)
            os.fsync(parent)
        except OSError:
            fail("cleanup_root_swap_detected")
    finally:
        os.close(parent)


def require_quarantined_cleanup_substrate_inert(context, platform):
    if not cleanup_postgres_absent(context, platform):
        fail("cleanup_postgres_active_after_quarantine")
    if any(
        not platform.launchd_absent(service["label"])
        for service in context.manifest["services"].values()
    ):
        fail("cleanup_launchd_active_after_quarantine")


def guarded_remove_root(context, platform):
    expected = isolated_runtime_root(context.manifest["run_id"])
    if context.root != expected or context.root.parent != pathlib.Path("/private/tmp"):
        fail("cleanup_root_guard_failed")
    identity = load_cleanup_root_identity(context)
    root_metadata = cleanup_path_metadata(context.root, "cleanup_root_invalid")
    quarantined = cleanup_root_quarantine_path(context)
    quarantine_metadata = cleanup_path_metadata(quarantined, "cleanup_root_invalid")
    progress = load_cleanup_root_progress(context, identity)
    if root_metadata is not None and quarantine_metadata is not None:
        fail("cleanup_root_swap_detected")
    if (
        identity is None
        and (
            root_metadata is not None
            or quarantine_metadata is not None
            or progress is not None
        )
    ):
        fail("cleanup_root_identity_invalid")
    if identity is None:
        return
    if progress is not None and progress["phase"] == "deleted" and (
        root_metadata is not None or quarantine_metadata is not None
    ):
        fail("cleanup_root_swap_detected")
    if root_metadata is not None:
        validated = validate_cleanup_root_directory(context, context.root)
        if not cleanup_root_identity_matches(validated, identity):
            fail("cleanup_root_swap_detected")
        if progress is None:
            progress = {
                "schema_version": 1,
                "kind": CLEANUP_ROOT_PROGRESS_KIND,
                "manifest_sha256": context.digest,
                "run_id": context.manifest["run_id"],
                "root_device": identity["root_device"],
                "root_inode": identity["root_inode"],
                "quarantine_name": cleanup_root_quarantine_name(context),
                "phase": "planned",
            }
            write_atomic(
                cleanup_root_progress_path(context),
                canonical_json(progress) + "\n",
            )
        if not cleanup_root_metadata_matches(validated, progress):
            fail("cleanup_root_swap_detected")
        if progress["phase"] != "planned":
            fail("cleanup_root_swap_detected")
        flags = (
            os.O_RDONLY
            | getattr(os, "O_DIRECTORY", 0)
            | getattr(os, "O_NOFOLLOW", 0)
        )
        try:
            parent = os.open(context.root.parent, flags)
        except OSError:
            fail("cleanup_root_invalid")
        try:
            try:
                before = os.stat(
                    context.root.name, dir_fd=parent, follow_symlinks=False
                )
                if not cleanup_root_metadata_matches(before, progress):
                    fail("cleanup_root_swap_detected")
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
                if not cleanup_root_metadata_matches(after, progress):
                    fail("cleanup_root_swap_detected")
                os.fsync(parent)
            except OSError:
                fail("cleanup_root_swap_detected")
        finally:
            os.close(parent)
        require_quarantined_cleanup_substrate_inert(context, platform)
        progress = save_cleanup_root_progress(context, progress, "quarantined")
    elif quarantine_metadata is not None:
        if progress is None or not cleanup_root_metadata_matches(
            quarantine_metadata, progress
        ):
            fail("cleanup_root_swap_detected")
        if progress["phase"] not in {"planned", "quarantined"}:
            fail("cleanup_root_swap_detected")
        validate_cleanup_root_directory(context, quarantined)
        require_quarantined_cleanup_substrate_inert(context, platform)
        progress = save_cleanup_root_progress(context, progress, "quarantined")
    elif progress is not None:
        if progress["phase"] == "quarantined":
            save_cleanup_root_progress(context, progress, "deleted")
            return
        if progress["phase"] == "deleted":
            return
        fail("cleanup_root_loss_unproven")
    else:
        fail("cleanup_root_loss_unproven")
    remove_cleanup_quarantine(context, progress)
    save_cleanup_root_progress(context, progress, "deleted")


def validate_cleanup_mutation_roots(context):
    expected = isolated_runtime_root(context.manifest["run_id"])
    if context.root != expected or context.cluster_root != expected / "postgres":
        fail("cleanup_root_guard_failed")
    root_metadata = cleanup_path_metadata(context.root, "cleanup_root_invalid")
    quarantined = cleanup_root_quarantine_path(context)
    quarantine_metadata = cleanup_path_metadata(quarantined, "cleanup_root_invalid")
    identity = load_cleanup_root_identity(context)
    progress = load_cleanup_root_progress(context, identity)
    if root_metadata is not None and quarantine_metadata is not None:
        fail("cleanup_root_swap_detected")
    if (
        identity is None
        and (
            root_metadata is not None
            or quarantine_metadata is not None
            or progress is not None
        )
    ):
        fail("cleanup_root_identity_invalid")
    if root_metadata is not None:
        validated = validate_cleanup_root_directory(context, context.root)
        if not cleanup_root_identity_matches(validated, identity):
            fail("cleanup_root_swap_detected")
        if progress is not None and progress["phase"] != "planned":
            fail("cleanup_root_swap_detected")
    if quarantine_metadata is not None:
        if progress is None or not cleanup_root_metadata_matches(
            quarantine_metadata, progress
        ):
            fail("cleanup_root_swap_detected")
        if progress["phase"] not in {"planned", "quarantined"}:
            fail("cleanup_root_swap_detected")
        validate_cleanup_root_directory(context, quarantined)


def filesystem_entry_present(path, code):
    try:
        path.lstat()
    except FileNotFoundError:
        return False
    except OSError:
        fail(code)
    return True


def cleanup_postgres_absent(context, platform):
    original = context.cluster_root
    quarantined = cleanup_root_quarantine_path(context) / "postgres"
    original_present = filesystem_entry_present(
        original, "cleanup_cluster_invalid"
    )
    quarantine_present = filesystem_entry_present(
        quarantined, "cleanup_cluster_invalid"
    )
    if original_present and quarantine_present:
        fail("cleanup_root_swap_detected")
    if original_present:
        return platform.postgres_absent(original)
    if quarantine_present:
        return platform.postgres_absent(
            quarantined
        ) and platform.postgres_process_path_absent(original)
    return platform.postgres_process_path_absent(
        original
    ) and platform.postgres_process_path_absent(quarantined)


def cleanup_absence(context, platform, expected_snapshot):
    root_present = filesystem_entry_present(
        context.root, "cleanup_root_invalid"
    ) or filesystem_entry_present(
        cleanup_root_quarantine_path(context), "cleanup_root_invalid"
    )
    return {
        "database_absent": not root_present,
        "postgres_process_absent": cleanup_postgres_absent(context, platform),
        "launchd_jobs_absent": all(
            platform.launchd_absent(service["label"])
            for service in context.manifest["services"].values()
        ),
        "keychain_items_absent": all(
            not platform.keychain_present(service, account)
            for service, account in keychain_inventory(context)
        ),
        "isolated_root_absent": not root_present,
        "protected_staging_unchanged": standing_snapshot(context, platform)
        == expected_snapshot,
    }


def new_cleanup_evidence(context, absence):
    evidence = {
        "schema_version": 1,
        "manifest_sha256": context.digest,
        "observed_at": utc_now(),
        **absence,
    }
    return validate_cleanup_evidence(context, evidence)


def validate_cleanup_evidence(context, evidence):
    boolean_fields = {
        "database_absent",
        "postgres_process_absent",
        "launchd_jobs_absent",
        "keychain_items_absent",
        "isolated_root_absent",
        "protected_staging_unchanged",
    }
    if (
        not isinstance(evidence, dict)
        or set(evidence)
        != {
            "schema_version",
            "manifest_sha256",
            "observed_at",
            *boolean_fields,
        }
        or evidence.get("schema_version") != 1
        or evidence.get("manifest_sha256") != context.digest
        or not isinstance(evidence.get("observed_at"), str)
        or not EVIDENCE_RECORDED_AT_PATTERN.fullmatch(evidence["observed_at"])
        or any(evidence.get(field) is not True for field in boolean_fields)
    ):
        fail("cleanup_evidence_invalid")
    return evidence


def require_terminal_cleanup_root_progress(context):
    root = cleanup_path_metadata(context.root, "cleanup_root_invalid")
    quarantine = cleanup_path_metadata(
        cleanup_root_quarantine_path(context), "cleanup_root_invalid"
    )
    if root is not None or quarantine is not None:
        fail("cleanup_root_not_deleted")
    identity = load_cleanup_root_identity(context)
    progress = load_cleanup_root_progress(context, identity)
    if identity is None:
        if progress is not None:
            fail("cleanup_root_progress_invalid")
        return None
    if progress is None:
        fail("cleanup_root_progress_invalid")
    if progress["phase"] != "deleted":
        fail("cleanup_root_progress_not_terminal")
    return progress


def cleanup_journal_rows(context):
    try:
        rows = load_lifecycle_journal(context)
    except BaseException:
        fail("cleanup_journal_invalid")
    for row in rows:
        if row["action"] == "cleanup" and (
            row["target"] != "run"
            or row["status"] not in {"intent", "failed", "complete"}
        ):
            fail("cleanup_journal_invalid")
    return rows


def ensure_cleanup_journal_complete(context):
    rows = cleanup_journal_rows(context)
    cleanup_rows = [
        (index, row)
        for index, row in enumerate(rows)
        if row["action"] == "cleanup" and row["target"] == "run"
    ]
    intents = [index for index, row in cleanup_rows if row["status"] == "intent"]
    if not intents:
        fail("cleanup_journal_incomplete")
    last_intent = intents[-1]
    if (
        cleanup_rows[-1][0] <= last_intent
        or cleanup_rows[-1][1]["status"] != "complete"
    ):
        append_journal(context, "cleanup", "complete", "run")
        rows = cleanup_journal_rows(context)
        cleanup_rows = [
            (index, row)
            for index, row in enumerate(rows)
            if row["action"] == "cleanup" and row["target"] == "run"
        ]
        if (
            cleanup_rows[-1][0] <= last_intent
            or cleanup_rows[-1][1]["status"] != "complete"
        ):
            fail("cleanup_journal_incomplete")


def cleanup_keychain_baseline_path(context):
    return context.artifact_directory / "cleanup-keychain-baseline.json"


def validate_cleanup_keychain_baseline(context, baseline):
    inventory = tuple(keychain_inventory(context))
    if (
        not isinstance(baseline, dict)
        or set(baseline)
        != {
            "schema_version",
            "kind",
            "manifest_sha256",
            "run_id",
            "inventory",
        }
        or baseline.get("schema_version") != 1
        or baseline.get("kind") != CLEANUP_KEYCHAIN_BASELINE_KIND
        or baseline.get("manifest_sha256") != context.digest
        or baseline.get("run_id") != context.manifest["run_id"]
        or not isinstance(baseline.get("inventory"), list)
        or len(baseline["inventory"]) != len(inventory)
    ):
        fail("cleanup_keychain_baseline_invalid")
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
            fail("cleanup_keychain_baseline_invalid")
        identity = (entry["service"], entry["account"])
        if identity in identities:
            fail("cleanup_keychain_baseline_invalid")
        observed_inventory.append(identity)
        identities[identity] = entry["identity_sha256"]
    if tuple(observed_inventory) != inventory:
        fail("cleanup_keychain_baseline_invalid")
    for service in {service for service, _account in inventory}:
        service_identities = {
            account: identities[(service, account)]
            for item_service, account in inventory
            if item_service == service
        }
        if any(value is not None for value in service_identities.values()) and (
            service_identities.get(OWNER_ACCOUNT) is None
        ):
            fail("cleanup_keychain_baseline_invalid")
    return identities


def observe_cleanup_keychain_inventory(context, platform):
    observed = {}
    for service, account in keychain_inventory(context):
        identity = platform.keychain_item_identity(service, account)
        if identity is not None and (
            not isinstance(identity, str)
            or not DIGEST_PATTERN.fullmatch(identity)
        ):
            fail("cleanup_keychain_identity_invalid")
        observed[(service, account)] = identity
    return observed


def load_cleanup_keychain_baseline(context):
    path = cleanup_keychain_baseline_path(context)
    if cleanup_path_metadata(path, "cleanup_keychain_baseline_invalid") is None:
        fail("cleanup_keychain_baseline_invalid")
    require_owned_mode(path, 0o600, "cleanup_keychain_baseline")
    return validate_cleanup_keychain_baseline(
        context, load_json(path, "cleanup_keychain_baseline_invalid")
    )


def load_or_create_cleanup_keychain_baseline(context, platform):
    path = cleanup_keychain_baseline_path(context)
    metadata = cleanup_path_metadata(path, "cleanup_keychain_baseline_invalid")
    if metadata is None:
        observed = observe_cleanup_keychain_inventory(context, platform)
        baseline = {
            "schema_version": 1,
            "kind": CLEANUP_KEYCHAIN_BASELINE_KIND,
            "manifest_sha256": context.digest,
            "run_id": context.manifest["run_id"],
            "inventory": [
                {
                    "service": service,
                    "account": account,
                    "identity_sha256": observed[(service, account)],
                }
                for service, account in keychain_inventory(context)
            ],
        }
        identities = validate_cleanup_keychain_baseline(context, baseline)
        for service in {service for service, _account in identities}:
            if any(
                identity is not None
                for (item_service, _account), identity in identities.items()
                if item_service == service
            ) and not platform.keychain_owner_matches(
                service, context.manifest["run_id"]
            ):
                fail("cleanup_keychain_ownership_invalid")
        write_atomic(path, canonical_json(baseline) + "\n")
    return load_cleanup_keychain_baseline(context)


def validate_cleanup_keychain_replay(context, platform, baseline):
    current = observe_cleanup_keychain_inventory(context, platform)
    for identity, original in baseline.items():
        observed = current[identity]
        if original is None:
            if observed is not None:
                fail("cleanup_keychain_identity_drift")
        elif observed not in {None, original}:
            fail("cleanup_keychain_identity_drift")
    for service in {service for service, _account in baseline}:
        pending = {
            account
            for (item_service, account), original in baseline.items()
            if item_service == service
            and original is not None
            and current[(item_service, account)] == original
        }
        if pending and (
            OWNER_ACCOUNT not in pending
            or not platform.keychain_owner_matches(
                service, context.manifest["run_id"]
            )
        ):
            fail("cleanup_keychain_ownership_invalid")
    return current


def cleanup_keychain_inventory(context, platform):
    baseline = load_or_create_cleanup_keychain_baseline(context, platform)
    for service in sorted({service for service, _account in baseline}):
        accounts = sorted(
            (
                account
                for (item_service, account), original in baseline.items()
                if item_service == service and original is not None
            ),
            key=lambda account: account == OWNER_ACCOUNT,
        )
        for account in accounts:
            current = validate_cleanup_keychain_replay(
                context, platform, baseline
            )
            target = (service, account)
            if current[target] is None:
                continue
            if not platform.keychain_owner_matches(
                service, context.manifest["run_id"]
            ):
                fail("cleanup_keychain_ownership_invalid")
            platform.keychain_delete_exact(service, account, baseline[target])
            if platform.keychain_item_identity(service, account) is not None:
                fail("cleanup_keychain_delete_unconfirmed")
    current = validate_cleanup_keychain_replay(context, platform, baseline)
    if any(identity is not None for identity in current.values()):
        fail("cleanup_keychain_delete_unconfirmed")


def require_cleanup_keychain_baseline_absent(context, platform):
    baseline = load_cleanup_keychain_baseline(context)
    current = validate_cleanup_keychain_replay(context, platform, baseline)
    if any(identity is not None for identity in current.values()):
        fail("cleanup_keychain_delete_unconfirmed")


def cleanup(context, platform, expected_snapshot, from_failure=False):
    validate_cleanup_mutation_roots(context)
    append_journal(context, "cleanup", "intent", "run")
    failures = []
    for name in SERVICE_STOP_ORDER:
        label = context.manifest["services"][name]["label"]
        try:
            platform.launchd_bootout(label)
        except BaseException:
            failures.append(f"launchd:{name}")
    try:
        platform.postgres_stop(context.cluster_root)
    except BaseException:
        failures.append("postgres")
    try:
        cleanup_keychain_inventory(context, platform)
    except BaseException:
        failures.append("keychain")
    try:
        postgres_inert = cleanup_postgres_absent(context, platform)
    except BaseException:
        failures.append("postgres_observation")
        postgres_inert = False
    try:
        launchd_inert = all(
            platform.launchd_absent(service["label"])
            for service in context.manifest["services"].values()
        )
    except BaseException:
        failures.append("launchd_observation")
        launchd_inert = False
    if postgres_inert and launchd_inert and not failures:
        try:
            guarded_remove_root(context, platform)
        except BaseException:
            failures.append("root")
    else:
        failures.append("root_removal_blocked")
    try:
        absence = cleanup_absence(context, platform, expected_snapshot)
    except BaseException:
        failures.append("absence_observation")
        absence = {
            "database_absent": False,
            "postgres_process_absent": False,
            "launchd_jobs_absent": False,
            "keychain_items_absent": False,
            "isolated_root_absent": False,
            "protected_staging_unchanged": False,
        }
    if not all(absence.values()):
        failures.append("absence_verification")
    if failures:
        append_journal(context, "cleanup", "failed", "run")
        fail("cleanup_incomplete")
    require_terminal_cleanup_root_progress(context)
    try:
        release_discord_ownership(context)
    except BaseException:
        append_journal(context, "cleanup", "failed", "run")
        fail("cleanup_incomplete")
    save_state(context, "cleaned", expected_snapshot)
    evidence = new_cleanup_evidence(context, absence)
    write_atomic(
        context.artifact_directory / "cleanup-evidence.json",
        canonical_json(evidence) + "\n",
    )
    append_journal(context, "cleanup", "complete", "run")
    ensure_cleanup_journal_complete(context)
    return {
        "status": "cleaned_after_failure" if from_failure else "cleaned",
        "phase": "cleaned",
        "database_absent": True,
        "postgres_process_absent": True,
        "launchd_jobs_absent": True,
        "keychain_items_absent": True,
        "isolated_root_absent": True,
        "protected_staging_unchanged": True,
    }


def command_cleanup_internal(context, platform, retire_committed):
    state = load_state(context)
    if retire_committed and state["phase"] != "cleaned":
        persist_candidate_abort_retirement(
            context, state, "explicit_cleanup"
        )
    if state["phase"] == "cleaned":
        require_discord_ownership_released(context)
        require_terminal_cleanup_root_progress(context)
        require_cleanup_keychain_baseline_absent(context, platform)
        absence = cleanup_absence(context, platform, state["standing_snapshot"])
        if not all(absence.values()):
            fail("cleanup_incomplete")
        path = context.artifact_directory / "cleanup-evidence.json"
        if path.exists() or path.is_symlink():
            require_owned_mode(path, 0o600, "cleanup_evidence")
            validate_cleanup_evidence(
                context, load_json(path, "cleanup_evidence_invalid")
            )
        else:
            write_atomic(
                path,
                canonical_json(new_cleanup_evidence(context, absence)) + "\n",
            )
        ensure_cleanup_journal_complete(context)
        return {
            "status": "already_cleaned",
            "phase": "cleaned",
            **absence,
        }
    return cleanup(context, platform, state["standing_snapshot"])


def command_cleanup(context, platform):
    return command_cleanup_internal(context, platform, retire_committed=True)


def command_status(context, platform):
    state = load_state(context)
    return {
        "status": "observed",
        "phase": state["phase"],
        "postgres_running": context.cluster_root.exists()
        and platform.postgres_running(context.cluster_root),
        "candidate_launchd_jobs_loaded": sum(
            1
            for service in context.manifest["services"].values()
            if platform.launchd_loaded(service["label"])
        ),
        "protected_staging_unchanged": standing_snapshot(context, platform)
        == state["standing_snapshot"],
    }


def build_parser():
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    for command in (
        "dry-run",
        "prepare",
        "start",
        "restart-drained-runtime",
        "resource-inventory",
        "teardown-discord-resources",
        "finalize-run",
        "stop",
        "cleanup",
        "status",
    ):
        child = subparsers.add_parser(command)
        child.add_argument("--manifest", required=True)
    live_restart = subparsers.add_parser("certify-live-runtime-restart")
    live_restart.add_argument("--manifest", required=True)
    live_restart.add_argument("--confirmation-file")
    onboard = subparsers.add_parser("onboard")
    onboard.add_argument("--manifest", required=True)
    onboard.add_argument("--principal-id", required=True)
    onboard.add_argument("--display-name", required=True)
    transport = subparsers.add_parser("transport-control")
    transport.add_argument("--manifest", required=True)
    transport.add_argument(
        "--operation",
        required=True,
        choices=(
            "snapshot",
            "arm-next-duplicate",
            "disarm-duplicate",
            "arm-next-indeterminate",
            "disarm-indeterminate",
            "partition-gateway",
            "heal-gateway",
        ),
    )
    evidence = subparsers.add_parser("transport-evidence")
    evidence.add_argument("--manifest", required=True)
    evidence.add_argument(
        "--checkpoint",
        required=True,
        choices=tuple(TRANSPORT_EVIDENCE_KINDS),
    )
    worker_evidence = subparsers.add_parser("worker-authoring-evidence")
    worker_evidence.add_argument("--manifest", required=True)
    worker_evidence.add_argument(
        "--checkpoint", required=True, choices=("before", "after")
    )
    worker_evidence.add_argument("--browser-evidence")
    reconciliation_observation = subparsers.add_parser(
        "reconciliation-discord-observation"
    )
    reconciliation_observation.add_argument("--manifest", required=True)
    reconciliation_observation.add_argument(
        "--database-evidence", required=True
    )
    total_absence = subparsers.add_parser("finalize-total-absence")
    total_absence.add_argument("--manifest", required=True)
    total_absence.add_argument("--prefix-scan-evidence", required=True)
    total_absence.add_argument("--guild-deletion-evidence", required=True)
    legacy_status = subparsers.add_parser("legacy-substrate-status")
    legacy_status.add_argument("--manifest", required=True)
    legacy_recovery = subparsers.add_parser("recover-legacy-substrate")
    legacy_recovery.add_argument("--manifest", required=True)
    legacy_recovery.add_argument("--confirm-run-id", required=True)
    legacy_recovery.add_argument("--confirm-manifest-sha256", required=True)
    return parser


def main():
    arguments = build_parser().parse_args()
    try:
        if arguments.command in {
            "legacy-substrate-status",
            "recover-legacy-substrate",
        }:
            context, legacy_state = load_legacy_context(
                require_absolute_path(arguments.manifest, "manifest")
            )
        else:
            context = load_context(
                require_absolute_path(arguments.manifest, "manifest")
            )
            legacy_state = None
        platform = Platform()
        handlers = {
            "dry-run": command_dry_run,
            "prepare": command_prepare,
            "start": command_start,
            "restart-drained-runtime": command_restart_drained_runtime,
            "resource-inventory": command_resource_inventory,
            "teardown-discord-resources": command_teardown_discord_resources,
            "stop": command_stop,
            "cleanup": command_cleanup,
            "status": command_status,
        }
        with global_operation_lock():
            if arguments.command not in {
                "legacy-substrate-status",
                "recover-legacy-substrate",
                "finalize-run",
                "finalize-total-absence",
                "stop",
                "cleanup",
                "status",
            } and finalization_freeze_committed(context):
                fail("orchestrator_phase_invalid")
            if arguments.command not in {
                "legacy-substrate-status",
                "recover-legacy-substrate",
                "teardown-discord-resources",
                "stop",
                "cleanup",
                "status",
            }:
                require_candidate_start_not_retired(context)
            if arguments.command == "legacy-substrate-status":
                result = command_legacy_substrate_status(
                    context, legacy_state, platform
                )
            elif arguments.command == "recover-legacy-substrate":
                result = command_recover_legacy_substrate(
                    context,
                    legacy_state,
                    platform,
                    arguments.confirm_run_id,
                    arguments.confirm_manifest_sha256,
                )
            elif arguments.command == "onboard":
                result = command_onboard(
                    context, platform, arguments.principal_id, arguments.display_name
                )
            elif arguments.command == "transport-control":
                result = command_transport_control(
                    context,
                    platform,
                    arguments.operation,
                )
            elif arguments.command == "transport-evidence":
                result = command_transport_evidence(
                    context,
                    platform,
                    arguments.checkpoint,
                )
            elif arguments.command == "worker-authoring-evidence":
                result = command_worker_authoring_evidence(
                    context,
                    platform,
                    arguments.checkpoint,
                    arguments.browser_evidence,
                )
            elif arguments.command == "reconciliation-discord-observation":
                result = command_reconciliation_discord_observation(
                    context,
                    platform,
                    arguments.database_evidence,
                )
            elif arguments.command == "finalize-run":
                result = command_finalize_run(
                    context,
                    platform,
                    command_teardown_discord_resources,
                )
            elif arguments.command == "finalize-total-absence":
                result = command_finalize_total_absence(
                    context,
                    platform,
                    arguments.prefix_scan_evidence,
                    arguments.guild_deletion_evidence,
                )
            elif arguments.command == "certify-live-runtime-restart":
                confirmation_path = (
                    None
                    if arguments.confirmation_file is None
                    else require_absolute_path(
                        arguments.confirmation_file, "confirmation_file"
                    )
                )
                result = command_certify_live_runtime_restart(
                    context, platform, confirmation_path
                )
            else:
                result = handlers[arguments.command](context, platform)
        print(canonical_json(result))
    except (CertificationError, OrchestratorError) as error:
        print(canonical_json({"status": "failed", "code": str(error)}), file=sys.stderr)
        raise SystemExit(1)
    except KeyboardInterrupt:
        print(
            canonical_json({"status": "failed", "code": "d2_operation_interrupted"}),
            file=sys.stderr,
        )
        raise SystemExit(130)


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, lambda _signal, _frame: (_ for _ in ()).throw(KeyboardInterrupt()))
    main()
