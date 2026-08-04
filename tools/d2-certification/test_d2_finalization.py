import contextlib
import copy
import io
import json
import os
import pathlib
import subprocess
import sys
import unittest
from unittest import mock


DIRECTORY = pathlib.Path(__file__).parent
sys.path.insert(0, str(DIRECTORY))
import test_isolated_orchestrator as isolated_tests


ORCHESTRATOR = isolated_tests.ORCHESTRATOR
FINALIZATION = sys.modules["d2_finalization"]


class FinalizationPlatform(isolated_tests.FakePlatform):
    def __init__(self):
        super().__init__()
        self.database_present = True
        self.destroy_calls = 0
        self.inspect_calls = []
        self.fail_destroy_after_drop = False

    def run(self, arguments, input_bytes=None, timeout=30, environment=None):
        command = [str(value) for value in arguments]
        if command and pathlib.Path(command[0]).name == "sealed_provisioner":
            mode = command[1]
            if mode == "destroy":
                self.destroy_calls += 1
                outcome = "destroyed" if self.database_present else "exact_replay"
                self.database_present = False
                if self.fail_destroy_after_drop and self.destroy_calls == 1:
                    return subprocess.CompletedProcess(command, 1, b"", b"d2_destruction_failed\n")
                payload = {
                    "schema_version": 1,
                    "kind": FINALIZATION.DATABASE_DESTRUCTION_KIND,
                    "outcome": outcome,
                    "installation_id": self.installation_id,
                    "database_absent": True,
                }
                return subprocess.CompletedProcess(
                    command, 0, json.dumps(payload).encode(), b""
                )
            if mode == "inspect":
                checkpoint = command[-1]
                self.inspect_calls.append(checkpoint)
                if checkpoint == "precleanup":
                    payload = self.precleanup.copy()
                elif checkpoint == "absence" and not self.database_present:
                    payload = {
                        "schema_version": 1,
                        "kind": FINALIZATION.DATABASE_ABSENCE_KIND,
                        "observed_at": "2026-08-04T01:02:03.000001Z",
                        "run_id": self.run_id,
                        "installation_id": self.installation_id,
                        "database_absent": True,
                    }
                else:
                    return subprocess.CompletedProcess(
                        command, 1, b"", b"d2_inspection_failed\n"
                    )
                return subprocess.CompletedProcess(
                    command, 0, json.dumps(payload).encode(), b""
                )
        return super().run(arguments, input_bytes, timeout, environment)

    def bind(self, context):
        self.run_id = context.manifest["run_id"]
        self.installation_id = (
            f"installation:{context.manifest['discord']['resource_prefix']}"
        )
        self.precleanup = {
            "schema_version": 1,
            "kind": FINALIZATION.PRECLEANUP_KIND,
            "observed_at": "2026-08-04T01:02:01.000001Z",
            "installation_id": self.installation_id,
            "scoped_installation_count": 1,
            "scoped_deployment_count": 2,
            "terminal_product_operation_count": 2,
            "unresolved_product_operation_count": 0,
            "unresolved_receipt_count": 0,
            "unresolved_journal_entry_count": 0,
            "unresolved_rollback_count": 0,
            "ready_for_cleanup": True,
        }


