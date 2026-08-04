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
v = {"schema_version":1,"run_id":m["run_id"],"commit_sha":m["commit_sha"],"manifest_sha256":d,"steps":len(r),"status":"passed","resource_prefix":m["discord"]["resource_prefix"],"receipt_chain_head_sha256":r[-1]["receipt_sha256"]}
print(json.dumps(v, ensure_ascii=False, sort_keys=True, separators=(",", ":")))
"""
        write(
            self.seed / "tools" / "d2-certification" / "d2_certification.py",
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

    def invoke(self, arguments):
        stdout = io.StringIO()
        stderr = io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            status = MODULE.main(arguments)
        result = json.loads(stdout.getvalue()) if stdout.getvalue() else None
        return status, result, stderr.getvalue()

    def prepare(self, gates=None):
        gates = gates or ["python3 -c 'raise SystemExit(0)'", "git diff --quiet"]
        arguments = [
            "prepare",
            "--repo",
            str(self.repo),
            "--output-root",
            str(self.output),
            "--pr-number",
            "42",
            "--expected-head",
            self.head,
            "--expected-base",
            self.base,
        ]
        for gate in gates:
            arguments.extend(["--gate", gate])
        status, result, error = self.invoke(arguments)
        self.assertEqual(status, 0, error)
        return pathlib.Path(result["state"]), result, gates

    def test_prepare_pins_merge_parents_tree_and_detached_worktree(self):
        state_path, first, gates = self.prepare()
        state = json.loads(state_path.read_text(encoding="utf-8"))
        self.assertEqual(state["merge_commit"], self.merge)
        self.assertEqual(state["merge_tree"], self.tree)
        self.assertEqual(state["merge_parents"], [self.base, self.head])
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
        _, second, _ = self.invoke(
            [
                "prepare",
                "--repo",
                str(self.repo),
                "--output-root",
                str(self.output),
                "--pr-number",
                "42",
                "--expected-head",
                self.head,
                "--expected-base",
                self.base,
                "--gate",
                gates[0],
                "--gate",
                gates[1],
            ]
        )
        self.assertEqual(second["disposition"], "exact_replay")
        self.assertEqual(second["state_sha256"], first["state_sha256"])
        self.assertTrue(state_path.with_name("state.sha256").exists())

    def test_prepare_rejects_wrong_head_and_credential_remote(self):
        status, _, error = self.invoke(
            [
                "prepare",
                "--repo",
                str(self.repo),
                "--output-root",
                str(self.output),
                "--pr-number",
                "42",
                "--expected-head",
                "a" * 40,
                "--expected-base",
                self.base,
                "--gate",
                "true",
            ]
        )
        self.assertEqual(status, 1)
        self.assertIn("pr_head_mismatch", error)
        git(self.repo, "remote", "set-url", "origin", "https://user:pass@github.com/o/r.git")
        other = self.root / "other"
        other.mkdir(mode=0o700)
        status, _, error = self.invoke(
            [
                "prepare",
                "--repo",
                str(self.repo),
                "--output-root",
                str(other),
                "--pr-number",
                "42",
                "--expected-head",
                self.head,
                "--expected-base",
                self.base,
                "--gate",
                "true",
            ]
        )
        self.assertEqual(status, 1)
        self.assertIn("remote_url_credentials_forbidden", error)

    def test_gate_execution_is_chained_redacted_and_exactly_replayed(self):
        state_path, _, gates = self.prepare()
        arguments = ["run-gates", "--state", str(state_path)]
        for gate in gates:
            arguments.extend(["--gate", gate])
        status, result, error = self.invoke(arguments)
        self.assertEqual(status, 0, error)
        self.assertEqual(result["gates"], 2)
        evidence = state_path.with_name("gate-evidence.jsonl")
        before = evidence.read_bytes()
        lines = [json.loads(line) for line in before.splitlines()]
        self.assertEqual(len(lines), 4)
        self.assertNotIn(gates[0].encode("utf-8"), before)
        self.assertTrue(all("command_sha256" in line for line in lines))
        status, replay, error = self.invoke(arguments)
        self.assertEqual(status, 0, error)
        self.assertEqual(replay["evidence_chain_head_sha256"], result["evidence_chain_head_sha256"])
        self.assertEqual(evidence.read_bytes(), before)

    def test_gate_failure_is_durable_and_retry_can_succeed(self):
        marker = self.root / "marker"
        command = f"test -f {shlex_quote(marker)} || (touch {shlex_quote(marker)}; exit 9)"
        state_path, _, gates = self.prepare([command])
        arguments = ["run-gates", "--state", str(state_path), "--gate", gates[0]]
        status, _, error = self.invoke(arguments)
        self.assertEqual(status, 1)
        self.assertIn("gate_failed:1:9", error)
        status, result, error = self.invoke(arguments)
        self.assertEqual(status, 0, error)
        self.assertEqual(result["status"], "passed")
        records = [
            json.loads(line)
            for line in state_path.with_name("gate-evidence.jsonl").read_text().splitlines()
        ]
        self.assertEqual([record["attempt"] for record in records], [1, 1, 2, 2])
        self.assertEqual([record.get("exit_code") for record in records if "exit_code" in record], [9, 0])

    def test_gate_execution_resumes_a_durable_incomplete_attempt(self):
        state_path, _, gates = self.prepare(["true"])
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
        status, result, error = self.invoke(
            ["run-gates", "--state", str(state_path), "--gate", gates[0]]
        )
        self.assertEqual(status, 0, error)
        self.assertEqual(result["status"], "passed")
        evidence = [
            json.loads(line)
            for line in evidence_path.read_text(encoding="utf-8").splitlines()
        ]
        self.assertEqual(len(evidence), 2)
        self.assertEqual(evidence[0]["kind"], "starring.d3.gate-started.v1")
        self.assertEqual(evidence[1]["kind"], "starring.d3.gate-completed.v1")
        self.assertEqual(evidence[1]["attempt"], 1)

    def test_gate_plan_and_sensitive_command_fail_closed(self):
        state_path, _, _ = self.prepare(["true"])
        status, _, error = self.invoke(
            ["run-gates", "--state", str(state_path), "--gate", "false"]
        )
        self.assertEqual(status, 1)
        self.assertIn("gate_plan_mismatch", error)
        status, _, error = self.invoke(
            ["run-gates", "--state", str(state_path), "--gate", "env"]
        )
        self.assertEqual(status, 1)
        self.assertIn("gate_command_sensitive", error)

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
            "run_id": manifest["run_id"],
            "commit_sha": state["merge_commit"],
            "manifest_sha256": digest,
            "steps": 17,
            "status": "passed",
            "resource_prefix": manifest["discord"]["resource_prefix"],
            "receipt_chain_head_sha256": previous,
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
        self.assertEqual(gates[0], "python3 -c 'raise SystemExit(0)'")

    def complete_prerequisites(self):
        state_path, _, gates = self.prepare()
        gate_arguments = ["run-gates", "--state", str(state_path)]
        for gate in gates:
            gate_arguments.extend(["--gate", gate])
        self.assertEqual(self.invoke(gate_arguments)[0], 0)
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

    def install_fake_gh(self, head_sha):
        directory = self.root / "bin"
        directory.mkdir(exist_ok=True)
        script = directory / "gh"
        value = {
            "id": 101,
            "workflow_id": 202,
            "name": "CI",
            "event": "push",
            "head_sha": head_sha,
            "status": "completed",
            "conclusion": "success",
        }
        body = f"#!{sys.executable}\nimport json\nprint(json.dumps({value!r}))\n"
        write(script, body, 0o700)
        self.previous_path = os.environ.get("PATH", "")
        os.environ["PATH"] = str(directory) + os.pathsep + self.previous_path

    def test_finalize_requires_main_tree_and_successful_actions(self):
        state_path = self.complete_prerequisites()
        git(self.seed, "push", "origin", f"{self.merge}:refs/heads/main")
        self.install_fake_gh(self.merge)
        try:
            arguments = [
                "finalize",
                "--state",
                str(state_path),
                "--github-repository",
                "owner/repository",
                "--actions-run-id",
                "101",
            ]
            status, result, error = self.invoke(arguments)
            self.assertEqual(status, 0, error)
            self.assertEqual(result["status"], "passed")
            self.assertEqual(result["main_tree"], self.tree)
            self.assertEqual(result["actions_runs"][0]["id"], 101)
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


def shlex_quote(path):
    return "'" + str(path).replace("'", "'\"'\"'") + "'"


if __name__ == "__main__":
    unittest.main()
