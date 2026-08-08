import contextlib
import hashlib
import importlib.util
import io
import json
import os
import pathlib
import shlex
import shutil
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
import d3_candidate_bundle as BUNDLE
import d3_candidate_io as CANDIDATE_IO


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


def fixture_system_file_identity(path, _label):
    encoded = str(path).encode("utf-8")
    return {
        "path": str(path),
        "sha256": hashlib.sha256(encoded).hexdigest(),
        "size": len(encoded),
        "mode": 0o555,
        "uid": 0,
        "device": 1,
        "inode": int.from_bytes(hashlib.sha256(encoded).digest()[:8], "big"),
        "links": 1,
    }


def fixture_launchd_job(argv, cwd, environment, timeout, *_arguments):
    return subprocess.run(
        argv,
        cwd=cwd,
        env=environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        check=False,
    ).returncode


class D3BootstrapSafetyTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name).resolve()
        self.root.chmod(0o700)
        self.worktree = self.root / "worktree"
        self.worktree.mkdir(mode=0o700)

    def tearDown(self):
        self.temporary.cleanup()

    def test_current_and_legacy_staging_are_descriptor_safely_discarded(self):
        for name in (
            ".gate-bootstrap-staging",
            ".gate-bootstrap-staging-0123456789abcdef",
        ):
            staging = self.root / name
            nested = staging / "nested"
            nested.mkdir(parents=True, mode=0o700)
            write(nested / "value", "value", 0o600)
            MODULE.discard_gate_bootstrap_staging(self.root, staging)
            self.assertFalse(staging.exists())

    def test_staging_symlink_is_rejected_without_touching_target(self):
        target = self.root / "target"
        target.mkdir(mode=0o700)
        write(target / "value", "value", 0o600)
        staging = self.root / ".gate-bootstrap-staging"
        staging.symlink_to(target, target_is_directory=True)
        with self.assertRaisesRegex(
            MODULE.D3Error,
            "gate_bootstrap_staging_(?:unavailable|identity_invalid)",
        ):
            MODULE.discard_gate_bootstrap_staging(self.root, staging)
        self.assertEqual((target / "value").read_text(encoding="utf-8"), "value")

    def test_prepare_cleans_staging_when_bootstrap_build_fails(self):
        injected = MODULE.D3Error("injected_bootstrap_failure")
        with mock.patch.object(
            MODULE.gate_container,
            "require_bootstrap_capacity",
            return_value=10**12,
        ), mock.patch.object(
            MODULE,
            "build_gate_bootstrap",
            side_effect=injected,
        ):
            with self.assertRaisesRegex(MODULE.D3Error, "injected_bootstrap_failure"):
                MODULE.prepare_gate_bootstrap(
                    self.root,
                    self.worktree,
                    {"image_id": "sha256:" + "1" * 64},
                )
        self.assertFalse((self.root / ".gate-bootstrap-staging").exists())


