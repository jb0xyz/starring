#!/usr/bin/env python3

import copy
import hashlib
import json
import pathlib
import tempfile
import unittest
from unittest import mock

import d2_legacy_substrate_recovery as recovery
from d2_certification import canonical_json
from d2_orchestrator_contract import OrchestratorError, keychain_inventory


class FakePlatform:
    def __init__(self):
        self.loaded = set()
        self.postgres = False
        self.launchd_error = None
        self.postgres_error = None
        self.cluster_identity = "7669853998318333589"
        self.keychain = set()
        self.owner_values = {}
        self.deleted = []
        self.delete_failure = None
        self.replace_at_delete = None
        self.crash_after_delete = None
        self.identity_versions = {}
        self.identity_reads = {}
        self.replace_identity = None
        self.replace_identity_at_read = None
        self.owner_match_reads = {}
        self.owner_drift_service = None
        self.owner_drift_at_read = None

    def launchd_absent(self, label):
        if self.launchd_error is not None:
            raise OrchestratorError(self.launchd_error)
        return label not in self.loaded

    def postgres_absent(self, cluster_root):
        if self.postgres_error is not None:
            raise OrchestratorError(self.postgres_error)
        return not self.postgres

    def postgres_cluster_identity(self, cluster_root):
        return self.cluster_identity

    def keychain_present(self, service, account):
        return (service, account) in self.keychain

    def keychain_item_identity(self, service, account):
        identity = (service, account)
        if identity not in self.keychain:
            return None
        reads = self.identity_reads.get(identity, 0) + 1
        self.identity_reads[identity] = reads
        if (
            identity == self.replace_identity
            and reads == self.replace_identity_at_read
        ):
            self.identity_versions[identity] = (
                self.identity_versions.get(identity, 0) + 1
            )
        version = self.identity_versions.get(identity, 0)
        return hashlib.sha256(
            f"{service}\x00{account}\x00{version}".encode("utf-8")
        ).hexdigest()

    def keychain_owner_matches(self, service, expected):
        reads = self.owner_match_reads.get(service, 0) + 1
        self.owner_match_reads[service] = reads
        if (
            service == self.owner_drift_service
            and reads == self.owner_drift_at_read
        ):
            return False
        return self.owner_values.get(service) == expected

    def keychain_delete_exact(self, service, account, expected_identity):
        if self.delete_failure == (service, account):
            raise OrchestratorError("keychain_delete_failed")
        if self.keychain_item_identity(service, account) != expected_identity:
            raise OrchestratorError("keychain_reference_identity_drift")
        if self.replace_at_delete == (service, account):
            identity = (service, account)
            self.identity_versions[identity] = (
                self.identity_versions.get(identity, 0) + 1
            )
            return
        self.keychain.discard((service, account))
        self.deleted.append((service, account))
        if self.crash_after_delete == (service, account):
            raise OrchestratorError("injected_keychain_delete_crash")


class LegacySubstrateRecoveryTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.base = pathlib.Path(self.temporary.name).resolve()
        self.runtime_parent = self.base / "runtime"
        self.runtime_parent.mkdir(mode=0o700)
        self.release_root = self.base / "release-certifications"
        self.release_root.mkdir(mode=0o700)
        self.run_id = "d2-20260803t171032z-fd1232a7b31c"
        self.run_directory = self.release_root / self.run_id
        self.run_directory.mkdir(mode=0o700)
        self.artifact_directory = self.run_directory / "orchestrator"
        self.artifact_directory.mkdir(mode=0o700)
        self.root = self.runtime_parent / f"starring-d2-{self.run_id}"
        self.root.mkdir(mode=0o700)
        (self.root / "postgres").mkdir(mode=0o700)
        (self.root / "socket").mkdir(mode=0o700)
        self.snapshot = {
            "launchd_loaded": {
                "local.cloudflared.starring": True,
                "local.starring.api.staging": True,
                "local.starring.codex-worker": True,
                "local.starring.runtime.staging": True,
            },
            "plist_sha256": {
                "local.cloudflared.starring": "1" * 64,
                "local.starring.api.staging": "2" * 64,
                "local.starring.codex-worker": "3" * 64,
                "local.starring.runtime.staging": "4" * 64,
            },
            "port_occupied": {
                "5432": True,
                "18080": True,
                "18181": True,
                "19091": True,
            },
        }
        self.manifest = self.build_manifest()
        self.manifest_path = self.run_directory / "manifest.json"
        self.digest = self.write_manifest(self.manifest)
        self.write_state("candidate_started")
        self.write_historical_provenance()
        self.platform = FakePlatform()
        self.runtime_patch = mock.patch.object(
            recovery, "D2_RUNTIME_ROOT_PARENT", self.runtime_parent
        )
        self.snapshot_patch = mock.patch.object(
            recovery, "standing_snapshot", return_value=copy.deepcopy(self.snapshot)
        )
        self.registry_patch = mock.patch.object(
            recovery,
            "load_discord_ownership_registry",
            return_value={"schema_version": 1, "kind": "test", "owners": []},
        )
        self.runtime_patch.start()
        self.snapshot_patch.start()
        self.registry = self.registry_patch.start()
        self.context, self.state = recovery.load_legacy_context(self.manifest_path)
        self.production_allowlist = recovery.LEGACY_SUBSTRATE_ALLOWLIST
        historical = recovery.historical_provenance(self.context)
        fixture_identity = recovery.legacy_substrate_identity(
            self.context, historical
        )
        self.allowlist_patch = mock.patch.object(
            recovery, "LEGACY_SUBSTRATE_ALLOWLIST", frozenset({fixture_identity})
        )
        self.allowlist_patch.start()
        for service, account in keychain_inventory(self.context):
            self.platform.keychain.add((service, account))
        for service in self.manifest["keychain_services"].values():
            self.platform.owner_values[service] = self.run_id

    def tearDown(self):
        self.allowlist_patch.stop()
        self.registry_patch.stop()
        self.snapshot_patch.stop()
        self.runtime_patch.stop()
        self.temporary.cleanup()

    def build_manifest(self):
        suffix = self.run_id.rsplit("-", 1)[1]
        return {
            "schema_version": 1,
            "run_id": self.run_id,
            "commit_sha": "a" * 40,
            "created_at": "2026-08-03T17:10:32Z",
            "public_origin": "https://d2-api.starring.co.kr",
            "cloudflare": {},
            "authoring": {},
            "candidates": {"historical": "source-pins-need-not-remain-current"},
            "source_trees": {"historical": "source-pins-need-not-remain-current"},
            "discord": {
                "guild_id": "1533137713476272288",
                "hub_channel_id": "1533137713476272289",
                "application_id": "1533144492293754900",
                "bot_user_id": "1533144492293754901",
                "actor_id": "1056857223529250906",
                "resource_prefix": "starring-d2-fd1232a7b31c",
                "disposable_guild_required": True,
            },
            "database": {
                "cluster_root": str(self.root / "postgres"),
                "socket_directory": str(self.root / "socket"),
                "name": "starring_runtime_staging",
                "port": 55435,
            },
            "services": {
                "api": {
                    "label": f"local.starring.d2.{suffix}.api",
                    "port": 28080,
                },
                "runtime": {
                    "label": f"local.starring.d2.{suffix}.runtime",
                    "port": 29093,
                },
                "transport": {
                    "label": f"local.starring.d2.{suffix}.transport",
                    "gateway_port": 29105,
                    "http_port": 29106,
                },
                "tunnel": {"label": f"local.starring.d2.{suffix}.tunnel"},
                "worker": {
                    "label": f"local.starring.d2.{suffix}.worker",
                    "port": 28183,
                },
            },
            "keychain_services": {
                "api": f"starring.d2.{suffix}.api",
                "runtime": f"starring.d2.{suffix}.runtime",
                "postgres": f"starring.d2.{suffix}.postgres",
                "worker": f"starring.d2.{suffix}.worker",
            },
            "external_keychain": {
                "discord_oauth_client_secret": {
                    "service": "starring.d2.credentials",
                    "account": "discord.oauth-client-secret",
                },
                "discord_bot_token": {
                    "service": "starring.d2.credentials",
                    "account": "discord.bot-token",
                },
                "tunnel_token": {
                    "service": "starring.d2.credentials",
                    "account": "cloudflare.tunnel-token",
                },
            },
            "protected_staging": {
                "database": "starring_runtime_staging@127.0.0.1:5432",
                "launchd_labels": [
                    "local.starring.api.staging",
                    "local.starring.codex-worker",
                    "local.starring.runtime.staging",
                    "local.cloudflared.starring",
                ],
                "mutation_allowed": False,
            },
            "expected_steps": list(range(1, 18)),
            "human_boundaries": [],
        }

    def write_manifest(self, manifest):
        payload = canonical_json(manifest)
        digest = hashlib.sha256(payload.encode("utf-8")).hexdigest()
        self.manifest_path.write_text(payload + "\n", encoding="utf-8")
        self.manifest_path.chmod(0o600)
        digest_path = self.run_directory / "manifest.sha256"
        digest_path.write_text(digest + "\n", encoding="ascii")
        digest_path.chmod(0o600)
        return digest

    def write_state(self, phase):
        state = {
            "schema_version": 1,
            "manifest_sha256": self.digest,
            "run_id": self.run_id,
            "phase": phase,
            "updated_at": "2026-08-03T17:14:45Z",
            "standing_snapshot": copy.deepcopy(self.snapshot),
        }
        path = self.artifact_directory / "state.json"
        path.write_text(canonical_json(state) + "\n", encoding="utf-8")
        path.chmod(0o600)

    def write_historical_provenance(self):
        entries = (
            ("prepare", "intent", "run"),
            ("root_create", "intent", "isolated_root"),
            ("root_create", "complete", "isolated_root"),
            ("initdb", "intent", "cluster"),
            ("initdb", "complete", "cluster"),
            ("postgres_configure", "intent", "cluster"),
            ("postgres_configure", "complete", "cluster"),
            ("database_bootstrap", "intent", "database"),
            ("database_bootstrap", "complete", "database"),
        )
        rows = []
        for sequence, (action, status, target) in enumerate(entries, 1):
            rows.append(
                canonical_json(
                    {
                        "schema_version": 1,
                        "sequence": sequence,
                        "recorded_at": "2026-08-03T17:10:43Z",
                        "manifest_sha256": self.digest,
                        "action": action,
                        "status": status,
                        "target": target,
                    }
                )
            )
        journal = self.artifact_directory / "lifecycle.jsonl"
        journal.write_text("\n".join(rows) + "\n", encoding="utf-8")
        journal.chmod(0o600)
        evidence = {
            "database_system_identifier": "7669853998318333589",
            "migration_count": 121,
            "migration_head": "202608030002",
            "migration_ledger_sha256": "a" * 64,
            "relation_count": 198,
            "capability_function_count": 137,
        }
        path = self.artifact_directory / "database-evidence.json"
        path.write_text(canonical_json(evidence) + "\n", encoding="utf-8")
        path.chmod(0o600)

    def recover(self):
        return recovery.command_recover(
            self.context,
            self.state,
            self.platform,
            self.run_id,
            self.digest,
        )

    def allowlist_context(self, identity):
        context = mock.Mock()
        context.manifest = {"run_id": identity[0]}
        context.root = pathlib.Path(identity[1])
        context.digest = identity[2]
        historical = {
            "database_system_identifier": identity[3],
            "journal_sha256": identity[4],
            "database_evidence_sha256": identity[5],
            "provenance_sha256": identity[6],
        }
        return context, historical

    def test_code_reviewed_allowlist_accepts_only_two_exact_known_identities(self):
        self.assertEqual(len(self.production_allowlist), 2)
        with mock.patch.object(
            recovery,
            "LEGACY_SUBSTRATE_ALLOWLIST",
            self.production_allowlist,
        ):
            for identity in self.production_allowlist:
                context, historical = self.allowlist_context(identity)
                self.assertEqual(
                    recovery.require_allowlisted_legacy_substrate(
                        context, historical
                    ),
                    identity,
                )

    def test_code_reviewed_allowlist_rejects_drift_of_every_bound_field(self):
        identity = sorted(self.production_allowlist)[0]
        replacements = (
            "d2-20260803t171033z-fd1232a7b31c",
            "/private/tmp/starring-d2-d2-20260803t171033z-fd1232a7b31c",
            "0" * 64,
            "7669853998318333590",
            "1" * 64,
            "2" * 64,
            "3" * 64,
        )
        with mock.patch.object(
            recovery,
            "LEGACY_SUBSTRATE_ALLOWLIST",
            self.production_allowlist,
        ):
            for index, replacement in enumerate(replacements):
                with self.subTest(field=index):
                    changed = list(identity)
                    changed[index] = replacement
                    context, historical = self.allowlist_context(tuple(changed))
                    with self.assertRaisesRegex(
                        OrchestratorError,
                        "legacy_substrate_identity_not_allowlisted",
                    ):
                        recovery.require_allowlisted_legacy_substrate(
                            context, historical
                        )

    def test_forged_internally_consistent_substrate_is_not_allowlisted(self):
        status = recovery.command_status(self.context, self.state, self.platform)
        self.assertTrue(status["runtime_root_present"])
        with mock.patch.object(
            recovery,
            "LEGACY_SUBSTRATE_ALLOWLIST",
            self.production_allowlist,
        ):
            with self.assertRaisesRegex(
                OrchestratorError, "legacy_substrate_identity_not_allowlisted"
            ):
                self.recover()
        self.assertTrue(self.root.exists())
        self.assertEqual(len(self.platform.keychain), 29)

    def test_historical_source_pins_do_not_block_status(self):
        result = recovery.command_status(self.context, self.state, self.platform)
        self.assertEqual(result["phase"], "candidate_started")
        self.assertEqual(result["keychain_items_present"], 29)
        self.assertTrue(result["runtime_root_present"])
        self.assertFalse(result["postgres_running"])
        self.assertEqual(result["loaded_services"], [])

    def test_recovery_removes_only_owned_inert_substrate_and_is_idempotent(self):
        result = self.recover()
        self.assertEqual(result["status"], "recovered")
        self.assertFalse(self.root.exists())
        self.assertEqual(self.platform.keychain, set())
        for service in self.manifest["keychain_services"].values():
            deleted = [
                account
                for item_service, account in self.platform.deleted
                if item_service == service
            ]
            self.assertEqual(deleted[-1], "lifecycle-owner")
        state = json.loads((self.artifact_directory / "state.json").read_text())
        self.assertEqual(state["phase"], "cleaned")
        evidence = json.loads(
            (self.artifact_directory / "legacy-substrate-recovery.json").read_text()
        )
        self.assertEqual(evidence["kind"], recovery.LEGACY_RECOVERY_KIND)
        self.assertTrue(evidence["isolated_root_absent"])
        context, cleaned = recovery.load_legacy_context(self.manifest_path)
        again = recovery.command_recover(
            context, cleaned, self.platform, self.run_id, self.digest
        )
        self.assertEqual(again["status"], "already_recovered")

    def test_recovery_requires_exact_run_and_digest_confirmation(self):
        with self.assertRaisesRegex(
            OrchestratorError, "legacy_substrate_run_confirmation_mismatch"
        ):
            recovery.command_recover(
                self.context, self.state, self.platform, "wrong", self.digest
            )
        with self.assertRaisesRegex(
            OrchestratorError, "legacy_substrate_digest_confirmation_mismatch"
        ):
            recovery.command_recover(
                self.context, self.state, self.platform, self.run_id, "0" * 64
            )
        self.assertTrue(self.root.exists())
        self.assertEqual(len(self.platform.keychain), 29)

    def test_recovery_rejects_active_processes_before_mutation(self):
        self.platform.postgres = True
        with self.assertRaisesRegex(
            OrchestratorError, "legacy_substrate_postgres_active"
        ):
            self.recover()
        self.platform.postgres = False
        self.platform.loaded.add(self.manifest["services"]["runtime"]["label"])
        with self.assertRaisesRegex(
            OrchestratorError, "legacy_substrate_launchd_active"
        ):
            self.recover()
        self.assertEqual(len(self.platform.keychain), 29)

    def test_recovery_rejects_keychain_without_matching_owner(self):
        service = self.manifest["keychain_services"]["api"]
        self.platform.owner_values[service] = "another-run"
        with self.assertRaisesRegex(
            OrchestratorError, "legacy_substrate_keychain_ownership_invalid"
        ):
            self.recover()
        self.assertEqual(len(self.platform.keychain), 29)

    def test_recovery_rejects_registry_collision(self):
        self.registry.return_value = {
            "schema_version": 1,
            "kind": "test",
            "owners": [
                {
                    "run_id": self.run_id,
                    "manifest_sha256": "f" * 64,
                    "manifest_path": "/tmp/other",
                    "guild_id": "1533137713476272280",
                    "application_id": "1533144492293754909",
                    "bot_user_id": "1533144492293754908",
                }
            ],
        }
        with self.assertRaisesRegex(
            OrchestratorError, "legacy_substrate_registry_conflict"
        ):
            self.recover()

    def test_recovery_rejects_protected_staging_drift(self):
        changed = copy.deepcopy(self.snapshot)
        changed["port_occupied"]["5432"] = False
        recovery.standing_snapshot.return_value = changed
        with self.assertRaisesRegex(
            OrchestratorError, "legacy_substrate_protected_staging_drift"
        ):
            self.recover()
        self.assertTrue(self.root.exists())

    def test_status_allows_insufficient_provenance_but_recovery_fails_closed(self):
        result = recovery.command_status(self.context, self.state, self.platform)
        self.assertTrue(result["runtime_root_present"])
        (self.artifact_directory / "database-evidence.json").unlink()
        with self.assertRaisesRegex(
            OrchestratorError, "legacy_substrate_database_evidence_invalid"
        ):
            self.recover()
        self.assertTrue(self.root.exists())
        self.assertEqual(len(self.platform.keychain), 29)

    def test_recovery_rejects_forged_historical_journal(self):
        journal = self.artifact_directory / "lifecycle.jsonl"
        rows = journal.read_text(encoding="utf-8").splitlines()
        forged = json.loads(rows[2])
        forged["action"] = "unrelated_create"
        rows[2] = canonical_json(forged)
        journal.write_text("\n".join(rows) + "\n", encoding="utf-8")
        journal.chmod(0o600)
        with self.assertRaisesRegex(
            OrchestratorError, "legacy_substrate_provenance_invalid"
        ):
            self.recover()
        self.assertTrue(self.root.exists())

    def test_recovery_rejects_cluster_identity_mismatch(self):
        self.platform.cluster_identity = "7669853998318333590"
        with self.assertRaisesRegex(
            OrchestratorError, "legacy_substrate_cluster_identity_mismatch"
        ):
            self.recover()
        self.assertTrue(self.root.exists())

    def test_status_rejects_runtime_root_on_different_parent_device(self):
        original = recovery.path_metadata

        def different_device(path):
            metadata = original(path)
            if pathlib.Path(path) != self.root or metadata is None:
                return metadata
            changed = mock.Mock()
            changed.st_mode = metadata.st_mode
            changed.st_uid = metadata.st_uid
            changed.st_dev = metadata.st_dev + 1
            changed.st_ino = metadata.st_ino
            return changed

        with mock.patch.object(
            recovery, "path_metadata", side_effect=different_device
        ):
            with self.assertRaisesRegex(
                OrchestratorError, "legacy_substrate_root_invalid"
            ):
                recovery.command_status(self.context, self.state, self.platform)

    def test_recovery_detects_root_inode_swap_after_quarantine_rename(self):
        original_rename = recovery.rename_exclusive
        preserved = self.base / "preserved-root"

        def swap_then_rename(
            source_directory,
            source_name,
            destination_directory,
            destination_name,
        ):
            self.root.rename(preserved)
            self.root.mkdir(mode=0o700)
            (self.root / "postgres").mkdir(mode=0o700)
            return original_rename(
                source_directory,
                source_name,
                destination_directory,
                destination_name,
            )

        with mock.patch.object(
            recovery, "rename_exclusive", side_effect=swap_then_rename
        ):
            with self.assertRaisesRegex(
                OrchestratorError, "legacy_substrate_root_swap_detected"
            ):
                self.recover()
        self.assertTrue(preserved.exists())
        self.assertTrue(recovery.quarantine_path(self.context).exists())

    def test_recovery_resumes_crash_after_quarantine_rename(self):
        provenance = recovery.require_recovery_provenance(
            self.context, self.platform
        )
        progress = recovery.ensure_recovery_progress(self.context, provenance)
        self.assertEqual(progress["phase"], "planned")
        self.root.rename(recovery.quarantine_path(self.context))
        result = self.recover()
        self.assertEqual(result["status"], "recovered")
        self.assertFalse(recovery.quarantine_path(self.context).exists())
        saved = recovery.load_recovery_progress(self.context)
        self.assertEqual(saved["phase"], "deleted")

    def test_recovery_exclusive_rename_rejects_destination_race(self):
        real_rename = recovery.rename_exclusive
        quarantine = recovery.quarantine_path(self.context)

        def race_destination(
            source_directory,
            source_name,
            destination_directory,
            destination_name,
        ):
            quarantine.mkdir(mode=0o700)
            return real_rename(
                source_directory,
                source_name,
                destination_directory,
                destination_name,
            )

        with mock.patch.object(
            recovery, "rename_exclusive", side_effect=race_destination
        ):
            with self.assertRaisesRegex(
                OrchestratorError, "legacy_substrate_root_swap_detected"
            ):
                self.recover()
        self.assertTrue(self.root.exists())
        self.assertTrue(quarantine.is_dir())

    def test_recovery_rejects_planned_progress_after_external_root_loss(self):
        provenance = recovery.require_recovery_provenance(
            self.context, self.platform
        )
        progress = recovery.ensure_recovery_progress(self.context, provenance)
        self.assertEqual(progress["phase"], "planned")
        preserved = self.base / "externally-preserved-root"
        self.root.rename(preserved)
        try:
            with self.assertRaisesRegex(
                OrchestratorError, "legacy_substrate_root_loss_unproven"
            ):
                recovery.recover_runtime_root(
                    self.context, provenance, self.platform
                )
            self.assertEqual(
                recovery.load_recovery_progress(self.context)["phase"],
                "planned",
            )
        finally:
            preserved.rename(self.root)

    def test_recovery_does_not_certify_loss_before_quarantine_recheck(self):
        provenance = recovery.require_recovery_provenance(
            self.context, self.platform
        )
        progress = recovery.ensure_recovery_progress(self.context, provenance)
        quarantine = recovery.quarantine_path(self.context)
        self.root.rename(quarantine)
        self.platform.postgres_error = "injected_pre_recheck_crash"
        with self.assertRaisesRegex(
            OrchestratorError, "injected_pre_recheck_crash"
        ):
            recovery.recover_runtime_root(
                self.context, provenance, self.platform
            )
        self.assertEqual(
            recovery.load_recovery_progress(self.context)["phase"], "planned"
        )
        self.platform.postgres_error = None
        preserved = self.base / "externally-moved-quarantine"
        quarantine.rename(preserved)
        try:
            with self.assertRaisesRegex(
                OrchestratorError, "legacy_substrate_root_loss_unproven"
            ):
                recovery.recover_runtime_root(
                    self.context, provenance, self.platform
                )
        finally:
            preserved.rename(quarantine)
        result = recovery.recover_runtime_root(
            self.context, provenance, self.platform
        )
        self.assertEqual(result["phase"], "deleted")

    def test_recovery_fails_closed_on_launchd_and_postgres_observation_errors(self):
        self.platform.launchd_error = "launchd_observation_failed"
        with self.assertRaisesRegex(OrchestratorError, "launchd_observation_failed"):
            self.recover()
        self.platform.launchd_error = None
        self.platform.postgres_error = "postgres_observation_failed"
        with self.assertRaisesRegex(OrchestratorError, "postgres_observation_failed"):
            self.recover()
        self.assertTrue(self.root.exists())
        self.assertEqual(len(self.platform.keychain), 29)

    def test_recovery_revalidates_keychain_identity_before_delete(self):
        service = self.manifest["keychain_services"]["api"]
        target = (service, "database.apply-executor")
        self.platform.replace_identity = target
        self.platform.replace_identity_at_read = 4
        with self.assertRaisesRegex(
            OrchestratorError, "legacy_substrate_keychain_identity_drift"
        ):
            self.recover()
        self.assertIn(target, self.platform.keychain)
        self.assertTrue(self.root.exists())

    def test_recovery_revalidates_keychain_owner_before_each_delete(self):
        service = self.manifest["keychain_services"]["api"]
        self.platform.owner_drift_service = service
        self.platform.owner_drift_at_read = 4
        with self.assertRaisesRegex(
            OrchestratorError, "legacy_substrate_keychain_ownership_invalid"
        ):
            self.recover()
        self.assertEqual(self.platform.deleted, [])
        self.assertTrue(self.root.exists())

    def test_recovery_does_not_delete_replacement_at_exact_delete_boundary(self):
        service = self.manifest["keychain_services"]["api"]
        target = (service, "database.apply-executor")
        self.platform.replace_at_delete = target
        with self.assertRaisesRegex(
            OrchestratorError, "legacy_substrate_keychain_delete_unconfirmed"
        ):
            self.recover()
        self.assertIn(target, self.platform.keychain)
        self.assertNotIn(target, self.platform.deleted)

    def test_recovery_replay_preserves_replacement_after_delete_crash(self):
        service = self.manifest["keychain_services"]["api"]
        target = (service, "database.apply-executor")
        self.platform.crash_after_delete = target
        with self.assertRaisesRegex(
            OrchestratorError, "injected_keychain_delete_crash"
        ):
            self.recover()
        self.assertNotIn(target, self.platform.keychain)
        self.platform.crash_after_delete = None
        self.platform.keychain.add(target)
        self.platform.identity_versions[target] = 1
        with self.assertRaisesRegex(
            OrchestratorError, "legacy_substrate_keychain_identity_drift"
        ):
            self.recover()
        self.assertIn(target, self.platform.keychain)
        self.assertTrue(self.root.exists())

    def test_recovery_records_failure_and_resumes_partial_keychain_cleanup(self):
        service = self.manifest["keychain_services"]["api"]
        failing = (service, "database.authorized-snapshot-reader")
        self.platform.delete_failure = failing
        with self.assertRaisesRegex(OrchestratorError, "keychain_delete_failed"):
            self.recover()
        rows = [
            json.loads(line)
            for line in (self.artifact_directory / "lifecycle.jsonl")
            .read_text()
            .splitlines()
        ]
        self.assertEqual(rows[-1]["status"], "failed")
        self.platform.delete_failure = None
        result = self.recover()
        self.assertEqual(result["status"], "recovered")

    def test_cleaned_replay_requires_durable_evidence(self):
        self.recover()
        evidence = self.artifact_directory / "legacy-substrate-recovery.json"
        evidence.unlink()
        context, cleaned = recovery.load_legacy_context(self.manifest_path)
        with self.assertRaisesRegex(
            OrchestratorError, "legacy_substrate_recovery_evidence_invalid"
        ):
            recovery.command_recover(
                context, cleaned, self.platform, self.run_id, self.digest
            )

    def test_cleaned_replay_rejects_corrupt_durable_evidence(self):
        self.recover()
        path = self.artifact_directory / "legacy-substrate-recovery.json"
        evidence = json.loads(path.read_text(encoding="utf-8"))
        evidence["quarantine_absent"] = False
        path.write_text(canonical_json(evidence) + "\n", encoding="utf-8")
        path.chmod(0o600)
        context, cleaned = recovery.load_legacy_context(self.manifest_path)
        with self.assertRaisesRegex(
            OrchestratorError, "legacy_substrate_recovery_evidence_invalid"
        ):
            recovery.command_recover(
                context, cleaned, self.platform, self.run_id, self.digest
            )

    def test_cleaned_replay_repairs_incomplete_completion_journal(self):
        self.recover()
        journal = self.artifact_directory / "lifecycle.jsonl"
        rows = journal.read_text(encoding="utf-8").splitlines()
        self.assertEqual(json.loads(rows[-1])["status"], "complete")
        journal.write_text("\n".join(rows[:-1]) + "\n", encoding="utf-8")
        journal.chmod(0o600)
        context, cleaned = recovery.load_legacy_context(self.manifest_path)
        result = recovery.command_recover(
            context, cleaned, self.platform, self.run_id, self.digest
        )
        self.assertEqual(result["status"], "already_recovered")
        repaired = [
            json.loads(line)
            for line in journal.read_text(encoding="utf-8").splitlines()
        ]
        self.assertEqual(repaired[-1]["action"], "legacy_substrate_recovery")
        self.assertEqual(repaired[-1]["status"], "complete")
        self.assertEqual(
            result["keychain_identity_boundary"],
            "persistent-reference-baseline-reobserved-before-each-delete",
        )

    def test_cleaned_replay_reconstructs_deleted_progress_from_trusted_evidence(self):
        self.recover()
        progress = recovery.load_recovery_progress(self.context)
        recovery.save_recovery_progress(self.context, progress, "quarantined")
        context, cleaned = recovery.load_legacy_context(self.manifest_path)
        result = recovery.command_recover(
            context, cleaned, self.platform, self.run_id, self.digest
        )
        self.assertEqual(result["status"], "already_recovered")
        repaired = recovery.load_recovery_progress(context)
        self.assertEqual(repaired["phase"], "deleted")

    def test_loader_rejects_digest_label_root_and_state_tampering(self):
        cases = []
        digest_manifest = copy.deepcopy(self.manifest)
        digest_manifest["commit_sha"] = "b" * 40
        cases.append(("digest", digest_manifest, False))
        label_manifest = copy.deepcopy(self.manifest)
        label_manifest["services"]["api"]["label"] = "local.starring.api.staging"
        cases.append(("label", label_manifest, True))
        root_manifest = copy.deepcopy(self.manifest)
        root_manifest["database"]["cluster_root"] = "/private/tmp/other"
        cases.append(("root", root_manifest, True))
        for name, manifest, rewrite_digest in cases:
            with self.subTest(name=name):
                self.write_manifest(self.manifest)
                self.write_state("candidate_started")
                payload = canonical_json(manifest)
                self.manifest_path.write_text(payload + "\n", encoding="utf-8")
                self.manifest_path.chmod(0o600)
                if rewrite_digest:
                    digest = hashlib.sha256(payload.encode("utf-8")).hexdigest()
                    path = self.run_directory / "manifest.sha256"
                    path.write_text(digest + "\n", encoding="ascii")
                    path.chmod(0o600)
                    state = json.loads(
                        (self.artifact_directory / "state.json").read_text()
                    )
                    state["manifest_sha256"] = digest
                    state_path = self.artifact_directory / "state.json"
                    state_path.write_text(canonical_json(state) + "\n")
                    state_path.chmod(0o600)
                with self.assertRaises(OrchestratorError):
                    recovery.load_legacy_context(self.manifest_path)

    def test_loader_rejects_symlinked_runtime_root(self):
        target = self.base / "outside"
        target.mkdir()
        for child in sorted(self.root.rglob("*"), reverse=True):
            if child.is_dir():
                child.rmdir()
            else:
                child.unlink()
        self.root.rmdir()
        self.root.symlink_to(target, target_is_directory=True)
        with self.assertRaisesRegex(
            OrchestratorError, "legacy_substrate_root_invalid"
        ):
            recovery.command_status(self.context, self.state, self.platform)


if __name__ == "__main__":
    unittest.main()
