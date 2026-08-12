import hashlib
import copy
import fcntl
import importlib.util
import json
import os
import pathlib
import stat
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO


MODULE_PATH = pathlib.Path(__file__).with_name("d2a_bootstrap.py")
SPEC = importlib.util.spec_from_file_location("d2a_bootstrap", MODULE_PATH)
BOOTSTRAP = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BOOTSTRAP)


def write_file(path, value, mode, *, sort_keys=True):
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if isinstance(value, (dict, list)):
        payload = (json.dumps(value, ensure_ascii=False, sort_keys=sort_keys, separators=(",", ":")) + "\n").encode()
    elif isinstance(value, str):
        payload = value.encode()
    else:
        payload = value
    path.write_bytes(payload)
    path.chmod(mode)
    return path


def flag(argv, name):
    return argv[argv.index(name) + 1]


class FakeExecutor:
    def __init__(
        self,
        fixture,
        fail_once=None,
        interrupt_at=None,
        secret_stderr=b"",
        direct_evidence_tamper=None,
        probe_issuer_lock=False,
        bad_toolchain=None,
        bad_source=None,
        change_toolchain_after_build=False,
        source_drift_at_snapshot=None,
        preexisting_lifecycle=None,
        direct_onboard_error_code=None,
        candidate_launchd_overrides_absent=True,
        quarantined_direct_onboard=False,
        audited_recovery_tamper=None,
        one_shot_error_code=None,
        teardown_error_code=None,
    ):
        self.fixture = fixture
        self.fail_once = dict(fail_once or {})
        self.interrupt_at = interrupt_at
        self.secret_stderr = secret_stderr
        self.direct_evidence_tamper = direct_evidence_tamper
        self.probe_issuer_lock = probe_issuer_lock
        self.bad_toolchain = bad_toolchain
        self.bad_source = bad_source
        self.change_toolchain_after_build = change_toolchain_after_build
        self.source_drift_at_snapshot = source_drift_at_snapshot
        self.preexisting_lifecycle = preexisting_lifecycle
        self.direct_onboard_error_code = direct_onboard_error_code
        self.candidate_launchd_overrides_absent = (
            candidate_launchd_overrides_absent
        )
        self.quarantined_direct_onboard = quarantined_direct_onboard
        self.audited_recovery_tamper = audited_recovery_tamper
        self.one_shot_error_code = one_shot_error_code
        self.teardown_error_code = teardown_error_code
        self.source_snapshot_count = 0
        self.issuer_lock_acquired = False
        self.calls = []
        self.argv_calls = []
        self.taint_seen_before_followup = True
        self.preissuer_lifecycle_seen = None

    def completed(self, argv, value, returncode=0, stderr=b""):
        stdout = b"" if value is None else (BOOTSTRAP.canonical_json(value) + "\n").encode()
        return subprocess.CompletedProcess(argv, returncode, stdout=stdout, stderr=stderr)

    def write_revoked_lifecycle(self, operation):
        taint = json.loads(
            self.fixture.manifest_path.with_name("d2a-taint.json").read_bytes()
        )
        lifecycle = {
            "schema_version": 1,
            "kind": "starring.d2a.session-lifecycle.v1",
            "run_id": self.fixture.run_id,
            "manifest_sha256": self.fixture.manifest_sha256,
            "operation": operation,
            "origin": "issuer",
            "issuer_sha256": taint["issuer_sha256"],
            "issuer_source_sha256": taint["issuer_source_sha256"],
            "uid": os.getuid(),
            "boot_identity": "darwin-boottime:1:0",
            "process_group_id": 2_000_000_000,
            "started_at": "2026-08-12T00:00:00.000000000Z",
            "status": "revoked",
            "session_revoked": True,
            "revoked_at": "2026-08-12T00:00:01.000000000Z",
            "quarantined_at": None,
        }
        path = self.fixture.manifest_path.with_name("d2a-session-lifecycle.json")
        path.write_bytes(
            (json.dumps(lifecycle, ensure_ascii=False, separators=(",", ":")) + "\n").encode()
        )
        path.chmod(0o600)

    def write_quarantined_lifecycle(self):
        taint = json.loads(
            self.fixture.manifest_path.with_name("d2a-taint.json").read_bytes()
        )
        lifecycle = {
            "schema_version": 1,
            "kind": "starring.d2a.session-lifecycle.v1",
            "run_id": self.fixture.run_id,
            "manifest_sha256": self.fixture.manifest_sha256,
            "operation": "direct-onboard",
            "origin": "issuer",
            "issuer_sha256": taint["issuer_sha256"],
            "issuer_source_sha256": taint["issuer_source_sha256"],
            "uid": os.getuid(),
            "boot_identity": "darwin-boottime:1:0",
            "process_group_id": 2_000_000_000,
            "started_at": "2026-08-12T00:00:00.000000000Z",
            "status": "quarantined",
            "session_revoked": False,
            "revoked_at": None,
            "quarantined_at": "2026-08-12T00:00:01.000000000Z",
        }
        write_file(
            self.fixture.manifest_path.with_name("d2a-session-lifecycle.json"),
            lifecycle,
            0o600,
            sort_keys=False,
        )

    def __call__(self, argv):
        identity = BOOTSTRAP.command_identity(argv)
        self.calls.append(identity)
        self.argv_calls.append(list(argv))
        if identity != "manifest_prepare" and self.fixture.manifest_path is not None:
            self.taint_seen_before_followup &= self.fixture.manifest_path.with_name("d2a-taint.json").is_file()
        if self.interrupt_at == identity:
            raise KeyboardInterrupt()
        if self.fail_once.get(identity, 0):
            self.fail_once[identity] -= 1
            if identity == "d2a_one_shot" and self.one_shot_error_code is not None:
                self.write_revoked_lifecycle("one-shot")
                return self.completed(
                    argv,
                    {
                        "error_code": self.one_shot_error_code,
                        "kind": BOOTSTRAP.COMMAND_ERROR_KIND,
                        "operation": "one-shot",
                        "schema_version": 1,
                    },
                    1,
                )
            if (
                identity == "teardown_discord_resources"
                and self.teardown_error_code is not None
            ):
                diagnostic = (
                    BOOTSTRAP.canonical_json(
                        {"code": self.teardown_error_code, "status": "failed"}
                    )
                    + "\n"
                ).encode("ascii")
                return self.completed(argv, None, 1, diagnostic)
            return self.completed(argv, None, 1, self.secret_stderr or b"untrusted child diagnostics")
        if identity == "source_root":
            self.source_snapshot_count += 1
            return subprocess.CompletedProcess(
                argv,
                0,
                stdout=(str(self.fixture.root) + "\n").encode(),
                stderr=b"",
            )
        if identity == "source_commit":
            return subprocess.CompletedProcess(
                argv,
                0,
                stdout=(
                    (("a" if self.bad_source == "commit" else "c") * 40) + "\n"
                ).encode(),
                stderr=b"",
            )
        if identity == "source_tree":
            return subprocess.CompletedProcess(
                argv,
                0,
                stdout=("d" * 40 + "\n").encode(),
                stderr=b"",
            )
        if identity == "source_status":
            return subprocess.CompletedProcess(
                argv,
                0,
                stdout=b" M tools/d2-maintenance/d2a_bootstrap.py\n"
                if (
                    self.bad_source == "dirty"
                    or self.source_snapshot_count == self.source_drift_at_snapshot
                )
                else b"",
                stderr=b"",
            )
        if identity == "cargo_version":
            return subprocess.CompletedProcess(
                argv,
                0,
                stdout=(
                    b"cargo 1.96.0 (bad 2026-01-01)\n"
                    if self.bad_toolchain == "cargo"
                    else b"cargo 1.97.0 (c980f4866 2026-06-30)\n"
                ),
                stderr=b"",
            )
        if identity == "rustc_verbose_version":
            return subprocess.CompletedProcess(
                argv,
                0,
                stdout=(
                    b"rustc 1.97.0 (2d8144b78 2026-07-07)\n"
                    b"binary: rustc\ncommit-hash: 2d8144b78\n"
                    b"commit-date: 2026-07-07\nhost: "
                    + (
                        b"x86_64-apple-darwin\n"
                        if self.bad_toolchain == "host"
                        else b"aarch64-apple-darwin\n"
                    )
                    + b"release: 1.97.0\nLLVM version: 21.1.0\n"
                ),
                stderr=b"",
            )
        if identity == "issuer_build":
            target_root = pathlib.Path(flag(argv, "--target-dir"))
            write_file(
                target_root
                / "release"
                / "starring-d2-session-issuer",
                b"fixture-issuer",
                0o755,
            )
            if self.change_toolchain_after_build:
                write_file(
                    self.fixture.rust_toolchain_bin / "cargo",
                    b"changed-cargo",
                    0o755,
                )
            return self.completed(argv, None)
        if identity == "issuer_linkage":
            path = pathlib.Path(argv[2])
            return subprocess.CompletedProcess(
                argv,
                0,
                stdout=(
                    f"{path}:\n"
                    "\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1351.0.0)\n"
                ).encode(),
                stderr=b"",
            )
        if identity == "manifest_prepare":
            self.fixture.create_manifest(argv)
            if self.preexisting_lifecycle is not None:
                write_file(
                    self.fixture.manifest_path.with_name(
                        "d2a-session-lifecycle.json"
                    ),
                    self.preexisting_lifecycle,
                    0o600,
                    sort_keys=False,
                )
            return self.completed(
                argv,
                {
                    "run_id": self.fixture.run_id,
                    "manifest": str(self.fixture.manifest_path),
                    "receipts": str(self.fixture.manifest_path.with_name("receipts.jsonl")),
                    "resource_prefix": self.fixture.resource_prefix,
                },
            )
        if identity == "dry_run":
            lifecycle_path = self.fixture.manifest_path.with_name(
                "d2a-session-lifecycle.json"
            )
            self.preissuer_lifecycle_seen = lifecycle_path.read_bytes()
            return self.completed(
                argv,
                {
                    "status": "ready",
                    "manifest_sha256": self.fixture.manifest_sha256,
                    "standing_snapshot": {"protected": "unchanged"},
                    "standing_mutation_allowed": False,
                },
            )
        if identity == "preflight":
            return self.completed(
                argv,
                {
                    "status": "recorded",
                    "manifest_sha256": self.fixture.manifest_sha256,
                    "evidence": str(self.fixture.manifest_path.with_name("preflight.json")),
                    "coordinator_source": str(self.fixture.manifest_path.with_name("source.json")),
                },
            )
        if identity == "prepare":
            orchestrator = self.fixture.manifest_path.parent / "orchestrator"
            orchestrator.mkdir(mode=0o700)
            write_file(orchestrator / "state.json", {"phase": "prepared"}, 0o600)
            return self.completed(argv, {"status": "prepared", "phase": "prepared"})
        if identity == "start":
            return self.completed(
                argv,
                {
                    "status": "candidate_started",
                    "phase": "candidate_started",
                    "candidate_services_loaded": True,
                    "database_schema_ready": True,
                    "credentials_sealed": True,
                    "coordinator_sources": {},
                },
            )
        if identity == "direct_onboard":
            if self.quarantined_direct_onboard:
                self.write_quarantined_lifecycle()
                return self.completed(
                    argv, None, 1, b"untrusted issuer failure\n"
                )
            if self.direct_onboard_error_code is not None:
                return self.completed(
                    argv,
                    None,
                    1,
                    f"error: {self.direct_onboard_error_code}\n".encode("ascii"),
                )
            if self.probe_issuer_lock:
                descriptor = os.open(self.fixture.issuer_lock_path, os.O_RDWR | os.O_CREAT, 0o600)
                try:
                    fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    self.issuer_lock_acquired = True
                    fcntl.flock(descriptor, fcntl.LOCK_UN)
                finally:
                    os.close(descriptor)
            stdout_evidence = self.fixture.direct_onboarding_evidence()
            persisted = dict(stdout_evidence)
            mode = 0o600
            if self.direct_evidence_tamper == "persisted_mismatch":
                persisted["outcome"] = "exact_replay"
            elif self.direct_evidence_tamper == "mode":
                mode = 0o644
            write_file(
                self.fixture.manifest_path.with_name("d2a-onboarding-evidence.json"),
                persisted,
                mode,
            )
            if self.direct_evidence_tamper == "hardlink":
                os.link(
                    self.fixture.manifest_path.with_name("d2a-onboarding-evidence.json"),
                    self.fixture.manifest_path.with_name("d2a-onboarding-evidence-link.json"),
                )
            if self.direct_evidence_tamper == "commercial_artifact":
                write_file(
                    self.fixture.manifest_path.parent
                    / "orchestrator"
                    / "onboarding-evidence.json",
                    {"commercial": True},
                    0o600,
                )
            self.write_revoked_lifecycle("direct-onboard")
            return self.completed(argv, stdout_evidence)
        if identity in {"d2a_auth_smoke", "d2a_one_shot"}:
            operation = "auth-smoke" if identity == "d2a_auth_smoke" else "one-shot"
            result_directory = self.fixture.d2a_root / f"result-{operation}"
            result_directory.mkdir(mode=0o700, parents=True, exist_ok=True)
            record_path = write_file(
                result_directory / "final.json",
                {"operation": operation, "release_eligible": False},
                0o600,
            )
            self.write_revoked_lifecycle(operation)
            return self.completed(
                argv,
                {
                    "status": "passed",
                    "release_eligible": False,
                    "result": str(record_path),
                    "evidence_sha256": "e" * 64,
                },
            )
        if identity == "d2a_verify":
            return self.completed(argv, {"status": "verified", "release_eligible": False})
        if identity == "teardown_discord_resources":
            return self.completed(
                argv,
                {
                    "status": "torn_down",
                    "phase": "candidate_started",
                    "transport_instance_id": "d2ti-" + "a" * 32,
                    "inventory_digest_sha256": "b" * 64,
                    "resource_count": 0,
                    "all_resources_absent": True,
                    "evidence": str(self.fixture.manifest_path.with_name("discord-teardown.json")),
                },
            )
        if identity == "cleanup":
            return self.completed(
                argv,
                {
                    "status": "cleaned",
                    "phase": "cleaned",
                    "database_absent": True,
                    "postgres_process_absent": True,
                    "launchd_jobs_absent": True,
                    "keychain_items_absent": True,
                    "isolated_root_absent": True,
                    "protected_staging_unchanged": True,
                },
            )
        if identity == "recover_audited_quarantined_no_issue":
            return self.completed(
                argv,
                {
                    "status": "exact_replay"
                    if self.audited_recovery_tamper == "replay"
                    else "recovered",
                    "phase": "cleaned",
                    "run_id": self.fixture.run_id,
                    "manifest_sha256": self.fixture.manifest_sha256,
                    "intent": "fixture",
                    "source_transition": "fixture",
                    "source_transition_sha256": "9" * 64,
                    "reconciliation": "fixture",
                    "database_absence_evidence": "fixture",
                    "evidence": "fixture",
                    "transport_instance_id": "d2ti-" + "a" * 32,
                    "transport_inventory_sha256": "b" * 64,
                    "database_absence_sha256": "c" * 64,
                    "database_absence": {},
                    "database_absent": True,
                    "postgres_process_absent": True,
                    "launchd_jobs_absent": True,
                    "keychain_items_absent": True,
                    "isolated_root_absent": True,
                    "protected_staging_unchanged": True,
                    "source_drift_observed": True,
                    "cleanup_status": "already_cleaned"
                    if self.audited_recovery_tamper == "replay"
                    else "cleaned",
                    "postconditions": {
                        "status": "observed",
                        "phase": "cleaned",
                        "postgres_running": False,
                        "candidate_launchd_jobs_loaded": 0,
                        "candidate_launchd_overrides_absent": True,
                        "protected_staging_unchanged": True,
                    },
                },
            )
        if identity == "status":
            return self.completed(
                argv,
                {
                    "status": "observed",
                    "phase": "cleaned",
                    "postgres_running": False,
                    "candidate_launchd_jobs_loaded": 0,
                    "candidate_launchd_overrides_absent": (
                        self.candidate_launchd_overrides_absent
                    ),
                    "protected_staging_unchanged": True,
                },
            )
        raise AssertionError(identity)