class D3CertificationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name).resolve()
        self.seed = self.root / "seed"
        self.remote = self.root / "origin.git"
        self.repo = self.root / "repo"
        self.output = self.root / "state"
        self.postgres_database_url = (
            "postgres://postgres:postgres@127.0.0.1:5432/starring_test"
        )
        self.postgres_database_url_file = self.root / "postgres-database-url"
        self.github_repository = "owner/repository"
        self.github_remote_url = "https://github.com/owner/repository.git"
        self.output.mkdir(mode=0o700)
        write(
            self.postgres_database_url_file,
            self.postgres_database_url,
            0o600,
        )
        if sys.platform != "darwin":
            candidate_job = mock.patch.object(
                BUNDLE.launchd_job,
                "run_job",
                side_effect=fixture_launchd_job,
            )
            candidate_job.start()
            self.addCleanup(candidate_job.stop)
        self.create_repository()

    def tearDown(self):
        self.temporary.cleanup()

    def create_repository(self):
        self.seed.mkdir()
        git(self.seed, "init", "-b", "main")
        git(self.seed, "config", "user.email", "test@example.com")
        git(self.seed, "config", "user.name", "D3 Test")
        write(self.seed / "base.txt", "base\n")
        write(self.seed / "Cargo.toml", "workspace manifest\n")
        write(self.seed / "Cargo.lock", "workspace lockfile\n")
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
        for name in BUNDLE.D2_TOOLCHAIN_SOURCE_FILES:
            path = self.seed / "tools" / "d2-certification" / name
            if not path.exists():
                write(path, f"d2 source {name}\n")
        for name in BUNDLE.CERTIFICATION_TRANSPORT_SOURCE_FILES:
            write(
                self.seed / "tools" / "d2-certification-transport" / name,
                f"transport source {name}\n",
            )
        for name in BUNDLE.CODEX_WORKER_SOURCE_FILES:
            write(
                self.seed / "tools" / "codex-worker" / name,
                f"worker source {name}\n",
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

    def fetch_candidate_fixture(self, repo, remote, base_ref, pr_number, prefix):
        self.assertEqual(repo, self.repo)
        self.assertEqual(remote, "origin")
        self.assertEqual(base_ref, "main")
        self.assertEqual(pr_number, 42)
        head_ref = f"refs/d3/pr-{pr_number}/{prefix}-head"
        merge_ref = f"refs/d3/pr-{pr_number}/{prefix}-merge"
        base_remote_ref = f"refs/remotes/{remote}/{base_ref}"
        git(
            repo,
            "fetch",
            "--atomic",
            "--no-tags",
            "--force",
            str(self.remote),
            f"refs/heads/{base_ref}:{base_remote_ref}",
            f"refs/pull/{pr_number}/head:{head_ref}",
            f"refs/pull/{pr_number}/merge:{merge_ref}",
        )
        return (
            git(repo, "rev-parse", head_ref),
            git(repo, "rev-parse", base_remote_ref),
            git(repo, "rev-parse", merge_ref),
        )

    def fetch_main_fixture(self, repo, remote, base_ref):
        self.assertEqual(repo, self.repo)
        self.assertEqual(remote, "origin")
        self.assertEqual(base_ref, "main")
        base_remote_ref = f"refs/remotes/{remote}/{base_ref}"
        git(
            repo,
            "fetch",
            "--atomic",
            "--no-tags",
            "--force",
            str(self.remote),
            f"refs/heads/{base_ref}:{base_remote_ref}",
        )
        return git(repo, "rev-parse", base_remote_ref)

    def invoke(self, arguments):
        stdout = io.StringIO()
        stderr = io.StringIO()
        with (
            contextlib.redirect_stdout(stdout),
            contextlib.redirect_stderr(stderr),
            mock.patch.object(
                MODULE,
                "fetch_candidate",
                side_effect=self.fetch_candidate_fixture,
            ),
            mock.patch.object(
                MODULE,
                "fetch_main",
                side_effect=self.fetch_main_fixture,
            ),
            mock.patch.object(
                MODULE,
                "load_gate_container_runtime",
                return_value={"record_sha256": "2" * 64},
            ),
            mock.patch.object(
                MODULE,
                "load_gate_bootstrap_record",
                return_value={"record_sha256": "4" * 64},
            ),
            mock.patch.object(
                MODULE,
                "candidate_dependency_snapshot",
                return_value=getattr(self, "current_dependency_snapshot", {}),
            ),
        ):
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

    def make_dependency_snapshot(self, dependency_root):
        if not dependency_root.exists():
            workspace_vendor = dependency_root / "vendor" / "workspace"
            transport_vendor = dependency_root / "vendor" / "transport"
            workspace_vendor.mkdir(mode=0o700, parents=True)
            transport_vendor.mkdir(mode=0o700)
            workspace_config = dependency_root / "native-cargo-config.toml"
            transport_config = dependency_root / "native-transport-cargo-config.toml"
            write(workspace_config, "workspace\n", 0o400)
            write(transport_config, "transport\n", 0o400)
            workspace_vendor.chmod(0o500)
            transport_vendor.chmod(0o500)
            (dependency_root / "vendor").chmod(0o500)
            dependency_root.chmod(0o555)
        return {
            "schema_version": 1,
            "kind": "starring.d3.candidate-dependency-snapshot.v1",
            "gate_runtime_sha256": "2" * 64,
            "gate_bootstrap_sha256": "4" * 64,
            "gate_bootstrap_tree_sha256": "5" * 64,
            "candidate_builder_implementation_sha256": (
                BUNDLE.candidate_builder_implementation_sha256()
            ),
            "bootstrap_root": str(dependency_root),
            "workspace": {
                "vendor_root": str(dependency_root / "vendor" / "workspace"),
                "cargo_config": BUNDLE.file_identity(
                    dependency_root / "native-cargo-config.toml",
                    "test_workspace_cargo_config",
                    expected_mode=0o400,
                ),
            },
            "transport": {
                "vendor_root": str(dependency_root / "vendor" / "transport"),
                "cargo_config": BUNDLE.file_identity(
                    dependency_root / "native-transport-cargo-config.toml",
                    "test_transport_cargo_config",
                    expected_mode=0o400,
                ),
            },
        }

    def prepare(self, gates=None):
        arguments, gates = self.prepare_arguments(gates)
        status, result, error = self.invoke(arguments)
        self.assertEqual(status, 0, error)
        return pathlib.Path(result["state"]), result, gates

    def invoke_gate_plan(self, state_path, gates=None, outcomes=None):
        gates = list(MODULE.REQUIRED_GATE_COMMANDS if gates is None else gates)
        arguments = [
            "run-gates",
            "--state",
            str(state_path),
            "--postgres-database-url-file",
            str(self.postgres_database_url_file),
        ]
        for gate in gates:
            arguments.extend(["--gate", gate])
        original = MODULE.run_process
        observed = []
        outcomes = iter([] if outcomes is None else outcomes)

        database_urls = []

        def recording_gate(
            _root,
            _worktree,
            _bootstrap,
            _toolchain,
            index,
            _attempt,
            command,
            _timeout,
            database_url,
        ):
            observed.append(command)
            database_urls.append(database_url if index > 16 else None)
            return next(outcomes, 0)

        def recording_runner(
            argv,
            cwd,
            label,
            allowed=(0,),
            timeout=None,
            discard=False,
            postgres_database_url=None,
        ):
            if argv[:3] == ["/bin/zsh", "-f", "-c"]:
                observed.append(argv[3])
                database_urls.append(postgres_database_url)
                return next(outcomes, 0), b""
            return original(
                argv,
                cwd,
                label,
                allowed,
                timeout,
                discard,
                postgres_database_url,
            )

        tool_root = state_path.parent / "candidate-build-tools"
        tool_root.mkdir(mode=0o700, exist_ok=True)
        dependency_root = state_path.parent / "candidate-test-dependencies"
        self.current_dependency_snapshot = self.make_dependency_snapshot(
            dependency_root
        )
        native_arguments = {
            "clang": ("--version",),
            "ar": (),
            "ld": ("-v",),
        }
        def resolve_toolchain(_worktree, directories):
            tools = []
            for name in ("rustup", "cargo", "rustc"):
                path = tool_root / name
                if not path.exists():
                    write(path, f"fake {name}\n", 0o555)
                version = f"{name} 1.0.0"
                if name == "rustc":
                    version += f"\nhost: {BUNDLE.FIXED_RUST_TARGET}"
                tools.append(
                    {
                        "name": name,
                        **BUNDLE.file_identity(path, f"fake_{name}"),
                        "version": version,
                    }
                )
            for name in ("clang", "ar", "ld"):
                path = BUNDLE.FIXED_DEVELOPER_DIRECTORY / "usr" / "bin" / name
                tools.append(
                    {
                        "name": name,
                        **BUNDLE.system_file_identity(path, f"fake_{name}"),
                        "version": " ".join((name, *native_arguments[name]))
                        or name,
                    }
                )
            toolchain = BUNDLE.seal_record(
                {
                    "schema_version": BUNDLE.SCHEMA_VERSION,
                    "kind": "starring.d3.candidate-build-toolchain.v1",
                    "rust_target": BUNDLE.FIXED_RUST_TARGET,
                    "developer_directory": str(BUNDLE.FIXED_DEVELOPER_DIRECTORY),
                    "directories": directories,
                    "environment": BUNDLE.toolchain_environment(tools, directories),
                    "tools": tools,
                }
            )
            return BUNDLE.validate_candidate_toolchain(toolchain)

        def build_candidates(
            state,
            worktree,
            root,
            recipe,
            pinned_toolchain,
            _fence_descriptor,
        ):
            self.build_invocations = getattr(self, "build_invocations", 0) + 1
            BUNDLE.require_cargo_configuration_absent(worktree)
            build_root = pathlib.Path(recipe["build_root"])
            build_root.mkdir(mode=0o700, exist_ok=True)
            for specification in recipe["artifacts"]:
                source = pathlib.Path(specification["source"])
                if source.exists():
                    source.chmod(0o755)
                write(
                    source,
                    f"candidate {specification['candidate']} {state['merge_commit']}\n",
                    0o555,
                )
            return pinned_toolchain["tools"]

        with mock.patch.object(
            MODULE, "run_process", side_effect=recording_runner
        ), mock.patch.object(
            MODULE, "run_sandboxed_gate", side_effect=recording_gate
        ), mock.patch.object(
            MODULE,
            "prepare_gate_bootstrap",
            return_value=tool_root,
        ), mock.patch.object(
            MODULE,
            "ensure_gate_container_runtime",
            return_value={
                "image_id": "sha256:" + "1" * 64,
                "record_sha256": "2" * 64,
            },
        ), mock.patch.object(
            MODULE,
            "load_gate_container_runtime",
            return_value={"record_sha256": "2" * 64},
        ), mock.patch.object(
            MODULE,
            "ensure_gate_bootstrap_record",
            return_value={"record_sha256": "4" * 64},
        ), mock.patch.object(
            MODULE,
            "load_gate_bootstrap_record",
            return_value={"record_sha256": "4" * 64},
        ), mock.patch.object(
            BUNDLE, "execute_candidate_build", side_effect=build_candidates
        ), mock.patch.object(
            BUNDLE, "resolve_candidate_toolchain", side_effect=resolve_toolchain
        ), mock.patch.object(
            BUNDLE,
            "system_file_identity",
            side_effect=fixture_system_file_identity,
        ):
            status, result, error = self.invoke(arguments)
        self.gate_database_urls = database_urls
        return status, result, error, observed

    def remove_candidate_bundle(self, state_path):
        root = state_path.parent / "candidate-bundle"
        for path in root.rglob("*"):
            if path.is_dir():
                path.chmod(0o700)
            elif path.is_file():
                path.chmod(0o600)
        root.chmod(0o700)
        shutil.rmtree(root)

    def rewrite_d2_manifest(self, manifest_path, mutation):
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        mutation(manifest)
        write(manifest_path, MODULE.canonical_json(manifest) + "\n", 0o600)
        digest = MODULE.sha256_bytes(MODULE.canonical_json(manifest).encode("utf-8"))
        write(manifest_path.with_name("manifest.sha256"), digest + "\n", 0o600)

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

    def test_state_rejects_boolean_schema_version(self):
        state_path, _, _ = self.prepare()
        state = json.loads(state_path.read_text(encoding="utf-8"))
        state["schema_version"] = True
        write(state_path, MODULE.canonical_json(state) + "\n", 0o600)
        digest = MODULE.sha256_bytes(MODULE.canonical_json(state).encode("utf-8"))
        write(state_path.with_name("state.sha256"), digest + "\n", 0o600)
        status, _, error = self.invoke(["recheck", "--state", str(state_path)])
        self.assertEqual(status, 1)
        self.assertIn("state_schema_invalid", error)

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

    def test_prepare_rejects_git_transport_rewrites_and_overrides(self):
        overrides = (
            (f"url.{self.remote.as_uri()}.insteadOf", self.github_remote_url),
            ("remote.origin.pushurl", str(self.remote)),
            ("core.sshCommand", "/usr/bin/false"),
            ("http.sslVerify", "false"),
            ("http.curloptResolve", "github.com:443:127.0.0.1"),
        )
        for index, (key, value) in enumerate(overrides, start=1):
            with self.subTest(key=key):
                git(self.repo, "config", "--add", key, value)
                output = self.root / f"transport-override-{index}"
                output.mkdir(mode=0o700)
                arguments, _ = self.prepare_arguments(output_root=output)
                status, _, error = self.invoke(arguments)
                self.assertEqual(status, 1)
                self.assertIn("git_transport_override_forbidden", error)
                git(self.repo, "config", "--unset-all", key)

    def test_gate_execution_is_chained_redacted_and_exactly_replayed(self):
        state_path, _, gates = self.prepare()
        status, result, error, observed = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        self.assertEqual(observed, list(MODULE.REQUIRED_GATE_COMMANDS))
        self.assertEqual(self.gate_database_urls[:16], [None] * 16)
        self.assertEqual(
            self.gate_database_urls[16:],
            [self.postgres_database_url] * 13,
        )
        self.assertEqual(result["gates"], len(MODULE.REQUIRED_GATE_COMMANDS))
        evidence = state_path.with_name("gate-evidence.jsonl")
        before = evidence.read_bytes()
        lines = [json.loads(line) for line in before.splitlines()]
        self.assertEqual(len(lines), 2 * len(MODULE.REQUIRED_GATE_COMMANDS))
        self.assertNotIn(gates[0].encode("utf-8"), before)
        self.assertNotIn(self.postgres_database_url.encode("utf-8"), before)
        self.assertNotIn(b"postgres:postgres", before)
        self.assertTrue(all("command_sha256" in line for line in lines))
        self.assertEqual(self.build_invocations, 1)
        status, replay, error, observed = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        self.assertEqual(observed, [])
        self.assertEqual(replay["evidence_chain_head_sha256"], result["evidence_chain_head_sha256"])
        self.assertEqual(evidence.read_bytes(), before)
        self.assertEqual(self.build_invocations, 1)

    def test_postgres_gate_secret_is_required_owned_and_test_only(self):
        self.assertEqual(
            MODULE.load_postgres_database_url(str(self.postgres_database_url_file)),
            self.postgres_database_url,
        )
        state_path, _, gates = self.prepare()
        arguments = ["run-gates", "--state", str(state_path)]
        for gate in gates:
            arguments.extend(["--gate", gate])
        status, _, error = self.invoke(arguments)
        self.assertEqual(status, 1)
        self.assertIn("postgres_database_url_file_required", error)
        invalid = (
            "postgres://postgres:postgres@example.com:5432/starring_test",
            "postgres://postgres:postgres@127.0.0.1:5432/starring_runtime_staging",
            "postgres://postgres@127.0.0.1:5432/starring_test",
            "postgres://postgres:postgres@127.0.0.1:5432/starring_test?sslmode=disable",
        )
        for index, value in enumerate(invalid, start=1):
            with self.subTest(value=value):
                path = self.root / f"invalid-postgres-{index}"
                write(path, value, 0o600)
                with self.assertRaisesRegex(
                    MODULE.D3Error,
                    "postgres_database_url_invalid",
                ):
                    MODULE.load_postgres_database_url(str(path))
        write(self.postgres_database_url_file, self.postgres_database_url, 0o644)
        with self.assertRaisesRegex(
            MODULE.D3Error,
            "postgres_database_url_ownership_invalid",
        ):
            MODULE.load_postgres_database_url(str(self.postgres_database_url_file))

    def test_postgres_gate_secret_reaches_only_explicit_gate_process(self):
        script = (
            "import json,os;print(json.dumps({"
            "'database':os.environ.get('STARRING_TEST_DATABASE_URL'),"
            "'forbidden':'STARRING_FORBIDDEN_SECRET' in os.environ,"
            "'incremental':os.environ.get('CARGO_INCREMENTAL')}))"
        )
        with mock.patch.dict(
            os.environ,
            {
                "STARRING_TEST_DATABASE_URL": "parent-value",
                "STARRING_FORBIDDEN_SECRET": "must-not-propagate",
            },
            clear=False,
        ):
            _, raw = MODULE.run_process(
                [sys.executable, "-c", script],
                self.repo,
                "postgres_environment_probe",
                postgres_database_url=self.postgres_database_url,
            )
        self.assertEqual(
            json.loads(raw),
            {
                "database": self.postgres_database_url,
                "forbidden": False,
                "incremental": "0",
            },
        )

    def test_required_gate_manifest_matches_phase_d_contract(self):
        self.assertEqual(
            MODULE.REQUIRED_GATE_COMMANDS,
            (
                "cargo fmt --all -- --check",
                "cargo build --locked --workspace --all-targets",
                "cargo test --locked --workspace",
                "cargo clippy --locked --workspace --all-targets -- -D warnings",
                "cargo build --locked -p interaction-smoke --features unsafe-dev-activation",
                "npm --prefix tools/codex-worker run check",
                "npm --prefix tools/codex-worker test",
                "npm --prefix eval/codex-worker-slo run check",
                "npm --prefix eval/design-harness ci",
                "npm --prefix eval/design-harness run audit",
                "npm --prefix eval/design-harness run check",
                "python3 -m unittest discover -s tools/d2-certification -p 'test_*.py'",
                "node --test tools/d2-certification/product_driver.test.mjs",
                "cargo fmt --manifest-path tools/d2-certification-transport/Cargo.toml -- --check",
                "cargo test --locked --manifest-path tools/d2-certification-transport/Cargo.toml",
                "cargo clippy --locked --manifest-path tools/d2-certification-transport/Cargo.toml --all-targets -- -D warnings",
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

    def test_readme_gate_manifest_matches_code_order(self):
        readme = MODULE_PATH.with_name("README.md").read_text(encoding="utf-8")
        lines = readme.splitlines()
        start = lines.index("gates=(") + 1
        end = lines.index(")", start)
        documented = []
        for line in lines[start:end]:
            parts = shlex.split(line.strip())
            self.assertEqual(len(parts), 1)
            documented.append(parts[0])
        self.assertEqual(tuple(documented), MODULE.REQUIRED_GATE_COMMANDS)

    def test_prepare_rejects_each_missing_d2_standalone_gate(self):
        standalone = (
            "python3 -m unittest discover -s tools/d2-certification -p 'test_*.py'",
            "node --test tools/d2-certification/product_driver.test.mjs",
            "cargo fmt --manifest-path tools/d2-certification-transport/Cargo.toml -- --check",
            "cargo test --locked --manifest-path tools/d2-certification-transport/Cargo.toml",
            "cargo clippy --locked --manifest-path tools/d2-certification-transport/Cargo.toml --all-targets -- -D warnings",
        )
        required = list(MODULE.REQUIRED_GATE_COMMANDS)
        for command in standalone:
            with self.subTest(command=command):
                gates = [candidate for candidate in required if candidate != command]
                arguments, _ = self.prepare_arguments(gates)
                status, _, error = self.invoke(arguments)
                self.assertEqual(status, 1)
                self.assertIn("gate_manifest_mismatch", error)

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
            "2" * 64,
            "4" * 64,
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

    def test_gate_evidence_rejects_boolean_schema_version(self):
        state_path, _, _ = self.prepare()
        state = json.loads(state_path.read_text(encoding="utf-8"))
        evidence_path = state_path.with_name("gate-evidence.jsonl")
        MODULE.append_gate_evidence(
            evidence_path,
            state,
            [],
            {
                "kind": "starring.d3.gate-started.v1",
                "gate_index": 1,
                "command_sha256": state["gate_command_sha256"][0],
                "attempt": 1,
            },
            "2" * 64,
            "4" * 64,
        )
        record = json.loads(evidence_path.read_text(encoding="utf-8"))
        record["schema_version"] = True
        record.pop("record_sha256")
        record["record_sha256"] = MODULE.sha256_bytes(
            MODULE.canonical_json(record).encode("utf-8")
        )
        write(evidence_path, MODULE.canonical_json(record) + "\n", 0o600)
        with self.assertRaisesRegex(
            MODULE.D3Error,
            "gate_evidence_identity_invalid",
        ):
            MODULE.load_gate_evidence(
                evidence_path,
                state,
                "2" * 64,
                "4" * 64,
            )

    def test_gate_evidence_rejects_legacy_unisolated_record(self):
        state_path, _, _ = self.prepare()
        state = json.loads(state_path.read_text(encoding="utf-8"))
        evidence_path = state_path.with_name("gate-evidence.jsonl")
        record = {
            "schema_version": 1,
            "kind": "starring.d3.gate-started.v1",
            "merge_commit": state["merge_commit"],
            "merge_tree": state["merge_tree"],
            "gate_index": 1,
            "command_sha256": state["gate_command_sha256"][0],
            "attempt": 1,
            "observed_at": MODULE.utc_now(),
            "previous_sha256": MODULE.ZERO_DIGEST,
        }
        record["record_sha256"] = MODULE.sha256_bytes(
            MODULE.canonical_json(record).encode("utf-8")
        )
        write(evidence_path, MODULE.canonical_json(record) + "\n", 0o600)
        with self.assertRaisesRegex(
            MODULE.D3Error,
            "gate_evidence_fields_invalid",
        ):
            MODULE.load_gate_evidence(
                evidence_path,
                state,
                "2" * 64,
                "4" * 64,
            )

    def test_gate_evidence_rejects_changed_container_runtime(self):
        state_path, _, _ = self.prepare()
        state = json.loads(state_path.read_text(encoding="utf-8"))
        evidence_path = state_path.with_name("gate-evidence.jsonl")
        MODULE.append_gate_evidence(
            evidence_path,
            state,
            [],
            {
                "kind": "starring.d3.gate-started.v1",
                "gate_index": 1,
                "command_sha256": state["gate_command_sha256"][0],
                "attempt": 1,
            },
            "2" * 64,
            "4" * 64,
        )
        with self.assertRaisesRegex(
            MODULE.D3Error,
            "gate_evidence_identity_invalid",
        ):
            MODULE.load_gate_evidence(
                evidence_path,
                state,
                "3" * 64,
                "4" * 64,
            )

    def test_candidate_bundle_resumes_from_durable_intent_and_partial_build(self):
        state_path, _, gates = self.prepare()
        status, _, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        self.assertEqual(self.build_invocations, 1)
        self.remove_candidate_bundle(state_path)
        status, result, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        self.assertEqual(result["candidate_bundle_disposition"], "created")
        self.assertEqual(self.build_invocations, 2)

    def test_copy_snapshot_copies_and_revalidates_destination(self):
        source = self.root / "copy-success-source"
        destination = self.root / "copy-success-destination"
        payload = (b"stable candidate payload\n" * 1024) + b"complete\n"
        source.write_bytes(payload)
        source.chmod(0o600)

        source_identity, destination_identity = CANDIDATE_IO.copy_snapshot(
            source,
            destination,
            0o400,
            "copy",
        )

        self.assertEqual(destination.read_bytes(), payload)
        self.assertEqual(stat.S_IMODE(destination.stat().st_mode), 0o400)
        self.assertEqual(source_identity["sha256"], destination_identity["sha256"])
        self.assertEqual(source_identity["size"], len(payload))
        self.assertEqual(destination_identity["size"], len(payload))

    def test_copy_snapshot_rejects_torn_same_inode_source(self):
        source = self.root / "copy-source"
        destination = self.root / "copy-destination"
        source.write_bytes(b"A" * (2 * 1024 * 1024))
        source.chmod(0o600)
        original_write_all = CANDIDATE_IO.write_all
        mutated = False

        def mutate_after_first_chunk(descriptor, payload):
            nonlocal mutated
            original_write_all(descriptor, payload)
            if mutated:
                return
            mutated = True
            source_descriptor = os.open(source, os.O_WRONLY)
            try:
                os.pwrite(
                    source_descriptor,
                    b"B" * (2 * 1024 * 1024),
                    0,
                )
                os.fsync(source_descriptor)
            finally:
                os.close(source_descriptor)

        with mock.patch.object(
            CANDIDATE_IO,
            "write_all",
            side_effect=mutate_after_first_chunk,
        ):
            with self.assertRaisesRegex(
                BUNDLE.CandidateBundleError,
                "copy_source_changed_during_copy",
            ):
                CANDIDATE_IO.copy_snapshot(
                    source,
                    destination,
                    0o400,
                    "copy",
                )

    def test_copy_snapshot_rejects_atomic_source_replacement(self):
        source = self.root / "replace-source"
        replacement = self.root / "replace-source-new"
        destination = self.root / "replace-destination"
        source.write_bytes(b"A" * (2 * 1024 * 1024))
        source.chmod(0o600)
        replacement.write_bytes(b"B" * (2 * 1024 * 1024))
        replacement.chmod(0o600)
        original_write_all = CANDIDATE_IO.write_all
        replaced = False

        def replace_after_first_chunk(descriptor, payload):
            nonlocal replaced
            original_write_all(descriptor, payload)
            if not replaced:
                replaced = True
                os.replace(replacement, source)

        with mock.patch.object(
            CANDIDATE_IO,
            "write_all",
            side_effect=replace_after_first_chunk,
        ):
            with self.assertRaisesRegex(
                BUNDLE.CandidateBundleError,
                "copy_source_(?:path_)?changed_during_copy",
            ):
                CANDIDATE_IO.copy_snapshot(
                    source,
                    destination,
                    0o400,
                    "copy",
                )

    def test_candidate_build_fence_blocks_retry_until_holder_releases(self):
        root = self.root / "build-fence-root"
        root.mkdir(mode=0o700)
        descriptor = BUNDLE.acquire_candidate_build_fence(root)
        holder = os.dup(descriptor)
        os.close(descriptor)
        try:
            with self.assertRaisesRegex(
                BUNDLE.CandidateBundleError,
                "candidate_build_writer_active",
            ):
                BUNDLE.acquire_candidate_build_fence(root)
        finally:
            os.close(holder)
        recovered = BUNDLE.acquire_candidate_build_fence(root)
        os.close(recovered)

    def test_candidate_build_delegates_to_deterministic_launchd_job(self):
        root = self.root / "build-launchd-root"
        root.mkdir(mode=0o700)
        descriptor = BUNDLE.acquire_candidate_build_fence(root)
        try:
            with (
                mock.patch.object(
                    BUNDLE,
                    "require_candidate_build_capacity",
                    return_value=(0, 0),
                ),
                mock.patch.object(
                    BUNDLE,
                    "sandboxed_argv",
                    side_effect=lambda argv, *_arguments: ["/sandbox", *argv],
                ),
                mock.patch.object(
                    BUNDLE.launchd_job,
                    "run_job",
                    return_value=0,
                ) as invoked,
            ):
                first = BUNDLE.run_contained_build_process(
                    ["/usr/bin/true"],
                    root,
                    {"PATH": "/usr/bin:/bin"},
                    30,
                    descriptor,
                    "candidate_test",
                    root,
                    "none",
                )
                second = BUNDLE.run_contained_build_process(
                    ["/usr/bin/true"],
                    root,
                    {"PATH": "/usr/bin:/bin"},
                    30,
                    descriptor,
                    "candidate_test",
                    root,
                    "none",
                )
        finally:
            os.close(descriptor)
        self.assertEqual(first, 0)
        self.assertEqual(second, 0)
        self.assertEqual(invoked.call_count, 2)
        first_arguments = invoked.call_args_list[0].args
        second_arguments = invoked.call_args_list[1].args
        self.assertEqual(first_arguments[0], ["/sandbox", "/usr/bin/true"])
        self.assertEqual(first_arguments[5], second_arguments[5])
        self.assertEqual(len(first_arguments[5]), 32)

    def test_candidate_build_maps_launchd_failure(self):
        root = self.root / "build-launchd-error-root"
        root.mkdir(mode=0o700)
        descriptor = BUNDLE.acquire_candidate_build_fence(root)
        try:
            with mock.patch.object(
                BUNDLE,
                "require_candidate_build_capacity",
                return_value=(0, 0),
            ), mock.patch.object(
                BUNDLE,
                "sandboxed_argv",
                side_effect=lambda argv, *_arguments: argv,
            ), mock.patch.object(
                BUNDLE.launchd_job,
                "run_job",
                side_effect=BUNDLE.launchd_job.LaunchdJobError(
                    "candidate_launchd_timeout"
                ),
            ):
                with self.assertRaisesRegex(
                    BUNDLE.CandidateBundleError,
                    "candidate_test_candidate_launchd_timeout",
                ):
                    BUNDLE.run_contained_build_process(
                        ["/usr/bin/true"],
                        root,
                        {"PATH": "/usr/bin:/bin"},
                        30,
                        descriptor,
                        "candidate_test",
                        root,
                        "none",
                    )
        finally:
            os.close(descriptor)

    def test_candidate_build_capacity_preserves_baseline_and_reserve(self):
        root = self.root / "build-capacity-root"
        root.mkdir(mode=0o700)
        gibibyte = 1024 * 1024 * 1024
        with mock.patch.object(
            BUNDLE,
            "candidate_build_usage",
            return_value=(1, gibibyte),
        ), mock.patch.object(
            BUNDLE.os,
            "statvfs",
            return_value=mock.Mock(f_bavail=7 * gibibyte, f_frsize=1),
        ):
            self.assertEqual(
                BUNDLE.require_candidate_build_capacity(root),
                (1, gibibyte),
            )
        for free in (gibibyte, 6 * gibibyte):
            with mock.patch.object(
                BUNDLE,
                "candidate_build_usage",
                return_value=(1, gibibyte),
            ), mock.patch.object(
                BUNDLE.os,
                "statvfs",
                return_value=mock.Mock(f_bavail=free, f_frsize=1),
            ):
                with self.assertRaisesRegex(
                    BUNDLE.CandidateBundleError,
                    "candidate_build_capacity_insufficient",
                ):
                    BUNDLE.require_candidate_build_capacity(root)

    def test_candidate_build_root_is_retired_by_sealed_identity(self):
        state_path, _, gates = self.prepare()
        status, _, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        build_root = state_path.parent / "candidate-build"
        self.assertTrue(build_root.is_dir())
        self.assertTrue(BUNDLE.retire_candidate_build_root(state_path.parent))
        self.assertFalse(build_root.exists())
        self.assertFalse(BUNDLE.retire_candidate_build_root(state_path.parent))

    def test_candidate_build_root_retirement_rejects_replacement(self):
        state_path, _, gates = self.prepare()
        status, _, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        build_root = state_path.parent / "candidate-build"
        displaced = state_path.parent / "candidate-build-displaced"
        build_root.rename(displaced)
        build_root.mkdir(mode=0o700)
        with self.assertRaisesRegex(
            BUNDLE.CandidateBundleError,
            "candidate_build_retirement_identity_invalid",
        ):
            BUNDLE.retire_candidate_build_root(state_path.parent)

    def test_candidate_bundle_rejects_partial_build_under_changed_toolchain(self):
        state_path, _, gates = self.prepare()
        status, _, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        self.assertEqual(self.build_invocations, 1)
        self.remove_candidate_bundle(state_path)
        cargo = state_path.parent / "candidate-build-tools" / "cargo"
        cargo.chmod(0o700)
        write(cargo, "fake cargo changed\n", 0o555)
        status, _, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 1)
        self.assertIn("candidate_build_toolchain_drift", error)
        self.assertEqual(self.build_invocations, 1)

    def test_candidate_bundle_recovers_owned_partial_staging(self):
        state_path, _, gates = self.prepare()
        status, _, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        intent = json.loads(
            state_path.with_name("candidate-bundle-intent.json").read_text(
                encoding="utf-8"
            )
        )
        bundle = state_path.parent / "candidate-bundle"
        staging = pathlib.Path(intent["staging_path"])
        bundle.chmod(0o700)
        bundle.rename(staging)
        status, result, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        self.assertEqual(result["candidate_bundle_disposition"], "created")
        self.assertEqual(self.build_invocations, 2)
        self.assertFalse(staging.exists())

    def test_candidate_bundle_resumes_complete_unpublished_staging(self):
        state_path, _, gates = self.prepare()
        status, first, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        intent = json.loads(
            state_path.with_name("candidate-bundle-intent.json").read_text(
                encoding="utf-8"
            )
        )
        bundle = state_path.parent / "candidate-bundle"
        staging = pathlib.Path(intent["staging_path"])
        bundle.rename(staging)
        status, replay, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        self.assertEqual(replay["candidate_bundle_disposition"], "exact_replay")
        self.assertEqual(
            replay["candidate_bundle_sha256"], first["candidate_bundle_sha256"]
        )
        self.assertEqual(self.build_invocations, 1)

    def test_candidate_bundle_recovers_atomic_intent_and_publication_temps(self):
        for boundary in ("intent", "publication"):
            with self.subTest(boundary=boundary):
                output = self.root / f"atomic-{boundary}"
                output.mkdir(mode=0o700)
                arguments, gates = self.prepare_arguments(output_root=output)
                status, prepared, error = self.invoke(arguments)
                self.assertEqual(status, 0, error)
                state_path = pathlib.Path(prepared["state"])
                status, _, error, _ = self.invoke_gate_plan(state_path, gates)
                self.assertEqual(status, 0, error)
                intent_path = state_path.with_name("candidate-bundle-intent.json")
                intent = json.loads(intent_path.read_text(encoding="utf-8"))
                if boundary == "intent":
                    self.remove_candidate_bundle(state_path)
                    temporary = state_path.parent / (
                        f".candidate-bundle-intent.json.tmp-{intent['staging_nonce']}"
                    )
                    intent_path.rename(temporary)
                else:
                    bundle = state_path.parent / "candidate-bundle"
                    staging = pathlib.Path(intent["staging_path"])
                    bundle.chmod(0o700)
                    bundle.rename(staging)
                    publication = staging / "publication.json"
                    publication.rename(
                        staging
                        / f".publication.json.tmp-{intent['staging_nonce']}"
                    )
                status, result, error, _ = self.invoke_gate_plan(state_path, gates)
                self.assertEqual(status, 0, error)
                self.assertTrue(
                    pathlib.Path(result["candidate_bundle"]).is_file()
                )

    def test_candidate_bundle_cleans_partial_atomic_temps(self):
        state_path, _, gates = self.prepare()
        partial_intent = state_path.parent / (
            ".candidate-bundle-intent.json.tmp-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        )
        write(partial_intent, '{"schema_version":', 0o600)
        status, _, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        self.assertFalse(partial_intent.exists())
        intent = json.loads(
            state_path.with_name("candidate-bundle-intent.json").read_text(
                encoding="utf-8"
            )
        )
        self.remove_candidate_bundle(state_path)
        staging = pathlib.Path(intent["staging_path"])
        staging.mkdir(mode=0o700)
        partial_publication = staging / (
            f".publication.json.tmp-{intent['staging_nonce']}"
        )
        write(partial_publication, '{"schema_version":', 0o400)
        status, result, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        self.assertTrue(pathlib.Path(result["candidate_bundle"]).is_file())
        self.assertFalse(staging.exists())

    def test_candidate_bundle_resumes_journaled_discard_after_crash(self):
        state_path, _, gates = self.prepare()
        status, _, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        intent = json.loads(
            state_path.with_name("candidate-bundle-intent.json").read_text(
                encoding="utf-8"
            )
        )
        bundle = state_path.parent / "candidate-bundle"
        staging = pathlib.Path(intent["staging_path"])
        bundle.chmod(0o700)
        bundle.rename(staging)
        with mock.patch.object(
            BUNDLE,
            "remove_tree_descriptor",
            side_effect=BUNDLE.CandidateBundleError("injected_discard_crash"),
        ):
            status, _, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 1)
        self.assertIn("injected_discard_crash", error)
        discard = state_path.parent / (
            f".candidate-bundle-discard-{intent['staging_nonce']}"
        )
        self.assertTrue(discard.is_dir())
        self.assertTrue(
            state_path.with_name("candidate-bundle-discard.json").is_file()
        )
        status, result, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        self.assertTrue(pathlib.Path(result["candidate_bundle"]).is_file())
        self.assertFalse(discard.exists())
        self.assertFalse(
            state_path.with_name("candidate-bundle-discard.json").exists()
        )

    def test_candidate_bundle_rejects_foreign_staging_directory(self):
        state_path, _, gates = self.prepare()
        foreign = state_path.parent / ".candidate-bundle-staging-ffffffffffffffffffffffffffffffff"
        foreign.mkdir(mode=0o700)
        status, _, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 1)
        self.assertIn("candidate_bundle_foreign_staging_present", error)

    def test_candidate_bundle_replay_does_not_depend_on_live_build_tools(self):
        state_path, _, gates = self.prepare()
        status, first, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        cargo = state_path.parent / "candidate-build-tools" / "cargo"
        cargo.chmod(0o755)
        cargo.write_text("updated cargo\n", encoding="utf-8")
        cargo.chmod(0o555)
        status, replay, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        self.assertEqual(replay["candidate_bundle_sha256"], first["candidate_bundle_sha256"])
        self.assertEqual(self.build_invocations, 1)

    def test_candidate_bundle_rejects_boolean_integer_substitutions(self):
        cases = ("intent_schema", "record_schema", "artifact_links")
        for case in cases:
            with self.subTest(case=case):
                output = self.root / f"boolean-{case}"
                output.mkdir(mode=0o700)
                arguments, gates = self.prepare_arguments(output_root=output)
                status, prepared, error = self.invoke(arguments)
                self.assertEqual(status, 0, error)
                state_path = pathlib.Path(prepared["state"])
                status, _, error, _ = self.invoke_gate_plan(state_path, gates)
                self.assertEqual(status, 0, error)
                if case == "intent_schema":
                    path = state_path.with_name("candidate-bundle-intent.json")
                    value = json.loads(path.read_text(encoding="utf-8"))
                    value.pop("record_sha256")
                    value["schema_version"] = True
                    write(
                        path,
                        BUNDLE.canonical_json(BUNDLE.seal_record(value)) + "\n",
                        0o600,
                    )
                else:
                    path = state_path.parent / "candidate-bundle" / "bundle.json"
                    value = json.loads(path.read_text(encoding="utf-8"))
                    value.pop("record_sha256")
                    if case == "record_schema":
                        value["schema_version"] = True
                    else:
                        value["artifacts"][0]["artifact"]["links"] = True
                    path.chmod(0o600)
                    write(
                        path,
                        BUNDLE.canonical_json(BUNDLE.seal_record(value)) + "\n",
                        0o400,
                    )
                status, _, error, _ = self.invoke_gate_plan(state_path, gates)
                self.assertEqual(status, 1)
                self.assertIn("candidate_bundle", error)

    def test_candidate_bundle_rejects_artifact_and_inventory_drift(self):
        cases = ("content", "mode", "hardlink", "symlink", "extra")
        for case in cases:
            with self.subTest(case=case):
                output = self.root / f"bundle-{case}"
                output.mkdir(mode=0o700)
                arguments, gates = self.prepare_arguments(output_root=output)
                status, result, error = self.invoke(arguments)
                self.assertEqual(status, 0, error)
                state_path = pathlib.Path(result["state"])
                status, _, error, _ = self.invoke_gate_plan(state_path, gates)
                self.assertEqual(status, 0, error)
                bundle = state_path.parent / "candidate-bundle"
                artifact = bundle / "starring-api"
                if case == "content":
                    artifact.chmod(0o755)
                    artifact.write_text("replacement\n", encoding="utf-8")
                    artifact.chmod(0o555)
                elif case == "mode":
                    artifact.chmod(0o755)
                elif case == "hardlink":
                    bundle.chmod(0o755)
                    original = state_path.parent / "saved-api"
                    artifact.rename(original)
                    os.link(original, artifact)
                    bundle.chmod(0o555)
                elif case == "symlink":
                    bundle.chmod(0o755)
                    original = state_path.parent / "saved-api"
                    artifact.rename(original)
                    artifact.symlink_to(original)
                    bundle.chmod(0o555)
                else:
                    bundle.chmod(0o755)
                    write(bundle / "unexpected", "unexpected\n", 0o444)
                    bundle.chmod(0o555)
                status, _, error, _ = self.invoke_gate_plan(state_path, gates)
                self.assertEqual(status, 1)
                self.assertIn("candidate_bundle", error)

    def test_candidate_bundle_never_replaces_preexisting_destination(self):
        state_path, _, gates = self.prepare()
        bundle = state_path.parent / "candidate-bundle"
        bundle.mkdir(mode=0o555)
        sentinel = state_path.parent / "destination-sentinel"
        write(sentinel, "preserved\n", 0o600)
        status, _, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 1)
        self.assertIn("candidate_bundle", error)
        self.assertTrue(bundle.is_dir())
        self.assertEqual(sentinel.read_text(encoding="utf-8"), "preserved\n")

    def test_candidate_bundle_rejects_exact_worktree_source_drift(self):
        state_path, _, gates = self.prepare()
        status, _, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 0, error)
        state = json.loads(state_path.read_text(encoding="utf-8"))
        source = (
            pathlib.Path(state["worktree_path"])
            / "tools"
            / "codex-worker"
            / "worker.mjs"
        )
        source.write_text("worker drift\n", encoding="utf-8")
        status, _, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 1)
        self.assertIn("worktree_tracked_changes", error)

    def test_candidate_bundle_rejects_tracked_symlinks_and_gitlinks(self):
        for kind in ("symlink", "gitlink"):
            with self.subTest(kind=kind):
                repo = self.root / f"forbidden-{kind}"
                repo.mkdir()
                git(repo, "init", "-b", "main")
                git(repo, "config", "user.email", "test@example.com")
                git(repo, "config", "user.name", "D3 Test")
                write(repo / "base", "base\n")
                git(repo, "add", "base")
                git(repo, "commit", "-m", "base")
                if kind == "symlink":
                    (repo / "linked").symlink_to("base")
                    git(repo, "add", "linked")
                else:
                    commit = git(repo, "rev-parse", "HEAD")
                    git(
                        repo,
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        f"160000,{commit},nested",
                    )
                git(repo, "commit", "-m", kind)
                with self.assertRaisesRegex(
                    BUNDLE.CandidateBundleError,
                    "candidate_source_git_entry_forbidden",
                ):
                    BUNDLE.validate_git_tree_entries(repo)

    def test_candidate_build_rejects_cargo_config_in_worktree_ancestor(self):
        state_path, _, gates = self.prepare()
        state = json.loads(state_path.read_text(encoding="utf-8"))
        ancestor_config = (
            pathlib.Path(state["worktree_path"]).parent / ".cargo" / "config.toml"
        )
        write(
            ancestor_config,
            '[build]\nrustc-wrapper = "/private/tmp/foreign-wrapper"\n',
            0o600,
        )
        status, _, error, _ = self.invoke_gate_plan(state_path, gates)
        self.assertEqual(status, 1)
        self.assertIn("candidate_build_cargo_config_forbidden", error)

    def test_candidate_build_excludes_foreign_native_toolchain_environment(self):
        foreign = self.root / "foreign-native-tools"
        foreign.mkdir(mode=0o700)
        marker = self.root / "foreign-cc-invoked"
        foreign_cc = foreign / "cc"
        write(
            foreign_cc,
            "#!/bin/sh\n"
            f"/usr/bin/touch {shlex.quote(str(marker))}\n"
            "exit 99\n",
            0o700,
        )
        cargo = self.root / "record-build-environment"
        observed = self.root / "observed-build-environment"
        write(
            cargo,
            "#!/bin/sh\n"
            f"/usr/bin/printf '%s\\n' \"$PATH\" \"$DEVELOPER_DIR\" \"$CC\" > {shlex.quote(str(observed))}\n"
            "/usr/bin/which cc >> " + shlex.quote(str(observed)) + "\n"
            "cc --version >/dev/null\n",
            0o700,
        )
        clang = BUNDLE.FIXED_DEVELOPER_DIRECTORY / "usr" / "bin" / "clang"
        build_root = self.root / "foreign-toolchain-build"
        cargo_home = build_root / "cargo-home"
        build_root.mkdir(mode=0o700)
        cargo_home.mkdir(mode=0o700)
        dependency_snapshot = self.make_dependency_snapshot(
            self.root / "foreign-toolchain-dependencies"
        )
        environment = {
            "AR": str(BUNDLE.FIXED_DEVELOPER_DIRECTORY / "usr" / "bin" / "ar"),
            "CC": str(clang),
            "CXX": str(clang),
            "DEVELOPER_DIR": str(BUNDLE.FIXED_DEVELOPER_DIRECTORY),
            "LD": str(BUNDLE.FIXED_DEVELOPER_DIRECTORY / "usr" / "bin" / "ld"),
            "RUSTC": str(self.root / "rustc"),
            "CARGO_HOME": str(cargo_home),
            BUNDLE.FIXED_RUST_LINKER_ENVIRONMENT: str(clang),
        }
        descriptor = BUNDLE.acquire_candidate_build_fence(self.root)
        try:
            with mock.patch.dict(
                os.environ,
                {
                    "PATH": str(foreign) + ":" + os.environ.get("PATH", ""),
                    "DEVELOPER_DIR": str(self.root / "foreign-developer"),
                },
            ), mock.patch.object(
                BUNDLE,
                "require_candidate_build_capacity",
                return_value=(0, 0),
            ), mock.patch.object(
                BUNDLE,
                "sandboxed_argv",
                side_effect=lambda argv, *_arguments: argv,
            ):
                BUNDLE.run_build_command(
                    [
                        "cargo",
                        "--config",
                        dependency_snapshot["workspace"]["cargo_config"]["path"],
                        "build",
                    ],
                    cargo,
                    environment,
                    self.repo,
                    self.merge,
                    dependency_snapshot,
                    descriptor,
                )
        finally:
            os.close(descriptor)
        self.assertFalse(marker.exists())
        self.assertEqual(
            observed.read_text(encoding="utf-8").splitlines(),
            [
                BUNDLE.FIXED_EXECUTABLE_PATH,
                str(BUNDLE.FIXED_DEVELOPER_DIRECTORY),
                str(clang),
                "/usr/bin/cc",
            ],
        )

    def test_candidate_recipe_is_offline_and_selects_sealed_configs(self):
        state_path, _, _ = self.prepare()
        state = json.loads(state_path.read_text(encoding="utf-8"))
        snapshot = self.make_dependency_snapshot(
            state_path.parent / "recipe-dependencies"
        )
        recipe = BUNDLE.candidate_build_recipe(
            state,
            state_path.parent,
            pathlib.Path(state["worktree_path"]),
            snapshot,
        )
        self.assertNotIn("fetch_commands", recipe)
        self.assertEqual(recipe["dependency_snapshot"], snapshot)
        self.assertEqual(len(recipe["commands"]), 5)
        workspace_config = snapshot["workspace"]["cargo_config"]["path"]
        transport_config = snapshot["transport"]["cargo_config"]["path"]
        self.assertTrue(
            all(command[1:3] == ["--config", workspace_config] for command in recipe["commands"][:4])
        )
        self.assertEqual(
            recipe["commands"][4][1:3],
            ["--config", transport_config],
        )
        self.assertTrue(all("--frozen" in command for command in recipe["commands"]))

    def test_candidate_build_command_is_networkless_offline_and_bootstrap_read_only(self):
        snapshot = self.make_dependency_snapshot(
            self.root / "networkless-dependencies"
        )
        build_root = self.root / "networkless-build"
        cargo_home = build_root / "cargo-home"
        build_root.mkdir(mode=0o700)
        cargo_home.mkdir(mode=0o700)
        command = [
            "cargo",
            "--config",
            snapshot["workspace"]["cargo_config"]["path"],
            "build",
            "--frozen",
        ]
        with mock.patch.object(
            BUNDLE,
            "run_contained_build_process",
            return_value=0,
        ) as invoked:
            BUNDLE.run_build_command(
                command,
                self.root / "cargo",
                {"CARGO_HOME": str(cargo_home)},
                self.repo,
                self.merge,
                snapshot,
            )
        arguments = invoked.call_args.args
        keywords = invoked.call_args.kwargs
        self.assertEqual(arguments[0][1:], command[1:])
        self.assertEqual(arguments[2]["CARGO_NET_OFFLINE"], "true")
        self.assertEqual(arguments[7], "none")
        self.assertEqual(
            keywords["sandbox_additional_read_roots"],
            (pathlib.Path(snapshot["bootstrap_root"]),),
        )

    def test_candidate_dependency_snapshot_rejects_config_vendor_and_builder_drift(self):
        dependency_root = self.root / "drift-dependencies"
        snapshot = self.make_dependency_snapshot(dependency_root)
        workspace_config = dependency_root / "native-cargo-config.toml"
        workspace_vendor = dependency_root / "vendor" / "workspace"
        workspace_config.chmod(0o600)
        with self.assertRaises(BUNDLE.CandidateBundleError):
            BUNDLE.validate_dependency_snapshot(snapshot)
        workspace_config.chmod(0o400)
        workspace_vendor.chmod(0o700)
        with self.assertRaises(BUNDLE.CandidateBundleError):
            BUNDLE.validate_dependency_snapshot(snapshot)
        workspace_vendor.chmod(0o500)
        changed = json.loads(json.dumps(snapshot))
        changed["candidate_builder_implementation_sha256"] = "0" * 64
        with self.assertRaisesRegex(
            BUNDLE.CandidateBundleError,
            "candidate_dependency_snapshot_identity_invalid",
        ):
            BUNDLE.validate_dependency_snapshot(changed)

    def test_npm_lock_source_validation_rejects_non_registry_origins(self):
        lock = self.root / "package-lock.json"
        for resolved in (
            "https://example.com/package.tgz",
            "http://registry.npmjs.org/package.tgz",
            "https://user:password@registry.npmjs.org/package.tgz",
            "https://registry.npmjs.org/package.tgz#fragment",
        ):
            with self.subTest(resolved=resolved):
                write(
                    lock,
                    json.dumps({"packages": {"node_modules/x": {"resolved": resolved}}}),
                )
                with self.assertRaisesRegex(
                    MODULE.D3Error,
                    "gate_npm_lock_source_invalid",
                ):
                    MODULE.validate_npm_lock_sources(lock)

    def test_vendor_configuration_supports_spaces_and_rejects_toml_escapes(self):
        raw = (
            '[source.crates-io]\nreplace-with = "vendored-sources"\n\n'
            '[source.vendored-sources]\ndirectory = "/stage/vendor"\n'
        ).encode("utf-8")
        selected = pathlib.Path("/Users/test/Application Support/vendor")
        value = MODULE.normalize_vendor_configuration(
            raw,
            pathlib.Path("/stage/vendor"),
            selected,
        )
        self.assertIn(f'directory = "{selected}"', value)
        with self.assertRaisesRegex(
            MODULE.D3Error,
            "gate_vendor_configuration_invalid",
        ):
            MODULE.normalize_vendor_configuration(
                raw,
                pathlib.Path("/stage/vendor"),
                pathlib.Path('/Users/test/invalid"path'),
            )

    def test_composite_vendor_configuration_separates_registry_and_git_sources(self):
        value = MODULE.composite_vendor_configuration(
            pathlib.Path("/vendor/workspace"),
            pathlib.Path("/vendor/transport"),
        )
        self.assertIn(
            '[source.crates-io]\nreplace-with = "workspace-vendored-sources"',
            value,
        )
        self.assertIn(
            f'[source."{MODULE.gate_container.TWILIGHT_SOURCE_KEY}"]',
            value,
        )
        self.assertIn('directory = "/vendor/workspace"', value)
        self.assertIn('directory = "/vendor/transport"', value)
        self.assertEqual(value.count("offline = true"), 1)
        with self.assertRaisesRegex(
            MODULE.D3Error,
            "gate_vendor_configuration_invalid",
        ):
            MODULE.composite_vendor_configuration(
                pathlib.Path('/vendor/invalid"path'),
                pathlib.Path("/vendor/transport"),
            )

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
        if not (state_path.parent / "candidate-bundle" / "bundle.json").exists():
            status, _, error, _ = self.invoke_gate_plan(state_path)
            self.assertEqual(status, 0, error)
        bundle = json.loads(
            (state_path.parent / "candidate-bundle" / "bundle.json").read_text(
                encoding="utf-8"
            )
        )
        root = self.root / "d2"
        root.mkdir(mode=0o700)
        (root / "orchestrator").mkdir(mode=0o700)
        artifacts = {
            value["candidate"]: value["artifact"] for value in bundle["artifacts"]
        }
        worker = next(
            value["artifact"]
            for value in bundle["worker"]["files"]
            if value["name"] == "worker.mjs"
        )
        candidates = {
            name: {"path": value["path"], "sha256": value["sha256"]}
            for name, value in artifacts.items()
        }
        candidates["codex_worker"] = {
            "path": worker["path"],
            "sha256": worker["sha256"],
        }
        external_root = root / "external"
        external_root.mkdir(mode=0o700)
        for name in ("codex", "node", "cloudflared"):
            path = external_root / name
            write(path, f"external {name}\n", 0o555)
            candidates[name] = {
                "path": str(path),
                "sha256": BUNDLE.file_identity(path, f"external_{name}")["sha256"],
            }
        manifest = {
            "schema_version": 1,
            "run_id": "d2-20260804t120000z-123456789abc",
            "commit_sha": state["merge_commit"],
            "discord": {"resource_prefix": "d2-123456789abc"},
            "candidates": candidates,
            "source_trees": {
                "codex_worker": {
                    "root": bundle["worker"]["root"],
                    "files": list(BUNDLE.CODEX_WORKER_SOURCE_FILES),
                    "sha256": bundle["worker"]["sha256"],
                },
                "d2_toolchain": {
                    "root": str(
                        pathlib.Path(state["worktree_path"])
                        / "tools"
                        / "d2-certification"
                    ),
                    "files": list(BUNDLE.D2_TOOLCHAIN_SOURCE_FILES),
                    "sha256": bundle["source_trees"]["d2_toolchain"]["sha256"],
                },
                "certification_transport": {
                    "root": str(
                        pathlib.Path(state["worktree_path"])
                        / "tools"
                        / "d2-certification-transport"
                    ),
                    "files": list(BUNDLE.CERTIFICATION_TRANSPORT_SOURCE_FILES),
                    "sha256": bundle["source_trees"]["certification_transport"][
                        "sha256"
                    ],
                },
            },
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

    def test_d2_binding_rejects_bound_run_tree_drift(self):
        state_path, _, _ = self.prepare()
        manifest_path, final_path = self.make_d2(state_path)
        status, binding, error = self.invoke(
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
        write(
            manifest_path.parent / "orchestrator" / "unbound-evidence.json",
            "{}\n",
            0o600,
        )
        with self.assertRaisesRegex(
            MODULE.D3Error,
            "d2_binding_run_identity_changed",
        ):
            MODULE.require_active_d2_binding(binding)

    def test_d2_run_tree_rejects_entry_overflow_during_enumeration(self):
        root = self.root / "d2-entry-bound"
        root.mkdir(mode=0o700)
        for index in range(3):
            write(root / f"entry-{index}", "x", 0o600)
        with mock.patch.object(
            MODULE, "D2_RUN_MAX_ENTRIES", 2
        ), self.assertRaisesRegex(MODULE.D3Error, "d2_run_tree_too_large"):
            MODULE.d2_run_tree_identity(root)

    def test_d2_run_tree_rejects_oversized_file_before_read(self):
        root = self.root / "d2-byte-bound"
        root.mkdir(mode=0o700)
        oversized = root / "oversized"
        with oversized.open("wb") as stream:
            stream.truncate(MODULE.D2_RUN_MAX_BYTES + 1)
        oversized.chmod(0o600)
        with mock.patch.object(
            MODULE.os, "read", wraps=MODULE.os.read
        ) as read, self.assertRaisesRegex(
            MODULE.D3Error, "d2_run_tree_too_large"
        ):
            MODULE.d2_run_tree_identity(root)
        read.assert_not_called()

    def test_d2_run_tree_bounds_a_file_appended_during_read(self):
        root = self.root / "d2-append-bound"
        root.mkdir(mode=0o700)
        target = root / "growing"
        write(target, "x", 0o600)
        real_read = MODULE.os.read
        appended = False

        def append_after_read(descriptor, length):
            nonlocal appended
            value = real_read(descriptor, length)
            if value and not appended:
                appended = True
                with target.open("ab") as stream:
                    stream.write(b"y")
            return value

        with mock.patch.object(
            MODULE.os,
            "read",
            side_effect=append_after_read,
        ), self.assertRaisesRegex(
            MODULE.D3Error, "d2_run_tree_changed_during_read"
        ):
            MODULE.d2_run_tree_identity(root)

    def test_d2_run_identity_rejects_entry_added_after_final_tree_scan(self):
        state_path, _, _ = self.prepare()
        manifest_path, _final_path = self.make_d2(state_path)
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        digest = (manifest_path.parent / "manifest.sha256").read_text(
            encoding="ascii"
        ).strip()
        nested = manifest_path.parent / "orchestrator" / "nested"
        nested.mkdir(mode=0o700)
        real_identity = MODULE.d2_run_tree_identity
        calls = 0

        def raced_identity(root):
            nonlocal calls
            value = real_identity(root)
            calls += 1
            if calls == 2:
                write(nested / "late-entry.json", "{}\n", 0o600)
            return value

        with mock.patch.object(
            MODULE,
            "d2_run_tree_identity",
            side_effect=raced_identity,
        ), self.assertRaisesRegex(MODULE.D3Error, "d2_run_identity_changed"):
            MODULE.capture_d2_run_identity(
                manifest_path,
                manifest["run_id"],
                digest,
            )

    def test_d2_binding_rejects_candidate_start_retirement(self):
        state_path, _, _ = self.prepare()
        manifest_path, final_path = self.make_d2(state_path)
        retirement = manifest_path.parent / "orchestrator" / "candidate-start-retirement.json"
        write(retirement, "retired\n", 0o600)
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
        self.assertIn("candidate_start_transition_retirement_required", error)

    def test_d2_binding_rejects_abort_teardown_tombstone(self):
        state_path, _, _ = self.prepare()
        manifest_path, final_path = self.make_d2(state_path)
        tombstone = (
            manifest_path.parent
            / "orchestrator"
            / "discord-resource-teardown-abort.json"
        )
        write(tombstone, "aborted\n", 0o600)
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
        self.assertIn("candidate_start_transition_retirement_required", error)

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

    def test_d2_binding_rejects_candidate_and_source_tree_drift(self):
        state_path, _, _ = self.prepare()
        manifest_path, final_path = self.make_d2(state_path)
        original = json.loads(manifest_path.read_text(encoding="utf-8"))

        def candidate_path(value):
            value["candidates"]["api"]["path"] = "/private/tmp/foreign-api"

        def candidate_digest(value):
            value["candidates"]["api"]["sha256"] = "0" * 64

        mutations = [("candidate_path", candidate_path), ("candidate_digest", candidate_digest)]
        for name in ("codex_worker", "d2_toolchain", "certification_transport"):
            mutations.append(
                (
                    f"{name}_root",
                    lambda value, key=name: value["source_trees"][key].__setitem__(
                        "root", "/private/tmp/foreign-source"
                    ),
                )
            )
            mutations.append(
                (
                    f"{name}_digest",
                    lambda value, key=name: value["source_trees"][key].__setitem__(
                        "sha256", "0" * 64
                    ),
                )
            )
        for label, mutation in mutations:
            with self.subTest(label=label):
                changed = json.loads(json.dumps(original))
                mutation(changed)
                self.rewrite_d2_manifest(
                    manifest_path, lambda value, replacement=changed: (value.clear(), value.update(replacement))
                )
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
                self.assertIn("bundle_mismatch", error)

    def test_d2_binding_rejects_boolean_schema_version(self):
        state_path, _, _ = self.prepare()
        manifest_path, final_path = self.make_d2(state_path)
        final = json.loads(final_path.read_text(encoding="utf-8"))
        final["schema_version"] = True
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
        self.assertIn("d2_final_record_mismatch", error)

    def test_d2_receipts_reject_boolean_schema_and_step(self):
        state_path, _, _ = self.prepare()
        manifest_path, _ = self.make_d2(state_path)
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        manifest_digest = (
            manifest_path.with_name("manifest.sha256").read_text().strip()
        )
        receipts_path = manifest_path.with_name("receipts.jsonl")
        receipts = [
            json.loads(line)
            for line in receipts_path.read_text(encoding="utf-8").splitlines()
        ]
        receipts[0]["schema_version"] = True
        receipts[0]["step"] = True
        previous = MODULE.ZERO_DIGEST
        for receipt in receipts:
            receipt["previous_sha256"] = previous
            receipt.pop("receipt_sha256")
            receipt["receipt_sha256"] = MODULE.sha256_bytes(
                MODULE.canonical_json(receipt).encode("utf-8")
            )
            previous = receipt["receipt_sha256"]
        write(
            receipts_path,
            "".join(MODULE.canonical_json(receipt) + "\n" for receipt in receipts),
            0o600,
        )
        with self.assertRaisesRegex(
            MODULE.D3Error,
            "d2_receipt_sequence_invalid",
        ):
            MODULE.load_d2_receipts(receipts_path, manifest, manifest_digest)

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

    def test_recheck_rejects_post_binding_bundle_replacement(self):
        state_path = self.complete_prerequisites()
        artifact = state_path.parent / "candidate-bundle" / "starring-runtime"
        artifact.chmod(0o755)
        artifact.write_text("replaced runtime\n", encoding="utf-8")
        artifact.chmod(0o555)
        status, _, error = self.invoke(["recheck", "--state", str(state_path)])
        self.assertEqual(status, 1)
        self.assertIn("candidate_bundle_artifact", error)

    def test_recheck_and_finalize_reject_post_binding_retirement(self):
        state_path = self.complete_prerequisites()
        binding = json.loads(
            state_path.with_name("d2-binding.json").read_text(encoding="utf-8")
        )
        manifest_path = pathlib.Path(binding["d2_manifest_path"])
        retirement = manifest_path.parent / "orchestrator" / "candidate-start-retirement.json"
        write(retirement, "retired\n", 0o600)
        status, _, error = self.invoke(["recheck", "--state", str(state_path)])
        self.assertEqual(status, 1)
        self.assertIn("candidate_start_transition_retirement_required", error)
        status, _, error = self.invoke(self.finalize_arguments(state_path))
        self.assertEqual(status, 1)
        self.assertIn("candidate_start_transition_retirement_required", error)

    def test_recheck_and_finalize_reject_post_binding_abort_teardown(self):
        state_path = self.complete_prerequisites()
        binding = json.loads(
            state_path.with_name("d2-binding.json").read_text(encoding="utf-8")
        )
        manifest_path = pathlib.Path(binding["d2_manifest_path"])
        tombstone = (
            manifest_path.parent
            / "orchestrator"
            / "discord-resource-teardown-abort.json"
        )
        write(tombstone, "aborted\n", 0o600)
        status, _, error = self.invoke(["recheck", "--state", str(state_path)])
        self.assertEqual(status, 1)
        self.assertIn("candidate_start_transition_retirement_required", error)
        status, _, error = self.invoke(self.finalize_arguments(state_path))
        self.assertEqual(status, 1)
        self.assertIn("candidate_start_transition_retirement_required", error)

    def test_recheck_rejects_d2_orchestrator_directory_replacement(self):
        state_path = self.complete_prerequisites()
        binding = json.loads(
            state_path.with_name("d2-binding.json").read_text(encoding="utf-8")
        )
        manifest_path = pathlib.Path(binding["d2_manifest_path"])
        orchestrator = manifest_path.parent / "orchestrator"
        replaced = manifest_path.parent / "orchestrator-replaced"
        orchestrator.rename(replaced)
        orchestrator.mkdir(mode=0o700)
        status, _, error = self.invoke(["recheck", "--state", str(state_path)])
        self.assertEqual(status, 1)
        self.assertIn("d2_binding_run_identity_changed", error)

    def test_finalize_rejects_non_string_d2_manifest_binding_path(self):
        state_path = self.complete_prerequisites()
        binding_path = state_path.with_name("d2-binding.json")
        binding = json.loads(binding_path.read_text(encoding="utf-8"))
        binding.pop("record_sha256")
        binding["d2_manifest_path"] = 1
        write(
            binding_path,
            MODULE.canonical_json(MODULE.seal_record(binding)) + "\n",
            0o600,
        )
        status, _, error = self.invoke(self.finalize_arguments(state_path))
        self.assertEqual(status, 1)
        self.assertIn("d2_binding_run_identity_invalid", error)

    def test_finalize_rejects_post_recheck_bundle_mode_drift(self):
        state_path = self.complete_prerequisites()
        artifact = state_path.parent / "candidate-bundle" / "starring-runtime"
        artifact.chmod(0o755)
        status, _, error = self.invoke(self.finalize_arguments(state_path))
        self.assertEqual(status, 1)
        self.assertIn("candidate_bundle_artifact", error)

    def test_finalize_rejects_candidate_bundle_path_drift(self):
        state_path = self.complete_prerequisites()
        binding_path = state_path.with_name("d2-binding.json")
        binding = json.loads(binding_path.read_text(encoding="utf-8"))
        binding.pop("record_sha256")
        binding["candidate_bundle_path"] = "/private/tmp/foreign-bundle.json"
        write(
            binding_path,
            MODULE.canonical_json(MODULE.seal_record(binding)) + "\n",
            0o600,
        )
        status, _, error = self.invoke(self.finalize_arguments(state_path))
        self.assertEqual(status, 1)
        self.assertIn("d2_binding_identity_invalid", error)

    def test_sealed_binding_and_recheck_require_canonical_identity(self):
        state_path = self.complete_prerequisites()
        state = json.loads(state_path.read_text(encoding="utf-8"))
        binding_path = state_path.with_name("d2-binding.json")
        binding = json.loads(binding_path.read_text(encoding="utf-8"))
        binding.pop("record_sha256")
        binding["schema_version"] = True
        write(
            binding_path,
            MODULE.canonical_json(MODULE.seal_record(binding)) + "\n",
            0o600,
        )
        with self.assertRaisesRegex(
            MODULE.D3Error,
            "d2_binding_identity_invalid",
        ):
            MODULE.load_binding(state_path.parent, state)
        recheck_path = state_path.with_name("recheck.json")
        original = json.loads(recheck_path.read_text(encoding="utf-8"))
        cases = (
            ("schema_version", True),
            ("kind", "starring.d3.foreign-recheck.v1"),
            ("pr_number", 41),
        )
        for key, value in cases:
            with self.subTest(key=key):
                changed = dict(original)
                changed.pop("record_sha256")
                changed[key] = value
                write(
                    recheck_path,
                    MODULE.canonical_json(MODULE.seal_record(changed)) + "\n",
                    0o600,
                )
                with self.assertRaisesRegex(
                    MODULE.D3Error,
                    "recheck_identity_invalid",
                ):
                    MODULE.load_recheck(state_path.parent, state)

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
        pull_overrides=None,
    ):
        directory = self.root / "bin"
        directory.mkdir(exist_ok=True)
        script = directory / "gh"
        run_value = {
            "id": 101,
            "workflow_id": 202,
            "name": "CI",
            "path": ".github/workflows/ci.yml",
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
        pull_value = {
            "number": 42,
            "state": "closed",
            "draft": False,
            "merged": True,
            "merged_at": "2026-08-04T12:00:00Z",
            "merge_commit_sha": head_sha,
            "base": {
                "ref": "main",
                "sha": self.base,
                "repo": {"full_name": "owner/repository"},
            },
            "head": {
                "sha": self.head,
                "repo": {"full_name": "owner/repository"},
            },
        }
        if pull_overrides:
            pull_value.update(pull_overrides)
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
            "repos/owner/repository/pulls/42": pull_value,
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
            self.assertEqual(
                result["pull_request"],
                {
                    "number": 42,
                    "state": "closed",
                    "draft": False,
                    "merged": True,
                    "merged_at": "2026-08-04T12:00:00Z",
                    "merge_commit_sha": self.merge,
                    "base_ref": "main",
                    "base_sha": self.base,
                    "head_sha": self.head,
                    "repository": self.github_repository,
                },
            )
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
            changed = dict(final)
            changed.pop("record_sha256")
            changed["schema_version"] = True
            write(
                final_path,
                MODULE.canonical_json(MODULE.seal_record(changed)) + "\n",
                0o600,
            )
            status, _, error = self.invoke(arguments)
            self.assertEqual(status, 1)
            self.assertIn("final_schema_invalid", error)
            write(final_path, MODULE.canonical_json(final) + "\n", 0o600)
            final["finalized_at"] = "2026-08-04T00:00:00Z"
            write(final_path, MODULE.canonical_json(final) + "\n", 0o600)
            status, _, error = self.invoke(arguments)
            self.assertEqual(status, 1)
            self.assertIn("final_record_mismatch", error)
        finally:
            os.environ["PATH"] = self.previous_path

    def test_finalize_rejects_unmerged_or_drifted_pull_request(self):
        cases = (
            {"state": "open", "merged": False, "merged_at": None},
            {"merge_commit_sha": "a" * 40},
            {"base": {"ref": "main", "sha": "b" * 40, "repo": {"full_name": "owner/repository"}}},
            {"head": {"sha": "c" * 40, "repo": {"full_name": "owner/repository"}}},
        )
        state_path = self.complete_prerequisites()
        git(self.seed, "push", "origin", f"{self.merge}:refs/heads/main")
        try:
            for overrides in cases:
                with self.subTest(overrides=overrides):
                    self.install_fake_gh(self.merge, pull_overrides=overrides)
                    status, _, error = self.invoke(self.finalize_arguments(state_path))
                    self.assertEqual(status, 1)
                    self.assertIn("pull_request_merge_identity_invalid", error)
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
