import importlib.util
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


MODULE_PATH = pathlib.Path(__file__).with_name("d3_gate_container.py")
SPEC = importlib.util.spec_from_file_location("d3_gate_container_tests", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class D3GateContainerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name).resolve()
        self.source = self.root / "source"
        self.bootstrap = self.root / "bootstrap"
        for path in (self.source, self.bootstrap):
            path.mkdir(mode=0o700)
        for path in (
            self.bootstrap / "vendor",
            self.bootstrap / "node-stage" / "node_modules",
            self.bootstrap / "bin",
            self.bootstrap / "git",
            self.bootstrap / "npm-cache",
        ):
            path.mkdir(mode=0o700, parents=True, exist_ok=True)
        self.image_id = "sha256:" + "1" * 64

    def tearDown(self):
        self.temporary.cleanup()

    def create_arguments(self, index, network):
        observed = []

        def record(arguments, _label, timeout=90, input_bytes=None):
            observed.append((arguments, timeout, input_bytes))
            return b"container\n"

        with mock.patch.object(MODULE, "SHARED_ROOT", self.root), mock.patch.object(
            MODULE,
            "docker_success",
            side_effect=record,
        ), mock.patch.object(
            MODULE,
            "inspect_object",
            return_value=None,
        ), mock.patch.object(
            MODULE,
            "ensure_gate_cache",
            return_value="starring-d3-test-cargo-target",
        ):
            MODULE.create_gate_container(
                self.root,
                self.source,
                self.bootstrap,
                self.image_id,
                index,
                1,
                "true",
                network,
                None,
            )
        return observed[-1][0]

    def test_networkless_gate_mounts_only_required_read_only_inputs(self):
        arguments = self.create_arguments(1, "none")
        self.assertIn("none", arguments)
        log_index = arguments.index("--log-driver")
        self.assertEqual(arguments[log_index + 1], "none")
        mounts = [
            arguments[index + 1]
            for index, value in enumerate(arguments[:-1])
            if value == "--mount"
        ]
        self.assertEqual(len(mounts), 7)
        self.assertTrue(any("dst=/workspace,readonly" in value for value in mounts))
        self.assertTrue(any("dst=/vendor,readonly" in value for value in mounts))
        self.assertTrue(any("dst=/node_modules,readonly" in value for value in mounts))
        self.assertTrue(any("dst=/bootstrap-bin,readonly" in value for value in mounts))
        self.assertTrue(any("dst=/git,readonly" in value for value in mounts))
        self.assertTrue(any("type=volume" in value and "dst=/scratch/target" in value for value in mounts))
        self.assertTrue(any("dst=/gate-cargo-config.toml,readonly" in value for value in mounts))
        tmpfs = [
            arguments[index + 1]
            for index, value in enumerate(arguments[:-1])
            if value == "--tmpfs"
        ]
        scratch = [value for value in tmpfs if value.startswith("/scratch:rw")]
        self.assertEqual(len(scratch), 1)
        self.assertIn("noexec", scratch[0])
        self.assertIn("nosuid", scratch[0])
        self.assertIn(f"uid={os.getuid()}", scratch[0])
        self.assertIn(f"gid={os.getgid()}", scratch[0])
        self.assertIn("mode=0700", scratch[0])
        self.assertNotIn(str(MODULE.DOCKER_SOCKET), " ".join(arguments))

    def test_every_gate_prepares_private_scratch_before_work(self):
        setup = (
            "umask 077 && mkdir -p /scratch/tmp && "
            "chmod 0700 /scratch/tmp"
        )
        expected_work = {
            1: "gate-one",
            9: "mkdir -p /scratch/package /scratch/npm-cache",
            10: "npm --prefix eval/design-harness audit --audit-level=high",
            12: "gate-twelve",
            17: "gate-seventeen",
        }
        for index, work in expected_work.items():
            with self.subTest(index=index):
                command = MODULE.fixed_gate_command(
                    index,
                    {
                        1: "gate-one",
                        12: "gate-twelve",
                        17: "gate-seventeen",
                    }.get(index, "unused"),
                )
                self.assertEqual(command.count(setup), 1)
                self.assertLess(command.index(setup), command.index(work))

    def test_gate_uses_private_tmpfs_for_default_d2_runtime_root(self):
        arguments = self.create_arguments(12, "none")
        environment = {
            arguments[index + 1]
            for index, value in enumerate(arguments[:-1])
            if value == "--env"
        }
        self.assertIn("TMPDIR=/scratch/tmp", environment)
        self.assertFalse(
            any(
                value.startswith("STARRING_D2_TEST_RUNTIME_PARENT=")
                for value in environment
            )
        )
        tmpfs = [
            arguments[index + 1]
            for index, value in enumerate(arguments[:-1])
            if value == "--tmpfs"
        ]
        private_tmp = [value for value in tmpfs if value.startswith("/private/tmp:")]
        self.assertEqual(len(private_tmp), 1)
        self.assertIn("noexec", private_tmp[0])
        self.assertIn("nosuid", private_tmp[0])
        self.assertIn("uid=0", private_tmp[0])
        self.assertIn("gid=0", private_tmp[0])
        self.assertIn("mode=1777", private_tmp[0])
        user_index = arguments.index("--user")
        self.assertEqual(
            arguments[user_index + 1], f"{os.getuid()}:{os.getgid()}"
        )
        self.assertIn(
            "/private/tmp:tmpfs:512m:uid=0:gid=0:mode=01777",
            MODULE.RUNNER_POLICY["common"]["writable_mounts"],
        )

    def test_external_audit_mounts_only_package_projection_and_scratch(self):
        arguments = self.create_arguments(10, "bridge")
        self.assertIn("bridge", arguments)
        mounts = [
            arguments[index + 1]
            for index, value in enumerate(arguments[:-1])
            if value == "--mount"
        ]
        self.assertEqual(len(mounts), 1)
        self.assertTrue(any("dst=/workspace,readonly" in value for value in mounts))
        self.assertFalse(any("dst=/git" in value for value in mounts))
        self.assertFalse(any("dst=/vendor" in value for value in mounts))
        self.assertFalse(any("dst=/node_modules" in value for value in mounts))
        self.assertEqual(arguments[-3:-1], ["/bin/sh", "-c"])
        self.assertIn(
            "npm --prefix eval/design-harness audit --audit-level=high",
            arguments[-1],
        )
        self.assertIn("/sys/fs/cgroup/memory.events", arguments[-1])

    def test_offline_install_uses_read_only_projection_and_bounded_scratch(self):
        arguments = self.create_arguments(9, "none")
        mounts = [
            arguments[index + 1]
            for index, value in enumerate(arguments[:-1])
            if value == "--mount"
        ]
        self.assertEqual(len(mounts), 2)
        self.assertTrue(any("dst=/workspace,readonly" in value for value in mounts))
        self.assertTrue(any("dst=/npm-cache,readonly" in value for value in mounts))
        self.assertFalse(any("type=volume" in value for value in mounts))
        self.assertIn("npm ci --ignore-scripts", arguments[-1])
        self.assertIn("--offline", arguments[-1])

    def test_gate_cache_is_uid_owned_bounded_tmpfs(self):
        observed = []
        name, labels = MODULE.gate_cache_identity(self.root, 1, 1)
        volume = {"Labels": labels, "Options": {
            "device": "tmpfs",
            "o": (
                f"size={MODULE.GATE_TARGET_SIZE},uid={os.getuid()},"
                f"gid={os.getgid()},mode=0700"
            ),
            "type": "tmpfs",
        }}

        def inspect(_kind, _identity):
            return None if not observed else volume

        def create(arguments, _label, timeout=90, input_bytes=None):
            observed.append((arguments, timeout, input_bytes))
            return name.encode("utf-8")

        with mock.patch.object(MODULE, "inspect_object", side_effect=inspect), mock.patch.object(
            MODULE,
            "docker_success",
            side_effect=create,
        ):
            self.assertEqual(MODULE.ensure_gate_cache(self.root, 1, 1), name)
        arguments = observed[0][0]
        self.assertIn("type=tmpfs", arguments)
        self.assertIn("device=tmpfs", arguments)
        self.assertIn(
            (
                f"o=size={MODULE.GATE_TARGET_SIZE},uid={os.getuid()},"
                f"gid={os.getgid()},mode=0700"
            ),
            arguments,
        )

    def test_gate_cache_rejects_option_drift(self):
        _, labels = MODULE.gate_cache_identity(self.root, 1, 1)
        with self.assertRaisesRegex(
            MODULE.GateContainerError,
            "gate_container_volume_owner_mismatch",
        ):
            MODULE.require_volume_labels(
                {
                    "Labels": labels,
                    "Options": {
                        "device": "tmpfs",
                        "o": "size=12g,uid=0,gid=0,mode=0700",
                        "type": "tmpfs",
                    },
                },
                labels,
            )

    def test_gate_resource_policy_matches_container_arguments(self):
        arguments = self.create_arguments(1, "none")
        for option, expected in (
            ("--memory", MODULE.GATE_MEMORY_LIMIT),
            ("--memory-swap", MODULE.GATE_MEMORY_LIMIT),
            ("--memory-swappiness", "0"),
        ):
            position = arguments.index(option)
            self.assertEqual(arguments[position + 1], expected)
        environment = {
            arguments[index + 1]
            for index, value in enumerate(arguments[:-1])
            if value == "--env"
        }
        self.assertIn(
            f"CARGO_BUILD_JOBS={MODULE.GATE_BUILD_JOBS}",
            environment,
        )
        self.assertIn(
            f"CARGO_PROFILE_DEV_DEBUG={MODULE.GATE_PROFILE_DEV_DEBUG}",
            environment,
        )
        self.assertIn(
            f"CARGO_PROFILE_TEST_DEBUG={MODULE.GATE_PROFILE_TEST_DEBUG}",
            environment,
        )
        common = MODULE.RUNNER_POLICY["common"]
        self.assertEqual(common["memory_swap_limit"], MODULE.GATE_MEMORY_LIMIT)
        self.assertEqual(common["memory_swappiness"], 0)
        self.assertEqual(common["cargo_build_jobs"], MODULE.GATE_BUILD_JOBS)
        self.assertEqual(
            common["cargo_profile_dev_debug"],
            MODULE.GATE_PROFILE_DEV_DEBUG,
        )
        self.assertEqual(
            common["cargo_profile_test_debug"],
            MODULE.GATE_PROFILE_TEST_DEBUG,
        )

    def test_attached_container_reports_pid_and_child_oom(self):
        name = "owned-gate"
        labels = {"owner": "test"}
        process = mock.Mock()
        process.poll.return_value = 0
        for state, expected in (
            (
                {"Running": False, "ExitCode": 137, "OOMKilled": True},
                "gate_container_oom",
            ),
            (
                {
                    "Running": False,
                    "ExitCode": MODULE.GATE_CHILD_OOM_EXIT,
                    "OOMKilled": False,
                },
                "gate_container_child_oom",
            ),
        ):
            with mock.patch.object(
                MODULE,
                "docker_environment",
                return_value=(pathlib.Path("/docker"), {}),
            ), mock.patch.object(
                MODULE.subprocess,
                "Popen",
                return_value=process,
            ), mock.patch.object(
                MODULE,
                "inspect_object",
                return_value={"Config": {"Labels": labels}, "State": state},
            ), mock.patch.object(
                MODULE,
                "remove_owned_container",
            ) as removed:
                with self.assertRaisesRegex(MODULE.GateContainerError, expected):
                    MODULE.start_attached_container(name, labels, 30)
                removed.assert_called_once_with(name, labels)

    def test_structural_cargo_lock_validation_rejects_quoted_source_key(self):
        candidates = [
            pathlib.Path("/opt/homebrew/bin/python3"),
            pathlib.Path(sys.executable),
        ]
        interpreter = None
        for candidate in candidates:
            if not candidate.exists():
                continue
            probe = subprocess.run(
                [str(candidate), "-c", "import tomllib"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
            )
            if probe.returncode == 0:
                interpreter = candidate
                break
        if interpreter is None:
            self.skipTest("tomllib interpreter unavailable")
        allowed = self.root / "allowed.lock"
        foreign = self.root / "foreign.lock"
        allowed.write_text(
            "version = 4\n[[package]]\nname = \"allowed\"\nversion = \"1.0.0\"\n"
            f"source = \"{MODULE.ALLOWED_CARGO_LOCK_SOURCES[0]}\"\n",
            encoding="utf-8",
        )
        foreign.write_text(
            "version = 4\n[[package]]\nname = \"foreign\"\nversion = \"1.0.0\"\n"
            '"source" = "git+http://127.0.0.1/private#deadbeef"\n',
            encoding="utf-8",
        )
        rejected = subprocess.run(
            [
                str(interpreter),
                "-c",
                MODULE.CARGO_LOCK_VALIDATOR,
                str(foreign),
                str(allowed),
                *MODULE.ALLOWED_CARGO_LOCK_SOURCES,
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(rejected.returncode, 23)
        accepted = subprocess.run(
            [
                str(interpreter),
                "-c",
                MODULE.CARGO_LOCK_VALIDATOR,
                str(allowed),
                str(allowed),
                *MODULE.ALLOWED_CARGO_LOCK_SOURCES,
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(accepted.returncode, 0, accepted.stderr.decode("utf-8"))

    def test_registry_vendor_materializer_builds_checksum_verified_directory_source(self):
        interpreter = pathlib.Path("/opt/homebrew/bin/python3")
        if not interpreter.exists():
            self.skipTest("tomllib interpreter unavailable")
        source_parent = self.root / "registry"
        source = source_parent / "index" / "example-1.2.3"
        source.mkdir(parents=True)
        payload = b"pub fn example() {}\n"
        (source / "src").mkdir()
        (source / "src" / "lib.rs").write_bytes(payload)
        (source / "Cargo.toml").write_text(
            '[package]\nname = "example"\nversion = "1.2.3"\n',
            encoding="utf-8",
        )
        (source / ".cargo-ok").write_bytes(b"ok")
        destination = self.root / "vendor"
        destination.mkdir()
        package_checksum = "a" * 64
        lock = self.root / "Cargo.lock"
        lock.write_text(
            "version = 4\n\n[[package]]\n"
            'name = "example"\nversion = "1.2.3"\n'
            'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
            f'checksum = "{package_checksum}"\n',
            encoding="utf-8",
        )
        result = subprocess.run(
            [
                str(interpreter),
                "-c",
                MODULE.REGISTRY_VENDOR_MATERIALIZER,
                str(lock),
                str(source_parent),
                str(destination),
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr.decode("utf-8"))
        materialized = destination / "example-1.2.3"
        checksum = json.loads(
            (materialized / ".cargo-checksum.json").read_text(encoding="utf-8")
        )
        self.assertEqual(checksum["package"], package_checksum)
        self.assertEqual(
            checksum["files"]["src/lib.rs"],
            hashlib.sha256(payload).hexdigest(),
        )
        self.assertFalse((materialized / ".cargo-ok").exists())

    def test_registry_vendor_materializer_rejects_symlinked_source_entry(self):
        interpreter = pathlib.Path("/opt/homebrew/bin/python3")
        if not interpreter.exists():
            self.skipTest("tomllib interpreter unavailable")
        source_parent = self.root / "registry"
        source = source_parent / "index" / "example-1.2.3"
        source.mkdir(parents=True)
        target = self.root / "target"
        target.write_text("target", encoding="utf-8")
        (source / "link").symlink_to(target)
        destination = self.root / "vendor"
        destination.mkdir()
        lock = self.root / "Cargo.lock"
        lock.write_text(
            "version = 4\n\n[[package]]\n"
            'name = "example"\nversion = "1.2.3"\n'
            'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
            'checksum = "' + "a" * 64 + '"\n',
            encoding="utf-8",
        )
        result = subprocess.run(
            [
                str(interpreter),
                "-c",
                MODULE.REGISTRY_VENDOR_MATERIALIZER,
                str(lock),
                str(source_parent),
                str(destination),
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(result.returncode, 34)

    def test_bootstrap_usage_and_capacity_limits_fail_closed(self):
        monitored = self.root / "monitored"
        monitored.mkdir()
        (monitored / "large").write_bytes(b"1234")
        with mock.patch.object(MODULE, "MAX_BOOTSTRAP_BYTES", 3):
            with self.assertRaisesRegex(
                MODULE.GateContainerError,
                "gate_bootstrap_staging_limit_exceeded",
            ):
                MODULE.bounded_tree_usage(monitored)
        with mock.patch.object(
            MODULE.shutil,
            "disk_usage",
            return_value=shutil._ntuple_diskusage(100, 99, 1),
        ), mock.patch.object(MODULE, "MINIMUM_BOOTSTRAP_FREE_BYTES", 2):
            with self.assertRaisesRegex(
                MODULE.GateContainerError,
                "gate_bootstrap_capacity_insufficient",
            ):
                MODULE.require_bootstrap_capacity(self.root)

    def test_bind_probe_cleanup_is_exclusive_and_rejects_hardlinks(self):
        probe = self.root / ".gate-container-bind-probe-0123456789abcdef"
        probe.mkdir(mode=0o700)
        output = probe / "roundtrip"
        output.write_bytes(b"starring-d3-bind-ok")
        output.chmod(0o600)
        MODULE.discard_bind_probe(self.root, probe)
        self.assertFalse(probe.exists())
        probe.mkdir(mode=0o700)
        protected = self.root / "protected"
        protected.write_bytes(b"protected")
        protected.chmod(0o600)
        os.link(protected, probe / "roundtrip")
        with self.assertRaisesRegex(
            MODULE.GateContainerError,
            "gate_container_bind_probe_cleanup_invalid",
        ):
            MODULE.discard_bind_probe(self.root, probe)
        self.assertEqual(protected.read_bytes(), b"protected")

    def test_database_gate_uses_only_postgres_network_namespace(self):
        network = "container:starring-d3-owned-postgres"
        arguments = self.create_arguments(17, network)
        network_index = arguments.index("--network")
        self.assertEqual(arguments[network_index + 1], network)
        self.assertNotIn("--publish", arguments)
        self.assertNotIn("--add-host", arguments)

    def test_attempt_preflight_and_finally_clean_gate_before_postgres(self):
        calls = []
        expected = (
            (
                MODULE.container_name(self.root, 17, 1, "gate"),
                MODULE.container_labels(self.root, 17, 1, "gate"),
            ),
            (
                MODULE.container_name(self.root, 17, 1, "postgres"),
                MODULE.container_labels(self.root, 17, 1, "postgres"),
            ),
        )

        def cleanup(_root, _index, _attempt, value):
            calls.append(value)

        with mock.patch.object(MODULE, "canonical_directory", side_effect=lambda value, *_: value), mock.patch.object(
            MODULE,
            "inspect_object",
            return_value={"Id": self.image_id},
        ), mock.patch.object(
            MODULE,
            "cleanup_gate_attempt",
            side_effect=cleanup,
        ), mock.patch.object(
            MODULE,
            "start_postgres",
            side_effect=MODULE.GateContainerError("injected"),
        ):
            with self.assertRaisesRegex(MODULE.GateContainerError, "injected"):
                MODULE.run_gate(
                    self.root,
                    self.source,
                    self.bootstrap,
                    {"image_id": self.image_id},
                    17,
                    1,
                    "true",
                    30,
                    "postgres://postgres:x@127.0.0.1:5432/starring_test",
                )
        self.assertEqual(calls, [expected, expected])

    def test_cleanup_attempts_postgres_when_gate_removal_fails(self):
        observed = []

        def remove(name, _labels):
            observed.append(name)
            if len(observed) == 1:
                raise MODULE.GateContainerError("gate_cleanup_failed")

        with mock.patch.object(MODULE, "remove_owned_container", side_effect=remove):
            with self.assertRaisesRegex(
                MODULE.GateContainerError,
                "gate_cleanup_failed",
            ):
                MODULE.remove_owned_containers((("gate", {}), ("postgres", {})))
        self.assertEqual(observed, ["gate", "postgres"])


if __name__ == "__main__":
    unittest.main()
