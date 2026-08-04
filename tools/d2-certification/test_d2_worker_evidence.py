import copy
import json
import pathlib
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock


DIRECTORY = pathlib.Path(__file__).parent
sys.path.insert(0, str(DIRECTORY))

import d2_orchestrator_contract as CONTRACT
import d2_worker_evidence as WORKER


def manifest():
    return {
        "authoring": {
            "provider": "codex_chatgpt",
            "model": "gpt-5.6-luna",
            "reasoning_effort": "medium",
            "auth_mode": "chatgpt",
        },
        "source_trees": {"codex_worker": {"sha256": "a" * 64}},
    }


def health(accepted=7, settled=7, **overrides):
    value = {
        "schema_version": 1,
        "status": "ok",
        "provider": "codex_chatgpt",
        "model": "gpt-5.6-luna",
        "reasoning_effort": "medium",
        "auth_mode": "chatgpt",
        "codex_cli_version": "codex-cli 1.2.3",
        "instance_id": "worker-0123456789abcdef",
        "worker_source_sha256": "a" * 64,
        "concurrency_limit": 1,
        "queue_capacity": 4,
        "request_timeout_ms": 55000,
        "active_requests": 0,
        "queued_requests": 0,
        "accepted_requests_total": accepted,
        "settled_requests_total": settled,
        "last_successful_request_id": (
            "worker-request-1" if accepted >= 8 else "worker-prior-request"
        ),
        "last_successful_completion_sha256": ("b" if accepted >= 8 else "c") * 64,
    }
    value.update(overrides)
    return value


def browser(session="session-1"):
    return {
        "schema_version": 1,
        "kind": "starring.d2.browser-authoring-evidence.v1",
        "observed_at": "2026-08-04T12:00:01.123Z",
        "public_origin": "https://d2-api.starring.co.kr",
        "authoring_http_status": 201,
        "authoring_session_id": session,
        "authoring_generation": 1,
        "expected_generation": 0,
        "authoring_disposition": "created",
        "installation_id": "installation-1",
        "one_shot": True,
        "worker_request_id": "worker-request-1",
        "worker_completion_sha256": "b" * 64,
    }


class WorkerAuthoringEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        root = pathlib.Path(self.temporary.name)
        artifact = root / "orchestrator"
        artifact.mkdir(mode=0o700)
        self.context = SimpleNamespace(
            manifest=manifest(),
            digest="b" * 64,
            artifact_directory=artifact,
        )
        self.utc_patch = mock.patch.object(
            WORKER, "utc_now", return_value="2026-08-04T12:00:01.123Z"
        )
        self.utc_patch.start()

    def tearDown(self):
        self.utc_patch.stop()
        self.temporary.cleanup()

    def assert_failure(self, code, operation):
        with self.assertRaisesRegex(CONTRACT.OrchestratorError, f"^{code}$"):
            operation()

    def test_before_after_proves_exactly_one_settled_request(self):
        before = WORKER.capture_worker_authoring_checkpoint(
            self.context, health(), "before"
        )
        self.assertEqual(before["status"], "recorded")
        result = WORKER.capture_worker_authoring_checkpoint(
            self.context, health(8, 8), "after", browser()
        )
        self.assertEqual(result["status"], "recorded")
        evidence_path = pathlib.Path(result["evidence"])
        self.assertEqual(evidence_path.stat().st_mode & 0o777, 0o600)
        evidence = json.loads(evidence_path.read_text(encoding="utf-8"))
        self.assertEqual(evidence["accepted_requests_delta"], 1)
        self.assertEqual(evidence["settled_requests_delta"], 1)
        self.assertEqual(evidence["worker_instance_id"], health()["instance_id"])
        self.assertNotIn("authoring_session_id", evidence)
        self.assertEqual(
            evidence["worker_before_observed_at"], browser()["observed_at"]
        )
        self.assertEqual(
            evidence["worker_after_observed_at"], browser()["observed_at"]
        )
        replay = WORKER.capture_worker_authoring_checkpoint(
            self.context, health(20, 20), "after", browser()
        )
        self.assertEqual(replay["status"], "exact_replay")

    def test_before_exact_replay_requires_same_counter_boundary(self):
        first = WORKER.capture_worker_authoring_checkpoint(
            self.context, health(), "before"
        )
        replay = WORKER.capture_worker_authoring_checkpoint(
            self.context, health(), "before"
        )
        self.assertEqual(first["status"], "recorded")
        self.assertEqual(replay["status"], "exact_replay")
        self.assert_failure(
            "worker_checkpoint_replay_drift",
            lambda: WORKER.capture_worker_authoring_checkpoint(
                self.context, health(8, 8), "before"
            ),
        )

    def test_after_rejects_multiple_calls_busy_worker_and_identity_change(self):
        cases = [
            (health(9, 9), "worker_authoring_evidence_invalid"),
            (health(8, 8, active_requests=1), "worker_authoring_evidence_invalid"),
            (health(8, 8, instance_id="worker-other"), "worker_identity_changed"),
        ]
        for index, (after, code) in enumerate(cases):
            with self.subTest(code=code):
                root = self.context.artifact_directory / f"case-{index}"
                root.mkdir(mode=0o700)
                context = SimpleNamespace(
                    manifest=manifest(), digest="b" * 64, artifact_directory=root
                )
                WORKER.capture_worker_authoring_checkpoint(context, health(), "before")
                self.assert_failure(
                    code,
                    lambda context=context, after=after: WORKER.capture_worker_authoring_checkpoint(
                        context, after, "after", browser()
                    ),
                )

    def test_browser_binding_and_snapshot_permissions_fail_closed(self):
        WORKER.capture_worker_authoring_checkpoint(self.context, health(), "before")
        result = WORKER.capture_worker_authoring_checkpoint(
            self.context, health(8, 8), "after", browser()
        )
        changed = copy.deepcopy(browser())
        changed["authoring_session_id"] = "session-other"
        self.assert_failure(
            "worker_authoring_evidence_invalid",
            lambda: WORKER.capture_worker_authoring_checkpoint(
                self.context, health(8, 8), "after", changed
            ),
        )
        before_path = WORKER.snapshot_path(self.context, "before")
        before_path.chmod(0o644)
        self.assert_failure(
            "worker_before_snapshot_ownership_invalid",
            lambda: WORKER.capture_worker_authoring_checkpoint(
                self.context, health(), "before"
            ),
        )
        pathlib.Path(result["evidence"]).chmod(0o600)

    def test_health_and_browser_shapes_are_exact(self):
        malformed_health = health()
        malformed_health["unexpected"] = True
        self.assert_failure(
            "worker_health_evidence_invalid",
            lambda: WORKER.worker_snapshot(
                self.context.manifest, self.context.digest, "before", malformed_health
            ),
        )
        malformed_browser = browser()
        malformed_browser["provider"] = "codex_chatgpt"
        self.assert_failure(
            "browser_authoring_evidence_invalid",
            lambda: WORKER.validate_browser_authoring_evidence(malformed_browser),
        )

    def test_browser_observation_must_fall_inside_worker_request_window(self):
        self.utc_patch.stop()
        with mock.patch.object(
            WORKER,
            "utc_now",
            side_effect=[
                "2026-08-04T12:00:02Z",
                "2026-08-04T12:00:04Z",
            ],
        ):
            WORKER.capture_worker_authoring_checkpoint(
                self.context, health(), "before"
            )
            stale = browser()
            stale["observed_at"] = "2026-08-04T12:00:01Z"
            self.assert_failure(
                "worker_authoring_time_boundary_invalid",
                lambda: WORKER.capture_worker_authoring_checkpoint(
                    self.context, health(8, 8), "after", stale
                ),
            )
        self.utc_patch.start()


if __name__ == "__main__":
    unittest.main()
