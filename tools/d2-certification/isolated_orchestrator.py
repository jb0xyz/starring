#!/usr/bin/env python3

import argparse
import os
import pathlib
import re
import shutil
import signal
import stat
import sys
import unicodedata

from d2_certification import (
    CertificationError,
    canonical_json,
    require_absolute_path,
    validate_snowflake,
)
from d2_orchestrator_composition import (
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
    external_keychain_inventory,
    fail,
    global_operation_lock,
    keychain_inventory,
    load_json,
    load_context,
    load_state,
    owner_identities,
    save_state,
    standing_snapshot,
    utc_now,
    validate_candidate_programs,
    validate_dedicated_discord_identity,
    validate_ports,
    validate_programs,
    write_atomic,
)
from d2_orchestrator_platform import Platform
from d2_drained_runtime_restart import (
    command_restart_drained_runtime,
    drained_runtime_restart_directory,
    drained_runtime_restart_temporary_directory,
)
from d2_live_runtime_restart import command_certify_live_runtime_restart


SERVICE_START_ORDER = ("transport", "worker", "api", "runtime", "tunnel")
SERVICE_STOP_ORDER = tuple(reversed(SERVICE_START_ORDER))
TRANSPORT_INSTANCE_PATTERN = re.compile(r"^d2ti-[0-9a-f]{32}$")


