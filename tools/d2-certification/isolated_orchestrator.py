#!/usr/bin/env python3

import argparse
import hashlib
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
    STEP_SPECS,
    canonical_json,
    load_json_file,
    require_absolute_path,
    require_owned_mode,
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
from d2_finalization import (
    abort_teardown_evidence_path,
    abort_teardown_progress_path,
    abort_teardown_tombstone_path,
    certified_teardown_binding,
    command_finalize_run,
    command_finalize_total_absence,
    require_certification_eligible_teardown,
)
from d2_live_runtime_restart import command_certify_live_runtime_restart
from d2_source_contract import (
    publish_bootstrap_source,
    publish_candidate_source,
    publish_onboarding_source,
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
    return publish_bootstrap_source(context, step_one_evidence, utc_now())


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
    return publish_candidate_source(context, evidence, utc_now())


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


def candidate_coordinator_sources(context):
    bootstrap = publish_bootstrap_source(
        context, load_step_evidence(context, 1), utc_now()
    )
    candidate = publish_candidate_source(
        context, load_step_evidence(context, 3), utc_now()
    )
    return {"1": str(bootstrap), "3": str(candidate)}


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
        return {
            "status": "already_started",
            "phase": "candidate_started",
            "coordinator_sources": candidate_coordinator_sources(context),
        }
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
        candidate_source = write_candidate_evidence(context, statuses, platform)
        append_journal(context, "postgres_start", "complete", "cluster")
        save_state(context, "candidate_started", state["standing_snapshot"])
        return {
            "status": "candidate_started",
            "phase": "candidate_started",
            "candidate_services_loaded": True,
            "database_schema_ready": True,
            "credentials_sealed": True,
            "coordinator_sources": {
                "1": str(bootstrap_source),
                "3": str(candidate_source),
            },
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
        coordinator_source = publish_onboarding_source(
            context, output, utc_now()
        )
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


def require_candidate_certification_boundary(context, platform):
    state = load_state(context, {"candidate_started"})
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
    require_pinned_transport_snapshot(context, snapshot)
    return state, snapshot


def require_frozen_discord_teardown_boundary(context, platform):
    state = load_state(context, {"candidate_started"})
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
    require_pinned_transport_snapshot(context, snapshot)
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
    state = load_state(context, {"candidate_started"})
    if not platform.postgres_running(context.cluster_root) or any(
        not platform.launchd_loaded(context.manifest["services"][name]["label"])
        for name in SERVICE_START_ORDER
    ):
        fail("candidate_state_drift")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    snapshot = platform.transport_control(context, "snapshot")
    require_pinned_transport_snapshot(context, snapshot)
    runtime_status = platform.http_status(
        "http://127.0.0.1:"
        f"{context.manifest['services']['runtime']['port']}/health/ready"
    )
    if runtime_status != 503:
        fail("gateway_loss_runtime_readiness_invalid")
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
    if certification_binding is not None and any(
        evidence[field] != value
        for field, value in certification_binding.items()
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
    boundary = (
        require_frozen_discord_teardown_boundary
        if frozen
        else require_candidate_certification_boundary
    )
    _state, snapshot = boundary(context, platform)
    inventory = platform.transport_control(context, "resource_inventory")
    if inventory["instance_id"] != snapshot["instance_id"]:
        fail("transport_instance_changed")
    certification_binding = certified_teardown_binding(context) if frozen else None
    if certification_binding is not None and inventory["digest_sha256"] != (
        certification_binding["freeze_resource_inventory_digest_sha256"]
    ):
        fail("discord_teardown_live_inventory_drift")
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
    else:
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


def validate_cleanup_mutation_roots(context):
    expected = pathlib.Path(f"/private/tmp/starring-d2-{context.manifest['run_id']}")
    if context.root != expected or context.cluster_root != expected / "postgres":
        fail("cleanup_root_guard_failed")
    try:
        root_metadata = context.root.lstat()
    except FileNotFoundError:
        return
    except OSError:
        fail("cleanup_root_invalid")
    if (
        not stat.S_ISDIR(root_metadata.st_mode)
        or context.root.is_symlink()
        or root_metadata.st_uid != os.getuid()
        or stat.S_IMODE(root_metadata.st_mode) != 0o700
    ):
        fail("cleanup_root_invalid")
    try:
        cluster_metadata = context.cluster_root.lstat()
    except FileNotFoundError:
        return
    except OSError:
        fail("cleanup_cluster_invalid")
    if (
        not stat.S_ISDIR(cluster_metadata.st_mode)
        or context.cluster_root.is_symlink()
        or cluster_metadata.st_uid != os.getuid()
        or stat.S_IMODE(cluster_metadata.st_mode) != 0o700
    ):
        fail("cleanup_cluster_invalid")


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
            "resource-inventory": command_resource_inventory,
            "teardown-discord-resources": command_teardown_discord_resources,
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
                    command_cleanup,
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