class D2FinalizationTest(unittest.TestCase):
    def setUp(self):
        self.fixture = isolated_tests.D2IsolatedOrchestratorTest("runTest")
        self.fixture.setUp()
        self.context = self.fixture.context
        self.platform = FinalizationPlatform()
        self.platform.bind(self.context)
        self.fixture.platform = self.platform
        self.fixture.start_candidate_with_discord_resources()
        self.reconciliation_role_id = "1524810437118525590"
        self.platform.resource_history.append(
            {
                "kind": "role",
                "resource_id": self.reconciliation_role_id,
                "state": "created",
            }
        )
        self.platform.discord_existing.add(self.reconciliation_role_id)
        self.certified_inventory_digest = self.platform.resource_inventory(
            self.context
        )["digest_sha256"]
        self.certification_gate = mock.patch.object(
            FINALIZATION,
            "require_certification_prefix",
            return_value={
                "step15_receipt_sha256": "a" * 64,
                "step15_completion_sha256": "b" * 64,
                "step15_completed_at": "2026-08-04T01:02:00.000001Z",
                "reconciliation_inventory_digest_sha256": (
                    self.certified_inventory_digest
                ),
            },
        )
        self.certification_mock = self.certification_gate.start()
        self.step16_gate = mock.patch.object(
            FINALIZATION,
            "require_certification_step_sixteen",
            return_value={
                "step16_receipt_sha256": "c" * 64,
                "step16_completion_sha256": "d" * 64,
                "step16_completed_at": "2026-08-04T01:02:30.000001Z",
            },
        )
        self.step16_mock = self.step16_gate.start()

    def tearDown(self):
        self.step16_gate.stop()
        self.certification_gate.stop()
        self.fixture.tearDown()

    def finalize(self):
        return FINALIZATION.command_finalize_run(
            self.context,
            self.platform,
            ORCHESTRATOR.command_cleanup,
            ORCHESTRATOR.command_teardown_discord_resources,
        )

    def prepare_certified_teardown(self):
        FINALIZATION._ensure_effect_freeze(self.context, self.platform)
        return ORCHESTRATOR.command_teardown_discord_resources(
            self.context, self.platform, frozen=True
        )

    def write_browser_evidence(self, name, value):
        path = self.context.run_directory / name
        path.write_text(json.dumps(value), encoding="utf-8")
        path.chmod(0o600)
        return path

    def prefix_scan(self, **overrides):
        value = {
            "schema_version": 1,
            "kind": FINALIZATION.PREFIX_SCAN_KIND,
            "observed_at": "2026-08-04T01:03:00.000001Z",
            "guild_id": self.context.manifest["discord"]["guild_id"],
            "resource_prefix": self.context.manifest["discord"]["resource_prefix"],
            "guild_observation_http_status": 200,
            "role_count": 0,
            "channel_count": 0,
            "panel_count": 0,
            "resource_prefix_match_count": 0,
        }
        value.update(overrides)
        return self.write_browser_evidence("prefix-scan.json", value)

    def guild_deletion(self, **overrides):
        value = {
            "schema_version": 1,
            "kind": FINALIZATION.GUILD_DELETION_KIND,
            "observed_at": "2026-08-04T01:04:00.000001Z",
            "guild_id": self.context.manifest["discord"]["guild_id"],
            "deletion_confirmed": True,
            "guild_observation_http_status": 404,
            "discord_error_code": 10004,
            "confirmation_surface": "chrome",
        }
        value.update(overrides)
        return self.write_browser_evidence("guild-deletion.json", value)

    def test_finalize_run_drops_only_scoped_database_and_cleans_owned_state(self):
        standing = ORCHESTRATOR.standing_snapshot(self.context, self.platform)
        external = set(ORCHESTRATOR.external_keychain_inventory(self.context))
        candidate_labels = {
            service["label"] for service in self.context.manifest["services"].values()
        }
        result = self.finalize()
        self.assertEqual(result["status"], "finalized")
        self.assertFalse(self.platform.database_present)
        self.assertFalse(self.platform.postgres)
        self.assertFalse(self.context.root.exists())
        self.assertTrue(candidate_labels.isdisjoint(self.platform.loaded))
        self.assertTrue(external.issubset(self.platform.keychain))
        self.assertEqual(
            ORCHESTRATOR.standing_snapshot(self.context, self.platform), standing
        )
        self.assertEqual(self.platform.destroy_calls, 1)
        self.assertEqual(self.platform.inspect_calls, ["precleanup", "absence"])
        step = json.loads(
            FINALIZATION.step_sixteen_evidence_path(self.context).read_text()
        )
        teardown = json.loads(
            (
                self.context.artifact_directory
                / "discord-resource-teardown-evidence.json"
            ).read_text()
        )
        self.assertEqual(step["discord_resource_ids_deleted"], teardown["resource_ids"])
        for path in (
            FINALIZATION.destroy_intent_path(self.context),
            FINALIZATION.destroy_result_path(self.context),
            FINALIZATION.database_absence_path(self.context),
            FINALIZATION.finalization_evidence_path(self.context),
            FINALIZATION.step_sixteen_evidence_path(self.context),
        ):
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_finalize_run_exact_replay_is_non_mutating(self):
        self.finalize()
        bootouts = list(self.platform.bootouts)
        deletes = list(self.platform.keychain_deletes)
        result = self.finalize()
        self.assertEqual(result["status"], "exact_replay")
        self.assertEqual(self.platform.destroy_calls, 1)
        self.assertEqual(self.platform.bootouts, bootouts)
        self.assertEqual(self.platform.keychain_deletes, deletes)

    def test_finalize_requires_complete_coordinator_prefix_before_freeze(self):
        self.certification_mock.side_effect = ORCHESTRATOR.OrchestratorError(
            "finalization_certification_prefix_incomplete"
        )
        bootouts = list(self.platform.bootouts)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "finalization_certification_prefix_incomplete",
        ):
            self.finalize()
        self.assertEqual(self.platform.bootouts, bootouts)
        self.assertEqual(self.platform.destroy_calls, 0)
        self.assertFalse(FINALIZATION.freeze_intent_path(self.context).exists())

    def test_finalize_requires_external_credentials_before_freeze(self):
        external = ORCHESTRATOR.external_keychain_inventory(self.context)[0]
        self.platform.keychain.remove(external)
        bootouts = list(self.platform.bootouts)
        existing = set(self.platform.discord_existing)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "external_keychain_identity_absent",
        ):
            self.finalize()
        self.assertEqual(self.platform.bootouts, bootouts)
        self.assertEqual(self.platform.discord_existing, existing)
        self.assertEqual(self.platform.destroy_calls, 0)
        self.assertFalse(FINALIZATION.freeze_intent_path(self.context).exists())
        self.assertFalse(
            ORCHESTRATOR.discord_teardown_evidence_path(
                self.context, frozen=True
            ).exists()
        )

    def test_finalize_rechecks_external_credentials_after_freeze(self):
        external = ORCHESTRATOR.external_keychain_inventory(self.context)[0]
        existing = set(self.platform.discord_existing)
        real_bootout = self.platform.launchd_bootout
        removed = False

        def bootout(label):
            nonlocal removed
            real_bootout(label)
            if not removed:
                removed = True
                self.platform.keychain.remove(external)

        self.platform.launchd_bootout = bootout
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "external_keychain_identity_absent",
        ):
            self.finalize()
        self.assertEqual(self.platform.discord_existing, existing)
        self.assertEqual(self.platform.destroy_calls, 0)
        self.assertTrue(FINALIZATION.freeze_intent_path(self.context).is_file())
        self.assertFalse(
            ORCHESTRATOR.discord_teardown_evidence_path(
                self.context, frozen=True
            ).exists()
        )

    def test_existing_freeze_revalidates_step15_completion_before_mutation(self):
        FINALIZATION._ensure_effect_freeze(self.context, self.platform)
        bootouts = list(self.platform.bootouts)
        self.certification_mock.side_effect = ORCHESTRATOR.OrchestratorError(
            "finalization_certification_prefix_incomplete"
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "finalization_certification_prefix_incomplete",
        ):
            self.finalize()
        self.assertEqual(self.platform.bootouts, bootouts)
        self.assertEqual(self.platform.destroy_calls, 0)

    def test_existing_freeze_rejects_step15_completion_and_chronology_drift(self):
        FINALIZATION._ensure_effect_freeze(self.context, self.platform)
        bootouts = list(self.platform.bootouts)
        self.certification_mock.return_value = {
            "step15_receipt_sha256": "a" * 64,
            "step15_completion_sha256": "e" * 64,
            "step15_completed_at": "2026-08-04T01:02:00.000001Z",
            "reconciliation_inventory_digest_sha256": (
                self.certified_inventory_digest
            ),
        }
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "finalization_freeze_certification_invalid",
        ):
            self.finalize()
        self.assertEqual(self.platform.bootouts, bootouts)
        self.certification_mock.return_value = {
            "step15_receipt_sha256": "a" * 64,
            "step15_completion_sha256": "b" * 64,
            "step15_completed_at": "2026-08-04T01:02:00.000001Z",
            "reconciliation_inventory_digest_sha256": (
                self.certified_inventory_digest
            ),
        }
        path = FINALIZATION.freeze_intent_path(self.context)
        intent = json.loads(path.read_text())
        intent["recorded_at"] = "2026-08-04T01:01:59.000001Z"
        path.write_text(json.dumps(intent), encoding="utf-8")
        path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "finalization_freeze_chronology_invalid",
        ):
            self.finalize()
        self.assertEqual(self.platform.bootouts, bootouts)
        self.assertEqual(self.platform.destroy_calls, 0)

    def test_freeze_rejects_uncertified_inventory_before_any_mutation(self):
        resource_id = "1524810437118525599"
        self.platform.resource_history.append(
            {"kind": "role", "resource_id": resource_id, "state": "created"}
        )
        self.platform.discord_existing.add(resource_id)
        bootouts = list(self.platform.bootouts)
        existing = set(self.platform.discord_existing)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "finalization_freeze_certification_invalid",
        ):
            self.finalize()
        self.assertEqual(self.platform.bootouts, bootouts)
        self.assertEqual(self.platform.discord_existing, existing)
        self.assertEqual(self.platform.destroy_calls, 0)
        self.assertFalse(FINALIZATION.freeze_intent_path(self.context).exists())

    def test_freeze_rejects_boolean_schema_version(self):
        FINALIZATION._ensure_effect_freeze(self.context, self.platform)
        path = FINALIZATION.freeze_intent_path(self.context)
        intent = json.loads(path.read_text())
        intent["schema_version"] = True
        path.write_text(json.dumps(intent), encoding="utf-8")
        path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "finalization_freeze_intent_invalid",
        ):
            self.finalize()
        self.assertEqual(self.platform.destroy_calls, 0)

    def test_finalize_requires_healed_unarmed_transport_before_freeze(self):
        cases = (
            ("gateway", "partitioned", True),
            ("gateway", "duplicate_armed", True),
            ("effect_http", "indeterminate_claimed", True),
        )
        for section, field, value in cases:
            with self.subTest(section=section, field=field):
                self.platform.transport_state[section][field] = value
                bootouts = list(self.platform.bootouts)
                with self.assertRaisesRegex(
                    ORCHESTRATOR.OrchestratorError,
                    "finalization_transport_quiescence_invalid",
                ):
                    self.finalize()
                self.assertEqual(self.platform.bootouts, bootouts)
                self.assertEqual(self.platform.destroy_calls, 0)
                self.assertFalse(FINALIZATION.freeze_intent_path(self.context).exists())
                self.platform.transport_state[section][field] = False

    def test_standalone_teardown_permanently_disqualifies_certification(self):
        result = ORCHESTRATOR.command_teardown_discord_resources(
            self.context, self.platform
        )
        self.assertEqual(result["status"], "torn_down")
        self.assertTrue(FINALIZATION.abort_teardown_tombstone_path(self.context).exists())
        self.assertTrue(FINALIZATION.abort_teardown_evidence_path(self.context).exists())
        self.assertFalse(
            (
                self.context.artifact_directory
                / "discord-resource-teardown-evidence.json"
            ).exists()
        )
        bootouts = list(self.platform.bootouts)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "standalone_teardown_certification_disqualified",
        ):
            self.finalize()
        self.assertEqual(self.platform.bootouts, bootouts)
        self.assertEqual(self.platform.destroy_calls, 0)
        self.assertFalse(FINALIZATION.freeze_intent_path(self.context).exists())

    def test_finalization_directory_rejects_symlink_and_permissive_mode(self):
        finalization = FINALIZATION.finalization_directory(self.context)
        outside = self.context.run_directory / "outside-finalization"
        outside.mkdir(mode=0o700)
        finalization.symlink_to(outside, target_is_directory=True)
        bootouts = list(self.platform.bootouts)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "finalization_directory_invalid"
        ):
            self.finalize()
        self.assertEqual(self.platform.bootouts, bootouts)
        self.assertEqual(list(outside.iterdir()), [])
        finalization.unlink()
        finalization.mkdir(mode=0o755)
        finalization.chmod(0o755)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "finalization_directory_invalid"
        ):
            self.finalize()
        self.assertEqual(list(finalization.iterdir()), [])

    def test_mutation_roots_reject_symlink_before_any_service_stop(self):
        bootouts = list(self.platform.bootouts)
        root = self.context.root
        backup = root.with_name(root.name + "-real")
        root.rename(backup)
        root.symlink_to(backup, target_is_directory=True)
        try:
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError,
                "finalization_mutation_root_invalid",
            ):
                self.finalize()
            self.assertEqual(self.platform.bootouts, bootouts)
            self.assertEqual(self.platform.destroy_calls, 0)
        finally:
            root.unlink()
            backup.rename(root)

    def test_mutation_cluster_rejects_symlink_before_any_service_stop(self):
        bootouts = list(self.platform.bootouts)
        cluster = self.context.cluster_root
        backup = cluster.with_name(cluster.name + "-real")
        cluster.rename(backup)
        cluster.symlink_to(backup, target_is_directory=True)
        try:
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError,
                "finalization_mutation_cluster_invalid",
            ):
                self.finalize()
            self.assertEqual(self.platform.bootouts, bootouts)
            self.assertEqual(self.platform.destroy_calls, 0)
        finally:
            cluster.unlink()
            backup.rename(cluster)

    def test_finalization_evidence_is_immutable_across_step_write_retry(self):
        real_write = FINALIZATION.write_atomic
        failed = False

        def fail_step(path, payload, mode=0o600):
            nonlocal failed
            if pathlib.Path(path) == FINALIZATION.step_sixteen_evidence_path(
                self.context
            ) and not failed:
                failed = True
                raise ORCHESTRATOR.OrchestratorError("injected_step16_write_failure")
            return real_write(path, payload, mode)

        with mock.patch.object(FINALIZATION, "write_atomic", side_effect=fail_step):
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, "injected_step16_write_failure"
            ):
                self.finalize()
        path = FINALIZATION.finalization_evidence_path(self.context)
        before = path.read_bytes()
        result = self.finalize()
        self.assertEqual(result["status"], "finalized")
        self.assertEqual(path.read_bytes(), before)

    def test_freeze_catches_resource_created_during_first_bootout(self):
        real_bootout = self.platform.launchd_bootout
        injected = False
        resource_id = "1524810437118525599"

        def bootout(label):
            nonlocal injected
            if not injected:
                injected = True
                self.platform.resource_history.append(
                    {"kind": "role", "resource_id": resource_id, "state": "created"}
                )
                self.platform.discord_existing.add(resource_id)
            return real_bootout(label)

        self.platform.launchd_bootout = bootout
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "discord_teardown_live_inventory_drift",
        ):
            self.finalize()
        self.assertEqual(self.platform.destroy_calls, 0)
        self.assertTrue(self.platform.database_present)
        self.assertIn(resource_id, self.platform.discord_existing)

    def test_api_worker_stop_precedes_final_zero_blocker_capture(self):
        real_bootout = self.platform.launchd_bootout
        api_label = self.context.manifest["services"]["api"]["label"]

        def bootout(label):
            if label == api_label:
                self.platform.precleanup["unresolved_receipt_count"] = 1
            return real_bootout(label)

        self.platform.launchd_bootout = bootout
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "database_precleanup_blocked"
        ):
            self.finalize()
        self.assertEqual(self.platform.destroy_calls, 0)
        self.assertFalse(FINALIZATION.destroy_intent_path(self.context).exists())

    def test_public_step16_assembler_rejects_partial_and_extra_sources(self):
        self.finalize()
        database = json.loads(FINALIZATION.precleanup_path(self.context).read_text())
        teardown = json.loads(
            (
                self.context.artifact_directory
                / "discord-resource-teardown-evidence.json"
            ).read_text()
        )
        finalization = json.loads(
            FINALIZATION.finalization_evidence_path(self.context).read_text()
        )
        self.assertTrue(
            FINALIZATION.assemble_teardown_evidence(
                database,
                teardown,
                finalization,
                FINALIZATION.local_step16_binding(self.context, teardown),
            )["services_stopped"]
        )
        for index, values in enumerate(
            (
                ({}, teardown, finalization),
                (database, {}, finalization),
                (database, teardown, {}),
            )
        ):
            with self.subTest(index=index):
                with self.assertRaises(ORCHESTRATOR.OrchestratorError):
                    FINALIZATION.assemble_teardown_evidence(
                        *values,
                        FINALIZATION.local_step16_binding(self.context, teardown),
                    )
        for index, values in enumerate(
            (
                (copy.deepcopy(database), teardown, finalization),
                (database, copy.deepcopy(teardown), finalization),
                (database, teardown, copy.deepcopy(finalization)),
            )
        ):
            values[index]["schema_version"] = True
            with self.subTest(boolean_schema=index):
                with self.assertRaises(ORCHESTRATOR.OrchestratorError):
                    FINALIZATION.assemble_teardown_evidence(
                        *values,
                        FINALIZATION.local_step16_binding(self.context, teardown),
                    )
        for index, values in enumerate(
            (
                (copy.deepcopy(database), teardown, finalization),
                (database, copy.deepcopy(teardown), finalization),
                (database, teardown, copy.deepcopy(finalization)),
            )
        ):
            values[index]["extra"] = True
            with self.subTest(extra=index):
                with self.assertRaises(ORCHESTRATOR.OrchestratorError):
                    FINALIZATION.assemble_teardown_evidence(
                        *values,
                        FINALIZATION.local_step16_binding(self.context, teardown),
                    )

    def test_active_discord_resources_block_database_inspection_and_stop(self):
        self.prepare_certified_teardown()
        path = self.context.artifact_directory / "discord-resource-teardown-evidence.json"
        value = json.loads(path.read_text())
        value["active_resources"] = [value["created_resources"][0]]
        path.write_text(json.dumps(value), encoding="utf-8")
        path.chmod(0o600)
        bootouts = list(self.platform.bootouts)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "discord_teardown_evidence_invalid"
        ):
            self.finalize()
        self.assertEqual(self.platform.inspect_calls, [])
        self.assertEqual(self.platform.bootouts, bootouts)
        self.assertTrue(self.platform.database_present)

    def test_precleanup_blocker_prevents_service_stop_and_destroy(self):
        self.platform.precleanup["unresolved_receipt_count"] = 1
        bootouts = list(self.platform.bootouts)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "database_precleanup_blocked"
        ):
            self.finalize()
        self.assertEqual(
            self.platform.bootouts[len(bootouts) :],
            [
                self.context.manifest["services"][name]["label"]
                for name in FINALIZATION.FREEZE_STOP_ORDER + ("api", "worker")
            ],
        )
        self.assertEqual(self.platform.destroy_calls, 0)
        self.assertTrue(self.platform.database_present)

    def test_post_teardown_active_resource_drift_blocks_before_stop_or_destroy(self):
        self.prepare_certified_teardown()
        resource_id = "1524810437118525599"
        self.platform.resource_history.append(
            {"kind": "role", "resource_id": resource_id, "state": "created"}
        )
        self.platform.discord_existing.add(resource_id)
        bootouts = list(self.platform.bootouts)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "discord_resource_teardown_evidence_invalid|discord_teardown_live_inventory_drift",
        ):
            self.finalize()
        self.assertEqual(self.platform.bootouts, bootouts)
        self.assertEqual(self.platform.destroy_calls, 0)
        self.assertIn(resource_id, self.platform.discord_existing)

    def test_teardown_direct_observation_rejects_false_with_success_status(self):
        self.prepare_certified_teardown()
        path = self.context.artifact_directory / "discord-resource-teardown-evidence.json"
        value = json.loads(path.read_text())
        observation = next(
            item
            for item in value["direct_observations"]
            if item["resource_kind"] == "channel"
        )
        observation["http_status"] = 200
        observation["discord_code"] = None
        path.write_text(json.dumps(value), encoding="utf-8")
        path.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "discord_teardown_evidence_invalid"
        ):
            self.finalize()
        self.assertEqual(self.platform.destroy_calls, 0)

    def test_destroy_interruption_resumes_with_exact_scoped_replay(self):
        self.platform.fail_destroy_after_drop = True
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "sealed_destroy_failed"
        ):
            self.finalize()
        self.assertFalse(self.platform.database_present)
        self.assertTrue(FINALIZATION.destroy_intent_path(self.context).is_file())
        self.assertFalse(FINALIZATION.destroy_result_path(self.context).exists())
        result = self.finalize()
        self.assertEqual(result["status"], "finalized")
        self.assertEqual(self.platform.destroy_calls, 2)
        destroy = json.loads(FINALIZATION.destroy_result_path(self.context).read_text())
        self.assertEqual(destroy["outcome"], "exact_replay")

    def test_partial_cleanup_with_stopped_postgres_resumes(self):
        self.prepare_certified_teardown()
        FINALIZATION.command_finalize_database(self.context, self.platform)
        self.platform.postgres_stop(self.context.cluster_root)
        result = self.finalize()
        self.assertEqual(result["status"], "finalized")
        self.assertEqual(self.platform.destroy_calls, 1)

    def test_total_absence_combines_prefix_scan_and_chrome_guild_deletion(self):
        self.finalize()
        prefix = self.prefix_scan()
        guild = self.guild_deletion()
        result = FINALIZATION.command_finalize_total_absence(
            self.context, self.platform, str(prefix), str(guild)
        )
        self.assertEqual(result["status"], "total_absence_confirmed")
        step = json.loads(
            FINALIZATION.step_seventeen_evidence_path(self.context).read_text()
        )
        self.assertEqual(
            step,
            {
                "unresolved_operation_count": 0,
                "unresolved_receipt_count": 0,
                "unresolved_journal_count": 0,
                "route_count": 0,
                "instance_count": 0,
                "role_count": 0,
                "channel_count": 0,
                "panel_count": 0,
                "resource_prefix_match_count": 0,
                "database_absent": True,
                "postgres_process_absent": True,
                "launchd_jobs_absent": True,
                "keychain_items_absent": True,
                "discord_guild_deleted": True,
            },
        )
        replay = FINALIZATION.command_finalize_total_absence(
            self.context, self.platform, str(prefix), str(guild)
        )
        self.assertEqual(replay["status"], "exact_replay")

    def test_total_absence_requires_step16_completion_before_any_write(self):
        self.finalize()
        prefix = self.prefix_scan()
        guild = self.guild_deletion()
        self.step16_mock.side_effect = ORCHESTRATOR.OrchestratorError(
            "total_absence_step16_completion_missing"
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "total_absence_step16_completion_missing",
        ):
            FINALIZATION.command_finalize_total_absence(
                self.context, self.platform, str(prefix), str(guild)
            )
        self.assertFalse(FINALIZATION.orchestration_absence_path(self.context).exists())
        self.assertFalse(FINALIZATION.step_seventeen_evidence_path(self.context).exists())

    def test_total_absence_remains_disqualified_by_abort_tombstone(self):
        self.finalize()
        prefix = self.prefix_scan()
        guild = self.guild_deletion()
        tombstone = FINALIZATION.abort_teardown_tombstone_path(self.context)
        tombstone.write_text("{}\n", encoding="utf-8")
        tombstone.chmod(0o600)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "standalone_teardown_certification_disqualified",
        ):
            FINALIZATION.command_finalize_total_absence(
                self.context, self.platform, str(prefix), str(guild)
            )
        self.assertFalse(FINALIZATION.orchestration_absence_path(self.context).exists())
        self.assertFalse(FINALIZATION.step_seventeen_evidence_path(self.context).exists())

    def test_total_absence_rejects_future_browser_evidence(self):
        self.finalize()
        prefix = self.prefix_scan(observed_at="2099-01-01T01:00:00.000001Z")
        guild = self.guild_deletion(observed_at="2099-01-01T02:00:00.000001Z")
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "total_absence_observation_chronology_invalid",
        ):
            FINALIZATION.command_finalize_total_absence(
                self.context, self.platform, str(prefix), str(guild)
            )
        self.assertFalse(FINALIZATION.orchestration_absence_path(self.context).exists())
        self.assertFalse(FINALIZATION.step_seventeen_evidence_path(self.context).exists())

    def test_total_absence_requires_current_keychain_boundaries(self):
        self.finalize()
        prefix = self.prefix_scan()
        guild = self.guild_deletion()
        external = ORCHESTRATOR.external_keychain_inventory(self.context)[0]
        self.platform.keychain.remove(external)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "external_keychain_identity_absent",
        ):
            FINALIZATION.command_finalize_total_absence(
                self.context, self.platform, str(prefix), str(guild)
            )
        self.platform.keychain.add(external)
        owned = ORCHESTRATOR.keychain_inventory(self.context)[0]
        self.platform.keychain.add(owned)
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "run_keychain_items_still_present",
        ):
            FINALIZATION.command_finalize_total_absence(
                self.context, self.platform, str(prefix), str(guild)
            )
        self.platform.keychain.remove(owned)
        self.assertFalse(FINALIZATION.orchestration_absence_path(self.context).exists())
        self.assertFalse(FINALIZATION.step_seventeen_evidence_path(self.context).exists())

    def test_total_absence_step_write_crash_replays_immutable_observation(self):
        self.finalize()
        prefix = self.prefix_scan()
        guild = self.guild_deletion()
        real_write = FINALIZATION.write_atomic
        failed = False

        def fail_step(path, payload, mode=0o600):
            nonlocal failed
            if (
                pathlib.Path(path)
                == FINALIZATION.step_seventeen_evidence_path(self.context)
                and not failed
            ):
                failed = True
                raise ORCHESTRATOR.OrchestratorError(
                    "injected_step17_write_failure"
                )
            return real_write(path, payload, mode)

        with mock.patch.object(FINALIZATION, "write_atomic", side_effect=fail_step):
            with self.assertRaisesRegex(
                ORCHESTRATOR.OrchestratorError, "injected_step17_write_failure"
            ):
                FINALIZATION.command_finalize_total_absence(
                    self.context, self.platform, str(prefix), str(guild)
                )
        orchestration = FINALIZATION.orchestration_absence_path(self.context)
        before = orchestration.read_bytes()
        result = FINALIZATION.command_finalize_total_absence(
            self.context, self.platform, str(prefix), str(guild)
        )
        self.assertEqual(result["status"], "total_absence_confirmed")
        self.assertEqual(orchestration.read_bytes(), before)

    def test_total_absence_requires_step16_before_prefix_scan(self):
        self.finalize()
        prefix = self.prefix_scan()
        guild = self.guild_deletion()
        self.step16_mock.return_value = {
            "step16_receipt_sha256": "c" * 64,
            "step16_completion_sha256": "d" * 64,
            "step16_completed_at": "2026-08-04T01:03:00.000001Z",
        }
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "step16_absence_chronology_invalid",
        ):
            FINALIZATION.command_finalize_total_absence(
                self.context, self.platform, str(prefix), str(guild)
            )
        self.assertFalse(FINALIZATION.orchestration_absence_path(self.context).exists())
        self.assertFalse(FINALIZATION.step_seventeen_evidence_path(self.context).exists())

    def test_public_step17_assembler_rejects_partial_extra_and_identity_drift(self):
        self.finalize()
        prefix = self.prefix_scan()
        guild_path = self.guild_deletion()
        FINALIZATION.command_finalize_total_absence(
            self.context, self.platform, str(prefix), str(guild_path)
        )
        database = json.loads(
            FINALIZATION.database_absence_path(self.context).read_text()
        )
        orchestration = json.loads(
            FINALIZATION.orchestration_absence_path(self.context).read_text()
        )
        guild = json.loads(guild_path.read_text())
        self.assertTrue(
            FINALIZATION.assemble_absence_evidence(
                database,
                orchestration,
                json.loads(prefix.read_text()),
                guild,
                FINALIZATION.local_step17_binding(
                    self.context,
                    json.loads(FINALIZATION.precleanup_path(self.context).read_text()),
                    json.loads(
                        (
                            self.context.artifact_directory
                            / "discord-resource-teardown-evidence.json"
                        ).read_text()
                    ),
                    self.step16_mock.return_value,
                ),
            )["discord_guild_deleted"]
        )
        binding = FINALIZATION.local_step17_binding(
            self.context,
            json.loads(FINALIZATION.precleanup_path(self.context).read_text()),
            json.loads(
                (
                    self.context.artifact_directory
                    / "discord-resource-teardown-evidence.json"
                ).read_text()
            ),
            self.step16_mock.return_value,
        )
        prefix_value = json.loads(prefix.read_text())
        for index, values in enumerate(
            (
                ({}, orchestration, prefix_value, guild),
                (database, {}, prefix_value, guild),
                (database, orchestration, {}, guild),
                (database, orchestration, prefix_value, {}),
            )
        ):
            with self.subTest(index=index):
                with self.assertRaises(ORCHESTRATOR.OrchestratorError):
                    FINALIZATION.assemble_absence_evidence(*values, binding)
        extra = copy.deepcopy(orchestration)
        extra["extra"] = True
        with self.assertRaises(ORCHESTRATOR.OrchestratorError):
            FINALIZATION.assemble_absence_evidence(
                database, extra, prefix_value, guild, binding
            )
        drifted = copy.deepcopy(guild)
        drifted["guild_id"] = "1524810437118525599"
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "step17_source_evidence_invalid"
        ):
            FINALIZATION.assemble_absence_evidence(
                database, orchestration, prefix_value, drifted, binding
            )
        chronology = copy.deepcopy(orchestration)
        chronology["observed_at"] = guild["observed_at"]
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "step17_source_chronology_invalid"
        ):
            FINALIZATION.assemble_absence_evidence(
                database, chronology, prefix_value, guild, binding
            )
        for index, values in enumerate(
            (
                (copy.deepcopy(database), orchestration, prefix_value, guild),
                (database, copy.deepcopy(orchestration), prefix_value, guild),
                (database, orchestration, copy.deepcopy(prefix_value), guild),
                (database, orchestration, prefix_value, copy.deepcopy(guild)),
            )
        ):
            values[index]["schema_version"] = True
            with self.subTest(boolean_schema=index):
                with self.assertRaises(ORCHESTRATOR.OrchestratorError):
                    FINALIZATION.assemble_absence_evidence(*values, binding)

    def test_total_absence_rejects_nonzero_prefix_scan(self):
        self.finalize()
        prefix = self.prefix_scan(resource_prefix_match_count=1, role_count=1)
        guild = self.guild_deletion()
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError, "discord_prefix_scan_evidence_invalid"
        ):
            FINALIZATION.command_finalize_total_absence(
                self.context, self.platform, str(prefix), str(guild)
            )
        self.assertFalse(FINALIZATION.step_seventeen_evidence_path(self.context).exists())

    def test_total_absence_requires_chrome_confirmed_unknown_guild(self):
        self.finalize()
        prefix = self.prefix_scan()
        guild = self.guild_deletion(
            guild_observation_http_status=200,
            discord_error_code=None,
            deletion_confirmed=False,
        )
        with self.assertRaisesRegex(
            ORCHESTRATOR.OrchestratorError,
            "discord_guild_deletion_evidence_invalid",
        ):
            FINALIZATION.command_finalize_total_absence(
                self.context, self.platform, str(prefix), str(guild)
            )

    def test_cli_surface_exposes_only_explicit_finalization_boundaries(self):
        parser = ORCHESTRATOR.build_parser()
        finalize = parser.parse_args(
            ["finalize-run", "--manifest", str(self.fixture.manifest_path)]
        )
        self.assertEqual(finalize.command, "finalize-run")
        absence = parser.parse_args(
            [
                "finalize-total-absence",
                "--manifest",
                str(self.fixture.manifest_path),
                "--prefix-scan-evidence",
                str(self.context.run_directory / "prefix.json"),
                "--guild-deletion-evidence",
                str(self.context.run_directory / "guild.json"),
            ]
        )
        self.assertEqual(absence.command, "finalize-total-absence")


if __name__ == "__main__":
    with contextlib.redirect_stdout(io.StringIO()):
        unittest.main()
