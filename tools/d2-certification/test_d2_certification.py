#!/usr/bin/env python3

import contextlib
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


COMMIT = "a" * 40
DIGEST = "b" * 64
RUN_ID = "d2-20260801t120000z-0123456789ab"


def complete_evidence(manifest):
    prefix = manifest["discord"]["resource_prefix"]
    return {
        1: {
            "database_system_identifier": "7667905772642692043",
            "migration_count": 117,
            "migration_head": "202608010002",
            "migration_ledger_sha256": DIGEST,
            "discord_resource_prefix": prefix,
        },
        2: {"prior_runtime_owner_count": 0, "prior_smoke_process_count": 0},
        3: {
            "api_sha256": manifest["candidates"]["api"]["sha256"],
            "runtime_sha256": manifest["candidates"]["runtime"]["sha256"],
            "codex_worker_sha256": manifest["source_trees"]["codex_worker"]["sha256"],
            "d2_toolchain_sha256": manifest["source_trees"]["d2_toolchain"]["sha256"],
            "api_build_revision": COMMIT,
            "runtime_build_revision": COMMIT,
            "api_ready_status": 200,
            "runtime_ready_status": 200,
            "worker_ready_status": 200,
            "cloudflare_tunnel_id": manifest["cloudflare"]["tunnel_id"],
            "public_origin": manifest["cloudflare"]["public_origin"],
            "origin_service": manifest["cloudflare"]["origin_service"],
            "tunnel_ready": True,
        },
        4: {
            "oauth_callback_status": 303,
            "me_status": 200,
            "principal_id": "discord:1056857223529250906",
            "installation_id": "installation-1",
            "guild_id": "1524810437118525551",
            "authority_check_status": 204,
        },
        5: {
            "authoring_http_status": 200,
            "authoring_session_id": "authoring-session-1",
            "authoring_generation": 1,
            "installation_id": "installation-1",
            "model": "gpt-5.6-luna",
            "provider": "codex_chatgpt",
            "reasoning_effort": "medium",
            "auth_mode": "chatgpt",
            "one_shot": True,
        },
        6: {
            "generation_encrypted": True,
            "projection_state": "preview_ready",
            "generation": 1,
            "payload_digest": DIGEST,
            "installation_id": "installation-1",
            "authoring_session_id": "authoring-session-1",
        },
        7: {
            "installation_id": "installation-1",
            "promotion_id": DIGEST,
            "preview_state": "pending_approval",
            "approval_state": "approved",
            "apply_state": "runtime_pending",
        },
        8: {
            "pending_observed": True,
            "live_observed": True,
            "installation_id": "installation-1",
            "promotion_id": DIGEST,
            "deployment_id": "deployment-1",
            "route_id": "route-1",
            "attestation_id": "attestation-1",
            "serving_lease_id": "lease-1",
        },
        9: {
            "create_interaction_id": "1532677575736819845",
            "join_interaction_id": "1532677575736819846",
            "deployment_id": "deployment-1",
            "route_id": "route-1",
            "instance_id": "instance-1",
            "role_ids": ["1532677575736819847"],
            "channel_ids": ["1532677575736819848"],
            "panel_message_ids": ["1532677575736819849"],
            "ephemeral_count": 2,
        },
        10: {
            "interaction_id": "1532677575736819846",
            "delivery_count": 2,
            "external_effect_count": 1,
            "receipt_state": "completed",
        },
        11: {
            "old_pid": 100,
            "new_pid": 101,
            "runtime_sha256": manifest["candidates"]["runtime"]["sha256"],
            "ready_after_restart": True,
            "checkpoint": "live_fresh_lease",
            "deployment_id": "deployment-1",
            "route_id": "route-1",
            "instance_id": "instance-1",
        },
        12: {
            "route_reconstructed": True,
            "instance_reconstructed": True,
            "deployment_id": "deployment-1",
            "route_id": "route-1",
            "instance_id": "instance-1",
            "pinned_ruleset_digest": DIGEST,
        },
        13: {
            "effect_id": "effect-1",
            "interaction_id": "1532677575736819850",
            "route_id": "route-1",
            "injected_outcome": "indeterminate",
            "reconciliation_state": "known_success",
            "duplicate_external_effect_count": 0,
            "unsafe_deletion_count": 0,
        },
        14: {
            "replacement_target_id": "deployment-2",
            "replacement_kind": "rollback",
            "source_deployment_id": "deployment-1",
            "source_route_id": "route-1",
            "replacement_deployment_id": "deployment-2",
            "replacement_route_id": "route-2",
            "previous_target_drained": True,
            "replacement_live": True,
            "prior_route_absent": True,
        },
        15: {
            "gateway_disconnected": True,
            "live_lost": True,
            "runtime_ready_status": 503,
            "public_code": "runtime_gateway_disconnected",
            "route_id": "route-2",
        },
        16: {
            "teardown_started": True,
            "discord_resource_ids_deleted": [
                "1532677575736819847",
                "1532677575736819848",
                "1532677575736819849",
            ],
            "database_drop_requested": True,
            "services_stopped": True,
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
        self.root = pathlib.Path(self.temporary.name)
        self.candidates = {}
        for name in MODULE.REQUIRED_CANDIDATES:
            path = (
                self.root / "worker-tree" / "worker.mjs"
                if name == "codex_worker"
                else self.root / name
            )
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(f"candidate:{name}".encode())
            self.candidates[name] = path
        for name in MODULE.CODEX_WORKER_SOURCE_FILES:
            path = self.candidates["codex_worker"].parent / name
            if not path.exists():
                path.write_bytes(f"worker-source:{name}".encode())

    def tearDown(self):
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
        return status

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
        self.assertEqual(len(set(value["port"] for value in manifest["services"].values() if "port" in value)), 3)
        self.assertEqual(
            manifest["candidates"]["runtime"]["sha256"],
            MODULE.sha256_file(self.candidates["runtime"]),
        )
        self.assertEqual(oct(manifest_path.stat().st_mode & 0o777), "0o600")
        self.assertEqual(oct(manifest_path.parent.stat().st_mode & 0o777), "0o700")

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

    def test_complete_seventeen_step_evidence_passes(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        for step, evidence in complete_evidence(manifest).items():
            self.assertEqual(self.record(manifest_path, step, evidence), 0)
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.assertEqual(
                MODULE.main(["verify", "--manifest", str(manifest_path)]), 0
            )
        summary = json.loads(output.getvalue())
        self.assertEqual(summary["status"], "passed")
        self.assertEqual(summary["steps"], 17)
        self.assertRegex(summary["receipt_chain_head_sha256"], r"^[0-9a-f]{64}$")

    def test_record_rejects_out_of_order_step(self):
        manifest_path = self.prepare()
        manifest = json.loads(manifest_path.read_text())
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = self.record(manifest_path, 2, complete_evidence(manifest)[2])
        self.assertEqual(status, 1)
        self.assertIn("step_out_of_order:expected_1", stderr.getvalue())

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

    def test_candidate_permissions_and_worker_inventory_are_revalidated(self):
        manifest_path = self.prepare()
        self.candidates["api"].chmod(0o666)
        with self.assertRaisesRegex(MODULE.CertificationError, "candidate_api_writable"):
            MODULE.load_verified_manifest(manifest_path)
        self.candidates["api"].chmod(0o644)
        extra = self.candidates["codex_worker"].parent / "unpinned-runtime.mjs"
        extra.write_text("export const unpinned = true;\n", encoding="utf-8")
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
