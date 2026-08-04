import contextlib
import importlib.util
import io
import json
import os
import pathlib
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


MODULE_PATH = pathlib.Path(__file__).with_name("d3_certification.py")
SPEC = importlib.util.spec_from_file_location("d3_certification", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def run(argv, cwd):
    result = subprocess.run(
        argv,
        cwd=cwd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    )
    return result.stdout.decode("utf-8").strip()


def git(cwd, *arguments):
    return run(["git", *arguments], cwd)


def write(path, value, mode=0o644):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(value, encoding="utf-8")
    path.chmod(mode)


class D3CertificationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name).resolve()
        self.seed = self.root / "seed"
        self.remote = self.root / "origin.git"
        self.repo = self.root / "repo"
        self.output = self.root / "state"
        self.github_repository = "owner/repository"
        self.github_remote_url = "https://github.com/owner/repository.git"
        self.output.mkdir(mode=0o700)
        self.create_repository()

    def tearDown(self):
        self.temporary.cleanup()

    def create_repository(self):
        self.seed.mkdir()
        git(self.seed, "init", "-b", "main")
        git(self.seed, "config", "user.email", "test@example.com")
        git(self.seed, "config", "user.name", "D3 Test")
        write(self.seed / "base.txt", "base\n")
        verifier = """import json
import pathlib
import sys

p = pathlib.Path(sys.argv[sys.argv.index("--manifest") + 1])
m = json.loads(p.read_text(encoding="utf-8"))
r = [json.loads(line) for line in p.with_name("receipts.jsonl").read_text(encoding="utf-8").splitlines()]
d = p.with_name("manifest.sha256").read_text(encoding="ascii").strip()
v = {"schema_version":1,"kind":"starring.d2.coordinator-final-record.v1","run_id":m["run_id"],"commit_sha":m["commit_sha"],"manifest_sha256":d,"steps":len(r),"status":"passed","resource_prefix":m["discord"]["resource_prefix"],"receipt_chain_head_sha256":r[-1]["receipt_sha256"],"coordinator_evidence_sha256":"f"*64}
print(json.dumps(v, ensure_ascii=False, sort_keys=True, separators=(",", ":")))
"""
        write(
            self.seed / "tools" / "d2-certification" / "d2_run.py",
            verifier,
        )
        git(self.seed, "add", ".")
        git(self.seed, "commit", "-m", "base")
        self.base = git(self.seed, "rev-parse", "HEAD")
        git(self.seed, "checkout", "-b", "feature")
        write(self.seed / "feature.txt", "feature\n")
        git(self.seed, "add", ".")
        git(self.seed, "commit", "-m", "feature")
        self.head = git(self.seed, "rev-parse", "HEAD")
        git(self.seed, "checkout", "main")
        git(self.seed, "merge", "--no-ff", "feature", "-m", "merge candidate")
        self.merge = git(self.seed, "rev-parse", "HEAD")
        self.tree = git(self.seed, "show", "-s", "--format=%T", self.merge)
        git(self.seed, "reset", "--hard", self.base)
        git(self.root, "init", "--bare", str(self.remote))
        git(self.seed, "remote", "add", "origin", str(self.remote))
        git(self.seed, "push", "origin", f"{self.base}:refs/heads/main")
        git(self.seed, "push", "origin", f"{self.head}:refs/pull/42/head")
        git(self.seed, "push", "origin", f"{self.merge}:refs/pull/42/merge")
        git(self.root, "clone", str(self.remote), str(self.repo))
        git(self.repo, "remote", "set-url", "origin", self.github_remote_url)
        git(
            self.repo,
            "config",
            f"url.{self.remote.as_uri()}.insteadOf",
            self.github_remote_url,
        )

    def invoke(self, arguments):
        stdout = io.StringIO()
        stderr = io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            status = MODULE.main(arguments)
        result = json.loads(stdout.getvalue()) if stdout.getvalue() else None
        return status, result, stderr.getvalue()

    def prepare_arguments(self, gates=None, output_root=None, expected_head=None):
        gates = list(MODULE.REQUIRED_GATE_COMMANDS if gates is None else gates)
        arguments = [
            "prepare",
            "--repo",
            str(self.repo),
            "--output-root",
            str(self.output if output_root is None else output_root),
            "--pr-number",
            "42",
            "--expected-head",
            self.head if expected_head is None else expected_head,
            "--expected-base",
            self.base,
        ]
        for gate in gates:
            arguments.extend(["--gate", gate])
        return arguments, gates

    def prepare(self, gates=None):
        arguments, gates = self.prepare_arguments(gates)
        status, result, error = self.invoke(arguments)
        self.assertEqual(status, 0, error)
        return pathlib.Path(result["state"]), result, gates

    def invoke_gate_plan(self, state_path, gates=None, outcomes=None):
        gates = list(MODULE.REQUIRED_GATE_COMMANDS if gates is None else gates)
        arguments = ["run-gates", "--state", str(state_path)]
        for gate in gates:
            arguments.extend(["--gate", gate])
        original = MODULE.run_process
        observed = []
        outcomes = iter([] if outcomes is None else outcomes)

        def recording_runner(argv, cwd, label, allowed=(0,), timeout=None, discard=False):
            if argv[:3] == ["/bin/zsh", "-f", "-c"]:
                observed.append(argv[3])
                return next(outcomes, 0), b""
            return original(argv, cwd, label, allowed, timeout, discard)

        with mock.patch.object(MODULE, "run_process", side_effect=recording_runner):
            status, result, error = self.invoke(arguments)
        return status, result, error, observed

    def test_prepare_pins_merge_parents_tree_and_detached_worktree(self):
        state_path, first, gates = self.prepare()
        state = json.loads(state_path.read_text(encoding="utf-8"))
        self.assertEqual(state["merge_commit"], self.merge)
        self.assertEqual(state["merge_tree"], self.tree)
        self.assertEqual(state["merge_parents"], [self.base, self.head])
        self.assertEqual(state["github_repository"], self.github_repository)
        worktree = pathlib.Path(state["worktree_path"])
        self.assertEqual(stat.S_IMODE(worktree.stat().st_mode), 0o700)
        detached = subprocess.run(
            ["git", "-C", str(worktree), "symbolic-ref", "-q", "HEAD"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        self.assertEqual(detached.returncode, 1)
        state_path.with_name("state.sha256").unlink()
        replay_arguments, _ = self.prepare_arguments(gates)
        _, second, _ = self.invoke(replay_arguments)
        self.assertEqual(second["disposition"], "exact_replay")
        self.assertEqual(second["state_sha256"], first["state_sha256"])
        self.assertTrue(state_path.with_name("state.sha256").exists())

    def test_prepare_rejects_wrong_head_and_credential_remote(self):
        arguments, _ = self.prepare_arguments(expected_head="a" * 40)
        status, _, error = self.invoke(arguments)
        self.assertEqual(status, 1)
        self.assertIn("pr_head_mismatch", error)
        git(self.repo, "remote", "set-url", "origin", "https://user:pass@github.com/o/r.git")
        other = self.root / "other"
        other.mkdir(mode=0o700)
        arguments, _ = self.prepare_arguments(output_root=other)
        status, _, error = self.invoke(arguments)
        self.assertEqual(status, 1)
        self.assertIn("remote_url_credentials_forbidden", error)

    def test_prepare_rejects_local_and_non_github_origins(self):
        for index, remote_url in enumerate(
            (
                str(self.remote),
                "https://example.com/owner/repository.git",
                "https://[github.com/owner/repository.git",
            ),
            start=1,
        ):
            with self.subTest(remote_url=remote_url):
                git(self.repo, "remote", "set-url", "origin", remote_url)
                output = self.root / f"foreign-{index}"
                output.mkdir(mode=0o700)
                arguments, _ = self.prepare_arguments(output_root=output)
                status, _, error = self.invoke(arguments)
                self.assertEqual(status, 1)
                self.assertIn("remote_url_invalid", error)

    def test_github_ssh_and_https_origins_canonicalize(self):
        urls = (
            "https://github.com/owner/repository.git",
            "git@github.com:owner/repository.git",
            "ssh://git@github.com/owner/repository.git",
        )
        for remote_url in urls:
            with self.subTest(remote_url=remote_url):
                git(self.repo, "remote", "set-url", "origin", remote_url)
                self.assertEqual(
                    MODULE.github_repository_from_remote(self.repo, "origin"),
                    self.github_repository,
                )

    def test_gate_execution_is_chained_redacted_and_exactly_replayed(self):
        state_path, _, gates = self.prepare()
        status, result, error, observed = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        self.assertEqual(observed, list(MODULE.REQUIRED_GATE_COMMANDS))
        self.assertEqual(result["gates"], len(MODULE.REQUIRED_GATE_COMMANDS))
        evidence = state_path.with_name("gate-evidence.jsonl")
        before = evidence.read_bytes()
        lines = [json.loads(line) for line in before.splitlines()]
        self.assertEqual(len(lines), 2 * len(MODULE.REQUIRED_GATE_COMMANDS))
        self.assertNotIn(gates[0].encode("utf-8"), before)
        self.assertTrue(all("command_sha256" in line for line in lines))
        status, replay, error, observed = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        self.assertEqual(observed, [])
        self.assertEqual(replay["evidence_chain_head_sha256"], result["evidence_chain_head_sha256"])
        self.assertEqual(evidence.read_bytes(), before)

    def test_required_gate_manifest_matches_phase_d_contract(self):
        self.assertEqual(
            MODULE.REQUIRED_GATE_COMMANDS,
            (
                "cargo fmt --all -- --check",
                "cargo build --locked --workspace --all-targets",
                "cargo test --locked --workspace",
                "cargo clippy --locked --workspace --all-targets -- -D warnings",
                "cargo build --locked -p interaction-smoke --features unsafe-dev-activation",
                "python3 -m unittest discover -s tools/d2-certification -p 'test_*.py'",
                "npm --prefix tools/codex-worker run check",
                "npm --prefix tools/codex-worker test",
                "npm --prefix eval/codex-worker-slo run check",
                "npm --prefix eval/design-harness ci",
                "npm --prefix eval/design-harness run audit",
                "npm --prefix eval/design-harness run check",
                "cargo test --locked -p automation-ruleset-postgres -- --ignored --test-threads=1",
                "cargo test --locked -p automation-instance-postgres -- --ignored --test-threads=1",
                "cargo test --locked -p automation-panel-installation-postgres -- --ignored --test-threads=1",
                "cargo test --locked -p automation-ruleset-activation-postgres -- --ignored --test-threads=1",
                "cargo test --locked -p authoring-promotion-postgres -- --ignored --test-threads=1",
                "cargo test --locked -p authoring-application-postgres -- --ignored --test-threads=1",
                "cargo test --locked -p automation-ruleset-dispatch -- --ignored --test-threads=1",
                "cargo test --locked -p automation-ruleset-readiness -- --ignored --test-threads=1",
                "cargo test --locked -p automation-runtime-convergence-postgres -- --ignored --test-threads=1",
                "cargo test --locked -p automation-runtime-execution-postgres --test postgres_security -- --ignored --test-threads=1",
                "cargo test --locked -p automation-runtime-serving-postgres -- --ignored --test-threads=1",
                "cargo test --locked -p automation-runtime-interaction-postgres -- --ignored --test-threads=1",
                "cargo test --locked -p automation-runtime-panel-postgres -- --ignored --test-threads=1",
            ),
        )

    def test_gate_failure_is_durable_and_retry_can_succeed(self):
        state_path, _, gates = self.prepare()
        status, _, error, observed = self.invoke_gate_plan(
            state_path,
            gates,
            outcomes=[9],
        )
        self.assertEqual(status, 1)
        self.assertEqual(observed, [MODULE.REQUIRED_GATE_COMMANDS[0]])
        self.assertIn("gate_failed:1:9", error)
        status, result, error, observed = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        self.assertEqual(observed, list(MODULE.REQUIRED_GATE_COMMANDS))
        self.assertEqual(result["status"], "passed")
        records = [
            json.loads(line)
            for line in state_path.with_name("gate-evidence.jsonl").read_text().splitlines()
        ]
        first_gate = [record for record in records if record["gate_index"] == 1]
        self.assertEqual([record["attempt"] for record in first_gate], [1, 1, 2, 2])
        self.assertEqual(
            [record.get("exit_code") for record in first_gate if "exit_code" in record],
            [9, 0],
        )

    def test_gate_execution_resumes_a_durable_incomplete_attempt(self):
        state_path, _, gates = self.prepare()
        state = json.loads(state_path.read_text(encoding="utf-8"))
        evidence_path = state_path.with_name("gate-evidence.jsonl")
        records = []
        MODULE.append_gate_evidence(
            evidence_path,
            state,
            records,
            {
                "kind": "starring.d3.gate-started.v1",
                "gate_index": 1,
                "command_sha256": state["gate_command_sha256"][0],
                "attempt": 1,
            },
        )
        status, result, error, observed = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        self.assertEqual(observed, list(MODULE.REQUIRED_GATE_COMMANDS))
        self.assertEqual(result["status"], "passed")
        evidence = [
            json.loads(line)
            for line in evidence_path.read_text(encoding="utf-8").splitlines()
        ]
        self.assertEqual(len(evidence), 2 * len(MODULE.REQUIRED_GATE_COMMANDS))
        self.assertEqual(evidence[0]["kind"], "starring.d3.gate-started.v1")
        self.assertEqual(evidence[1]["kind"], "starring.d3.gate-completed.v1")
        self.assertEqual(evidence[1]["attempt"], 1)

    def test_gate_plan_and_sensitive_command_fail_closed(self):
        state_path, _, _ = self.prepare()
        status, _, error = self.invoke(
            ["run-gates", "--state", str(state_path), "--gate", "false"]
        )
        self.assertEqual(status, 1)
        self.assertIn("gate_manifest_mismatch", error)
        status, _, error = self.invoke(
            ["run-gates", "--state", str(state_path), "--gate", "env"]
        )
        self.assertEqual(status, 1)
        self.assertIn("gate_command_sensitive", error)

    def test_prepare_rejects_changed_missing_added_and_duplicate_gates(self):
        required = list(MODULE.REQUIRED_GATE_COMMANDS)
        variants = {
            "changed": ["cargo fmt --all"] + required[1:],
            "missing": required[:-1],
            "added": required + ["true"],
            "duplicate": required + [required[-1]],
            "reordered": [required[1], required[0]] + required[2:],
        }
        for name, gates in variants.items():
            with self.subTest(name=name):
                arguments, _ = self.prepare_arguments(gates)
                status, _, error = self.invoke(arguments)
                self.assertEqual(status, 1)
                self.assertIn("gate_manifest_mismatch", error)

    def test_process_environment_is_allowlisted(self):
        previous_secret = os.environ.get("STARRING_TEST_SECRET")
        os.environ["STARRING_TEST_SECRET"] = "must-not-cross-boundary"
        completed = subprocess.CompletedProcess(
            args=["true"],
            returncode=0,
            stdout=b"ok\n",
            stderr=b"",
        )
        try:
            with mock.patch.object(MODULE.subprocess, "run", return_value=completed) as invoked:
                _, output = MODULE.run_process(["true"], self.root, "test")
            environment = invoked.call_args.kwargs["env"]
            self.assertEqual(output, b"ok\n")
            self.assertNotIn("STARRING_TEST_SECRET", environment)
            self.assertEqual(environment["GIT_TERMINAL_PROMPT"], "0")
            self.assertEqual(environment["LC_ALL"], "C")
            if "HOME" in os.environ:
                self.assertEqual(environment["HOME"], os.environ["HOME"])
        finally:
            if previous_secret is None:
                os.environ.pop("STARRING_TEST_SECRET", None)
            else:
                os.environ["STARRING_TEST_SECRET"] = previous_secret

    def test_untracked_worktree_content_is_rejected(self):
        state_path, _, _ = self.prepare()
        state = json.loads(state_path.read_text(encoding="utf-8"))
        write(pathlib.Path(state["worktree_path"]) / "untracked.txt", "drift\n")
        status, _, error = self.invoke(["recheck", "--state", str(state_path)])
        self.assertEqual(status, 1)
        self.assertIn("worktree_tracked_changes", error)

    def test_d2_step_codes_match_the_certification_contract(self):
        self.assertEqual(
            MODULE.D2_STEP_CODES,
            (
                "isolated_target_created",
                "prior_guild_ownership_absent",
                "candidate_processes_started",
                "oauth_authenticated",
                "one_shot_authoring_submitted",
                "encrypted_preview_ready",
                "product_decisions_applied",
                "runtime_live",
                "create_and_join_executed",
                "duplicate_interaction_suppressed",
                "runtime_restarted_with_canonical_process_identity",
                "route_and_instance_reconstructed",
                "indeterminate_effect_reconciled",
                "target_replaced",
                "gateway_disconnect_failed_closed",
                "test_resources_torn_down",
                "total_absence_confirmed",
            ),
        )

    def make_d2(self, state_path):
        state = json.loads(state_path.read_text(encoding="utf-8"))
        root = self.root / "d2"
        root.mkdir(mode=0o700)
        manifest = {
            "schema_version": 1,
            "run_id": "d2-20260804t120000z-123456789abc",
            "commit_sha": state["merge_commit"],
            "discord": {"resource_prefix": "d2-123456789abc"},
        }
        manifest_path = root / "manifest.json"
        write(manifest_path, MODULE.canonical_json(manifest) + "\n", 0o600)
        digest = MODULE.sha256_bytes(MODULE.canonical_json(manifest).encode("utf-8"))
        write(root / "manifest.sha256", digest + "\n", 0o600)
        receipts = []
        previous = MODULE.ZERO_DIGEST
        for step, code in enumerate(MODULE.D2_STEP_CODES, start=1):
            receipt = {
                "schema_version": 1,
                "run_id": manifest["run_id"],
                "manifest_sha256": digest,
                "step": step,
                "code": code,
                "observed_at": "2026-08-04T12:00:00Z",
                "previous_sha256": previous,
                "evidence": {},
            }
            receipt["receipt_sha256"] = MODULE.sha256_bytes(
                MODULE.canonical_json(receipt).encode("utf-8")
            )
            previous = receipt["receipt_sha256"]
            receipts.append(receipt)
        write(
            root / "receipts.jsonl",
            "".join(MODULE.canonical_json(receipt) + "\n" for receipt in receipts),
            0o600,
        )
        final = {
            "schema_version": 1,
            "kind": "starring.d2.coordinator-final-record.v1",
            "run_id": manifest["run_id"],
            "commit_sha": state["merge_commit"],
            "manifest_sha256": digest,
            "steps": 17,
            "status": "passed",
            "resource_prefix": manifest["discord"]["resource_prefix"],
            "receipt_chain_head_sha256": previous,
            "coordinator_evidence_sha256": "f" * 64,
        }
        final_path = root / "final.json"
        write(final_path, MODULE.canonical_json(final) + "\n", 0o600)
        return manifest_path, final_path

    def test_d2_binding_requires_exact_commit_tree_run_and_chain(self):
        state_path, _, gates = self.prepare()
        manifest_path, final_path = self.make_d2(state_path)
        status, result, error = self.invoke(
            [
                "bind-d2",
                "--state",
                str(state_path),
                "--d2-manifest",
                str(manifest_path),
                "--d2-final-record",
                str(final_path),
            ]
        )
        self.assertEqual(status, 0, error)
        self.assertEqual(result["steps"], 17)
        self.assertEqual(result["merge_tree"], self.tree)
        status, replay, error = self.invoke(
            [
                "bind-d2",
                "--state",
                str(state_path),
                "--d2-manifest",
                str(manifest_path),
                "--d2-final-record",
                str(final_path),
            ]
        )
        self.assertEqual(status, 0, error)
        self.assertEqual(replay["disposition"], "exact_replay")
        receipts_path = manifest_path.with_name("receipts.jsonl")
        receipts = receipts_path.read_text(encoding="utf-8").splitlines()
        changed = json.loads(receipts[-1])
        changed["previous_sha256"] = "a" * 64
        receipts[-1] = MODULE.canonical_json(changed)
        write(receipts_path, "\n".join(receipts) + "\n", 0o600)
        status, _, error = self.invoke(
            [
                "bind-d2",
                "--state",
                str(state_path),
                "--d2-manifest",
                str(manifest_path),
                "--d2-final-record",
                str(final_path),
            ]
        )
        self.assertEqual(status, 1)
        self.assertIn("d2_receipt_sequence_invalid", error)
        self.assertEqual(gates, list(MODULE.REQUIRED_GATE_COMMANDS))

    def test_d2_binding_rejects_the_legacy_receipt_only_final_record(self):
        state_path, _, _ = self.prepare()
        manifest_path, final_path = self.make_d2(state_path)
        final = json.loads(final_path.read_text(encoding="utf-8"))
        final.pop("kind")
        final.pop("coordinator_evidence_sha256")
        write(final_path, MODULE.canonical_json(final) + "\n", 0o600)
        status, _, error = self.invoke(
            [
                "bind-d2",
                "--state",
                str(state_path),
                "--d2-manifest",
                str(manifest_path),
                "--d2-final-record",
                str(final_path),
            ]
        )
        self.assertEqual(status, 1)
        self.assertIn("d2_final_record_fields_invalid", error)

    def complete_prerequisites(self):
        state_path, _, gates = self.prepare()
        status, _, error, observed = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        self.assertEqual(observed, list(MODULE.REQUIRED_GATE_COMMANDS))
        manifest_path, final_path = self.make_d2(state_path)
        self.assertEqual(
            self.invoke(
                [
                    "bind-d2",
                    "--state",
                    str(state_path),
                    "--d2-manifest",
                    str(manifest_path),
                    "--d2-final-record",
                    str(final_path),
                ]
            )[0],
            0,
        )
        self.assertEqual(self.invoke(["recheck", "--state", str(state_path)])[0], 0)
        return state_path

    def test_recheck_detects_base_movement(self):
        state_path = self.complete_prerequisites()
        write(self.seed / "new.txt", "new\n")
        git(self.seed, "add", ".")
        git(self.seed, "commit", "-m", "new base")
        git(self.seed, "push", "--force", "origin", "main")
        status, _, error = self.invoke(["recheck", "--state", str(state_path)])
        self.assertEqual(status, 1)
        self.assertIn("pr_base_changed", error)

    def test_state_rejects_origin_repository_movement(self):
        state_path, _, _ = self.prepare()
        git(
            self.repo,
            "remote",
            "set-url",
            "origin",
            "https://github.com/foreign/repository.git",
        )
        status, _, error = self.invoke(["recheck", "--state", str(state_path)])
        self.assertEqual(status, 1)
        self.assertIn("state_github_repository_mismatch", error)

    def install_fake_gh(
        self,
        head_sha,
        run_overrides=None,
        workflow_overrides=None,
        jobs=None,
    ):
        directory = self.root / "bin"
        directory.mkdir(exist_ok=True)
        script = directory / "gh"
        run_value = {
            "id": 101,
            "workflow_id": 202,
            "name": "CI",
            "path": ".github/workflows/ci.yml@main",
            "event": "push",
            "head_branch": "main",
            "head_sha": head_sha,
            "status": "completed",
            "conclusion": "success",
            "run_attempt": 1,
            "pull_requests": [],
            "repository": {"full_name": "owner/repository"},
            "head_repository": {"full_name": "owner/repository"},
        }
        if run_overrides:
            run_value.update(run_overrides)
        workflow_value = {
            "id": 202,
            "name": "CI",
            "path": ".github/workflows/ci.yml",
            "state": "active",
        }
        if workflow_overrides:
            workflow_value.update(workflow_overrides)
        if jobs is None:
            jobs = [
                {
                    "id": 301,
                    "run_id": 101,
                    "workflow_name": "CI",
                    "head_branch": "main",
                    "head_sha": head_sha,
                    "name": "checks",
                    "status": "completed",
                    "conclusion": "success",
                },
                {
                    "id": 302,
                    "run_id": 101,
                    "workflow_name": "CI",
                    "head_branch": "main",
                    "head_sha": head_sha,
                    "name": "postgres",
                    "status": "completed",
                    "conclusion": "success",
                },
            ]
        responses = {
            "repos/owner/repository/actions/runs/101": run_value,
            "repos/owner/repository/actions/workflows/202": workflow_value,
            "repos/owner/repository/actions/runs/101/jobs?filter=latest&per_page=100": {
                "total_count": len(jobs),
                "jobs": jobs,
            },
        }
        body = f"""#!{sys.executable}
import json
import sys

responses = {responses!r}
endpoint = sys.argv[-1]
if endpoint not in responses:
    raise SystemExit(2)
print(json.dumps(responses[endpoint]))
"""
        write(script, body, 0o700)
        if not hasattr(self, "previous_path"):
            self.previous_path = os.environ.get("PATH", "")
            os.environ["PATH"] = str(directory) + os.pathsep + self.previous_path

    def finalize_arguments(self, state_path, repository=None):
        return [
            "finalize",
            "--state",
            str(state_path),
            "--github-repository",
            self.github_repository if repository is None else repository,
            "--actions-run-id",
            "101",
        ]

    def test_finalize_requires_main_tree_and_successful_actions(self):
        state_path = self.complete_prerequisites()
        git(self.seed, "push", "origin", f"{self.merge}:refs/heads/main")
        self.install_fake_gh(self.merge)
        try:
            arguments = self.finalize_arguments(state_path)
            status, result, error = self.invoke(arguments)
            self.assertEqual(status, 0, error)
            self.assertEqual(result["status"], "passed")
            self.assertEqual(result["main_tree"], self.tree)
            self.assertEqual(result["actions_runs"][0]["id"], 101)
            self.assertEqual(
                result["actions_runs"][0]["workflow_path"],
                ".github/workflows/ci.yml",
            )
            self.assertEqual(
                [job["name"] for job in result["actions_runs"][0]["jobs"]],
                ["checks", "postgres"],
            )
            status, replay, error = self.invoke(arguments)
            self.assertEqual(status, 0, error)
            self.assertEqual(replay["disposition"], "exact_replay")
            final_path = state_path.with_name("final.json")
            final = json.loads(final_path.read_text(encoding="utf-8"))
            final["finalized_at"] = "2026-08-04T00:00:00Z"
            write(final_path, MODULE.canonical_json(final) + "\n", 0o600)
            status, _, error = self.invoke(arguments)
            self.assertEqual(status, 1)
            self.assertIn("final_record_mismatch", error)
        finally:
            os.environ["PATH"] = self.previous_path

    def test_finalize_rejects_repository_other_than_pinned_origin(self):
        state_path = self.complete_prerequisites()
        git(self.seed, "push", "origin", f"{self.merge}:refs/heads/main")
        status, _, error = self.invoke(
            self.finalize_arguments(state_path, repository="foreign/repository")
        )
        self.assertEqual(status, 1)
        self.assertIn("github_repository_mismatch", error)

    def test_finalize_rejects_pull_request_actions_run(self):
        state_path = self.complete_prerequisites()
        git(self.seed, "push", "origin", f"{self.merge}:refs/heads/main")
        self.install_fake_gh(self.merge, run_overrides={"event": "pull_request"})
        try:
            status, _, error = self.invoke(self.finalize_arguments(state_path))
            self.assertEqual(status, 1)
            self.assertIn("actions_run_identity_invalid", error)
        finally:
            os.environ["PATH"] = self.previous_path

    def test_finalize_rejects_workflow_dispatch_actions_run(self):
        state_path = self.complete_prerequisites()
        git(self.seed, "push", "origin", f"{self.merge}:refs/heads/main")
        self.install_fake_gh(self.merge, run_overrides={"event": "workflow_dispatch"})
        try:
            status, _, error = self.invoke(self.finalize_arguments(state_path))
            self.assertEqual(status, 1)
            self.assertIn("actions_run_identity_invalid", error)
        finally:
            os.environ["PATH"] = self.previous_path

    def test_finalize_rejects_foreign_actions_workflow(self):
        state_path = self.complete_prerequisites()
        git(self.seed, "push", "origin", f"{self.merge}:refs/heads/main")
        self.install_fake_gh(
            self.merge,
            workflow_overrides={"path": ".github/workflows/foreign.yml"},
        )
        try:
            status, _, error = self.invoke(self.finalize_arguments(state_path))
            self.assertEqual(status, 1)
            self.assertIn("actions_workflow_identity_invalid", error)
        finally:
            os.environ["PATH"] = self.previous_path

    def test_finalize_rejects_foreign_actions_branch(self):
        state_path = self.complete_prerequisites()
        git(self.seed, "push", "origin", f"{self.merge}:refs/heads/main")
        self.install_fake_gh(self.merge, run_overrides={"head_branch": "release"})
        try:
            status, _, error = self.invoke(self.finalize_arguments(state_path))
            self.assertEqual(status, 1)
            self.assertIn("actions_run_identity_invalid", error)
        finally:
            os.environ["PATH"] = self.previous_path

    def test_finalize_rejects_skipped_postgres_job(self):
        state_path = self.complete_prerequisites()
        git(self.seed, "push", "origin", f"{self.merge}:refs/heads/main")
        jobs = [
            {
                "id": 301,
                "run_id": 101,
                "workflow_name": "CI",
                "head_branch": "main",
                "head_sha": self.merge,
                "name": "checks",
                "status": "completed",
                "conclusion": "success",
            },
            {
                "id": 302,
                "run_id": 101,
                "workflow_name": "CI",
                "head_branch": "main",
                "head_sha": self.merge,
                "name": "postgres",
                "status": "completed",
                "conclusion": "skipped",
            },
        ]
        self.install_fake_gh(self.merge, jobs=jobs)
        try:
            status, _, error = self.invoke(self.finalize_arguments(state_path))
            self.assertEqual(status, 1)
            self.assertIn("actions_jobs_invalid", error)
        finally:
            os.environ["PATH"] = self.previous_path

    def test_finalize_rejects_missing_postgres_job(self):
        state_path = self.complete_prerequisites()
        git(self.seed, "push", "origin", f"{self.merge}:refs/heads/main")
        jobs = [
            {
                "id": 301,
                "run_id": 101,
                "workflow_name": "CI",
                "head_branch": "main",
                "head_sha": self.merge,
                "name": "checks",
                "status": "completed",
                "conclusion": "success",
            }
        ]
        self.install_fake_gh(self.merge, jobs=jobs)
        try:
            status, _, error = self.invoke(self.finalize_arguments(state_path))
            self.assertEqual(status, 1)
            self.assertIn("actions_jobs_invalid", error)
        finally:
            os.environ["PATH"] = self.previous_path

    def test_finalize_rejects_a_different_main_tree(self):
        state_path = self.complete_prerequisites()
        write(self.seed / "different.txt", "different\n")
        git(self.seed, "add", ".")
        git(self.seed, "commit", "-m", "different main")
        different = git(self.seed, "rev-parse", "HEAD")
        git(self.seed, "push", "--force", "origin", f"{different}:refs/heads/main")
        self.install_fake_gh(different)
        try:
            status, _, error = self.invoke(
                [
                    "finalize",
                    "--state",
                    str(state_path),
                    "--github-repository",
                    "owner/repository",
                    "--actions-run-id",
                    "101",
                ]
            )
            self.assertEqual(status, 1)
            self.assertIn("main_tree_mismatch", error)
        finally:
            os.environ["PATH"] = self.previous_path

    def test_state_digest_symlink_is_rejected(self):
        state_path, _, _ = self.prepare()
        digest_path = state_path.with_name("state.sha256")
        value = digest_path.read_text(encoding="ascii")
        digest_path.unlink()
        target = self.root / "digest"
        write(target, value, 0o600)
        digest_path.symlink_to(target)
        status, _, error = self.invoke(["recheck", "--state", str(state_path)])
        self.assertEqual(status, 1)
        self.assertIn("state_digest_unavailable", error)
if __name__ == "__main__":
    unittest.main()
