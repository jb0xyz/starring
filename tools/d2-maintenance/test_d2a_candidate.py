import ast
import hashlib
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
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from unittest import mock


MODULE_PATH = pathlib.Path(__file__).with_name("d2a_candidate.py")
SPEC = importlib.util.spec_from_file_location("d2a_candidate", MODULE_PATH)
CANDIDATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CANDIDATE)

TEST_FIXED_LINKER = pathlib.Path("/usr/bin/true")


def write_file(path, payload, mode):
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if isinstance(payload, (dict, list)):
        payload = (json.dumps(payload, sort_keys=True, separators=(",", ":")) + "\n").encode()
    elif isinstance(payload, str):
        payload = payload.encode()
    path.write_bytes(payload)
    path.chmod(mode)
    return path


class FakeExecutor:
    def __init__(
        self,
        fixture,
        dirty=False,
        diff_dirty=False,
        build_failure=None,
        interrupt_at=None,
        source_changes=False,
        dependency_changes_at=None,
    ):
        self.fixture = fixture
        self.dirty = dirty
        self.diff_dirty = diff_dirty
        self.build_failure = build_failure
        self.interrupt_at = interrupt_at
        self.source_changes = source_changes
        self.dependency_changes_at = dependency_changes_at
        self.calls = []
        self.builds = []
        self.metadata = []
        self.head_reads = 0

    def complete(self, argv, code=0, stdout=b"", stderr=b""):
        return subprocess.CompletedProcess(argv, code, stdout=stdout, stderr=stderr)

    def __call__(self, argv, cwd, environment):
        self.calls.append(list(argv))
        executable = pathlib.Path(argv[0]).name
        if executable == "git":
            command = argv[3:]
            if command == ["rev-parse", "--show-toplevel"]:
                return self.complete(argv, stdout=(str(self.fixture.repo) + "\n").encode())
            if command == ["rev-parse", "--verify", "HEAD"]:
                self.head_reads += 1
                commit = self.fixture.commit
                if self.source_changes and self.head_reads > 1:
                    commit = "a" * 40
                return self.complete(argv, stdout=(commit + "\n").encode())
            if command == ["rev-parse", "--verify", "HEAD^{tree}"]:
                return self.complete(argv, stdout=(self.fixture.tree + "\n").encode())
            if command == ["status", "--porcelain=v1", "--untracked-files=all"]:
                return self.complete(argv, stdout=b"?? untracked-secret.txt\n" if self.dirty else b"")
            if command in (
                ["diff", "--quiet", "--no-ext-diff", "HEAD", "--"],
                ["diff", "--cached", "--quiet", "--no-ext-diff", "HEAD", "--"],
            ):
                return self.complete(argv, code=1 if self.diff_dirty else 0)
            raise AssertionError(command)
        if argv[1:] == ["--version"] and executable in {"cargo", "rustc"}:
            version = (
                b"cargo 1.97.0 (c980f4866 2026-06-30)\n"
                if executable == "cargo"
                else b"rustc 1.97.0 (2d8144b78 2026-07-07)\n"
            )
            return self.complete(argv, stdout=version)
        if argv[1:] == ["-vV"] and executable == "rustc":
            return self.complete(
                argv,
                stdout=(
                    b"rustc 1.97.0 (2d8144b78 2026-07-07)\n"
                    b"binary: rustc\ncommit-hash: 2d8144b78\n"
                    b"commit-date: 2026-07-07\nhost: aarch64-apple-darwin\n"
                    b"release: 1.97.0\nLLVM version: 21.1.0\n"
                ),
            )
        if executable == "cargo" and "metadata" in argv[1:5]:
            self.metadata.append({"argv": list(argv), "env": dict(environment)})
            return self.complete(argv, stdout=b'{"packages":[],"version":1}\n')
        if executable == "cargo" and "build" in argv[1:5]:
            number = len(self.builds) + 1
            self.builds.append({"argv": list(argv), "cwd": pathlib.Path(cwd), "env": dict(environment)})
            if self.interrupt_at == number:
                raise KeyboardInterrupt()
            if self.build_failure == number:
                return self.complete(argv, code=101, stderr=b"compiler SECRET_TOKEN diagnostics")
            target = pathlib.Path(argv[argv.index("--target-dir") + 1])
            if "--manifest-path" in argv:
                binary = "d2-certification-transport"
            else:
                package = argv[argv.index("-p") + 1]
                binary = {
                    "starring-api": "starring-api",
                    "starring-runtime": "starring-runtime",
                    "starring-db-bootstrap": "starring-d2-db-bootstrap",
                    "starring-staging-provisioner": "starring-d2-sealed-provisioner",
                }[package]
            write_file(target / "release" / binary, f"binary:{binary}\n", 0o755)
            if self.dependency_changes_at == number:
                dependency = (
                    self.fixture.bootstrap
                    / "vendor"
                    / "workspace"
                    / "jobserver-0.1.35"
                    / "Cargo.toml"
                )
                dependency.chmod(0o600)
                dependency.write_text("[package]\nname='drift'\n", encoding="utf-8")
                dependency.chmod(0o400)
            return self.complete(argv, stderr=b"Finished release build\n")
        if executable == "otool" and argv[1] == "-L":
            path = pathlib.Path(argv[2])
            return self.complete(
                argv,
                stdout=(
                    f"{path}:\n"
                    "\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1351.0.0)\n"
                ).encode(),
            )
        raise AssertionError(argv)