class BootstrapFixture:
    def __init__(self, testcase):
        temporary_parent = pathlib.Path("/private/tmp") if pathlib.Path("/private/tmp").is_dir() else None
        self.temporary = tempfile.TemporaryDirectory(dir=temporary_parent)
        self.root = pathlib.Path(self.temporary.name).resolve()
        self.tool_root = self.root / "tools" / "d2-maintenance"
        self.certification_root = self.root / "tools" / "d2-certification"
        self.rust_toolchain_bin = self.root / "rust-toolchain" / "bin"
        self.git_path = write_file(self.root / "system" / "git", b"fixture-git", 0o555)
        self.lock_path = self.root / "bootstrap.lock"
        self.issuer_lock_path = self.root / "issuer-global-d2.lock"
        self.release_root = self.root / "release-runs"
        self.d2a_root = self.root / "d2a-runs"
        self.state_root = self.root / "bootstrap-states"
        self.manifest_path = None
        self.manifest_sha256 = None
        self.run_id = None
        self.resource_prefix = None
        self.create_tools()
        self.dependencies = self.create_dependencies()
        self.candidates = self.create_candidates()
        self.config = self.valid_config()
        self.config_path = write_file(self.root / "sandbox.json", self.config, 0o600)
        self.provenance = self.valid_provenance()
        self.provenance_path = write_file(
            self.candidate_bundle / "provenance.json",
            self.provenance,
            0o400,
        )
        self.candidate_spec = self.valid_candidate_spec()
        self.candidate_spec_path = write_file(
            self.candidate_bundle / "candidate-spec.json",
            self.candidate_spec,
            0o400,
        )
        self.candidate_bundle.chmod(0o555)
        self.testcase = testcase

    def cleanup(self):
        if hasattr(self, "dependency_bootstrap") and self.dependency_bootstrap.exists():
            for path in sorted(
                self.dependency_bootstrap.rglob("*"),
                key=lambda item: len(item.parts),
                reverse=True,
            ):
                if path.is_dir():
                    path.chmod(0o700)
                else:
                    path.chmod(0o600)
            self.dependency_bootstrap.chmod(0o700)
        candidate_parent = self.root / "candidate-artifacts"
        if candidate_parent.exists():
            candidate_parent.chmod(0o700)
        if hasattr(self, "candidate_bundle") and self.candidate_bundle.exists():
            self.candidate_bundle.chmod(0o700)
            worker_root = self.candidate_bundle / "codex-worker"
            if worker_root.exists():
                worker_root.chmod(0o700)
        self.temporary.cleanup()

    def create_tools(self):
        self.rust_toolchain_bin.mkdir(mode=0o755, parents=True)
        write_file(self.rust_toolchain_bin / "cargo", b"fixture-cargo", 0o755)
        write_file(self.rust_toolchain_bin / "rustc", b"fixture-rustc", 0o755)
        write_file(self.tool_root / "d2a.py", "# controller\n", 0o644)
        write_file(self.tool_root / "d2a_candidate.py", "# sealed candidate builder\n", 0o644)
        write_file(self.tool_root / "headless_product_runner.mjs", "export {};\n", 0o644)
        write_file(self.tool_root / "scenarios" / "study-room.v1.json", "{}\n", 0o644)
        for relative in BOOTSTRAP.CANDIDATE_DEPENDENCY_SOURCE_INPUTS.values():
            payload = f"fixture:{relative}\n".encode()
            if relative.name == "Cargo.lock":
                payload += b"# bounded fixture entry\n" * 4096
            write_file(self.root / relative, payload, 0o644)
        issuer_root = self.tool_root / "session-issuer"
        for name in ("Cargo.toml", "Cargo.lock", "src/lib.rs", "src/main.rs"):
            write_file(issuer_root / name, f"fixture:{name}\n", 0o644)
        issuer_root.chmod(0o755)
        write_file(self.certification_root / "product_driver.js", "export {};\n", 0o644)
        for name in ("d2_certification.py", "d2_preflight_evidence.py", "isolated_orchestrator.py"):
            write_file(self.certification_root / name, "# fixture\n", 0o644)

    def create_candidates(self):
        parent = self.root / "candidate-artifacts"
        parent.mkdir(mode=0o700)
        self.candidate_bundle = parent / "candidate-fixture"
        self.candidate_bundle.mkdir(mode=0o700)
        worker_root = self.candidate_bundle / "codex-worker"
        worker_root.mkdir(mode=0o700)
        values = {}
        for name in sorted(BOOTSTRAP.CODEX_WORKER_FILES):
            write_file(worker_root / name, f"worker:{name}\n", 0o444)
        worker_root.chmod(0o555)
        for name in sorted(BOOTSTRAP.D2A.CANDIDATE_KEYS):
            path = self.candidate_bundle / BOOTSTRAP.CANDIDATE_RELATIVE_PATHS[name]
            if name != "codex_worker":
                write_file(path, f"candidate:{name}\n", 0o555)
            values[name] = str(path)
        return values

    @staticmethod
    def identity(path):
        path = pathlib.Path(path)
        metadata = path.stat()
        return {
            "path": str(path),
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "size": metadata.st_size,
            "mode": stat.S_IMODE(metadata.st_mode),
            "uid": metadata.st_uid,
            "links": metadata.st_nlink,
        }

    def create_dependencies(self):
        state_root = self.root / "d3-state"
        state_root.mkdir(mode=0o700)
        bootstrap = state_root / "gate-bootstrap"
        bootstrap.mkdir(mode=0o700)
        vendor = bootstrap / "vendor"
        vendor.mkdir(mode=0o700)
        workspace_vendor = vendor / "workspace"
        transport_vendor = vendor / "transport"
        workspace_vendor.mkdir(mode=0o700)
        transport_vendor.mkdir(mode=0o700)
        workspace_config = write_file(
            bootstrap / "native-cargo-config.toml",
            BOOTSTRAP.workspace_dependency_cargo_configuration(bootstrap),
            0o400,
        )
        transport_config = write_file(
            bootstrap / "native-transport-cargo-config.toml",
            BOOTSTRAP.transport_dependency_cargo_configuration(bootstrap),
            0o400,
        )
        workspace_vendor.chmod(0o500)
        transport_vendor.chmod(0o500)
        vendor.chmod(0o500)
        bootstrap.chmod(0o555)
        self.dependency_bootstrap = bootstrap
        tree = BOOTSTRAP.candidate_dependency_tree_identity(bootstrap)
        record = {
            "schema_version": 1,
            "kind": "starring.d3.gate-bootstrap.v1",
            "gate_runtime_sha256": "e" * 64,
            **tree,
        }
        record["record_sha256"] = hashlib.sha256(
            BOOTSTRAP.canonical_json(record).encode("utf-8")
        ).hexdigest()
        record_path = write_file(state_root / "gate-bootstrap.json", record, 0o600)
        return {
            "schema_version": 1,
            "kind": BOOTSTRAP.CANDIDATE_DEPENDENCY_SNAPSHOT_KIND,
            "bootstrap_root": str(bootstrap),
            "record": self.identity(record_path),
            "gate_runtime_sha256": record["gate_runtime_sha256"],
            "record_sha256": record["record_sha256"],
            "tree_sha256": record["tree_sha256"],
            "entries": record["entries"],
            "total_bytes": record["total_bytes"],
            "workspace": {
                "vendor_root": str(workspace_vendor),
                "cargo_config": self.identity(workspace_config),
            },
            "transport": {
                "vendor_root": str(transport_vendor),
                "cargo_config": self.identity(transport_config),
            },
            "source_inputs": {
                name: self.identity(self.root / relative)
                for name, relative in BOOTSTRAP.CANDIDATE_DEPENDENCY_SOURCE_INPUTS.items()
            },
        }

    def valid_config(self):
        return {
            "schema_version": 1,
            "kind": BOOTSTRAP.CONFIG_KIND,
            "sandbox_id": "macmini-d2a",
            "guild_lifecycle": "persistent_reuse_no_delete_v1",
            "discord": {
                "guild_id": "1536845588954353676",
                "hub_channel_id": "1536845619266846792",
                "application_id": "1533144492293754900",
                "bot_user_id": "1533144492293754900",
                "actor_id": "1056857223529250906",
                "actor_display_name": "보건",
            },
            "credential_refs": {
                "discord_oauth": "starring.d2.credentials:discord.oauth-client-secret",
                "discord_bot": "starring.d2.credentials:discord.bot-token",
                "cloudflare_tunnel": "starring.d2.credentials:cloudflare.tunnel-token",
            },
            "cloudflare": {
                "tunnel_id": "57c22e8a-0ec2-4f67-a882-2c355b0348df",
                "public_origin": "https://d2-api.starring.co.kr",
            },
            "ports": {
                "postgres": 55433,
                "api": 28080,
                "runtime": 29091,
                "worker": 28181,
                "transport_gateway": 29101,
                "transport_http": 29102,
            },
            "release_run_root": str(self.release_root),
            "d2a_result_root": str(self.d2a_root),
            "bootstrap_state_root": str(self.state_root),
        }

    def valid_candidate_spec(self):
        return {
            "schema_version": BOOTSTRAP.CANDIDATE_SCHEMA_VERSION,
            "kind": BOOTSTRAP.CANDIDATE_KIND,
            "commit_sha": "c" * 40,
            "source_tree_sha": "d" * 40,
            "bundle": str(self.candidate_bundle),
            "provenance_sha256": hashlib.sha256(self.provenance_path.read_bytes()).hexdigest(),
            "candidates": {
                name: {
                    "path": path,
                    "sha256": hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest(),
                }
                for name, path in self.candidates.items()
            },
        }

    def valid_provenance(self):
        records = {
            name: {
                "source": self.identity(path),
                "artifact": self.identity(path),
            }
            for name, path in self.candidates.items()
        }
        worker_files = {
            name: {
                "source": self.identity(self.candidate_bundle / "codex-worker" / name),
                "artifact": self.identity(self.candidate_bundle / "codex-worker" / name),
            }
            for name in BOOTSTRAP.CODEX_WORKER_FILES
        }
        # Keep substitutions concrete and deterministic while retaining the
        # production `.build-d2ac-*` shape.
        build_root = self.candidate_bundle.parent / (".build-d2ac-" + "a" * 32)
        workspace_target = build_root / "workspace-target"
        transport_target = build_root / "transport-target"
        cargo = str(self.rust_toolchain_bin / "cargo")
        commands = [
            [cargo, "--config", self.dependencies["workspace"]["cargo_config"]["path"],
             "build", "--frozen", "--release", "--target-dir", str(workspace_target),
             "-p", package, "--bin", binary]
            for package, binary in (
                ("starring-api", "starring-api"),
                ("starring-runtime", "starring-runtime"),
                ("starring-db-bootstrap", "starring-d2-db-bootstrap"),
                ("starring-staging-provisioner", "starring-d2-sealed-provisioner"),
            )
        ]
        commands.append([
            cargo, "--config", self.dependencies["transport"]["cargo_config"]["path"],
            "build", "--frozen", "--release", "--manifest-path",
            "tools/d2-certification-transport/Cargo.toml", "--target-dir",
            str(transport_target),
        ])
        darwin = self.fixture_darwin_toolchain()
        empty_manifest_digest = hashlib.sha256(b"[]").hexdigest()
        cargo_identity = {**self.identity(self.rust_toolchain_bin / "cargo"),
                          "version": "cargo 1.97.0 (fixture)"}
        rustc_identity = {**self.identity(self.rust_toolchain_bin / "rustc"),
                          "version": "rustc 1.97.0 (fixture)"}
        return {
            "schema_version": 1,
            "kind": BOOTSTRAP.CANDIDATE_PROVENANCE_KIND,
            "status": "built",
            "release_eligible": False,
            "commercial_certification": False,
            "source": {
                "root": str(self.root),
                "commit": "c" * 40,
                "tree": "d" * 40,
                "clean": True,
                "git": self.identity(self.git_path),
            },
            "commands": commands,
            "dependencies": self.dependencies,
            "environment": {
                "HOME": str(pathlib.Path.home()),
                "PATH": f"{self.rust_toolchain_bin}:/usr/bin:/bin:/usr/sbin:/sbin",
                "RUSTC": str(self.rust_toolchain_bin / "rustc"),
                "CC": "/fixture/clang",
                "CXX": "/fixture/clang",
                "AR": "/fixture/ar",
                "RANLIB": "/fixture/ranlib",
                "SDKROOT": "/fixture/sdk",
                "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER": "/fixture/clang",
                "CARGO_HOME": str(build_root / "cargo-home"),
                "CARGO_INCREMENTAL": "0",
                "CARGO_BUILD_JOBS": "1",
                "CARGO_NET_OFFLINE": "true",
                "GIT_TERMINAL_PROMPT": "0",
                "GIT_CONFIG_NOSYSTEM": "1",
                "LC_ALL": "C",
                "STARRING_RUNTIME_BUILD_REVISION": "c" * 40,
                "TMPDIR": str(build_root / "tmp"),
            },
            "toolchain": {
                "target": "aarch64-apple-darwin",
                "root": str(self.rust_toolchain_bin),
                "rustc_verbose_version": ["rustc 1.97.0"],
                "tools": {"cargo": cargo_identity, "rustc": rustc_identity},
                "sysroot_manifest": {
                    "sysroot": str(self.root / "rust-toolchain"),
                    "files": [],
                    "sha256": empty_manifest_digest,
                    "linkers": [],
                    "linkers_sha256": empty_manifest_digest,
                },
                "darwin": darwin,
            },
            "artifacts": {
                name: records[name]
                for name in BOOTSTRAP.CANDIDATE_ARTIFACT_NAMES
            },
            "worker": {"tree_sha256": "b" * 64, "files": worker_files},
            "operators": {
                name: records[name]
                for name in BOOTSTRAP.CANDIDATE_OPERATOR_NAMES
            },
            "bundle": str(self.candidate_bundle),
            "builder": self.identity(self.tool_root / "d2a_candidate.py"),
            "built_at": "2026-08-12T00:00:00Z",
        }

    def controller(self, executor):
        return BOOTSTRAP.BootstrapController(
            executor=executor,
            tool_root=self.tool_root,
            certification_root=self.certification_root,
            lock_path=self.lock_path,
            rust_toolchain_bin=self.rust_toolchain_bin,
            expected_release_root=self.release_root,
            source_root=self.root,
            git_path=self.git_path,
            darwin_toolchain_provider=self.fixture_darwin_toolchain,
        )

    @staticmethod
    def fixture_darwin_toolchain():
        tools = {
            name: {
                "selected_path": f"/fixture/{name}",
                "selected_link_target": None,
                "resolved_path": f"/fixture/{name}",
                "sha256": hashlib.sha256(name.encode()).hexdigest(),
                "size": len(name),
                "mode": 0o755,
            }
            for name in ("clang", "ld", "ar", "ranlib", "otool")
        }
        # CommandLineTools exposes `otool` as a selector whose resolved binary
        # is named `llvm-otool`; the subprocess allowlist must accept the
        # already sealed resolved identity used by the real bootstrap.
        tools["otool"]["resolved_path"] = "/fixture/llvm-otool"
        return {
            "fixed_tools": {
                name: {
                    "path": f"/usr/bin/{name.replace('_', '-')}",
                    "sha256": hashlib.sha256(("fixed:" + name).encode()).hexdigest(),
                    "size": len(name) + 1,
                    "mode": 0o755,
                    "uid": 0,
                    "links": 1,
                }
                for name in ("xcrun", "xcode_select", "sw_vers")
            },
            "developer_root": "/fixture",
            "selected_tools": tools,
            "sdk": {
                "root": "/fixture/sdk",
                "root_identity": {"path": "/fixture/sdk", "mode": 0o755, "uid": 0},
                "ancestors": [
                    {"path": "/fixture/sdk", "mode": 0o755, "uid": 0},
                    {"path": "/fixture", "mode": 0o755, "uid": 0},
                    {"path": "/", "mode": 0o755, "uid": 0},
                ],
                "entries": 1,
                "sha256": "f" * 64,
                "selected_path": "/fixture/sdk",
                "selected_link_target": None,
            },
            "os_build_version": "fixture",
        }

    def create_manifest(self, argv):
        self.run_id = flag(argv, "--run-id")
        self.resource_prefix = f"starring-d2-{self.run_id[3:11]}-{self.run_id.rsplit('-', 1)[1]}"
        self.manifest_path = self.release_root / self.run_id / "manifest.json"
        suffix = self.run_id.rsplit("-", 1)[1]
        candidate_entries = {
            name: {
                "path": path,
                "sha256": hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest(),
            }
            for name, path in self.candidates.items()
        }
        credentials = self.config["credential_refs"]
        manifest = {
            "schema_version": 1,
            "certification_class": "commercial_human_v1",
            "run_id": self.run_id,
            "created_at": "2026-08-12T00:00:00Z",
            "commit_sha": self.candidate_spec["commit_sha"],
            "authoring": {},
            "public_origin": self.config["cloudflare"]["public_origin"],
            "cloudflare": {},
            "candidates": candidate_entries,
            "source_trees": {
                "certification_transport": {
                    "files": [],
                    "root": str(self.certification_root.parent / "d2-certification-transport"),
                    "sha256": "1" * 64,
                },
                "codex_worker": {
                    "files": sorted(BOOTSTRAP.CODEX_WORKER_FILES),
                    "root": str(self.candidate_bundle / "codex-worker"),
                    "sha256": "2" * 64,
                },
                "d2_toolchain": {
                    "files": [],
                    "root": str(self.certification_root),
                    "sha256": "3" * 64,
                },
            },
            "database": {
                "name": "starring_runtime_staging",
                "cluster_root": f"/private/tmp/starring-d2-{self.run_id}/postgres",
                "socket_directory": f"/private/tmp/starring-d2-{self.run_id}/socket",
                "port": self.config["ports"]["postgres"],
            },
            "discord": {
                **{key: self.config["discord"][key] for key in (
                    "guild_id", "hub_channel_id", "application_id", "bot_user_id", "actor_id"
                )},
                "resource_prefix": self.resource_prefix,
                "disposable_guild_required": True,
            },
            "services": {
                "api": {"label": f"local.starring.d2.{suffix}.api", "port": self.config["ports"]["api"]},
                "runtime": {"label": f"local.starring.d2.{suffix}.runtime", "port": self.config["ports"]["runtime"]},
                "worker": {"label": f"local.starring.d2.{suffix}.worker", "port": self.config["ports"]["worker"]},
                "transport": {
                    "label": f"local.starring.d2.{suffix}.transport",
                    "gateway_port": self.config["ports"]["transport_gateway"],
                    "http_port": self.config["ports"]["transport_http"],
                },
                "tunnel": {"label": f"local.starring.d2.{suffix}.tunnel"},
            },
            "keychain_services": {
                name: f"starring.d2.{suffix}.{name}"
                for name in ("api", "runtime", "postgres", "worker")
            },
            "external_keychain": {
                "discord_oauth_client_secret": BOOTSTRAP.split_keychain_ref(credentials["discord_oauth"]),
                "discord_bot_token": BOOTSTRAP.split_keychain_ref(credentials["discord_bot"]),
                "tunnel_token": BOOTSTRAP.split_keychain_ref(credentials["cloudflare_tunnel"]),
            },
            "protected_staging": {"mutation_allowed": False},
            "human_boundaries": [
                "create_disposable_discord_guild",
                "complete_discord_oauth",
                "confirm_product_preview",
                "execute_real_discord_interactions",
                "confirm_replacement_preview",
                "delete_disposable_discord_guild",
            ],
            "expected_steps": [],
        }
        payload = (BOOTSTRAP.canonical_json(manifest) + "\n").encode()
        self.manifest_sha256 = hashlib.sha256(BOOTSTRAP.canonical_json(manifest).encode()).hexdigest()
        write_file(self.manifest_path, payload, 0o600)
        write_file(self.manifest_path.with_name("manifest.sha256"), self.manifest_sha256 + "\n", 0o600)
        write_file(self.manifest_path.with_name("receipts.jsonl"), b"", 0o600)
        self.manifest_path.parent.chmod(0o700)

    def direct_onboarding_evidence(self):
        issuer = (
            self.tool_root
            / "session-issuer"
            / "target"
            / "release"
            / "starring-d2-session-issuer"
        )
        return {
            "schema_version": 1,
            "kind": "starring.d2a.direct-onboarding-evidence.v1",
            "certification_class": BOOTSTRAP.D2A.AUTOMATED_CLASS,
            "operation": "direct-onboard",
            "observed_at": "2026-08-12T01:02:03.123456789Z",
            "run_id": self.run_id,
            "manifest_sha256": self.manifest_sha256,
            "principal_id": f"discord:{self.config['discord']['actor_id']}",
            "guild_id": self.config["discord"]["guild_id"],
            "discord_application_id": self.config["discord"]["application_id"],
            "hub_channel_id": self.config["discord"]["hub_channel_id"],
            "binding_key": "community_hub",
            "installation_id": f"installation:{self.resource_prefix}",
            "outcome": "fresh",
            "provisioner_sha256": self.candidate_spec["candidates"]["sealed_provisioner"]["sha256"],
            "issuer_sha256": hashlib.sha256(issuer.read_bytes()).hexdigest(),
            "issuer_source_sha256": BOOTSTRAP.D2A.issuer_source_sha256(self.tool_root),
            "discord_hub_preflight": True,
            "direct_auth_used": True,
            "session_revoked": True,
            "release_eligible": False,
        }


