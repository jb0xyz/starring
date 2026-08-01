#!/usr/bin/env python3

import contextlib
import fcntl
import importlib.util
import io
import json
import pathlib
import plistlib
import secrets
import subprocess
import sys
import tempfile
import unittest
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
        self.keychain_writes = []
        self.keychain_deletes = []
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

    def keychain_present(self, service, account):
        return (service, account) in self.keychain

    def keychain_write_new(self, service, account, value):
        if (service, account) in self.keychain:
            ORCHESTRATOR.fail("keychain_identity_busy")
        self.keychain.add((service, account))
        self.keychain_writes.append((service, account))
        if account == ORCHESTRATOR.OWNER_ACCOUNT:
            self.owner_values[service] = value.decode("ascii")

    def keychain_delete(self, service, account):
        self.keychain.discard((service, account))
        self.keychain_deletes.append((service, account))
        if account == ORCHESTRATOR.OWNER_ACCOUNT:
            self.owner_values.pop(service, None)

    def keychain_owner_matches(self, service, expected):
        return self.owner_values.get(service) == expected

    def postgres_running(self, cluster_root):
        return self.postgres

    def initdb(self, cluster_root):
        cluster_root.mkdir(mode=0o700)
        (cluster_root / "PG_VERSION").write_text("16\n", encoding="utf-8")
        (cluster_root / "postgresql.conf").write_text("", encoding="utf-8")
        (cluster_root / "pg_hba.conf").write_text("", encoding="utf-8")
        if self.initdb_failure:
            ORCHESTRATOR.fail("injected_initdb_failure")

    def postgres_start(self, cluster_root, log_path):
        self.postgres = True
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
        }

    def launchd_start(self, label, plist_path):
        if self.launchd_failure == label:
            ORCHESTRATOR.fail("injected_launchd_failure")
        self.loaded.add(label)
        self.start_order.append(label)
        self.lifecycle_events.append(f"start:{label}")

    def http_status(self, url, timeout_seconds=3, host_header=None):
        self.http_probes.append((url, host_header))
        if self.health_failure and self.health_failure in url:
            return 503
        return 200

    def worker_health_status(self, context, timeout_seconds=3):
        self.lifecycle_events.append("health:worker")
        if self.health_failure == "worker":
            return 503
        return 200

    def transport_health_status(self, context, timeout_seconds=3):
        self.lifecycle_events.append("health:transport")
        if self.health_failure == "transport":
            return 503
        return 200

    def transport_control(self, context, command, fields=None, timeout_seconds=3):
        if self.transport_state is None:
            self.transport_state = {
                "version": 1,
                "ready": True,
                "run_id": context.manifest["run_id"],
                "guild_id": context.manifest["discord"]["guild_id"],
                "actor_id": context.manifest["discord"]["actor_id"],
                "bot_user_id": context.manifest["discord"]["bot_user_id"],
                "instance_id": "d2ti-0123456789abcdef0123456789abcdef",
                "gateway": {
                    "partitioned": False,
                    "connections": 1,
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

    def wait_for_status(self, probe, expected, timeout_seconds=60):
        return probe()

    def launchd_bootout(self, label):
        self.bootouts.append(label)
        self.loaded.discard(label)


class D2IsolatedOrchestratorTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name).resolve()
        self.artifact_root = self.root / "immutable-candidates"
        self.artifact_root.mkdir()
        self.run_id = f"d2-20260801t120000z-{secrets.token_hex(6)}"
        self.isolated_root = pathlib.Path(f"/private/tmp/starring-d2-{self.run_id}")
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

    def tearDown(self):
        if self.isolated_root.exists():
            ORCHESTRATOR.guarded_remove_root(self.context)
        for path in self.artifact_root.rglob("*"):
            if path.is_dir():
                path.chmod(0o700)
        self.artifact_root.chmod(0o700)
        self.temporary.cleanup()

    def prepare_manifest(self):
        arguments = [
            "prepare",
            "--output-root",
            str(self.root / "evidence"),
            "--commit",
            "a" * 40,
            "--discord-guild-id",
            "1524810437118525551",
            "--discord-application-id",
            "1524810437118525552",
            "--discord-bot-user-id",
            "1524810437118525553",
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

    def test_dry_run_is_read_only_and_binds_dedicated_namespaces(self):
        result = ORCHESTRATOR.command_dry_run(self.context, self.platform)
        self.assertEqual(result["status"], "ready")
        self.assertFalse(result["standing_mutation_allowed"])
        self.assertEqual(self.platform.keychain_writes, [])
        self.assertEqual(self.platform.bootouts, [])
        self.assertFalse(self.isolated_root.exists())

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

    def test_prepare_start_stop_cleanup_is_idempotent_and_preserves_staging(self):
        prepared = ORCHESTRATOR.command_prepare(self.context, self.platform)
        self.assertEqual(prepared["phase"], "prepared")
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
            "version": 1,
            "ready": True,
            "run_id": self.context.manifest["run_id"],
            "guild_id": self.context.manifest["discord"]["guild_id"],
            "actor_id": self.context.manifest["discord"]["actor_id"],
            "bot_user_id": self.context.manifest["discord"]["bot_user_id"],
            "instance_id": "d2ti-0123456789abcdef0123456789abcdef",
            "gateway": {
                "partitioned": False,
                "connections": 1,
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
        widened = json.loads(json.dumps(snapshot))
        widened["gateway"]["unpinned"] = 0
        self.assertFalse(platform._transport_snapshot_valid(self.context, widened))

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
            ORCHESTRATOR.OrchestratorError, "transport_instance_changed"
        ):
            ORCHESTRATOR.command_transport_control(
                self.context, self.platform, "snapshot"
            )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "transport_instance_changed"
        ):
            ORCHESTRATOR.command_start(self.context, self.platform)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_prepare_failure_runs_manifest_reconstructed_cleanup(self):
        self.platform.initdb_failure = True
        with self.assertRaisesRegex(ORCHESTRATOR.OrchestratorError, "injected_initdb_failure"):
            ORCHESTRATOR.command_prepare(self.context, self.platform)
        self.assertFalse(self.isolated_root.exists())
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

    def test_preparing_state_allows_manifest_reconstructed_sigkill_cleanup(self):
        preflight = ORCHESTRATOR.command_dry_run(self.context, self.platform)
        self.context.artifact_directory.mkdir(mode=0o700, parents=True)
        ORCHESTRATOR.save_state(
            self.context, "preparing", preflight["standing_snapshot"]
        )
        self.isolated_root.mkdir(mode=0o700)
        (self.isolated_root / "partial").write_text("partial", encoding="utf-8")
        result = ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertEqual(result["phase"], "cleaned")
        self.assertFalse(self.isolated_root.exists())

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
        self.assertNotIn("display_name", evidence)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

    def test_cleanup_refuses_symlinked_root_and_recovers_after_operator_restore(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        preserved = self.isolated_root.with_name(f"{self.isolated_root.name}-preserved")
        self.isolated_root.rename(preserved)
        self.isolated_root.symlink_to(self.root)
        try:
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
            ):
                ORCHESTRATOR.command_cleanup(self.context, self.platform)
        finally:
            self.isolated_root.unlink()
            preserved.rename(self.isolated_root)
        ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertFalse(self.isolated_root.exists())

    def test_cleanup_refuses_keychain_namespace_without_matching_owner(self):
        ORCHESTRATOR.command_prepare(self.context, self.platform)
        service = self.context.manifest["keychain_services"]["api"]
        account = ORCHESTRATOR.keychain_inventory(self.context)[0][1]
        self.platform.keychain.add((service, account))
        self.platform.owner_values[service] = "different-run"
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "cleanup_incomplete"
        ):
            ORCHESTRATOR.command_cleanup(self.context, self.platform)
        self.assertIn((service, account), self.platform.keychain)
        self.platform.owner_values[service] = self.context.manifest["run_id"]
        ORCHESTRATOR.command_cleanup(self.context, self.platform)

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
        with self.assertRaisesRegex(
            CERTIFICATION.CertificationError, "cloudflare_route_binding_invalid"
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


if __name__ == "__main__":
    unittest.main()
