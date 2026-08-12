import ast
import hashlib
import importlib.util
import inspect
import json
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock
from contextlib import redirect_stdout
from contextlib import redirect_stderr
from io import StringIO


MODULE_PATH = pathlib.Path(__file__).with_name("d2a.py")
SPEC = importlib.util.spec_from_file_location("d2a", MODULE_PATH)
D2A = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(D2A)


class D2AEvidenceTests(unittest.TestCase):
    def evidence(self):
        return {
            "schema_version": 1,
            "kind": "starring.d2a.authentication-evidence.v1",
            "certification_class": "automated_maintenance_v1",
            "operation": "auth-smoke",
            "observed_at": "2026-08-12T00:00:00Z",
            "run_id": "d2-20260811t211713z-c7b9d9710178",
            "manifest_sha256": "b" * 64,
            "public_origin": "https://d2-api.starring.co.kr",
            "direct_auth_used": True,
            "release_eligible": False,
            "principal_id": "discord:1056857223529250906",
            "guild_id": "1536845588954353676",
            "installation_id": "installation:starring-d2-test",
            "uncovered_release_boundaries": D2A.UNCOVERED_RELEASE_BOUNDARIES,
            "me_status": 200,
            "authority_check_status": 204,
            "logout_status": 204,
            "post_logout_me_status": 401,
        }

    def test_public_evidence_is_accepted_only_as_non_release(self):
        self.assertEqual(
            D2A.validate_public_evidence(
                self.evidence(), "auth-smoke", self.binding()
            )[
                "release_eligible"
            ],
            False,
        )
        for field, value in (("release_eligible", True), ("direct_auth_used", False)):
            forged = self.evidence()
            forged[field] = value
            with self.assertRaises(D2A.D2AError):
                D2A.validate_public_evidence(forged, "auth-smoke", self.binding())
        forged = self.evidence()
        forged["schema_version"] = True
        with self.assertRaises(D2A.D2AError):
            D2A.validate_public_evidence(forged, "auth-smoke", self.binding())

    def binding(self):
        evidence = self.evidence()
        return {
            field: evidence[field]
            for field in (
                "run_id",
                "manifest_sha256",
                "public_origin",
                "principal_id",
                "guild_id",
                "installation_id",
            )
        }

    def direct_onboarding_evidence(self):
        return {
            "schema_version": 1,
            "kind": D2A.DIRECT_ONBOARDING_EVIDENCE_KIND,
            "certification_class": D2A.AUTOMATED_CLASS,
            "operation": "direct-onboard",
            "observed_at": "2026-08-11T23:59:58Z",
            "run_id": "d2-20260811t211713z-c7b9d9710178",
            "manifest_sha256": "b" * 64,
            "principal_id": "discord:1056857223529250906",
            "guild_id": "1536845588954353676",
            "discord_application_id": "1533144492293754900",
            "hub_channel_id": "1536845619266846792",
            "binding_key": "community_hub",
            "installation_id": "installation:starring-d2-test",
            "outcome": "fresh",
            "provisioner_sha256": "5" * 64,
            "issuer_sha256": "e" * 64,
            "issuer_source_sha256": "4" * 64,
            "discord_hub_preflight": True,
            "direct_auth_used": True,
            "session_revoked": True,
            "release_eligible": False,
        }

    def direct_onboarding_binding(self):
        evidence = self.direct_onboarding_evidence()
        return {
            field: evidence[field]
            for field in D2A.DIRECT_ONBOARDING_BINDING_FIELDS
        }

    def test_direct_onboarding_evidence_is_exact_and_fully_bound(self):
        evidence = self.direct_onboarding_evidence()
        self.assertEqual(len(evidence), 21)
        self.assertEqual(
            D2A.validate_direct_onboarding_evidence(
                evidence,
                self.direct_onboarding_binding(),
            ),
            evidence,
        )
        for field, replacement in (
            ("issuer_sha256", "0" * 64),
            ("issuer_source_sha256", "1" * 64),
            ("provisioner_sha256", "2" * 64),
            ("manifest_sha256", "3" * 64),
            ("release_eligible", True),
            ("session_revoked", False),
            ("extra", "forbidden"),
        ):
            forged = dict(evidence)
            forged[field] = replacement
            with self.subTest(field=field), self.assertRaises(D2A.D2AError):
                D2A.validate_direct_onboarding_evidence(
                    forged,
                    self.direct_onboarding_binding(),
                )
        missing = dict(evidence)
        missing.pop("hub_channel_id")
        with self.assertRaises(D2A.D2AError):
            D2A.validate_direct_onboarding_evidence(
                missing,
                self.direct_onboarding_binding(),
            )

    def test_controller_dict_literals_have_no_duplicate_string_keys(self):
        tree = ast.parse(MODULE_PATH.read_text(encoding="utf-8"))
        for node in ast.walk(tree):
            if not isinstance(node, ast.Dict):
                continue
            keys = [
                key.value
                for key in node.keys
                if isinstance(key, ast.Constant) and isinstance(key.value, str)
            ]
            self.assertEqual(
                len(keys),
                len(set(keys)),
                f"duplicate dict key at line {node.lineno}",
            )

    def test_issuer_command_error_is_closed_canonical_and_coarsened(self):
        expected = {
            "authoring_flow_failed": "d2a_one_shot_authoring_flow_failed",
            "confirmation_flow_failed": "d2a_one_shot_confirmation_flow_failed",
            "convergence_flow_failed": "d2a_one_shot_convergence_flow_failed",
            "decision_flow_failed": "d2a_one_shot_decision_flow_failed",
            "dependency_timeout": "d2a_one_shot_dependency_timeout",
            "dependency_unavailable": "d2a_one_shot_dependency_unavailable",
            "deployment_flow_failed": "d2a_one_shot_deployment_flow_failed",
            "product_request_failed": "d2a_one_shot_product_request_failed",
        }
        self.assertEqual(D2A.ISSUER_COMMAND_ERROR_CODES, frozenset(expected))
        self.assertEqual(D2A.ONE_SHOT_COMMAND_ERROR_CODES, expected)
        for issuer_code, command_code in expected.items():
            envelope = {
                "error_code": issuer_code,
                "kind": D2A.ISSUER_COMMAND_ERROR_KIND,
                "operation": "one-shot",
                "schema_version": 1,
            }
            completed = subprocess.CompletedProcess(
                ["issuer"],
                1,
                stdout=(D2A.canonical_json(envelope) + "\n").encode("ascii"),
                stderr=b"",
            )
            with self.subTest(issuer_code=issuer_code):
                self.assertEqual(
                    D2A.parse_issuer_command_error(completed, "one-shot"),
                    command_code,
                )
                payload = D2A.command_error_payload("one-shot", command_code)
                self.assertEqual(
                    (D2A.canonical_json(payload) + "\n").encode("ascii"),
                    (
                        "{\"error_code\":\""
                        + command_code
                        + "\",\"kind\":\"starring.d2a.command-error.v1\","
                        "\"operation\":\"one-shot\",\"schema_version\":1}\n"
                    ).encode("ascii"),
                )

        valid = {
            "error_code": "dependency_timeout",
            "kind": D2A.ISSUER_COMMAND_ERROR_KIND,
            "operation": "one-shot",
            "schema_version": 1,
        }
        canonical = (D2A.canonical_json(valid) + "\n").encode("ascii")
        malformed = [
            (canonical, b"untrusted"),
            (canonical, b"", 2),
            (canonical, b"", True),
            (canonical[:-1], b""),
            (b" " + canonical, b""),
            (
                b'{"error_code":"dependency_timeout","error_code":"dependency_unavailable",'
                b'"kind":"starring.d2a.issuer-command-error.v1","operation":"one-shot",'
                b'"schema_version":1}\n',
                b"",
            ),
            (
                b'{"error_code":"product_request_504_dependency_timeout",'
                b'"kind":"starring.d2a.issuer-command-error.v1","operation":"one-shot",'
                b'"schema_version":1}\n',
                b"",
            ),
            (
                b'{"error_code":"scenario_confirmation_mismatch",'
                b'"kind":"starring.d2a.issuer-command-error.v1","operation":"one-shot",'
                b'"schema_version":1}\n',
                b"",
            ),
            (
                b'{"error_code":"dependency_timeout","extra":"secret",'
                b'"kind":"starring.d2a.issuer-command-error.v1","operation":"one-shot",'
                b'"schema_version":1}\n',
                b"",
            ),
            ("비밀".encode("utf-8"), b""),
            (b"x" * (D2A.MAX_COMMAND_ERROR_BYTES + 1), b""),
        ]
        for item in malformed:
            stdout, stderr, *returncode = item
            completed = subprocess.CompletedProcess(
                ["issuer"],
                returncode[0] if returncode else 1,
                stdout=stdout,
                stderr=stderr,
            )
            with self.subTest(stdout=stdout[:80], returncode=completed.returncode):
                self.assertIsNone(
                    D2A.parse_issuer_command_error(completed, "one-shot")
                )
        completed = subprocess.CompletedProcess(
            ["issuer"], 1, stdout=canonical, stderr=b""
        )
        self.assertIsNone(
            D2A.parse_issuer_command_error(completed, "auth-smoke")
        )

    def test_failure_diagnostic_is_trusted_only_after_all_tool_rehashes(self):
        source = inspect.getsource(D2A.execute)
        supervised = source.index("completed = supervise_issuer(command)")
        rehashes = [
            source.index('sha256_file(issuer, "issuer", 512 * 1024 * 1024)', supervised),
            source.index('sha256_file(node, "node", 512 * 1024 * 1024)', supervised),
            source.index("issuer_source_sha256(tool_root)", supervised),
            source.index('sha256_file(runner, "runner", 4 * 1024 * 1024)', supervised),
            source.index('sha256_file(product_driver, "product_driver", 4 * 1024 * 1024)', supervised),
            source.index('sha256_file(expected_scenario, "trusted_scenario", 49_152)', supervised),
            source.index('sha256_file(pathlib.Path(__file__), "controller", 4 * 1024 * 1024)', supervised),
        ]
        diagnostic = source.index("parse_issuer_command_error(completed, operation)")
        self.assertTrue(all(supervised < rehash < diagnostic for rehash in rehashes))

    def test_command_error_cli_uses_stdout_only_and_exit_one(self):
        arguments = type("Arguments", (), {"command": "run"})()
        parser = mock.Mock()
        parser.parse_args.return_value = arguments
        stdout = StringIO()
        stderr = StringIO()
        with mock.patch.object(D2A, "parser", return_value=parser), mock.patch.object(
            D2A,
            "execute",
            side_effect=D2A.D2ACommandError(
                "one-shot", "d2a_one_shot_dependency_timeout"
            ),
        ), redirect_stdout(stdout), redirect_stderr(stderr), self.assertRaises(
            SystemExit
        ) as raised:
            D2A.main()
        self.assertEqual(raised.exception.code, 1)
        self.assertEqual(
            stdout.getvalue(),
            '{"error_code":"d2a_one_shot_dependency_timeout",'
            '"kind":"starring.d2a.command-error.v1","operation":"one-shot",'
            '"schema_version":1}\n',
        )
        self.assertEqual(stderr.getvalue(), "")

    def test_direct_onboarding_file_is_required_private_single_link_and_canonical(self):
        evidence = self.direct_onboarding_evidence()
        payload = (D2A.canonical_json(evidence) + "\n").encode()
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            with self.assertRaises(D2A.D2AError):
                D2A.load_direct_onboarding_evidence(
                    root,
                    self.direct_onboarding_binding(),
                )
            path = root / D2A.DIRECT_ONBOARDING_EVIDENCE_NAME
            path.write_bytes(payload)
            path.chmod(0o600)
            loaded = D2A.load_direct_onboarding_evidence(
                root,
                self.direct_onboarding_binding(),
            )
            self.assertEqual(loaded[1], evidence)
            self.assertEqual(loaded[2], payload)
            path.chmod(0o644)
            with self.assertRaises(D2A.D2AError):
                D2A.load_direct_onboarding_evidence(
                    root,
                    self.direct_onboarding_binding(),
                )
            path.chmod(0o600)
            link = root / "onboarding-hardlink.json"
            path.link_to(link)
            with self.assertRaises(D2A.D2AError):
                D2A.load_direct_onboarding_evidence(
                    root,
                    self.direct_onboarding_binding(),
                )
            link.unlink()
            path.write_bytes(json.dumps(evidence).encode())
            path.chmod(0o600)
            with self.assertRaises(D2A.D2AError):
                D2A.load_direct_onboarding_evidence(
                    root,
                    self.direct_onboarding_binding(),
                )

    def test_commercial_onboarding_artifacts_are_always_rejected(self):
        for relative in D2A.COMMERCIAL_ONBOARDING_ARTIFACTS:
            with self.subTest(relative=str(relative)), tempfile.TemporaryDirectory() as directory:
                root = pathlib.Path(directory)
                path = root / relative
                path.parent.mkdir(parents=True)
                path.write_text("{}\n")
                with self.assertRaisesRegex(
                    D2A.D2AError,
                    "commercial_onboarding_artifact_present",
                ):
                    D2A.reject_commercial_onboarding_artifacts(root)

    def test_manifest_loading_rejects_commercial_onboarding_evidence(self):
        run_id = "d2-20260811t211713z-c7b9d9710178"
        suffix = run_id.rsplit("-", 1)[1]
        resource_prefix = f"starring-d2-20260811-{suffix}"
        with tempfile.TemporaryDirectory() as directory:
            run_root = pathlib.Path(directory) / run_id
            run_root.mkdir(mode=0o700)
            manifest_path = run_root / "manifest.json"
            manifest = {
                "schema_version": 1,
                "certification_class": D2A.COMMERCIAL_CLASS,
                "run_id": run_id,
                "created_at": "2026-08-11T21:17:13Z",
                "commit_sha": "a" * 40,
                "authoring": {},
                "public_origin": D2A.D2_PUBLIC_ORIGIN,
                "cloudflare": {},
                "candidates": {
                    name: {
                        "path": f"/private/tmp/candidates/{name}",
                        "sha256": "c" * 64,
                    }
                    for name in D2A.CANDIDATE_KEYS
                },
                "source_trees": {},
                "database": {
                    "name": "starring_runtime_staging",
                    "cluster_root": f"/private/tmp/starring-d2-{run_id}/postgres",
                    "socket_directory": f"/private/tmp/starring-d2-{run_id}/socket",
                    "port": 55433,
                },
                "discord": {
                    "actor_id": "1056857223529250906",
                    "guild_id": "1536845588954353676",
                    "application_id": "1533144492293754900",
                    "bot_user_id": "1533144492293754900",
                    "hub_channel_id": "1536845619266846792",
                    "resource_prefix": resource_prefix,
                    "disposable_guild_required": True,
                },
                "services": {},
                "keychain_services": {"api": f"starring.d2.{suffix}.api"},
                "external_keychain": {},
                "protected_staging": {},
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
            manifest_path.write_text(D2A.canonical_json(manifest) + "\n")
            manifest_path.chmod(0o600)
            digest = hashlib.sha256(
                D2A.canonical_json(manifest).encode()
            ).hexdigest()
            digest_path = run_root / "manifest.sha256"
            digest_path.write_text(digest + "\n")
            digest_path.chmod(0o600)
            orchestrator = run_root / "orchestrator"
            orchestrator.mkdir()
            state_path = orchestrator / "state.json"
            state_path.write_text('{"phase":"candidate_started"}\n')
            state_path.chmod(0o600)
            loaded = D2A.load_manifest(str(manifest_path))
            self.assertEqual(loaded[2], digest)
            self.assertEqual(
                loaded[3],
                f"installation:{resource_prefix}",
            )
            self.assertEqual(
                list(D2A.D2_HUMAN_BOUNDARIES),
                [
                    "create_disposable_discord_guild",
                    "complete_discord_oauth",
                    "confirm_product_preview",
                    "execute_real_discord_interactions",
                    "confirm_replacement_preview",
                    "delete_disposable_discord_guild",
                ],
            )
            removed_boundary = json.loads(json.dumps(manifest))
            removed_boundary["human_boundaries"].remove(
                "confirm_replacement_preview"
            )
            manifest_path.write_text(
                D2A.canonical_json(removed_boundary) + "\n",
                encoding="utf-8",
            )
            digest_path.write_text(
                hashlib.sha256(
                    D2A.canonical_json(removed_boundary).encode()
                ).hexdigest()
                + "\n",
                encoding="utf-8",
            )
            with self.assertRaisesRegex(
                D2A.D2AError,
                "manifest_human_boundaries_invalid",
            ):
                D2A.load_manifest(str(manifest_path))
            manifest_path.write_text(
                D2A.canonical_json(manifest) + "\n",
                encoding="utf-8",
            )
            digest_path.write_text(digest + "\n", encoding="utf-8")
            for relative in D2A.COMMERCIAL_ONBOARDING_ARTIFACTS:
                with self.subTest(relative=str(relative)):
                    artifact = run_root / relative
                    artifact.parent.mkdir(parents=True, exist_ok=True)
                    artifact.write_text("{}\n")
                    with self.assertRaisesRegex(
                        D2A.D2AError,
                        "commercial_onboarding_artifact_present",
                    ):
                        D2A.load_manifest(str(manifest_path))
                    artifact.unlink()

    def one_shot_evidence(self):
        evidence = self.evidence()
        evidence.pop("me_status")
        evidence.pop("authority_check_status")
        evidence.update(
            {
                "kind": "starring.d2a.one-shot-product-evidence.v1",
                "operation": "one-shot",
                "observed_at": "2026-08-12T00:00:12Z",
                "scenario_sha256": "6" * 64,
                "authoring_http_status": 201,
                "promotion_http_status": 201,
                "preview_http_status": 200,
                "approval_http_status": 201,
                "apply_http_status": 202,
                "authoring_session_id": "d2a-study-room-v1-0123456789abcdef",
                "authoring_generation": 1,
                "promotion_id": "5" * 64,
                "candidate_ruleset_hash": "2" * 64,
                "target_content_hash": "4" * 64,
                "payload_digest": "3" * 64,
                "preview_state": "pending_approval",
                "approval_state": "approved",
                "apply_state": "runtime_pending",
                "apply_attempts": 1,
                "runtime_drain_observed": False,
                "runtime_pending_observed": True,
                "apply_resumed_after_conflict": False,
                "apply_status_observations": 0,
                "summary": {
                    "panels": 1,
                    "modals": 1,
                    "rules": 4,
                    "actions": 15,
                    "target_version": 1,
                    "required_approvals": 1,
                },
                "live_observed_at": "2026-08-12T00:00:12Z",
                "deployment_http_status": 200,
                "operational_http_status": 200,
                "live_attempts": 1,
                "pending_observed": True,
                "live_observed": True,
                "product_state": "live",
                "operational_state": "live",
                "runtime_phase": "live",
                "serving_state": "fresh",
                "deployment_observed_at": "2026-08-12T00:00:05Z",
                "deployment_attestation_revision": 11,
                "deployment_last_heartbeat_at": "2026-08-12T00:00:00Z",
                "deployment_lease_expires_at": "2026-08-12T00:00:45Z",
                "decision_observed_at": "2026-08-12T00:00:08Z",
                "runtime_observed_at": "2026-08-12T00:00:10Z",
                "current_attempt": 1,
                "attestation_revision": 11,
                "convergence_attempt": 1,
                "process_instance_id": "0123456789abcdef0123456789abcdef",
                "last_heartbeat_at": "2026-08-12T00:00:00Z",
                "lease_expires_at": "2026-08-12T00:00:45Z",
            }
        )
        return evidence

    def one_shot_binding(self):
        evidence = self.one_shot_evidence()
        return {
            **self.binding(),
            "scenario_sha256": evidence["scenario_sha256"],
            "authoring_session_id": evidence["authoring_session_id"],
        }

    def test_browser_kind_and_secret_fields_are_rejected(self):
        forged = self.evidence()
        forged["kind"] = "starring.d2.browser-authentication-evidence.v1"
        with self.assertRaises(D2A.D2AError):
            D2A.validate_public_evidence(forged, "auth-smoke", self.binding())
        for key, value in (
            ("session", "opaque"),
            ("csrf_token", "opaque"),
            ("details", "postgresql://user:password@127.0.0.1/test"),
            ("headers", "Cookie: __Host-starring_session=opaque"),
        ):
            forged = self.evidence()
            forged[key] = value
            with self.assertRaises(D2A.D2AError):
                D2A.validate_public_evidence(forged, "auth-smoke", self.binding())

    def test_evidence_is_bound_to_the_exact_run_and_identity(self):
        for field, replacement in (
            ("run_id", "d2-20260812t000000z-aaaaaaaaaaaa"),
            ("manifest_sha256", "0" * 64),
            ("principal_id", "discord:1"),
            ("guild_id", "1"),
            ("installation_id", "installation:starring-d2-forged"),
            ("public_origin", "https://example.invalid"),
        ):
            forged = self.evidence()
            forged[field] = replacement
            with self.subTest(field=field), self.assertRaises(D2A.D2AError):
                D2A.validate_public_evidence(forged, "auth-smoke", self.binding())

    def test_scenario_schema_is_exact_and_confirmation_policy_is_nonempty(self):
        valid = {
            "schema_version": 1,
            "kind": "starring.d2a.product-scenario.v1",
            "session_id_prefix": "d2a-study-room-v1",
            "message": "Create a private study room automation",
            "expected_generation": 0,
            "expected_summary": {
                "panels": 1,
                "modals": 1,
                "rules": 4,
                "actions": 15,
                "target_version": 1,
                "required_approvals": 1,
            },
        }
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "scenario.json"
            for label, mutation in (
                ("valid", lambda value: None),
                ("extra", lambda value: value.__setitem__("approve_any", True)),
                (
                    "weak_summary",
                    lambda value: value.__setitem__("expected_summary", {}),
                ),
            ):
                candidate = json.loads(json.dumps(valid))
                mutation(candidate)
                path.write_text(D2A.canonical_json(candidate) + "\n")
                path.chmod(0o644)
                if label == "valid":
                    self.assertEqual(D2A.load_scenario(path), valid)
                else:
                    with self.assertRaises(D2A.D2AError):
                        D2A.load_scenario(path)

    def test_one_shot_requires_terminal_fresh_live_projection(self):
        evidence = self.one_shot_evidence()
        D2A.validate_public_evidence(
            evidence,
            "one-shot",
            self.one_shot_binding(),
            "d2a-study-room-v1",
        )
        for field, replacement in (
            ("live_observed", False),
            ("product_state", "pending"),
            ("operational_state", "pending"),
            ("runtime_phase", "starting"),
            ("serving_state", "stale"),
            ("deployment_http_status", 503),
            ("operational_http_status", 503),
            ("process_instance_id", "0" * 31),
            ("lease_expires_at", "2026-08-12T00:00:09Z"),
        ):
            forged = self.one_shot_evidence()
            forged[field] = replacement
            with self.subTest(field=field), self.assertRaises(D2A.D2AError):
                D2A.validate_public_evidence(
                    forged,
                    "one-shot",
                    self.one_shot_binding(),
                    "d2a-study-room-v1",
                )

    def test_resolved_authoring_session_is_invocation_scoped(self):
        prefix = "d2a-study-room-v1"
        self.assertTrue(
            D2A.valid_authoring_session_id(
                f"{prefix}-0123456789abcdef", prefix
            )
        )
        self.assertTrue(
            D2A.valid_authoring_session_id(
                f"{prefix}-fedcba9876543210", prefix
            )
        )
        for value in (
            prefix,
            f"{prefix}-0123456789abcde",
            f"{prefix}-0123456789ABCDEf",
            "other-prefix-0123456789abcdef",
        ):
            with self.subTest(value=value):
                self.assertFalse(D2A.valid_authoring_session_id(value, prefix))

    def test_taint_is_exact_and_bound_to_the_official_tools(self):
        taint = {
            "schema_version": 1,
            "kind": "starring.d2a.run-taint.v1",
            "run_id": "d2-20260811t211713z-c7b9d9710178",
            "manifest_sha256": "b" * 64,
            "certification_class": "automated_maintenance_v1",
            "direct_auth_used": True,
            "release_eligible": False,
            "issuer_sha256": "e" * 64,
            "issuer_source_sha256": "4" * 64,
            "runner_sha256": "f" * 64,
            "product_driver_sha256": "2" * 64,
            "scenario_sha256": "3" * 64,
        }
        binding = {
            key: taint[key]
            for key in (
                "run_id",
                "manifest_sha256",
                "issuer_sha256",
                "issuer_source_sha256",
                "runner_sha256",
                "product_driver_sha256",
                "scenario_sha256",
            )
        }
        self.assertEqual(D2A.validate_taint(taint, binding), taint)
        for field, replacement in (
            ("release_eligible", True),
            ("direct_auth_used", False),
            ("issuer_sha256", "0" * 64),
            ("extra", "field"),
        ):
            forged = dict(taint)
            forged[field] = replacement
            with self.subTest(field=field), self.assertRaises(D2A.D2AError):
                D2A.validate_taint(forged, binding)

    def test_early_taint_payload_matches_the_rust_marker_field_order(self):
        marker, payload = D2A.build_taint_marker(
            "d2-20260811t211713z-c7b9d9710178",
            "b" * 64,
            "e" * 64,
            "4" * 64,
            "f" * 64,
            "2" * 64,
            "3" * 64,
        )
        self.assertEqual(
            list(marker),
            [
                "schema_version",
                "kind",
                "run_id",
                "manifest_sha256",
                "certification_class",
                "direct_auth_used",
                "release_eligible",
                "issuer_sha256",
                "issuer_source_sha256",
                "runner_sha256",
                "product_driver_sha256",
                "scenario_sha256",
            ],
        )
        self.assertEqual(json.loads(payload), marker)
        self.assertTrue(payload.endswith(b"\n"))

    def test_verify_binds_evidence_digest_and_coverage_gaps(self):
        evidence = self.evidence()
        digest = hashlib.sha256(D2A.canonical_json(evidence).encode()).hexdigest()
        onboarding = self.direct_onboarding_evidence()
        onboarding_payload = (D2A.canonical_json(onboarding) + "\n").encode()
        taint = {
            "schema_version": 1,
            "kind": "starring.d2a.run-taint.v1",
            "run_id": "d2-20260811t211713z-c7b9d9710178",
            "manifest_sha256": "b" * 64,
            "certification_class": "automated_maintenance_v1",
            "direct_auth_used": True,
            "release_eligible": False,
            "issuer_sha256": "e" * 64,
            "issuer_source_sha256": "4" * 64,
            "runner_sha256": "f" * 64,
            "product_driver_sha256": "2" * 64,
            "scenario_sha256": "3" * 64,
        }
        taint_payload = (D2A.canonical_json(taint) + "\n").encode()
        record = {
            "schema_version": 1,
            "kind": "starring.d2a.automated-final-record.v1",
            "certification_class": "automated_maintenance_v1",
            "status": "passed",
            "release_eligible": False,
            "direct_auth_used": True,
            "run_id": "d2-20260811t211713z-c7b9d9710178",
            "source_commit": "a" * 40,
            "d2_manifest_sha256": "b" * 64,
            "candidate_api_sha256": "c" * 64,
            "candidate_node_sha256": "1" * 64,
            "candidate_sealed_provisioner_sha256": "5" * 64,
            "controller_sha256": "d" * 64,
            "issuer_sha256": "e" * 64,
            "issuer_source_sha256": "4" * 64,
            "runner_sha256": "f" * 64,
            "product_driver_sha256": "2" * 64,
            "trusted_scenario_sha256": "3" * 64,
            "d2a_taint_sha256": hashlib.sha256(taint_payload).hexdigest(),
            "direct_onboarding_evidence_sha256": hashlib.sha256(
                onboarding_payload
            ).hexdigest(),
            "scenario_sha256": None,
            "expected_summary": None,
            "scenario_session_id_prefix": None,
            "authoring_session_id": None,
            "installation_id": "installation:starring-d2-test",
            "principal_id": "discord:1056857223529250906",
            "guild_id": "1536845588954353676",
            "discord_application_id": "1533144492293754900",
            "hub_channel_id": "1536845619266846792",
            "public_origin": "https://d2-api.starring.co.kr",
            "operation": "auth-smoke",
            "evidence_sha256": digest,
            "completed_at": "2026-08-12T00:00:00Z",
            "uncovered_release_boundaries": D2A.UNCOVERED_RELEASE_BOUNDARIES,
        }
        self.assertEqual(set(record), D2A.FINAL_RECORD_FIELDS)
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            evidence_path = root / "evidence.json"
            record_path = root / "final.json"
            taint_path = root / "d2a-taint.json"
            onboarding_path = root / D2A.DIRECT_ONBOARDING_EVIDENCE_NAME
            evidence_path.write_text(D2A.canonical_json(evidence) + "\n")
            record_path.write_text(D2A.canonical_json(record) + "\n")
            taint_path.write_bytes(taint_payload)
            onboarding_path.write_bytes(onboarding_payload)
            evidence_path.chmod(0o600)
            record_path.chmod(0o600)
            taint_path.chmod(0o600)
            onboarding_path.chmod(0o600)
            arguments = type("Arguments", (), {"record": str(record_path)})()
            with redirect_stdout(StringIO()):
                D2A.verify(arguments)
            onboarding_path.unlink()
            with self.assertRaises(D2A.D2AError):
                with redirect_stdout(StringIO()):
                    D2A.verify(arguments)
            onboarding_path.write_bytes(onboarding_payload)
            onboarding_path.chmod(0o600)
            onboarding["issuer_source_sha256"] = "0" * 64
            onboarding_path.write_text(D2A.canonical_json(onboarding) + "\n")
            onboarding_path.chmod(0o600)
            with self.assertRaises(D2A.D2AError):
                with redirect_stdout(StringIO()):
                    D2A.verify(arguments)
            onboarding = self.direct_onboarding_evidence()
            onboarding_path.write_bytes(onboarding_payload)
            onboarding_path.chmod(0o600)
            taint["release_eligible"] = True
            taint_path.write_text(D2A.canonical_json(taint) + "\n")
            taint_path.chmod(0o600)
            with self.assertRaises(D2A.D2AError):
                with redirect_stdout(StringIO()):
                    D2A.verify(arguments)
            taint["release_eligible"] = False
            taint_path.write_bytes(taint_payload)
            taint_path.chmod(0o600)
            record["completed_at"] = "2026-08-11T23:59:59Z"
            record_path.write_text(D2A.canonical_json(record) + "\n")
            record_path.chmod(0o600)
            with self.assertRaises(D2A.D2AError):
                with redirect_stdout(StringIO()):
                    D2A.verify(arguments)
            record["completed_at"] = "2026-08-12T00:00:00Z"
            record_path.write_text(D2A.canonical_json(record) + "\n")
            record_path.chmod(0o600)
            evidence["me_status"] = 500
            evidence_path.write_text(D2A.canonical_json(evidence) + "\n")
            evidence_path.chmod(0o600)
            with self.assertRaises(D2A.D2AError):
                with redirect_stdout(StringIO()):
                    D2A.verify(arguments)


if __name__ == "__main__":
    unittest.main()