class D2ABootstrapTests(unittest.TestCase):
    def setUp(self):
        self.fixture = BootstrapFixture(self)

    def tearDown(self):
        self.fixture.cleanup()

    def create_quarantined_direct_onboard_failure(self):
        executor = FakeExecutor(
            self.fixture,
            quarantined_direct_onboard=True,
        )
        result = self.fixture.controller(executor).run(
            self.fixture.config_path,
            self.fixture.candidate_spec_path,
            "auth-smoke",
        )
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["error_code"], "manual_recovery_required")
        state_path = pathlib.Path(result["state"])
        state = json.loads(state_path.read_bytes())
        self.assertEqual(state["status"], "recovery_required")
        self.assertEqual(state["phase"], "direct_onboard")
        self.assertEqual(state["last_error"], "direct_onboard_failed")
        self.assertTrue(state["candidate_started"])
        self.assertFalse(state["discord_teardown_complete"])
        self.assertFalse(state["cleanup_complete"])
        self.assertFalse(state["postconditions_complete"])
        return state_path, state

    def audited_incident_overrides(self, state_path, state):
        lifecycle = self.fixture.manifest_path.with_name(
            "d2a-session-lifecycle.json"
        )
        return {
            **BOOTSTRAP.AUDITED_QUARANTINED_INCIDENT,
            "run_id": state["run_id"],
            "manifest_sha256": state["manifest_sha256"],
            "bootstrap_state_sha256": hashlib.sha256(
                state_path.read_bytes()
            ).hexdigest(),
            "bootstrap_state_semantic_sha256": (
                BOOTSTRAP.audited_recovery_bootstrap_semantic_sha256(state)
            ),
            "lifecycle_sha256": hashlib.sha256(
                lifecycle.read_bytes()
            ).hexdigest(),
            "transport_instance_id": "d2ti-" + "a" * 32,
            "empty_transport_inventory_sha256": "b" * 64,
        }

    def audited_recovery_output(self, state):
        paths = BOOTSTRAP.audited_recovery_artifact_paths(state)
        return {
            "status": "recovered",
            "phase": "cleaned",
            "run_id": state["run_id"],
            "manifest_sha256": state["manifest_sha256"],
            "intent": str(paths["intent"]),
            "source_transition": str(paths["source_transition"]),
            "source_transition_sha256": "9" * 64,
            "reconciliation": str(paths["reconciliation"]),
            "database_absence_evidence": str(paths["database_absence"]),
            "evidence": str(paths["evidence"]),
            "transport_instance_id": "d2ti-" + "a" * 32,
            "transport_inventory_sha256": "b" * 64,
            "database_absence_sha256": "c" * 64,
            "database_absence": {},
            "database_absent": True,
            "postgres_process_absent": True,
            "launchd_jobs_absent": True,
            "keychain_items_absent": True,
            "isolated_root_absent": True,
            "protected_staging_unchanged": True,
            "source_drift_observed": True,
            "cleanup_status": "cleaned",
            "postconditions": {
                "status": "observed",
                "phase": "cleaned",
                "postgres_running": False,
                "candidate_launchd_jobs_loaded": 0,
                "candidate_launchd_overrides_absent": True,
                "protected_staging_unchanged": True,
            },
        }

    def write_audited_recovery_artifacts(self, state_path, state):
        incident = self.audited_incident_overrides(state_path, state)
        artifact_root = self.fixture.manifest_path.parent / "orchestrator"
        historical_inputs = {
            "candidate_start_transition_sha256": artifact_root
            / "candidate-start-transition.json",
            "database_evidence_sha256": artifact_root / "database-evidence.json",
            "step_03_evidence_sha256": artifact_root / "step-03-evidence.json",
        }
        for field, path in historical_inputs.items():
            write_file(path, {"fixture": field}, 0o600)
            incident[field] = hashlib.sha256(path.read_bytes()).hexdigest()
        write_file(
            self.fixture.manifest_path.with_name("receipts.jsonl"), b"", 0o600
        )
        coordinator_lock = (
            self.fixture.manifest_path.parent
            / "coordinator"
            / "coordinator.lock"
        )
        write_file(coordinator_lock, b"", 0o600)
        coordinator_root = artifact_root / "coordinator-sources"
        coordinator_digests = {}
        for name in (
            "step-01-bootstrap.json",
            "step-02-prior-absence.json",
            "step-03-candidate.json",
        ):
            path = write_file(coordinator_root / name, {"fixture": name}, 0o600)
            coordinator_digests[name] = hashlib.sha256(path.read_bytes()).hexdigest()
        incident["receipts_sha256"] = hashlib.sha256(b"").hexdigest()
        incident["coordinator_lock_sha256"] = hashlib.sha256(b"").hexdigest()
        incident["coordinator_source_sha256"] = coordinator_digests
        plist_digests = {}
        suffix = state["run_id"].rsplit("-", 1)[1]
        for name in ("api", "runtime", "transport", "tunnel", "worker"):
            path = write_file(
                artifact_root
                / "launchd"
                / f"local.starring.d2.{suffix}.{name}.plist",
                {"fixture": name},
                0o600,
            )
            plist_digests[name] = hashlib.sha256(path.read_bytes()).hexdigest()
        tunnel_script = write_file(
            artifact_root / "run-tunnel.zsh", b"fixture tunnel\n", 0o700
        )
        incident["plist_sha256"] = plist_digests
        incident["tunnel_script_sha256"] = hashlib.sha256(
            tunnel_script.read_bytes()
        ).hexdigest()
        taint = self.fixture.manifest_path.with_name("d2a-taint.json")
        incident["taint_sha256"] = hashlib.sha256(taint.read_bytes()).hexdigest()

        manifest = json.loads(self.fixture.manifest_path.read_bytes())
        journal_rows = []
        for sequence in range(1, incident["journal_rows"] + 1):
            journal_rows.append({
                "schema_version": 1,
                "sequence": sequence,
                "manifest_sha256": state["manifest_sha256"],
                "action": (
                    "candidate_start"
                    if sequence == incident["journal_rows"] else "fixture"
                ),
                "status": (
                    "rolled_back"
                    if sequence == incident["journal_rows"] else "complete"
                ),
                "target": "run",
                "recorded_at": "2026-08-12T08:00:00Z",
            })
        baseline_raw = b"".join(
            (BOOTSTRAP.canonical_json(row) + "\n").encode("utf-8")
            for row in journal_rows
        )
        incident["journal_sha256"] = hashlib.sha256(baseline_raw).hexdigest()
        cleanup_rows = journal_rows + [
            {
                "schema_version": 1, "sequence": incident["journal_rows"] + 1,
                "manifest_sha256": state["manifest_sha256"],
                "action": "cleanup", "status": "intent", "target": "run",
                "recorded_at": "2026-08-12T09:00:02Z",
            },
            {
                "schema_version": 1, "sequence": incident["journal_rows"] + 2,
                "manifest_sha256": state["manifest_sha256"],
                "action": "cleanup", "status": "failed", "target": "run",
                "recorded_at": "2026-08-12T09:00:03Z",
            },
        ]
        cleanup_raw = b"".join(
            (BOOTSTRAP.canonical_json(row) + "\n").encode("utf-8")
            for row in cleanup_rows
        )
        write_file(artifact_root / "lifecycle.jsonl", cleanup_raw, 0o600)
        incident["cleanup_journal_rows"] = len(cleanup_rows)
        incident["cleanup_journal_sha256"] = hashlib.sha256(cleanup_raw).hexdigest()
        cleanup_root_identity = {
            "schema_version": 1,
            "kind": "starring.d2.cleanup-root-identity.v1",
            "manifest_sha256": state["manifest_sha256"],
            "run_id": state["run_id"],
            "root_path": str(
                pathlib.Path(manifest["database"]["cluster_root"]).parent
            ),
            "root_device": 1,
            "root_inode": 2,
            "parent_device": 1,
            "owner_uid": os.getuid(),
        }
        cleanup_root_identity_path = write_file(
            artifact_root / "cleanup-root-identity.json",
            cleanup_root_identity,
            0o600,
        )
        incident["cleanup_root_identity_sha256"] = hashlib.sha256(
            cleanup_root_identity_path.read_bytes()
        ).hexdigest()
        cleanup_keychain_inventory = [
            {
                "service": service,
                "account": account,
                "identity_sha256": hashlib.sha256(
                    f"{service}:{account}".encode("utf-8")
                ).hexdigest(),
            }
            for service, account in BOOTSTRAP.audited_recovery_keychain_inventory(
                manifest
            )
        ]
        incident["keychain_inventory_sha256"] = hashlib.sha256(
            BOOTSTRAP.canonical_json(cleanup_keychain_inventory).encode("utf-8")
        ).hexdigest()
        incident["keychain_item_count"] = len(cleanup_keychain_inventory)
        incident["keychain_anchor_policy_kind"] = (
            BOOTSTRAP.AUDITED_QUARANTINED_KEYCHAIN_ANCHOR_POLICY_KIND
        )
        incident["keychain_anchor_inventory_sha256"] = (
            BOOTSTRAP.audited_recovery_keychain_anchor_inventory_sha256()
        )
        incident["keychain_anchor_item_count"] = len(
            BOOTSTRAP.AUDITED_QUARANTINED_KEYCHAIN_ANCHORS
        )
        historical_source_trees = {
            name: record["sha256"]
            for name, record in manifest["source_trees"].items()
        }
        observations = {
            name: {
                "root": record["root"],
                "historical_sha256": record["sha256"],
                "observed_sha256": record["sha256"]
                if name == "codex_worker"
                else ("4" if name == "d2_toolchain" else "5") * 64,
                "matches_historical": name == "codex_worker",
            }
            for name, record in manifest["source_trees"].items()
        }
        audit_tools = {
            name: incident[name]
            for name in (
                "zsh_sha256",
                "security_sha256",
                "psql_sha256",
                "static_sql_sha256",
            )
        }
        paths = BOOTSTRAP.audited_recovery_artifact_paths(state)
        intent = {
            "schema_version": 1,
            "kind": BOOTSTRAP.AUDITED_QUARANTINED_RECOVERY_KINDS["intent"],
            "run_id": state["run_id"],
            "manifest_sha256": state["manifest_sha256"],
            "bootstrap_id": state["bootstrap_id"],
            "bootstrap_state_path": str(state_path),
            "baseline_bootstrap_state_sha256": incident["bootstrap_state_sha256"],
            "historical_manifest_commit_sha": manifest["commit_sha"],
            "historical_source_trees": historical_source_trees,
            "current_source": {
                "repository_root": str(self.fixture.root),
                "commit_sha": "a" * 40,
                "tree_sha": "b" * 40,
                "git_path": str(BOOTSTRAP.FIXED_GIT),
                "git_sha256": "6" * 64,
                "source_trees": observations,
            },
            "orchestrator_state_sha256": incident["orchestrator_state_sha256"],
            "baseline_journal_sha256": incident["journal_sha256"],
            "baseline_journal_rows": incident["journal_rows"],
            "taint_sha256": incident["taint_sha256"],
            "lifecycle_sha256": incident["lifecycle_sha256"],
            "candidate_start_transition_sha256": incident[
                "candidate_start_transition_sha256"
            ],
            "database_evidence_sha256": incident["database_evidence_sha256"],
            "step_03_evidence_sha256": incident["step_03_evidence_sha256"],
            "transport_instance_id": incident["transport_instance_id"],
            "pre_intent_transport_inventory_sha256": incident[
                "empty_transport_inventory_sha256"
            ],
            "effect_admission_operation_id": (
                f"audited-quarantine-recovery:{state['run_id'].rsplit('-', 1)[1]}"
            ),
            "database_system_identifier": incident["database_system_identifier"],
            "safe_after": incident["safe_after"],
            "service_identities": BOOTSTRAP.AUDITED_QUARANTINED_SERVICE_IDENTITIES,
            "audit_tools": audit_tools,
            "receipts_sha256": incident["receipts_sha256"],
            "coordinator_lock_sha256": incident["coordinator_lock_sha256"],
            "coordinator_source_sha256": incident[
                "coordinator_source_sha256"
            ],
            "plist_sha256": incident["plist_sha256"],
            "tunnel_script_sha256": incident["tunnel_script_sha256"],
            "created_at": "2026-08-12T09:00:00Z",
        }
        write_file(paths["intent"], intent, 0o600)
        intent_sha = hashlib.sha256(paths["intent"].read_bytes()).hexdigest()
        incident["intent_sha256"] = intent_sha
        incident["intent_source_commit_sha"] = "a" * 40
        incident["intent_source_tree_sha"] = "b" * 40
        incident["intent_source_sha256"] = hashlib.sha256(
            BOOTSTRAP.canonical_json(intent["current_source"]).encode("utf-8")
        ).hexdigest()

        transition_v1_to_source = copy.deepcopy(intent["current_source"])
        transition_v1_to_source["commit_sha"] = "1" * 40
        transition_v1_to_source["tree_sha"] = "2" * 40
        transition_v1_audit_configuration = (
            BOOTSTRAP.audited_recovery_transition_audit_configuration()
        )
        transition_v1_to_file_sha256 = {
            path: hashlib.sha256(("v1:" + path).encode()).hexdigest()
            for path in BOOTSTRAP.AUDITED_QUARANTINED_CHANGED_PATHS
        }
        transition_v1_base = {
            "schema_version": 1,
            "kind": BOOTSTRAP.AUDITED_QUARANTINED_SOURCE_INTERLOCK_V1_KIND,
            "run_id": state["run_id"],
            "manifest_sha256": state["manifest_sha256"],
            "intent_sha256": intent_sha,
            "from_source": intent["current_source"],
            "to_source": transition_v1_to_source,
            "parent_commit_sha": "a" * 40,
            "parent_count": 1,
            "changed_paths": BOOTSTRAP.AUDITED_QUARANTINED_CHANGED_PATHS,
            "file_sha256": {
                path: {
                    "from_sha256": BOOTSTRAP.AUDITED_QUARANTINED_CHANGED_PATH_FROM_SHA256[path],
                    "to_sha256": transition_v1_to_file_sha256[path],
                }
                for path in BOOTSTRAP.AUDITED_QUARANTINED_CHANGED_PATHS
            },
            "reason_codes": BOOTSTRAP.AUDITED_QUARANTINED_REASON_CODES_V1,
            "audit_configuration": transition_v1_audit_configuration,
            "bootstrap_state_semantic_sha256": incident[
                "bootstrap_state_semantic_sha256"
            ],
            "orchestrator_state_sha256": incident["orchestrator_state_sha256"],
            "baseline_journal_sha256": incident["journal_sha256"],
            "baseline_journal_rows": incident["journal_rows"],
            "lifecycle_sha256": incident["lifecycle_sha256"],
            "teardown_fence_sha256": incident["closing_teardown_fence_sha256"],
            "transport_instance_id": incident["transport_instance_id"],
            "transport_inventory_sha256": incident[
                "empty_transport_inventory_sha256"
            ],
            "effect_admission_operation_id": intent[
                "effect_admission_operation_id"
            ],
            "producer_launchd_jobs_absent": True,
            "issuer_process_group_absent": True,
            "transport_identity_verified": True,
            "transport_effect_admission_drained": True,
            "postgres_running": True,
            "protected_staging_unchanged": True,
            "database_absence_marker_absent": True,
            "reconciliation_marker_absent": True,
            "recovery_evidence_absent": True,
            "created_at": "2026-08-12T09:00:00Z",
        }
        transition_v1_interlock_path = (
            BOOTSTRAP.audited_recovery_source_interlock_v1_path(state)
        )
        write_file(transition_v1_interlock_path, transition_v1_base, 0o600)
        transition_v1_interlock_sha = hashlib.sha256(
            transition_v1_interlock_path.read_bytes()
        ).hexdigest()
        transition_v1 = {
            **transition_v1_base,
            "kind": BOOTSTRAP.AUDITED_QUARANTINED_SOURCE_TRANSITION_V1_KIND,
            "interlock_sha256": transition_v1_interlock_sha,
        }
        transition_v1_path = BOOTSTRAP.audited_recovery_source_transition_v1_path(
            state
        )
        write_file(transition_v1_path, transition_v1, 0o600)
        transition_v1_sha = hashlib.sha256(
            transition_v1_path.read_bytes()
        ).hexdigest()
        incident.update({
            "source_transition_v1_interlock_sha256": (
                transition_v1_interlock_sha
            ),
            "source_transition_v1_sha256": transition_v1_sha,
            "source_transition_v1_commit_sha": "1" * 40,
            "source_transition_v1_tree_sha": "2" * 40,
            "source_transition_v1_to_file_sha256": (
                transition_v1_to_file_sha256
            ),
        })

        to_source = copy.deepcopy(transition_v1_to_source)
        to_source["commit_sha"] = "c" * 40
        to_source["tree_sha"] = "d" * 40
        audit_configuration = (
            BOOTSTRAP.audited_recovery_transition_audit_configuration(
                incident["source_transition_v2_static_sql_sha256"]
            )
        )
        transition_base = {
            **transition_v1_base,
            "schema_version": 2,
            "kind": BOOTSTRAP.AUDITED_QUARANTINED_SOURCE_INTERLOCK_V2_KIND,
            "from_source": transition_v1_to_source,
            "to_source": to_source,
            "parent_commit_sha": "1" * 40,
            "file_sha256": {
                path: {
                    "from_sha256": transition_v1_to_file_sha256[path],
                    "to_sha256": hashlib.sha256(path.encode()).hexdigest(),
                }
                for path in BOOTSTRAP.AUDITED_QUARANTINED_CHANGED_PATHS
            },
            "reason_codes": BOOTSTRAP.AUDITED_QUARANTINED_REASON_CODES_V2,
            "audit_configuration": audit_configuration,
            "previous_source_transition_sha256": transition_v1_sha,
        }
        interlock_path = BOOTSTRAP.audited_recovery_source_interlock_v2_path(state)
        write_file(interlock_path, transition_base, 0o600)
        interlock_sha = hashlib.sha256(interlock_path.read_bytes()).hexdigest()
        transition = {
            **transition_base,
            "kind": BOOTSTRAP.AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_KIND,
            "interlock_sha256": interlock_sha,
        }
        transition_v2_path = BOOTSTRAP.audited_recovery_source_transition_v2_path(
            state
        )
        write_file(transition_v2_path, transition, 0o600)
        transition_sha = hashlib.sha256(
            transition_v2_path.read_bytes()
        ).hexdigest()
        transition_v2_to_file_sha256 = {
            path: transition["file_sha256"][path]["to_sha256"]
            for path in BOOTSTRAP.AUDITED_QUARANTINED_CHANGED_PATHS
        }
        incident.update({
            "source_transition_v2_interlock_sha256": interlock_sha,
            "source_transition_v2_sha256": transition_sha,
            "source_transition_v2_commit_sha": "c" * 40,
            "source_transition_v2_tree_sha": "d" * 40,
            "source_transition_v2_to_file_sha256": (
                transition_v2_to_file_sha256
            ),
        })

        database = {
            "schema_version": 1,
            "kind": BOOTSTRAP.AUDITED_QUARANTINED_RECOVERY_KINDS[
                "database_absence"
            ],
            "run_id": state["run_id"],
            "manifest_sha256": state["manifest_sha256"],
            "intent_sha256": intent_sha,
            "source_transition_sha256": transition_sha,
            "post_drain_transport_inventory_sha256": incident[
                "empty_transport_inventory_sha256"
            ],
            "observed_at": "2026-08-12T09:00:01Z",
            "database_name": "starring_runtime_staging",
            "database_system_identifier": incident["database_system_identifier"],
            "control_plane_identity": "run_owned_cluster_admin_tcp_v1",
            "topology_verified": True,
            "tables_locked": True,
            "locked_tables": BOOTSTRAP.AUDITED_QUARANTINED_LOCKED_TABLES,
            "transaction_committed": True,
            "process_group_quiescent": True,
            "oauth_flow_count": 0,
            "auth_session_count": 0,
            "principal_count": 0,
            "tenant_count": 0,
            "installation_count": 0,
            "authority_version_count": 0,
            "runtime_slot_writer_fence_count": 0,
            **audit_configuration,
            **{
                **audit_tools,
                "static_sql_sha256": audit_configuration[
                    "static_sql_sha256"
                ],
            },
        }
        write_file(paths["database_absence"], database, 0o600)
        database_sha = hashlib.sha256(
            paths["database_absence"].read_bytes()
        ).hexdigest()

        reconciliation = {
            "schema_version": 1,
            "kind": BOOTSTRAP.AUDITED_QUARANTINED_RECOVERY_KINDS[
                "reconciliation"
            ],
            "run_id": state["run_id"],
            "manifest_sha256": state["manifest_sha256"],
            "intent_sha256": intent_sha,
            "source_transition_sha256": transition_sha,
            "observed_at": "2026-08-12T09:00:02Z",
            "lifecycle_sha256": incident["lifecycle_sha256"],
            "transport_instance_id": incident["transport_instance_id"],
            "pre_intent_transport_inventory_sha256": incident[
                "empty_transport_inventory_sha256"
            ],
            "effect_admission_operation_id": intent[
                "effect_admission_operation_id"
            ],
            "effect_admission_status": "drained",
            "producer_launchd_jobs_absent": True,
            "issuer_process_group_absent": True,
            "post_drain_transport_inventory_sha256": incident[
                "empty_transport_inventory_sha256"
            ],
            "database_absence_sha256": database_sha,
            "final_transport_inventory_sha256": incident[
                "empty_transport_inventory_sha256"
            ],
            "postgres_running": True,
            "protected_staging_unchanged": True,
        }
        write_file(paths["reconciliation"], reconciliation, 0o600)
        reconciliation_sha = hashlib.sha256(
            paths["reconciliation"].read_bytes()
        ).hexdigest()
        incident["database_absence_sha256"] = database_sha
        incident["reconciliation_sha256"] = reconciliation_sha

        cleanup_to_source = copy.deepcopy(to_source)
        cleanup_to_source["commit_sha"] = "e" * 40
        cleanup_to_source["tree_sha"] = "f" * 40
        cleanup_base = {
            "schema_version": 1,
            "kind": BOOTSTRAP.AUDITED_QUARANTINED_SOURCE_INTERLOCK_KIND,
            "run_id": state["run_id"],
            "manifest_sha256": state["manifest_sha256"],
            "intent_sha256": intent_sha,
            "previous_source_transition_sha256": transition_sha,
            "database_absence_sha256": database_sha,
            "reconciliation_sha256": reconciliation_sha,
            "from_source": to_source,
            "to_source": cleanup_to_source,
            "parent_commit_sha": "c" * 40,
            "parent_count": 1,
            "changed_paths": BOOTSTRAP.AUDITED_QUARANTINED_CHANGED_PATHS,
            "file_sha256": {
                path: {
                    "from_sha256": transition_v2_to_file_sha256[path],
                    "to_sha256": hashlib.sha256(("cleanup:" + path).encode()).hexdigest(),
                }
                for path in BOOTSTRAP.AUDITED_QUARANTINED_CHANGED_PATHS
            },
            "reason_codes": BOOTSTRAP.AUDITED_QUARANTINED_REASON_CODES,
            "audit_configuration": audit_configuration,
            "bootstrap_state_semantic_sha256": incident[
                "bootstrap_state_semantic_sha256"
            ],
            "orchestrator_state_sha256": incident["orchestrator_state_sha256"],
            "baseline_journal_sha256": incident["journal_sha256"],
            "baseline_journal_rows": incident["journal_rows"],
            "lifecycle_sha256": incident["lifecycle_sha256"],
            "teardown_fence_sha256": "8" * 64,
            "cleanup_journal_sha256": incident["cleanup_journal_sha256"],
            "cleanup_journal_rows": incident["cleanup_journal_rows"],
            "keychain_inventory_sha256": incident["keychain_inventory_sha256"],
            "keychain_item_count": incident["keychain_item_count"],
            "keychain_anchor_policy_kind": incident[
                "keychain_anchor_policy_kind"
            ],
            "keychain_anchor_inventory_sha256": incident[
                "keychain_anchor_inventory_sha256"
            ],
            "keychain_anchor_item_count": incident[
                "keychain_anchor_item_count"
            ],
            "cleanup_root_identity_sha256": incident[
                "cleanup_root_identity_sha256"
            ],
            "producer_launchd_jobs_absent": True,
            "issuer_process_group_absent": True,
            "postgres_absent": True,
            "postgres_process_absent": True,
            "isolated_root_retained": True,
            "cleanup_keychain_baseline_absent": True,
            "cleanup_root_progress_absent": True,
            "cleanup_evidence_absent": True,
            "protected_staging_unchanged": True,
            "created_at": "2026-08-12T09:00:04Z",
        }
        cleanup_interlock_path = BOOTSTRAP.audited_recovery_source_interlock_path(
            state
        )
        write_file(cleanup_interlock_path, cleanup_base, 0o600)
        cleanup_interlock_sha = hashlib.sha256(
            cleanup_interlock_path.read_bytes()
        ).hexdigest()
        cleanup_transition = {
            **cleanup_base,
            "kind": BOOTSTRAP.AUDITED_QUARANTINED_RECOVERY_KINDS[
                "source_transition"
            ],
            "interlock_sha256": cleanup_interlock_sha,
        }
        write_file(paths["source_transition"], cleanup_transition, 0o600)
        cleanup_transition_sha = hashlib.sha256(
            paths["source_transition"].read_bytes()
        ).hexdigest()

        fence_path = self.fixture.manifest_path.with_name(
            "d2a-teardown-fence.json"
        )
        write_file(
            fence_path,
            {
                "schema_version": 1,
                "kind": "starring.d2a.teardown-fence.v1",
                "run_id": state["run_id"],
                "manifest_sha256": state["manifest_sha256"],
                "status": "closed",
                "updated_at": "2026-08-12T09:00:03Z",
            },
            0o600,
        )
        incident["closed_teardown_fence_sha256"] = hashlib.sha256(
            fence_path.read_bytes()
        ).hexdigest()
        cleanup_base["teardown_fence_sha256"] = incident[
            "closed_teardown_fence_sha256"
        ]
        write_file(cleanup_interlock_path, cleanup_base, 0o600)
        cleanup_interlock_sha = hashlib.sha256(
            cleanup_interlock_path.read_bytes()
        ).hexdigest()
        cleanup_transition.update({
            "teardown_fence_sha256": incident["closed_teardown_fence_sha256"],
            "interlock_sha256": cleanup_interlock_sha,
        })
        write_file(paths["source_transition"], cleanup_transition, 0o600)
        cleanup_transition_sha = hashlib.sha256(
            paths["source_transition"].read_bytes()
        ).hexdigest()
        completed_cleanup_rows = cleanup_rows + [
            {
                "schema_version": 1,
                "sequence": len(cleanup_rows) + 1,
                "manifest_sha256": state["manifest_sha256"],
                "action": "cleanup",
                "status": "intent",
                "target": "run",
                "recorded_at": "2026-08-12T09:00:04Z",
            },
            {
                "schema_version": 1,
                "sequence": len(cleanup_rows) + 2,
                "manifest_sha256": state["manifest_sha256"],
                "action": "cleanup",
                "status": "complete",
                "target": "run",
                "recorded_at": "2026-08-12T09:00:05Z",
            },
        ]
        write_file(
            artifact_root / "lifecycle.jsonl",
            b"".join(
                (BOOTSTRAP.canonical_json(row) + "\n").encode("utf-8")
                for row in completed_cleanup_rows
            ),
            0o600,
        )
        cleanup_path = artifact_root / "cleanup-evidence.json"
        cleanup = {
            "schema_version": 1,
            "manifest_sha256": state["manifest_sha256"],
            "observed_at": "2026-08-12T09:00:04Z",
            **{
                field: True
                for field in BOOTSTRAP.AUDITED_QUARANTINED_ABSENCE_FIELDS
            },
        }
        write_file(cleanup_path, cleanup, 0o600)
        cleanup_keychain_baseline_path = artifact_root / (
            "cleanup-keychain-baseline.json"
        )
        write_file(
            cleanup_keychain_baseline_path,
            {
                "schema_version": 1,
                "kind": "starring.d2.cleanup-keychain-baseline.v1",
                "manifest_sha256": state["manifest_sha256"],
                "run_id": state["run_id"],
                "inventory": cleanup_keychain_inventory,
            },
            0o600,
        )
        cleanup_root_progress_path = artifact_root / "cleanup-root-progress.json"
        cleanup_root = pathlib.Path(manifest["database"]["cluster_root"]).parent
        write_file(
            cleanup_root_progress_path,
            {
                "schema_version": 1,
                "kind": "starring.d2.cleanup-root-progress.v1",
                "manifest_sha256": state["manifest_sha256"],
                "run_id": state["run_id"],
                "root_device": cleanup_root_identity["root_device"],
                "root_inode": cleanup_root_identity["root_inode"],
                "quarantine_name": (
                    f".{cleanup_root.name}.cleanup-"
                    f"{state['manifest_sha256'][:16]}"
                ),
                "phase": "deleted",
            },
            0o600,
        )
        evidence = {
            "schema_version": 1,
            "kind": BOOTSTRAP.AUDITED_QUARANTINED_RECOVERY_KINDS["evidence"],
            "run_id": state["run_id"],
            "manifest_sha256": state["manifest_sha256"],
            "intent_sha256": intent_sha,
            "source_transition_sha256": cleanup_transition_sha,
            "reconciliation_sha256": reconciliation_sha,
            "database_absence_sha256": database_sha,
            "observed_at": "2026-08-12T09:00:05Z",
            "lifecycle_sha256": incident["lifecycle_sha256"],
            "teardown_fence_sha256": hashlib.sha256(
                fence_path.read_bytes()
            ).hexdigest(),
            "cleanup_evidence_sha256": hashlib.sha256(
                cleanup_path.read_bytes()
            ).hexdigest(),
            "cleanup_keychain_baseline_sha256": hashlib.sha256(
                cleanup_keychain_baseline_path.read_bytes()
            ).hexdigest(),
            "cleanup_root_progress_sha256": hashlib.sha256(
                cleanup_root_progress_path.read_bytes()
            ).hexdigest(),
            **{
                field: True
                for field in BOOTSTRAP.AUDITED_QUARANTINED_ABSENCE_FIELDS
            },
        }
        write_file(paths["evidence"], evidence, 0o600)
        output = self.audited_recovery_output(state)
        output["source_transition_sha256"] = cleanup_transition_sha
        output["database_absence_sha256"] = database_sha
        output["database_absence"] = {
            field: database[field]
            for field in BOOTSTRAP.AUDITED_QUARANTINED_DATABASE_PROJECTION_FIELDS
        }
        return incident, output, paths

    def test_exact_private_input_schemas_modes_and_links(self):
        _path, config, _digest = BOOTSTRAP.read_private_json(self.fixture.config_path, "sandbox_config")
        self.assertIs(BOOTSTRAP.validate_config(config), config)
        publication = BOOTSTRAP.load_candidate_publication(self.fixture.candidate_spec_path)
        _path, candidates, _digest, provenance_path, _provenance, provenance_digest = publication
        self.assertIs(BOOTSTRAP.validate_candidate_spec(candidates), candidates)
        self.assertEqual(stat.S_IMODE(self.fixture.candidate_spec_path.stat().st_mode), 0o400)
        self.assertEqual(stat.S_IMODE(provenance_path.stat().st_mode), 0o400)
        self.assertEqual(candidates["provenance_sha256"], provenance_digest)

        extra = dict(self.fixture.config)
        extra["unexpected"] = True
        extra_path = write_file(self.fixture.root / "extra.json", extra, 0o600)
        with self.assertRaises(BOOTSTRAP.BootstrapError):
            BOOTSTRAP.validate_config(BOOTSTRAP.read_private_json(extra_path, "sandbox_config")[1])

        loose_path = write_file(self.fixture.root / "loose.json", self.fixture.config, 0o644)
        with self.assertRaises(BOOTSTRAP.BootstrapError):
            BOOTSTRAP.read_private_json(loose_path, "sandbox_config")

        link_path = self.fixture.root / "linked.json"
        link_path.symlink_to(self.fixture.config_path)
        with self.assertRaises(BOOTSTRAP.BootstrapError):
            BOOTSTRAP.read_private_json(link_path, "sandbox_config")

        bad_spec = dict(self.fixture.candidate_spec)
        bad_spec["candidates"] = dict(bad_spec["candidates"])
        bad_spec["candidates"].pop("runtime")
        with self.assertRaises(BOOTSTRAP.BootstrapError):
            BOOTSTRAP.validate_candidate_spec(bad_spec)

        self.fixture.candidate_spec_path.chmod(0o600)
        with self.assertRaises(BOOTSTRAP.BootstrapError):
            BOOTSTRAP.load_candidate_publication(self.fixture.candidate_spec_path)
        self.fixture.candidate_spec_path.chmod(0o400)
        self.fixture.provenance_path.chmod(0o600)
        with self.assertRaises(BOOTSTRAP.BootstrapError):
            BOOTSTRAP.load_candidate_publication(self.fixture.candidate_spec_path)

    def test_candidate_publication_rejects_bound_provenance_or_artifact_drift(self):
        self.fixture.provenance_path.chmod(0o600)
        changed = dict(self.fixture.provenance)
        changed["release_eligible"] = True
        write_file(self.fixture.provenance_path, changed, 0o400)
        self.fixture.candidate_spec_path.chmod(0o600)
        changed_spec = dict(self.fixture.candidate_spec)
        changed_spec["provenance_sha256"] = hashlib.sha256(
            self.fixture.provenance_path.read_bytes()
        ).hexdigest()
        write_file(self.fixture.candidate_spec_path, changed_spec, 0o400)
        with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
            BOOTSTRAP.load_candidate_publication(self.fixture.candidate_spec_path)
        self.assertEqual(raised.exception.code, "candidate_provenance_invalid")

        # Recreate a valid publication, then alter one immutable candidate while
        # retaining its nominal mode. The spec/provenance pair must not mask it.
        self.fixture.provenance_path.chmod(0o600)
        write_file(self.fixture.provenance_path, self.fixture.provenance, 0o400)
        self.fixture.candidate_spec_path.chmod(0o600)
        write_file(self.fixture.candidate_spec_path, self.fixture.candidate_spec, 0o400)
        runtime = pathlib.Path(self.fixture.candidates["runtime"])
        runtime.chmod(0o700)
        runtime.write_bytes(b"changed-runtime\n")
        runtime.chmod(0o555)
        with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
            BOOTSTRAP.load_candidate_publication(self.fixture.candidate_spec_path)
        self.assertEqual(raised.exception.code, "candidate_digest_mismatch")

    def test_candidate_provenance_exact_recipe_toolchain_and_builder_negatives(self):
        mutations = (
            (
                "unknown_environment",
                lambda value: value["environment"].__setitem__("SECRET", "forbidden"),
                "candidate_provenance_binding_invalid",
            ),
            (
                "recipe",
                lambda value: value["commands"][0].append("--unexpected"),
                "candidate_provenance_recipe_invalid",
            ),
            (
                "recipe_dependency_config",
                lambda value: value["commands"][0].__setitem__(
                    2, value["dependencies"]["transport"]["cargo_config"]["path"]
                ),
                "candidate_provenance_recipe_invalid",
            ),
            (
                "toolchain",
                lambda value: value["toolchain"]["darwin"]["fixed_tools"]["xcrun"].pop("links"),
                "candidate_provenance_toolchain_invalid",
            ),
            (
                "builder",
                lambda value: value["builder"].__setitem__("sha256", "0" * 64),
                "candidate_provenance_builder_invalid",
            ),
        )
        for name, mutate, expected in mutations:
            with self.subTest(name=name):
                changed = copy.deepcopy(self.fixture.provenance)
                mutate(changed)
                self.fixture.provenance_path.chmod(0o600)
                write_file(self.fixture.provenance_path, changed, 0o400)
                changed_spec = copy.deepcopy(self.fixture.candidate_spec)
                changed_spec["provenance_sha256"] = hashlib.sha256(
                    self.fixture.provenance_path.read_bytes()
                ).hexdigest()
                self.fixture.candidate_spec_path.chmod(0o600)
                write_file(self.fixture.candidate_spec_path, changed_spec, 0o400)
                with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                    BOOTSTRAP.load_candidate_publication(
                        self.fixture.candidate_spec_path
                    )
                self.assertEqual(raised.exception.code, expected)
                self.fixture.cleanup()
                self.fixture = BootstrapFixture(self)

    def test_candidate_dependency_snapshot_exact_schema_and_drift_negatives(self):
        self.assertIs(
            BOOTSTRAP.validate_candidate_dependencies(
                self.fixture.dependencies, self.fixture.root
            ),
            self.fixture.dependencies,
        )
        mutations = (
            (
                "extra_field",
                lambda value: value.__setitem__("unexpected", True),
                "candidate_dependency_snapshot_invalid",
            ),
            (
                "record_identity",
                lambda value: value["record"].__setitem__("sha256", "0" * 64),
                "candidate_dependency_snapshot_changed",
            ),
            (
                "record_self_seal",
                lambda value: value.__setitem__("record_sha256", "0" * 64),
                "candidate_dependency_snapshot_changed",
            ),
            (
                "tree_digest",
                lambda value: value.__setitem__("tree_sha256", "0" * 64),
                "candidate_dependency_snapshot_changed",
            ),
            (
                "cargo_config_identity",
                lambda value: value["workspace"]["cargo_config"].__setitem__(
                    "sha256", "0" * 64
                ),
                "candidate_dependency_config_invalid",
            ),
            (
                "source_lock_identity",
                lambda value: value["source_inputs"]["workspace_lock"].__setitem__(
                    "sha256", "0" * 64
                ),
                "candidate_dependency_source_changed",
            ),
        )
        for name, mutate, expected in mutations:
            with self.subTest(name=name):
                changed = copy.deepcopy(self.fixture.dependencies)
                mutate(changed)
                with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                    BOOTSTRAP.validate_candidate_dependencies(changed, self.fixture.root)
                self.assertEqual(raised.exception.code, expected)

    def test_one_shot_sequence_writes_byte_exact_taint_before_orchestration(self):
        executor = FakeExecutor(self.fixture)
        result = self.fixture.controller(executor).run(
            self.fixture.config_path,
            self.fixture.candidate_spec_path,
            "one-shot",
        )
        self.assertEqual(result["status"], "passed")
        self.assertFalse(result["release_eligible"])
        self.assertTrue(result["persistent_sandbox_retained"])
        self.assertEqual(set(result), BOOTSTRAP.RESULT_FIELDS)
        self.assertEqual(
            executor.calls,
            [
                "source_root",
                "source_commit",
                "source_tree",
                "source_status",
                "cargo_version",
                "rustc_verbose_version",
                "issuer_build",
                "cargo_version",
                "rustc_verbose_version",
                "issuer_linkage",
                "source_root",
                "source_commit",
                "source_tree",
                "source_status",
                "source_root",
                "source_commit",
                "source_tree",
                "source_status",
                "manifest_prepare",
                "dry_run",
                "preflight",
                "prepare",
                "start",
                "source_root",
                "source_commit",
                "source_tree",
                "source_status",
                "direct_onboard",
                "source_root",
                "source_commit",
                "source_tree",
                "source_status",
                "d2a_auth_smoke",
                "source_root",
                "source_commit",
                "source_tree",
                "source_status",
                "d2a_one_shot",
                "source_root",
                "source_commit",
                "source_tree",
                "source_status",
                "d2a_verify",
                "d2a_verify",
                "source_root",
                "source_commit",
                "source_tree",
                "source_status",
                "teardown_discord_resources",
                "cleanup",
                "status",
            ],
        )
        self.assertNotIn("onboard", executor.calls)
        direct_argv = executor.argv_calls[executor.calls.index("direct_onboard")]
        self.assertEqual(
            direct_argv,
            [
                str(
                    self.fixture.tool_root
                    / "session-issuer"
                    / "target"
                    / "release"
                    / "starring-d2-session-issuer"
                ),
                "--manifest",
                str(self.fixture.manifest_path),
                "--operation",
                "direct-onboard",
                "--display-name",
                self.fixture.config["discord"]["actor_display_name"],
            ],
        )
        self.assertTrue(executor.taint_seen_before_followup)
        state_path, state = BOOTSTRAP.load_state(pathlib.Path(result["state"]))
        self.assertEqual(set(state), BOOTSTRAP.STATE_FIELDS)
        self.assertEqual(stat.S_IMODE(state_path.stat().st_mode), 0o600)
        evidence_path = self.fixture.manifest_path.with_name("d2a-onboarding-evidence.json")
        evidence = json.loads(evidence_path.read_bytes())
        self.assertEqual(set(evidence), BOOTSTRAP.DIRECT_ONBOARDING_FIELDS)
        self.assertEqual(stat.S_IMODE(evidence_path.stat().st_mode), 0o600)
        self.assertEqual(evidence_path.stat().st_nlink, 1)
        evidence_sha = hashlib.sha256(evidence_path.read_bytes()).hexdigest()
        self.assertEqual(state["onboarding_evidence_path"], str(evidence_path))
        self.assertEqual(state["onboarding_evidence_sha256"], evidence_sha)
        self.assertEqual(
            result["onboarding_evidence"],
            {"path": str(evidence_path), "sha256": evidence_sha},
        )
        for relative in BOOTSTRAP.COMMERCIAL_ONBOARDING_ARTIFACTS:
            self.assertFalse(os.path.lexists(self.fixture.manifest_path.parent / relative))
        taint_path = self.fixture.manifest_path.with_name("d2a-taint.json")
        self.assertEqual(stat.S_IMODE(taint_path.stat().st_mode), 0o600)
        digests = state["tool_digests"]
        self.assertEqual(set(digests), BOOTSTRAP.TOOL_DIGEST_FIELDS)
        self.assertEqual(
            digests["cargo_sha256"],
            hashlib.sha256((self.fixture.rust_toolchain_bin / "cargo").read_bytes()).hexdigest(),
        )
        self.assertEqual(
            digests["rustc_sha256"],
            hashlib.sha256((self.fixture.rust_toolchain_bin / "rustc").read_bytes()).hexdigest(),
        )
        self.assertEqual(state["source_commit_sha"], self.fixture.candidate_spec["commit_sha"])
        self.assertEqual(state["source_tree_sha"], self.fixture.candidate_spec["source_tree_sha"])
        self.assertEqual(
            state["candidate_dependency_record_sha256"],
            self.fixture.dependencies["record_sha256"],
        )
        self.assertEqual(
            state["candidate_dependency_tree_sha256"],
            self.fixture.dependencies["tree_sha256"],
        )
        self.assertEqual(
            result["candidate_dependencies"],
            {
                "record_sha256": self.fixture.dependencies["record_sha256"],
                "tree_sha256": self.fixture.dependencies["tree_sha256"],
            },
        )
        self.assertEqual(
            result["source_revision"],
            {
                "commit_sha": self.fixture.candidate_spec["commit_sha"],
                "tree_sha": self.fixture.candidate_spec["source_tree_sha"],
            },
        )
        self.assertEqual(
            result["issuer_toolchain"],
            {
                "cargo_sha256": digests["cargo_sha256"],
                "rustc_sha256": digests["rustc_sha256"],
                "rust_sysroot_sha256": digests["rust_sysroot_sha256"],
                "rust_linkers_sha256": digests["rust_linkers_sha256"],
                "darwin_toolchain_sha256": digests["darwin_toolchain_sha256"],
                "macos_sdk_sha256": digests["macos_sdk_sha256"],
                "build_environment_sha256": digests["issuer_build_environment_sha256"],
                "build_environment": state["issuer_build_environment"],
            },
        )
        _marker, expected = BOOTSTRAP.D2A.build_taint_marker(
            state["run_id"],
            state["manifest_sha256"],
            digests["issuer_sha256"],
            digests["issuer_source_sha256"],
            digests["runner_sha256"],
            digests["product_driver_sha256"],
            digests["scenario_sha256"],
        )
        self.assertEqual(taint_path.read_bytes(), expected)
        preissuer_raw = executor.preissuer_lifecycle_seen
        self.assertIsNotNone(preissuer_raw)
        preissuer = json.loads(preissuer_raw)
        self.assertTrue(
            BOOTSTRAP.valid_preissuer_lifecycle(
                preissuer, state, state["manifest_sha256"]
            )
        )
        self.assertEqual(preissuer["origin"], "bootstrap")
        self.assertEqual(preissuer["operation"], "direct-onboard")
        self.assertIsNone(preissuer["process_group_id"])
        self.assertEqual(
            preissuer_raw,
            (
                json.dumps(
                    preissuer,
                    ensure_ascii=False,
                    allow_nan=False,
                    separators=(",", ":"),
                )
                + "\n"
            ).encode("utf-8"),
        )

    def test_preissuer_sentinel_never_overwrites_and_global_lock_excludes_race(self):
        existing = {"do_not": "overwrite"}
        existing_raw = (
            json.dumps(existing, separators=(",", ":")) + "\n"
        ).encode()
        executor = FakeExecutor(
            self.fixture,
            preexisting_lifecycle=existing,
        )
        result = self.fixture.controller(executor).run(
            self.fixture.config_path,
            self.fixture.candidate_spec_path,
            "auth-smoke",
        )
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["error_code"], "manual_recovery_required")
        lifecycle_path = self.fixture.manifest_path.with_name(
            "d2a-session-lifecycle.json"
        )
        self.assertEqual(lifecycle_path.read_bytes(), existing_raw)

        self.fixture.cleanup()
        self.fixture = BootstrapFixture(self)
        exact_executor = FakeExecutor(self.fixture)
        real_create_manifest = self.fixture.create_manifest

        def create_with_exact_sentinel(argv):
            real_create_manifest(argv)
            placeholder_state = {
                "run_id": self.fixture.run_id,
                "manifest_path": str(self.fixture.manifest_path),
                "tool_digests": BOOTSTRAP.collect_tool_digests(
                    self.fixture.tool_root,
                    self.fixture.certification_root,
                ),
            }
            marker = BOOTSTRAP.preissuer_lifecycle_marker(
                placeholder_state,
                self.fixture.manifest_sha256,
            )
            write_file(
                self.fixture.manifest_path.with_name(
                    "d2a-session-lifecycle.json"
                ),
                marker,
                0o600,
                sort_keys=False,
            )

        with mock.patch.object(
            self.fixture,
            "create_manifest",
            side_effect=create_with_exact_sentinel,
        ):
            exact_result = self.fixture.controller(exact_executor).run(
                self.fixture.config_path,
                self.fixture.candidate_spec_path,
                "auth-smoke",
            )
        self.assertEqual(exact_result["status"], "failed")
        self.assertEqual(exact_result["error_code"], "manual_recovery_required")

        self.fixture.cleanup()
        self.fixture = BootstrapFixture(self)
        executor = FakeExecutor(self.fixture)
        marker_lock_path = self.fixture.root / "d2-global.lock"
        real_marker_lock = BOOTSTRAP.d2_global_marker_lock
        with real_marker_lock(marker_lock_path):
            with mock.patch.object(
                BOOTSTRAP,
                "d2_global_marker_lock",
                side_effect=lambda: real_marker_lock(marker_lock_path),
            ):
                result = self.fixture.controller(executor).run(
                    self.fixture.config_path,
                    self.fixture.candidate_spec_path,
                    "auth-smoke",
                )
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["error_code"], "d2_global_lock_busy")
        self.assertFalse(
            os.path.lexists(
                self.fixture.manifest_path.with_name(
                    "d2a-session-lifecycle.json"
                )
            )
        )

    def test_failed_session_operation_is_permanently_fail_closed(self):
        executor = FakeExecutor(self.fixture, fail_once={"d2a_one_shot": 1})
        result = self.fixture.controller(executor).run(
            self.fixture.config_path,
            self.fixture.candidate_spec_path,
            "one-shot",
        )
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["error_code"], "manual_recovery_required")
        self.assertFalse(result["discord_teardown_complete"])
        self.assertFalse(result["cleanup_complete"])
        failed_at = executor.calls.index("d2a_one_shot")
        self.assertEqual(executor.calls[failed_at + 1 :], [])

    def test_ordinary_resume_remains_blocked_for_quarantined_no_issue_run(self):
        state_path, _state = self.create_quarantined_direct_onboard_failure()
        resume_executor = FakeExecutor(self.fixture)
        resumed = self.fixture.controller(resume_executor).resume(state_path)
        self.assertEqual(resumed["status"], "failed")
        self.assertEqual(resumed["error_code"], "manual_recovery_required")
        self.assertFalse(resumed["discord_teardown_complete"])
        self.assertFalse(resumed["cleanup_complete"])
        self.assertFalse(resumed["total_local_absence"])
        self.assertEqual(resume_executor.calls, [])
        state = json.loads(state_path.read_bytes())
        self.assertEqual(state["status"], "recovery_required")
        self.assertEqual(state["phase"], "direct_onboard")
        self.assertEqual(state["last_error"], "direct_onboard_failed")

    def test_audited_quarantined_recovery_rejects_bootstrap_state_mutation(self):
        state_path, state = self.create_quarantined_direct_onboard_failure()
        incident = self.audited_incident_overrides(state_path, state)
        state["candidate_dependency_record_sha256"] = "f" * 64
        BOOTSTRAP.write_state(state_path, state)
        executor = FakeExecutor(self.fixture)
        with mock.patch.dict(
            BOOTSTRAP.AUDITED_QUARANTINED_INCIDENT,
            incident,
            clear=True,
        ):
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                self.fixture.controller(
                    executor
                ).recover_audited_quarantined_no_issue(
                    state_path,
                    "c" * 40,
                    "d" * 40,
                    state["run_id"],
                    state["manifest_sha256"],
                )
        self.assertEqual(
            raised.exception.code,
            "audited_recovery_bootstrap_state_invalid",
        )
        self.assertEqual(executor.calls, [])

    def test_audited_recovery_rejects_terminal_semantic_state_mutation(self):
        state_path, state = self.create_quarantined_direct_onboard_failure()
        incident = self.audited_incident_overrides(state_path, state)
        with mock.patch.dict(
            BOOTSTRAP.AUDITED_QUARANTINED_INCIDENT,
            incident,
            clear=True,
        ), mock.patch.object(
            BOOTSTRAP,
            "validate_audited_quarantined_recovery_output",
            side_effect=lambda value, *_args: value,
        ):
            first = self.fixture.controller(
                FakeExecutor(self.fixture)
            ).recover_audited_quarantined_no_issue(
                state_path, "c" * 40, "d" * 40,
                state["run_id"], state["manifest_sha256"],
            )
        self.assertTrue(first["total_local_absence"])

        terminal = json.loads(state_path.read_bytes())
        terminal["candidate_dependency_record_sha256"] = "f" * 64
        BOOTSTRAP.write_state(state_path, terminal)
        executor = FakeExecutor(self.fixture, audited_recovery_tamper="replay")
        with mock.patch.dict(
            BOOTSTRAP.AUDITED_QUARANTINED_INCIDENT,
            incident,
            clear=True,
        ):
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                self.fixture.controller(
                    executor
                ).recover_audited_quarantined_no_issue(
                    state_path, "c" * 40, "d" * 40,
                    state["run_id"], state["manifest_sha256"],
                )
        self.assertEqual(
            raised.exception.code,
            "audited_recovery_bootstrap_state_invalid",
        )
        self.assertEqual(executor.calls, [])

    def test_audited_quarantined_recovery_succeeds_and_replays_without_normal_status(self):
        state_path, state = self.create_quarantined_direct_onboard_failure()
        lifecycle_path = self.fixture.manifest_path.with_name(
            "d2a-session-lifecycle.json"
        )
        lifecycle_before = lifecycle_path.read_bytes()
        incident = self.audited_incident_overrides(state_path, state)
        first_executor = FakeExecutor(self.fixture)
        controller = self.fixture.controller(first_executor)
        with mock.patch.dict(
            BOOTSTRAP.AUDITED_QUARANTINED_INCIDENT,
            incident,
            clear=True,
        ), mock.patch.object(
            BOOTSTRAP,
            "validate_audited_quarantined_recovery_output",
            side_effect=lambda value, *_args: value,
        ):
            result = controller.recover_audited_quarantined_no_issue(
                state_path,
                "c" * 40,
                "d" * 40,
                state["run_id"],
                state["manifest_sha256"],
            )
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["error_code"], "direct_onboard_failed")
        self.assertTrue(result["discord_teardown_complete"])
        self.assertTrue(result["cleanup_complete"])
        self.assertTrue(result["total_local_absence"])
        persisted = json.loads(state_path.read_bytes())
        self.assertEqual(persisted["status"], "failed")
        self.assertEqual(persisted["phase"], "complete")
        self.assertEqual(persisted["last_error"], "direct_onboard_failed")
        self.assertTrue(persisted["candidate_started"])
        self.assertEqual(lifecycle_path.read_bytes(), lifecycle_before)
        self.assertEqual(
            first_executor.calls,
            [
                "source_root",
                "source_commit",
                "source_tree",
                "source_status",
                "recover_audited_quarantined_no_issue",
            ],
        )
        recovery_argv = first_executor.argv_calls[-1]
        self.assertEqual(
            recovery_argv[3:],
            [
                "--manifest",
                str(self.fixture.manifest_path),
                "--bootstrap-state",
                str(state_path),
                "--confirm-current-commit",
                "c" * 40,
                "--confirm-current-tree",
                "d" * 40,
                "--confirm-run-id",
                state["run_id"],
                "--confirm-manifest-sha256",
                state["manifest_sha256"],
            ],
        )

        replay_executor = FakeExecutor(
            self.fixture,
            audited_recovery_tamper="replay",
        )
        with mock.patch.dict(
            BOOTSTRAP.AUDITED_QUARANTINED_INCIDENT,
            incident,
            clear=True,
        ), mock.patch.object(
            BOOTSTRAP,
            "validate_audited_quarantined_recovery_output",
            side_effect=lambda value, *_args: value,
        ):
            replay = self.fixture.controller(
                replay_executor
            ).recover_audited_quarantined_no_issue(
                state_path,
                "c" * 40,
                "d" * 40,
                state["run_id"],
                state["manifest_sha256"],
            )
        self.assertEqual(replay["error_code"], "direct_onboard_failed")
        self.assertTrue(replay["total_local_absence"])
        self.assertNotIn("status", replay_executor.calls)
        self.assertEqual(lifecycle_path.read_bytes(), lifecycle_before)

    def test_audited_recovery_failed_replay_clears_stale_absence_and_can_retry(self):
        state_path, state = self.create_quarantined_direct_onboard_failure()
        incident = self.audited_incident_overrides(state_path, state)
        with mock.patch.dict(
            BOOTSTRAP.AUDITED_QUARANTINED_INCIDENT,
            incident,
            clear=True,
        ), mock.patch.object(
            BOOTSTRAP,
            "validate_audited_quarantined_recovery_output",
            side_effect=lambda value, *_args: value,
        ):
            first = self.fixture.controller(
                FakeExecutor(self.fixture)
            ).recover_audited_quarantined_no_issue(
                state_path, "c" * 40, "d" * 40,
                state["run_id"], state["manifest_sha256"],
            )
        self.assertTrue(first["total_local_absence"])

        failing_executor = FakeExecutor(
            self.fixture,
            fail_once={"recover_audited_quarantined_no_issue": 1},
        )
        with mock.patch.dict(
            BOOTSTRAP.AUDITED_QUARANTINED_INCIDENT,
            incident,
            clear=True,
        ):
            failed = self.fixture.controller(
                failing_executor
            ).recover_audited_quarantined_no_issue(
                state_path, "c" * 40, "d" * 40,
                state["run_id"], state["manifest_sha256"],
            )
        self.assertEqual(
            failed["error_code"],
            "recover_audited_quarantined_no_issue_failed",
        )
        self.assertFalse(failed["discord_teardown_complete"])
        self.assertFalse(failed["cleanup_complete"])
        self.assertFalse(failed["total_local_absence"])
        persisted = json.loads(state_path.read_bytes())
        self.assertEqual(persisted["status"], "failed")
        self.assertEqual(persisted["phase"], "complete")
        self.assertEqual(persisted["last_error"], "direct_onboard_failed")

        retry_executor = FakeExecutor(
            self.fixture,
            audited_recovery_tamper="replay",
        )
        with mock.patch.dict(
            BOOTSTRAP.AUDITED_QUARANTINED_INCIDENT,
            incident,
            clear=True,
        ), mock.patch.object(
            BOOTSTRAP,
            "validate_audited_quarantined_recovery_output",
            side_effect=lambda value, *_args: value,
        ):
            retry = self.fixture.controller(
                retry_executor
            ).recover_audited_quarantined_no_issue(
                state_path, "c" * 40, "d" * 40,
                state["run_id"], state["manifest_sha256"],
            )
        self.assertEqual(retry["error_code"], "direct_onboard_failed")
        self.assertTrue(retry["total_local_absence"])

    def test_audited_recovery_rejects_unknown_output_and_missing_evidence(self):
        state_path, state = self.create_quarantined_direct_onboard_failure()
        incident, output, _paths = self.write_audited_recovery_artifacts(
            state_path, state
        )
        with mock.patch.dict(
            BOOTSTRAP.AUDITED_QUARANTINED_INCIDENT,
            incident,
            clear=True,
        ):
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                BOOTSTRAP.validate_audited_quarantined_recovery_output(
                    {**output, "unexpected": True},
                    state,
                    state_path,
                    "e" * 40,
                    "f" * 40,
                    self.fixture.root,
                )
        self.assertEqual(raised.exception.code, "audited_recovery_output_invalid")

        read_artifact = BOOTSTRAP.read_canonical_private_json

        def missing_evidence(path, label):
            if label == "audited_recovery_evidence":
                raise BOOTSTRAP.BootstrapError(
                    "audited_recovery_evidence_unavailable"
                )
            return read_artifact(path, label)

        with mock.patch.dict(
            BOOTSTRAP.AUDITED_QUARANTINED_INCIDENT,
            incident,
            clear=True,
        ), mock.patch.object(
            BOOTSTRAP,
            "read_canonical_private_json",
            side_effect=missing_evidence,
        ), mock.patch.object(
            BOOTSTRAP, "validate_audited_source_transition_git"
        ):
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                BOOTSTRAP.validate_audited_quarantined_recovery_output(
                    output,
                    state,
                    state_path,
                    "e" * 40,
                    "f" * 40,
                    self.fixture.root,
                )
        self.assertEqual(
            raised.exception.code,
            "audited_recovery_evidence_unavailable",
        )

    def test_audited_recovery_validates_all_artifacts_and_rejects_mutation(self):
        state_path, state = self.create_quarantined_direct_onboard_failure()
        incident, output, paths = self.write_audited_recovery_artifacts(
            state_path, state
        )
        with mock.patch.dict(
            BOOTSTRAP.AUDITED_QUARANTINED_INCIDENT,
            incident,
            clear=True,
        ), mock.patch.object(
            BOOTSTRAP, "validate_audited_source_transition_git"
        ):
            self.assertIs(
                BOOTSTRAP.validate_audited_quarantined_recovery_output(
                    output,
                    state,
                    state_path,
                    "e" * 40,
                    "f" * 40,
                    self.fixture.root,
                ),
                output,
            )

            evidence = json.loads(paths["evidence"].read_bytes())
            evidence["unexpected"] = True
            write_file(paths["evidence"], evidence, 0o600)
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                BOOTSTRAP.validate_audited_quarantined_recovery_output(
                    output,
                    state,
                    state_path,
                    "e" * 40,
                    "f" * 40,
                    self.fixture.root,
                )
            self.assertEqual(
                raised.exception.code,
                "audited_recovery_evidence_invalid",
            )

            paths["evidence"].unlink()
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                BOOTSTRAP.validate_audited_quarantined_recovery_output(
                    output,
                    state,
                    state_path,
                    "e" * 40,
                    "f" * 40,
                    self.fixture.root,
                )
            self.assertEqual(
                raised.exception.code,
                "audited_recovery_evidence_path_invalid",
            )

    def test_audited_recovery_source_transition_chain_fails_closed(self):
        state_path, state = self.create_quarantined_direct_onboard_failure()
        incident, output, paths = self.write_audited_recovery_artifacts(
            state_path, state
        )
        interlock_path = BOOTSTRAP.audited_recovery_source_interlock_path(state)
        interlock_v1_path = (
            BOOTSTRAP.audited_recovery_source_interlock_v1_path(state)
        )
        transition_v1_path = (
            BOOTSTRAP.audited_recovery_source_transition_v1_path(state)
        )
        interlock_v2_path = (
            BOOTSTRAP.audited_recovery_source_interlock_v2_path(state)
        )
        transition_v2_path = (
            BOOTSTRAP.audited_recovery_source_transition_v2_path(state)
        )
        coordinator = interlock_path.parent
        original = {
            path: path.read_bytes()
            for path in (
                interlock_v1_path,
                transition_v1_path,
                interlock_v2_path,
                transition_v2_path,
                interlock_path,
                paths["source_transition"],
                paths["database_absence"],
                paths["reconciliation"],
                paths["evidence"],
            )
        }

        def restore():
            for path, raw in original.items():
                write_file(path, raw, 0o600)

        def validate(candidate=output):
            return BOOTSTRAP.validate_audited_quarantined_recovery_output(
                candidate,
                state,
                state_path,
                "e" * 40,
                "f" * 40,
                self.fixture.root,
            )

        with mock.patch.dict(
            BOOTSTRAP.AUDITED_QUARANTINED_INCIDENT,
            incident,
            clear=True,
        ), mock.patch.object(
            BOOTSTRAP, "validate_audited_source_transition_git"
        ):
            transition = json.loads(paths["source_transition"].read_bytes())
            self.assertEqual(
                transition["from_source"]["commit_sha"], "c" * 40
            )
            self.assertEqual(
                transition["to_source"]["commit_sha"], "e" * 40
            )
            self.assertIs(validate(), output)

            for missing, expected in (
                (
                    interlock_v1_path,
                    "audited_recovery_source_interlock_v1_path_invalid",
                ),
                (
                    transition_v1_path,
                    "audited_recovery_source_transition_v1_path_invalid",
                ),
                (
                    interlock_v2_path,
                    "audited_recovery_source_interlock_v2_path_invalid",
                ),
                (
                    transition_v2_path,
                    "audited_recovery_source_transition_v2_path_invalid",
                ),
                (interlock_path, "audited_recovery_source_interlock_path_invalid"),
                (
                    paths["source_transition"],
                    "audited_recovery_source_transition_path_invalid",
                ),
            ):
                with self.subTest(missing=missing.name):
                    missing.unlink()
                    with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                        validate()
                    self.assertEqual(raised.exception.code, expected)
                    restore()

            interlock_v1 = json.loads(interlock_v1_path.read_bytes())
            interlock_v1["reason_codes"] = interlock_v1["reason_codes"][:-1]
            write_file(interlock_v1_path, interlock_v1, 0o600)
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                validate()
            self.assertEqual(
                raised.exception.code,
                "audited_recovery_source_transition_invalid",
            )
            restore()

            interlock = json.loads(interlock_path.read_bytes())
            interlock["reason_codes"] = interlock["reason_codes"][:-1]
            write_file(interlock_path, interlock, 0o600)
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                validate()
            self.assertEqual(
                raised.exception.code,
                "audited_recovery_source_transition_invalid",
            )
            restore()

            interlock = json.loads(interlock_path.read_bytes())
            transition = json.loads(paths["source_transition"].read_bytes())
            interlock["previous_source_transition_sha256"] = "0" * 64
            transition["previous_source_transition_sha256"] = "0" * 64
            write_file(interlock_path, interlock, 0o600)
            transition["interlock_sha256"] = hashlib.sha256(
                interlock_path.read_bytes()
            ).hexdigest()
            write_file(paths["source_transition"], transition, 0o600)
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                validate({
                    **output,
                    "source_transition_sha256": hashlib.sha256(
                        paths["source_transition"].read_bytes()
                    ).hexdigest(),
                })
            self.assertEqual(
                raised.exception.code,
                "audited_recovery_source_transition_invalid",
            )
            restore()

            transition = json.loads(paths["source_transition"].read_bytes())
            transition["kind"] = BOOTSTRAP.AUDITED_QUARANTINED_SOURCE_INTERLOCK_KIND
            write_file(paths["source_transition"], transition, 0o600)
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                validate()
            self.assertEqual(
                raised.exception.code,
                "audited_recovery_source_transition_invalid",
            )
            restore()

            for name, expected in (
                ("database_absence", "audited_recovery_source_transition_invalid"),
                ("reconciliation", "audited_recovery_source_transition_invalid"),
                ("evidence", "audited_recovery_evidence_invalid"),
            ):
                with self.subTest(broken_link=name):
                    artifact = json.loads(paths[name].read_bytes())
                    artifact["source_transition_sha256"] = "0" * 64
                    write_file(paths[name], artifact, 0o600)
                    with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                        validate()
                    observed = raised.exception.code
                    restore()
                    self.assertEqual(observed, expected)

            database = json.loads(paths["database_absence"].read_bytes())
            database["static_sql_sha256"] = incident["static_sql_sha256"]
            write_file(paths["database_absence"], database, 0o600)
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                validate()
            self.assertEqual(
                raised.exception.code,
                "audited_recovery_source_transition_invalid",
            )
            restore()

            extra = write_file(coordinator / "unexpected", b"x", 0o600)
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                validate()
            self.assertEqual(
                raised.exception.code,
                "audited_recovery_intent_invalid",
            )
            extra.unlink()

            interlock = json.loads(interlock_path.read_bytes())
            transition = json.loads(paths["source_transition"].read_bytes())
            for artifact in (interlock, transition):
                artifact["audit_configuration"][
                    "login_keychain_policy_sha256"
                ] = "0" * 64
            write_file(interlock_path, interlock, 0o600)
            transition["interlock_sha256"] = hashlib.sha256(
                interlock_path.read_bytes()
            ).hexdigest()
            write_file(paths["source_transition"], transition, 0o600)
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                validate(
                    {
                        **output,
                        "source_transition_sha256": hashlib.sha256(
                            paths["source_transition"].read_bytes()
                        ).hexdigest(),
                    }
                )
            self.assertEqual(
                raised.exception.code,
                "audited_recovery_source_transition_invalid",
            )

    def test_audited_recovery_cleanup_boundary_fails_closed(self):
        state_path, state = self.create_quarantined_direct_onboard_failure()
        incident, output, paths = self.write_audited_recovery_artifacts(
            state_path, state
        )
        interlock_path = BOOTSTRAP.audited_recovery_source_interlock_path(state)
        transition_v2_path = (
            BOOTSTRAP.audited_recovery_source_transition_v2_path(state)
        )
        journal_path = (
            self.fixture.manifest_path.parent / "orchestrator" / "lifecycle.jsonl"
        )
        root_identity_path = (
            self.fixture.manifest_path.parent
            / "orchestrator"
            / "cleanup-root-identity.json"
        )
        keychain_baseline_path = (
            self.fixture.manifest_path.parent
            / "orchestrator"
            / "cleanup-keychain-baseline.json"
        )
        root_progress_path = (
            self.fixture.manifest_path.parent
            / "orchestrator"
            / "cleanup-root-progress.json"
        )
        fence_path = self.fixture.manifest_path.with_name(
            "d2a-teardown-fence.json"
        )
        original = {
            path: path.read_bytes()
            for path in (
                interlock_path,
                paths["source_transition"],
                paths["database_absence"],
                paths["reconciliation"],
                paths["evidence"],
                journal_path,
                root_identity_path,
                keychain_baseline_path,
                root_progress_path,
                fence_path,
            )
        }

        def restore():
            for path, raw in original.items():
                write_file(path, raw, 0o600)

        def validate(candidate=output):
            return BOOTSTRAP.validate_audited_quarantined_recovery_output(
                candidate,
                state,
                state_path,
                "e" * 40,
                "f" * 40,
                self.fixture.root,
            )

        with mock.patch.dict(
            BOOTSTRAP.AUDITED_QUARANTINED_INCIDENT,
            incident,
            clear=True,
        ), mock.patch.object(
            BOOTSTRAP, "validate_audited_source_transition_git"
        ):
            transition_v2_sha256 = hashlib.sha256(
                transition_v2_path.read_bytes()
            ).hexdigest()
            cleanup_transition_sha256 = hashlib.sha256(
                paths["source_transition"].read_bytes()
            ).hexdigest()
            database = json.loads(paths["database_absence"].read_bytes())
            reconciliation = json.loads(paths["reconciliation"].read_bytes())
            evidence = json.loads(paths["evidence"].read_bytes())
            self.assertNotEqual(
                transition_v2_sha256, cleanup_transition_sha256
            )
            self.assertEqual(
                database["source_transition_sha256"], transition_v2_sha256
            )
            self.assertEqual(
                reconciliation["source_transition_sha256"],
                transition_v2_sha256,
            )
            self.assertEqual(
                evidence["source_transition_sha256"],
                cleanup_transition_sha256,
            )
            self.assertEqual(
                output["source_transition_sha256"], cleanup_transition_sha256
            )
            self.assertIs(validate(), output)

            rows = [
                json.loads(line)
                for line in journal_path.read_text(encoding="utf-8").splitlines()
            ]
            rows[45]["status"] = "complete"
            write_file(
                journal_path,
                b"".join(
                    (BOOTSTRAP.canonical_json(row) + "\n").encode("utf-8")
                    for row in rows
                ),
                0o600,
            )
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                validate()
            observed = raised.exception.code
            restore()
            self.assertEqual(
                observed, "audited_recovery_cleanup_journal_invalid"
            )

            impossible_rows = [
                json.loads(line)
                for line in journal_path.read_text(encoding="utf-8").splitlines()
            ]
            impossible_rows[46]["status"] = "failed"
            write_file(
                journal_path,
                b"".join(
                    (BOOTSTRAP.canonical_json(row) + "\n").encode("utf-8")
                    for row in impossible_rows
                ),
                0o600,
            )
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                validate()
            observed = raised.exception.code
            restore()
            self.assertEqual(
                observed, "audited_recovery_cleanup_journal_invalid"
            )

            root_identity = json.loads(root_identity_path.read_bytes())
            root_identity["root_inode"] += 1
            write_file(root_identity_path, root_identity, 0o600)
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                validate()
            observed = raised.exception.code
            restore()
            self.assertEqual(
                observed, "audited_recovery_cleanup_root_identity_invalid"
            )

            keychain_baseline_path.unlink()
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                validate()
            observed = raised.exception.code
            restore()
            self.assertEqual(
                observed,
                "audited_recovery_cleanup_keychain_baseline_path_invalid",
            )

            baseline = json.loads(keychain_baseline_path.read_bytes())
            baseline["inventory"][0]["identity_sha256"] = "0" * 64
            write_file(keychain_baseline_path, baseline, 0o600)
            evidence = json.loads(paths["evidence"].read_bytes())
            evidence["cleanup_keychain_baseline_sha256"] = hashlib.sha256(
                keychain_baseline_path.read_bytes()
            ).hexdigest()
            write_file(paths["evidence"], evidence, 0o600)
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                validate()
            observed = raised.exception.code
            restore()
            self.assertEqual(
                observed, "audited_recovery_cleanup_keychain_baseline_invalid"
            )

            root_progress_path.unlink()
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                validate()
            observed = raised.exception.code
            restore()
            self.assertEqual(
                observed, "audited_recovery_cleanup_root_progress_path_invalid"
            )

            progress = json.loads(root_progress_path.read_bytes())
            progress["phase"] = "planned"
            write_file(root_progress_path, progress, 0o600)
            evidence = json.loads(paths["evidence"].read_bytes())
            evidence["cleanup_root_progress_sha256"] = hashlib.sha256(
                root_progress_path.read_bytes()
            ).hexdigest()
            write_file(paths["evidence"], evidence, 0o600)
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                validate()
            observed = raised.exception.code
            restore()
            self.assertEqual(
                observed, "audited_recovery_cleanup_root_progress_invalid"
            )

            cleanup_root = pathlib.Path(
                json.loads(self.fixture.manifest_path.read_bytes())["database"][
                    "cluster_root"
                ]
            ).parent
            real_lexists = os.path.lexists
            with mock.patch.object(
                BOOTSTRAP.os.path,
                "lexists",
                side_effect=lambda path: (
                    True if pathlib.Path(path) == cleanup_root
                    else real_lexists(path)
                ),
            ):
                with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                    validate()
            self.assertEqual(
                raised.exception.code,
                "audited_recovery_cleanup_root_progress_invalid",
            )

            fence = json.loads(fence_path.read_bytes())
            fence["updated_at"] = "2026-08-12T09:00:04Z"
            write_file(fence_path, fence, 0o600)
            evidence = json.loads(paths["evidence"].read_bytes())
            evidence["teardown_fence_sha256"] = hashlib.sha256(
                fence_path.read_bytes()
            ).hexdigest()
            write_file(paths["evidence"], evidence, 0o600)
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                validate()
            observed = raised.exception.code
            restore()
            self.assertEqual(
                observed, "audited_recovery_teardown_fence_invalid"
            )

            interlock = json.loads(interlock_path.read_bytes())
            transition = json.loads(paths["source_transition"].read_bytes())
            for artifact in (interlock, transition):
                artifact["keychain_inventory_sha256"] = "0" * 64
            write_file(interlock_path, interlock, 0o600)
            transition["interlock_sha256"] = hashlib.sha256(
                interlock_path.read_bytes()
            ).hexdigest()
            write_file(paths["source_transition"], transition, 0o600)
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                validate({
                    **output,
                    "source_transition_sha256": hashlib.sha256(
                        paths["source_transition"].read_bytes()
                    ).hexdigest(),
                })
            observed = raised.exception.code
            restore()
            self.assertEqual(
                observed, "audited_recovery_source_transition_invalid"
            )

            interlock = json.loads(interlock_path.read_bytes())
            transition = json.loads(paths["source_transition"].read_bytes())
            for artifact in (interlock, transition):
                artifact["keychain_anchor_inventory_sha256"] = "0" * 64
            write_file(interlock_path, interlock, 0o600)
            transition["interlock_sha256"] = hashlib.sha256(
                interlock_path.read_bytes()
            ).hexdigest()
            write_file(paths["source_transition"], transition, 0o600)
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                validate({
                    **output,
                    "source_transition_sha256": hashlib.sha256(
                        paths["source_transition"].read_bytes()
                    ).hexdigest(),
                })
            observed = raised.exception.code
            restore()
            self.assertEqual(
                observed, "audited_recovery_source_transition_invalid"
            )

    def test_audited_recovery_git_transition_is_single_parent_and_exact(self):
        parent = BOOTSTRAP.AUDITED_QUARANTINED_INCIDENT[
            "intent_source_commit_sha"
        ]
        current = "e" * 40
        tree = "f" * 40
        blobs = {
            path: ("blob:" + path).encode("utf-8")
            for path in BOOTSTRAP.AUDITED_QUARANTINED_CHANGED_PATHS
        }
        transition = {
            "parent_commit_sha": parent,
            "parent_count": 1,
            "file_sha256": {
                path: {
                    "from_sha256": BOOTSTRAP.AUDITED_QUARANTINED_CHANGED_PATH_FROM_SHA256[path],
                    "to_sha256": hashlib.sha256(blob).hexdigest(),
                }
                for path, blob in blobs.items()
            },
        }
        responses = [
            f"{current} {parent}\n".encode(),
            f"{tree}\n".encode(),
            ("\n".join(BOOTSTRAP.AUDITED_QUARANTINED_CHANGED_PATHS) + "\n").encode(),
            *[blobs[path] for path in BOOTSTRAP.AUDITED_QUARANTINED_CHANGED_PATHS],
        ]
        observed_environments = []

        def run(argv, **kwargs):
            observed_environments.append(kwargs["env"])
            return subprocess.CompletedProcess(
                argv, 0, stdout=responses.pop(0), stderr=b""
            )

        with mock.patch.object(BOOTSTRAP.subprocess, "run", side_effect=run):
            BOOTSTRAP.validate_audited_source_transition_git(
                transition,
                current,
                tree,
                self.fixture.root,
                expected_parent_commit=parent,
                expected_from_file_sha256=(
                    BOOTSTRAP.AUDITED_QUARANTINED_CHANGED_PATH_FROM_SHA256
                ),
            )
        self.assertFalse(responses)
        for environment in observed_environments:
            self.assertEqual(environment["GIT_CONFIG_GLOBAL"], "/dev/null")
            self.assertEqual(environment["GIT_CONFIG_NOSYSTEM"], "1")
            self.assertEqual(environment["GIT_TERMINAL_PROMPT"], "0")
            self.assertEqual(environment["GIT_OPTIONAL_LOCKS"], "0")

    def test_audited_recovery_rejects_tunnel_script_mode_mismatch(self):
        state_path, state = self.create_quarantined_direct_onboard_failure()
        incident, output, _paths = self.write_audited_recovery_artifacts(
            state_path, state
        )
        tunnel_script = (
            self.fixture.manifest_path.parent / "orchestrator" / "run-tunnel.zsh"
        )
        self.assertEqual(stat.S_IMODE(tunnel_script.stat().st_mode), 0o700)
        tunnel_script.chmod(0o600)
        with mock.patch.dict(
            BOOTSTRAP.AUDITED_QUARANTINED_INCIDENT,
            incident,
            clear=True,
        ), mock.patch.object(
            BOOTSTRAP, "validate_audited_source_transition_git"
        ):
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                BOOTSTRAP.validate_audited_quarantined_recovery_output(
                    output,
                    state,
                    state_path,
                    "e" * 40,
                    "f" * 40,
                    self.fixture.root,
                )
        self.assertEqual(
            raised.exception.code,
            "audited_recovery_run_tunnel_invalid",
        )

    def test_direct_onboarding_failure_and_evidence_tamper_both_cleanup(self):
        for options, expected in (
            ({"fail_once": {"direct_onboard": 1}}, "direct_onboard_failed"),
            (
                {"direct_evidence_tamper": "persisted_mismatch"},
                "direct_onboarding_stdout_mismatch",
            ),
            (
                {"direct_evidence_tamper": "mode"},
                "direct_onboarding_evidence_invalid",
            ),
            (
                {"direct_evidence_tamper": "hardlink"},
                "direct_onboarding_evidence_invalid",
            ),
            (
                {"direct_evidence_tamper": "commercial_artifact"},
                "commercial_onboarding_artifact_rejected",
            ),
        ):
            with self.subTest(expected=expected):
                executor = FakeExecutor(self.fixture, **options)
                result = self.fixture.controller(executor).run(
                    self.fixture.config_path,
                    self.fixture.candidate_spec_path,
                    "auth-smoke",
                )
                self.assertEqual(result["status"], "failed")
                self.assertEqual(result["error_code"], expected)
                self.assertTrue(result["discord_teardown_complete"])
                self.assertTrue(result["cleanup_complete"])
                failed_at = executor.calls.index("direct_onboard")
                self.assertEqual(
                    executor.calls[failed_at + 1 :],
                    ["teardown_discord_resources", "cleanup", "status"],
                )
                if expected != "commercial_onboarding_artifact_rejected":
                    for relative in BOOTSTRAP.COMMERCIAL_ONBOARDING_ARTIFACTS:
                        self.assertFalse(
                            os.path.lexists(self.fixture.manifest_path.parent / relative)
                        )

                self.fixture.cleanup()
                self.fixture = BootstrapFixture(self)

    def test_direct_onboarding_allowlisted_diagnostic_is_durable_and_secret_safe(self):
        executor = FakeExecutor(
            self.fixture,
            direct_onboard_error_code="discord_hub_preflight_failed",
        )
        result = self.fixture.controller(executor).run(
            self.fixture.config_path,
            self.fixture.candidate_spec_path,
            "auth-smoke",
        )
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["error_code"], "discord_hub_preflight_failed")
        state = json.loads(pathlib.Path(result["state"]).read_bytes())
        self.assertEqual(state["last_error"], "discord_hub_preflight_failed")
        self.assertNotIn("error:", pathlib.Path(result["state"]).read_text())
        self.assertTrue(result["discord_teardown_complete"])
        self.assertTrue(result["cleanup_complete"])

    def test_direct_onboarding_diagnostic_allowlist_is_exact(self):
        self.assertEqual(
            BOOTSTRAP.DIRECT_ONBOARD_ISSUER_ERROR_CODES,
            frozenset(
                {
                    "api_loopback_origin_invalid",
                    "api_loopback_connect_failed",
                    "api_loopback_write_failed",
                    "api_loopback_read_failed",
                    "api_loopback_response_empty",
                    "api_loopback_status_invalid",
                    "discord_hub_preflight_failed",
                    "session_lifecycle_binary_invalid",
                    "session_lifecycle_source_invalid",
                    "session_lifecycle_boot_identity_invalid",
                    "session_lifecycle_existing_marker_invalid",
                    "session_lifecycle_handoff_invalid",
                    "session_lifecycle_reentry_invalid",
                    "session_lifecycle_cas_failed",
                }
            ),
        )

    def test_direct_onboarding_unknown_or_multiline_stderr_remains_flattened(self):
        for diagnostic in (
            b"error: not_allowlisted\n",
            b"error: api_loopback_response_empty\nuntrusted detail\n",
            b"error: api_loopback_response_empty",
        ):
            with self.subTest(diagnostic=diagnostic):
                executor = FakeExecutor(
                    self.fixture,
                    fail_once={"direct_onboard": 1},
                    secret_stderr=diagnostic,
                )
                result = self.fixture.controller(executor).run(
                    self.fixture.config_path,
                    self.fixture.candidate_spec_path,
                    "auth-smoke",
                )
                self.assertEqual(result["error_code"], "direct_onboard_failed")
                state_payload = pathlib.Path(result["state"]).read_bytes()
                self.assertNotIn(diagnostic.rstrip(b"\n"), state_payload)
                self.fixture.cleanup()
                self.fixture = BootstrapFixture(self)

    def test_direct_onboarding_diagnostic_requires_exact_exit_and_empty_stdout(self):
        diagnostic = b"error: api_loopback_status_invalid\n"
        for completed in (
            subprocess.CompletedProcess(
                ["issuer"], 1, stdout=b"unexpected\n", stderr=diagnostic
            ),
            subprocess.CompletedProcess(
                ["issuer"], 2, stdout=b"", stderr=diagnostic
            ),
        ):
            with self.subTest(returncode=completed.returncode, stdout=completed.stdout):
                with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                    BOOTSTRAP.parse_child_json(completed, "direct_onboard")
                self.assertEqual(raised.exception.code, "direct_onboard_failed")

        successful = subprocess.CompletedProcess(
            ["issuer"],
            0,
            stdout=b'{"outcome":"fresh"}\n',
            stderr=diagnostic,
        )
        with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
            BOOTSTRAP.parse_child_json(successful, "direct_onboard")
        self.assertEqual(raised.exception.code, "direct_onboard_output_invalid")

    def test_one_shot_command_error_is_closed_canonical_and_exit_bound(self):
        self.assertEqual(
            BOOTSTRAP.ONE_SHOT_COMMAND_ERROR_CODES,
            frozenset(
                {
                    "d2a_one_shot_authoring_flow_failed",
                    "d2a_one_shot_confirmation_flow_failed",
                    "d2a_one_shot_convergence_flow_failed",
                    "d2a_one_shot_decision_flow_failed",
                    "d2a_one_shot_dependency_timeout",
                    "d2a_one_shot_dependency_unavailable",
                    "d2a_one_shot_deployment_flow_failed",
                    "d2a_one_shot_product_request_failed",
                }
            ),
        )
        for code in BOOTSTRAP.ONE_SHOT_COMMAND_ERROR_CODES:
            envelope = {
                "error_code": code,
                "kind": BOOTSTRAP.COMMAND_ERROR_KIND,
                "operation": "one-shot",
                "schema_version": 1,
            }
            completed = subprocess.CompletedProcess(
                ["d2a"],
                1,
                stdout=(BOOTSTRAP.canonical_json(envelope) + "\n").encode("ascii"),
                stderr=b"",
            )
            with self.subTest(code=code), self.assertRaises(
                BOOTSTRAP.BootstrapError
            ) as raised:
                BOOTSTRAP.parse_child_json(completed, "d2a_one_shot")
            self.assertEqual(raised.exception.code, code)

        valid = {
            "error_code": "d2a_one_shot_dependency_timeout",
            "kind": BOOTSTRAP.COMMAND_ERROR_KIND,
            "operation": "one-shot",
            "schema_version": 1,
        }
        canonical = (BOOTSTRAP.canonical_json(valid) + "\n").encode("ascii")
        malformed = [
            subprocess.CompletedProcess(["d2a"], 1, canonical, b"secret"),
            subprocess.CompletedProcess(["d2a"], 2, canonical, b""),
            subprocess.CompletedProcess(["d2a"], True, canonical, b""),
            subprocess.CompletedProcess(["d2a"], 1, canonical[:-1], b""),
            subprocess.CompletedProcess(["d2a"], 1, b" " + canonical, b""),
            subprocess.CompletedProcess(
                ["d2a"],
                1,
                b'{"error_code":"d2a_one_shot_dependency_timeout",'
                b'"error_code":"d2a_one_shot_dependency_unavailable",'
                b'"kind":"starring.d2a.command-error.v1","operation":"one-shot",'
                b'"schema_version":1}\n',
                b"",
            ),
            subprocess.CompletedProcess(
                ["d2a"],
                1,
                b'{"error_code":"product_request_504_dependency_timeout",'
                b'"kind":"starring.d2a.command-error.v1","operation":"one-shot",'
                b'"schema_version":1}\n',
                b"",
            ),
            subprocess.CompletedProcess(
                ["d2a"],
                1,
                b'{"error_code":"scenario_confirmation_mismatch",'
                b'"kind":"starring.d2a.command-error.v1","operation":"one-shot",'
                b'"schema_version":1}\n',
                b"",
            ),
            subprocess.CompletedProcess(
                ["d2a"],
                1,
                b'{"error_code":"d2a_one_shot_dependency_timeout","extra":"secret",'
                b'"kind":"starring.d2a.command-error.v1","operation":"one-shot",'
                b'"schema_version":1}\n',
                b"",
            ),
            subprocess.CompletedProcess(["d2a"], 1, "비밀".encode(), b""),
            subprocess.CompletedProcess(
                ["d2a"], 1, b"x" * (BOOTSTRAP.MAX_COMMAND_ERROR_BYTES + 1), b""
            ),
        ]
        exceeded = subprocess.CompletedProcess(["d2a"], 1, canonical, b"")
        exceeded.output_exceeded = True
        malformed.append(exceeded)
        for completed in malformed:
            with self.subTest(
                returncode=completed.returncode,
                stdout=completed.stdout[:80],
            ), self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                BOOTSTRAP.parse_child_json(completed, "d2a_one_shot")
            self.assertEqual(raised.exception.code, "d2a_one_shot_failed")
            self.assertNotIn("secret", raised.exception.code)

    def test_one_shot_safe_error_survives_recovery_without_raw_diagnostics(self):
        for code in sorted(BOOTSTRAP.ONE_SHOT_COMMAND_ERROR_CODES):
            executor = FakeExecutor(
                self.fixture,
                fail_once={"d2a_one_shot": 1},
                one_shot_error_code=code,
            )
            result = self.fixture.controller(executor).run(
                self.fixture.config_path,
                self.fixture.candidate_spec_path,
                "one-shot",
            )
            with self.subTest(code=code):
                self.assertEqual(result["status"], "failed")
                self.assertEqual(result["error_code"], code)
                state_payload = pathlib.Path(result["state"]).read_bytes()
                state = json.loads(state_payload)
                self.assertEqual(state["last_error"], code)
                self.assertNotIn(
                    b"product_request_504_dependency_timeout", state_payload
                )
                self.assertNotIn(b"scenario_confirmation_mismatch", state_payload)
                self.assertTrue(result["discord_teardown_complete"])
                self.assertTrue(result["cleanup_complete"])
                self.assertTrue(result["total_local_absence"])
                self.assertTrue(result["protected_staging_unchanged"])
            self.fixture.cleanup()
            self.fixture = BootstrapFixture(self)

    def test_teardown_diagnostic_is_closed_canonical_and_durable(self):
        self.assertEqual(
            BOOTSTRAP.TEARDOWN_DIAGNOSTIC_CODES,
            frozenset(
                {
                    "candidate_health_unready",
                    "transport_control_unavailable",
                    "discord_resource_teardown_final_drift",
                }
            ),
        )
        for child_code in BOOTSTRAP.TEARDOWN_DIAGNOSTIC_CODES:
            stderr = (
                BOOTSTRAP.canonical_json({"code": child_code, "status": "failed"})
                + "\n"
            ).encode("ascii")
            completed = subprocess.CompletedProcess(
                ["orchestrator"], 1, stdout=b"", stderr=stderr
            )
            expected = f"teardown_discord_resources_{child_code}"
            with self.subTest(child_code=child_code), self.assertRaises(
                BOOTSTRAP.BootstrapError
            ) as raised:
                BOOTSTRAP.parse_child_json(completed, "teardown_discord_resources")
            self.assertEqual(raised.exception.code, expected)

        canonical = (
            BOOTSTRAP.canonical_json(
                {"code": "candidate_health_unready", "status": "failed"}
            )
            + "\n"
        ).encode("ascii")
        malformed = [
            subprocess.CompletedProcess(["orchestrator"], 1, b"unexpected", canonical),
            subprocess.CompletedProcess(["orchestrator"], 2, b"", canonical),
            subprocess.CompletedProcess(["orchestrator"], 1, b"", canonical[:-1]),
            subprocess.CompletedProcess(["orchestrator"], 1, b"", b" " + canonical),
            subprocess.CompletedProcess(
                ["orchestrator"],
                1,
                b"",
                b'{"code":"candidate_health_unready","code":"transport_control_unavailable",'
                b'"status":"failed"}\n',
            ),
            subprocess.CompletedProcess(
                ["orchestrator"],
                1,
                b"",
                b'{"code":"secret_token","status":"failed"}\n',
            ),
            subprocess.CompletedProcess(
                ["orchestrator"],
                1,
                b"",
                b'{"code":"candidate_health_unready","extra":"secret",'
                b'"status":"failed"}\n',
            ),
            subprocess.CompletedProcess(
                ["orchestrator"], 1, b"", "비밀".encode("utf-8")
            ),
            subprocess.CompletedProcess(
                ["orchestrator"], 1, b"", b"x" * (BOOTSTRAP.MAX_COMMAND_ERROR_BYTES + 1)
            ),
        ]
        exceeded = subprocess.CompletedProcess(
            ["orchestrator"], 1, b"", canonical
        )
        exceeded.output_exceeded = True
        malformed.append(exceeded)
        for completed in malformed:
            with self.subTest(stderr=completed.stderr[:80]), self.assertRaises(
                BOOTSTRAP.BootstrapError
            ) as raised:
                BOOTSTRAP.parse_child_json(completed, "teardown_discord_resources")
            self.assertEqual(
                raised.exception.code,
                "teardown_discord_resources_failed",
            )
            self.assertNotIn("secret", raised.exception.code)

        child_code = "transport_control_unavailable"
        executor = FakeExecutor(
            self.fixture,
            fail_once={"teardown_discord_resources": 1},
            teardown_error_code=child_code,
        )
        first = self.fixture.controller(executor).run(
            self.fixture.config_path,
            self.fixture.candidate_spec_path,
            "one-shot",
        )
        expected = f"teardown_discord_resources_{child_code}"
        self.assertEqual(first["error_code"], expected)
        state_path = pathlib.Path(first["state"])
        self.assertEqual(json.loads(state_path.read_bytes())["last_error"], expected)
        resumed = self.fixture.controller(FakeExecutor(self.fixture)).resume(state_path)
        self.assertEqual(resumed["error_code"], expected)
        self.assertTrue(resumed["discord_teardown_complete"])
        self.assertTrue(resumed["cleanup_complete"])
        self.assertTrue(resumed["total_local_absence"])
        self.assertTrue(resumed["protected_staging_unchanged"])

    def test_teardown_failure_forbids_cleanup_and_resume_only_recovers(self):
        executor = FakeExecutor(
            self.fixture,
            fail_once={"teardown_discord_resources": 2},
        )
        controller = self.fixture.controller(executor)
        first = controller.run(
            self.fixture.config_path,
            self.fixture.candidate_spec_path,
            "one-shot",
        )
        self.assertEqual(first["status"], "failed")
        self.assertFalse(first["discord_teardown_complete"])
        self.assertFalse(first["cleanup_complete"])
        self.assertNotIn("cleanup", executor.calls)

        # Recovery is bound to the durable non-secret state and exact taint; it
        # must not need to reopen credential-reference config after mutation.
        self.fixture.config_path.unlink()
        self.fixture.candidate_bundle.chmod(0o700)
        self.fixture.candidate_spec_path.unlink()
        resume_executor = FakeExecutor(self.fixture)
        resumed = self.fixture.controller(resume_executor).resume(pathlib.Path(first["state"]))
        self.assertEqual(resumed["status"], "failed")
        self.assertEqual(resumed["error_code"], "teardown_discord_resources_failed")
        self.assertTrue(resumed["discord_teardown_complete"])
        self.assertTrue(resumed["cleanup_complete"])
        self.assertEqual(
            resume_executor.calls,
            ["teardown_discord_resources", "cleanup", "status"],
        )

    def test_postconditions_require_current_manifest_launchd_overrides_absent(self):
        controller = self.fixture.controller(FakeExecutor(self.fixture))
        status = {
            "status": "observed",
            "phase": "cleaned",
            "postgres_running": False,
            "candidate_launchd_jobs_loaded": 0,
            "candidate_launchd_overrides_absent": False,
            "protected_staging_unchanged": True,
        }
        with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
            controller.validate_postconditions(
                status,
                {"run_id": "d2-20260812t000000z-000000000000"},
            )
        self.assertEqual(raised.exception.code, "postconditions_invalid")

    def test_passed_terminal_resume_replays_cleanup_and_live_status(self):
        first = self.fixture.controller(FakeExecutor(self.fixture)).run(
            self.fixture.config_path,
            self.fixture.candidate_spec_path,
            "one-shot",
        )
        self.assertEqual(first["status"], "passed")
        self.assertTrue(first["total_local_absence"])

        resume_executor = FakeExecutor(self.fixture)
        resumed = self.fixture.controller(resume_executor).resume(
            pathlib.Path(first["state"])
        )
        self.assertEqual(resumed["status"], "passed")
        self.assertIsNone(resumed["error_code"])
        self.assertTrue(resumed["cleanup_complete"])
        self.assertTrue(resumed["total_local_absence"])
        self.assertEqual(resume_executor.calls, ["cleanup", "status"])
        state = json.loads(pathlib.Path(first["state"]).read_bytes())
        self.assertEqual(state["status"], "passed")
        self.assertEqual(state["phase"], "complete")

    def test_passed_terminal_resume_rejects_live_launchd_override(self):
        first = self.fixture.controller(FakeExecutor(self.fixture)).run(
            self.fixture.config_path,
            self.fixture.candidate_spec_path,
            "one-shot",
        )
        resume_executor = FakeExecutor(
            self.fixture,
            candidate_launchd_overrides_absent=False,
        )
        resumed = self.fixture.controller(resume_executor).resume(
            pathlib.Path(first["state"])
        )
        self.assertEqual(resumed["status"], "failed")
        self.assertEqual(resumed["error_code"], "postconditions_invalid")
        self.assertTrue(resumed["cleanup_complete"])
        self.assertFalse(resumed["total_local_absence"])
        self.assertFalse(resumed["protected_staging_unchanged"])
        self.assertEqual(resume_executor.calls, ["cleanup", "status"])
        state = json.loads(pathlib.Path(first["state"]).read_bytes())
        self.assertEqual(state["status"], "passed")
        self.assertEqual(state["phase"], "complete")
        self.assertIsNone(state["last_error"])
        self.assertFalse(state["postconditions_complete"])

        retry_executor = FakeExecutor(self.fixture)
        retried = self.fixture.controller(retry_executor).resume(
            pathlib.Path(first["state"])
        )
        self.assertEqual(retried["status"], "passed")
        self.assertIsNone(retried["error_code"])
        self.assertTrue(retried["cleanup_complete"])
        self.assertTrue(retried["total_local_absence"])
        self.assertEqual(retry_executor.calls, ["cleanup", "status"])

    def test_failed_terminal_resume_replays_cleanup_and_preserves_error(self):
        first = self.fixture.controller(
            FakeExecutor(
                self.fixture,
                direct_onboard_error_code="discord_hub_preflight_failed",
            )
        ).run(
            self.fixture.config_path,
            self.fixture.candidate_spec_path,
            "auth-smoke",
        )
        self.assertEqual(first["error_code"], "discord_hub_preflight_failed")
        self.assertTrue(first["total_local_absence"])

        resume_executor = FakeExecutor(self.fixture)
        resumed = self.fixture.controller(resume_executor).resume(
            pathlib.Path(first["state"])
        )
        self.assertEqual(resumed["status"], "failed")
        self.assertEqual(resumed["error_code"], "discord_hub_preflight_failed")
        self.assertTrue(resumed["cleanup_complete"])
        self.assertTrue(resumed["total_local_absence"])
        self.assertEqual(resume_executor.calls, ["cleanup", "status"])

    def test_failed_terminal_resume_rejects_live_launchd_override(self):
        first = self.fixture.controller(
            FakeExecutor(
                self.fixture,
                direct_onboard_error_code="discord_hub_preflight_failed",
            )
        ).run(
            self.fixture.config_path,
            self.fixture.candidate_spec_path,
            "auth-smoke",
        )
        resume_executor = FakeExecutor(
            self.fixture,
            candidate_launchd_overrides_absent=False,
        )
        resumed = self.fixture.controller(resume_executor).resume(
            pathlib.Path(first["state"])
        )
        self.assertEqual(resumed["status"], "failed")
        self.assertEqual(resumed["error_code"], "postconditions_invalid")
        self.assertTrue(resumed["cleanup_complete"])
        self.assertFalse(resumed["total_local_absence"])
        state = json.loads(pathlib.Path(first["state"]).read_bytes())
        self.assertEqual(state["last_error"], "discord_hub_preflight_failed")
        self.assertFalse(state["postconditions_complete"])
        self.assertEqual(resume_executor.calls, ["cleanup", "status"])

        retry_executor = FakeExecutor(self.fixture)
        retried = self.fixture.controller(retry_executor).resume(
            pathlib.Path(first["state"])
        )
        self.assertEqual(retried["status"], "failed")
        self.assertEqual(
            retried["error_code"],
            "discord_hub_preflight_failed",
        )
        self.assertTrue(retried["cleanup_complete"])
        self.assertTrue(retried["total_local_absence"])
        self.assertEqual(retry_executor.calls, ["cleanup", "status"])

    def test_resume_missing_orchestrator_state_does_not_claim_local_absence(self):
        executor = FakeExecutor(
            self.fixture,
            fail_once={"teardown_discord_resources": 2},
        )
        first = self.fixture.controller(executor).run(
            self.fixture.config_path,
            self.fixture.candidate_spec_path,
            "one-shot",
        )
        orchestrator_state = (
            self.fixture.manifest_path.parent / "orchestrator" / "state.json"
        )
        orchestrator_state.unlink()

        resume_executor = FakeExecutor(self.fixture)
        resumed = self.fixture.controller(resume_executor).resume(
            pathlib.Path(first["state"])
        )
        self.assertEqual(resumed["error_code"], "orchestrator_state_missing")
        self.assertFalse(resumed["discord_teardown_complete"])
        self.assertFalse(resumed["cleanup_complete"])
        self.assertFalse(resumed["total_local_absence"])
        self.assertEqual(resume_executor.calls, [])

    def test_resume_missing_manifest_does_not_claim_local_absence(self):
        executor = FakeExecutor(
            self.fixture,
            fail_once={"teardown_discord_resources": 2},
        )
        first = self.fixture.controller(executor).run(
            self.fixture.config_path,
            self.fixture.candidate_spec_path,
            "one-shot",
        )
        self.fixture.manifest_path.unlink()

        resume_executor = FakeExecutor(self.fixture)
        resumed = self.fixture.controller(resume_executor).resume(
            pathlib.Path(first["state"])
        )
        self.assertEqual(resumed["error_code"], "manifest_missing")
        self.assertFalse(resumed["discord_teardown_complete"])
        self.assertFalse(resumed["cleanup_complete"])
        self.assertFalse(resumed["total_local_absence"])
        self.assertEqual(resume_executor.calls, [])

    def test_interrupt_is_redacted_and_cleanup_still_runs(self):
        credential = b"__Host-starring_session=THIS_MUST_NOT_LEAK"
        executor = FakeExecutor(
            self.fixture,
            fail_once={"d2a_auth_smoke": 1},
            secret_stderr=credential,
        )
        result = self.fixture.controller(executor).run(
            self.fixture.config_path,
            self.fixture.candidate_spec_path,
            "auth-smoke",
        )
        rendered = BOOTSTRAP.canonical_json(result).encode()
        self.assertNotIn(credential, rendered)
        self.assertNotIn(credential, pathlib.Path(result["state"]).read_bytes())
        self.assertEqual(result["error_code"], "manual_recovery_required")
        self.assertEqual(executor.calls[-1:], ["d2a_auth_smoke"])

        interrupt_executor = FakeExecutor(self.fixture, interrupt_at="d2a_auth_smoke")
        interrupted = self.fixture.controller(interrupt_executor).run(
            self.fixture.config_path,
            self.fixture.candidate_spec_path,
            "auth-smoke",
        )
        self.assertEqual(interrupted["error_code"], "manual_recovery_required")
        self.assertFalse(interrupted["cleanup_complete"])
        self.assertEqual(interrupt_executor.calls[-1:], ["d2a_auth_smoke"])

    def test_global_lock_refuses_overlap_before_any_command(self):
        executor = FakeExecutor(self.fixture)
        controller = self.fixture.controller(executor)
        with BOOTSTRAP.bootstrap_lock(self.fixture.lock_path):
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                controller.run(
                    self.fixture.config_path,
                    self.fixture.candidate_spec_path,
                    "auth-smoke",
                )
        self.assertEqual(raised.exception.code, "bootstrap_lock_busy")
        self.assertEqual(executor.calls, [])

    def test_bootstrap_lock_is_distinct_and_direct_issuer_can_take_its_lock(self):
        self.assertEqual(
            BOOTSTRAP.GLOBAL_LOCK_PATH,
            pathlib.Path("/private/tmp/starring-d2a-bootstrap.lock"),
        )
        self.assertNotEqual(
            BOOTSTRAP.GLOBAL_LOCK_PATH,
            BOOTSTRAP.ISSUER_GLOBAL_D2_LOCK_PATH,
        )
        with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
            with BOOTSTRAP.bootstrap_lock(BOOTSTRAP.ISSUER_GLOBAL_D2_LOCK_PATH):
                pass
        self.assertEqual(raised.exception.code, "bootstrap_lock_conflict")

        executor = FakeExecutor(self.fixture, probe_issuer_lock=True)
        result = self.fixture.controller(executor).run(
            self.fixture.config_path,
            self.fixture.candidate_spec_path,
            "auth-smoke",
        )
        self.assertEqual(result["status"], "passed")
        self.assertTrue(executor.issuer_lock_acquired)

    def test_issuer_build_failure_records_durable_quiescent_lifecycle(self):
        executor = FakeExecutor(
            self.fixture,
            fail_once={"issuer_build": 1},
            secret_stderr=b"cargo diagnostic with SECRET_CREDENTIAL_VALUE",
        )
        with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
            self.fixture.controller(executor).run(
                self.fixture.config_path,
                self.fixture.candidate_spec_path,
                "auth-smoke",
            )
        self.assertEqual(raised.exception.code, "issuer_build_failed")
        self.assertEqual(
            executor.calls,
            [
                "source_root",
                "source_commit",
                "source_tree",
                "source_status",
                "cargo_version",
                "rustc_verbose_version",
                "issuer_build",
            ],
        )
        lifecycle = json.loads(
            (self.fixture.state_root / "issuer-build-lifecycle.json").read_bytes()
        )
        self.assertEqual(lifecycle["status"], "failed")
        self.assertTrue(lifecycle["process_group_quiescent"])
        self.assertEqual(lifecycle["error_code"], "issuer_build_failed")
        self.assertEqual(list(self.fixture.state_root.glob("bootstrap-*.json")), [])
        self.assertFalse(self.fixture.release_root.exists())

    def test_wrong_cargo_version_or_rust_host_is_rejected_before_build(self):
        for bad_toolchain, expected, calls in (
            (
                "cargo",
                "cargo_version_invalid",
                [
                    "source_root",
                    "source_commit",
                    "source_tree",
                    "source_status",
                    "cargo_version",
                ],
            ),
            (
                "host",
                "rustc_version_invalid",
                [
                    "source_root",
                    "source_commit",
                    "source_tree",
                    "source_status",
                    "cargo_version",
                    "rustc_verbose_version",
                ],
            ),
        ):
            with self.subTest(bad_toolchain=bad_toolchain):
                executor = FakeExecutor(self.fixture, bad_toolchain=bad_toolchain)
                with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                    self.fixture.controller(executor).run(
                        self.fixture.config_path,
                        self.fixture.candidate_spec_path,
                        "auth-smoke",
                    )
                self.assertEqual(raised.exception.code, expected)
                self.assertEqual(executor.calls, calls)
                self.assertTrue(self.fixture.state_root.is_dir())
                self.assertEqual(list(self.fixture.state_root.iterdir()), [])
                self.assertFalse(self.fixture.release_root.exists())

    def test_toolchain_change_during_build_retains_durable_build_lifecycle(self):
        executor = FakeExecutor(self.fixture, change_toolchain_after_build=True)
        with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
            self.fixture.controller(executor).run(
                self.fixture.config_path,
                self.fixture.candidate_spec_path,
                "auth-smoke",
            )
        self.assertEqual(raised.exception.code, "rust_toolchain_changed")
        self.assertEqual(
            executor.calls,
            [
                "source_root",
                "source_commit",
                "source_tree",
                "source_status",
                "cargo_version",
                "rustc_verbose_version",
                "issuer_build",
            ],
        )
        lifecycle = json.loads(
            (self.fixture.state_root / "issuer-build-lifecycle.json").read_bytes()
        )
        self.assertEqual(lifecycle["status"], "failed")
        self.assertTrue(lifecycle["process_group_quiescent"])
        self.assertEqual(list(self.fixture.state_root.glob("bootstrap-*.json")), [])
        self.assertFalse(self.fixture.release_root.exists())

    def test_resolved_darwin_or_sdk_drift_fails_before_issuer_publish(self):
        calls = 0

        def changing_provider():
            nonlocal calls
            calls += 1
            value = copy.deepcopy(self.fixture.fixture_darwin_toolchain())
            if calls > 1:
                value["sdk"]["sha256"] = "0" * 64
            return value

        controller = BOOTSTRAP.BootstrapController(
            executor=FakeExecutor(self.fixture),
            tool_root=self.fixture.tool_root,
            certification_root=self.fixture.certification_root,
            lock_path=self.fixture.lock_path,
            rust_toolchain_bin=self.fixture.rust_toolchain_bin,
            expected_release_root=self.fixture.release_root,
            source_root=self.fixture.root,
            git_path=self.fixture.git_path,
            darwin_toolchain_provider=changing_provider,
        )
        with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
            controller.run(
                self.fixture.config_path,
                self.fixture.candidate_spec_path,
                "auth-smoke",
            )
        self.assertEqual(raised.exception.code, "rust_toolchain_changed")
        fixed = (
            self.fixture.tool_root
            / "session-issuer"
            / "target"
            / "release"
            / "starring-d2-session-issuer"
        )
        self.assertFalse(os.path.lexists(fixed))

    def test_source_must_be_clean_candidate_commit_and_tree_before_build(self):
        for bad_source in ("commit", "dirty"):
            with self.subTest(bad_source=bad_source):
                executor = FakeExecutor(self.fixture, bad_source=bad_source)
                with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                    self.fixture.controller(executor).run(
                        self.fixture.config_path,
                        self.fixture.candidate_spec_path,
                        "auth-smoke",
                    )
                self.assertEqual(raised.exception.code, "source_revision_mismatch")
                self.assertEqual(
                    executor.calls,
                    ["source_root", "source_commit", "source_tree", "source_status"],
                )
                self.assertFalse(self.fixture.state_root.exists())
                self.assertFalse(self.fixture.release_root.exists())

    def test_active_or_quarantined_issuer_build_never_reuses_or_cleans_target(self):
        for status in ("active", "quarantined"):
            with self.subTest(status=status):
                self.fixture.state_root.mkdir(mode=0o700)
                build_id = "d2aib-" + "a" * 32
                target = self.fixture.state_root / f".issuer-build-{build_id}"
                target.mkdir(mode=0o700)
                marker = {
                    "schema_version": 1,
                    "kind": "starring.d2a.issuer-build-lifecycle.v1",
                    "build_id": build_id,
                    "status": status,
                    "source_commit": "c" * 40,
                    "source_tree": "d" * 40,
                    "target_dir": str(target),
                    "process_group_id": 2_000_000_000,
                    "process_group_quiescent": False,
                    "build_environment": {
                        "HOME": str(pathlib.Path.home()),
                        "PATH": f"{self.fixture.rust_toolchain_bin}:/usr/bin:/bin:/usr/sbin:/sbin",
                        "CARGO_HOME": str(target / ".cargo-home-fixture"),
                        "CARGO_INCREMENTAL": "0",
                        "CARGO_NET_OFFLINE": "true",
                        "GIT_CONFIG_NOSYSTEM": "1",
                        "GIT_TERMINAL_PROMPT": "0",
                        "LC_ALL": "C",
                        "RUSTC": str(self.fixture.rust_toolchain_bin / "rustc"),
                        "CC": "/fixture/clang",
                        "CXX": "/fixture/clang",
                        "AR": "/fixture/ar",
                        "RANLIB": "/fixture/ranlib",
                        "SDKROOT": "/fixture/sdk",
                        "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER": "/fixture/clang",
                    },
                    "build_environment_sha256": "",
                    "started_at": "2026-08-12T00:00:00Z",
                    "completed_at": None
                    if status == "active"
                    else "2026-08-12T00:00:01Z",
                    "error_code": None
                    if status == "active"
                    else "issuer_build_failed",
                }
                marker["build_environment_sha256"] = hashlib.sha256(
                    BOOTSTRAP.canonical_json(marker["build_environment"]).encode()
                ).hexdigest()
                BOOTSTRAP.validate_issuer_build_lifecycle(
                    marker, self.fixture.state_root
                )
                BOOTSTRAP.write_private_marker(
                    self.fixture.state_root / "issuer-build-lifecycle.json",
                    marker,
                    "issuer_build_lifecycle",
                )
                executor = FakeExecutor(self.fixture)
                with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                    self.fixture.controller(executor).run(
                        self.fixture.config_path,
                        self.fixture.candidate_spec_path,
                        "auth-smoke",
                    )
                self.assertEqual(raised.exception.code, "manual_recovery_required")
                self.assertTrue(target.is_dir())
                self.assertNotIn("issuer_build", executor.calls)
                self.fixture.cleanup()
                self.fixture = BootstrapFixture(self)

    def test_issuer_publish_rejects_symlinked_target_chain(self):
        issuer_root = self.fixture.tool_root / "session-issuer"
        outside = self.fixture.root / "outside-publish"
        outside.mkdir(mode=0o700)
        (issuer_root / "target").symlink_to(outside, target_is_directory=True)
        executor = FakeExecutor(self.fixture)
        with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
            self.fixture.controller(executor).run(
                self.fixture.config_path,
                self.fixture.candidate_spec_path,
                "auth-smoke",
            )
        self.assertIn(
            raised.exception.code,
            {"issuer_publish_destination_path_invalid", "issuer_publish_root_invalid"},
        )
        self.assertEqual(list(outside.iterdir()), [])

    def test_source_drift_before_auth_fails_and_runs_cleanup(self):
        # Two extra sealed snapshots bracket the final issuer publication.
        executor = FakeExecutor(self.fixture, source_drift_at_snapshot=5)
        result = self.fixture.controller(executor).run(
            self.fixture.config_path,
            self.fixture.candidate_spec_path,
            "auth-smoke",
        )
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["error_code"], "source_changed")
        self.assertTrue(result["discord_teardown_complete"])
        self.assertTrue(result["cleanup_complete"])
        direct_at = executor.calls.index("direct_onboard")
        self.assertNotIn("d2a_auth_smoke", executor.calls[direct_at + 1 :])
        self.assertEqual(
            executor.calls[-3:],
            ["teardown_discord_resources", "cleanup", "status"],
        )

    def test_source_drift_before_publish_leaves_fixed_issuer_unchanged(self):
        fixed = (
            self.fixture.tool_root
            / "session-issuer"
            / "target"
            / "release"
            / "starring-d2-session-issuer"
        )
        write_file(fixed, b"previous-sealed-issuer", 0o755)
        before = fixed.read_bytes()
        executor = FakeExecutor(self.fixture, source_drift_at_snapshot=2)
        with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
            self.fixture.controller(executor).run(
                self.fixture.config_path,
                self.fixture.candidate_spec_path,
                "auth-smoke",
            )
        self.assertEqual(raised.exception.code, "source_changed")
        self.assertEqual(fixed.read_bytes(), before)
        lifecycle = json.loads(
            (self.fixture.state_root / "issuer-build-lifecycle.json").read_bytes()
        )
        self.assertEqual(lifecycle["status"], "failed")
        self.assertTrue(lifecycle["process_group_quiescent"])

    def test_release_root_must_match_the_issuer_fixed_root(self):
        executor = FakeExecutor(self.fixture)
        controller = BOOTSTRAP.BootstrapController(
            executor=executor,
            tool_root=self.fixture.tool_root,
            certification_root=self.fixture.certification_root,
            lock_path=self.fixture.lock_path,
            rust_toolchain_bin=self.fixture.rust_toolchain_bin,
            expected_release_root=self.fixture.root / "different-release-root",
        )
        with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
            controller.run(
                self.fixture.config_path,
                self.fixture.candidate_spec_path,
                "auth-smoke",
            )
        self.assertEqual(raised.exception.code, "release_run_root_invalid")
        self.assertEqual(executor.calls, [])

    def test_cli_errors_are_bounded_and_do_not_echo_untrusted_values(self):
        stdout = StringIO()
        stderr = StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr):
            status = BOOTSTRAP.main(["run", "--operation", "SECRET_SESSION_VALUE"])
        self.assertEqual(status, 1)
        self.assertEqual(stderr.getvalue(), "")
        self.assertNotIn("SECRET_SESSION_VALUE", stdout.getvalue())
        result = json.loads(stdout.getvalue())
        self.assertEqual(result["error_code"], "cli_invalid")
        self.assertEqual(set(result), BOOTSTRAP.RESULT_FIELDS)
        self.assertIsNone(result["onboarding_evidence"])
        self.assertIsNone(result["source_revision"])
        self.assertIsNone(result["candidate_dependencies"])
        self.assertIsNone(result["issuer_toolchain"])

    def test_audited_recovery_cli_requires_all_exact_confirmations(self):
        state = "/absolute/bootstrap-d2-20260812t082042z-c52d220457d1.json"
        arguments = BOOTSTRAP.parser().parse_args(
            [
                "recover-audited-quarantined-no-issue",
                "--state",
                state,
                "--confirm-current-commit",
                "c" * 40,
                "--confirm-current-tree",
                "d" * 40,
                "--confirm-run-id",
                "d2-20260812t082042z-c52d220457d1",
                "--confirm-manifest-sha256",
                "e" * 64,
            ]
        )
        self.assertEqual(arguments.command, BOOTSTRAP.AUDITED_QUARANTINED_RECOVERY_COMMAND)
        self.assertEqual(arguments.state, state)
        self.assertEqual(arguments.confirm_current_commit, "c" * 40)
        self.assertEqual(arguments.confirm_current_tree, "d" * 40)
        self.assertEqual(
            arguments.confirm_run_id,
            "d2-20260812t082042z-c52d220457d1",
        )
        self.assertEqual(arguments.confirm_manifest_sha256, "e" * 64)
        with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
            BOOTSTRAP.parser().parse_args(
                [
                    "recover-audited-quarantined-no-issue",
                    "--state",
                    state,
                ]
            )
        self.assertEqual(raised.exception.code, "cli_invalid")

    def test_boolean_schema_versions_are_rejected(self):
        for validator, value, expected in (
            (
                BOOTSTRAP.validate_config,
                {**self.fixture.config, "schema_version": True},
                "sandbox_config_schema_invalid",
            ),
            (
                BOOTSTRAP.validate_candidate_spec,
                {**self.fixture.candidate_spec, "schema_version": True},
                "candidate_spec_schema_invalid",
            ),
        ):
            with self.subTest(expected=expected):
                with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                    validator(value)
                self.assertEqual(raised.exception.code, expected)

    def test_durable_write_all_handles_short_writes_and_zero_progress(self):
        payload = b"durable-marker-payload"
        with tempfile.TemporaryFile() as handle:
            real_write = os.write

            def short_write(descriptor, value):
                return real_write(descriptor, value[:3])

            with mock.patch.object(BOOTSTRAP.os, "write", side_effect=short_write):
                BOOTSTRAP.write_all(handle.fileno(), payload, "fixture")
            handle.seek(0)
            self.assertEqual(handle.read(), payload)
        with tempfile.TemporaryFile() as handle:
            with mock.patch.object(BOOTSTRAP.os, "write", return_value=0):
                with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                    BOOTSTRAP.write_all(handle.fileno(), payload, "fixture")
            self.assertEqual(raised.exception.code, "fixture_write_failed")

    def test_boot_identity_reboot_parity_probes_only_same_boot(self):
        state = {
            "manifest_path": str(self.fixture.root / "run" / "manifest.json"),
            "last_session_operation": "auth-smoke",
            "run_id": "d2-20260812t000000z-" + "a" * 12,
            "manifest_sha256": "a" * 64,
            "tool_digests": {
                "issuer_sha256": "b" * 64,
                "issuer_source_sha256": "c" * 64,
            },
        }
        lifecycle_path = pathlib.Path(state["manifest_path"]).with_name(
            "d2a-session-lifecycle.json"
        )
        lifecycle = {
            "schema_version": 1,
            "kind": "starring.d2a.session-lifecycle.v1",
            "run_id": state["run_id"],
            "manifest_sha256": state["manifest_sha256"],
            "operation": "auth-smoke",
            "origin": "issuer",
            "issuer_sha256": "b" * 64,
            "issuer_source_sha256": "c" * 64,
            "uid": os.getuid(),
            "boot_identity": "darwin-boottime:1:0",
            "process_group_id": 12345,
            "started_at": "2026-08-12T00:00:00.000000000Z",
            "status": "not_issued",
            "session_revoked": False,
            "revoked_at": None,
            "quarantined_at": None,
        }
        write_file(lifecycle_path, lifecycle, 0o600, sort_keys=False)
        with mock.patch.object(
            BOOTSTRAP, "current_boot_identity", return_value="darwin-boottime:2:0"
        ), mock.patch.object(BOOTSTRAP.os, "killpg") as killpg:
            self.assertEqual(
                BOOTSTRAP.require_revoked_session_lifecycle(state), lifecycle
            )
            killpg.assert_not_called()
        with mock.patch.object(
            BOOTSTRAP, "current_boot_identity", return_value="darwin-boottime:1:0"
        ), mock.patch.object(
            BOOTSTRAP.os, "killpg", side_effect=ProcessLookupError
        ) as killpg:
            self.assertEqual(
                BOOTSTRAP.require_revoked_session_lifecycle(state), lifecycle
            )
            killpg.assert_called_once_with(12345, 0)
        with mock.patch.object(
            BOOTSTRAP, "current_boot_identity", return_value="darwin-boottime:1:0"
        ), mock.patch.object(BOOTSTRAP.os, "killpg", return_value=None):
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                BOOTSTRAP.require_revoked_session_lifecycle(state)
            self.assertEqual(raised.exception.code, "manual_recovery_required")

    def test_nested_cargo_configuration_and_sdk_symlink_escape_are_rejected(self):
        nested = self.fixture.root / "nested" / "crate"
        nested.mkdir(mode=0o700, parents=True)
        write_file(self.fixture.root / "nested" / ".cargo" / "config.toml", "[build]\n", 0o600)
        with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
            BOOTSTRAP.reject_cargo_configuration(nested)
        self.assertEqual(raised.exception.code, "cargo_config_present")

        fake_home = self.fixture.root / "home"
        for name in ("registry", "git"):
            (fake_home / ".cargo" / name).mkdir(mode=0o700, parents=True)
        target = self.fixture.root / "issuer-target"
        target.mkdir(mode=0o700)
        with mock.patch.object(pathlib.Path, "home", return_value=fake_home):
            cargo_home = BOOTSTRAP.prepare_isolated_cargo_home(
                target, "a" * 32
            )
        self.assertEqual(
            {entry.name for entry in cargo_home.iterdir()}, {"registry", "git"}
        )
        self.assertTrue(all(entry.is_symlink() for entry in cargo_home.iterdir()))

        # The SDK helper is deliberately root-only.  A mocked metadata view
        # exercises its broken/escaping-symlink rejection without weakening
        # that production ownership contract for test directories.
        sdk = self.fixture.root / "sdk"
        sdk.mkdir(mode=0o755)
        (sdk / "escape").symlink_to("/private/tmp")
        real_lstat = pathlib.Path.lstat

        def root_owned(path):
            observed = real_lstat(path)
            values = list(observed)
            values[4] = 0
            if stat.S_ISDIR(observed.st_mode):
                values[0] = stat.S_IFDIR | 0o755
            return os.stat_result(values)

        with mock.patch.object(pathlib.Path, "lstat", root_owned):
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                BOOTSTRAP.rooted_tree_digest(sdk, "macos_sdk")
        self.assertEqual(raised.exception.code, "macos_sdk_invalid")
        (sdk / "escape").unlink()
        (sdk / "broken").symlink_to("missing-target")
        with mock.patch.object(pathlib.Path, "lstat", root_owned):
            with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
                BOOTSTRAP.rooted_tree_digest(sdk, "macos_sdk")
        self.assertEqual(raised.exception.code, "macos_sdk_invalid")

    def test_recursive_rust_sysroot_rejects_symlink_and_world_writable_input(self):
        rust_root = self.fixture.rust_toolchain_bin.parent
        escape = rust_root / "escape"
        escape.symlink_to("/private/tmp")
        with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
            BOOTSTRAP.rust_toolchain_manifest(self.fixture.rust_toolchain_bin)
        self.assertEqual(raised.exception.code, "rust_sysroot_invalid")
        escape.unlink()
        nested = rust_root / "lib"
        nested.mkdir(mode=0o777)
        nested.chmod(0o777)
        with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
            BOOTSTRAP.rust_toolchain_manifest(self.fixture.rust_toolchain_bin)
        self.assertEqual(raised.exception.code, "rust_sysroot_invalid")

    def test_real_bounded_supervisor_caps_and_reaps_process_groups(self):
        environment = {
            "HOME": str(pathlib.Path.home()),
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            "LC_ALL": "C",
        }
        result = BOOTSTRAP.bounded_subprocess(
            [sys.executable, "-c", "import os; os.write(1, b'x' * 4097)"],
            self.fixture.root,
            environment,
            10,
            maximum=4096,
        )
        self.assertTrue(result.output_exceeded)
        self.assertTrue(result.process_group_quiescent)

        observed = []

        def reject_marker_write(process_group):
            observed.append(process_group)
            raise BOOTSTRAP.BootstrapError("marker_write_failed")

        with self.assertRaises(BOOTSTRAP.BootstrapError) as raised:
            BOOTSTRAP.bounded_subprocess(
                [sys.executable, "-c", "import time; time.sleep(60)"],
                self.fixture.root,
                environment,
                10,
                on_spawn=reject_marker_write,
            )
        self.assertEqual(raised.exception.code, "marker_write_failed")
        self.assertEqual(len(observed), 1)
        with self.assertRaises(ProcessLookupError):
            os.killpg(observed[0], 0)

        started = time.monotonic()
        result = BOOTSTRAP.bounded_subprocess(
            [
                sys.executable,
                "-c",
                (
                    "import os,subprocess,sys; "
                    "subprocess.Popen([sys.executable,'-c','import time; time.sleep(60)'],"
                    "stdout=sys.stdout,stderr=sys.stderr); os._exit(0)"
                ),
            ],
            self.fixture.root,
            environment,
            10,
        )
        self.assertTrue(result.timed_out)
        self.assertTrue(result.process_group_quiescent)
        self.assertLess(time.monotonic() - started, 8)


if __name__ == "__main__":
    unittest.main()
