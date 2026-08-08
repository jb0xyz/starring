#!/usr/bin/env python3

import contextlib
import ctypes
import datetime
import fcntl
import hashlib
import importlib.util
import io
import json
import os
import pathlib
import plistlib
import secrets
import stat
import subprocess
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock


DIRECTORY = pathlib.Path(__file__).parent
sys.path.insert(0, str(DIRECTORY))
CERTIFICATION_SPEC = importlib.util.spec_from_file_location(
    "d2_certification", DIRECTORY / "d2_certification.py"
)
CERTIFICATION = importlib.util.module_from_spec(CERTIFICATION_SPEC)
sys.modules["d2_certification"] = CERTIFICATION
CERTIFICATION_SPEC.loader.exec_module(CERTIFICATION)
ORCHESTRATOR_SPEC = importlib.util.spec_from_file_location(
    "isolated_orchestrator", DIRECTORY / "isolated_orchestrator.py"
)
ORCHESTRATOR = importlib.util.module_from_spec(ORCHESTRATOR_SPEC)
ORCHESTRATOR_SPEC.loader.exec_module(ORCHESTRATOR)
CONTRACT = sys.modules["d2_orchestrator_contract"]
PLATFORM = sys.modules["d2_orchestrator_platform"]
DRAINED_RUNTIME_RESTART = sys.modules["d2_drained_runtime_restart"]
LIVE_RUNTIME_RESTART = sys.modules["d2_live_runtime_restart"]
D2_RUN = sys.modules["d2_run"]
D2_SOURCE_CONTRACT = sys.modules["d2_source_contract"]
from test_d2_certification import complete_evidence as complete_certification_evidence


class RecordingLaunchdPlatform(PLATFORM.Platform):
    def __init__(self):
        self.loaded = False
        self.commands = []
        self.fail_action = None
        self.bootouts = []

    def launchd_loaded(self, label):
        return self.loaded

    def run(self, arguments, input_bytes=None, timeout=30, environment=None):
        command = [str(argument) for argument in arguments]
        self.commands.append(command)
        if len(command) > 1 and command[1] == "bootstrap":
            self.loaded = True
        returncode = 1 if len(command) > 1 and command[1] == self.fail_action else 0
        return subprocess.CompletedProcess(command, returncode, b"", b"")

    def launchd_bootout(self, label):
        self.bootouts.append(label)
        self.loaded = False


class FakePlatform:
    def __init__(self):
        self.keychain = {
            ("starring.d2.credentials", "discord.oauth-client-secret"),
            ("starring.d2.credentials", "discord.bot-token"),
            ("starring.d2.credentials", "cloudflare.tunnel-token"),
        }
        self.loaded = {
            "local.starring.api.staging",
            "local.starring.codex-worker",
            "local.starring.runtime.staging",
            "local.cloudflared.starring",
        }
        self.postgres = False
        self.postgres_tcp = False
        self.postgres_port = None
        self.postgres_process_pid = None
        self.postgres_observation_error = False
        self.postgres_process_observations = []
        self.launchd_observation_error = False
        self.keychain_writes = []
        self.keychain_deletes = []
        self.keychain_identity_versions = {}
        self.keychain_replace_at_delete = None
        self.keychain_crash_after_delete = None
        self.owner_values = {}
        self.bootouts = []
        self.initdb_failure = False
        self.bootstrap_failure = False
        self.start_order = []
        self.health_failure = None
        self.launchd_failure = None
        self.transport_state = None
        self.http_probes = []
        self.lifecycle_events = []
        self.pids = {}
        self.programs = {}
        self.plist_paths = {}
        self.program_arguments = {}
        self.launchd_runs = {}
        self.launchd_states = {}
        self.last_exit_codes = {}
        self.runtime_process_instance_ids = {}
        self.runtime_identity_override = None
        self.process_start_times = {}
        self.process_identity_sequences = {}
        self.process_identity_observations = []
        self.process_identity_hook = None
        self.process_identity_call_counts = {}
        self.launchd_job_hook = None
        self.launchd_job_call_counts = {}
        self.http_status_hook = None
        self.http_status_call_counts = {}
        self.signals = []
        self.suspended_pids = set()
        self.signal_exit_code = 0
        self.signal_failure = False
        self.next_pid = 41000
        self.resource_history = []
        self.discord_existing = set()
        self.proxy_deletions = []
        self.proxy_failure_resource_id = None
        self.proxy_failure_after_delete = False
        self.worker_instance_id = "worker-0123456789abcdef"
        self.worker_accepted_requests = 0
        self.worker_settled_requests = 0
        self.worker_active_requests = 0
        self.worker_queued_requests = 0

    def run(self, arguments, input_bytes=None, timeout=30, environment=None):
        executable = pathlib.Path(arguments[0]).name
        if executable == "initdb" and "--version" in arguments:
            return subprocess.CompletedProcess(arguments, 0, b"initdb (PostgreSQL) 16.14\n", b"")
        return subprocess.CompletedProcess(arguments, 0, b"", b"")

    def executable(self, path):
        return True

    def port_available(self, port):
        return port not in ORCHESTRATOR.PROTECTED_PORTS and not (
            self.postgres and self.postgres_tcp and port == self.postgres_port
        )

    def launchd_loaded(self, label):
        return label in self.loaded

    def launchd_absent(self, label):
        if self.launchd_observation_error:
            ORCHESTRATOR.fail("launchd_observation_failed")
        return label not in self.loaded

    def launchd_job(self, label):
        if label not in self.loaded:
            return None
        value = {
            "pid": self.pids.get(label),
            "program": self.programs.get(label),
            "plist_path": self.plist_paths.get(label),
            "arguments": self.program_arguments.get(label),
            "runs": self.launchd_runs.get(label),
            "state": self.launchd_states.get(label),
            "last_exit_code": self.last_exit_codes.get(label),
        }
        count = self.launchd_job_call_counts.get(label, 0) + 1
        self.launchd_job_call_counts[label] = count
        if self.launchd_job_hook is not None:
            return self.launchd_job_hook(label, count, value)
        return value

    def exit_launchd(self, label, exit_code=0):
        self.pids.pop(label, None)
        self.launchd_states[label] = "exited"
        self.last_exit_codes[label] = exit_code

    def keychain_present(self, service, account):
        return (service, account) in self.keychain

    def keychain_item_identity(self, service, account):
        identity = (service, account)
        if identity not in self.keychain:
            return None
        version = self.keychain_identity_versions.get(identity, 0)
        return hashlib.sha256(
            f"{service}\x00{account}\x00{version}".encode("utf-8")
        ).hexdigest()

    def keychain_write_new(self, service, account, value):
        if (service, account) in self.keychain:
            ORCHESTRATOR.fail("keychain_identity_busy")
        self.keychain.add((service, account))
        self.keychain_writes.append((service, account))
        if account == ORCHESTRATOR.OWNER_ACCOUNT:
            self.owner_values[service] = value.decode("ascii")

    def keychain_delete_exact(self, service, account, expected_identity):
        identity = (service, account)
        if self.keychain_item_identity(service, account) != expected_identity:
            ORCHESTRATOR.fail("keychain_reference_identity_drift")
        if self.keychain_replace_at_delete == identity:
            self.keychain_identity_versions[identity] = (
                self.keychain_identity_versions.get(identity, 0) + 1
            )
            return
        self.keychain.discard((service, account))
        self.keychain_deletes.append((service, account))
        if account == ORCHESTRATOR.OWNER_ACCOUNT:
            self.owner_values.pop(service, None)
        if self.keychain_crash_after_delete == identity:
            ORCHESTRATOR.fail("injected_keychain_delete_crash")

    def keychain_owner_matches(self, service, expected):
        return self.owner_values.get(service) == expected

    def postgres_running(self, cluster_root):
        return self.postgres

    def postgres_absent(self, cluster_root):
        if self.postgres_observation_error:
            ORCHESTRATOR.fail("postgres_observation_failed")
        return not self.postgres

    def postgres_process_path_absent(self, cluster_root):
        self.postgres_process_observations.append(pathlib.Path(cluster_root))
        if self.postgres_observation_error:
            ORCHESTRATOR.fail("postgres_process_observation_failed")
        return not self.postgres

    def postgres_pid(self, cluster_root):
        return self.postgres_process_pid if self.postgres else None

    def initdb(self, cluster_root):
        cluster_root.mkdir(mode=0o700)
        (cluster_root / "PG_VERSION").write_text("16\n", encoding="utf-8")
        (cluster_root / "postgresql.conf").write_text("", encoding="utf-8")
        (cluster_root / "pg_hba.conf").write_text("", encoding="utf-8")
        if self.initdb_failure:
            ORCHESTRATOR.fail("injected_initdb_failure")

    def postgres_start(self, cluster_root, log_path):
        self.postgres = True
        if self.postgres_process_pid is None:
            self.postgres_process_pid = 40001
        configuration = (cluster_root / "postgresql.conf").read_text(encoding="utf-8")
        self.postgres_port = int(
            next(
                line.partition("=")[2].strip()
                for line in configuration.splitlines()
                if line.startswith("port =")
            )
        )
        network = (cluster_root.parent / "postgres-network.conf").read_text(
            encoding="utf-8"
        )
        self.postgres_tcp = "127.0.0.1" in network

    def postgres_stop(self, cluster_root):
        self.postgres = False
        self.postgres_tcp = False
        self.postgres_process_pid = None

    def bootstrap_database(self, context):
        if self.bootstrap_failure:
            ORCHESTRATOR.fail("injected_bootstrap_failure")
        return {
            "database_system_identifier": "7667905772642692043",
            "migration_count": 117,
            "migration_head": "202608010002",
            "migration_ledger_sha256": "b" * 64,
            "relation_count": 198,
            "capability_function_count": 135,
        }

    def provision_credentials(self, context):
        inventory = ORCHESTRATOR.managed_keychain_inventory(context)
        present = [identity for identity in inventory if identity in self.keychain]
        if present and len(present) != len(inventory):
            for identity in present:
                self.keychain.discard(identity)
            ORCHESTRATOR.fail("sealed_provisioning_failed")
        if not present:
            self.keychain.update(inventory)
            outcome = "fresh"
        else:
            if not self.postgres_tcp:
                ORCHESTRATOR.fail("sealed_provisioning_failed")
            outcome = "exact_replay"
        return {
            "outcome": outcome,
            "application_credentials": 20,
            "keyrings": 3,
            "worker_credentials": 1,
            "external_credentials_checked": 3,
            "activated_roles": 20,
        }

    def postgres_loopback_accepting(self, context):
        return self.postgres and self.postgres_tcp

    def onboard_installation(self, context, principal_id, display_name, installation_id):
        return {
            "outcome": "fresh",
            "installation_id": installation_id,
            "principal_id": principal_id,
            "binding_key": "community_hub",
            "hub_channel_id": context.manifest["discord"]["hub_channel_id"],
        }

    def launchd_start(self, label, plist_path):
        if self.launchd_failure == label:
            ORCHESTRATOR.fail("injected_launchd_failure")
        value = plistlib.loads(pathlib.Path(plist_path).read_bytes())
        self.loaded.add(label)
        self.next_pid += 1
        self.pids[label] = self.next_pid
        self.process_start_times[self.next_pid] = (
            1_700_000_000 + self.next_pid,
            self.next_pid % 1_000_000,
        )
        self.programs[label] = value["ProgramArguments"][0]
        self.plist_paths[label] = str(plist_path)
        self.program_arguments[label] = value["ProgramArguments"]
        self.launchd_runs[label] = 1
        self.launchd_states[label] = "running"
        self.last_exit_codes[label] = None
        self.start_order.append(label)
        self.lifecycle_events.append(f"start:{label}")
        if label.endswith(".runtime"):
            self.runtime_process_instance_ids[label] = f"{self.next_pid:032x}"

    def http_status(self, url, timeout_seconds=3, host_header=None):
        self.http_probes.append((url, host_header))
        count = self.http_status_call_counts.get(url, 0) + 1
        self.http_status_call_counts[url] = count
        if self.health_failure and self.health_failure in url:
            return 503
        if "127.0.0.1:29091/health/ready" in url:
            runtime_label = next(
                (label for label in self.loaded if label.endswith(".runtime")), None
            )
            if (
                runtime_label is None
                or self.pids.get(runtime_label) is None
                or self.pids.get(runtime_label) in self.suspended_pids
            ):
                return 0
        status = 200
        if self.http_status_hook is not None:
            status = self.http_status_hook(url, count, status)
        return status

    def runtime_process_identity(self, context, timeout_seconds=3):
        runtime_label = context.manifest["services"]["runtime"]["label"]
        pid = self.pids.get(runtime_label)
        if pid is None or self.health_failure == "runtime_identity":
            return None
        if self.runtime_identity_override is not None:
            return dict(self.runtime_identity_override)
        return {
            "schema_version": 1,
            "os_pid": pid,
            "process_instance_id": self.runtime_process_instance_ids[
                runtime_label
            ],
        }

    def candidate_process_identity(self, pid, expected_path):
        self.process_identity_observations.append((pid, pathlib.Path(expected_path)))
        sequence = self.process_identity_sequences.get(pid)
        if sequence:
            value = sequence.pop(0)
            if isinstance(value, BaseException):
                raise value
            return dict(value)
        path = pathlib.Path(expected_path)
        metadata = path.stat()
        seconds, microseconds = self.process_start_times[pid]
        value = {
            "pid": pid,
            "start_time_seconds": seconds,
            "start_time_microseconds": microseconds,
            "uid": metadata.st_uid,
            "path": str(path),
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "size": metadata.st_size,
            "mode": stat.S_IMODE(metadata.st_mode),
            "device": metadata.st_dev,
            "inode": metadata.st_ino,
            "links": metadata.st_nlink,
        }
        key = str(path)
        count = self.process_identity_call_counts.get(key, 0) + 1
        self.process_identity_call_counts[key] = count
        if self.process_identity_hook is not None:
            return self.process_identity_hook(pid, path, count, value)
        return value

    def candidate_process_stopped(self, pid, expected_path, expected_identity):
        if self.candidate_process_identity(pid, expected_path) != expected_identity:
            ORCHESTRATOR.fail("candidate_process_suspend_identity_drift")
        return pid in self.suspended_pids

    def candidate_process_suspend(self, pid, expected_path, expected_identity):
        if self.candidate_process_stopped(pid, expected_path, expected_identity):
            return
        self.suspended_pids.add(pid)
        self.signals.append((pid, "SIGSTOP"))
        if self.candidate_process_identity(pid, expected_path) != expected_identity:
            ORCHESTRATOR.fail("candidate_process_suspend_identity_drift")

    def worker_health_status(self, context, timeout_seconds=3):
        self.lifecycle_events.append("health:worker")
        if self.health_failure == "worker":
            return 503
        return 200

    def worker_health_snapshot(self, context, timeout_seconds=3):
        if self.worker_health_status(context, timeout_seconds) != 200:
            ORCHESTRATOR.fail("worker_health_unready")
        return {
            "schema_version": 1,
            "status": "ok",
            **context.manifest["authoring"],
            "codex_cli_version": "codex-cli 1.2.3",
            "instance_id": self.worker_instance_id,
            "worker_source_sha256": context.manifest["source_trees"][
                "codex_worker"
            ]["sha256"],
            "concurrency_limit": 1,
            "queue_capacity": 4,
            "request_timeout_ms": 55000,
            "active_requests": self.worker_active_requests,
            "queued_requests": self.worker_queued_requests,
            "accepted_requests_total": self.worker_accepted_requests,
            "settled_requests_total": self.worker_settled_requests,
            "last_successful_request_id": (
                "worker-request-1" if self.worker_accepted_requests > 0 else None
            ),
            "last_successful_completion_sha256": (
                "b" * 64 if self.worker_accepted_requests > 0 else None
            ),
        }

    def transport_health_status(self, context, timeout_seconds=3):
        self.lifecycle_events.append("health:transport")
        if self.health_failure == "transport":
            return 503
        return 200

    def transport_control(self, context, command, fields=None, timeout_seconds=3):
        if self.transport_state is None:
            self.transport_state = {
                "version": 3,
                "ready": True,
                "run_id": context.manifest["run_id"],
                "guild_id": context.manifest["discord"]["guild_id"],
                "hub_channel_id": context.manifest["discord"]["hub_channel_id"],
                "actor_id": context.manifest["discord"]["actor_id"],
                "bot_user_id": context.manifest["discord"]["bot_user_id"],
                "instance_id": "d2ti-0123456789abcdef0123456789abcdef",
                "gateway": {
                    "partitioned": False,
                    "connections": 1,
                    "active_connections": 1,
                    "completed_connections": 0,
                    "clean_close_relays": 0,
                    "relay_failures": 0,
                    "connection_aborts": 0,
                    "ready_rewrites": 1,
                    "partition_events": 0,
                    "identity_rejections": 0,
                    "duplicate_armed": False,
                    "armed_duplicate_operation_id": None,
                    "duplicate_claimed": False,
                    "claimed_duplicate_operation_id": None,
                    "duplicate_injections": 0,
                    "duplicate_failed_attempts": 0,
                    "last_failed_duplicate_operation_id": None,
                    "duplicate_delivery_count": 0,
                    "last_duplicate_interaction_id": None,
                    "last_duplicate_operation_id": None,
                },
                "effect_http": {
                    "forwarded_requests": 0,
                    "rejected_requests": 0,
                    "indeterminate_armed": False,
                    "armed_indeterminate_operation_id": None,
                    "indeterminate_claimed": False,
                    "claimed_indeterminate_operation_id": None,
                    "indeterminate_injections": 0,
                    "last_indeterminate_audit_reason_sha256": None,
                    "last_indeterminate_operation_id": None,
                    "last_indeterminate_upstream_status": None,
                    "owned_role_count": 0,
                    "owned_channel_count": 0,
                    "owned_message_count": 0,
                },
            }
        fields = fields or {}
        if command == "snapshot":
            return json.loads(json.dumps(self.transport_state))
        if command == "resource_inventory":
            return self.resource_inventory(context)
        gateway = self.transport_state["gateway"]
        effect = self.transport_state["effect_http"]
        if command == "arm_next_duplicate":
            operation_id = fields["operation_id"]
            if gateway["duplicate_armed"]:
                return {
                    "changed": False,
                    "disposition": "replayed"
                    if gateway["armed_duplicate_operation_id"] == operation_id
                    else "busy",
                }
            gateway["duplicate_armed"] = True
            gateway["armed_duplicate_operation_id"] = operation_id
            return {"changed": True, "disposition": "armed"}
        if command == "disarm_duplicate":
            changed = gateway["duplicate_armed"]
            gateway["duplicate_armed"] = False
            gateway["armed_duplicate_operation_id"] = None
            return {"changed": changed}
        if command == "arm_next_create_role_indeterminate":
            operation_id = fields["operation_id"]
            if effect["indeterminate_armed"] or effect["indeterminate_claimed"]:
                return {
                    "changed": False,
                    "disposition": "replayed"
                    if effect["armed_indeterminate_operation_id"] == operation_id
                    else "busy",
                }
            effect["indeterminate_armed"] = True
            effect["armed_indeterminate_operation_id"] = operation_id
            return {"changed": True, "disposition": "armed"}
        if command == "disarm_indeterminate":
            changed = effect["indeterminate_armed"]
            effect["indeterminate_armed"] = False
            effect["armed_indeterminate_operation_id"] = None
            return {"changed": changed}
        if command == "partition_gateway":
            if gateway["partitioned"]:
                return {"changed": False}
            gateway["partitioned"] = True
            gateway["partition_events"] += 1
            return {"changed": True}
        if command == "heal_gateway":
            changed = gateway["partitioned"]
            gateway["partitioned"] = False
            return {"changed": changed}
        raise AssertionError(command)

    def resource_inventory(self, context):
        history = sorted(
            (dict(entry) for entry in self.resource_history),
            key=lambda entry: (
                entry["kind"],
                entry["resource_id"],
                entry.get("channel_id", ""),
            ),
        )
        created = []
        deleted = []
        active = []
        for entry in history:
            identity = {"kind": entry["kind"], "resource_id": entry["resource_id"]}
            if entry["kind"] == "message":
                identity["channel_id"] = entry["channel_id"]
            created.append(identity)
            if entry["state"] == "deleted":
                deleted.append(identity)
            else:
                active.append(identity)
        payload = {
            "version": 1,
            "kind": "starring.d2.run-owned-resource-inventory.v1",
            "instance_id": self.transport_state["instance_id"],
            "run_id": context.manifest["run_id"],
            "guild_id": context.manifest["discord"]["guild_id"],
            "hub_channel_id": context.manifest["discord"]["hub_channel_id"],
            "actor_id": context.manifest["discord"]["actor_id"],
            "bot_user_id": context.manifest["discord"]["bot_user_id"],
            "history_limit": 128,
            "history": history,
            "created": created,
            "deleted": deleted,
            "active": active,
        }
        encoded = json.dumps(payload, separators=(",", ":")).encode("ascii")
        return {**payload, "digest_sha256": hashlib.sha256(encoded).hexdigest()}

    def discord_delete_resource_through_transport(
        self, context, resource, inventory=None, timeout_seconds=10
    ):
        inventory = inventory or self.resource_inventory(context)
        self.proxy_deletions.append(dict(resource))
        fail_before = (
            resource["resource_id"] == self.proxy_failure_resource_id
            and not self.proxy_failure_after_delete
        )
        if fail_before:
            ORCHESTRATOR.fail("injected_proxy_delete_failure")
        existed = resource["resource_id"] in self.discord_existing
        for entry in self.resource_history:
            if (
                entry["kind"] == resource["kind"]
                and entry["resource_id"] == resource["resource_id"]
                and entry.get("channel_id") == resource.get("channel_id")
            ):
                entry["state"] = "deleted"
        self.discord_existing.discard(resource["resource_id"])
        if resource["kind"] == "channel":
            for entry in self.resource_history:
                if entry["kind"] == "message" and entry["channel_id"] == resource[
                    "resource_id"
                ]:
                    entry["state"] = "deleted"
                    self.discord_existing.discard(entry["resource_id"])
        if (
            resource["resource_id"] == self.proxy_failure_resource_id
            and self.proxy_failure_after_delete
        ):
            ORCHESTRATOR.fail("injected_proxy_delete_lost_response")
        success = {"role": 204, "channel": 200, "message": 204}[
            resource["kind"]
        ]
        unknown = {"role": 10011, "channel": 10003, "message": 10008}[
            resource["kind"]
        ]
        return {
            "schema_version": 1,
            "kind": "starring.d2.discord-resource-proxy-deletion.v1",
            "transport_instance_id": inventory["instance_id"],
            "inventory_digest_sha256": inventory["digest_sha256"],
            "resource_kind": resource["kind"],
            "resource_id": resource["resource_id"],
            "channel_id": resource.get("channel_id"),
            "http_status": success if existed else 404,
            "discord_code": None if existed else unknown,
            "deleted": existed,
        }

    def discord_observe_resource(
        self, context, resource, inventory=None, timeout_seconds=10
    ):
        inventory = inventory or self.resource_inventory(context)
        exists = resource["resource_id"] in self.discord_existing
        if exists or resource["kind"] == "role":
            status = 200
            code = None
        else:
            status = 404
            if resource["kind"] == "message" and resource[
                "channel_id"
            ] not in self.discord_existing:
                code = 10003
            else:
                code = {"channel": 10003, "message": 10008}[resource["kind"]]
        return {
            "schema_version": 1,
            "kind": "starring.d2.discord-resource-observation.v1",
            "transport_instance_id": inventory["instance_id"],
            "inventory_digest_sha256": inventory["digest_sha256"],
            "resource_kind": resource["kind"],
            "resource_id": resource["resource_id"],
            "channel_id": resource.get("channel_id"),
            "http_status": status,
            "discord_code": code,
            "exists": exists,
        }

    def wait_for_status(self, probe, expected, timeout_seconds=60):
        return probe()

    def launchd_signal(self, label, signal_name):
        if self.signal_failure:
            ORCHESTRATOR.fail("injected_launchd_signal_failure")
        self.signals.append((label, signal_name))
        self.exit_launchd(label, self.signal_exit_code)

    def launchd_bootout(self, label):
        self.bootouts.append(label)
        self.loaded.discard(label)
        pid = self.pids.pop(label, None)
        if pid is not None:
            self.suspended_pids.discard(pid)
        self.programs.pop(label, None)
        self.plist_paths.pop(label, None)
        self.program_arguments.pop(label, None)
        self.launchd_runs.pop(label, None)
        self.launchd_states.pop(label, None)
        self.last_exit_codes.pop(label, None)
        self.runtime_process_instance_ids.pop(label, None)


