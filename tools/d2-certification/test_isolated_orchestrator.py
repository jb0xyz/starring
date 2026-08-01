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

    def http_status(self, url, timeout_seconds=3):
        if self.health_failure and self.health_failure in url:
            return 503
        return 200

    def worker_health_status(self, context, timeout_seconds=3):
        if self.health_failure == "worker":
            return 503
        return 200

    def wait_for_status(self, probe, expected, timeout_seconds=60):
        return probe()

    def launchd_bootout(self, label):
        self.bootouts.append(label)
        self.loaded.discard(label)


class D2IsolatedOrchestratorTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name)
        self.run_id = f"d2-20260801t120000z-{secrets.token_hex(6)}"
        self.isolated_root = pathlib.Path(f"/private/tmp/starring-d2-{self.run_id}")
        self.candidates = {}
        for name in CERTIFICATION.REQUIRED_CANDIDATES:
            path = (
                self.root / "worker-tree" / "worker.mjs"
                if name == "codex_worker"
                else self.root / name
            )
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(f"candidate:{name}".encode())
            self.candidates[name] = path
        for name in CERTIFICATION.CODEX_WORKER_SOURCE_FILES:
            path = self.candidates["codex_worker"].parent / name
            if not path.exists():
                path.write_bytes(f"worker-source:{name}".encode())
        self.manifest_path = self.prepare_manifest()
        self.context = ORCHESTRATOR.load_context(self.manifest_path)
        self.platform = FakePlatform()

    def tearDown(self):
        if self.isolated_root.exists():
            ORCHESTRATOR.guarded_remove_root(self.context)
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

    def test_prepare_start_stop_cleanup_is_idempotent_and_preserves_staging(self):
        prepared = ORCHESTRATOR.command_prepare(self.context, self.platform)
        self.assertEqual(prepared["phase"], "prepared")
        self.assertTrue((self.isolated_root / "postgres" / "PG_VERSION").is_file())
        self.assertEqual(len(self.platform.keychain_writes), 4)
        for name in ("api", "runtime", "worker", "tunnel"):
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
        self.assertEqual(
            self.platform.start_order,
            [
                self.context.manifest["services"][name]["label"]
                for name in ORCHESTRATOR.SERVICE_START_ORDER
            ],
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
        for name in ("api", "runtime", "worker", "tunnel"):
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


if __name__ == "__main__":
    unittest.main()