class Fixture:
    def __init__(self):
        parent = pathlib.Path("/private/tmp") if pathlib.Path("/private/tmp").is_dir() else None
        self.temporary = tempfile.TemporaryDirectory(dir=parent)
        self.root = pathlib.Path(self.temporary.name).resolve()
        self.repo = self.root / "repo"
        self.repo.mkdir(mode=0o700)
        self.output = self.root / "Application Support" / "Starring" / "d2a-candidates"
        self.toolchain = self.root / "rust" / "bin"
        self.toolchain.mkdir(mode=0o755, parents=True)
        self.git = write_file(self.root / "system" / "git", b"fixture git", 0o555)
        write_file(self.toolchain / "cargo", b"fixture cargo", 0o755)
        write_file(self.toolchain / "rustc", b"fixture rustc", 0o755)
        self.commit = "c" * 40
        self.tree = "d" * 40
        worker = self.repo / "tools" / "codex-worker"
        for name in CANDIDATE.CODEX_WORKER_FILES:
            write_file(worker / name, f"worker:{name}\n", 0o644)
        for name in ("ignored.test.mjs",):
            write_file(worker / name, "test only\n", 0o644)
        write_file(self.repo / "Cargo.toml", "[workspace]\n", 0o644)
        write_file(
            self.repo / "Cargo.lock",
            b"# lock\n" + b"# bounded fixture entry\n" * 4096,
            0o644,
        )
        write_file(
            self.repo / "tools" / "d2-certification-transport" / "Cargo.toml",
            "[package]\nname='d2-certification-transport'\n",
            0o644,
        )
        write_file(
            self.repo / "tools" / "d2-certification-transport" / "Cargo.lock",
            "# transport lock\n",
            0o644,
        )
        write_file(
            self.repo / "tools" / "d2-maintenance" / "d2a_candidate.py",
            MODULE_PATH.read_bytes(),
            0o644,
        )
        self.operator_root = self.root / "operators"
        self.operator_root.mkdir(mode=0o700)
        self.operators = {}
        for name in CANDIDATE.OPERATOR_NAMES:
            self.operators[name] = str(write_file(self.operator_root / name, f"operator:{name}\n", 0o555))
        self.operator_root.chmod(0o555)
        self.dependency_state = self.root / "d3-state"
        self.dependency_state.mkdir(mode=0o700)
        self.bootstrap = self.dependency_state / "gate-bootstrap"
        self.bootstrap.mkdir(mode=0o700)
        workspace_vendor = self.bootstrap / "vendor" / "workspace"
        transport_vendor = self.bootstrap / "vendor" / "transport"
        write_file(workspace_vendor / "jobserver-0.1.35" / "Cargo.toml", "[package]\n", 0o400)
        write_file(transport_vendor / "twilight-0.1.0" / "Cargo.toml", "[package]\n", 0o400)
        write_file(
            self.bootstrap / "native-cargo-config.toml",
            CANDIDATE.workspace_cargo_configuration(self.bootstrap),
            0o400,
        )
        write_file(
            self.bootstrap / "native-transport-cargo-config.toml",
            CANDIDATE.transport_cargo_configuration(self.bootstrap),
            0o400,
        )
        for path in sorted(
            self.bootstrap.rglob("*"), key=lambda value: len(value.parts), reverse=True
        ):
            if path.is_dir():
                path.chmod(0o500)
        self.bootstrap.chmod(0o555)
        tree = CANDIDATE.gate_bootstrap_tree_identity(self.bootstrap)
        record = {
            "schema_version": 1,
            "kind": "starring.d3.gate-bootstrap.v1",
            "gate_runtime_sha256": "e" * 64,
            **tree,
        }
        record["record_sha256"] = hashlib.sha256(
            CANDIDATE.canonical_json(record).encode("utf-8")
        ).hexdigest()
        self.dependency_record = write_file(
            self.dependency_state / "gate-bootstrap.json", record, 0o600
        )
        self.config = {
            "schema_version": 2,
            "kind": CANDIDATE.CONFIG_KIND,
            "operators": dict(self.operators),
            "dependencies": {
                "bootstrap_root": str(self.bootstrap),
                "record_path": str(self.dependency_record),
                "record_sha256": record["record_sha256"],
                "tree_sha256": record["tree_sha256"],
            },
        }
        self.config_path = write_file(self.root / "candidate-config.json", self.config, 0o600)
        self.lock = self.root / "candidate-builder.lock"

    def builder(self, executor, builder_class=CANDIDATE.CandidateBuilder):
        class BoundBuilder(builder_class):
            def provenance(self, *args, **kwargs):
                value = super().provenance(*args, **kwargs)
                _raw, identity = CANDIDATE.read_file(
                    self.source_root
                    / "tools"
                    / "d2-maintenance"
                    / "d2a_candidate.py",
                    "candidate_builder_fixture",
                    4 * 1024 * 1024,
                )
                value["builder"] = identity
                return value

            def darwin_toolchain(self, _environment):
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

        return BoundBuilder(
            executor=executor,
            source_root=self.repo,
            bundle_parent=self.output,
            rust_toolchain_bin=self.toolchain,
            lock_path=self.lock,
            git_path=self.git,
        )

    def cleanup(self):
        if self.operator_root.exists():
            self.operator_root.chmod(0o700)
        for bundle in self.output.glob("candidate-*") if self.output.exists() else ():
            for directory in [bundle / "codex-worker", bundle]:
                if directory.exists():
                    directory.chmod(0o700)
        if self.bootstrap.exists():
            self.bootstrap.chmod(0o700)
            for path in self.bootstrap.rglob("*"):
                path.chmod(0o700 if path.is_dir() else 0o600)
        self.temporary.cleanup()


