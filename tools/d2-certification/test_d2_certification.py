#!/usr/bin/env python3

import contextlib
import copy
import importlib.util
import io
import json
import os
import pathlib
import tempfile
import unittest
from unittest import mock


MODULE_PATH = pathlib.Path(__file__).with_name("d2_certification.py")
SPEC = importlib.util.spec_from_file_location("d2_certification", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
RUN_MODULE_PATH = pathlib.Path(__file__).with_name("d2_run.py")
RUN_SPEC = importlib.util.spec_from_file_location("d2_run_tests", RUN_MODULE_PATH)
RUN_MODULE = importlib.util.module_from_spec(RUN_SPEC)
RUN_SPEC.loader.exec_module(RUN_MODULE)


COMMIT = "a" * 40
DIGEST = "b" * 64
RUN_ID = "d2-20260801t120000z-0123456789ab"
TRANSPORT_INSTANCE_ID = "d2ti-0123456789abcdef0123456789abcdef"
ATTESTATION_ID = "c" * 64
PROCESS_INSTANCE_OLD = "11111111111111111111111111111111"
PROCESS_INSTANCE_NEW = "0123456789abcdef0123456789abcdef"


def route_identity(deployment_id, process_instance_id, generation, fence, incarnation):
    return {
        "deployment_id": deployment_id,
        "runtime_generation": generation,
        "route_controller_fencing_token": fence,
        "route_incarnation": incarnation,
        "origin_process_instance_id": process_instance_id,
        "origin_serving_lease_epoch": generation,
        "origin_serving_revision": generation,
        "origin_gateway_shard_id": "shard-0",
        "origin_gateway_owner_lease_epoch": generation,
        "origin_gateway_owner_revision": generation,
    }


def serving_identity(deployment_id, process_instance_id, generation, lease_epoch):
    return {
        "guild_id": "1524810437118525551",
        "ruleset_key": "studyroom",
        "tenant_id": "tenant-1",
        "installation_id": "installation-1",
        "deployment_id": deployment_id,
        "attestation_id": ATTESTATION_ID,
        "process_instance_id": process_instance_id,
        "runtime_generation": generation,
        "target_version": generation,
        "target_content_hash": "d" * 64,
        "binding_revision": generation,
        "binding_fingerprint": "e" * 64,
        "lease_epoch": lease_epoch,
        "revision": generation,
    }


ROUTE_ID_INITIAL = MODULE.canonical_route_identity_sha256(
    route_identity("deployment-1", PROCESS_INSTANCE_OLD, 1, 1, 1)
)
ROUTE_ID_RECONSTRUCTED = MODULE.canonical_route_identity_sha256(
    route_identity("deployment-1", PROCESS_INSTANCE_NEW, 1, 2, 2)
)
ROUTE_ID_REPLACEMENT = MODULE.canonical_route_identity_sha256(
    route_identity("deployment-2", PROCESS_INSTANCE_NEW, 2, 3, 3)
)
SERVING_ID_INITIAL = MODULE.canonical_serving_identity_sha256(
    serving_identity("deployment-1", PROCESS_INSTANCE_OLD, 1, 1)
)
SERVING_ID_RECONSTRUCTED = MODULE.canonical_serving_identity_sha256(
    serving_identity("deployment-1", PROCESS_INSTANCE_NEW, 1, 2)
)
JOIN_EFFECT_ID = MODULE.canonical_effect_identity_sha256(
    {
        "application_id": "1524810437118525552",
        "interaction_id": "1532677575736819846",
        "action_index": 0,
    }
)
INDETERMINATE_EFFECT_ID = MODULE.canonical_effect_identity_sha256(
    {
        "application_id": "1524810437118525552",
        "interaction_id": "1532677575736819850",
        "action_index": 0,
    }
)


def complete_evidence(manifest):
    prefix = manifest["discord"]["resource_prefix"]
    installation_id = f"installation:{prefix}"
    return {
        1: {
            "database_system_identifier": "7667905772642692043",
            "migration_count": 125,
            "migration_head": "202608040004",
            "migration_ledger_sha256": DIGEST,
            "discord_resource_prefix": prefix,
        },
        2: {"prior_runtime_owner_count": 0, "prior_smoke_process_count": 0},
        3: {
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
            "api_build_revision": COMMIT,
            "runtime_build_revision": COMMIT,
            "api_ready_status": 200,
            "runtime_ready_status": 200,
            "worker_ready_status": 200,
            "cloudflare_tunnel_id": manifest["cloudflare"]["tunnel_id"],
            "public_origin": manifest["cloudflare"]["public_origin"],
            "origin_service": manifest["cloudflare"]["origin_service"],
            "transport_instance_id": TRANSPORT_INSTANCE_ID,
            "transport_ready": True,
            "tunnel_ready": True,
        },
        4: {
            "me_status": 200,
            "principal_id": "discord:1056857223529250906",
            "installation_id": installation_id,
            "guild_id": "1524810437118525551",
            "authority_check_status": 204,
            "public_origin": manifest["cloudflare"]["public_origin"],
        },
        5: {
            "authoring_http_status": 200,
            "authoring_session_id": "authoring-session-1",
            "authoring_generation": 1,
            "installation_id": installation_id,
            "model": "gpt-5.6-luna",
            "provider": "codex_chatgpt",
            "reasoning_effort": "medium",
            "auth_mode": "chatgpt",
            "browser_observed_at": "2026-08-04T01:02:03Z",
            "worker_before_observed_at": "2026-08-04T01:02:02Z",
            "worker_after_observed_at": "2026-08-04T01:02:04Z",
            "one_shot": True,
            "public_origin": manifest["cloudflare"]["public_origin"],
        },
        6: {
            "generation_encrypted": True,
            "projection_state": "preview_ready",
            "generation": 1,
            "payload_digest": DIGEST,
            "installation_id": installation_id,
            "authoring_session_id": "authoring-session-1",
            "generation_created_at": "2026-08-04T01:02:03Z",
            "public_origin": manifest["cloudflare"]["public_origin"],
            "preview_observed_at": "2026-08-04T01:02:03Z",
        },
        7: {
            "installation_id": installation_id,
            "promotion_id": DIGEST,
            "authoring_session_id": "authoring-session-1",
            "authoring_generation": 1,
            "target_content_hash": DIGEST,
            "payload_digest": DIGEST,
            "preview_state": "pending_approval",
            "approval_state": "approved",
            "apply_state": "runtime_pending",
            "public_origin": manifest["cloudflare"]["public_origin"],
            "decision_observed_at": "2099-08-04T01:02:03Z",
        },
        8: {
            "pending_observed": True,
            "live_observed": True,
            "installation_id": installation_id,
            "promotion_id": DIGEST,
            "deployment_id": "deployment-1",
            "route_id": ROUTE_ID_INITIAL,
            "attestation_id": ATTESTATION_ID,
            "serving_lease_id": SERVING_ID_INITIAL,
            "deployment_revision": 11,
            "convergence_attempt": 1,
            "process_instance_id": PROCESS_INSTANCE_OLD,
            "public_observed_at": "2099-08-04T01:02:10Z",
            "database_observed_at": "2099-08-04T01:02:15Z",
            "public_last_heartbeat_at": "2099-08-04T01:02:04Z",
            "database_last_heartbeat_at": "2099-08-04T01:02:12Z",
            "public_lease_expires_at": "2099-08-04T01:02:49Z",
            "database_lease_expires_at": "2099-08-04T01:02:57Z",
            "public_origin": manifest["cloudflare"]["public_origin"],
        },
        9: {
            "create_interaction_id": "1532677575736819845",
            "join_interaction_id": "1532677575736819846",
            "actor_user_id": manifest["discord"]["actor_id"],
            "joined_role_id": "1532677575736819847",
            "deployment_id": "deployment-1",
            "route_id": ROUTE_ID_INITIAL,
            "instance_id": "instance-1",
            "role_ids": ["1532677575736819847"],
            "channel_ids": ["1532677575736819848"],
            "panel_message_ids": ["1532677575736819849"],
            "ephemeral_count": 2,
            "inventory_digest_sha256": DIGEST,
            "transport_instance_id": TRANSPORT_INSTANCE_ID,
        },
        10: {
            "interaction_id": "1532677575736819846",
            "effect_id": JOIN_EFFECT_ID,
            "delivery_count": 2,
            "external_effect_count": 1,
            "receipt_state": "completed",
            "transport_duplicate_injections": 1,
            "transport_duplicate_delivery_count": 2,
            "transport_last_duplicate_interaction_id": "1532677575736819846",
            "role_ids": ["1532677575736819847"],
            "channel_ids": ["1532677575736819848"],
            "panel_message_ids": ["1532677575736819849"],
            "inventory_digest_sha256": DIGEST,
            "transport_instance_id": TRANSPORT_INSTANCE_ID,
        },
        11: {
            "old_pid": 100,
            "new_pid": 101,
            "runtime_sha256": manifest["candidates"]["runtime"]["sha256"],
            "ready_after_restart": True,
            "process_identity_joined": True,
            "process_instance_id": PROCESS_INSTANCE_NEW,
            "checkpoint": "live_fresh_lease",
            "deployment_id": "deployment-1",
            "route_id": ROUTE_ID_INITIAL,
            "instance_id": "instance-1",
            "canonical_confirmation_sha256": DIGEST,
            "operation_id": (
                f"d2:{MODULE.manifest_digest(manifest)[:16]}:"
                "certify-live-runtime-restart"
            ),
            "shutdown_boundary": "2026-08-03T01:00:00Z",
            "installation_id": installation_id,
            "promotion_id": DIGEST,
            "attestation_revision": 11,
            "public_origin": manifest["cloudflare"]["public_origin"],
        },
        12: {
            "route_reconstructed": True,
            "instance_reconstructed": True,
            "deployment_id": "deployment-1",
            "source_route_id": ROUTE_ID_INITIAL,
            "reconstructed_route_id": ROUTE_ID_RECONSTRUCTED,
            "source_serving_lease_id": SERVING_ID_INITIAL,
            "reconstructed_serving_lease_id": SERVING_ID_RECONSTRUCTED,
            "instance_id": "instance-1",
            "pinned_ruleset_digest": DIGEST,
            "probe_interaction_id": "1532677575736819851",
            "process_instance_id": PROCESS_INSTANCE_NEW,
        },
        13: {
            "effect_id": INDETERMINATE_EFFECT_ID,
            "interaction_id": "1532677575736819850",
            "route_id": ROUTE_ID_RECONSTRUCTED,
            "injected_outcome": "indeterminate",
            "reconciliation_state": "known_success",
            "duplicate_external_effect_count": 0,
            "unsafe_deletion_count": 0,
            "output_role_id": "1532677575736819852",
            "reconciliation_inventory_digest_sha256": DIGEST,
            "transport_indeterminate_injections": 1,
            "transport_last_audit_reason_sha256": DIGEST,
            "transport_last_upstream_status": 201,
            "transport_instance_id": TRANSPORT_INSTANCE_ID,
        },
        14: {
            "installation_id": installation_id,
            "source_promotion_id": DIGEST,
            "replacement_promotion_id": "a" * 64,
            "replacement_target_id": "deployment-2",
            "replacement_kind": "rollback",
            "source_deployment_id": "deployment-1",
            "source_route_id": ROUTE_ID_RECONSTRUCTED,
            "replacement_deployment_id": "deployment-2",
            "replacement_route_id": ROUTE_ID_REPLACEMENT,
            "previous_target_drained": True,
            "replacement_live": True,
            "prior_route_absent": True,
            "public_origin": manifest["cloudflare"]["public_origin"],
        },
        15: {
            "installation_id": installation_id,
            "promotion_id": "a" * 64,
            "gateway_disconnected": True,
            "live_lost": True,
            "deployment_http_status": 200,
            "operational_http_status": 200,
            "product_state": "pending",
            "operational_state": "pending",
            "runtime_phase": "live",
            "serving_state": "disconnected",
            "runtime_ready_status": 503,
            "public_code": "runtime_gateway_disconnected",
            "retryable": True,
            "route_id": ROUTE_ID_REPLACEMENT,
            "transport_gateway_partitioned": True,
            "transport_gateway_partition_events": 1,
            "transport_instance_id": TRANSPORT_INSTANCE_ID,
            "public_origin": manifest["cloudflare"]["public_origin"],
        },
        16: {
            "teardown_started": True,
            "discord_resource_ids_deleted": [
                "1532677575736819847",
                "1532677575736819848",
                "1532677575736819849",
                "1532677575736819852",
            ],
            "database_drop_requested": True,
            "services_stopped": True,
            "precleanup_sha256": DIGEST,
            "discord_teardown_sha256": DIGEST,
        },
        17: {
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
    }


class D2CertificationTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name).resolve()
        self.artifact_root = self.root / "immutable-candidates"
        self.artifact_root.mkdir()
        self.candidates = {}
        for name in MODULE.REQUIRED_CANDIDATES:
            path = (
                self.artifact_root / "worker-tree" / "worker.mjs"
                if name == "codex_worker"
                else self.artifact_root / name
            )
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(f"candidate:{name}".encode())
            self.candidates[name] = path
        for name in MODULE.CODEX_WORKER_SOURCE_FILES:
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

    def tearDown(self):
        for path in self.artifact_root.rglob("*"):
            if path.is_dir():
                path.chmod(0o700)
        self.artifact_root.chmod(0o700)
        self.temporary.cleanup()

    def prepare(self):
        arguments = [
            "prepare",
            "--output-root",
            str(self.root / "evidence"),
            "--commit",
            COMMIT,
            "--discord-guild-id",
            "1524810437118525551",
            "--discord-hub-channel-id",
            "1524810437118525554",
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
            MODULE.D2_CLOUDFLARE_TUNNEL_ID,
            "--public-origin",
            "https://d2-api.starring.co.kr",
            "--run-id",
            RUN_ID,
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
            self.assertEqual(MODULE.main(arguments), 0)
        result = json.loads(output.getvalue())
        return pathlib.Path(result["manifest"])

    def write_evidence(self, step, evidence):
        path = self.root / f"step-{step}.json"
        path.write_text(json.dumps(evidence), encoding="utf-8")
        path.chmod(0o600)
        return path

    def record(self, manifest_path, step, evidence):
        status, _ = self.record_result(manifest_path, step, evidence)
        return status

    def record_result(self, manifest_path, step, evidence):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            status = MODULE.main(
                [
                    "record",
                    "--manifest",
                    str(manifest_path),
                    "--step",
                    str(step),
                    "--evidence",
                    str(self.write_evidence(step, evidence)),
                ]
            )
        result = json.loads(output.getvalue()) if output.getvalue() else None
        return status, result

    def test_prepare_derives_isolated_names_and_hashes_without_mutating_staging(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        self.assertEqual(manifest["run_id"], RUN_ID)
        self.assertEqual(manifest["protected_staging"]["mutation_allowed"], False)
        self.assertEqual(
            manifest["cloudflare"],
            {
                "tunnel_id": MODULE.D2_CLOUDFLARE_TUNNEL_ID,
                "public_origin": MODULE.D2_PUBLIC_ORIGIN,
                "origin_service": MODULE.D2_ORIGIN_SERVICE,
            },
        )
        self.assertTrue(manifest["database"]["cluster_root"].startswith("/private/tmp/starring-d2-"))
        self.assertEqual(
            {
                manifest["services"]["transport"]["gateway_port"],
                manifest["services"]["transport"]["http_port"],
            },
            {29101, 29102},
        )
        self.assertEqual(
            manifest["candidates"]["runtime"]["sha256"],
            MODULE.sha256_file(self.candidates["runtime"]),
        )
        self.assertEqual(manifest["discord"]["actor_id"], "1056857223529250906")
        self.assertEqual(
            manifest["discord"]["hub_channel_id"], "1524810437118525554"
        )
        self.assertEqual(
            manifest["source_trees"]["certification_transport"]["files"],
            list(MODULE.CERTIFICATION_TRANSPORT_SOURCE_FILES),
        )
        self.assertEqual(oct(manifest_path.stat().st_mode & 0o777), "0o600")
        self.assertEqual(oct(manifest_path.parent.stat().st_mode & 0o777), "0o700")

    def test_transport_inventory_prunes_the_build_target(self):
        root = self.root / "transport-inventory"
        for name in MODULE.CERTIFICATION_TRANSPORT_SOURCE_FILES:
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(name, encoding="utf-8")
        (root / "target").symlink_to(root / "missing-build-output")
        MODULE.validate_certification_transport_inventory(root)
        extra = root / "src" / "unexpected.rs"
        extra.write_text("unexpected", encoding="utf-8")
        with self.assertRaisesRegex(
            MODULE.CertificationError, "certification_transport_inventory_invalid"
        ):
            MODULE.validate_certification_transport_inventory(root)

    def test_prepare_fsyncs_parent_then_complete_run_directory(self):
        observed = []
        real_fsync_directory = MODULE.fsync_directory

        def record(path, label):
            observed.append((pathlib.Path(path), label))
            real_fsync_directory(path, label)

        with mock.patch.object(MODULE, "fsync_directory", side_effect=record):
            manifest_path = self.prepare()
        self.assertEqual(
            observed,
            [
                (self.root, "output_root_parent"),
                (self.root / "evidence", "output_root"),
                (manifest_path.parent, "run_directory"),
            ],
        )
        self.assertEqual(
            {path.name for path in manifest_path.parent.iterdir()},
            {"manifest.json", "manifest.sha256", "receipts.jsonl"},
        )

    def test_directory_fsync_uses_directory_and_nofollow_flags(self):
        path = self.root / "durable"
        path.mkdir(mode=0o700)
        real_open = os.open
        with mock.patch.object(MODULE.os, "open", wraps=real_open) as opened:
            MODULE.fsync_directory(path, "durable")
        flags = opened.call_args.args[1]
        if hasattr(os, "O_DIRECTORY"):
            self.assertNotEqual(flags & os.O_DIRECTORY, 0)
        if hasattr(os, "O_NOFOLLOW"):
            self.assertNotEqual(flags & os.O_NOFOLLOW, 0)

    def test_cloudflare_route_binding_rejects_wrong_identity_origin_and_api_port(self):
        for tunnel_id, origin, api_port in (
            ("57c22e8a-0ec2-4f67-a882-2c355b0348de", MODULE.D2_PUBLIC_ORIGIN, 28080),
            (MODULE.D2_CLOUDFLARE_TUNNEL_ID, "https://other.starring.co.kr", 28080),
            (MODULE.D2_CLOUDFLARE_TUNNEL_ID, MODULE.D2_PUBLIC_ORIGIN, 28081),
        ):
            with self.subTest(tunnel_id=tunnel_id, origin=origin, api_port=api_port):
                with self.assertRaisesRegex(
                    MODULE.CertificationError, "cloudflare_route_binding_invalid"
                ):
                    MODULE.validate_cloudflare_route_binding(
                        MODULE.validate_cloudflare_tunnel_id(tunnel_id),
                        MODULE.validate_public_origin(origin),
                        api_port,
                    )

    def test_cloudflare_tunnel_id_requires_canonical_v4_uuid(self):
        for value in (
            "57C22E8A-0EC2-4F67-A882-2C355B0348DF",
            "57c22e8a0ec24f67a8822c355b0348df",
            "57c22e8a-0ec2-3f67-a882-2c355b0348df",
            "not-a-uuid",
        ):
            with self.subTest(value=value):
                with self.assertRaisesRegex(
                    MODULE.CertificationError, "cloudflare_tunnel_id_invalid"
                ):
                    MODULE.validate_cloudflare_tunnel_id(value)

    def test_complete_legacy_receipt_chain_is_not_release_authority(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        for step, evidence in complete_evidence(manifest).items():
            self.assertEqual(self.record(manifest_path, step, evidence), 0)
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            self.assertEqual(
                MODULE.main(["verify", "--manifest", str(manifest_path)]), 1
            )
        self.assertIn(
            "authoritative_verification_requires_d2_run", stderr.getvalue()
        )
        with self.assertRaisesRegex(
            RUN_MODULE.CertificationError, "coordinator_intent_missing:1"
        ):
            RUN_MODULE.verify_certification(str(manifest_path))

    def test_coordinator_rejects_legacy_raw_receipt_prefix(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        evidence_by_step = complete_evidence(manifest)
        action = RUN_MODULE.next_certification_action(str(manifest_path))
        self.assertEqual(action["step"], 1)
        self.assertEqual(self.record(manifest_path, 1, evidence_by_step[1]), 0)
        with self.assertRaisesRegex(
            RUN_MODULE.CertificationError, "coordinator_intent_missing:1"
        ):
            RUN_MODULE.next_certification_action(str(manifest_path))

    def test_coordinator_does_not_advance_without_a_receipt(self):
        manifest_path = self.prepare()
        before = RUN_MODULE.next_certification_action(str(manifest_path))
        after = RUN_MODULE.next_certification_action(str(manifest_path))
        self.assertEqual(before, after)
        self.assertEqual(before["status"], "next_step")
        self.assertEqual(before["step"], 1)
        self.assertEqual(manifest_path.with_name("receipts.jsonl").read_bytes(), b"")

    def test_step_eleven_requires_the_fixed_live_fresh_lease_checkpoint(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        evidence_by_step = complete_evidence(manifest)
        for step in range(1, 11):
            self.assertEqual(
                self.record(manifest_path, step, evidence_by_step[step]), 0
            )
        evidence_by_step[11]["checkpoint"] = "runtime_ready"
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = self.record(manifest_path, 11, evidence_by_step[11])
        self.assertEqual(status, 1)
        self.assertIn("step_contract_failed:checkpoint", stderr.getvalue())

    def test_step_eleven_binds_the_canonical_confirmation_scope(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        evidence_by_step = complete_evidence(manifest)
        for step in range(1, 11):
            self.assertEqual(
                self.record(manifest_path, step, evidence_by_step[step]), 0
            )
        evidence = evidence_by_step[11]
        mutations = (
            ("process_identity_joined", False),
            ("process_instance_id", "not-a-process-id"),
            ("public_origin", "https://api.starring.co.kr"),
            ("installation_id", "installation-other"),
            (
                "operation_id",
                "d2:ffffffffffffffff:certify-live-runtime-restart",
            ),
        )
        for field, replacement in mutations:
            original = evidence[field]
            evidence[field] = replacement
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(self.record(manifest_path, 11, evidence), 1)
            evidence[field] = original
        self.assertEqual(self.record(manifest_path, 11, evidence), 0)

    def test_oauth_principal_must_match_the_pinned_transport_actor(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        evidence_by_step = complete_evidence(manifest)
        for step in range(1, 4):
            self.assertEqual(self.record(manifest_path, step, evidence_by_step[step]), 0)
        evidence_by_step[4]["principal_id"] = "discord:1056857223529250907"
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = self.record(manifest_path, 4, evidence_by_step[4])
        self.assertEqual(status, 1)
        self.assertIn("step_contract_failed:principal_id", stderr.getvalue())

    def test_fault_receipt_requires_the_pinned_transport_instance(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        evidence_by_step = complete_evidence(manifest)
        for step in range(1, 10):
            self.assertEqual(self.record(manifest_path, step, evidence_by_step[step]), 0)
        evidence_by_step[10]["transport_instance_id"] = (
            "d2ti-fedcba9876543210fedcba9876543210"
        )
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = self.record(manifest_path, 10, evidence_by_step[10])
        self.assertEqual(status, 1)
        self.assertIn("step_contract_failed:transport_instance_id", stderr.getvalue())

    def test_step_seven_binds_exact_authoring_preview_identity(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        evidence_by_step = complete_evidence(manifest)
        for step in range(1, 7):
            self.assertEqual(self.record(manifest_path, step, evidence_by_step[step]), 0)
        mutations = (
            ("authoring_session_id", "authoring-session-other"),
            ("authoring_generation", 2),
            ("target_content_hash", "f" * 64),
        )
        for field, replacement in mutations:
            evidence = copy.deepcopy(evidence_by_step[7])
            evidence[field] = replacement
            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                self.assertEqual(self.record(manifest_path, 7, evidence), 1)
            self.assertIn(
                "step_contract_failed:product_decision_authoring_identity",
                stderr.getvalue(),
            )
        self.assertEqual(self.record(manifest_path, 7, evidence_by_step[7]), 0)

    def test_record_rejects_out_of_order_step(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = self.record(manifest_path, 2, complete_evidence(manifest)[2])
        self.assertEqual(status, 1)
        self.assertIn("step_out_of_order:expected_1", stderr.getvalue())

    def test_record_exact_replay_is_a_noop(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        evidence = complete_evidence(manifest)[1]
        first_status, first = self.record_result(manifest_path, 1, evidence)
        receipts_path = manifest_path.with_name("receipts.jsonl")
        before = receipts_path.read_bytes()
        second_status, second = self.record_result(manifest_path, 1, evidence)
        self.assertEqual((first_status, second_status), (0, 0))
        self.assertEqual(first["disposition"], "created")
        self.assertEqual(second["disposition"], "exact_replay")
        self.assertFalse(first["replayed"])
        self.assertTrue(second["replayed"])
        self.assertEqual(first["receipt_sha256"], second["receipt_sha256"])
        self.assertEqual(receipts_path.read_bytes(), before)

    def test_record_exact_replay_accepts_canonical_key_order(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        evidence = complete_evidence(manifest)[1]
        self.assertEqual(self.record(manifest_path, 1, evidence), 0)
        reordered = dict(reversed(tuple(evidence.items())))
        status, result = self.record_result(manifest_path, 1, reordered)
        self.assertEqual(status, 0)
        self.assertEqual(result["disposition"], "exact_replay")
        self.assertEqual(len(manifest_path.with_name("receipts.jsonl").read_text().splitlines()), 1)

    def test_record_divergent_replay_fails_closed(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        evidence = complete_evidence(manifest)[1]
        self.assertEqual(self.record(manifest_path, 1, evidence), 0)
        receipts_path = manifest_path.with_name("receipts.jsonl")
        before = receipts_path.read_bytes()
        changed = dict(evidence)
        changed["migration_count"] += 1
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status, result = self.record_result(manifest_path, 1, changed)
        self.assertEqual(status, 1)
        self.assertIsNone(result)
        self.assertIn("step_replay_mismatch", stderr.getvalue())
        self.assertEqual(receipts_path.read_bytes(), before)

    def test_completed_certification_replays_step_seventeen(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        evidence_by_step = complete_evidence(manifest)
        for step, evidence in evidence_by_step.items():
            self.assertEqual(self.record(manifest_path, step, evidence), 0)
        status, result = self.record_result(manifest_path, 17, evidence_by_step[17])
        self.assertEqual(status, 0)
        self.assertEqual(result["disposition"], "exact_replay")
        self.assertEqual(len(manifest_path.with_name("receipts.jsonl").read_text().splitlines()), 17)

    def test_record_rejects_secret_keys_and_values(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        evidence = complete_evidence(manifest)[1]
        evidence["bot_token"] = "not-recorded"
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = self.record(manifest_path, 1, evidence)
        self.assertEqual(status, 1)
        self.assertIn("evidence_forbidden_key", stderr.getvalue())

    def test_record_rejects_non_contract_extra_fields(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        evidence = complete_evidence(manifest)[1]
        evidence["operator_note"] = "not part of the receipt contract"
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = self.record(manifest_path, 1, evidence)
        self.assertEqual(status, 1)
        self.assertIn("evidence_fields_invalid", stderr.getvalue())

    def test_record_rejects_duplicate_json_keys(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        evidence = complete_evidence(manifest)[1]
        raw = json.dumps(evidence)[:-1] + ',"migration_count":125}'
        evidence_path = self.root / "duplicate-evidence.json"
        evidence_path.write_text(raw, encoding="utf-8")
        evidence_path.chmod(0o600)
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = MODULE.main(
                [
                    "record",
                    "--manifest",
                    str(manifest_path),
                    "--step",
                    "1",
                    "--evidence",
                    str(evidence_path),
                ]
            )
        self.assertEqual(status, 1)
        self.assertIn("json_duplicate_key", stderr.getvalue())

    def test_receipt_chain_rejects_post_record_mutation(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        self.assertEqual(self.record(manifest_path, 1, complete_evidence(manifest)[1]), 0)
        receipts_path = manifest_path.with_name("receipts.jsonl")
        receipt = json.loads(receipts_path.read_text(encoding="utf-8"))
        receipt["evidence"]["migration_count"] += 1
        receipts_path.write_text(json.dumps(receipt) + "\n", encoding="utf-8")
        receipts_path.chmod(0o600)
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = MODULE.main(["verify", "--manifest", str(manifest_path)])
        self.assertEqual(status, 1)
        self.assertIn("receipt_chain_invalid", stderr.getvalue())

    def test_non_last_candidate_shape_is_validated(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        manifest["candidates"]["api"]["path"] = 7
        with self.assertRaisesRegex(
            MODULE.CertificationError, "manifest_candidate_api_invalid"
        ):
            MODULE.validate_manifest(manifest)

    def test_manifest_requires_the_exact_pinned_hub_channel_shape(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        manifest["discord"].pop("hub_channel_id")
        with self.assertRaisesRegex(
            MODULE.CertificationError, "manifest_discord_invalid"
        ):
            MODULE.validate_manifest(manifest)
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        manifest["discord"]["hub_channel_id"] = "not-a-snowflake"
        with self.assertRaisesRegex(
            MODULE.CertificationError, "manifest_discord_hub_channel_invalid"
        ):
            MODULE.validate_manifest(manifest)
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        manifest["discord"]["hub_channel_id"] = manifest["discord"]["guild_id"]
        with self.assertRaisesRegex(
            MODULE.CertificationError, "manifest_discord_hub_channel_invalid"
        ):
            MODULE.validate_manifest(manifest)
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        manifest["discord"]["unexpected_binding"] = "1524810437118525554"
        with self.assertRaisesRegex(
            MODULE.CertificationError, "manifest_discord_invalid"
        ):
            MODULE.validate_manifest(manifest)

    def test_snowflake_validation_matches_the_unsigned_64_bit_contract(self):
        self.assertEqual(MODULE.validate_snowflake("1", "snowflake"), "1")
        self.assertEqual(
            MODULE.validate_snowflake("18446744073709551615", "snowflake"),
            "18446744073709551615",
        )
        for value in (
            "0",
            "01",
            "18446744073709551616",
            "9999999999999999999999999",
        ):
            with self.subTest(value=value), self.assertRaisesRegex(
                MODULE.CertificationError, "snowflake_invalid"
            ):
                MODULE.validate_snowflake(value, "snowflake")

    def test_candidate_permissions_and_worker_inventory_are_revalidated(self):
        manifest_path = self.prepare()
        self.artifact_root.chmod(0o755)
        with self.assertRaisesRegex(
            MODULE.CertificationError, "candidate_api_directory_mutable"
        ):
            MODULE.load_verified_manifest(manifest_path)
        self.artifact_root.chmod(0o555)
        self.candidates["api"].chmod(0o666)
        with self.assertRaisesRegex(MODULE.CertificationError, "candidate_api_writable"):
            MODULE.load_verified_manifest(manifest_path)
        self.candidates["api"].chmod(0o555)
        extra = self.candidates["codex_worker"].parent / "unpinned-runtime.mjs"
        self.candidates["codex_worker"].parent.chmod(0o755)
        extra.write_text("export const unpinned = true;\n", encoding="utf-8")
        self.candidates["codex_worker"].parent.chmod(0o555)
        with self.assertRaisesRegex(
            MODULE.CertificationError, "codex_worker_inventory_invalid"
        ):
            MODULE.load_verified_manifest(manifest_path)

    def test_evidence_and_run_directory_require_exact_owned_modes(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        evidence_path = self.write_evidence(1, complete_evidence(manifest)[1])
        evidence_path.chmod(0o644)
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = MODULE.main(
                [
                    "record",
                    "--manifest",
                    str(manifest_path),
                    "--step",
                    "1",
                    "--evidence",
                    str(evidence_path),
                ]
            )
        self.assertEqual(status, 1)
        self.assertIn("evidence_ownership_invalid", stderr.getvalue())

    def test_cleanup_requires_total_absence(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        evidence_by_step = complete_evidence(manifest)
        for step in range(1, 17):
            self.assertEqual(self.record(manifest_path, step, evidence_by_step[step]), 0)
        evidence_by_step[17]["unresolved_receipt_count"] = 1
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = self.record(manifest_path, 17, evidence_by_step[17])
        self.assertEqual(status, 1)
        self.assertIn("step_contract_failed:unresolved_receipt_count", stderr.getvalue())

    def test_step_fourteen_accepts_canonical_replacement_identity(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        evidence_by_step = complete_evidence(manifest)
        for step in range(1, 15):
            self.assertEqual(self.record(manifest_path, step, evidence_by_step[step]), 0)
        receipts = manifest_path.with_name("receipts.jsonl").read_text().splitlines()
        self.assertEqual(len(receipts), 14)
        self.assertEqual(json.loads(receipts[-1])["code"], "target_replaced")

    def test_manifest_tampering_is_detected(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        manifest["commit_sha"] = "c" * 40
        manifest_path.write_text(json.dumps(manifest), encoding="utf-8")
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = MODULE.main(["verify", "--manifest", str(manifest_path)])
        self.assertEqual(status, 1)
        self.assertIn("manifest_digest_mismatch", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
