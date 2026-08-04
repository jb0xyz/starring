import json
import os
import pathlib
import tempfile
import types
import unittest

import d2_run
import d2_source_contract
import test_d2_certification
from d2_orchestrator_contract import OrchestratorError


OBSERVED_AT = "2026-08-04T12:00:00Z"


class D2SourceContractTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temporary.name).resolve()
        self.artifact_directory = self.root / "orchestrator"
        self.artifact_directory.mkdir(mode=0o700)
        self.manifest = {
            "run_id": "d2-20260804t120000z-abcdef123456",
            "discord": {
                "resource_prefix": "starring-d2-abcdef123456",
                "actor_id": "1056857223529250906",
                "guild_id": "1524810437118525551",
                "application_id": "1524810437118525552",
                "hub_channel_id": "1524810437118525554",
            },
        }
        self.context = types.SimpleNamespace(
            artifact_directory=self.artifact_directory,
            digest="a" * 64,
            manifest=self.manifest,
        )

    def tearDown(self):
        self.temporary.cleanup()

    def evidence(self, step):
        return {
            field: f"value-{field}"
            for field in d2_source_contract.STEP_SPECS[step].required
        }

    def test_direct_source_is_private_and_exactly_replayable(self):
        evidence = self.evidence(1)
        path = d2_source_contract.publish_bootstrap_source(
            self.context, evidence, OBSERVED_AT
        )
        replay = d2_source_contract.publish_bootstrap_source(
            self.context, evidence, OBSERVED_AT
        )
        self.assertEqual(path, replay)
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        self.assertEqual(path.parent.stat().st_mode & 0o777, 0o700)
        value = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(value["kind"], d2_source_contract.BOOTSTRAP_KIND)
        self.assertEqual(value["manifest_sha256"], self.context.digest)
        self.assertEqual(value["run_id"], self.manifest["run_id"])
        self.assertEqual(value["evidence"], evidence)
        changed = dict(evidence)
        changed["migration_count"] = "changed"
        with self.assertRaisesRegex(
            OrchestratorError, "coordinator_source_replay_drift"
        ):
            d2_source_contract.publish_bootstrap_source(
                self.context, changed, OBSERVED_AT
            )

    def test_direct_source_replay_retains_first_observation(self):
        evidence = self.evidence(1)
        path = d2_source_contract.publish_bootstrap_source(
            self.context, evidence, OBSERVED_AT
        )
        replay = d2_source_contract.publish_bootstrap_source(
            self.context, evidence, "2026-08-04T12:00:01Z"
        )
        self.assertEqual(path, replay)
        value = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(value["observed_at"], OBSERVED_AT)

    def test_source_directory_symlink_is_rejected_without_external_write(self):
        external = self.root / "external"
        external.mkdir()
        directory = d2_source_contract.source_directory(self.context)
        directory.symlink_to(external, target_is_directory=True)
        with self.assertRaisesRegex(
            OrchestratorError, "coordinator_source_directory_invalid"
        ):
            d2_source_contract.publish_candidate_source(
                self.context, self.evidence(3), OBSERVED_AT
            )
        self.assertEqual(list(external.iterdir()), [])

    def test_artifact_directory_symlink_is_rejected_without_external_write(self):
        self.artifact_directory.rmdir()
        external = self.root / "external-parent"
        external.mkdir()
        self.artifact_directory.symlink_to(external, target_is_directory=True)
        with self.assertRaisesRegex(
            OrchestratorError, "coordinator_source_directory_parent_invalid"
        ):
            d2_source_contract.publish_bootstrap_source(
                self.context, self.evidence(1), OBSERVED_AT
            )
        self.assertEqual(list(external.iterdir()), [])

    def test_preflight_source_binds_manifest_and_extracts_zero_counts(self):
        preflight = {
            "schema_version": 1,
            "kind": d2_source_contract.PREFLIGHT_KIND,
            "observed_at": OBSERVED_AT,
            "manifest_sha256": self.context.digest,
            "prior_runtime_owner_count": 0,
            "prior_smoke_process_count": 0,
            "standing_snapshot_sha256": "b" * 64,
            "external_credential_count": 3,
        }
        path = d2_source_contract.publish_prior_absence_source(
            self.context, preflight
        )
        value = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(value["kind"], d2_source_contract.PRIOR_ABSENCE_KIND)
        self.assertEqual(
            value["evidence"],
            {
                "prior_runtime_owner_count": 0,
                "prior_smoke_process_count": 0,
            },
        )
        preflight["manifest_sha256"] = "c" * 64
        with self.assertRaisesRegex(OrchestratorError, "preflight_source_invalid"):
            d2_source_contract.publish_prior_absence_source(
                self.context, preflight
            )

    def test_onboarding_source_is_manifest_pinned(self):
        onboarding = {
            "outcome": "fresh",
            "installation_id": "installation:starring-d2-abcdef123456",
            "principal_id": "discord:1056857223529250906",
            "guild_id": "1524810437118525551",
            "discord_application_id": "1524810437118525552",
            "binding_key": "community_hub",
            "hub_channel_id": "1524810437118525554",
        }
        path = d2_source_contract.publish_onboarding_source(
            self.context, onboarding, OBSERVED_AT
        )
        value = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(value["kind"], d2_source_contract.ONBOARDING_KIND)
        self.assertEqual(value["manifest_sha256"], self.context.digest)
        replay = d2_source_contract.publish_onboarding_source(
            self.context, onboarding, OBSERVED_AT
        )
        self.assertEqual(path, replay)
        onboarding["hub_channel_id"] = "1524810437118525555"
        with self.assertRaisesRegex(OrchestratorError, "onboarding_source_invalid"):
            d2_source_contract.publish_onboarding_source(
                self.context, onboarding, OBSERVED_AT
            )

    def test_invalid_timestamp_and_field_shape_are_rejected(self):
        with self.assertRaisesRegex(
            OrchestratorError, "coordinator_source_timestamp_invalid"
        ):
            d2_source_contract.publish_live_runtime_restart_source(
                self.context, self.evidence(11), "2026-08-04"
            )
        evidence = self.evidence(3)
        evidence.pop("transport_ready")
        with self.assertRaisesRegex(
            OrchestratorError, "coordinator_source_evidence_invalid"
        ):
            d2_source_contract.publish_candidate_source(
                self.context, evidence, OBSERVED_AT
            )

    def test_preexisting_source_symlink_is_rejected(self):
        directory = d2_source_contract.ensure_source_directory(self.context)
        target = self.root / "target.json"
        target.write_text("{}", encoding="utf-8")
        path = d2_source_contract.source_path(self.context, 1, "bootstrap")
        path.symlink_to(target)
        with self.assertRaises(OrchestratorError):
            d2_source_contract.publish_bootstrap_source(
                self.context, self.evidence(1), OBSERVED_AT
            )
        self.assertEqual(target.read_text(encoding="utf-8"), "{}")
        self.assertTrue(os.path.lexists(directory))