class CandidateBuilderTests(unittest.TestCase):
    def setUp(self):
        self.fixed_linker_patch = mock.patch.object(
            CANDIDATE,
            "FIXED_LINKERS",
            (TEST_FIXED_LINKER,) * len(CANDIDATE.FIXED_LINKERS),
        )
        self.fixed_linker_patch.start()
        self.addCleanup(self.fixed_linker_patch.stop)
        self.fixture = Fixture()

    def tearDown(self):
        self.fixture.cleanup()

    def test_success_uses_exact_five_commands_and_atomically_publishes_immutable_bundle(self):
        executor = FakeExecutor(self.fixture)
        result = self.fixture.builder(executor).build(self.fixture.config_path)
        self.assertEqual(result["status"], "passed")
        self.assertFalse(result["release_eligible"])
        self.assertFalse(result["commercial_certification"])
        self.assertEqual(len(executor.builds), 5)
        self.assertEqual(len(executor.metadata), 2)
        self.assertEqual(
            [call["argv"][2] for call in executor.metadata],
            [
                str(self.fixture.bootstrap / "native-cargo-config.toml"),
                str(self.fixture.bootstrap / "native-transport-cargo-config.toml"),
            ],
        )
        self.assertTrue(
            all(call["env"]["CARGO_NET_OFFLINE"] == "true" for call in executor.metadata)
        )
        self.assertTrue(
            all("--no-deps" in call["argv"] for call in executor.metadata)
        )
        expected_packages = [
            ("starring-api", "starring-api"),
            ("starring-runtime", "starring-runtime"),
            ("starring-db-bootstrap", "starring-d2-db-bootstrap"),
            ("starring-staging-provisioner", "starring-d2-sealed-provisioner"),
        ]
        for build, (package, binary) in zip(executor.builds[:4], expected_packages):
            argv = build["argv"]
            self.assertEqual(argv[1], "--config")
            self.assertEqual(argv[2], str(self.fixture.bootstrap / "native-cargo-config.toml"))
            self.assertEqual(argv[3:6], ["build", "--frozen", "--release"])
            self.assertEqual(argv[argv.index("-p") + 1], package)
            self.assertEqual(argv[argv.index("--bin") + 1], binary)
            self.assertEqual(build["env"]["STARRING_RUNTIME_BUILD_REVISION"], self.fixture.commit)
        transport = executor.builds[4]["argv"]
        self.assertEqual(transport[1], "--config")
        self.assertEqual(
            transport[2], str(self.fixture.bootstrap / "native-transport-cargo-config.toml")
        )
        self.assertEqual(transport[3:6], ["build", "--frozen", "--release"])
        self.assertEqual(
            transport[transport.index("--manifest-path") + 1],
            "tools/d2-certification-transport/Cargo.toml",
        )

        bundle = pathlib.Path(result["bundle"])
        spec_path = pathlib.Path(result["candidate_spec"])
        provenance_path = pathlib.Path(result["provenance"])
        self.assertEqual(stat.S_IMODE(bundle.stat().st_mode), 0o555)
        self.assertEqual(stat.S_IMODE(spec_path.stat().st_mode), 0o400)
        self.assertEqual(stat.S_IMODE(provenance_path.stat().st_mode), 0o400)
        spec = json.loads(spec_path.read_bytes())
        self.assertEqual(set(spec), CANDIDATE.CANDIDATE_FIELDS)
        self.assertEqual(spec["schema_version"], 2)
        self.assertEqual(spec["kind"], CANDIDATE.CANDIDATE_KIND)
        self.assertEqual(set(spec["candidates"]), CANDIDATE.CANDIDATE_NAMES)
        self.assertEqual(spec["commit_sha"], self.fixture.commit)
        self.assertEqual(spec["source_tree_sha"], self.fixture.tree)
        self.assertEqual(spec["bundle"], str(bundle))
        self.assertEqual(
            spec["provenance_sha256"],
            hashlib.sha256(provenance_path.read_bytes()).hexdigest(),
        )
        for name, record in spec["candidates"].items():
            self.assertEqual(set(record), CANDIDATE.CANDIDATE_RECORD_FIELDS)
            path = pathlib.Path(record["path"])
            self.assertIn(bundle, path.parents)
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o444 if name == "codex_worker" else 0o555)
            self.assertEqual(record["sha256"], hashlib.sha256(path.read_bytes()).hexdigest())
        worker_root = bundle / "codex-worker"
        self.assertEqual(stat.S_IMODE(worker_root.stat().st_mode), 0o555)
        self.assertEqual({path.name for path in worker_root.iterdir()}, set(CANDIDATE.CODEX_WORKER_FILES))
        self.assertTrue(all(stat.S_IMODE(path.stat().st_mode) == 0o444 for path in worker_root.iterdir()))
        provenance = json.loads(provenance_path.read_bytes())
        self.assertEqual(provenance["source"]["commit"], self.fixture.commit)
        self.assertEqual(provenance["source"]["tree"], self.fixture.tree)
        self.assertIs(provenance["source"]["clean"], True)
        self.assertEqual(provenance["environment"]["STARRING_RUNTIME_BUILD_REVISION"], self.fixture.commit)
        self.assertEqual(
            set(provenance["dependencies"]), CANDIDATE.DEPENDENCY_SNAPSHOT_FIELDS
        )
        self.assertEqual(
            provenance["dependencies"]["record_sha256"],
            self.fixture.config["dependencies"]["record_sha256"],
        )
        self.assertFalse(provenance["release_eligible"])
        self.assertFalse(provenance["commercial_certification"])
        state_path, state = CANDIDATE.load_state(result["state"])
        self.assertEqual(stat.S_IMODE(state_path.stat().st_mode), 0o600)
        self.assertEqual(state["status"], "passed")
        self.assertFalse(pathlib.Path(state["build_root"]).exists())
        self.assertFalse(pathlib.Path(state["publication_staging"]).exists())

    def test_publication_validation_rejects_mutable_metadata_or_digest_drift(self):
        result = self.fixture.builder(FakeExecutor(self.fixture)).build(self.fixture.config_path)
        state_path, state = CANDIDATE.load_state(result["state"])
        bundle = pathlib.Path(result["bundle"])
        spec_path = pathlib.Path(result["candidate_spec"])
        provenance_path = pathlib.Path(result["provenance"])

        spec_path.chmod(0o600)
        with self.assertRaises(CANDIDATE.CandidateError):
            self.fixture.builder(FakeExecutor(self.fixture)).validate_bundle(bundle, state)
        spec_path.chmod(0o400)

        provenance_path.chmod(0o600)
        provenance_path.write_bytes(provenance_path.read_bytes() + b" ")
        provenance_path.chmod(0o400)
        with self.assertRaises(CANDIDATE.CandidateError) as raised:
            self.fixture.builder(FakeExecutor(self.fixture)).validate_bundle(bundle, state)
        self.assertEqual(raised.exception.code, "candidate_publication_changed")
        self.assertTrue(state_path.is_file())

    def test_real_producer_provenance_round_trips_through_bootstrap_consumer(self):
        result = self.fixture.builder(FakeExecutor(self.fixture)).build(
            self.fixture.config_path
        )
        bootstrap_path = MODULE_PATH.with_name("d2a_bootstrap.py")
        bootstrap_spec = importlib.util.spec_from_file_location(
            "d2a_bootstrap_roundtrip", bootstrap_path
        )
        bootstrap = importlib.util.module_from_spec(bootstrap_spec)
        bootstrap_spec.loader.exec_module(bootstrap)
        publication = bootstrap.load_candidate_publication(
            result["candidate_spec"]
        )
        self.assertEqual(publication[1]["commit_sha"], self.fixture.commit)
        self.assertEqual(
            publication[1]["provenance_sha256"], publication[5]
        )

    def test_build_recipe_and_worker_inventory_match_d3_source_constants(self):
        d3_path = MODULE_PATH.parent.parent / "d3-certification" / "d3_candidate_bundle.py"
        tree = ast.parse(d3_path.read_text(encoding="utf-8"))
        values = {}
        wanted = {
            "CODEX_WORKER_SOURCE_FILES",
            "REPO_CANDIDATE_ARTIFACTS",
            "WORKSPACE_RELEASE_COMMANDS",
            "TRANSPORT_RELEASE_COMMAND",
        }
        for statement in tree.body:
            if (
                isinstance(statement, ast.Assign)
                and len(statement.targets) == 1
                and isinstance(statement.targets[0], ast.Name)
                and statement.targets[0].id in wanted
            ):
                values[statement.targets[0].id] = ast.literal_eval(statement.value)
        self.assertEqual(set(values), wanted)
        self.assertEqual(CANDIDATE.CODEX_WORKER_FILES, values["CODEX_WORKER_SOURCE_FILES"])
        self.assertEqual(CANDIDATE.REPO_ARTIFACTS, values["REPO_CANDIDATE_ARTIFACTS"])
        self.assertEqual(
            tuple(("cargo", *command) for command in CANDIDATE.WORKSPACE_COMMANDS),
            values["WORKSPACE_RELEASE_COMMANDS"],
        )
        self.assertEqual(
            ("cargo", *CANDIDATE.TRANSPORT_COMMAND),
            values["TRANSPORT_RELEASE_COMMAND"],
        )

    def test_dirty_or_diffing_head_fails_before_output_or_build(self):
        for options in ({"dirty": True}, {"diff_dirty": True}):
            with self.subTest(options=options):
                executor = FakeExecutor(self.fixture, **options)
                with self.assertRaises(CANDIDATE.CandidateError) as raised:
                    self.fixture.builder(executor).build(self.fixture.config_path)
                self.assertEqual(raised.exception.code, "source_dirty")
                self.assertEqual(executor.builds, [])
                self.assertFalse(self.fixture.output.exists())

    def test_config_is_exact_private_and_operators_are_existing_immutable_files(self):
        _path, config, _digest = CANDIDATE.read_private_json(self.fixture.config_path, "candidate_config")
        validated, identities = CANDIDATE.validate_config(config)
        self.assertIs(validated, config)
        self.assertEqual(set(identities), set(CANDIDATE.OPERATOR_NAMES))
        self.assertEqual(
            {name: identity["path"] for name, identity in identities.items()},
            self.fixture.operators,
        )
        extra = dict(self.fixture.config)
        extra["unexpected"] = True
        with self.assertRaises(CANDIDATE.CandidateError):
            CANDIDATE.validate_config(extra)
        legacy = dict(self.fixture.config)
        legacy["schema_version"] = 1
        legacy["kind"] = "starring.d2a.candidate-operator-config.v1"
        with self.assertRaises(CANDIDATE.CandidateError):
            CANDIDATE.validate_config(legacy)
        loose = write_file(self.fixture.root / "loose.json", self.fixture.config, 0o644)
        with self.assertRaises(CANDIDATE.CandidateError):
            CANDIDATE.read_private_json(loose, "candidate_config")
        self.fixture.operator_root.chmod(0o700)
        pathlib.Path(self.fixture.operators["node"]).chmod(0o755)
        with self.assertRaises(CANDIDATE.CandidateError):
            CANDIDATE.validate_config(self.fixture.config)

    def test_dependency_record_tree_and_source_inputs_are_exactly_bound(self):
        snapshot = CANDIDATE.load_dependency_snapshot(
            self.fixture.config["dependencies"], self.fixture.repo
        )
        self.assertEqual(set(snapshot), CANDIDATE.DEPENDENCY_SNAPSHOT_FIELDS)
        self.assertEqual(
            set(snapshot["source_inputs"]), set(CANDIDATE.DEPENDENCY_SOURCE_INPUTS)
        )
        self.assertEqual(snapshot["record"]["path"], str(self.fixture.dependency_record))
        self.assertEqual(snapshot["tree_sha256"], self.fixture.config["dependencies"]["tree_sha256"])
        self.assertIs(CANDIDATE.validate_dependency_snapshot(snapshot, self.fixture.repo), snapshot)

        changed = dict(self.fixture.config["dependencies"])
        changed["tree_sha256"] = "0" * 64
        with self.assertRaises(CANDIDATE.CandidateError) as raised:
            CANDIDATE.load_dependency_snapshot(changed, self.fixture.repo)
        self.assertEqual(raised.exception.code, "candidate_dependencies_invalid")

    def test_dependency_tree_drift_fails_before_state_or_build_output(self):
        target = (
            self.fixture.bootstrap
            / "vendor"
            / "workspace"
            / "jobserver-0.1.35"
            / "Cargo.toml"
        )
        target.chmod(0o600)
        target.write_text("[package]\nname='changed'\n", encoding="utf-8")
        target.chmod(0o400)
        executor = FakeExecutor(self.fixture)
        with self.assertRaises(CANDIDATE.CandidateError) as raised:
            self.fixture.builder(executor).build(self.fixture.config_path)
        self.assertEqual(raised.exception.code, "dependency_tree_mismatch")
        self.assertEqual(executor.builds, [])
        self.assertFalse(self.fixture.output.exists())

    def test_dependency_tree_is_rechecked_after_every_build(self):
        executor = FakeExecutor(self.fixture, dependency_changes_at=2)
        result = self.fixture.builder(executor).build(self.fixture.config_path)
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["error_code"], "dependency_tree_mismatch")
        self.assertEqual(len(executor.builds), 2)
        self.assertIsNone(result["candidate_spec"])

    def test_operator_identity_must_match_initial_config_validation(self):
        class ChangedOperatorBuilder(CANDIDATE.CandidateBuilder):
            def operator_identities(self, config):
                identities = super().operator_identities(config)
                identities["node"] = {**identities["node"], "sha256": "0" * 64}
                return identities

        executor = FakeExecutor(self.fixture)
        with self.assertRaises(CANDIDATE.CandidateError) as raised:
            self.fixture.builder(executor, ChangedOperatorBuilder).build(self.fixture.config_path)
        self.assertEqual(raised.exception.code, "operator_changed")
        self.assertEqual(executor.builds, [])
        self.assertFalse(self.fixture.output.exists())

    def test_failed_or_interrupted_build_never_publishes_spec_and_resume_cleans_partial(self):
        for mode in ("failure", "interrupt"):
            with self.subTest(mode=mode):
                executor = FakeExecutor(
                    self.fixture,
                    build_failure=3 if mode == "failure" else None,
                    interrupt_at=3 if mode == "interrupt" else None,
                )
                result = self.fixture.builder(executor).build(self.fixture.config_path)
                self.assertEqual(result["status"], "failed")
                self.assertEqual(
                    result["error_code"],
                    "candidate_build_failed" if mode == "failure" else "candidate_interrupted",
                )
                self.assertIsNone(result["candidate_spec"])
                state_path, state = CANDIDATE.load_state(result["state"])
                self.assertFalse(pathlib.Path(state["final_bundle"]).exists())
                self.assertTrue(pathlib.Path(state["build_root"]).exists())
                if mode == "interrupt":
                    self.assertFalse(state["build_processes_quiescent"])
                    with self.assertRaises(CANDIDATE.CandidateError) as raised:
                        self.fixture.builder(FakeExecutor(self.fixture)).resume_cleanup(state_path)
                    self.assertEqual(
                        raised.exception.code, "candidate_manual_recovery_required"
                    )
                else:
                    cleaned = self.fixture.builder(FakeExecutor(self.fixture)).resume_cleanup(state_path)
                    self.assertEqual(cleaned["status"], "cleaned")
                    self.assertFalse(pathlib.Path(state["build_root"]).exists())
                    self.assertFalse(pathlib.Path(state["publication_staging"]).exists())

                # Use a fresh fixture for the second subtest because cleanup is
                # intentionally terminal for this build id.
                if mode == "failure":
                    self.fixture.cleanup()
                    self.fixture = Fixture()

    def test_source_change_after_build_refuses_publication(self):
        executor = FakeExecutor(self.fixture, source_changes=True)
        result = self.fixture.builder(executor).build(self.fixture.config_path)
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["error_code"], "source_changed")
        _path, state = CANDIDATE.load_state(result["state"])
        self.assertFalse(pathlib.Path(state["final_bundle"]).exists())
        self.assertIsNone(state["candidate_spec_sha256"])

    def test_crash_after_atomic_rename_is_recovered_as_passed_not_deleted(self):
        class CrashAfterPublish(CANDIDATE.CandidateBuilder):
            def finish_cleanup(self, state_path, state):
                raise KeyboardInterrupt()

        executor = FakeExecutor(self.fixture)
        crashed = self.fixture.builder(executor, CrashAfterPublish).build(self.fixture.config_path)
        self.assertEqual(crashed["status"], "failed")
        state_path, state = CANDIDATE.load_state(crashed["state"])
        final = pathlib.Path(state["final_bundle"])
        self.assertTrue(final.is_dir())
        recovered = self.fixture.builder(FakeExecutor(self.fixture)).resume_cleanup(state_path)
        self.assertEqual(recovered["status"], "passed")
        self.assertTrue(final.is_dir())
        self.assertTrue(pathlib.Path(recovered["candidate_spec"]).is_file())

    def test_cli_parser_redacts_untrusted_values(self):
        stdout = StringIO()
        stderr = StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr):
            status = CANDIDATE.main(["build", "--unknown", "SECRET_VALUE"])
        self.assertEqual(status, 1)
        self.assertEqual(stderr.getvalue(), "")
        self.assertNotIn("SECRET_VALUE", stdout.getvalue())
        self.assertEqual(json.loads(stdout.getvalue())["error_code"], "cli_invalid")

    def test_boolean_schema_versions_are_rejected_for_config_and_state(self):
        config = dict(self.fixture.config)
        config["schema_version"] = True
        write_file(self.fixture.config_path, config, 0o600)
        with self.assertRaises(CANDIDATE.CandidateError) as raised:
            self.fixture.builder(FakeExecutor(self.fixture)).build(
                self.fixture.config_path
            )
        self.assertEqual(raised.exception.code, "candidate_config_invalid")

        write_file(self.fixture.config_path, self.fixture.config, 0o600)
        result = self.fixture.builder(
            FakeExecutor(self.fixture, build_failure=1)
        ).build(self.fixture.config_path)
        _path, state = CANDIDATE.load_state(result["state"])
        state["schema_version"] = True
        with self.assertRaises(CANDIDATE.CandidateError) as raised:
            CANDIDATE.validate_state(state)
        self.assertEqual(raised.exception.code, "candidate_state_invalid")

    def test_rust_toolchain_manifest_rejects_untrusted_linker(self):
        linker = write_file(self.fixture.root / "linker", b"fixture linker", 0o777)
        with mock.patch.object(CANDIDATE, "FIXED_LINKERS", (linker,)):
            with self.assertRaises(CANDIDATE.CandidateError) as raised:
                CANDIDATE.rust_toolchain_manifest(self.fixture.toolchain)
        self.assertEqual(raised.exception.code, "rust_linker_invalid")

    def test_bounded_subprocess_stops_at_cap_plus_one(self):
        result = CANDIDATE.bounded_subprocess(
            [sys.executable, "-c", "import os; os.write(1, b'x' * 4097)"],
            self.fixture.root,
            {"PATH": "/usr/bin:/bin"},
            10,
            maximum=4096,
        )
        self.assertTrue(result.output_exceeded)
        self.assertTrue(result.process_group_quiescent)

    def test_bounded_subprocess_does_not_wait_for_orphan_pipe_holder(self):
        script = (
            "import os,time; pid=os.fork(); "
            "(os._exit(0) if pid else time.sleep(30))"
        )
        started = time.monotonic()
        result = CANDIDATE.bounded_subprocess(
            [sys.executable, "-c", script],
            self.fixture.root,
            {"PATH": "/usr/bin:/bin"},
            20,
            maximum=4096,
        )
        self.assertTrue(result.timed_out)
        self.assertLess(time.monotonic() - started, 5)

    def test_bounded_subprocess_kills_term_ignoring_group_on_timeout(self):
        script = (
            "import os,signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); "
            "pid=os.fork(); time.sleep(30)"
        )
        started = time.monotonic()
        result = CANDIDATE.bounded_subprocess(
            [sys.executable, "-c", script],
            self.fixture.root,
            {"PATH": "/usr/bin:/bin"},
            0.2,
            maximum=4096,
        )
        self.assertTrue(result.timed_out)
        self.assertLess(time.monotonic() - started, 6)


if __name__ == "__main__":
    unittest.main()