class D2IsolatedOrchestratorTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name).resolve()
        self.registry_path = self.root / "discord-ownership-registry.json"
        self.runtime_root_parent = self.root / "runtime-roots"
        self.runtime_root_parent.mkdir(mode=0o700)
        self.isolated_root_parent_patch = None
        test_runtime_parent = os.environ.get("STARRING_D2_TEST_RUNTIME_PARENT")
        if test_runtime_parent is not None:
            parent = pathlib.Path(test_runtime_parent)
            temporary_parent = pathlib.Path(
                os.environ.get("TMPDIR", "")
            )
            try:
                resolved_parent = parent.resolve(strict=True)
                resolved_temporary = temporary_parent.resolve(strict=True)
                metadata = resolved_parent.lstat()
            except OSError as error:
                self.fail(f"test_runtime_parent_unavailable:{error.__class__.__name__}")
            if (
                resolved_parent != parent
                or resolved_parent == resolved_temporary
                or resolved_temporary not in resolved_parent.parents
                or not stat.S_ISDIR(metadata.st_mode)
                or metadata.st_uid != os.getuid()
                or stat.S_IMODE(metadata.st_mode) != 0o700
            ):
                self.fail("test_runtime_parent_invalid")
            self.isolated_root_parent_patch = mock.patch.object(
                CERTIFICATION,
                "D2_ISOLATED_ROOT_PARENT",
                resolved_parent,
            )
            self.isolated_root_parent_patch.start()
        self.registry_patch = mock.patch.object(
            CONTRACT,
            "GLOBAL_DISCORD_OWNERSHIP_REGISTRY_PATH",
            self.registry_path,
        )
        self.runtime_root_patch = mock.patch.object(
            CONTRACT,
            "D2_RUNTIME_ROOT_PARENT",
            self.runtime_root_parent,
        )
        self.registry_patch.start()
        self.runtime_root_patch.start()
        self.artifact_root = self.root / "immutable-candidates"
        self.artifact_root.mkdir()
        self.run_id = f"d2-20260801t120000z-{secrets.token_hex(6)}"
        self.isolated_root = CERTIFICATION.isolated_runtime_root(self.run_id)
        self.candidates = {}
        for name in CERTIFICATION.REQUIRED_CANDIDATES:
            path = (
                self.artifact_root / "worker-tree" / "worker.mjs"
                if name == "codex_worker"
                else self.artifact_root / name
            )
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(f"candidate:{name}".encode())
            self.candidates[name] = path
        for name in CERTIFICATION.CODEX_WORKER_SOURCE_FILES:
            path = self.candidates["codex_worker"].parent / name
            if not path.exists():
                path.write_bytes(f"worker-source:{name}".encode())
        for path in self.artifact_root.rglob("*"):
            if path.is_file():
                path.chmod(0o555)
        for path in sorted(
            (path for path in self.artifact_root.rglob("*") if path.is_dir()),
            reverse=True,
        ):
            path.chmod(0o555)
        self.artifact_root.chmod(0o555)
        self.manifest_path = self.prepare_manifest()
        self.context = ORCHESTRATOR.load_context(self.manifest_path)
        self.platform = FakePlatform()
        self.live_clock = 0.0
        self.live_monotonic = mock.patch.object(
            LIVE_RUNTIME_RESTART,
            "monotonic_time",
            side_effect=lambda: self.live_clock,
        )
        self.live_sleep = mock.patch.object(
            LIVE_RUNTIME_RESTART,
            "wait_interval",
            side_effect=self.advance_live_clock,
        )
        self.live_monotonic.start()
        self.live_sleep.start()

    def tearDown(self):
        self.live_sleep.stop()
        self.live_monotonic.stop()
        self.remove_test_root(self.context)
        for path in self.artifact_root.rglob("*"):
            if path.is_dir():
                path.chmod(0o700)
        self.artifact_root.chmod(0o700)
        self.runtime_root_patch.stop()
        self.registry_patch.stop()
        if self.isolated_root_parent_patch is not None:
            self.isolated_root_parent_patch.stop()
        self.temporary.cleanup()

    def remove_test_root(self, context):
        if not context.root.exists():
            return
        self.platform.postgres = False
        self.platform.postgres_process_pid = None
        self.platform.postgres_observation_error = False
        self.platform.launchd_observation_error = False
        for service in context.manifest["services"].values():
            self.platform.loaded.discard(service["label"])
        ORCHESTRATOR.guarded_remove_root(context, self.platform)

    def advance_live_clock(self, seconds):
        self.live_clock += seconds

    def start_candidate_with_discord_resources(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        hub_channel_id = self.context.manifest["discord"]["hub_channel_id"]
        self.platform.resource_history = [
            {
                "kind": "channel",
                "resource_id": "1524810437118525560",
                "state": "created",
            },
            {
                "kind": "channel",
                "resource_id": "1524810437118525561",
                "state": "created",
            },
            {
                "kind": "message",
                "resource_id": "1524810437118525570",
                "channel_id": "1524810437118525560",
                "state": "created",
            },
            {
                "kind": "message",
                "resource_id": "1524810437118525571",
                "channel_id": hub_channel_id,
                "state": "created",
            },
            {
                "kind": "role",
                "resource_id": "1524810437118525580",
                "state": "created",
            },
            {
                "kind": "role",
                "resource_id": "1524810437118525581",
                "state": "created",
            },
        ]
        self.platform.discord_existing = {
            entry["resource_id"] for entry in self.platform.resource_history
        } | {hub_channel_id}
        return self.platform.resource_inventory(self.context)

    def assert_candidate_identity_failure_rolls_back(self, configure, pattern):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        configure()
        with self.assertRaisesRegex(CONTRACT.OrchestratorError, pattern):
            ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertFalse(self.platform.postgres)
        self.assertTrue(
            all(
                service["label"] not in self.platform.loaded
                for service in self.context.manifest["services"].values()
            )
        )
        self.assertEqual(
            CONTRACT.load_state(self.context, {"stopped"})["phase"], "stopped"
        )
        self.assertFalse(
            (self.context.artifact_directory / "step-03-evidence.json").exists()
        )
        self.assertFalse(
            D2_SOURCE_CONTRACT.source_path(
                self.context, 3, "candidate"
            ).exists()
        )

    def prepare_manifest(
        self,
        guild_id="1524810437118525551",
        application_id="1524810437118525552",
        bot_user_id="1524810437118525553",
    ):
        arguments = [
            "prepare",
            "--output-root",
            str(self.root / "evidence"),
            "--commit",
            "a" * 40,
            "--discord-guild-id",
            guild_id,
            "--discord-hub-channel-id",
            "1524810437118525554",
            "--discord-application-id",
            application_id,
            "--discord-bot-user-id",
            bot_user_id,
            "--discord-actor-id",
            "1056857223529250906",
            "--discord-oauth-keychain",
            "starring.d2.credentials:discord.oauth-client-secret",
            "--discord-bot-keychain",
            "starring.d2.credentials:discord.bot-token",
            "--tunnel-token-keychain",
            "starring.d2.credentials:cloudflare.tunnel-token",
            "--cloudflare-tunnel-id",
            CERTIFICATION.D2_CLOUDFLARE_TUNNEL_ID,
            "--public-origin",
            "https://d2-api.starring.co.kr",
            "--run-id",
            self.run_id,
        ]
        for name, path in sorted(self.candidates.items()):
            arguments.extend(("--candidate", f"{name}={path}"))
        for name, port in {
            "postgres": 55433,
            "api": 28080,
            "runtime": 29091,
            "worker": 28181,
            "transport_gateway": 29101,
            "transport_http": 29102,
        }.items():
            arguments.extend(("--port", f"{name}={port}"))
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(CERTIFICATION.main(arguments), 0)
        return pathlib.Path(json.loads(output.getvalue())["manifest"])

    def prepare_additional_context(
        self,
        guild_id,
        application_id,
        bot_user_id,
    ):
        original_run_id = self.run_id
        self.run_id = f"d2-20260801t120000z-{secrets.token_hex(6)}"
        try:
            manifest_path = self.prepare_manifest(
                guild_id=guild_id,
                application_id=application_id,
                bot_user_id=bot_user_id,
            )
        finally:
            self.run_id = original_run_id
        context = ORCHESTRATOR.load_context(manifest_path)
        self.addCleanup(
            lambda: self.remove_test_root(context)
        )
        return context

    def record_prerequisite_receipts(self, coordinator=True):
        evidence_by_step = complete_certification_evidence(self.context.manifest)
        for step in range(1, 11):
            path = self.root / f"live-restart-step-{step}.json"
            path.write_text(
                json.dumps(evidence_by_step[step]), encoding="utf-8"
            )
            path.chmod(0o600)
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(
                    CERTIFICATION.main(
                        [
                            "record",
                            "--manifest",
                            str(self.manifest_path),
                            "--step",
                            str(step),
                            "--evidence",
                            str(path),
                        ]
                    ),
                    0,
                )
        if not coordinator:
            return
        receipts = D2_RUN.load_receipts(
            self.manifest_path, self.context.manifest, self.context.digest
        )
        D2_RUN.ensure_coordinator_directory(self.manifest_path)
        for step in range(1, 11):
            intent = {
                "schema_version": 1,
                "kind": D2_RUN.COORDINATOR_INTENT_KIND,
                "run_id": self.context.manifest["run_id"],
                "manifest_sha256": self.context.digest,
                "step": step,
                "code": D2_RUN.STEP_SPECS[step].code,
                "observed_at": "2026-08-04T01:02:03Z",
                "receipt_chain_head_sha256": (
                    D2_RUN.ZERO_DIGEST
                    if step == 1
                    else receipts[step - 2]["receipt_sha256"]
                ),
                "sources": [
                    {
                        "kind": specification["kind"],
                        "sha256": hashlib.sha256(
                            f"{step}:{specification['kind']}".encode("utf-8")
                        ).hexdigest(),
                    }
                    for specification in D2_RUN.STEP_SOURCE_SPECS[step]
                ],
            }
            D2_RUN.write_private_json(
                D2_RUN.coordinator_intent_path(self.manifest_path, step), intent
            )
            completion = {
                "schema_version": 1,
                "kind": D2_RUN.COORDINATOR_COMPLETION_KIND,
                "run_id": self.context.manifest["run_id"],
                "manifest_sha256": self.context.digest,
                "step": step,
                "code": D2_RUN.STEP_SPECS[step].code,
                "observed_at": "2026-08-04T01:02:03Z",
                "intent_sha256": D2_RUN.intent_digest(intent),
                "receipt_sha256": receipts[step - 1]["receipt_sha256"],
                "receipt_disposition": "created",
            }
            D2_RUN.write_private_json(
                D2_RUN.coordinator_completion_path(self.manifest_path, step),
                completion,
            )

    def write_live_restart_confirmation(self, awaiting, overrides=None):
        boundary = datetime.datetime.fromisoformat(
            awaiting["shutdown_boundary"].replace("Z", "+00:00")
        )
        heartbeat = boundary + datetime.timedelta(microseconds=1)
        observed = boundary + datetime.timedelta(microseconds=2)
        expires = heartbeat + datetime.timedelta(seconds=45)
        confirmation = {
            "schema_version": 1,
            "kind": LIVE_RUNTIME_RESTART.CONFIRMATION_KIND,
            "checkpoint": "live_fresh_lease",
            "operation_id": awaiting["operation_id"],
            "installation_id": awaiting["installation_id"],
            "promotion_id": awaiting["promotion_id"],
            "public_origin": awaiting["public_origin"],
            "shutdown_boundary": awaiting["shutdown_boundary"],
            "observed_at": observed.isoformat().replace("+00:00", "Z"),
            "product_state": "live",
            "operational_state": "live",
            "runtime_phase": "live",
            "serving_state": "fresh",
            "attestation_revision": 11,
            "process_instance_id": awaiting["process_instance_id"],
            "last_heartbeat_at": heartbeat.isoformat().replace(
                "+00:00", "Z"
            ),
            "lease_expires_at": expires.isoformat().replace(
                "+00:00", "Z"
            ),
        }
        confirmation.update(overrides or {})
        path = self.root / "live-runtime-restart-confirmation.json"
        path.write_text(json.dumps(confirmation), encoding="utf-8")
        path.chmod(0o600)
        return path

    def certify_and_advance_live_restart(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        awaiting = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        result = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context,
            self.platform,
            self.write_live_restart_confirmation(awaiting),
        )
        D2_RUN.advance_certification(
            self.manifest_path, 11, [result["coordinator_source"]]
        )
        return result

    def test_dry_run_is_read_only_and_binds_dedicated_namespaces(self):
        result = ORCHESTRATOR.command_dry_run(self.context, self.platform)
        self.assertEqual(result["status"], "ready")
        self.assertFalse(result["standing_mutation_allowed"])
        self.assertEqual(self.platform.keychain_writes, [])
        self.assertEqual(self.platform.bootouts, [])
        self.assertFalse(self.isolated_root.exists())

    def test_restart_drained_runtime_parser_requires_manifest(self):
        arguments = ORCHESTRATOR.build_parser().parse_args(
            ["restart-drained-runtime", "--manifest", str(self.manifest_path)]
        )
        self.assertEqual(arguments.command, "restart-drained-runtime")
        self.assertEqual(arguments.manifest, str(self.manifest_path))

    def test_certify_live_runtime_restart_parser_requires_manifest(self):
        arguments = ORCHESTRATOR.build_parser().parse_args(
            [
                "certify-live-runtime-restart",
                "--manifest",
                str(self.manifest_path),
            ]
        )
        self.assertEqual(arguments.command, "certify-live-runtime-restart")
        self.assertEqual(arguments.manifest, str(self.manifest_path))
        self.assertIsNone(arguments.confirmation_file)
        confirmation = self.root / "confirmation.json"
        arguments = ORCHESTRATOR.build_parser().parse_args(
            [
                "certify-live-runtime-restart",
                "--manifest",
                str(self.manifest_path),
                "--confirmation-file",
                str(confirmation),
            ]
        )
        self.assertEqual(arguments.confirmation_file, str(confirmation))

    def test_atomic_write_fsyncs_created_parent_and_renamed_entry(self):
        path = self.root / "atomic" / "state.json"
        observed = []
        real_fsync_directory = CONTRACT.fsync_directory

        def record(directory, label):
            observed.append((pathlib.Path(directory), label))
            real_fsync_directory(directory, label)

        with mock.patch.object(CONTRACT, "fsync_directory", side_effect=record):
            CONTRACT.write_atomic(path, "{}\n")
        self.assertEqual(
            observed,
            [
                (self.root, "atomic_parent_parent"),
                (path.parent, "atomic_parent"),
            ],
        )
        self.assertEqual(path.read_text(encoding="utf-8"), "{}\n")
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_first_journal_append_fsyncs_artifact_lock_and_entry_directories(self):
        observed = []
        real_fsync_directory = CONTRACT.fsync_directory

        def record(directory, label):
            observed.append((pathlib.Path(directory), label))
            real_fsync_directory(directory, label)

        with mock.patch.object(CONTRACT, "fsync_directory", side_effect=record):
            CONTRACT.append_journal(self.context, "test_action", "complete", "test")
        self.assertEqual(
            observed,
            [
                (self.context.run_directory, "journal_artifact_parent"),
                (self.context.artifact_directory, "journal_lock_parent"),
                (self.context.artifact_directory, "journal_entry_parent"),
            ],
        )
        self.assertEqual(self.context.lock_path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(self.context.journal_path.stat().st_mode & 0o777, 0o600)

    def test_cleanup_recovers_a_partial_final_journal_row(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        descriptor = os.open(self.context.journal_path, os.O_WRONLY | os.O_APPEND)
        try:
            os.write(descriptor, b'{"schema_version":1,"sequence":')
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
        result = ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(result["phase"], "cleaned")
        raw = self.context.journal_path.read_bytes()
        self.assertTrue(raw.endswith(b"\n"))
        rows = CONTRACT.parse_journal_rows(self.context, raw)
        self.assertEqual([row["sequence"] for row in rows], list(range(1, len(rows) + 1)))
        self.assertEqual(rows[-1]["action"], "cleanup")
        self.assertEqual(rows[-1]["status"], "complete")

    def test_drained_restart_reader_recovers_a_partial_final_journal_row(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        descriptor = os.open(self.context.journal_path, os.O_WRONLY | os.O_APPEND)
        try:
            os.write(descriptor, b'{"schema_version":1,"sequence":')
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
        found = DRAINED_RUNTIME_RESTART.drained_runtime_restart_journal_contains(
            self.context,
            "complete",
            "d2:0123456789abcdef:restart-drained-runtime:0001",
        )
        self.assertFalse(found)
        raw = self.context.journal_path.read_bytes()
        self.assertTrue(raw.endswith(b"\n"))
        CONTRACT.parse_journal_rows(self.context, raw)

    def test_live_restart_reader_recovers_a_partial_final_journal_row(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        descriptor = os.open(self.context.journal_path, os.O_WRONLY | os.O_APPEND)
        try:
            os.write(descriptor, b'{"schema_version":1,"sequence":')
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
        found = LIVE_RUNTIME_RESTART.live_runtime_restart_journal_contains(
            self.context,
            "complete",
            "d2:0123456789abcdef:certify-live-runtime-restart",
        )
        self.assertFalse(found)
        raw = self.context.journal_path.read_bytes()
        self.assertTrue(raw.endswith(b"\n"))
        CONTRACT.parse_journal_rows(self.context, raw)

    def test_journal_rejects_boolean_integer_fields(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        original = json.loads(
            self.context.journal_path.read_text(encoding="utf-8").splitlines()[0]
        )
        for field in ("schema_version", "sequence"):
            with self.subTest(field=field):
                changed = dict(original)
                changed[field] = True
                raw = (
                    json.dumps(changed, sort_keys=True, separators=(",", ":"))
                    + "\n"
                ).encode("utf-8")
                with self.assertRaisesRegex(
                    CONTRACT.OrchestratorError, "journal_invalid"
                ):
                    CONTRACT.parse_journal_rows(self.context, raw)

    def test_prepare_start_stop_cleanup_is_idempotent_and_preserves_staging(self):
        prepared = ORCHESTRATOR.command_prepare(self.context, self.platform)
        self.assertEqual(prepared["phase"], "prepared")
        registry = CONTRACT.load_discord_ownership_registry()
        self.assertEqual(
            registry["owners"],
            [CONTRACT.discord_ownership_record(self.context)],
        )
        self.assertEqual(self.registry_path.stat().st_mode & 0o777, 0o600)
        replayed_prepare = ORCHESTRATOR.command_prepare(self.context, self.platform)
        self.assertEqual(replayed_prepare["status"], "already_prepared")
        self.assertTrue((self.isolated_root / "postgres" / "PG_VERSION").is_file())
        self.assertEqual(len(self.platform.keychain_writes), 4)
        for name in ("api", "runtime", "worker", "transport", "tunnel"):
            label = self.context.manifest["services"][name]["label"]
            plist_path = self.context.plist_directory / f"{label}.plist"
            self.assertTrue(plist_path.is_file())
            self.assertNotIn(
                label, self.context.manifest["protected_staging"]["launchd_labels"]
            )
        started = ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertEqual(started["phase"], "candidate_started")
        self.assertEqual(set(started["coordinator_sources"]), {"1", "3"})
        for source in started["coordinator_sources"].values():
            self.assertEqual(pathlib.Path(source).stat().st_mode & 0o777, 0o600)
        self.assertTrue(started["candidate_services_loaded"])
        self.assertTrue(started["database_schema_ready"])
        self.assertTrue(
            (self.context.artifact_directory / "database-evidence.json").is_file()
        )
        step_one = json.loads(
            (self.context.artifact_directory / "step-01-evidence.json").read_text(
                encoding="utf-8"
            )
        )
        self.assertEqual(
            step_one["discord_resource_prefix"],
            self.context.manifest["discord"]["resource_prefix"],
        )
        self.assertTrue(
            (self.context.artifact_directory / "step-03-evidence.json").is_file()
        )
        step_three = json.loads(
            (self.context.artifact_directory / "step-03-evidence.json").read_text(
                encoding="utf-8"
            )
        )
        self.assertTrue(step_three["transport_ready"])
        self.assertEqual(
            step_three["transport_instance_id"],
            "d2ti-0123456789abcdef0123456789abcdef",
        )
        self.assertEqual(
            step_three["certification_transport_sha256"],
            self.context.manifest["candidates"]["certification_transport"]["sha256"],
        )
        self.assertEqual(
            set(step_three["process_identities"]),
            {"schema_version", "api", "runtime"},
        )
        self.assertEqual(step_three["process_identities"]["schema_version"], 1)
        self.assertNotEqual(
            step_three["process_identities"]["api"]["process"]["pid"],
            step_three["process_identities"]["runtime"]["process"]["pid"],
        )
        self.assertEqual(
            step_three["process_identities"]["runtime"]["runtime_health"][
                "os_pid"
            ],
            step_three["process_identities"]["runtime"]["process"]["pid"],
        )
        self.assertEqual(
            {
                path: self.platform.process_identity_call_counts[str(path)]
                for path in (self.candidates["api"], self.candidates["runtime"])
            },
            {self.candidates["api"]: 7, self.candidates["runtime"]: 7},
        )
        self.assertEqual(
            self.platform.start_order,
            [
                self.context.manifest["services"][name]["label"]
                for name in ORCHESTRATOR.SERVICE_START_ORDER
            ],
        )
        self.assertIn(
            (
                f"http://127.0.0.1:{self.context.manifest['services']['api']['port']}/health/ready",
                "d2-api.starring.co.kr",
            ),
            self.platform.http_probes,
        )
        api_label = self.context.manifest["services"]["api"]["label"]
        worker_label = self.context.manifest["services"]["worker"]["label"]
        self.assertLess(
            self.platform.lifecycle_events.index("health:worker"),
            self.platform.lifecycle_events.index(f"start:{api_label}"),
        )
        self.assertLess(
            self.platform.lifecycle_events.index("health:transport"),
            self.platform.lifecycle_events.index(f"start:{worker_label}"),
        )
        stopped = ORCHESTRATOR.command_stop(self.context, self.platform)
        self.assertEqual(stopped["phase"], "stopped")
        cleaned = ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(cleaned["phase"], "cleaned")
        self.assertFalse(self.isolated_root.exists())
        self.assertTrue(cleaned["protected_staging_unchanged"])
        self.assertTrue(
            all(label in self.platform.loaded for label in self.context.manifest["protected_staging"]["launchd_labels"])
        )
        again = ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(again["status"], "already_cleaned")
        self.assertEqual(CONTRACT.load_discord_ownership_registry()["owners"], [])

    def test_candidate_start_rejects_foreign_runtime_health_pid_and_rolls_back(self):
        def configure():
            self.platform.runtime_identity_override = {
                "schema_version": 1,
                "os_pid": 999,
                "process_instance_id": "f" * 32,
            }

        self.assert_candidate_identity_failure_rolls_back(
            configure, "candidate_runtime_health_identity_mismatch"
        )

    def test_candidate_start_rejects_process_start_drift_and_rolls_back(self):
        def configure():
            def hook(_pid, path, count, value):
                observed = dict(value)
                if path == self.candidates["api"] and count == 2:
                    observed["start_time_microseconds"] += 1
                return observed

            self.platform.process_identity_hook = hook

        self.assert_candidate_identity_failure_rolls_back(
            configure, "candidate_api_process_identity_drift"
        )

    def test_candidate_start_rejects_process_digest_and_rolls_back(self):
        def configure():
            def hook(_pid, path, _count, value):
                observed = dict(value)
                if path == self.candidates["api"]:
                    observed["sha256"] = "0" * 64
                return observed

            self.platform.process_identity_hook = hook

        self.assert_candidate_identity_failure_rolls_back(
            configure, "candidate_api_process_digest_mismatch"
        )

    def test_candidate_start_rejects_api_runtime_pid_collision(self):
        api_label = self.context.manifest["services"]["api"]["label"]
        runtime_label = self.context.manifest["services"]["runtime"]["label"]

        def configure():
            original = self.platform.launchd_start

            def launch(label, plist_path):
                original(label, plist_path)
                if label == runtime_label:
                    self.platform.pids[label] = self.platform.pids[api_label]

            self.platform.launchd_start = launch

        self.assert_candidate_identity_failure_rolls_back(
            configure, "candidate_process_pid_collision"
        )

    def test_candidate_start_rejects_launchd_runs_drift_and_rolls_back(self):
        api_label = self.context.manifest["services"]["api"]["label"]

        def configure():
            def hook(label, count, value):
                observed = dict(value)
                if label == api_label and count == 2:
                    observed["runs"] += 1
                return observed

            self.platform.launchd_job_hook = hook

        self.assert_candidate_identity_failure_rolls_back(
            configure, "candidate_api_launchd_identity_drift"
        )

    def test_candidate_start_rejects_identity_window_readiness_loss(self):
        api_ready = (
            f"http://127.0.0.1:{self.context.manifest['services']['api']['port']}"
            "/health/ready"
        )

        def configure():
            self.platform.http_status_hook = (
                lambda url, count, status: 503
                if url == api_ready and count == 2
                else status
            )

        self.assert_candidate_identity_failure_rolls_back(
            configure, "candidate_api_health_identity_unready"
        )

    def test_candidate_start_rejects_api_loss_during_runtime_observation(self):
        api_label = self.context.manifest["services"]["api"]["label"]

        def configure():
            def hook(_pid, path, count, value):
                if path == self.candidates["runtime"] and count == 1:
                    self.platform.exit_launchd(api_label, 1)
                return value

            self.platform.process_identity_hook = hook

        self.assert_candidate_identity_failure_rolls_back(
            configure, "candidate_api_launchd_final_identity_drift"
        )

    def test_candidate_start_rejects_plist_content_drift_and_rolls_back(self):
        api_label = self.context.manifest["services"]["api"]["label"]
        api_plist = self.context.plist_directory / f"{api_label}.plist"

        def configure():
            def hook(_pid, path, count, value):
                if path == self.candidates["api"] and count == 2:
                    api_plist.write_bytes(b"foreign plist")
                    api_plist.chmod(0o600)
                return value

            self.platform.process_identity_hook = hook

        self.assert_candidate_identity_failure_rolls_back(
            configure, "candidate_api_plist_content_mismatch"
        )

    def test_candidate_start_rejects_disappearing_process_and_rolls_back(self):
        def configure():
            def hook(_pid, path, count, value):
                if path == self.candidates["api"] and count == 2:
                    raise CONTRACT.OrchestratorError(
                        "process_identity_bsdinfo_unavailable"
                    )
                return value

            self.platform.process_identity_hook = hook

        self.assert_candidate_identity_failure_rolls_back(
            configure, "process_identity_bsdinfo_unavailable"
        )

    def test_candidate_plist_identity_rejects_mode_links_symlink_and_content(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        api_label = self.context.manifest["services"]["api"]["label"]
        path = self.context.plist_directory / f"{api_label}.plist"
        raw = path.read_bytes()
        baseline = ORCHESTRATOR.candidate_plist_identity(self.context, "api")
        self.assertEqual(baseline["sha256"], hashlib.sha256(raw).hexdigest())
        path.chmod(0o644)
        with self.assertRaisesRegex(
            CONTRACT.OrchestratorError, "candidate_api_plist_identity_invalid"
        ):
            ORCHESTRATOR.candidate_plist_identity(self.context, "api")
        path.chmod(0o600)
        path.write_bytes(b"foreign plist")
        with self.assertRaisesRegex(
            CONTRACT.OrchestratorError, "candidate_api_plist_content_mismatch"
        ):
            ORCHESTRATOR.candidate_plist_identity(self.context, "api")
        path.write_bytes(raw)
        path.chmod(0o600)
        backup = self.context.plist_directory / "api-plist-backup"
        backup.write_bytes(raw)
        backup.chmod(0o600)
        path.unlink()
        os.link(backup, path)
        with self.assertRaisesRegex(
            CONTRACT.OrchestratorError, "candidate_api_plist_identity_invalid"
        ):
            ORCHESTRATOR.candidate_plist_identity(self.context, "api")
        path.unlink()
        path.symlink_to(backup)
        with self.assertRaisesRegex(
            CONTRACT.OrchestratorError, "candidate_plist_invalid"
        ):
            ORCHESTRATOR.candidate_plist_identity(self.context, "api")
        path.unlink()
        backup.unlink()
        path.write_bytes(raw)
        path.chmod(0o600)

    def test_candidate_process_rejects_initial_launchd_mapping_drift(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        name = "api"
        label = self.context.manifest["services"][name]["label"]
        plist_path = ORCHESTRATOR.service_plist_path(self.context, name)
        self.platform.launchd_start(label, plist_path)
        originals = {
            "program": self.platform.programs[label],
            "plist_path": self.platform.plist_paths[label],
            "arguments": list(self.platform.program_arguments[label]),
            "runs": self.platform.launchd_runs[label],
            "state": self.platform.launchd_states[label],
        }
        mutations = (
            (self.platform.programs, "/private/tmp/foreign"),
            (self.platform.plist_paths, "/private/tmp/foreign.plist"),
            (self.platform.program_arguments, []),
            (self.platform.launchd_runs, True),
            (self.platform.launchd_states, "exited"),
        )
        fields = ("program", "plist_path", "arguments", "runs", "state")
        for field, (mapping, replacement) in zip(fields, mutations):
            with self.subTest(field=field):
                mapping[label] = replacement
                with self.assertRaisesRegex(
                    CONTRACT.OrchestratorError,
                    "candidate_api_launchd_identity_invalid",
                ):
                    ORCHESTRATOR.observe_candidate_process(
                        self.context, self.platform, name
                    )
                mapping[label] = originals[field]

    def test_prepare_rejects_other_run_with_same_guild_or_application(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        same_guild = self.prepare_additional_context(
            guild_id=self.context.manifest["discord"]["guild_id"],
            application_id="1524810437118525602",
            bot_user_id="1524810437118525603",
        )
        same_application = self.prepare_additional_context(
            guild_id="1524810437118525611",
            application_id=self.context.manifest["discord"]["application_id"],
            bot_user_id="1524810437118525613",
        )
        keychain_write_count = len(self.platform.keychain_writes)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "discord_guild_owned_by_other_d2_run",
        ):
            ORCHESTRATOR.command_prepare(same_guild, self.platform)
        self.assertFalse(same_guild.artifact_directory.exists())
        self.assertFalse(same_guild.root.exists())
        self.assertEqual(len(self.platform.keychain_writes), keychain_write_count)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "discord_application_owned_by_other_d2_run",
        ):
            ORCHESTRATOR.command_prepare(same_application, self.platform)
        self.assertFalse(same_application.artifact_directory.exists())
        self.assertFalse(same_application.root.exists())
        self.assertEqual(len(self.platform.keychain_writes), keychain_write_count)
        self.assertEqual(
            CONTRACT.load_discord_ownership_registry()["owners"],
            [CONTRACT.discord_ownership_record(self.context)],
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_prepare_rejects_unregistered_legacy_d2_runtime_root(self):
        legacy_run_id = f"d2-20260731t235959z-{secrets.token_hex(6)}"
        legacy_root = self.runtime_root_parent / f"starring-d2-{legacy_run_id}"
        legacy_root.mkdir(mode=0o700)
        try:
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError,
                "unregistered_d2_runtime_present",
            ):
                ORCHESTRATOR.command_prepare(self.context, self.platform)
            self.assertFalse(self.context.artifact_directory.exists())
            self.assertFalse(self.context.root.exists())
            self.assertFalse(self.registry_path.exists())
        finally:
            legacy_root.rmdir()

    def test_distinct_discord_identities_have_distinct_durable_owners(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        other = self.prepare_additional_context(
            guild_id="1524810437118525621",
            application_id="1524810437118525622",
            bot_user_id="1524810437118525623",
        )
        ORCHESTRATOR.command_prepare(other, self.platform)
        self.assertEqual(
            set(
                owner["run_id"]
                for owner in CONTRACT.load_discord_ownership_registry()["owners"]
            ),
            {self.context.manifest["run_id"], other.manifest["run_id"]},
        )
        ORCHESTRATOR.command_cleanup(other, self.platform)
        self.assertEqual(
            CONTRACT.load_discord_ownership_registry()["owners"],
            [CONTRACT.discord_ownership_record(self.context)],
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_prepare_replay_fails_closed_when_durable_owner_is_missing(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        CONTRACT.write_discord_ownership_registry(
            CONTRACT.empty_discord_ownership_registry()
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "discord_ownership_claim_absent",
        ):
            ORCHESTRATOR.command_prepare(self.context, self.platform)
        registry = CONTRACT.empty_discord_ownership_registry()
        registry["owners"] = [CONTRACT.discord_ownership_record(self.context)]
        CONTRACT.write_discord_ownership_registry(registry)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_cleaned_replay_refuses_unexpected_live_ownership_claim(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)
        registry = CONTRACT.empty_discord_ownership_registry()
        registry["owners"] = [CONTRACT.discord_ownership_record(self.context)]
        CONTRACT.write_discord_ownership_registry(registry)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "cleaned_state_discord_ownership_drift",
        ):
            ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(
            CONTRACT.load_discord_ownership_registry()["owners"],
            [CONTRACT.discord_ownership_record(self.context)],
        )
        CONTRACT.release_discord_ownership(self.context)
        replay = ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(replay["status"], "already_cleaned")

    def test_cleaned_replay_does_not_release_later_run_reusing_identity(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)
        later = self.prepare_additional_context(
            guild_id=self.context.manifest["discord"]["guild_id"],
            application_id=self.context.manifest["discord"]["application_id"],
            bot_user_id=self.context.manifest["discord"]["bot_user_id"],
        )
        ORCHESTRATOR.command_prepare(later, self.platform)
        replay = ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(replay["status"], "already_cleaned")
        self.assertEqual(
            CONTRACT.load_discord_ownership_registry()["owners"],
            [CONTRACT.discord_ownership_record(later)],
        )
        ORCHESTRATOR.command_cleanup(later, self.platform)

    def test_registry_permissions_and_shape_fail_closed(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.registry_path.chmod(0o644)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "discord_ownership_registry_invalid",
        ):
            ORCHESTRATOR.command_dry_run(self.context, self.platform)
        self.registry_path.chmod(0o600)
        self.registry_path.write_text(
            (
                '{"schema_version":1,"kind":'
                f'"{CONTRACT.DISCORD_OWNERSHIP_REGISTRY_KIND}",'
                '"owners":[],"owners":[]}\n'
            ),
            encoding="utf-8",
        )
        self.registry_path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "discord_ownership_registry_invalid",
        ):
            ORCHESTRATOR.command_dry_run(self.context, self.platform)
        self.registry_path.write_text(
            (
                '{"schema_version":true,"kind":'
                f'"{CONTRACT.DISCORD_OWNERSHIP_REGISTRY_KIND}",'
                '"owners":[]}\n'
            ),
            encoding="utf-8",
        )
        self.registry_path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "discord_ownership_registry_invalid",
        ):
            ORCHESTRATOR.command_dry_run(self.context, self.platform)
        self.registry_path.unlink()
        outside = self.root / "registry-target.json"
        outside.write_text("{}\n", encoding="utf-8")
        outside.chmod(0o600)
        self.registry_path.symlink_to(outside)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "discord_ownership_registry_invalid",
        ):
            ORCHESTRATOR.command_dry_run(self.context, self.platform)

    def test_generated_jobs_are_unloaded_and_reference_only_dedicated_credentials(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        bootstrap_hba = (self.context.cluster_root / "pg_hba.conf").read_text(
            encoding="utf-8"
        )
        self.assertEqual(
            bootstrap_hba.splitlines(),
            [
                "local postgres,starring_runtime_staging starring_cluster_admin peer map=starring_bootstrap",
                "host all all 0.0.0.0/0 reject",
                "host all all ::0/0 reject",
                "local all all reject",
                "host replication all 0.0.0.0/0 reject",
                "host replication all ::0/0 reject",
                "local replication all reject",
            ],
        )
        self.assertEqual(
            (self.context.cluster_root / "pg_ident.conf").read_text(encoding="utf-8"),
            "starring_bootstrap jungbogeon starring_cluster_admin\n",
        )
        values = {}
        for name in ("api", "runtime", "worker", "transport", "tunnel"):
            label = self.context.manifest["services"][name]["label"]
            values[name] = plistlib.loads(
                (self.context.plist_directory / f"{label}.plist").read_bytes()
            )
            self.assertFalse(values[name]["RunAtLoad"])
            self.assertEqual(values[name]["Label"], label)
        api_environment = values["api"]["EnvironmentVariables"]
        self.assertEqual(
            api_environment["STARRING_API_DISCORD_BOT_TOKEN_REFERENCE"],
            "keychain:starring.d2.credentials:discord.bot-token",
        )
        self.assertEqual(
            values["runtime"]["EnvironmentVariables"][
                "STARRING_RUNTIME_DISCORD_BOT_TOKEN_SECRET_REFERENCE"
            ],
            "keychain:starring.d2.credentials:discord.bot-token",
        )
        runtime_environment = values["runtime"]["EnvironmentVariables"]
        self.assertEqual(
            runtime_environment["STARRING_RUNTIME_DISCORD_TRANSPORT_MODE"],
            "loopback_proxy_v1",
        )
        self.assertEqual(
            runtime_environment["STARRING_RUNTIME_DISCORD_GATEWAY_PROXY_URL"],
            "ws://127.0.0.1:29101",
        )
        self.assertEqual(
            runtime_environment[
                "STARRING_RUNTIME_DISCORD_EFFECT_HTTP_PROXY_AUTHORITY"
            ],
            "127.0.0.1:29102",
        )
        self.assertEqual(
            values["transport"]["ProgramArguments"],
            [
                str(self.candidates["certification_transport"]),
                "--root",
                str(self.context.root),
                "--run-id",
                self.context.manifest["run_id"],
                "--guild-id",
                self.context.manifest["discord"]["guild_id"],
                "--hub-channel-id",
                self.context.manifest["discord"]["hub_channel_id"],
                "--actor-id",
                self.context.manifest["discord"]["actor_id"],
                "--bot-user-id",
                self.context.manifest["discord"]["bot_user_id"],
                "--gateway-listen",
                "127.0.0.1:29101",
                "--http-listen",
                "127.0.0.1:29102",
            ],
        )
        self.assertEqual(
            values["worker"]["EnvironmentVariables"]["STARRING_CODEX_PATH"],
            str(self.candidates["codex"]),
        )
        tunnel_runner = (
            self.context.artifact_directory / "run-tunnel.zsh"
        ).read_text(encoding="utf-8")
        self.assertIn("STARRING_D2_TUNNEL_KEYCHAIN_SERVICE", tunnel_runner)
        self.assertIn("STARRING_D2_CLOUDFLARE_ORIGIN_SERVICE", tunnel_runner)
        self.assertIn("STARRING_D2_CLOUDFLARE_TUNNEL_ID", tunnel_runner)
        self.assertNotIn("starring.d2.credentials", tunnel_runner)
        tunnel_environment = values["tunnel"]["EnvironmentVariables"]
        self.assertEqual(
            tunnel_environment["STARRING_D2_CLOUDFLARE_TUNNEL_ID"],
            CERTIFICATION.D2_CLOUDFLARE_TUNNEL_ID,
        )
        self.assertEqual(
            tunnel_environment["STARRING_D2_CLOUDFLARE_ORIGIN_SERVICE"],
            CERTIFICATION.D2_ORIGIN_SERVICE,
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_transport_snapshot_health_is_exact_and_identity_bound(self):
        snapshot = {
            "version": 3,
            "ready": True,
            "run_id": self.context.manifest["run_id"],
            "guild_id": self.context.manifest["discord"]["guild_id"],
            "hub_channel_id": self.context.manifest["discord"]["hub_channel_id"],
            "actor_id": self.context.manifest["discord"]["actor_id"],
            "bot_user_id": self.context.manifest["discord"]["bot_user_id"],
            "instance_id": "d2ti-0123456789abcdef0123456789abcdef",
            "gateway": {
                "partitioned": False,
                "connections": 1,
                "active_connections": 1,
                "completed_connections": 0,
                "clean_close_relays": 0,
                "relay_failures": 0,
                "connection_aborts": 0,
                "ready_rewrites": 1,
                "partition_events": 0,
                "identity_rejections": 0,
                "duplicate_armed": False,
                "armed_duplicate_operation_id": None,
                "duplicate_claimed": False,
                "claimed_duplicate_operation_id": None,
                "duplicate_injections": 0,
                "duplicate_failed_attempts": 0,
                "last_failed_duplicate_operation_id": None,
                "duplicate_delivery_count": 0,
                "last_duplicate_interaction_id": None,
                "last_duplicate_operation_id": None,
            },
            "effect_http": {
                "forwarded_requests": 0,
                "rejected_requests": 0,
                "indeterminate_armed": False,
                "armed_indeterminate_operation_id": None,
                "indeterminate_claimed": False,
                "claimed_indeterminate_operation_id": None,
                "indeterminate_injections": 0,
                "last_indeterminate_audit_reason_sha256": None,
                "last_indeterminate_operation_id": None,
                "last_indeterminate_upstream_status": None,
                "owned_role_count": 0,
                "owned_channel_count": 0,
                "owned_message_count": 0,
            },
        }
        platform = ORCHESTRATOR.Platform()
        self.assertTrue(platform._transport_snapshot_valid(self.context, snapshot))
        not_ready = json.loads(json.dumps(snapshot))
        not_ready["ready"] = False
        self.assertFalse(platform._transport_snapshot_valid(self.context, not_ready))
        self.assertTrue(
            platform._transport_snapshot_valid(
                self.context, not_ready, require_ready=False
            )
        )
        identity_drift = json.loads(json.dumps(snapshot))
        identity_drift["actor_id"] = "1056857223529250907"
        self.assertFalse(platform._transport_snapshot_valid(self.context, identity_drift))
        hub_drift = json.loads(json.dumps(snapshot))
        hub_drift["hub_channel_id"] = "1524810437118525555"
        self.assertFalse(platform._transport_snapshot_valid(self.context, hub_drift))
        widened = json.loads(json.dumps(snapshot))
        widened["gateway"]["unpinned"] = 0
        self.assertFalse(platform._transport_snapshot_valid(self.context, widened))
        failed = json.loads(json.dumps(snapshot))
        failed["ready"] = False
        failed["gateway"]["active_connections"] = 0
        failed["gateway"]["completed_connections"] = 1
        failed["gateway"]["relay_failures"] = 1
        self.assertTrue(
            platform._transport_snapshot_valid(
                self.context, failed, require_ready=False
            )
        )
        self.assertFalse(platform._transport_snapshot_valid(self.context, failed))
        dishonest = json.loads(json.dumps(failed))
        dishonest["ready"] = True
        self.assertFalse(
            platform._transport_snapshot_valid(
                self.context, dishonest, require_ready=False
            )
        )
        overlapping = json.loads(json.dumps(failed))
        overlapping["gateway"]["clean_close_relays"] = 1
        self.assertFalse(
            platform._transport_snapshot_valid(
                self.context, overlapping, require_ready=False
            )
        )
        impossible = json.loads(json.dumps(snapshot))
        impossible["gateway"]["completed_connections"] = 1
        self.assertFalse(
            platform._transport_snapshot_valid(
                self.context, impossible, require_ready=False
            )
        )

    def test_live_restart_has_no_direct_database_observation_path(self):
        self.assertFalse(hasattr(PLATFORM.Platform(), "runtime_live_observation"))
        source = (DIRECTORY / "d2_orchestrator_platform.py").read_text(
            encoding="utf-8"
        )
        self.assertNotIn("runtime_serving_leases", source)
        self.assertNotIn("PGPASSFILE", source)

    def test_runtime_identity_probe_accepts_only_the_exact_bounded_shape(self):
        platform = PLATFORM.Platform()

        def response(body, status=200):
            platform.run = lambda *args, **kwargs: subprocess.CompletedProcess(
                args, 0, body + f"\n{status}".encode("ascii"), b""
            )

        response(
            b'{"schema_version":1,"os_pid":1234,'
            b'"process_instance_id":"0123456789abcdef0123456789abcdef"}'
        )
        self.assertEqual(
            platform.runtime_process_identity(self.context),
            {
                "schema_version": 1,
                "os_pid": 1234,
                "process_instance_id": "0123456789abcdef0123456789abcdef",
            },
        )
        response(b'{"schema_version":1,"schema_version":1,"os_pid":1234,'
                 b'"process_instance_id":"0123456789abcdef0123456789abcdef"}')
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "runtime_identity_probe_output_invalid",
        ):
            platform.runtime_process_identity(self.context)
        response(b"unavailable", 503)
        self.assertIsNone(platform.runtime_process_identity(self.context))

    def test_transport_control_lost_response_reuses_durable_operation(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        original = self.platform.transport_control
        injected = False

        def lose_first_response(context, command, fields=None, timeout_seconds=3):
            nonlocal injected
            result = original(context, command, fields, timeout_seconds)
            if command == "partition_gateway" and not injected:
                injected = True
                raise ORCHESTRATOR.OrchestratorError("injected_lost_response")
            return result

        self.platform.transport_control = lose_first_response
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "injected_lost_response"
        ):
            ORCHESTRATOR.command_transport_control(
                self.context, self.platform, "partition-gateway"
            )
        directory = ORCHESTRATOR.transport_control_directory(self.context)
        intent_path = directory / "0001-partition-gateway-intent.json"
        complete_path = directory / "0001-partition-gateway-complete.json"
        intent = json.loads(intent_path.read_text(encoding="utf-8"))
        self.assertFalse(complete_path.exists())
        self.platform.transport_control = original
        result = ORCHESTRATOR.command_transport_control(
            self.context, self.platform, "partition-gateway"
        )
        complete = json.loads(complete_path.read_text(encoding="utf-8"))
        self.assertEqual(result["operation_id"], intent["operation_id"])
        self.assertEqual(complete["operation_id"], intent["operation_id"])
        self.assertEqual(result["response"], {"changed": False})
        self.assertTrue(result["snapshot"]["gateway"]["partitioned"])
        self.assertEqual(intent_path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(complete_path.stat().st_mode & 0o777, 0o600)
        ORCHESTRATOR.command_transport_control(
            self.context, self.platform, "heal-gateway"
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_transport_arm_next_is_operation_bound_and_busy_fails_closed(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        armed = ORCHESTRATOR.command_transport_control(
            self.context, self.platform, "arm-next-duplicate"
        )
        self.assertEqual(armed["response"]["disposition"], "armed")
        self.assertEqual(
            armed["snapshot"]["gateway"]["armed_duplicate_operation_id"],
            armed["operation_id"],
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "transport_operation_busy"
        ):
            ORCHESTRATOR.command_transport_control(
                self.context, self.platform, "arm-next-duplicate"
            )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_transport_instance_rotation_invalidates_candidate_run(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        self.platform.transport_state["instance_id"] = (
            "d2ti-fedcba9876543210fedcba9876543210"
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_transport_control(
                self.context, self.platform, "snapshot"
            )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertTrue(
            ORCHESTRATOR.candidate_start_retirement_path(self.context).is_file()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_prepare_failure_runs_manifest_reconstructed_cleanup(self):
        self.platform.initdb_failure = True
        with self.assertRaisesRegex(ORCHESTRATOR.OrchestratorError, "injected_initdb_failure"):
            ORCHESTRATOR.command_prepare(self.context, self.platform)
        self.assertFalse(self.isolated_root.exists())
        self.assertEqual(CONTRACT.load_discord_ownership_registry()["owners"], [])
        state = ORCHESTRATOR.load_state(self.context, {"cleaned"})
        self.assertEqual(state["phase"], "cleaned")
        receipts = [
            json.loads(line)
            for line in self.context.journal_path.read_text(encoding="utf-8").splitlines()
        ]
        self.assertEqual(receipts[-1]["action"], "cleanup")
        self.assertEqual(receipts[-1]["status"], "complete")

    def test_prepare_idempotency_rejects_owner_marker_drift(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        service = self.context.manifest["keychain_services"]["runtime"]
        self.platform.owner_values[service] = "different-run"
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "prepared_state_drift"
        ):
            ORCHESTRATOR.command_prepare(self.context, self.platform)
        self.platform.owner_values[service] = self.context.manifest["run_id"]
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_protected_state_drift_blocks_start_before_postgres_mutation(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        self.platform.loaded.remove("local.starring.api.staging")
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "protected_staging_state_changed"
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertFalse(self.platform.postgres)
        self.platform.loaded.add("local.starring.api.staging")
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_database_bootstrap_failure_stops_postgres_and_preserves_prepared_state(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        self.platform.bootstrap_failure = True
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "injected_bootstrap_failure"
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertFalse(self.platform.postgres)
        self.assertEqual(
            ORCHESTRATOR.load_state(self.context, {"stopped"})["phase"],
            "stopped",
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_candidate_health_failure_rolls_back_and_exact_replay_restarts(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        self.platform.health_failure = str(
            self.context.manifest["services"]["runtime"]["port"]
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "candidate_health_unready"
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertFalse(self.platform.postgres)
        self.assertEqual(
            ORCHESTRATOR.load_state(self.context, {"stopped"})["phase"], "stopped"
        )
        for name in ORCHESTRATOR.SERVICE_START_ORDER:
            self.assertNotIn(
                self.context.manifest["services"][name]["label"], self.platform.loaded
            )
        self.platform.health_failure = None
        result = ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertEqual(result["phase"], "candidate_started")
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_restart_retires_pre_intent_api_identity_drift(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        api_label = self.context.manifest["services"]["api"]["label"]
        prior_pid = self.platform.pids[api_label]
        replacement_pid = prior_pid + 100
        prior_start = self.platform.process_start_times[prior_pid]
        self.platform.pids[api_label] = replacement_pid
        self.platform.process_start_times[replacement_pid] = (
            prior_start[0] + 1,
            prior_start[1],
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform
            )
        self.assertTrue(
            ORCHESTRATOR.candidate_start_retirement_path(self.context).is_file()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_restart_retires_pre_intent_runtime_identity_drift(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        prior_pid = self.platform.pids[runtime_label]
        replacement_pid = prior_pid + 100
        prior_start = self.platform.process_start_times[prior_pid]
        self.platform.pids[runtime_label] = replacement_pid
        self.platform.process_start_times[replacement_pid] = (
            prior_start[0] + 1,
            prior_start[1],
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform
            )
        self.assertTrue(
            ORCHESTRATOR.candidate_start_retirement_path(self.context).is_file()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_certifies_step_11_and_exactly_replays(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        old_pid = self.platform.pids[runtime_label]
        start_count = len(self.platform.start_order)
        awaiting = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        self.assertEqual(awaiting["status"], "awaiting_canonical_confirmation")
        self.assertEqual(awaiting["old_pid"], old_pid)
        self.assertNotEqual(awaiting["new_pid"], old_pid)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "orchestrator_phase_invalid"
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertFalse(
            os.path.lexists(
                ORCHESTRATOR.candidate_start_retirement_path(self.context)
            )
        )
        intent = json.loads(
            LIVE_RUNTIME_RESTART.live_runtime_restart_intent_path(
                self.context
            ).read_text(encoding="utf-8")
        )
        self.assertNotEqual(
            awaiting["process_instance_id"],
            intent["old_process_instance_id"],
        )
        self.assertFalse(
            LIVE_RUNTIME_RESTART.live_runtime_restart_complete_path(
                self.context
            ).exists()
        )
        self.assertFalse(
            LIVE_RUNTIME_RESTART.live_runtime_restart_evidence_path(
                self.context
            ).exists()
        )
        confirmation_path = self.write_live_restart_confirmation(awaiting)
        result = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform, confirmation_path
        )
        self.assertEqual(result["status"], "live_runtime_restart_certified")
        self.assertEqual(result["old_pid"], old_pid)
        self.assertNotEqual(result["new_pid"], old_pid)
        self.assertEqual(result["checkpoint"], "live_fresh_lease")
        self.assertEqual(result["deployment_id"], "deployment-1")
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "orchestrator_phase_invalid"
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertFalse(
            os.path.lexists(
                ORCHESTRATOR.candidate_start_retirement_path(self.context)
            )
        )
        self.assertEqual(
            result["route_id"], intent["deployment_identity"]["route_id"]
        )
        self.assertEqual(result["instance_id"], "instance-1")
        self.assertEqual(
            self.platform.signals,
            [(runtime_label, "SIGTERM")],
        )
        self.assertEqual(
            self.platform.start_order[start_count:],
            [runtime_label],
        )
        evidence_path = pathlib.Path(result["evidence_path"])
        self.assertEqual(evidence_path.stat().st_mode & 0o777, 0o600)
        shutdown_path = (
            LIVE_RUNTIME_RESTART.live_runtime_restart_shutdown_path(
                self.context
            )
        )
        self.assertEqual(shutdown_path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(
            json.loads(shutdown_path.read_text(encoding="utf-8"))[
                "stability_seconds"
            ],
            30,
        )
        self.assertEqual(
            set(json.loads(evidence_path.read_text(encoding="utf-8"))),
            set(CERTIFICATION.STEP_SPECS[11].required),
        )
        evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
        completion = json.loads(
            LIVE_RUNTIME_RESTART.live_runtime_restart_complete_path(
                self.context
            ).read_text(encoding="utf-8")
        )
        self.assertIs(evidence["process_identity_joined"], True)
        self.assertEqual(
            evidence["process_instance_id"],
            awaiting["process_instance_id"],
        )
        self.assertEqual(
            evidence["canonical_confirmation_sha256"],
            completion["canonical_confirmation_sha256"],
        )
        self.assertEqual(
            evidence["public_origin"],
            self.context.manifest["cloudflare"]["public_origin"],
        )
        receipts_path = self.manifest_path.with_name("receipts.jsonl")
        self.assertEqual(len(receipts_path.read_text().splitlines()), 10)
        replay_start_count = len(self.platform.start_order)
        replay = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        self.assertEqual(replay["status"], "exact_replay")
        self.assertEqual(replay["new_pid"], result["new_pid"])
        self.assertEqual(
            replay["coordinator_source"], result["coordinator_source"]
        )
        self.assertEqual(len(self.platform.signals), 1)
        self.assertEqual(len(self.platform.start_order), replay_start_count)
        coordinator_source = pathlib.Path(result["coordinator_source"])
        self.assertEqual(coordinator_source.stat().st_mode & 0o777, 0o600)
        advanced = D2_RUN.advance_certification(
            self.manifest_path, 11, [str(coordinator_source)]
        )
        self.assertEqual(advanced["step"], 11)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_rejects_local_identity_pid_mismatch_before_signal(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        self.platform.runtime_identity_override = {
            "schema_version": 1,
            "os_pid": self.platform.pids[runtime_label] + 1,
            "process_instance_id": self.platform.runtime_process_instance_ids[
                runtime_label
            ],
        }
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform
            )
        self.assertEqual(self.platform.signals, [])
        self.assertTrue(
            ORCHESTRATOR.candidate_start_retirement_path(self.context).is_file()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_rejects_local_process_identity_drift(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        awaiting = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        self.platform.runtime_identity_override = {
            "schema_version": 1,
            "os_pid": awaiting["new_pid"],
            "process_instance_id": "f" * 32,
        }
        confirmation_path = self.write_live_restart_confirmation(awaiting)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_confirmation_process_drift",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform, confirmation_path
            )
        self.assertFalse(
            LIVE_RUNTIME_RESTART.live_runtime_restart_complete_path(
                self.context
            ).exists()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_rejects_foreign_canonical_process_identity(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        awaiting = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        confirmation_path = self.write_live_restart_confirmation(
            awaiting, {"process_instance_id": "f" * 32}
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_confirmation_scope_mismatch",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform, confirmation_path
            )
        self.assertFalse(
            LIVE_RUNTIME_RESTART.live_runtime_restart_complete_path(
                self.context
            ).exists()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_rejects_premature_confirmation_without_signal(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        confirmation_path = self.root / "premature-confirmation.json"
        confirmation_path.write_text("{}", encoding="utf-8")
        confirmation_path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_confirmation_premature",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform, confirmation_path
            )
        self.assertEqual(self.platform.signals, [])
        self.assertFalse(
            LIVE_RUNTIME_RESTART.live_runtime_restart_intent_path(
                self.context
            ).exists()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_rejects_tampered_completion_binding(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        awaiting = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        confirmation_path = self.write_live_restart_confirmation(awaiting)
        ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform, confirmation_path
        )
        completion_path = (
            LIVE_RUNTIME_RESTART.live_runtime_restart_complete_path(
                self.context
            )
        )
        completion = json.loads(completion_path.read_text(encoding="utf-8"))
        completion["canonical_confirmation"]["operation_id"] = (
            "d2:ffffffffffffffff:certify-live-runtime-restart"
        )
        completion["canonical_confirmation_sha256"] = (
            LIVE_RUNTIME_RESTART.canonical_confirmation_digest(
                completion["canonical_confirmation"]
            )
        )
        completion_path.write_text(json.dumps(completion), encoding="utf-8")
        completion_path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_completion_invalid",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform
            )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_persists_intent_before_sigterm(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        original_signal = self.platform.launchd_signal

        def require_intent(label, signal_name):
            intent_path = LIVE_RUNTIME_RESTART.live_runtime_restart_intent_path(
                self.context
            )
            self.assertTrue(intent_path.is_file())
            self.assertEqual(intent_path.stat().st_mode & 0o777, 0o600)
            original_signal(label, signal_name)

        self.platform.launchd_signal = require_intent
        ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_rechecks_readiness_before_sigterm(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        original_write = LIVE_RUNTIME_RESTART.write_live_runtime_restart_record

        def close_readiness(context, name, record):
            path = original_write(context, name, record)
            if name == "intent":
                self.platform.health_failure = str(
                    self.context.manifest["services"]["runtime"]["port"]
                )
            return path

        with mock.patch.object(
            LIVE_RUNTIME_RESTART,
            "write_live_runtime_restart_record",
            side_effect=close_readiness,
        ), self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_precondition_changed",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform
            )
        self.assertEqual(self.platform.signals, [])
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_binds_transport_to_receipt_three(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        replacement = "d2ti-fedcba9876543210fedcba9876543210"
        self.platform.transport_state["instance_id"] = replacement
        evidence_path = self.context.artifact_directory / "step-03-evidence.json"
        evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
        evidence["transport_instance_id"] = replacement
        evidence_path.write_text(json.dumps(evidence), encoding="utf-8")
        evidence_path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform
            )
        self.assertEqual(self.platform.signals, [])
        self.assertTrue(
            ORCHESTRATOR.candidate_start_retirement_path(self.context).is_file()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_recovers_after_sigterm_before_start(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        with mock.patch.object(
            LIVE_RUNTIME_RESTART,
            "command_restart_drained_runtime",
            side_effect=ORCHESTRATOR.OrchestratorError("injected_restart_loss"),
        ), self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "injected_restart_loss"
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform
            )
        self.assertEqual(len(self.platform.signals), 1)
        recovered = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        self.assertEqual(recovered["status"], "awaiting_canonical_confirmation")
        self.assertEqual(len(self.platform.signals), 1)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_repeats_stability_after_interruption(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        interrupted = False

        def interrupt_stability(seconds):
            nonlocal interrupted
            if seconds == 0.25 and not interrupted:
                interrupted = True
                raise ORCHESTRATOR.OrchestratorError("injected_stability_loss")
            self.advance_live_clock(seconds)

        with mock.patch.object(
            LIVE_RUNTIME_RESTART,
            "wait_interval",
            side_effect=interrupt_stability,
        ), self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "injected_stability_loss"
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform
            )
        self.assertEqual(len(self.platform.signals), 1)
        self.assertFalse(
            LIVE_RUNTIME_RESTART.live_runtime_restart_shutdown_path(
                self.context
            ).exists()
        )
        self.assertFalse(
            ORCHESTRATOR.drained_runtime_restart_directory(self.context).exists()
        )
        restart_clock = self.live_clock
        recovered = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        self.assertEqual(recovered["status"], "awaiting_canonical_confirmation")
        self.assertGreaterEqual(self.live_clock - restart_clock, 30)
        self.assertEqual(len(self.platform.signals), 1)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_recovers_after_inner_restart(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        awaiting = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        confirmation_path = self.write_live_restart_confirmation(awaiting)
        original_write = LIVE_RUNTIME_RESTART.write_live_runtime_restart_record
        injected = False

        def lose_completion(context, name, record):
            nonlocal injected
            if name == "complete" and not injected:
                injected = True
                raise ORCHESTRATOR.OrchestratorError("injected_completion_loss")
            return original_write(context, name, record)

        with mock.patch.object(
            LIVE_RUNTIME_RESTART,
            "write_live_runtime_restart_record",
            side_effect=lose_completion,
        ), self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "injected_completion_loss"
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform, confirmation_path
            )
        start_count = len(self.platform.start_order)
        recovered = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform, confirmation_path
        )
        self.assertEqual(recovered["status"], "live_runtime_restart_certified")
        self.assertEqual(len(self.platform.signals), 1)
        self.assertEqual(len(self.platform.start_order), start_count)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_requires_the_exact_fresh_live_confirmation(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        awaiting = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        invalid_path = self.write_live_restart_confirmation(
            awaiting, {"serving_state": "stale"}
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_confirmation_invalid",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform, invalid_path
            )
        self.assertFalse(
            LIVE_RUNTIME_RESTART.live_runtime_restart_complete_path(
                self.context
            ).exists()
        )
        self.assertEqual(len(self.platform.signals), 1)
        valid_path = self.write_live_restart_confirmation(awaiting)
        recovered = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform, valid_path
        )
        self.assertEqual(recovered["status"], "live_runtime_restart_certified")
        self.assertEqual(len(self.platform.signals), 1)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_rejects_unprotected_confirmation_file(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        awaiting = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        confirmation_path = self.write_live_restart_confirmation(awaiting)
        confirmation_path.chmod(0o644)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_confirmation_file_invalid",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform, confirmation_path
            )
        self.assertFalse(
            LIVE_RUNTIME_RESTART.live_runtime_restart_complete_path(
                self.context
            ).exists()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_rejects_expired_confirmation(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        awaiting = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        confirmation_path = self.write_live_restart_confirmation(awaiting)
        confirmation = json.loads(
            confirmation_path.read_text(encoding="utf-8")
        )
        with mock.patch.object(
            LIVE_RUNTIME_RESTART,
            "current_utc_timestamp_key",
            return_value=LIVE_RUNTIME_RESTART.utc_timestamp_key(
                confirmation["lease_expires_at"]
            ),
        ), self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_confirmation_expired",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform, confirmation_path
            )
        self.assertFalse(
            LIVE_RUNTIME_RESTART.live_runtime_restart_complete_path(
                self.context
            ).exists()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_rejects_fractionally_expired_confirmation(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        awaiting = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        confirmation_path = self.write_live_restart_confirmation(awaiting)
        confirmation = json.loads(
            confirmation_path.read_text(encoding="utf-8")
        )
        expires = LIVE_RUNTIME_RESTART.utc_timestamp_key(
            confirmation["lease_expires_at"]
        )
        same_second_after_expiry = (expires[0], expires[1] + 1)
        with mock.patch.object(
            LIVE_RUNTIME_RESTART,
            "current_utc_timestamp_key",
            return_value=same_second_after_expiry,
        ), self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_confirmation_expired",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform, confirmation_path
            )
        self.assertFalse(
            LIVE_RUNTIME_RESTART.live_runtime_restart_complete_path(
                self.context
            ).exists()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_rejects_wrong_origin_and_boolean_schema(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        awaiting = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        wrong_origin = self.write_live_restart_confirmation(
            awaiting, {"public_origin": "https://api.starring.co.kr"}
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_confirmation_scope_mismatch",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform, wrong_origin
            )
        boolean_schema = self.write_live_restart_confirmation(
            awaiting, {"schema_version": True}
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_confirmation_invalid",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform, boolean_schema
            )
        boundary = datetime.datetime.fromisoformat(
            awaiting["shutdown_boundary"].replace("Z", "+00:00")
        )
        overlong_lease = self.write_live_restart_confirmation(
            awaiting,
            {
                "lease_expires_at": (
                    boundary + datetime.timedelta(seconds=47)
                ).isoformat().replace("+00:00", "Z")
            },
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_confirmation_invalid",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform, overlong_lease
            )
        self.assertFalse(
            LIVE_RUNTIME_RESTART.live_runtime_restart_complete_path(
                self.context
            ).exists()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_rejects_heartbeat_before_shutdown_boundary(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        awaiting = ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        confirmation_path = self.write_live_restart_confirmation(
            awaiting,
            {"last_heartbeat_at": awaiting["shutdown_boundary"]},
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_confirmation_invalid",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform, confirmation_path
            )
        self.assertFalse(
            LIVE_RUNTIME_RESTART.live_runtime_restart_complete_path(
                self.context
            ).exists()
        )
        self.assertEqual(len(self.platform.signals), 1)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_rejects_missing_prerequisites_before_signal(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_prerequisites_invalid",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform
            )
        self.assertEqual(self.platform.signals, [])
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_rejects_raw_receipts_without_coordinator_prefix(self):
        self.record_prerequisite_receipts(coordinator=False)
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        with self.assertRaisesRegex(
            D2_RUN.CertificationError,
            "coordinator_intent_missing:1",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform
            )
        self.assertEqual(self.platform.signals, [])
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_rejects_nonzero_sigterm_exit(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        self.platform.signal_exit_code = 70
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_shutdown_unsuccessful",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform
            )
        self.assertEqual(len(self.platform.signals), 1)
        self.assertFalse(
            ORCHESTRATOR.drained_runtime_restart_directory(self.context).exists()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_rejects_auto_restart_after_nonzero_exit(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        self.platform.signal_exit_code = 70
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_shutdown_unsuccessful",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform
            )
        self.assertEqual(len(self.platform.signals), 1)
        self.platform.next_pid += 1
        self.platform.pids[runtime_label] = self.platform.next_pid
        self.platform.launchd_runs[runtime_label] += 1
        self.platform.launchd_states[runtime_label] = "running"
        self.platform.last_exit_codes[runtime_label] = 70
        self.platform.runtime_process_instance_ids[runtime_label] = (
            f"{self.platform.next_pid:032x}"
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_shutdown_unjournaled_state",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform
            )
        self.assertEqual(len(self.platform.signals), 1)
        self.assertFalse(
            LIVE_RUNTIME_RESTART.live_runtime_restart_shutdown_path(
                self.context
            ).exists()
        )
        self.assertFalse(
            LIVE_RUNTIME_RESTART.live_runtime_restart_awaiting_path(
                self.context
            ).exists()
        )
        self.assertFalse(
            ORCHESTRATOR.drained_runtime_restart_directory(self.context).exists()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_observes_the_complete_throttle_window(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        identity = DRAINED_RUNTIME_RESTART.drained_runtime_restart_identity(
            self.context
        )
        runtime_label = identity["label"]
        old_runs = self.platform.launchd_runs[runtime_label]
        self.platform.exit_launchd(runtime_label)
        clock = 0.0
        sleeps = []

        def monotonic():
            return clock

        def sleep(seconds):
            nonlocal clock
            sleeps.append(seconds)
            clock += seconds

        with mock.patch.object(
            LIVE_RUNTIME_RESTART, "LIVE_EXIT_STABILITY_SECONDS", 1
        ), mock.patch.object(
            LIVE_RUNTIME_RESTART, "monotonic_time", side_effect=monotonic
        ), mock.patch.object(
            LIVE_RUNTIME_RESTART, "wait_interval", side_effect=sleep
        ):
            LIVE_RUNTIME_RESTART.wait_for_clean_runtime_exit(
                self.context,
                self.platform,
                identity,
                {"old_pid": 41004, "old_runs": old_runs},
            )
        self.assertGreaterEqual(clock, 1)
        self.assertEqual(sleeps, [0.25, 0.25, 0.25, 0.25])
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_rejects_an_exit_after_the_deadline(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        identity = DRAINED_RUNTIME_RESTART.drained_runtime_restart_identity(
            self.context
        )
        runtime_label = identity["label"]
        old_runs = self.platform.launchd_runs[runtime_label]
        self.platform.exit_launchd(runtime_label)
        with mock.patch.object(
            LIVE_RUNTIME_RESTART,
            "monotonic_time",
            side_effect=[0.0, 30.000001],
        ), self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_shutdown_timeout",
        ):
            LIVE_RUNTIME_RESTART.wait_for_clean_runtime_exit(
                self.context,
                self.platform,
                identity,
                {"old_pid": 41004, "old_runs": old_runs},
            )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_live_runtime_restart_deadline_starts_before_sigterm_returns(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        original_signal = self.platform.launchd_signal

        def delayed_signal(label, signal_name):
            original_signal(label, signal_name)
            self.advance_live_clock(30.000001)

        self.platform.launchd_signal = delayed_signal
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "live_runtime_restart_shutdown_timeout",
        ):
            ORCHESTRATOR.command_certify_live_runtime_restart(
                self.context, self.platform
            )
        self.assertEqual(len(self.platform.signals), 1)
        self.assertFalse(
            LIVE_RUNTIME_RESTART.live_runtime_restart_shutdown_path(
                self.context
            ).exists()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_drained_runtime_restart_is_manifest_scoped_and_exactly_replayable(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        drained_pid = self.platform.pids[runtime_label]
        self.platform.exit_launchd(runtime_label)
        dependency_pids = {
            name: self.platform.pids[
                self.context.manifest["services"][name]["label"]
            ]
            for name in ("api", "worker", "transport", "tunnel")
        }
        start_count = len(self.platform.start_order)
        bootout_count = len(self.platform.bootouts)
        result = ORCHESTRATOR.command_restart_drained_runtime(self.context, self.platform)
        self.assertEqual(result["status"], "drained_runtime_restarted")
        self.assertIsNone(result["old_pid"])
        self.assertNotEqual(result["new_pid"], drained_pid)
        self.assertEqual(self.platform.start_order[start_count:], [runtime_label])
        self.assertEqual(self.platform.bootouts[bootout_count:], [runtime_label])
        self.assertEqual(
            dependency_pids,
            {
                name: self.platform.pids[
                    self.context.manifest["services"][name]["label"]
                ]
                for name in ("api", "worker", "transport", "tunnel")
            },
        )
        self.assertEqual(self.platform.postgres_process_pid, 40001)
        intent_path = (
            ORCHESTRATOR.drained_runtime_restart_directory(self.context)
            / "0001-intent.json"
        )
        complete_path = (
            ORCHESTRATOR.drained_runtime_restart_directory(self.context)
            / "0001-complete.json"
        )
        self.assertEqual(
            ORCHESTRATOR.drained_runtime_restart_directory(self.context).stat().st_mode
            & 0o777,
            0o700,
        )
        self.assertEqual(intent_path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(complete_path.stat().st_mode & 0o777, 0o600)
        completion = json.loads(complete_path.read_text(encoding="utf-8"))
        self.assertEqual(completion["new_pid"], result["new_pid"])
        self.assertEqual(completion["dependencies"]["postgres"]["pid"], 40001)
        self.assertEqual(
            completion["dependencies"]["api"]["program"],
            str(self.candidates["api"]),
        )
        self.assertEqual(
            completion["dependencies"]["api"]["program_arguments"],
            [str(self.candidates["api"])],
        )
        self.assertEqual(completion["dependencies"]["api"]["runs"], 1)
        self.assertEqual(
            completion["dependencies"]["api"]["plist_path"],
            str(ORCHESTRATOR.service_plist_path(self.context, "api")),
        )
        self.assertRegex(
            completion["dependencies"]["api"]["plist_sha256"],
            r"^[0-9a-f]{64}$",
        )
        self.assertEqual(
            completion["transport_instance_id"],
            "d2ti-0123456789abcdef0123456789abcdef",
        )
        self.assertEqual(
            completion["runtime_identity"]["runtime_sha256"],
            self.context.manifest["candidates"]["runtime"]["sha256"],
        )
        receipts = [
            json.loads(line)
            for line in self.context.journal_path.read_text(encoding="utf-8").splitlines()
        ]
        restart_receipts = [
            receipt for receipt in receipts if receipt["action"] == "drained_runtime_restart"
        ]
        self.assertEqual(
            [receipt["status"] for receipt in restart_receipts],
            ["intent", "complete"],
        )
        replay_start_count = len(self.platform.start_order)
        replay_bootout_count = len(self.platform.bootouts)
        replay = ORCHESTRATOR.command_restart_drained_runtime(self.context, self.platform)
        self.assertEqual(replay["status"], "exact_replay")
        self.assertEqual(replay["new_pid"], result["new_pid"])
        self.assertEqual(len(self.platform.start_order), replay_start_count)
        self.assertEqual(len(self.platform.bootouts), replay_bootout_count)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_drained_runtime_restart_rejects_fresh_absent_runtime_evidence(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        self.platform.exit_launchd(runtime_label)
        self.platform.programs.pop(runtime_label)
        self.platform.plist_paths.pop(runtime_label)
        self.platform.program_arguments.pop(runtime_label)
        self.platform.launchd_runs.pop(runtime_label)
        self.platform.launchd_states.pop(runtime_label)
        self.platform.last_exit_codes.pop(runtime_label)
        self.platform.loaded.remove(runtime_label)
        start_count = len(self.platform.start_order)
        bootout_count = len(self.platform.bootouts)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "runtime_drain_evidence_absent"
        ):
            ORCHESTRATOR.command_restart_drained_runtime(self.context, self.platform)
        self.assertEqual(len(self.platform.start_order), start_count)
        self.assertEqual(len(self.platform.bootouts), bootout_count)
        self.assertFalse(
            ORCHESTRATOR.drained_runtime_restart_directory(self.context).exists()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_drained_runtime_restart_never_stops_a_live_runtime(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        runtime_pid = self.platform.pids[runtime_label]
        start_count = len(self.platform.start_order)
        bootout_count = len(self.platform.bootouts)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "runtime_drain_incomplete"
        ):
            ORCHESTRATOR.command_restart_drained_runtime(self.context, self.platform)
        self.assertEqual(self.platform.pids[runtime_label], runtime_pid)
        self.assertEqual(len(self.platform.start_order), start_count)
        self.assertEqual(len(self.platform.bootouts), bootout_count)
        self.assertFalse(ORCHESTRATOR.drained_runtime_restart_directory(self.context).exists())
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_drained_runtime_restart_requires_a_successful_drain_exit(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        self.platform.exit_launchd(runtime_label, exit_code=70)
        bootout_count = len(self.platform.bootouts)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "runtime_drain_unsuccessful"
        ):
            ORCHESTRATOR.command_restart_drained_runtime(self.context, self.platform)
        self.assertEqual(len(self.platform.bootouts), bootout_count)
        self.assertFalse(ORCHESTRATOR.drained_runtime_restart_directory(self.context).exists())
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_drained_runtime_restart_reobserves_drain_immediately_before_bootout(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        self.platform.exit_launchd(runtime_label)
        original_write = DRAINED_RUNTIME_RESTART.write_drained_runtime_restart_record

        def restart_after_intent(context, sequence, kind, record):
            original_write(context, sequence, kind, record)
            if kind == "intent":
                self.platform.next_pid += 1
                self.platform.pids[runtime_label] = self.platform.next_pid
                self.platform.launchd_states[runtime_label] = "running"
                self.platform.last_exit_codes[runtime_label] = None

        bootout_count = len(self.platform.bootouts)
        with mock.patch.object(
            DRAINED_RUNTIME_RESTART,
            "write_drained_runtime_restart_record",
            side_effect=restart_after_intent,
        ), self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "runtime_drain_incomplete"
        ):
            ORCHESTRATOR.command_restart_drained_runtime(self.context, self.platform)
        self.assertEqual(len(self.platform.bootouts), bootout_count)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_drained_runtime_restart_recovers_started_process_without_second_launch(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        self.platform.exit_launchd(runtime_label)
        original_write = DRAINED_RUNTIME_RESTART.write_atomic
        injected = False

        def lose_completion(path, payload, mode=0o600):
            nonlocal injected
            if path.name == "0001-complete.pending" and not injected:
                injected = True
                raise ORCHESTRATOR.OrchestratorError("injected_completion_loss")
            return original_write(path, payload, mode)

        with mock.patch.object(
            DRAINED_RUNTIME_RESTART, "write_atomic", side_effect=lose_completion
        ):
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, "injected_completion_loss"
            ):
                ORCHESTRATOR.command_restart_drained_runtime(self.context, self.platform)
        restarted_pid = self.platform.pids[runtime_label]
        start_count = len(self.platform.start_order)
        recovered = ORCHESTRATOR.command_restart_drained_runtime(self.context, self.platform)
        self.assertEqual(recovered["status"], "drained_runtime_restarted")
        self.assertEqual(recovered["new_pid"], restarted_pid)
        self.assertEqual(len(self.platform.start_order), start_count)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_drained_runtime_restart_allows_absent_only_after_pending_bootout(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        self.platform.exit_launchd(runtime_label)
        self.platform.launchd_failure = runtime_label
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "injected_launchd_failure"
        ):
            ORCHESTRATOR.command_restart_drained_runtime(
                self.context, self.platform
            )
        self.assertNotIn(runtime_label, self.platform.loaded)
        self.platform.launchd_failure = None
        recovered = ORCHESTRATOR.command_restart_drained_runtime(
            self.context, self.platform
        )
        self.assertEqual(recovered["status"], "drained_runtime_restarted")
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_drained_runtime_restart_recovers_strict_sigkill_temporary_files(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        self.platform.exit_launchd(runtime_label)
        directory = ORCHESTRATOR.drained_runtime_restart_temporary_directory(self.context)
        directory.mkdir(mode=0o700)
        for name in (
            ".0001-intent.pending.49152.tmp",
            "0001-intent.pending",
        ):
            path = directory / name
            path.write_bytes(b"partial")
            path.chmod(0o600)
        result = ORCHESTRATOR.command_restart_drained_runtime(self.context, self.platform)
        self.assertEqual(result["status"], "drained_runtime_restarted")
        self.assertEqual(list(directory.iterdir()), [])
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_drained_runtime_restart_rejects_an_unrecognized_temporary_entry(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        self.platform.exit_launchd(runtime_label)
        directory = ORCHESTRATOR.drained_runtime_restart_temporary_directory(self.context)
        directory.mkdir(mode=0o700)
        unexpected = directory / "unexpected"
        unexpected.write_bytes(b"partial")
        unexpected.chmod(0o600)
        bootout_count = len(self.platform.bootouts)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "drained_runtime_restart_temporary_inventory_invalid",
        ):
            ORCHESTRATOR.command_restart_drained_runtime(self.context, self.platform)
        self.assertEqual(len(self.platform.bootouts), bootout_count)
        unexpected.unlink()
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_drained_runtime_restart_fails_closed_if_a_dependency_pid_changes(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        api_label = self.context.manifest["services"]["api"]["label"]
        self.platform.exit_launchd(runtime_label)
        original_start = self.platform.launchd_start

        def start_with_dependency_drift(label, plist_path):
            original_start(label, plist_path)
            if label == runtime_label:
                self.platform.pids[api_label] += 1

        self.platform.launchd_start = start_with_dependency_drift
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_restart_drained_runtime(self.context, self.platform)
        self.assertNotIn(api_label, self.platform.bootouts[-1:])
        self.assertTrue(
            ORCHESTRATOR.candidate_start_retirement_path(self.context).is_file()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_drained_runtime_restart_rechecks_expected_pid_after_ready_wait(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        self.platform.exit_launchd(runtime_label)
        original_wait = self.platform.wait_for_status

        def rotate_after_wait(probe, expected, timeout_seconds=60):
            status = original_wait(probe, expected, timeout_seconds)
            self.platform.next_pid += 1
            self.platform.pids[runtime_label] = self.platform.next_pid
            return status

        self.platform.wait_for_status = rotate_after_wait
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "drained_runtime_restart_final_observation_changed",
        ):
            ORCHESTRATOR.command_restart_drained_runtime(self.context, self.platform)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_drained_runtime_restart_fails_closed_on_phase_plist_and_transport_drift(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "orchestrator_phase_invalid"
        ):
            ORCHESTRATOR.command_restart_drained_runtime(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        plist_path = ORCHESTRATOR.service_plist_path(self.context, "runtime")
        original_plist = plist_path.read_bytes()
        value = plistlib.loads(original_plist)
        value["Label"] = f"{runtime_label}.drift"
        plist_path.write_bytes(plistlib.dumps(value))
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "candidate_plist_changed"
        ):
            ORCHESTRATOR.command_restart_drained_runtime(self.context, self.platform)
        plist_path.write_bytes(original_plist)
        self.platform.transport_state["instance_id"] = (
            "d2ti-fedcba9876543210fedcba9876543210"
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_restart_drained_runtime(self.context, self.platform)
        self.assertNotIn(runtime_label, self.platform.bootouts[-1:])
        self.platform.transport_state["instance_id"] = (
            "d2ti-0123456789abcdef0123456789abcdef"
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_preparing_state_allows_identity_bound_sigkill_cleanup(self):
        preflight = ORCHESTRATOR.command_dry_run(self.context, self.platform)
        self.context.artifact_directory.mkdir(mode=0o700, parents=True)
        ORCHESTRATOR.save_state(
            self.context, "preparing", preflight["standing_snapshot"]
        )
        CONTRACT.claim_discord_ownership(self.context)
        self.assertEqual(
            CONTRACT.load_discord_ownership_registry()["owners"],
            [CONTRACT.discord_ownership_record(self.context)],
        )
        self.isolated_root.mkdir(mode=0o700)
        ORCHESTRATOR.record_cleanup_root_identity(self.context)
        (self.isolated_root / "partial").write_text("partial", encoding="utf-8")
        result = ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(result["phase"], "cleaned")
        self.assertFalse(self.isolated_root.exists())
        self.assertEqual(CONTRACT.load_discord_ownership_registry()["owners"], [])
        replay = ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(replay["status"], "already_cleaned")

    def test_preparing_state_before_claim_cleans_and_replays(self):
        preflight = ORCHESTRATOR.command_dry_run(self.context, self.platform)
        self.context.artifact_directory.mkdir(mode=0o700, parents=True)
        ORCHESTRATOR.save_state(
            self.context, "preparing", preflight["standing_snapshot"]
        )
        self.assertFalse(self.registry_path.exists())
        result = ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(result["phase"], "cleaned")
        self.assertEqual(CONTRACT.load_discord_ownership_registry()["owners"], [])
        replay = ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(replay["status"], "already_cleaned")

    def test_release_before_cleaned_state_write_is_exactly_recoverable(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        real_save_state = ORCHESTRATOR.save_state

        def crash_on_cleaned(context, phase, snapshot):
            if phase == "cleaned":
                raise ORCHESTRATOR.OrchestratorError(
                    "injected_cleaned_state_write_crash"
                )
            return real_save_state(context, phase, snapshot)

        with mock.patch.object(
            ORCHESTRATOR, "save_state", side_effect=crash_on_cleaned
        ), self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "injected_cleaned_state_write_crash",
        ):
            ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertFalse(self.context.root.exists())
        self.assertEqual(CONTRACT.load_discord_ownership_registry()["owners"], [])
        self.assertEqual(ORCHESTRATOR.load_state(self.context)["phase"], "prepared")
        recovered = ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(recovered["phase"], "cleaned")
        self.assertEqual(CONTRACT.load_discord_ownership_registry()["owners"], [])

    def test_candidate_starting_state_is_bounded_stopped_then_retried(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        state = ORCHESTRATOR.load_state(self.context, {"prepared"})
        ORCHESTRATOR.configure_postgres_bootstrap_network(self.context)
        self.platform.postgres_start(self.context.cluster_root, self.context.postgres_log)
        label = self.context.manifest["services"]["worker"]["label"]
        self.platform.loaded.add(label)
        ORCHESTRATOR.save_state(
            self.context, "candidate_starting", state["standing_snapshot"]
        )
        result = ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertEqual(result["phase"], "candidate_started")
        self.assertIn(label, self.platform.bootouts)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_candidate_start_recovers_crash_after_source_publication(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        real_save_state = ORCHESTRATOR.save_state

        def crash_after_source(context, phase, snapshot):
            if phase == "candidate_started":
                raise ORCHESTRATOR.OrchestratorError("injected_after_source_publish")
            return real_save_state(context, phase, snapshot)

        with mock.patch.object(
            ORCHESTRATOR, "save_state", side_effect=crash_after_source
        ), self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "injected_after_source_publish"
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertEqual(
            ORCHESTRATOR.load_state(self.context)["phase"], "candidate_starting"
        )
        self.assertTrue(ORCHESTRATOR.candidate_start_source_path(self.context).is_file())
        original_pids = dict(self.platform.pids)
        recovered = ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertEqual(recovered["status"], "candidate_start_recovered")
        self.assertEqual(recovered["phase"], "candidate_started")
        self.assertEqual(self.platform.pids, original_pids)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_candidate_start_recovers_crash_before_source_publication(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        with mock.patch.object(
            ORCHESTRATOR,
            "publish_candidate_source",
            side_effect=ORCHESTRATOR.OrchestratorError(
                "injected_before_source_publish"
            ),
        ), self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "injected_before_source_publish"
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertTrue(
            ORCHESTRATOR.candidate_start_transition_path(self.context).is_file()
        )
        self.assertFalse(
            ORCHESTRATOR.candidate_start_source_path(self.context).exists()
        )
        original_pids = dict(self.platform.pids)
        recovered = ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertEqual(recovered["status"], "candidate_start_recovered")
        self.assertEqual(self.platform.pids, original_pids)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_candidate_start_identity_drift_requires_whole_run_retirement(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        real_save_state = ORCHESTRATOR.save_state

        def crash_after_source(context, phase, snapshot):
            if phase == "candidate_started":
                raise ORCHESTRATOR.OrchestratorError("injected_after_source_publish")
            return real_save_state(context, phase, snapshot)

        with mock.patch.object(
            ORCHESTRATOR, "save_state", side_effect=crash_after_source
        ), self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "injected_after_source_publish"
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        api_label = self.context.manifest["services"]["api"]["label"]
        prior_pid = self.platform.pids[api_label]
        replacement_pid = prior_pid + 100
        prior_start = self.platform.process_start_times[prior_pid]
        self.platform.pids[api_label] = replacement_pid
        self.platform.process_start_times[replacement_pid] = (
            prior_start[0] + 1,
            prior_start[1],
        )
        for _ in range(2):
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError,
                "candidate_start_transition_retirement_required",
            ):
                ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertTrue(
            ORCHESTRATOR.candidate_start_retirement_path(self.context).is_file()
        )
        self.platform.pids[api_label] = prior_pid
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertEqual(
            ORCHESTRATOR.load_state(self.context)["phase"], "candidate_starting"
        )
        self.assertTrue(ORCHESTRATOR.candidate_start_source_path(self.context).is_file())
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_candidate_start_rechecks_standing_before_transition_commit(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        protected_label = self.context.manifest["protected_staging"][
            "launchd_labels"
        ][0]
        real_build = ORCHESTRATOR.build_candidate_evidence

        def mutate_standing(context, statuses, platform):
            evidence = real_build(context, statuses, platform)
            platform.loaded.remove(protected_label)
            return evidence

        with mock.patch.object(
            ORCHESTRATOR, "build_candidate_evidence", side_effect=mutate_standing
        ), self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "protected_staging_state_changed",
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertFalse(
            ORCHESTRATOR.candidate_start_transition_path(self.context).exists()
        )
        self.assertFalse(
            ORCHESTRATOR.candidate_start_source_path(self.context).exists()
        )
        self.platform.loaded.add(protected_label)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_candidate_start_retires_identity_drift_after_source_publication(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        real_publish = ORCHESTRATOR.publish_candidate_source
        observed = {}

        def publish_then_replace(context, evidence, observed_at):
            path = real_publish(context, evidence, observed_at)
            api_label = context.manifest["services"]["api"]["label"]
            prior_pid = self.platform.pids[api_label]
            replacement_pid = prior_pid + 100
            prior_start = self.platform.process_start_times[prior_pid]
            self.platform.pids[api_label] = replacement_pid
            self.platform.process_start_times[replacement_pid] = (
                prior_start[0] + 1,
                prior_start[1],
            )
            observed.update(label=api_label, pid=prior_pid)
            return path

        with mock.patch.object(
            ORCHESTRATOR,
            "publish_candidate_source",
            side_effect=publish_then_replace,
        ), self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertTrue(
            ORCHESTRATOR.candidate_start_retirement_path(self.context).is_file()
        )
        self.platform.pids[observed["label"]] = observed["pid"]
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_candidate_started_source_drift_is_irreversibly_retired(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        path = ORCHESTRATOR.candidate_start_source_path(self.context)
        original = json.loads(path.read_text(encoding="utf-8"))
        changed = dict(original)
        changed["observed_at"] = "2026-08-04T01:02:03Z"
        path.write_text(
            ORCHESTRATOR.canonical_json(changed) + "\n", encoding="utf-8"
        )
        path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        retirement = json.loads(
            ORCHESTRATOR.candidate_start_retirement_path(
                self.context
            ).read_text(encoding="utf-8")
        )
        self.assertEqual(retirement["reason"], "candidate_source_drift")
        path.write_text(
            ORCHESTRATOR.canonical_json(original) + "\n", encoding="utf-8"
        )
        path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_onboard(
                self.context,
                self.platform,
                "discord:1056857223529250906",
                "보건",
            )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_candidate_started_api_replacement_is_irreversibly_retired(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        api_label = self.context.manifest["services"]["api"]["label"]
        prior_pid = self.platform.pids[api_label]
        replacement_pid = prior_pid + 100
        prior_start = self.platform.process_start_times[prior_pid]
        self.platform.pids[api_label] = replacement_pid
        self.platform.process_start_times[replacement_pid] = (
            prior_start[0] + 1,
            prior_start[1],
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        retirement = json.loads(
            ORCHESTRATOR.candidate_start_retirement_path(
                self.context
            ).read_text(encoding="utf-8")
        )
        self.assertEqual(retirement["reason"], "candidate_identity_drift")
        self.platform.pids[api_label] = prior_pid
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_empty_restart_directory_does_not_bypass_identity_replay(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        LIVE_RUNTIME_RESTART.ensure_live_runtime_restart_directory(self.context)
        api_label = self.context.manifest["services"]["api"]["label"]
        prior_pid = self.platform.pids[api_label]
        replacement_pid = prior_pid + 100
        prior_start = self.platform.process_start_times[prior_pid]
        self.platform.pids[api_label] = replacement_pid
        self.platform.process_start_times[replacement_pid] = (
            prior_start[0] + 1,
            prior_start[1],
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertTrue(
            ORCHESTRATOR.candidate_start_retirement_path(self.context).is_file()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_malformed_drained_restart_inventory_retires_candidate_run(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        directory = ORCHESTRATOR.drained_runtime_restart_directory(self.context)
        directory.mkdir(mode=0o700)
        intent = directory / "0001-intent.json"
        intent.write_text("{}\n", encoding="utf-8")
        intent.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertTrue(
            ORCHESTRATOR.candidate_start_retirement_path(self.context).is_file()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_pending_live_restart_api_drift_retires_at_candidate_boundary(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        api_label = self.context.manifest["services"]["api"]["label"]
        prior_pid = self.platform.pids[api_label]
        replacement_pid = prior_pid + 100
        prior_start = self.platform.process_start_times[prior_pid]
        self.platform.pids[api_label] = replacement_pid
        self.platform.process_start_times[replacement_pid] = (
            prior_start[0] + 1,
            prior_start[1],
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_resource_inventory(
                self.context, self.platform
            )
        self.assertTrue(
            ORCHESTRATOR.candidate_start_retirement_path(self.context).is_file()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_committed_step_11_malformed_intent_retires_at_candidate_boundary(self):
        self.certify_and_advance_live_restart()
        path = LIVE_RUNTIME_RESTART.live_runtime_restart_intent_path(
            self.context
        )
        path.write_text("{}\n", encoding="utf-8")
        path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_resource_inventory(
                self.context, self.platform
            )
        self.assertTrue(
            ORCHESTRATOR.candidate_start_retirement_path(self.context).is_file()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_committed_step_11_malformed_completion_retires_before_freeze(self):
        self.certify_and_advance_live_restart()
        path = LIVE_RUNTIME_RESTART.live_runtime_restart_complete_path(
            self.context
        )
        path.write_text("{}\n", encoding="utf-8")
        path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_finalize_run(
                self.context,
                self.platform,
                ORCHESTRATOR.command_teardown_discord_resources,
            )
        self.assertFalse(
            os.path.lexists(ORCHESTRATOR.freeze_intent_path(self.context))
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_committed_step_11_deleted_terminal_chain_retires_on_start(self):
        self.certify_and_advance_live_restart()
        LIVE_RUNTIME_RESTART.live_runtime_restart_intent_path(
            self.context
        ).unlink()
        LIVE_RUNTIME_RESTART.live_runtime_restart_complete_path(
            self.context
        ).unlink()
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertTrue(
            ORCHESTRATOR.candidate_start_retirement_path(self.context).is_file()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_committed_step_11_source_drift_retires_candidate_run(self):
        result = self.certify_and_advance_live_restart()
        path = pathlib.Path(result["coordinator_source"])
        value = json.loads(path.read_text(encoding="utf-8"))
        value["evidence"]["old_pid"] += 1
        path.write_text(
            ORCHESTRATOR.canonical_json(value) + "\n", encoding="utf-8"
        )
        path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_resource_inventory(
                self.context, self.platform
            )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_restart_commitment_missing_transition_retires_candidate_run(self):
        self.record_prerequisite_receipts()
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        ORCHESTRATOR.command_certify_live_runtime_restart(
            self.context, self.platform
        )
        transition = ORCHESTRATOR.candidate_start_transition_path(self.context)
        backup = transition.with_suffix(".backup")
        transition.rename(backup)
        try:
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError,
                "candidate_start_transition_retirement_required",
            ):
                ORCHESTRATOR.command_start(self.context, self.platform)
            self.assertTrue(
                ORCHESTRATOR.candidate_start_retirement_path(
                    self.context
                ).is_file()
            )
        finally:
            backup.rename(transition)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_unjournaled_drained_runtime_generation_retires_candidate_run(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        prior_pid = self.platform.pids[runtime_label]
        self.platform.next_pid += 1
        replacement_pid = self.platform.next_pid
        self.platform.pids[runtime_label] = replacement_pid
        self.platform.process_start_times[replacement_pid] = (
            1_700_000_000 + replacement_pid,
            replacement_pid % 1_000_000,
        )
        self.platform.launchd_runs[runtime_label] += 1
        self.platform.runtime_process_instance_ids[runtime_label] = (
            f"{replacement_pid:032x}"
        )
        self.platform.process_start_times.pop(prior_pid, None)
        self.platform.exit_launchd(runtime_label)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_restart_drained_runtime(
                self.context, self.platform
            )
        self.assertTrue(
            ORCHESTRATOR.candidate_start_retirement_path(self.context).is_file()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_second_drained_runtime_sequence_retires_without_relaunch(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        self.platform.exit_launchd(runtime_label)
        first = ORCHESTRATOR.command_restart_drained_runtime(
            self.context, self.platform
        )
        start_count = len(self.platform.start_order)
        self.platform.exit_launchd(runtime_label)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_restart_drained_runtime(
                self.context, self.platform
            )
        self.assertEqual(len(self.platform.start_order), start_count)
        records, pending = ORCHESTRATOR.drained_runtime_restart_inventory(
            self.context
        )
        self.assertIsNone(pending)
        self.assertEqual(len(records), 1)
        self.assertEqual(records[0]["complete"]["new_pid"], first["new_pid"])
        self.assertTrue(
            ORCHESTRATOR.candidate_start_retirement_path(self.context).is_file()
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_finalization_freeze_blocks_restart_without_retirement(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        with mock.patch.object(
            ORCHESTRATOR, "finalization_freeze_committed", return_value=True
        ), self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "orchestrator_phase_invalid"
        ):
            ORCHESTRATOR.command_restart_drained_runtime(
                self.context, self.platform
            )
        self.assertFalse(
            os.path.lexists(
                ORCHESTRATOR.candidate_start_retirement_path(self.context)
            )
        )
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_malformed_candidate_transition_is_irreversibly_retired(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        path = ORCHESTRATOR.candidate_start_transition_path(self.context)
        original = json.loads(path.read_text(encoding="utf-8"))
        changed = dict(original)
        changed["evidence_sha256"] = 7
        path.write_text(
            ORCHESTRATOR.canonical_json(changed) + "\n", encoding="utf-8"
        )
        path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        path.write_text(
            ORCHESTRATOR.canonical_json(original) + "\n", encoding="utf-8"
        )
        path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_coherent_malformed_candidate_evidence_is_irreversibly_retired(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        evidence_path = self.context.artifact_directory / "step-03-evidence.json"
        transition_path = ORCHESTRATOR.candidate_start_transition_path(
            self.context
        )
        evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
        transition = json.loads(transition_path.read_text(encoding="utf-8"))
        changed_evidence = dict(evidence)
        changed_evidence["process_identities"] = 1
        changed_transition = dict(transition)
        changed_transition["evidence_sha256"] = ORCHESTRATOR.digest_json(
            changed_evidence
        )
        evidence_path.write_text(
            ORCHESTRATOR.canonical_json(changed_evidence) + "\n",
            encoding="utf-8",
        )
        evidence_path.chmod(0o600)
        transition_path.write_text(
            ORCHESTRATOR.canonical_json(changed_transition) + "\n",
            encoding="utf-8",
        )
        transition_path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        evidence_path.write_text(
            ORCHESTRATOR.canonical_json(evidence) + "\n", encoding="utf-8"
        )
        evidence_path.chmod(0o600)
        transition_path.write_text(
            ORCHESTRATOR.canonical_json(transition) + "\n",
            encoding="utf-8",
        )
        transition_path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_onboarding_phase_rejects_start_without_retiring_recovery(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        state = ORCHESTRATOR.load_state(self.context, {"candidate_started"})
        ORCHESTRATOR.save_state(
            self.context, "onboarding", state["standing_snapshot"]
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "orchestrator_phase_invalid"
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertFalse(
            os.path.lexists(
                ORCHESTRATOR.candidate_start_retirement_path(self.context)
            )
        )
        result = ORCHESTRATOR.command_onboard(
            self.context,
            self.platform,
            "discord:1056857223529250906",
            "보건",
        )
        self.assertEqual(result["installation_id"], f"installation:{self.context.manifest['discord']['resource_prefix']}")
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_explicit_stop_retires_before_service_mutation(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        with mock.patch.object(
            self.platform,
            "launchd_bootout",
            side_effect=ORCHESTRATOR.OrchestratorError("injected_stop_failure"),
        ), self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "candidate_stop_incomplete"
        ):
            ORCHESTRATOR.command_stop(self.context, self.platform)
        retirement = json.loads(
            ORCHESTRATOR.candidate_start_retirement_path(
                self.context
            ).read_text(encoding="utf-8")
        )
        self.assertEqual(retirement["reason"], "explicit_stop")
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_explicit_cleanup_retires_before_cleanup_mutation(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        with mock.patch.object(
            ORCHESTRATOR,
            "cleanup",
            side_effect=ORCHESTRATOR.OrchestratorError(
                "injected_cleanup_failure"
            ),
        ), self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "injected_cleanup_failure"
        ):
            ORCHESTRATOR.command_cleanup(self.context, self.platform)
        retirement = json.loads(
            ORCHESTRATOR.candidate_start_retirement_path(
                self.context
            ).read_text(encoding="utf-8")
        )
        self.assertEqual(retirement["reason"], "explicit_cleanup")
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_certified_cleanup_stays_marker_free_and_cleaned_is_terminal(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)

        def finalize(
            context,
            platform,
            cleanup_boundary,
            teardown_boundary,
            identity_boundary,
        ):
            self.assertIs(teardown_boundary, ORCHESTRATOR.command_teardown_discord_resources)
            identity_boundary(context, platform, "capture", None)
            return cleanup_boundary(context, platform)

        with mock.patch.object(
            ORCHESTRATOR, "run_finalize_run", side_effect=finalize
        ):
            result = ORCHESTRATOR.command_finalize_run(
                self.context,
                self.platform,
                ORCHESTRATOR.command_teardown_discord_resources,
            )
        self.assertEqual(result["phase"], "cleaned")
        self.assertFalse(
            os.path.lexists(
                ORCHESTRATOR.candidate_start_retirement_path(self.context)
            )
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "orchestrator_phase_invalid"
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        self.assertFalse(
            os.path.lexists(
                ORCHESTRATOR.candidate_start_retirement_path(self.context)
            )
        )

    def test_onboarding_is_manifest_scoped_and_exactly_replayable(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "onboarding_principal_invalid"
        ):
            ORCHESTRATOR.command_onboard(
                self.context,
                self.platform,
                "discord:1056857223529250907",
                "보건",
            )
        result = ORCHESTRATOR.command_onboard(
            self.context,
            self.platform,
            "discord:1056857223529250906",
            "보건",
        )
        self.assertEqual(
            result["installation_id"],
            f"installation:{self.context.manifest['discord']['resource_prefix']}",
        )
        evidence = json.loads(
            (self.context.artifact_directory / "onboarding-evidence.json").read_text(
                encoding="utf-8"
            )
        )
        self.assertEqual(evidence["guild_id"], self.context.manifest["discord"]["guild_id"])
        self.assertEqual(evidence["binding_key"], "community_hub")
        self.assertEqual(
            evidence["hub_channel_id"],
            self.context.manifest["discord"]["hub_channel_id"],
        )
        self.assertEqual(result["binding_key"], "community_hub")
        self.assertEqual(
            result["hub_channel_id"],
            self.context.manifest["discord"]["hub_channel_id"],
        )
        coordinator_source = pathlib.Path(result["coordinator_source"])
        self.assertEqual(coordinator_source.stat().st_mode & 0o777, 0o600)
        source = json.loads(coordinator_source.read_text(encoding="utf-8"))
        self.assertEqual(
            source["kind"],
            "starring.d2.orchestrator-onboarding-evidence.v1",
        )
        self.assertEqual(source["manifest_sha256"], self.context.digest)
        replay = ORCHESTRATOR.command_onboard(
            self.context,
            self.platform,
            "discord:1056857223529250906",
            "보건",
        )
        self.assertEqual(
            replay["coordinator_source"], result["coordinator_source"]
        )
        self.assertNotIn("display_name", evidence)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_actual_start_and_onboarding_sources_advance_coordinator(self):
        self.context.artifact_directory.mkdir(mode=0o700, parents=True)
        prior_absence = D2_SOURCE_CONTRACT.publish_prior_absence_source(
            self.context,
            {
                "schema_version": 1,
                "kind": D2_SOURCE_CONTRACT.PREFLIGHT_KIND,
                "observed_at": "2026-08-04T01:02:03Z",
                "manifest_sha256": self.context.digest,
                "prior_runtime_owner_count": 0,
                "prior_smoke_process_count": 0,
                "standing_snapshot_sha256": "a" * 64,
                "external_credential_count": 3,
            },
        )
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        started = ORCHESTRATOR.command_start(self.context, self.platform)
        D2_RUN.advance_certification(
            self.manifest_path, 1, [started["coordinator_sources"]["1"]]
        )
        D2_RUN.advance_certification(
            self.manifest_path, 2, [str(prior_absence)]
        )
        D2_RUN.advance_certification(
            self.manifest_path, 3, [started["coordinator_sources"]["3"]]
        )
        onboarded = ORCHESTRATOR.command_onboard(
            self.context,
            self.platform,
            "discord:1056857223529250906",
            "보건",
        )
        authentication = self.root / "browser-authentication.json"
        authentication.write_text(
            D2_RUN.canonical_json(
                {
                    "schema_version": 1,
                    "kind": "starring.d2.browser-authentication-evidence.v1",
                    "observed_at": "2026-08-04T01:02:03Z",
                    **complete_certification_evidence(self.context.manifest)[4],
                }
            )
            + "\n",
            encoding="utf-8",
        )
        authentication.chmod(0o600)
        result = D2_RUN.advance_certification(
            self.manifest_path,
            4,
            [str(authentication), onboarded["coordinator_source"]],
        )
        self.assertEqual(result["step"], 4)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_platform_onboarding_output_is_bound_to_the_manifest_hub(self):
        installation_id = (
            f"installation:{self.context.manifest['discord']['resource_prefix']}"
        )
        principal_id = "discord:1056857223529250906"
        evidence = {
            "outcome": "fresh",
            "installation_id": installation_id,
            "principal_id": principal_id,
            "binding_key": "community_hub",
            "hub_channel_id": self.context.manifest["discord"]["hub_channel_id"],
        }
        platform = PLATFORM.Platform()

        def onboarding_completed(value):
            return subprocess.CompletedProcess(
                [], 0, json.dumps(value).encode("utf-8"), b""
            )

        hub = {
            "id": self.context.manifest["discord"]["hub_channel_id"],
            "guild_id": self.context.manifest["discord"]["guild_id"],
            "type": 0,
        }
        hub_completed = subprocess.CompletedProcess(
            [], 0, json.dumps(hub).encode("utf-8") + b"\n200", b""
        )
        with mock.patch.object(
            platform,
            "run",
            side_effect=[hub_completed, onboarding_completed(evidence)],
        ) as run:
            self.assertEqual(
                platform.onboard_installation(
                    self.context, principal_id, "보건", installation_id
                ),
                evidence,
            )
        preflight = run.call_args_list[0]
        command = preflight.args[0]
        self.assertEqual(command[:2], ["/bin/zsh", "-c"])
        self.assertIn('header = "Authorization: Bot %s"', command[2])
        self.assertIn("--config -", command[2])
        self.assertEqual(
            command[4:7],
            [
                self.context.manifest["external_keychain"]["discord_bot_token"][
                    "service"
                ],
                self.context.manifest["external_keychain"]["discord_bot_token"][
                    "account"
                ],
                "https://discord.com/api/v10/channels/"
                + self.context.manifest["discord"]["hub_channel_id"],
            ],
        )
        self.assertNotIn("environment", preflight.kwargs)
        self.assertEqual(
            run.call_args_list[1].args[0][0],
            str(self.candidates["sealed_provisioner"]),
        )
        for field, value in (
            ("binding_key", "another_hub"),
            ("hub_channel_id", "1524810437118525555"),
        ):
            invalid = {**evidence, field: value}
            with self.subTest(field=field):
                with mock.patch.object(
                    platform,
                    "run",
                    side_effect=[hub_completed, onboarding_completed(invalid)],
                ), self.assertRaisesRegex(
                    ORCHESTRATOR.OrchestratorError,
                    "installation_onboarding_output_invalid",
                ):
                    platform.onboard_installation(
                        self.context, principal_id, "보건", installation_id
                    )

    def test_discord_hub_preflight_rejects_wrong_scope_type_status_and_shape(self):
        installation_id = (
            f"installation:{self.context.manifest['discord']['resource_prefix']}"
        )
        principal_id = "discord:1056857223529250906"
        expected = {
            "id": self.context.manifest["discord"]["hub_channel_id"],
            "guild_id": self.context.manifest["discord"]["guild_id"],
            "type": 0,
        }
        cases = (
            (
                "wrong_id",
                {**expected, "id": "1524810437118525555"},
                200,
                "discord_hub_channel_preflight_response_invalid",
            ),
            (
                "wrong_guild",
                {**expected, "guild_id": "1524810437118525555"},
                200,
                "discord_hub_channel_preflight_response_invalid",
            ),
            (
                "wrong_type",
                {**expected, "type": 2},
                200,
                "discord_hub_channel_preflight_response_invalid",
            ),
            (
                "wrong_status",
                expected,
                404,
                "discord_hub_channel_preflight_status_invalid",
            ),
            (
                "wrong_shape",
                [expected],
                200,
                "discord_hub_channel_preflight_response_invalid",
            ),
        )
        platform = PLATFORM.Platform()
        for name, body, status, code in cases:
            response = subprocess.CompletedProcess(
                [],
                0,
                json.dumps(body).encode("utf-8") + f"\n{status}".encode("ascii"),
                b"",
            )
            with self.subTest(name=name), mock.patch.object(
                platform, "run", return_value=response
            ) as run, self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, code
            ):
                platform.onboard_installation(
                    self.context, principal_id, "보건", installation_id
                )
            self.assertEqual(run.call_count, 1)

    def test_cleanup_refuses_symlinked_root_and_recovers_after_operator_restore(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        preserved = self.isolated_root.with_name(f"{self.isolated_root.name}-preserved")
        sentinel = self.root / "cleanup-external-sentinel"
        sentinel.write_text("preserve", encoding="utf-8")
        bootouts = list(self.platform.bootouts)
        deletes = list(self.platform.keychain_deletes)
        postgres = self.platform.postgres
        journal = self.context.journal_path.read_bytes()
        self.isolated_root.rename(preserved)
        self.isolated_root.symlink_to(self.root)
        try:
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, "cleanup_root_invalid"
            ):
                ORCHESTRATOR.command_cleanup(self.context, self.platform)
            self.assertEqual(self.platform.bootouts, bootouts)
            self.assertEqual(self.platform.keychain_deletes, deletes)
            self.assertEqual(self.platform.postgres, postgres)
            self.assertEqual(self.context.journal_path.read_bytes(), journal)
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserve")
        finally:
            self.isolated_root.unlink()
            preserved.rename(self.isolated_root)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertFalse(self.isolated_root.exists())

    def test_cleanup_refuses_symlinked_cluster_before_any_mutation(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        cluster = self.context.cluster_root
        preserved = cluster.with_name(cluster.name + "-preserved")
        outside = self.root / "cleanup-external-cluster"
        outside.mkdir(mode=0o700)
        sentinel = outside / "sentinel"
        sentinel.write_text("preserve", encoding="utf-8")
        bootouts = list(self.platform.bootouts)
        deletes = list(self.platform.keychain_deletes)
        postgres = self.platform.postgres
        journal = self.context.journal_path.read_bytes()
        cluster.rename(preserved)
        cluster.symlink_to(outside, target_is_directory=True)
        try:
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, "cleanup_cluster_invalid"
            ):
                ORCHESTRATOR.command_cleanup(self.context, self.platform)
            self.assertEqual(self.platform.bootouts, bootouts)
            self.assertEqual(self.platform.keychain_deletes, deletes)
            self.assertEqual(self.platform.postgres, postgres)
            self.assertEqual(self.context.journal_path.read_bytes(), journal)
            self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserve")
        finally:
            cluster.unlink()
            preserved.rename(cluster)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_cleanup_refuses_keychain_namespace_without_matching_owner(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        owner = CONTRACT.discord_ownership_record(self.context)
        service = self.context.manifest["keychain_services"]["api"]
        account = ORCHESTRATOR.keychain_inventory(self.context)[0][1]
        self.platform.keychain.add((service, account))
        self.platform.owner_values[service] = "different-run"
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
        ):
            ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertIn((service, account), self.platform.keychain)
        self.assertEqual(
            CONTRACT.load_discord_ownership_registry()["owners"], [owner]
        )
        self.platform.owner_values[service] = self.context.manifest["run_id"]
        ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(CONTRACT.load_discord_ownership_registry()["owners"], [])

    def test_cleanup_does_not_delete_keychain_replacement_at_delete_boundary(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        service = self.context.manifest["keychain_services"]["api"]
        account = ORCHESTRATOR.OWNER_ACCOUNT
        target = (service, account)
        self.platform.keychain_replace_at_delete = target
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
        ):
            ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertIn(target, self.platform.keychain)
        self.assertNotIn(target, self.platform.keychain_deletes)

    def test_cleanup_replay_preserves_replacement_after_delete_crash(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        service = self.context.manifest["keychain_services"]["api"]
        account = next(
            account
            for item_service, account in ORCHESTRATOR.keychain_inventory(
                self.context
            )
            if item_service == service and account != ORCHESTRATOR.OWNER_ACCOUNT
        )
        target = (service, account)
        self.platform.keychain.add(target)
        self.platform.keychain_crash_after_delete = target
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
        ):
            ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertNotIn(target, self.platform.keychain)
        self.platform.keychain_crash_after_delete = None
        self.platform.keychain.add(target)
        self.platform.keychain_identity_versions[target] = 1
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
        ):
            ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertIn(target, self.platform.keychain)
        self.platform.keychain.remove(target)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_cleanup_rejects_root_replaced_after_prepare_identity_capture(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        preserved = self.isolated_root.with_name(
            f".{self.isolated_root.name}.preserved-{secrets.token_hex(4)}"
        )
        self.isolated_root.rename(preserved)
        self.isolated_root.mkdir(mode=0o700)
        try:
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError,
                "cleanup_root_swap_detected",
            ):
                ORCHESTRATOR.command_cleanup(self.context, self.platform)
            self.assertTrue(self.isolated_root.exists())
            self.assertTrue(preserved.exists())
            self.assertEqual(self.platform.keychain_deletes, [])
        finally:
            self.isolated_root.rmdir()
            preserved.rename(self.isolated_root)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_cleanup_rejects_root_device_drift_before_mutation(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        real_metadata = ORCHESTRATOR.cleanup_path_metadata

        def drifted_metadata(path, code):
            metadata = real_metadata(path, code)
            if pathlib.Path(path) == self.isolated_root and metadata is not None:
                return SimpleNamespace(
                    st_mode=metadata.st_mode,
                    st_uid=metadata.st_uid,
                    st_dev=metadata.st_dev + 1,
                    st_ino=metadata.st_ino,
                )
            return metadata

        with mock.patch.object(
            ORCHESTRATOR,
            "cleanup_path_metadata",
            side_effect=drifted_metadata,
        ):
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError,
                "cleanup_root_invalid",
            ):
                ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertTrue(self.isolated_root.exists())
        self.assertEqual(self.platform.keychain_deletes, [])
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_cleanup_requires_prepare_time_root_identity(self):
        preflight = ORCHESTRATOR.command_dry_run(self.context, self.platform)
        self.context.artifact_directory.mkdir(mode=0o700, parents=True)
        ORCHESTRATOR.save_state(
            self.context, "preparing", preflight["standing_snapshot"]
        )
        CONTRACT.claim_discord_ownership(self.context)
        self.isolated_root.mkdir(mode=0o700)
        partial = self.isolated_root / "partial"
        partial.write_text("partial", encoding="utf-8")
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "cleanup_root_identity_invalid",
        ):
            ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertTrue(partial.exists())
        partial.unlink()
        self.isolated_root.rmdir()
        CONTRACT.release_discord_ownership(self.context)

    def test_cleanup_resumes_after_crash_between_quarantine_and_phase_write(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        real_save = ORCHESTRATOR.save_cleanup_root_progress
        injected = False

        def fail_once(context, progress, phase):
            nonlocal injected
            if phase == "quarantined" and not injected:
                injected = True
                ORCHESTRATOR.fail("injected_quarantine_crash")
            return real_save(context, progress, phase)

        with mock.patch.object(
            ORCHESTRATOR,
            "save_cleanup_root_progress",
            side_effect=fail_once,
        ):
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
            ):
                ORCHESTRATOR.command_cleanup(self.context, self.platform)
        quarantine = ORCHESTRATOR.cleanup_root_quarantine_path(self.context)
        self.assertFalse(self.isolated_root.exists())
        self.assertTrue(quarantine.exists())
        self.assertEqual(
            ORCHESTRATOR.load_cleanup_root_progress(self.context)["phase"],
            "planned",
        )
        result = ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(result["phase"], "cleaned")
        self.assertFalse(quarantine.exists())
        self.assertEqual(
            ORCHESTRATOR.load_cleanup_root_progress(self.context)["phase"],
            "deleted",
        )

    def test_cleanup_reobserves_postgres_and_launchd_after_quarantine(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        real_observation = (
            ORCHESTRATOR.require_quarantined_cleanup_substrate_inert
        )
        injected_postgres = False

        def activate_postgres(context, platform):
            nonlocal injected_postgres
            if not injected_postgres:
                injected_postgres = True
                platform.postgres = True
            return real_observation(context, platform)

        with mock.patch.object(
            ORCHESTRATOR,
            "require_quarantined_cleanup_substrate_inert",
            side_effect=activate_postgres,
        ):
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
            ):
                ORCHESTRATOR.command_cleanup(self.context, self.platform)
        quarantine = ORCHESTRATOR.cleanup_root_quarantine_path(self.context)
        self.assertTrue(quarantine.exists())
        self.assertEqual(
            ORCHESTRATOR.load_cleanup_root_progress(self.context)["phase"],
            "planned",
        )
        self.platform.postgres = False
        runtime_label = self.context.manifest["services"]["runtime"]["label"]
        injected_launchd = False

        def activate_launchd(context, platform):
            nonlocal injected_launchd
            if not injected_launchd:
                injected_launchd = True
                platform.loaded.add(runtime_label)
            return real_observation(context, platform)

        with mock.patch.object(
            ORCHESTRATOR,
            "require_quarantined_cleanup_substrate_inert",
            side_effect=activate_launchd,
        ):
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
            ):
                ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertTrue(quarantine.exists())
        self.platform.loaded.discard(runtime_label)
        result = ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(result["phase"], "cleaned")
        self.assertFalse(quarantine.exists())

    def test_cleanup_does_not_certify_loss_before_quarantine_recheck(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        with mock.patch.object(
            ORCHESTRATOR,
            "require_quarantined_cleanup_substrate_inert",
            side_effect=ORCHESTRATOR.OrchestratorError(
                "injected_pre_recheck_crash"
            ),
        ):
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
            ):
                ORCHESTRATOR.command_cleanup(self.context, self.platform)
        quarantine = ORCHESTRATOR.cleanup_root_quarantine_path(self.context)
        self.assertEqual(
            ORCHESTRATOR.load_cleanup_root_progress(self.context)["phase"],
            "planned",
        )
        preserved = quarantine.with_name(
            f"{quarantine.name}.external-{secrets.token_hex(4)}"
        )
        quarantine.rename(preserved)
        try:
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
            ):
                ORCHESTRATOR.command_cleanup(self.context, self.platform)
            self.assertEqual(
                ORCHESTRATOR.load_cleanup_root_progress(self.context)["phase"],
                "planned",
            )
        finally:
            preserved.rename(quarantine)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_cleanup_recovers_only_verified_delete_completion_window(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        real_save = ORCHESTRATOR.save_cleanup_root_progress
        injected = False

        def fail_deleted_write(context, progress, phase):
            nonlocal injected
            if phase == "deleted" and not injected:
                injected = True
                ORCHESTRATOR.fail("injected_deleted_phase_crash")
            return real_save(context, progress, phase)

        with mock.patch.object(
            ORCHESTRATOR,
            "save_cleanup_root_progress",
            side_effect=fail_deleted_write,
        ):
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
            ):
                ORCHESTRATOR.command_cleanup(self.context, self.platform)
        quarantine = ORCHESTRATOR.cleanup_root_quarantine_path(self.context)
        self.assertFalse(self.isolated_root.exists())
        self.assertFalse(quarantine.exists())
        self.assertEqual(
            ORCHESTRATOR.load_cleanup_root_progress(self.context)["phase"],
            "quarantined",
        )
        result = ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(result["phase"], "cleaned")
        self.assertEqual(
            ORCHESTRATOR.load_cleanup_root_progress(self.context)["phase"],
            "deleted",
        )

    def test_cleanup_exclusive_rename_rejects_destination_race(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        quarantine = ORCHESTRATOR.cleanup_root_quarantine_path(self.context)

        def race_destination(
            source_directory,
            source_name,
            destination_directory,
            destination_name,
        ):
            quarantine.mkdir(mode=0o700)
            return PLATFORM.rename_exclusive(
                source_directory,
                source_name,
                destination_directory,
                destination_name,
            )

        with mock.patch.object(
            ORCHESTRATOR,
            "rename_exclusive",
            side_effect=race_destination,
        ):
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
            ):
                ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertTrue(self.isolated_root.exists())
        self.assertTrue(quarantine.is_dir())
        quarantine.rmdir()
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_cleanup_rejects_external_root_loss_without_progress(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        preserved = self.isolated_root.with_name(
            f".{self.isolated_root.name}.lost-{secrets.token_hex(4)}"
        )
        self.isolated_root.rename(preserved)
        try:
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
            ):
                ORCHESTRATOR.command_cleanup(self.context, self.platform)
            self.assertTrue(preserved.exists())
            self.assertFalse(
                ORCHESTRATOR.cleanup_root_progress_path(self.context).exists()
            )
        finally:
            preserved.rename(self.isolated_root)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_cleanup_rejects_external_root_loss_from_planned_progress(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        identity = ORCHESTRATOR.load_cleanup_root_identity(self.context)
        progress = {
            "schema_version": 1,
            "kind": ORCHESTRATOR.CLEANUP_ROOT_PROGRESS_KIND,
            "manifest_sha256": self.context.digest,
            "run_id": self.context.manifest["run_id"],
            "root_device": identity["root_device"],
            "root_inode": identity["root_inode"],
            "quarantine_name": ORCHESTRATOR.cleanup_root_quarantine_name(
                self.context
            ),
            "phase": "planned",
        }
        ORCHESTRATOR.write_atomic(
            ORCHESTRATOR.cleanup_root_progress_path(self.context),
            ORCHESTRATOR.canonical_json(progress) + "\n",
        )
        preserved = self.isolated_root.with_name(
            f".{self.isolated_root.name}.planned-{secrets.token_hex(4)}"
        )
        self.isolated_root.rename(preserved)
        try:
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
            ):
                ORCHESTRATOR.command_cleanup(self.context, self.platform)
            self.assertEqual(
                ORCHESTRATOR.load_cleanup_root_progress(self.context)["phase"],
                "planned",
            )
        finally:
            preserved.rename(self.isolated_root)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_postgres_observation_error_blocks_root_deletion(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        self.platform.postgres_observation_error = True
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
        ):
            ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertTrue(self.isolated_root.exists())
        self.platform.postgres_observation_error = False
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_launchd_observation_error_blocks_root_deletion(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        self.platform.launchd_observation_error = True
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
        ):
            ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertTrue(self.isolated_root.exists())
        self.platform.launchd_observation_error = False
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_missing_root_still_requires_process_path_observation(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        preserved = self.isolated_root.with_name(
            f".{self.isolated_root.name}.missing-{secrets.token_hex(4)}"
        )
        self.isolated_root.rename(preserved)
        self.platform.postgres_observation_error = True
        try:
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
            ):
                ORCHESTRATOR.command_cleanup(self.context, self.platform)
            self.assertIn(
                self.context.cluster_root,
                self.platform.postgres_process_observations,
            )
            self.assertTrue(preserved.exists())
        finally:
            self.platform.postgres_observation_error = False
            preserved.rename(self.isolated_root)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_cleaned_replay_rejects_corrupt_evidence_and_progress(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)
        evidence_path = self.context.artifact_directory / "cleanup-evidence.json"
        evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
        corrupted = {**evidence, "isolated_root_absent": False}
        evidence_path.write_text(json.dumps(corrupted), encoding="utf-8")
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "cleanup_evidence_invalid"
        ):
            ORCHESTRATOR.command_cleanup(self.context, self.platform)
        evidence_path.write_text(json.dumps(evidence), encoding="utf-8")
        progress_path = ORCHESTRATOR.cleanup_root_progress_path(self.context)
        progress = json.loads(progress_path.read_text(encoding="utf-8"))
        progress_path.write_text(
            json.dumps({**progress, "phase": "planned"}), encoding="utf-8"
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "cleanup_root_progress_not_terminal",
        ):
            ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_cleaned_replay_reconstructs_only_missing_cleanup_evidence(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)
        evidence_path = self.context.artifact_directory / "cleanup-evidence.json"
        evidence_path.unlink()
        result = ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(result["status"], "already_cleaned")
        ORCHESTRATOR.validate_cleanup_evidence(
            self.context,
            json.loads(evidence_path.read_text(encoding="utf-8")),
        )

    def test_cleaned_replay_completes_journal_after_evidence_write_crash(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        evidence_path = self.context.artifact_directory / "cleanup-evidence.json"
        real_write = ORCHESTRATOR.write_atomic
        injected = False

        def fail_evidence_write(path, payload, mode=0o600):
            nonlocal injected
            if pathlib.Path(path) == evidence_path and not injected:
                injected = True
                ORCHESTRATOR.fail("injected_cleanup_evidence_crash")
            return real_write(path, payload, mode)

        with mock.patch.object(
            ORCHESTRATOR,
            "write_atomic",
            side_effect=fail_evidence_write,
        ):
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError,
                "injected_cleanup_evidence_crash",
            ):
                ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(ORCHESTRATOR.load_state(self.context)["phase"], "cleaned")
        self.assertFalse(evidence_path.exists())
        replay = ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(replay["status"], "already_cleaned")
        cleanup_rows = [
            row
            for row in ORCHESTRATOR.cleanup_journal_rows(self.context)
            if row["action"] == "cleanup"
        ]
        self.assertEqual(cleanup_rows[-1]["status"], "complete")

    def test_cleaned_replay_completes_journal_after_complete_append_crash(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        real_append = ORCHESTRATOR.append_journal
        injected = False

        def fail_complete(context, action, status, target):
            nonlocal injected
            if action == "cleanup" and status == "complete" and not injected:
                injected = True
                ORCHESTRATOR.fail("injected_cleanup_journal_crash")
            return real_append(context, action, status, target)

        with mock.patch.object(
            ORCHESTRATOR,
            "append_journal",
            side_effect=fail_complete,
        ):
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError,
                "injected_cleanup_journal_crash",
            ):
                ORCHESTRATOR.command_cleanup(self.context, self.platform)
        evidence_path = self.context.artifact_directory / "cleanup-evidence.json"
        self.assertTrue(evidence_path.exists())
        replay = ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(replay["status"], "already_cleaned")
        cleanup_rows = [
            row
            for row in ORCHESTRATOR.cleanup_journal_rows(self.context)
            if row["action"] == "cleanup"
        ]
        self.assertEqual(cleanup_rows[-1]["status"], "complete")

    def test_cleanup_never_dispatches_a_protected_label_or_credential(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)
        protected_labels = set(self.context.manifest["protected_staging"]["launchd_labels"])
        self.assertTrue(protected_labels.isdisjoint(self.platform.bootouts))
        self.assertTrue(
            set(ORCHESTRATOR.STANDING_DISCORD_IDENTITIES).isdisjoint(
                self.platform.keychain_deletes
            )
        )

    def test_standing_origin_is_rejected_before_lifecycle(self):
        manifest = json.loads(self.manifest_path.read_text(encoding="utf-8"))
        manifest["public_origin"] = ORCHESTRATOR.STANDING_PUBLIC_ORIGIN
        self.manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        self.manifest_path.with_name("manifest.sha256").write_text(
            CERTIFICATION.manifest_digest(manifest) + "\n", encoding="utf-8"
        )
        certification_error = CONTRACT.load_verified_manifest.__globals__[
            "CertificationError"
        ]
        with self.assertRaisesRegex(
            certification_error, "cloudflare_route_binding_invalid"
        ):
            ORCHESTRATOR.load_context(self.manifest_path)

    def test_standing_origin_default_port_case_and_trailing_dot_are_canonicalized(self):
        self.assertEqual(
            CERTIFICATION.validate_public_origin("https://API.STARRING.CO.KR.:443"),
            ORCHESTRATOR.STANDING_PUBLIC_ORIGIN,
        )

    def test_global_lock_rejects_concurrent_d2_mutation(self):
        descriptor = ORCHESTRATOR.os.open(
            ORCHESTRATOR.GLOBAL_LOCK_PATH,
            ORCHESTRATOR.os.O_RDWR | ORCHESTRATOR.os.O_CREAT,
            0o600,
        )
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, "d2_operation_busy"
            ):
                with ORCHESTRATOR.global_operation_lock():
                    pass
        finally:
            fcntl.flock(descriptor, fcntl.LOCK_UN)
            ORCHESTRATOR.os.close(descriptor)


class LaunchdPlatformTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name).resolve()
        candidate = self.root / "api"
        candidate.write_bytes(b"candidate:api")
        candidate.chmod(0o555)
        self.candidates = {"api": candidate}

    def tearDown(self):
        self.temporary.cleanup()

    def test_launchd_signal_is_exactly_sigterm_and_manifest_label_scoped(self):
        platform = PLATFORM.Platform()
        success = subprocess.CompletedProcess([], 0, b"", b"")
        with mock.patch.object(platform, "run", return_value=success) as run:
            platform.launchd_signal("local.starring.d2.test.runtime", "SIGTERM")
        self.assertEqual(
            run.call_args.args[0],
            [
                CONTRACT.REQUIRED_PROGRAMS["launchctl"],
                "kill",
                "SIGTERM",
                f"gui/{ORCHESTRATOR.os.getuid()}/local.starring.d2.test.runtime",
            ],
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "launchd_signal_invalid"
        ):
            platform.launchd_signal("local.starring.d2.test.runtime", "SIGKILL")

    def test_candidate_process_identity_binds_kernel_path_start_and_fd_digest(self):
        platform = PLATFORM.Platform()
        candidate = self.candidates["api"]
        metadata = candidate.stat()
        kernel = {
            "pid": 1234,
            "start_time_seconds": 1_700_000_000,
            "start_time_microseconds": 123456,
            "uid": metadata.st_uid,
            "path": str(candidate),
        }
        with mock.patch.object(
            platform, "_kernel_process_identity", side_effect=[kernel, kernel]
        ):
            observed = platform.candidate_process_identity(1234, candidate)
        self.assertEqual(observed["pid"], 1234)
        self.assertEqual(observed["path"], str(candidate))
        self.assertEqual(
            observed["sha256"], hashlib.sha256(candidate.read_bytes()).hexdigest()
        )
        self.assertEqual(observed["inode"], metadata.st_ino)

    def test_candidate_process_identity_rejects_kernel_and_path_races(self):
        platform = PLATFORM.Platform()
        candidate = self.candidates["api"]
        metadata = candidate.stat()
        kernel = {
            "pid": 1234,
            "start_time_seconds": 1_700_000_000,
            "start_time_microseconds": 123456,
            "uid": metadata.st_uid,
            "path": str(candidate),
        }
        changed = dict(kernel)
        changed["start_time_microseconds"] += 1
        with mock.patch.object(
            platform, "_kernel_process_identity", side_effect=[kernel, changed]
        ), self.assertRaisesRegex(
            CONTRACT.OrchestratorError, "process_identity_changed_during_observation"
        ):
            platform.candidate_process_identity(1234, candidate)
        replaced = SimpleNamespace(
            st_dev=metadata.st_dev,
            st_ino=metadata.st_ino + 1,
            st_mode=metadata.st_mode,
            st_uid=metadata.st_uid,
            st_nlink=metadata.st_nlink,
            st_size=metadata.st_size,
            st_mtime_ns=metadata.st_mtime_ns,
            st_ctime_ns=metadata.st_ctime_ns,
        )
        with mock.patch.object(
            platform, "_kernel_process_identity", side_effect=[kernel, kernel]
        ), mock.patch.object(
            PLATFORM.os, "stat", return_value=replaced
        ), self.assertRaisesRegex(
            CONTRACT.OrchestratorError, "process_identity_changed_during_observation"
        ):
            platform.candidate_process_identity(1234, candidate)

    def test_candidate_process_identity_rejects_symlink_hardlink_and_bad_modes(self):
        platform = PLATFORM.Platform()
        directory = self.root / "process-identity-files"
        directory.mkdir(mode=0o700)
        regular = directory / "regular"
        regular.write_bytes(b"candidate")
        regular.chmod(0o555)
        symlink = directory / "symlink"
        symlink.symlink_to(regular)
        hardlink = directory / "hardlink"
        os.link(regular, hardlink)
        writable = directory / "writable"
        writable.write_bytes(b"candidate")
        writable.chmod(0o755)
        non_executable = directory / "non-executable"
        non_executable.write_bytes(b"candidate")
        non_executable.chmod(0o444)
        empty = directory / "empty"
        empty.write_bytes(b"")
        empty.chmod(0o555)
        for path, pattern in (
            (symlink, "process_identity_executable_unavailable"),
            (hardlink, "process_identity_executable_invalid"),
            (writable, "process_identity_executable_invalid"),
            (non_executable, "process_identity_executable_invalid"),
            (empty, "process_identity_executable_invalid"),
        ):
            with self.subTest(path=path):
                kernel = {
                    "pid": 1234,
                    "start_time_seconds": 1_700_000_000,
                    "start_time_microseconds": 123456,
                    "uid": path.lstat().st_uid,
                    "path": str(path),
                }
                with mock.patch.object(
                    platform,
                    "_kernel_process_identity",
                    side_effect=[kernel, kernel],
                ), self.assertRaisesRegex(CONTRACT.OrchestratorError, pattern):
                    platform.candidate_process_identity(1234, path)

    def test_kernel_process_identity_rejects_pidinfo_and_pidpath_boundaries(self):
        platform = PLATFORM.Platform()

        class Libproc:
            def __init__(
                self,
                pidinfo_size=None,
                path=b"/private/tmp/candidate",
                pid_value=None,
                uid=None,
                seconds=1_700_000_000,
                microseconds=123456,
                path_return=None,
            ):
                self.pidinfo_size = pidinfo_size
                self.path = path
                self.pid_value = pid_value
                self.uid = uid
                self.seconds = seconds
                self.microseconds = microseconds
                self.path_return = path_return

            def proc_pidinfo(self, pid, _flavor, _arg, pointer, size):
                if self.pidinfo_size is not None:
                    return self.pidinfo_size
                information = ctypes.cast(
                    pointer, ctypes.POINTER(PLATFORM.ProcBsdInfo)
                ).contents
                information.pbi_pid = pid if self.pid_value is None else self.pid_value
                information.pbi_uid = os.getuid() if self.uid is None else self.uid
                information.pbi_start_tvsec = self.seconds
                information.pbi_start_tvusec = self.microseconds
                return size

            def proc_pidpath(self, _pid, buffer, _size):
                ctypes.memmove(buffer, self.path + b"\x00", len(self.path) + 1)
                return len(self.path) if self.path_return is None else self.path_return

        for pid, library, pattern in (
            (True, Libproc(), "process_identity_pid_invalid"),
            (2_147_483_648, Libproc(), "process_identity_pid_invalid"),
            (1234, Libproc(135), "process_identity_bsdinfo_unavailable"),
            (1234, Libproc(pid_value=4321), "process_identity_kernel_invalid"),
            (1234, Libproc(uid=os.getuid() + 1), "process_identity_kernel_invalid"),
            (1234, Libproc(seconds=0), "process_identity_kernel_invalid"),
            (1234, Libproc(microseconds=1_000_000), "process_identity_kernel_invalid"),
            (1234, Libproc(path_return=0), "process_identity_path_unavailable"),
            (
                1234,
                Libproc(path_return=PLATFORM.PROC_PIDPATHINFO_MAXSIZE),
                "process_identity_path_unavailable",
            ),
            (1234, Libproc(path=b"relative"), "process_identity_kernel_invalid"),
            (1234, Libproc(path=b"\xff"), "process_identity_path_invalid"),
        ):
            with self.subTest(pid=pid, pattern=pattern), mock.patch.object(
                platform, "_libproc", return_value=library
            ), self.assertRaisesRegex(CONTRACT.OrchestratorError, pattern):
                platform._kernel_process_identity(pid)
        mismatch = Libproc()
        mismatch.proc_pidpath = lambda _pid, _buffer, _size: 1
        with mock.patch.object(
            platform, "_libproc", return_value=mismatch
        ), self.assertRaisesRegex(
            CONTRACT.OrchestratorError, "process_identity_path_invalid"
        ):
            platform._kernel_process_identity(1234)

    def test_launchd_job_observation_binds_program_and_optional_pid(self):
        platform = PLATFORM.Platform()
        running = subprocess.CompletedProcess(
            [],
            0,
            b"\tpath = /Users/test/Application Support/Starring/runtime.plist\n\tprogram = /Users/test/Application Support/Starring/starring-runtime\n\targuments = {\n\t\t/Users/test/Application Support/Starring/starring-runtime\n\t\t--config\n\t\t/Users/test/Application Support/Starring/runtime config.json\n\t}\n\tstate = running\n\truns = 3\n\tpid = 49152\n\tlast exit code = (never exited)\n\t\tstate = active\n\t\tpath = nested-noise\n",
            b"",
        )
        with mock.patch.object(platform, "run", return_value=running):
            self.assertEqual(
                platform.launchd_job("local.starring.d2.test.runtime"),
                {
                    "pid": 49152,
                    "program": "/Users/test/Application Support/Starring/starring-runtime",
                    "plist_path": "/Users/test/Application Support/Starring/runtime.plist",
                    "arguments": [
                        "/Users/test/Application Support/Starring/starring-runtime",
                        "--config",
                        "/Users/test/Application Support/Starring/runtime config.json",
                    ],
                    "runs": 3,
                    "state": "running",
                    "last_exit_code": None,
                },
            )
        exited = subprocess.CompletedProcess(
            [],
            0,
            b"\tpath = /Users/test/Application Support/Starring/runtime.plist\n\tprogram = /Users/test/Application Support/Starring/starring-runtime\n\targuments = {\n\t\t/Users/test/Application Support/Starring/starring-runtime\n\t}\n\tstate = exited\n\truns = 1\n\tlast exit code = 0\n",
            b"",
        )
        with mock.patch.object(platform, "run", return_value=exited):
            self.assertEqual(
                platform.launchd_job("local.starring.d2.test.runtime"),
                {
                    "pid": None,
                    "program": "/Users/test/Application Support/Starring/starring-runtime",
                    "plist_path": "/Users/test/Application Support/Starring/runtime.plist",
                    "arguments": [
                        "/Users/test/Application Support/Starring/starring-runtime"
                    ],
                    "runs": 1,
                    "state": "exited",
                    "last_exit_code": 0,
                },
            )
        decorated = subprocess.CompletedProcess(
            [],
            0,
            b"\tpath = /Users/test/Application Support/Starring/runtime.plist\n\tprogram = /Users/test/Application Support/Starring/starring-runtime\n\targuments = {\n\t\t/Users/test/Application Support/Starring/starring-runtime\n\t}\n\tstate = exited\n\truns = 2\n\tlast exit code = 70: EX_SOFTWARE\n",
            b"",
        )
        with mock.patch.object(platform, "run", return_value=decorated):
            self.assertEqual(
                platform.launchd_job("local.starring.d2.test.runtime")[
                    "last_exit_code"
                ],
                70,
            )

    def test_launchd_job_rejects_ambiguous_exit_decorations(self):
        platform = PLATFORM.Platform()
        invalid = [
            "0: EX_OK",
            "70: ex_software",
            "70: EX SOFTWARE",
            "70: EX-SOFTWARE",
            "70 garbage",
            f"70: {'X' * 65}",
        ]
        for exit_value in invalid:
            output = (
                "\tpath = /Users/test/runtime.plist\n"
                "\tprogram = /Users/test/starring-runtime\n"
                "\targuments = {\n"
                "\t\t/Users/test/starring-runtime\n"
                "\t}\n"
                "\tstate = exited\n"
                "\truns = 1\n"
                f"\tlast exit code = {exit_value}\n"
            ).encode("utf-8")
            observed = subprocess.CompletedProcess([], 0, output, b"")
            with self.subTest(exit_value=exit_value), mock.patch.object(
                platform, "run", return_value=observed
            ), self.assertRaisesRegex(
                CONTRACT.OrchestratorError, "launchd_observation_invalid"
            ):
                platform.launchd_job("local.starring.d2.test.runtime")

    def test_launchd_job_normalizes_real_not_running_state(self):
        platform = PLATFORM.Platform()
        observed = subprocess.CompletedProcess(
            [],
            0,
            b"\tpath = /Users/test/runtime.plist\n\tprogram = /Users/test/starring-runtime\n\targuments = {\n\t\t/Users/test/starring-runtime\n\t}\n\tstate = not running\n\truns = 1\n\tlast exit code = 0\n\t\tstate = active\n",
            b"",
        )
        with mock.patch.object(platform, "run", return_value=observed):
            self.assertEqual(
                platform.launchd_job("local.starring.d2.test.runtime"),
                {
                    "pid": None,
                    "program": "/Users/test/starring-runtime",
                    "plist_path": "/Users/test/runtime.plist",
                    "arguments": ["/Users/test/starring-runtime"],
                    "runs": 1,
                    "state": "exited",
                    "last_exit_code": 0,
                },
            )
        decorated = subprocess.CompletedProcess(
            [],
            0,
            observed.stdout.replace(
                b"last exit code = 0", b"last exit code = 70: EX_SOFTWARE"
            ),
            b"",
        )
        with mock.patch.object(platform, "run", return_value=decorated):
            result = platform.launchd_job("local.starring.d2.test.runtime")
        self.assertEqual(result["state"], "exited")
        self.assertEqual(result["last_exit_code"], 70)

    def test_launchd_job_rejects_inconsistent_or_unknown_states(self):
        platform = PLATFORM.Platform()
        invalid = [
            (
                "\tstate = not running\n",
                "\tpid = 49152\n",
                "\tlast exit code = 0\n",
            ),
            ("\tstate = not running\n", "", ""),
            (
                "\tstate = not running\n",
                "",
                "\tlast exit code = (never exited)\n",
            ),
            (
                "\tstate = running\n",
                "",
                "\tlast exit code = (never exited)\n",
            ),
            (
                "\tstate = waiting for debugger\n",
                "",
                "\tlast exit code = 0\n",
            ),
            ("\tstate = not-running\n", "", "\tlast exit code = 0\n"),
            (
                "\tstate = running\n\tstate = not running\n",
                "\tpid = 49152\n",
                "\tlast exit code = 0\n",
            ),
        ]
        for state, pid, exit_value in invalid:
            output = (
                "\tpath = /Users/test/runtime.plist\n"
                "\tprogram = /Users/test/starring-runtime\n"
                "\targuments = {\n"
                "\t\t/Users/test/starring-runtime\n"
                "\t}\n"
                f"{state}"
                "\truns = 1\n"
                f"{pid}"
                f"{exit_value}"
            ).encode("utf-8")
            observed = subprocess.CompletedProcess([], 0, output, b"")
            with (
                self.subTest(
                    state=state,
                    pid=pid,
                    exit_value=exit_value,
                ),
                mock.patch.object(platform, "run", return_value=observed),
                self.assertRaisesRegex(
                    CONTRACT.OrchestratorError,
                    "launchd_observation_invalid",
                ),
            ):
                platform.launchd_job("local.starring.d2.test.runtime")

    def test_launchd_job_rejects_duplicate_exit_fields(self):
        platform = PLATFORM.Platform()
        observed = subprocess.CompletedProcess(
            [],
            0,
            b"\tpath = /Users/test/runtime.plist\n\tprogram = /Users/test/starring-runtime\n\targuments = {\n\t\t/Users/test/starring-runtime\n\t}\n\tstate = exited\n\truns = 1\n\tlast exit code = 70: EX_SOFTWARE\n\tlast exit code = 0\n",
            b"",
        )
        with mock.patch.object(
            platform, "run", return_value=observed
        ), self.assertRaisesRegex(
            CONTRACT.OrchestratorError, "launchd_observation_invalid"
        ):
            platform.launchd_job("local.starring.d2.test.runtime")

    def test_launchd_job_only_treats_known_absence_as_absent(self):
        platform = PLATFORM.Platform()
        absent = subprocess.CompletedProcess([], 113, b"", b"")
        with mock.patch.object(platform, "run", return_value=absent):
            self.assertIsNone(platform.launchd_job("local.starring.d2.absent"))
        failed = subprocess.CompletedProcess([], 1, b"", b"")
        with mock.patch.object(platform, "run", return_value=failed):
            with self.assertRaisesRegex(
                CONTRACT.OrchestratorError, "launchd_observation_failed"
            ):
                platform.launchd_job("local.starring.d2.unknown")

    def test_http_probe_preserves_loopback_and_supplies_public_host(self):
        platform = PLATFORM.Platform()
        completed = subprocess.CompletedProcess([], 0, b"200", b"")
        with mock.patch.object(platform, "run", return_value=completed) as run:
            status = platform.http_status(
                "http://127.0.0.1:28080/health/ready",
                host_header="d2-api.starring.co.kr",
            )
        self.assertEqual(status, 200)
        arguments = run.call_args.args[0]
        self.assertEqual(arguments[-3:], ["--header", "Host: d2-api.starring.co.kr", "http://127.0.0.1:28080/health/ready"])

    def test_start_does_not_terminate_a_freshly_bootstrapped_keepalive_job(self):
        platform = RecordingLaunchdPlatform()
        platform.launchd_start("local.starring.d2.test", pathlib.Path("/tmp/test.plist"))
        actions = [command[1:] for command in platform.commands]
        self.assertEqual(
            actions,
            [
                ["bootstrap", f"gui/{ORCHESTRATOR.os.getuid()}", "/tmp/test.plist"],
                ["enable", f"gui/{ORCHESTRATOR.os.getuid()}/local.starring.d2.test"],
                ["kickstart", f"gui/{ORCHESTRATOR.os.getuid()}/local.starring.d2.test"],
            ],
        )

    def test_kickstart_failure_boots_out_the_loaded_job_once(self):
        platform = RecordingLaunchdPlatform()
        platform.fail_action = "kickstart"
        with self.assertRaisesRegex(
            CONTRACT.OrchestratorError, "launchd_kickstart_failed"
        ):
            platform.launchd_start(
                "local.starring.d2.test", pathlib.Path("/tmp/test.plist")
            )
        self.assertEqual(platform.bootouts, ["local.starring.d2.test"])


class D2DiscordResourceOrchestratorTest(unittest.TestCase):
    setUp = D2IsolatedOrchestratorTest.setUp
    tearDown = D2IsolatedOrchestratorTest.tearDown
    remove_test_root = D2IsolatedOrchestratorTest.remove_test_root
    advance_live_clock = D2IsolatedOrchestratorTest.advance_live_clock
    prepare_manifest = D2IsolatedOrchestratorTest.prepare_manifest
    start_candidate_with_discord_resources = (
        D2IsolatedOrchestratorTest.start_candidate_with_discord_resources
    )

    def test_discord_resource_command_parsers_require_manifest(self):
        for command in ("resource-inventory", "teardown-discord-resources"):
            arguments = ORCHESTRATOR.build_parser().parse_args(
                [command, "--manifest", str(self.manifest_path)]
            )
            self.assertEqual(arguments.command, command)
            self.assertEqual(arguments.manifest, str(self.manifest_path))

    def test_transport_evidence_parser_requires_checkpoint(self):
        for checkpoint in ORCHESTRATOR.TRANSPORT_EVIDENCE_KINDS:
            arguments = ORCHESTRATOR.build_parser().parse_args(
                [
                    "transport-evidence",
                    "--manifest",
                    str(self.manifest_path),
                    "--checkpoint",
                    checkpoint,
                ]
            )
            self.assertEqual(arguments.command, "transport-evidence")
            self.assertEqual(arguments.checkpoint, checkpoint)
        observation = ORCHESTRATOR.build_parser().parse_args(
            [
                "reconciliation-discord-observation",
                "--manifest",
                str(self.manifest_path),
                "--database-evidence",
                str(self.root / "database.json"),
            ]
        )
        self.assertEqual(
            observation.command, "reconciliation-discord-observation"
        )

    def test_interaction_transport_evidence_projects_exact_active_inventory(self):
        inventory = self.start_candidate_with_discord_resources()
        result = ORCHESTRATOR.command_transport_evidence(
            self.context, self.platform, "interaction"
        )
        self.assertEqual(result["status"], "recorded")
        path = ORCHESTRATOR.transport_evidence_path(self.context, "interaction")
        evidence = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(
            set(evidence),
            {
                "schema_version",
                "kind",
                "observed_at",
                "role_ids",
                "channel_ids",
                "panel_message_ids",
                "inventory_digest_sha256",
                "transport_instance_id",
            },
        )
        self.assertEqual(
            evidence["inventory_digest_sha256"], inventory["digest_sha256"]
        )
        self.assertEqual(
            evidence["role_ids"],
            sorted(
                resource["resource_id"]
                for resource in inventory["active"]
                if resource["kind"] == "role"
            ),
        )
        self.assertEqual(
            evidence["channel_ids"],
            sorted(
                resource["resource_id"]
                for resource in inventory["active"]
                if resource["kind"] == "channel"
            ),
        )
        self.assertEqual(
            evidence["panel_message_ids"],
            sorted(
                resource["resource_id"]
                for resource in inventory["active"]
                if resource["kind"] == "message"
            ),
        )
        replay = ORCHESTRATOR.command_transport_evidence(
            self.context, self.platform, "interaction"
        )
        self.assertEqual(replay["status"], "exact_replay")
        self.platform.resource_history.append(
            {
                "kind": "role",
                "resource_id": "1524810437118525590",
                "state": "created",
            }
        )
        self.platform.discord_existing.add("1524810437118525590")
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "transport_evidence_replay_drift"
        ):
            ORCHESTRATOR.command_transport_evidence(
                self.context, self.platform, "interaction"
            )

    def test_duplicate_transport_evidence_is_counter_and_interaction_bound(self):
        inventory = self.start_candidate_with_discord_resources()
        gateway = self.platform.transport_state["gateway"]
        gateway["last_duplicate_interaction_id"] = "1532677575736819846"
        gateway["duplicate_injections"] = 1
        gateway["duplicate_delivery_count"] = 2
        result = ORCHESTRATOR.command_transport_evidence(
            self.context, self.platform, "duplicate"
        )
        evidence = json.loads(
            pathlib.Path(result["evidence"]).read_text(encoding="utf-8")
        )
        self.assertEqual(
            evidence["interaction_id"], "1532677575736819846"
        )
        self.assertEqual(evidence["delivery_count"], 2)
        self.assertEqual(evidence["transport_duplicate_injections"], 1)
        self.assertEqual(evidence["transport_duplicate_delivery_count"], 2)
        self.assertEqual(
            evidence["inventory_digest_sha256"], inventory["digest_sha256"]
        )
        self.assertEqual(
            evidence["role_ids"],
            sorted(
                resource["resource_id"]
                for resource in inventory["active"]
                if resource["kind"] == "role"
            ),
        )
        self.assertNotIn("operation_id", evidence)
        gateway["duplicate_delivery_count"] = 3
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "transport_duplicate_evidence_invalid"
        ):
            ORCHESTRATOR.command_transport_evidence(
                self.context, self.platform, "duplicate"
            )
        gateway["duplicate_delivery_count"] = 2
        gateway["duplicate_injections"] = True
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "transport_duplicate_evidence_invalid"
        ):
            ORCHESTRATOR.command_transport_evidence(
                self.context, self.platform, "duplicate"
            )

    def test_reconciliation_transport_evidence_excludes_interaction_and_raw_snapshot(self):
        self.start_candidate_with_discord_resources()
        effect = self.platform.transport_state["effect_http"]
        effect["indeterminate_injections"] = 1
        effect["last_indeterminate_audit_reason_sha256"] = "a" * 64
        effect["last_indeterminate_upstream_status"] = 201
        result = ORCHESTRATOR.command_transport_evidence(
            self.context, self.platform, "reconciliation"
        )
        evidence = json.loads(
            pathlib.Path(result["evidence"]).read_text(encoding="utf-8")
        )
        self.assertEqual(
            set(evidence),
            {
                "schema_version",
                "kind",
                "observed_at",
                "injected_outcome",
                "transport_indeterminate_injections",
                "transport_last_audit_reason_sha256",
                "transport_last_upstream_status",
                "transport_instance_id",
            },
        )
        serialized = json.dumps(evidence)
        for forbidden in (
            "interaction_id",
            "operation_id",
            "snapshot",
            "authorization",
            "token",
        ):
            self.assertNotIn(forbidden, serialized.lower())
        effect["last_indeterminate_audit_reason_sha256"] = "bad"
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "transport_reconciliation_evidence_invalid",
        ):
            ORCHESTRATOR.command_transport_evidence(
                self.context, self.platform, "reconciliation"
            )
        effect["last_indeterminate_audit_reason_sha256"] = "a" * 64
        effect["indeterminate_injections"] = True
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "transport_reconciliation_evidence_invalid",
        ):
            ORCHESTRATOR.command_transport_evidence(
                self.context, self.platform, "reconciliation"
            )

    def write_reconciliation_database_evidence(
        self, output_role_id="1524810437118525590", **overrides
    ):
        interaction_id = "1532677575736819850"
        evidence = {
            "schema_version": 1,
            "kind": "starring.d2.db-reconciliation-evidence.v1",
            "observed_at": "2026-08-04T12:00:01.123Z",
            "effect_identity": {
                "application_id": self.context.manifest["discord"]["application_id"],
                "interaction_id": interaction_id,
                "action_index": 0,
            },
            "interaction_id": interaction_id,
            "route_identity": {"deployment_id": "deployment-1"},
            "reconciliation_state": "known_success",
            "duplicate_external_effect_count": 0,
            "unsafe_deletion_count": 0,
            "output_role_id": output_role_id,
        }
        evidence.update(overrides)
        path = self.root / "db-reconciliation.json"
        path.write_text(json.dumps(evidence), encoding="utf-8")
        path.chmod(0o600)
        return path

    def add_reconciliation_role(self):
        resource_id = "1524810437118525590"
        self.platform.resource_history.append(
            {"kind": "role", "resource_id": resource_id, "state": "created"}
        )
        self.platform.discord_existing.add(resource_id)
        return resource_id

    def test_reconciliation_discord_observation_binds_active_role_and_inventory(self):
        inventory = self.start_candidate_with_discord_resources()
        output_role_id = self.add_reconciliation_role()
        inventory = self.platform.resource_inventory(self.context)
        database = self.write_reconciliation_database_evidence()
        result = ORCHESTRATOR.command_reconciliation_discord_observation(
            self.context, self.platform, str(database)
        )
        self.assertEqual(result["status"], "recorded")
        path = ORCHESTRATOR.reconciliation_discord_observation_path(self.context)
        evidence = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(
            set(evidence),
            {
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
            },
        )
        self.assertEqual(
            evidence["kind"],
            ORCHESTRATOR.RECONCILIATION_DISCORD_OBSERVATION_KIND,
        )
        self.assertEqual(evidence["resource_id"], output_role_id)
        self.assertEqual(evidence["inventory_digest_sha256"], inventory["digest_sha256"])
        self.assertEqual(evidence["http_status"], 200)
        self.assertIsNone(evidence["discord_code"])
        self.assertTrue(evidence["exists"])
        replay = ORCHESTRATOR.command_reconciliation_discord_observation(
            self.context, self.platform, str(database)
        )
        self.assertEqual(replay["status"], "exact_replay")
        serialized = json.dumps(evidence).lower()
        for forbidden in ("authorization", "bot ", "token", "response_body"):
            self.assertNotIn(forbidden, serialized)

    def test_reconciliation_discord_observation_rejects_wrong_or_absent_role(self):
        self.start_candidate_with_discord_resources()
        self.add_reconciliation_role()
        wrong = self.write_reconciliation_database_evidence(
            output_role_id="1524810437118525599"
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "reconciliation_output_role_not_active",
        ):
            ORCHESTRATOR.command_reconciliation_discord_observation(
                self.context, self.platform, str(wrong)
            )
        database = self.write_reconciliation_database_evidence()
        self.platform.discord_existing.discard("1524810437118525590")
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "reconciliation_discord_observation_invalid",
        ):
            ORCHESTRATOR.command_reconciliation_discord_observation(
                self.context, self.platform, str(database)
            )

    def test_reconciliation_discord_observation_rejects_non_success_database(self):
        self.start_candidate_with_discord_resources()
        self.add_reconciliation_role()
        database = self.write_reconciliation_database_evidence(
            reconciliation_state="known_failure"
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "reconciliation_database_evidence_invalid",
        ):
            ORCHESTRATOR.command_reconciliation_discord_observation(
                self.context, self.platform, str(database)
            )

    def test_gateway_loss_transport_evidence_uses_unready_boundary_without_route(self):
        self.start_candidate_with_discord_resources()
        partition = ORCHESTRATOR.command_transport_control(
            self.context, self.platform, "partition-gateway"
        )
        self.platform.health_failure = "29091"
        result = ORCHESTRATOR.command_transport_evidence(
            self.context, self.platform, "gateway-loss"
        )
        evidence = json.loads(
            pathlib.Path(result["evidence"]).read_text(encoding="utf-8")
        )
        self.assertTrue(evidence["gateway_disconnected"])
        self.assertEqual(evidence["runtime_ready_status"], 503)
        self.assertTrue(evidence["transport_gateway_partitioned"])
        self.assertEqual(evidence["transport_gateway_partition_events"], 1)
        self.assertEqual(
            evidence["partition_operation_id"], partition["operation_id"]
        )
        partition_completion = json.loads(
            pathlib.Path(partition["evidence"]).read_text(encoding="utf-8")
        )
        self.assertEqual(
            evidence["partition_completion_sha256"],
            hashlib.sha256(
                CERTIFICATION.canonical_json(partition_completion).encode("utf-8")
            ).hexdigest(),
        )
        loaded, digest = D2_RUN.read_private_json(
            pathlib.Path(result["evidence"]), "gateway_loss"
        )
        self.assertEqual(loaded, evidence)
        self.assertEqual(
            digest,
            hashlib.sha256(pathlib.Path(result["evidence"]).read_bytes()).hexdigest(),
        )
        self.assertEqual(
            loaded["kind"], D2_RUN.STEP_SOURCE_SPECS[15][1]["kind"]
        )
        self.assertNotIn("route_id", evidence)
        self.assertNotIn("route_identity", evidence)
        self.platform.health_failure = None
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "gateway_loss_runtime_readiness_invalid",
        ):
            ORCHESTRATOR.command_transport_evidence(
                self.context, self.platform, "gateway-loss"
            )
        self.platform.health_failure = "29091"
        self.platform.transport_state["gateway"]["partition_events"] = True
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "transport_gateway_loss_evidence_invalid",
        ):
            ORCHESTRATOR.command_transport_evidence(
                self.context, self.platform, "gateway-loss"
            )

    def test_gateway_healed_evidence_binds_durable_operations_and_replays(self):
        self.start_candidate_with_discord_resources()
        partition = ORCHESTRATOR.command_transport_control(
            self.context, self.platform, "partition-gateway"
        )
        self.platform.health_failure = "29091"
        loss = ORCHESTRATOR.command_transport_evidence(
            self.context, self.platform, "gateway-loss"
        )
        self.platform.health_failure = None
        heal = ORCHESTRATOR.command_transport_control(
            self.context, self.platform, "heal-gateway"
        )
        result = ORCHESTRATOR.command_transport_evidence(
            self.context, self.platform, "gateway-healed"
        )
        path = pathlib.Path(result["evidence"])
        evidence = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(
            path,
            ORCHESTRATOR.transport_evidence_path(self.context, "gateway-healed"),
        )
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(evidence["partition_operation_id"], partition["operation_id"])
        self.assertEqual(evidence["heal_operation_id"], heal["operation_id"])
        self.assertEqual(evidence["transport_gateway_partition_events"], 1)
        self.assertFalse(evidence["transport_gateway_partitioned"])
        self.assertTrue(evidence["gateway_connected"])
        self.assertEqual(evidence["runtime_ready_status"], 200)
        for field in (
            "transport_duplicate_armed",
            "transport_duplicate_claimed",
            "transport_indeterminate_armed",
            "transport_indeterminate_claimed",
        ):
            self.assertFalse(evidence[field])
        loss_source = json.loads(
            pathlib.Path(loss["evidence"]).read_text(encoding="utf-8")
        )
        self.assertEqual(
            evidence["partition_completion_sha256"],
            loss_source["partition_completion_sha256"],
        )
        heal_completion = json.loads(
            pathlib.Path(heal["evidence"]).read_text(encoding="utf-8")
        )
        self.assertEqual(
            evidence["heal_completion_sha256"],
            hashlib.sha256(
                CERTIFICATION.canonical_json(heal_completion).encode("utf-8")
            ).hexdigest(),
        )
        loaded, _digest = D2_RUN.read_private_json(path, "gateway_healed")
        self.assertEqual(loaded, evidence)
        self.assertEqual(
            loaded["kind"], D2_RUN.STEP_SOURCE_SPECS[15][2]["kind"]
        )
        replay = ORCHESTRATOR.command_transport_evidence(
            self.context, self.platform, "gateway-healed"
        )
        self.assertEqual(replay["status"], "exact_replay")

    def test_gateway_healed_evidence_recovers_lost_heal_response(self):
        self.start_candidate_with_discord_resources()
        ORCHESTRATOR.command_transport_control(
            self.context, self.platform, "partition-gateway"
        )
        self.platform.health_failure = "29091"
        ORCHESTRATOR.command_transport_evidence(
            self.context, self.platform, "gateway-loss"
        )
        self.platform.health_failure = None
        original = self.platform.transport_control
        injected = False

        def lose_heal_response(context, command, fields=None, timeout_seconds=3):
            nonlocal injected
            result = original(context, command, fields, timeout_seconds)
            if command == "heal_gateway" and not injected:
                injected = True
                raise ORCHESTRATOR.OrchestratorError("injected_heal_response_loss")
            return result

        self.platform.transport_control = lose_heal_response
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "injected_heal_response_loss"
        ):
            ORCHESTRATOR.command_transport_control(
                self.context, self.platform, "heal-gateway"
            )
        records, pending = ORCHESTRATOR.transport_control_inventory(self.context)
        self.assertIsNotNone(pending)
        intent_operation_id = pending["intent"]["operation_id"]
        self.platform.transport_control = original
        replayed = ORCHESTRATOR.command_transport_control(
            self.context, self.platform, "heal-gateway"
        )
        self.assertEqual(replayed["operation_id"], intent_operation_id)
        self.assertEqual(replayed["response"], {"changed": False})
        healed = ORCHESTRATOR.command_transport_evidence(
            self.context, self.platform, "gateway-healed"
        )
        evidence = json.loads(
            pathlib.Path(healed["evidence"]).read_text(encoding="utf-8")
        )
        self.assertEqual(evidence["heal_operation_id"], intent_operation_id)
        records, pending = ORCHESTRATOR.transport_control_inventory(self.context)
        self.assertIsNone(pending)
        self.assertEqual(len(records), 2)

    def test_gateway_healed_evidence_rejects_durable_completion_drift(self):
        self.start_candidate_with_discord_resources()
        ORCHESTRATOR.command_transport_control(
            self.context, self.platform, "partition-gateway"
        )
        self.platform.health_failure = "29091"
        loss = ORCHESTRATOR.command_transport_evidence(
            self.context, self.platform, "gateway-loss"
        )
        self.platform.health_failure = None
        ORCHESTRATOR.command_transport_control(
            self.context, self.platform, "heal-gateway"
        )
        loss_path = pathlib.Path(loss["evidence"])
        loss_value = json.loads(loss_path.read_text(encoding="utf-8"))
        loss_value["partition_completion_sha256"] = "f" * 64
        loss_path.write_text(json.dumps(loss_value), encoding="utf-8")
        loss_path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "transport_gateway_heal_binding_invalid",
        ):
            ORCHESTRATOR.command_transport_evidence(
                self.context, self.platform, "gateway-healed"
            )

    def test_gateway_evidence_rejects_unhealed_armed_and_unrelated_faults(self):
        self.start_candidate_with_discord_resources()
        ORCHESTRATOR.command_transport_control(
            self.context, self.platform, "partition-gateway"
        )
        self.platform.health_failure = "29091"
        ORCHESTRATOR.command_transport_evidence(
            self.context, self.platform, "gateway-loss"
        )
        self.platform.health_failure = None
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "transport_gateway_operation_history_invalid",
        ):
            ORCHESTRATOR.command_transport_evidence(
                self.context, self.platform, "gateway-healed"
            )
        ORCHESTRATOR.command_transport_control(
            self.context, self.platform, "heal-gateway"
        )
        gateway = self.platform.transport_state["gateway"]
        gateway["duplicate_armed"] = True
        gateway["armed_duplicate_operation_id"] = "d2:test:duplicate-armed"
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "transport_gateway_healed_evidence_invalid",
        ):
            ORCHESTRATOR.command_transport_evidence(
                self.context, self.platform, "gateway-healed"
            )
        gateway["duplicate_armed"] = False
        gateway["armed_duplicate_operation_id"] = None
        ORCHESTRATOR.command_transport_control(
            self.context, self.platform, "partition-gateway"
        )
        self.platform.health_failure = "29091"
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "transport_gateway_operation_history_invalid",
        ):
            ORCHESTRATOR.command_transport_evidence(
                self.context, self.platform, "gateway-loss"
            )

    def test_transport_evidence_requires_ready_health_phase_and_pinned_instance(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "orchestrator_phase_invalid"
        ):
            ORCHESTRATOR.command_transport_evidence(
                self.context, self.platform, "duplicate"
            )
        ORCHESTRATOR.command_start(self.context, self.platform)
        gateway = self.platform.transport_state["gateway"]
        gateway["last_duplicate_interaction_id"] = "1532677575736819846"
        gateway["duplicate_injections"] = 1
        gateway["duplicate_delivery_count"] = 2
        self.platform.health_failure = "worker"
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "candidate_health_unready"
        ):
            ORCHESTRATOR.command_transport_evidence(
                self.context, self.platform, "duplicate"
            )
        self.platform.health_failure = None
        self.platform.transport_state["instance_id"] = (
            "d2ti-fedcba9876543210fedcba9876543210"
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_transport_evidence(
                self.context, self.platform, "duplicate"
            )

    def write_browser_authoring_evidence(self, name="browser-authoring.json"):
        path = self.root / name
        path.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "kind": "starring.d2.browser-authoring-evidence.v1",
                    "observed_at": ORCHESTRATOR.utc_now(),
                    "public_origin": self.context.manifest["public_origin"],
                    "authoring_http_status": 201,
                    "authoring_session_id": "session-1",
                    "authoring_generation": 1,
                    "expected_generation": 0,
                    "authoring_disposition": "created",
                    "installation_id": "installation-1",
                    "one_shot": True,
                    "worker_request_id": "worker-request-1",
                    "worker_completion_sha256": "b" * 64,
                },
                separators=(",", ":"),
            ),
            encoding="utf-8",
        )
        path.chmod(0o600)
        return path

    def test_worker_authoring_evidence_brackets_one_exact_luna_request(self):
        self.start_candidate_with_discord_resources()
        before = ORCHESTRATOR.command_worker_authoring_evidence(
            self.context, self.platform, "before"
        )
        self.assertEqual(before["status"], "recorded")
        self.platform.worker_accepted_requests = 1
        self.platform.worker_settled_requests = 1
        browser = self.write_browser_authoring_evidence()
        after = ORCHESTRATOR.command_worker_authoring_evidence(
            self.context, self.platform, "after", str(browser)
        )
        self.assertEqual(after["status"], "recorded")
        evidence = json.loads(
            pathlib.Path(after["evidence"]).read_text(encoding="utf-8")
        )
        self.assertEqual(evidence["provider"], "codex_chatgpt")
        self.assertEqual(evidence["model"], "gpt-5.6-luna")
        self.assertEqual(evidence["accepted_requests_delta"], 1)
        self.assertEqual(evidence["settled_requests_delta"], 1)
        self.assertNotIn("prompt", json.dumps(evidence).lower())
        self.platform.worker_accepted_requests = 12
        self.platform.worker_settled_requests = 12
        replay = ORCHESTRATOR.command_worker_authoring_evidence(
            self.context, self.platform, "after", str(browser)
        )
        self.assertEqual(replay["status"], "exact_replay")

    def test_worker_authoring_evidence_rejects_non_single_or_unbound_calls(self):
        self.start_candidate_with_discord_resources()
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "worker_browser_evidence_required"
        ):
            ORCHESTRATOR.command_worker_authoring_evidence(
                self.context, self.platform, "after"
            )
        ORCHESTRATOR.command_worker_authoring_evidence(
            self.context, self.platform, "before"
        )
        self.platform.worker_accepted_requests = 2
        self.platform.worker_settled_requests = 2
        browser = self.write_browser_authoring_evidence()
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "worker_authoring_evidence_invalid"
        ):
            ORCHESTRATOR.command_worker_authoring_evidence(
                self.context, self.platform, "after", str(browser)
            )

    def test_resource_inventory_command_is_candidate_and_instance_bound(self):
        inventory = self.start_candidate_with_discord_resources()
        result = ORCHESTRATOR.command_resource_inventory(self.context, self.platform)
        self.assertEqual(result["status"], "observed")
        self.assertEqual(result["phase"], "candidate_started")
        self.assertEqual(result["transport_instance_id"], inventory["instance_id"])
        self.assertEqual(result["inventory_digest_sha256"], inventory["digest_sha256"])
        self.assertEqual(result["created_count"], 6)
        self.assertEqual(result["deleted_count"], 0)
        self.assertEqual(result["active_count"], 6)
        self.assertEqual(result["resource_inventory"], inventory)
        self.platform.transport_state["instance_id"] = (
            "d2ti-fedcba9876543210fedcba9876543210"
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_resource_inventory(self.context, self.platform)
        self.assertTrue(
            ORCHESTRATOR.candidate_start_retirement_path(self.context).is_file()
        )

    def test_resource_commands_fail_closed_on_phase_health_and_standing_drift(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "orchestrator_phase_invalid"
        ):
            ORCHESTRATOR.command_resource_inventory(self.context, self.platform)
        ORCHESTRATOR.command_start(self.context, self.platform)
        self.platform.health_failure = "worker"
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "candidate_health_unready"
        ):
            ORCHESTRATOR.command_teardown_discord_resources(
                self.context, self.platform
            )
        self.platform.health_failure = None
        state = ORCHESTRATOR.load_state(self.context, {"candidate_started"})
        label = sorted(state["standing_snapshot"]["launchd_loaded"])[0]
        state["standing_snapshot"]["launchd_loaded"][label] = not state[
            "standing_snapshot"
        ]["launchd_loaded"][label]
        ORCHESTRATOR.save_state(
            self.context, "candidate_started", state["standing_snapshot"]
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "candidate_start_transition_retirement_required",
        ):
            ORCHESTRATOR.command_resource_inventory(self.context, self.platform)
        self.assertTrue(
            ORCHESTRATOR.candidate_start_retirement_path(self.context).is_file()
        )

    def test_discord_teardown_orders_proxy_deletes_and_writes_redacted_evidence(self):
        inventory = self.start_candidate_with_discord_resources()
        result = ORCHESTRATOR.command_teardown_discord_resources(
            self.context, self.platform
        )
        self.assertEqual(result["status"], "torn_down")
        self.assertTrue(result["all_resources_absent"])
        self.assertEqual(
            [resource["kind"] for resource in self.platform.proxy_deletions],
            ["message", "message", "channel", "channel", "role", "role"],
        )
        expected = sorted(
            inventory["created"], key=ORCHESTRATOR.discord_resource_teardown_key
        )
        self.assertEqual(self.platform.proxy_deletions, expected)
        protected = {
            self.context.manifest["discord"][field]
            for field in (
                "guild_id",
                "hub_channel_id",
                "application_id",
                "actor_id",
                "bot_user_id",
            )
        }
        self.assertTrue(
            protected.isdisjoint(
                resource["resource_id"]
                for resource in self.platform.proxy_deletions
            )
        )
        self.assertIn(
            self.context.manifest["discord"]["hub_channel_id"],
            self.platform.discord_existing,
        )
        evidence_path = ORCHESTRATOR.discord_teardown_evidence_path(self.context)
        progress_path = ORCHESTRATOR.discord_teardown_progress_path(self.context)
        tombstone_path = ORCHESTRATOR.abort_teardown_tombstone_path(self.context)
        evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
        tombstone = json.loads(tombstone_path.read_text(encoding="utf-8"))
        self.assertEqual(evidence_path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(progress_path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(tombstone_path.stat().st_mode & 0o777, 0o600)
        self.assertTrue(tombstone["certification_permanently_disqualified"])
        self.assertFalse(
            ORCHESTRATOR.discord_teardown_evidence_path(
                self.context, frozen=True
            ).exists()
        )
        self.assertEqual(evidence["created_resources"], evidence["deleted_resources"])
        self.assertEqual(evidence["active_resources"], [])
        self.assertEqual(evidence["resource_ids"], sorted(evidence["resource_ids"]))
        self.assertEqual(evidence["message_ids"], sorted(evidence["message_ids"]))
        self.assertEqual(evidence["channel_ids"], sorted(evidence["channel_ids"]))
        self.assertEqual(evidence["role_ids"], sorted(evidence["role_ids"]))
        self.assertEqual(len(evidence["proxy_deletions"]), 6)
        self.assertEqual(len(evidence["direct_observations"]), 6)
        serialized = json.dumps(evidence)
        self.assertNotIn("Bot ", serialized)
        self.assertNotIn("discord.bot-token", serialized)
        self.assertNotIn("Authorization", serialized)

    def test_discord_teardown_partial_failure_resumes_without_redeleting_successes(self):
        self.start_candidate_with_discord_resources()
        self.platform.proxy_failure_resource_id = "1524810437118525560"
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "injected_proxy_delete_failure"
        ):
            ORCHESTRATOR.command_teardown_discord_resources(
                self.context, self.platform
            )
        progress_path = ORCHESTRATOR.discord_teardown_progress_path(self.context)
        progress = json.loads(progress_path.read_text(encoding="utf-8"))
        self.assertEqual(len(progress["deletions"]), 2)
        first_attempt = list(self.platform.proxy_deletions)
        self.assertEqual(
            [resource["kind"] for resource in first_attempt],
            ["message", "message", "channel"],
        )
        with self.assertRaisesRegex(
            D2_RUN.CertificationError,
            "candidate_start_transition_retirement_required",
        ):
            D2_RUN.next_certification_action(self.manifest_path)
        self.platform.proxy_failure_resource_id = None
        output = io.StringIO()
        with mock.patch.object(
            sys,
            "argv",
            [
                "isolated_orchestrator.py",
                "teardown-discord-resources",
                "--manifest",
                str(self.manifest_path),
            ],
        ), mock.patch.object(
            ORCHESTRATOR, "Platform", return_value=self.platform
        ), contextlib.redirect_stdout(output):
            self.assertIsNone(ORCHESTRATOR.main())
        result = json.loads(output.getvalue())
        self.assertEqual(result["status"], "torn_down")
        for resource in first_attempt[:2]:
            self.assertEqual(self.platform.proxy_deletions.count(resource), 1)
        final_progress = json.loads(progress_path.read_text(encoding="utf-8"))
        self.assertEqual(len(final_progress["deletions"]), 6)
        replay = ORCHESTRATOR.command_teardown_discord_resources(
            self.context, self.platform
        )
        self.assertEqual(replay["status"], "exact_replay")

    def test_discord_teardown_reconciles_delete_with_lost_response(self):
        self.start_candidate_with_discord_resources()
        self.platform.proxy_failure_resource_id = "1524810437118525571"
        self.platform.proxy_failure_after_delete = True
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "injected_proxy_delete_lost_response",
        ):
            ORCHESTRATOR.command_teardown_discord_resources(
                self.context, self.platform
            )
        self.platform.proxy_failure_resource_id = None
        self.platform.proxy_failure_after_delete = False
        result = ORCHESTRATOR.command_teardown_discord_resources(
            self.context, self.platform
        )
        self.assertEqual(result["status"], "torn_down")
        evidence = json.loads(
            ORCHESTRATOR.discord_teardown_evidence_path(self.context).read_text(
                encoding="utf-8"
            )
        )
        reconciled = [
            record
            for record in evidence["proxy_deletions"]
            if record["resource_id"] == "1524810437118525571"
        ]
        self.assertEqual(len(reconciled), 1)
        self.assertEqual(reconciled[0]["disposition"], "reconciled_deleted")

    def test_discord_teardown_exact_replay_reobserves_external_absence(self):
        self.start_candidate_with_discord_resources()
        first = ORCHESTRATOR.command_teardown_discord_resources(
            self.context, self.platform
        )
        deletions = list(self.platform.proxy_deletions)
        replay = ORCHESTRATOR.command_teardown_discord_resources(
            self.context, self.platform
        )
        self.assertEqual(first["status"], "torn_down")
        self.assertEqual(replay["status"], "exact_replay")
        self.assertEqual(self.platform.proxy_deletions, deletions)
        self.platform.discord_existing.add("1524810437118525580")
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "discord_resource_absence_unconfirmed"
        ):
            ORCHESTRATOR.command_teardown_discord_resources(
                self.context, self.platform
            )

    def test_discord_teardown_rejects_progress_and_final_inventory_mismatch(self):
        self.start_candidate_with_discord_resources()
        self.platform.proxy_failure_resource_id = "1524810437118525560"
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "injected_proxy_delete_failure"
        ):
            ORCHESTRATOR.command_teardown_discord_resources(
                self.context, self.platform
            )
        progress_path = ORCHESTRATOR.discord_teardown_progress_path(self.context)
        progress = json.loads(progress_path.read_text(encoding="utf-8"))
        progress["resource_union_sha256"] = "f" * 64
        progress_path.write_text(json.dumps(progress), encoding="utf-8")
        progress_path.chmod(0o600)
        self.platform.proxy_failure_resource_id = None
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "discord_resource_teardown_progress_invalid",
        ):
            ORCHESTRATOR.command_teardown_discord_resources(
                self.context, self.platform
            )


if __name__ == "__main__":
    unittest.main()