class D2ProducerCoordinatorIntegrationTest(unittest.TestCase):
    def setUp(self):
        self.fixture = test_d2_certification.D2CertificationTest()
        self.fixture.setUp()
        self.addCleanup(self.fixture.tearDown)
        self.manifest_path = self.fixture.prepare()
        (
            self.verified_path,
            self.manifest,
            self.digest,
        ) = d2_run.load_verified_manifest(self.manifest_path)
        self.complete = test_d2_certification.complete_evidence(self.manifest)
        self.artifact_directory = self.fixture.root / "producer-artifacts"
        self.artifact_directory.mkdir(mode=0o700)
        self.context = types.SimpleNamespace(
            artifact_directory=self.artifact_directory,
            digest=self.digest,
            manifest=self.manifest,
        )
        self.source_index = 0

    def write_source(self, value):
        self.source_index += 1
        path = self.fixture.root / f"browser-source-{self.source_index}.json"
        path.write_text(
            d2_run.canonical_json(value) + "\n", encoding="utf-8"
        )
        path.chmod(0o600)
        return path

    def advance_machine_prefix(self):
        bootstrap = d2_source_contract.publish_bootstrap_source(
            self.context, self.complete[1], OBSERVED_AT
        )
        d2_run.advance_certification(
            self.manifest_path, 1, [str(bootstrap)]
        )
        preflight = {
            "schema_version": 1,
            "kind": d2_source_contract.PREFLIGHT_KIND,
            "observed_at": OBSERVED_AT,
            "manifest_sha256": self.digest,
            "prior_runtime_owner_count": 0,
            "prior_smoke_process_count": 0,
            "standing_snapshot_sha256": "b" * 64,
            "external_credential_count": 3,
        }
        prior_absence = d2_source_contract.publish_prior_absence_source(
            self.context, preflight
        )
        d2_run.advance_certification(
            self.manifest_path, 2, [str(prior_absence)]
        )
        candidate = d2_source_contract.publish_candidate_source(
            self.context, self.complete[3], OBSERVED_AT
        )
        d2_run.advance_certification(
            self.manifest_path, 3, [str(candidate)]
        )
        return bootstrap, prior_absence, candidate

    def onboarding(self):
        discord = self.manifest["discord"]
        return {
            "outcome": "fresh",
            "installation_id": f"installation:{discord['resource_prefix']}",
            "principal_id": f"discord:{discord['actor_id']}",
            "guild_id": discord["guild_id"],
            "discord_application_id": discord["application_id"],
            "binding_key": "community_hub",
            "hub_channel_id": discord["hub_channel_id"],
        }

    def browser_authentication(self):
        evidence = self.complete[4]
        return {
            "schema_version": 1,
            "kind": "starring.d2.browser-authentication-evidence.v1",
            "observed_at": OBSERVED_AT,
            **evidence,
        }

    def test_step_one_two_three_producer_sources_advance_directly(self):
        paths = self.advance_machine_prefix()
        self.assertTrue(all(path.is_file() for path in paths))
        receipts = d2_run.load_receipts(
            self.verified_path, self.manifest, self.digest
        )
        self.assertEqual([receipt["step"] for receipt in receipts], [1, 2, 3])

    def test_step_four_requires_and_accepts_strict_onboarding_source(self):
        self.advance_machine_prefix()
        browser = self.write_source(self.browser_authentication())
        onboarding = d2_source_contract.publish_onboarding_source(
            self.context, self.onboarding(), OBSERVED_AT
        )
        result = d2_run.advance_certification(
            self.manifest_path,
            4,
            [str(browser), str(onboarding)],
        )
        self.assertEqual(result["step"], 4)
        intent = json.loads(
            d2_run.coordinator_intent_path(self.manifest_path, 4).read_text(
                encoding="utf-8"
            )
        )
        source = json.loads(onboarding.read_text(encoding="utf-8"))
        expected_digest = d2_run.sha256_bytes(
            (d2_run.canonical_json(source) + "\n").encode("utf-8")
        )
        self.assertEqual(intent["sources"][1]["sha256"], expected_digest)

    def test_step_four_rejects_manifest_drift(self):
        self.advance_machine_prefix()
        browser = self.write_source(self.browser_authentication())
        onboarding = self.onboarding()
        path = d2_source_contract.publish_onboarding_source(
            self.context, onboarding, OBSERVED_AT
        )
        value = json.loads(path.read_text(encoding="utf-8"))
        value["manifest_sha256"] = "f" * 64
        path.write_text(
            d2_run.canonical_json(value) + "\n", encoding="utf-8"
        )
        path.chmod(0o600)
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "coordinator_onboarding_source_invalid",
        ):
            d2_run.advance_certification(
                self.manifest_path, 4, [str(browser), str(path)]
            )


if __name__ == "__main__":
    unittest.main()