def command_dry_run(context, platform):
    validate_programs(platform)
    validate_candidate_programs(context, platform)
    validate_ports(context, platform, require_available=True)
    validate_dedicated_discord_identity(context)
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
            if (
                not (context.cluster_root / "PG_VERSION").is_file()
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
    append_journal(context, "prepare", "intent", "run")
    try:
        append_journal(context, "root_create", "intent", "isolated_root")
        context.root.mkdir(mode=0o700)
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


def write_candidate_evidence(context, statuses, platform):
    manifest = context.manifest
    transport_snapshot = platform.transport_control(context, "snapshot")
    evidence = {
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
        "api_ready_status": statuses["api"],
        "runtime_ready_status": statuses["runtime"],
        "worker_ready_status": statuses["worker"],
        "cloudflare_tunnel_id": manifest["cloudflare"]["tunnel_id"],
        "public_origin": manifest["cloudflare"]["public_origin"],
        "origin_service": manifest["cloudflare"]["origin_service"],
        "transport_instance_id": transport_snapshot["instance_id"],
        "transport_ready": statuses["transport"] == 200,
        "tunnel_ready": statuses["tunnel"] == 200,
    }
    write_atomic(
        context.artifact_directory / "step-03-evidence.json",
        canonical_json(evidence) + "\n",
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
    if state["phase"] == "candidate_started":
        if not platform.postgres_running(context.cluster_root) or any(
            not platform.launchd_loaded(context.manifest["services"][name]["label"])
            for name in SERVICE_START_ORDER
        ):
            fail("candidate_state_drift")
        statuses = candidate_health(context, platform, wait=True)
        if any(status != 200 for status in statuses.values()):
            fail("candidate_health_unready")
        require_pinned_transport_snapshot(
            context, platform.transport_control(context, "snapshot")
        )
        return {"status": "already_started", "phase": "candidate_started"}
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
        write_database_evidence(context, database_evidence)
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
        write_candidate_evidence(context, statuses, platform)
        append_journal(context, "postgres_start", "complete", "cluster")
        save_state(context, "candidate_started", state["standing_snapshot"])
        return {
            "status": "candidate_started",
            "phase": "candidate_started",
            "candidate_services_loaded": True,
            "database_schema_ready": True,
            "credentials_sealed": True,
        }
    except BaseException:
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
    if platform.postgres_running(context.cluster_root):
        failures.append("postgres_running")
    if failures:
        fail("candidate_stop_incomplete")
    append_journal(context, "postgres_stop", "complete", "cluster")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    save_state(context, "stopped", state["standing_snapshot"])
    return {"status": "stopped", "phase": "stopped"}


def command_onboard(context, platform, principal_id, display_name):
    state = load_state(context, {"candidate_started", "onboarding"})
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
        append_journal(context, "installation_onboard", "complete", "installation")
        save_state(context, "candidate_started", state["standing_snapshot"])
        return {"status": "onboarded", **output}
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


def command_transport_control(context, platform, operation):
    state = load_state(context, {"candidate_started"})
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
    require_pinned_transport_snapshot(context, pre_snapshot)
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
    require_pinned_transport_snapshot(context, snapshot)
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


def guarded_remove_root(context):
    expected = pathlib.Path(f"/private/tmp/starring-d2-{context.manifest['run_id']}")
    if context.root != expected or context.root.parent != pathlib.Path("/private/tmp"):
        fail("cleanup_root_guard_failed")
    if not context.root.exists():
        return
    metadata = context.root.lstat()
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or context.root.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        fail("cleanup_root_invalid")
    shutil.rmtree(context.root)


def cleanup(context, platform, expected_snapshot, from_failure=False):
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
    accounts_by_service = {}
    for service, account in keychain_inventory(context):
        accounts_by_service.setdefault(service, []).append(account)
    for service, accounts in accounts_by_service.items():
        present = [
            account for account in accounts if platform.keychain_present(service, account)
        ]
        if not present:
            continue
        if not platform.keychain_owner_matches(service, context.manifest["run_id"]):
            failures.append(f"keychain_ownership:{service}")
            continue
        for account in sorted(present, key=lambda value: value == OWNER_ACCOUNT):
            try:
                platform.keychain_delete(service, account)
            except BaseException:
                failures.append(f"keychain:{service}")
    try:
        guarded_remove_root(context)
    except BaseException:
        failures.append("root")
    launchd_absent = all(
        not platform.launchd_loaded(service["label"])
        for service in context.manifest["services"].values()
    )
    keychain_absent = all(
        not platform.keychain_present(service, account)
        for service, account in keychain_inventory(context)
    )
    postgres_absent = not context.cluster_root.exists() or not platform.postgres_running(
        context.cluster_root
    )
    root_absent = not context.root.exists()
    standing_unchanged = standing_snapshot(context, platform) == expected_snapshot
    if not all(
        (launchd_absent, keychain_absent, postgres_absent, root_absent, standing_unchanged)
    ):
        failures.append("absence_verification")
    if failures:
        append_journal(context, "cleanup", "failed", "run")
        fail("cleanup_incomplete")
    save_state(context, "cleaned", expected_snapshot)
    evidence = {
        "schema_version": 1,
        "manifest_sha256": context.digest,
        "observed_at": utc_now(),
        "database_absent": True,
        "postgres_process_absent": True,
        "launchd_jobs_absent": True,
        "keychain_items_absent": True,
        "isolated_root_absent": True,
        "protected_staging_unchanged": True,
    }
    write_atomic(
        context.artifact_directory / "cleanup-evidence.json",
        canonical_json(evidence) + "\n",
    )
    append_journal(context, "cleanup", "complete", "run")
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


def command_cleanup(context, platform):
    state = load_state(context)
    if state["phase"] == "cleaned":
        return {
            "status": "already_cleaned",
            "phase": "cleaned",
            "protected_staging_unchanged": standing_snapshot(context, platform)
            == state["standing_snapshot"],
        }
    return cleanup(context, platform, state["standing_snapshot"])


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
    return parser


def main():
    arguments = build_parser().parse_args()
    try:
        context = load_context(require_absolute_path(arguments.manifest, "manifest"))
        platform = Platform()
        handlers = {
            "dry-run": command_dry_run,
            "prepare": command_prepare,
            "start": command_start,
            "restart-drained-runtime": command_restart_drained_runtime,
            "stop": command_stop,
            "cleanup": command_cleanup,
            "status": command_status,
        }
        with global_operation_lock():
            if arguments.command == "onboard":
                result = command_onboard(
                    context, platform, arguments.principal_id, arguments.display_name
                )
            elif arguments.command == "transport-control":
                result = command_transport_control(
                    context,
                    platform,
                    arguments.operation,
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
