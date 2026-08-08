import importlib.util
import json
import os
import pathlib
import sys
import tempfile
import unittest
from unittest import mock


MODULE_PATH = pathlib.Path(__file__).with_name("d3_launchd_job.py")
SPEC = importlib.util.spec_from_file_location("d3_launchd_job_tests", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class D3LaunchdJobTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name).resolve()
        self.root.chmod(0o700)

    def tearDown(self):
        self.temporary.cleanup()

    def write_result(self, nonce, label):
        path = self.root / f".candidate-launchd-result-{nonce}.json"
        path.write_text(
            json.dumps(
                {"exit_code": 0, "label": label, "nonce": nonce},
                sort_keys=True,
                separators=(",", ":"),
            ),
            encoding="utf-8",
        )
        path.chmod(0o600)
        return path

    def write_plist(self, nonce, argv=None, environment=None):
        label = f"co.starring.d3.candidate.{nonce}"
        result_path = self.root / f".candidate-launchd-result-{nonce}.json"
        selected_argv = ["/usr/bin/true"] if argv is None else argv
        selected_environment = (
            {"PATH": "/usr/bin:/bin"}
            if environment is None
            else environment
        )
        environment_payload = json.dumps(
            selected_environment,
            sort_keys=True,
            separators=(",", ":"),
        )
        program_arguments = [
            str(MODULE.PYTHON),
            "-I",
            "-c",
            MODULE.LAUNCHER,
            environment_payload,
            str(self.root),
            str(result_path),
            nonce,
            label,
            *selected_argv,
        ]
        payload = MODULE.job_plist(label, program_arguments)
        path = self.root / f".candidate-launchd-job-{nonce}.plist"
        path.write_bytes(payload)
        path.chmod(0o600)
        return path, payload

    def test_result_is_identity_checked_and_removed(self):
        nonce = "1" * 32
        label = f"co.starring.d3.candidate.{nonce}"
        path = self.write_result(nonce, label)
        self.assertEqual(MODULE.read_result(path, nonce, label), 0)
        self.assertFalse(path.exists())

    def test_result_hardlink_is_rejected(self):
        nonce = "2" * 32
        label = f"co.starring.d3.candidate.{nonce}"
        path = self.write_result(nonce, label)
        linked = self.root / "linked"
        os.link(path, linked)
        with self.assertRaisesRegex(
            MODULE.LaunchdJobError,
            "candidate_launchd_result_invalid",
        ):
            MODULE.read_result(path, nonce, label)
        self.assertTrue(linked.exists())

    def test_run_job_retires_service_before_reading_result(self):
        nonce = "3" * 32
        label = f"co.starring.d3.candidate.{nonce}"
        result_path = self.root / f".candidate-launchd-result-{nonce}.json"
        observed = []
        service_states = iter(((False, False), (True, True), (False, False)))

        def submit(arguments, operation, allowed=(0,)):
            observed.append((arguments, operation, allowed))
            if arguments[0] == "bootstrap":
                self.write_result(nonce, label)
            return mock.Mock(returncode=0, stdout=b"", stderr=b"")

        with mock.patch.object(
            MODULE,
            "service_status",
            side_effect=lambda _label: next(service_states),
        ), mock.patch.object(
            MODULE,
            "launchctl",
            side_effect=submit,
        ), mock.patch.object(
            MODULE,
            "require_executable",
            side_effect=lambda path, _label: path,
        ):
            monitored = []
            code = MODULE.run_job(
                ["/usr/bin/true"],
                self.root,
                {"PATH": "/usr/bin:/bin"},
                30,
                self.root,
                nonce,
                lambda: monitored.append(True),
            )
        self.assertEqual(code, 0)
        self.assertEqual(len(observed), 2)
        self.assertEqual(
            observed[0][0][:2],
            ["bootstrap", f"gui/{os.getuid()}"],
        )
        self.assertEqual(observed[1][0], ["bootout", MODULE.service_target(label)])
        self.assertGreaterEqual(len(monitored), 1)
        self.assertFalse(result_path.exists())

    def test_run_job_recovers_completed_existing_service(self):
        nonce = "4" * 32
        label = f"co.starring.d3.candidate.{nonce}"
        result_path = self.write_result(nonce, label)
        plist_path, _ = self.write_plist(nonce)
        service_states = iter(((True, True), (True, True), (False, False)))
        observed = []

        def invoke(arguments, operation, allowed=(0,)):
            observed.append((arguments, operation, allowed))
            return mock.Mock(returncode=0, stdout=b"", stderr=b"")

        with mock.patch.object(
            MODULE,
            "service_status",
            side_effect=lambda _label: next(service_states),
        ), mock.patch.object(MODULE, "launchctl", side_effect=invoke):
            code = MODULE.run_job(
                ["/usr/bin/true"],
                self.root,
                {"PATH": "/usr/bin:/bin"},
                30,
                self.root,
                nonce,
                lambda: None,
            )
        self.assertEqual(code, 0)
        self.assertEqual(len(observed), 1)
        self.assertEqual(observed[0][0], ["bootout", MODULE.service_target(label)])
        self.assertFalse(result_path.exists())
        self.assertFalse(plist_path.exists())

    def test_run_job_preserves_active_existing_service(self):
        nonce = "5" * 32
        self.write_plist(nonce)
        with mock.patch.object(
            MODULE,
            "service_status",
            return_value=(True, True),
        ):
            with self.assertRaisesRegex(
                MODULE.LaunchdJobError,
                "candidate_launchd_job_active",
            ):
                MODULE.run_job(
                    ["/usr/bin/true"],
                    self.root,
                    {"PATH": "/usr/bin:/bin"},
                    30,
                    self.root,
                    nonce,
                    lambda: None,
                )

    @unittest.skipUnless(
        sys.platform == "darwin" and pathlib.Path("/bin/launchctl").exists(),
        "launchd unavailable",
    )
    def test_launchd_coalition_reaps_detached_session(self):
        nonce = os.urandom(16).hex()
        pid_path = self.root / "detached.pid"
        script = (
            "import os,pathlib,time;"
            "pid=os.fork();"
            "os._exit(0) if pid else None;"
            "os.setsid();"
            f"pathlib.Path({str(pid_path)!r}).write_text(str(os.getpid()),encoding='ascii');"
            "[os.close(fd) for fd in range(3,256)];"
            "time.sleep(60)"
        )
        pid = None
        try:
            code = MODULE.run_job(
                ["/usr/bin/python3", "-c", script],
                self.root,
                {"HOME": "/var/empty", "PATH": "/usr/bin:/bin"},
                30,
                self.root,
                nonce,
                lambda: None,
            )
            self.assertEqual(code, 0)
            pid = int(pid_path.read_text(encoding="ascii"))
            with self.assertRaises(ProcessLookupError):
                os.kill(pid, 0)
        finally:
            if pid is not None:
                try:
                    os.kill(pid, 9)
                except ProcessLookupError:
                    pass


if __name__ == "__main__":
    unittest.main()
