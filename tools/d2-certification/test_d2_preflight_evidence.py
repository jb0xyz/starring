import json
import pathlib
import subprocess
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock


DIRECTORY = pathlib.Path(__file__).parent
sys.path.insert(0, str(DIRECTORY))

import d2_orchestrator_contract as CONTRACT
import d2_preflight_evidence as PREFLIGHT


class FakePlatform:
    def __init__(self, process_output=b""):
        self.process_output = process_output
        self.loaded = set()
        self.keychain = {
            ("starring.d2.external", "discord.bot-token"),
            ("starring.d2.external", "discord.oauth-client-secret"),
            ("starring.d2.external", "cloudflare.tunnel-token"),
        }
        self.busy_ports = set()

    def run(self, arguments, input_bytes=None, timeout=30, environment=None):
        return subprocess.CompletedProcess(arguments, 0, self.process_output, b"")

    def launchd_loaded(self, label):
        return label in self.loaded

    def keychain_present(self, service, account):
        return (service, account) in self.keychain

    def port_available(self, port):
        return port not in self.busy_ports


def context(root):
    run_id = "d2-20260804t010203z-012345abcdef"
    run_directory = root / run_id
    run_directory.mkdir(mode=0o700)
    return SimpleNamespace(
        digest="a" * 64,
        run_directory=run_directory,
        artifact_directory=run_directory / "orchestrator",
        root=pathlib.Path(f"/private/tmp/starring-d2-{run_id}"),
        manifest={
            "run_id": run_id,
            "discord": {"resource_prefix": "starring-d2-20260804-012345abcdef"},
            "database": {"port": 55433},
            "services": {
                "api": {"label": "local.starring.d2.a.api", "port": 28080},
                "runtime": {
                    "label": "local.starring.d2.a.runtime",
                    "port": 29091,
                },
                "worker": {
                    "label": "local.starring.d2.a.worker",
                    "port": 28181,
                },
                "transport": {
                    "label": "local.starring.d2.a.transport",
                    "gateway_port": 29101,
                    "http_port": 29102,
                },
                "tunnel": {"label": "local.starring.d2.a.tunnel"},
            },
            "keychain_services": {
                "api": "starring.d2.a.api",
                "runtime": "starring.d2.a.runtime",
                "postgres": "starring.d2.a.postgres",
                "worker": "starring.d2.a.worker",
            },
            "external_keychain": {
                "discord_bot_token": {
                    "service": "starring.d2.external",
                    "account": "discord.bot-token",
                },
                "discord_oauth_client_secret": {
                    "service": "starring.d2.external",
                    "account": "discord.oauth-client-secret",
                },
                "tunnel_token": {
                    "service": "starring.d2.external",
                    "account": "cloudflare.tunnel-token",
                },
            },
        },
    )


class PreflightEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.context = context(pathlib.Path(self.temporary.name))
        self.snapshot = {
            "launchd_loaded": {"local.starring.api.staging": True},
            "postgres": {"running": True},
        }

    def tearDown(self):
        self.temporary.cleanup()

    def dry_run(self, _context, _platform):
        return {
            "status": "ready",
            "manifest_sha256": self.context.digest,
            "standing_snapshot": self.snapshot,
            "standing_mutation_allowed": False,
        }

    def test_process_scan_excludes_self_ancestry_and_counts_stale_owner(self):
        rows = [
            (10, 1, "codex"),
            (20, 10, "python d2_preflight_evidence.py"),
            (30, 1, f"runtime {self.context.manifest['run_id']}"),
            (31, 1, "unrelated"),
        ]
        with mock.patch.object(PREFLIGHT.os, "getpid", return_value=20):
            self.assertEqual(PREFLIGHT.ancestor_process_ids(rows), {20, 10, 1})
            self.assertEqual(PREFLIGHT.smoke_process_count(self.context, rows), 1)

    def test_records_private_zero_absence_and_exact_replay(self):
        platform = FakePlatform(b"1 0 launchd\n")
        with mock.patch.object(
            PREFLIGHT, "command_dry_run", side_effect=self.dry_run
        ):
            first = PREFLIGHT.command_preflight_evidence(self.context, platform)
            replay = PREFLIGHT.command_preflight_evidence(self.context, platform)
        self.assertEqual(first["status"], "recorded")
        self.assertEqual(replay["status"], "exact_replay")
        path = pathlib.Path(first["evidence"])
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        value = PREFLIGHT.load_private_evidence(path)
        self.assertEqual(value["prior_runtime_owner_count"], 0)
        self.assertEqual(value["prior_smoke_process_count"], 0)
        self.assertEqual(value["external_credential_count"], 3)
        source_path = pathlib.Path(first["coordinator_source"])
        self.assertEqual(first["coordinator_source"], replay["coordinator_source"])
        self.assertEqual(source_path.stat().st_mode & 0o777, 0o600)
        source = json.loads(source_path.read_text(encoding="utf-8"))
        self.assertEqual(
            source["kind"],
            "starring.d2.orchestrator-prior-absence-evidence.v1",
        )
        self.assertEqual(source["manifest_sha256"], self.context.digest)
        self.assertEqual(source["run_id"], self.context.manifest["run_id"])
        self.assertEqual(
            source["evidence"],
            {
                "prior_runtime_owner_count": 0,
                "prior_smoke_process_count": 0,
            },
        )

    def test_busy_owner_and_process_fail_without_writing_evidence(self):
        cases = [
            (FakePlatform(b"1 0 launchd\n"), "owner"),
            (
                FakePlatform(
                    f"1 0 runtime {self.context.manifest['run_id']}\n".encode()
                ),
                "process",
            ),
        ]
        cases[0][0].loaded.add("local.starring.d2.a.api")
        for platform, name in cases:
            with self.subTest(name=name), mock.patch.object(
                PREFLIGHT, "command_dry_run", side_effect=self.dry_run
            ), self.assertRaisesRegex(
                CONTRACT.OrchestratorError, "preflight_evidence_invalid"
            ):
                PREFLIGHT.command_preflight_evidence(self.context, platform)
            path = PREFLIGHT.preflight_evidence_path(self.context)
            if path.exists():
                path.unlink()

    def test_private_mode_and_process_shape_are_strict(self):
        platform = FakePlatform(b"1 0 launchd\n")
        with mock.patch.object(
            PREFLIGHT, "command_dry_run", side_effect=self.dry_run
        ):
            result = PREFLIGHT.command_preflight_evidence(self.context, platform)
        path = pathlib.Path(result["evidence"])
        path.chmod(0o644)
        with self.assertRaisesRegex(
            CONTRACT.OrchestratorError, "preflight_evidence_ownership_invalid"
        ):
            PREFLIGHT.load_private_evidence(path)
        invalid = FakePlatform(b"not-a-process-row\n")
        with self.assertRaisesRegex(
            CONTRACT.OrchestratorError, "preflight_process_scan_invalid"
        ):
            PREFLIGHT.process_rows(invalid)


if __name__ == "__main__":
    unittest.main()
