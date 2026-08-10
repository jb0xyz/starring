import concurrent.futures
import contextlib
import datetime
import io
import json
import os
import pathlib
import unittest
from unittest import mock

import d2_evidence
import d2_finalization
import d2_run
import test_d2_certification


OBSERVED_AT = "2026-08-04T01:02:03Z"


class D2RunCoordinatorTest(unittest.TestCase):
    def setUp(self):
        self.fixture = test_d2_certification.D2CertificationTest()
        self.fixture.setUp()
        self.addCleanup(self.fixture.tearDown)
        self.manifest_path = self.fixture.prepare()
        (
            self.verified_path,
            self.manifest,
            self.manifest_digest,
        ) = d2_run.load_verified_manifest(self.manifest_path)
        self.complete = test_d2_certification.complete_evidence(self.manifest)
        self.source_index = 0

    def write_source(self, value, mode=0o600, raw=None):
        self.source_index += 1
        path = self.fixture.root / f"source-{self.source_index}.json"
        if raw is None:
            raw = d2_run.canonical_json(value) + "\n"
        path.write_text(raw, encoding="utf-8")
        path.chmod(mode)
        return path

    def envelope(self, kind, **values):
        return {
            "schema_version": 1,
            "kind": kind,
            "observed_at": OBSERVED_AT,
            **values,
        }

    def direct_source(self, step, kind):
        return self.write_source(
            self.envelope(
                kind,
                manifest_sha256=self.manifest_digest,
                run_id=self.manifest["run_id"],
                evidence=self.complete[step],
            )
        )

    def gateway_healed_source(self, **overrides):
        values = {
            "gateway_connected": True,
            "runtime_ready_status": 200,
            "transport_gateway_partitioned": False,
            "transport_gateway_partition_events": 1,
            "transport_duplicate_armed": False,
            "transport_duplicate_claimed": False,
            "transport_indeterminate_armed": False,
            "transport_indeterminate_claimed": False,
            "transport_instance_id": test_d2_certification.TRANSPORT_INSTANCE_ID,
            "partition_operation_id": (
                "d2:0123456789abcdef:0007:partition-gateway"
            ),
            "partition_completion_sha256": "c" * 64,
            "heal_operation_id": "d2:0123456789abcdef:0008:heal-gateway",
            "heal_completion_sha256": "d" * 64,
        }
        values.update(overrides)
        return self.envelope(d2_run.GATEWAY_HEALED_KIND, **values)

    def step_nine_sources(self):
        final = self.complete[9]
        database = self.envelope(
            "starring.d2.db-interaction-evidence.v1",
            create_interaction_id=final["create_interaction_id"],
            join_interaction_id=final["join_interaction_id"],
            actor_user_id=final["actor_user_id"],
            joined_role_id=final["joined_role_id"],
            deployment_id=final["deployment_id"],
            route_identity=test_d2_certification.route_identity(
                "deployment-1",
                test_d2_certification.PROCESS_INSTANCE_OLD,
                1,
                1,
                1,
            ),
            instance_id=final["instance_id"],
            role_ids=final["role_ids"],
            channel_ids=final["channel_ids"],
            panel_message_ids=final["panel_message_ids"],
            ephemeral_count=2,
        )
        transport = self.envelope(
            "starring.d2.transport-resource-evidence.v1",
            role_ids=final["role_ids"],
            channel_ids=final["channel_ids"],
            panel_message_ids=final["panel_message_ids"],
            inventory_digest_sha256=final["inventory_digest_sha256"],
            transport_instance_id=final["transport_instance_id"],
        )
        browser = self.envelope(
            d2_run.DISCORD_INTERACTION_OBSERVATION_KIND,
            guild_id=self.manifest["discord"]["guild_id"],
            resource_prefix=self.manifest["discord"]["resource_prefix"],
            actor_user_id=final["actor_user_id"],
            create_interaction_id=final["create_interaction_id"],
            join_interaction_id=final["join_interaction_id"],
            joined_role_id=final["joined_role_id"],
            role_ids=final["role_ids"],
            channel_ids=final["channel_ids"],
            panel_message_ids=final["panel_message_ids"],
            create_response_observed=True,
            join_response_observed=True,
            private_channel_observed=True,
            role_assignment_observed=True,
            welcome_panel_observed=True,
            join_panel_observed=True,
            confirmation_surface="chrome_discord_web",
        )
        return database, transport, browser

    def step_eight_sources(self):
        final = self.complete[8]
        browser = self.envelope(
            "starring.d2.browser-live-evidence.v1",
            observed_at=final["public_observed_at"],
            public_origin=final["public_origin"],
            installation_id=final["installation_id"],
            promotion_id=final["promotion_id"],
            pending_observed=True,
            live_observed=True,
            attempts=2,
            product_state="live",
            operational_state="live",
            runtime_phase="live",
            serving_state="fresh",
            deployment_http_status=200,
            operational_http_status=200,
            deployment_observed_at="2099-08-04T01:02:05Z",
            deployment_attestation_revision=final["deployment_revision"],
            deployment_last_heartbeat_at="2099-08-04T01:02:04Z",
            deployment_lease_expires_at=final["public_lease_expires_at"],
            decision_observed_at="2099-08-04T01:02:06Z",
            runtime_observed_at="2099-08-04T01:02:07Z",
            current_attempt=final["convergence_attempt"],
            attestation_revision=final["deployment_revision"],
            convergence_attempt=final["convergence_attempt"],
            process_instance_id=final["process_instance_id"],
            last_heartbeat_at=final["public_last_heartbeat_at"],
            lease_expires_at=final["public_lease_expires_at"],
        )
        database = self.envelope(
            "starring.d2.db-live-evidence.v1",
            observed_at=final["database_observed_at"],
            installation_id=final["installation_id"],
            promotion_id=final["promotion_id"],
            deployment_id=final["deployment_id"],
            attestation_id=final["attestation_id"],
            deployment_revision=final["deployment_revision"],
            convergence_attempt=final["convergence_attempt"],
            process_instance_id=final["process_instance_id"],
            last_heartbeat_at=final["database_last_heartbeat_at"],
            lease_expires_at=final["database_lease_expires_at"],
            route_identity=test_d2_certification.route_identity(
                "deployment-1",
                test_d2_certification.PROCESS_INSTANCE_OLD,
                1,
                1,
                1,
            ),
            serving_identity={
                **test_d2_certification.serving_identity(
                    "deployment-1",
                    test_d2_certification.PROCESS_INSTANCE_OLD,
                    1,
                    1,
                ),
                "installation_id": final["installation_id"],
            },
        )
        return browser, database

    def object_digest(self, value):
        return d2_run.sha256_bytes(
            d2_run.canonical_json(value).encode("utf-8")
        )

    def protected_freeze_intent(self, receipts, completion):
        artifact_directory = self.manifest_path.parent / "orchestrator"
        artifact_directory.mkdir(mode=0o700, exist_ok=True)
        artifact_directory.chmod(0o700)
        transport_evidence = artifact_directory / "step-03-evidence.json"
        transport_evidence.write_text(
            d2_run.canonical_json(
                {
                    "transport_instance_id": self.complete[3][
                        "transport_instance_id"
                    ]
                }
            )
            + "\n",
            encoding="utf-8",
        )
        transport_evidence.chmod(0o600)
        finalization_directory = artifact_directory / "finalization"
        finalization_directory.mkdir(mode=0o700, exist_ok=True)
        finalization_directory.chmod(0o700)
        runtime = self.manifest["candidates"]["runtime"]
        runtime_service = self.manifest["services"]["runtime"]
        runtime_pid = 4242
        runtime_plist = str(
            artifact_directory
            / "launchd"
            / f"{runtime_service['label']}.plist"
        )
        runtime_binding = {
            "launchd": {
                "pid": runtime_pid,
                "program": runtime["path"],
                "plist_path": runtime_plist,
                "arguments": [runtime["path"]],
                "runs": 1,
                "state": "running",
            },
            "process": {
                "pid": runtime_pid,
                "start_time_seconds": 1_700_000_000,
                "start_time_microseconds": 1,
                "uid": os.getuid(),
                "path": runtime["path"],
                "sha256": runtime["sha256"],
                "size": 1,
                "mode": 0o555,
                "device": 1,
                "inode": 1,
                "links": 1,
            },
            "plist": {
                "path": runtime_plist,
                "sha256": "f" * 64,
                "size": 1,
                "mode": 0o600,
                "uid": os.getuid(),
                "device": 1,
                "inode": 1,
                "links": 1,
            },
            "runtime_health": {
                "schema_version": 1,
                "os_pid": runtime_pid,
                "process_instance_id": "e" * 32,
            },
        }
        certification = {
            "step15_receipt_sha256": receipts[14]["receipt_sha256"],
            "step15_completion_sha256": self.object_digest(completion),
            "step15_completed_at": completion["observed_at"],
            "reconciliation_inventory_digest_sha256": self.complete[13][
                "reconciliation_inventory_digest_sha256"
            ],
        }
        context = d2_finalization.RunContext(
            self.manifest_path, self.manifest, self.manifest_digest
        )
        operation_id = d2_finalization.effect_admission_operation_id(
            context,
            certification,
            self.complete[3]["transport_instance_id"],
            runtime_binding,
        )
        admission = {
            "schema_version": 1,
            "kind": "starring.d2.effect-admission-freeze-intent.v1",
            "recorded_at": completion["observed_at"],
            "manifest_sha256": self.manifest_digest,
            "run_id": self.manifest["run_id"],
            "transport_instance_id": self.complete[3]["transport_instance_id"],
            "certification_step15_receipt_sha256": receipts[14][
                "receipt_sha256"
            ],
            "coordinator_step15_completion_sha256": self.object_digest(completion),
            "operation_id": operation_id,
            "runtime_binding": runtime_binding,
            "initial_phase": "open",
            "initial_accepted_requests": 0,
            "initial_active_requests": 0,
            "initial_completed_requests": 0,
            "initial_uncertain_requests": 0,
            "initial_snapshot_sha256": "9" * 64,
        }
        admission_path = (
            finalization_directory / "effect-admission-freeze-intent.json"
        )
        admission_path.write_text(
            d2_run.canonical_json(admission) + "\n", encoding="utf-8"
        )
        admission_path.chmod(0o600)
        freeze = {
            "schema_version": 1,
            "kind": "starring.d2.finalization-freeze-intent.v1",
            "recorded_at": completion["observed_at"],
            "manifest_sha256": self.manifest_digest,
            "run_id": self.manifest["run_id"],
            "transport_instance_id": self.complete[3]["transport_instance_id"],
            "certification_step15_receipt_sha256": receipts[14][
                "receipt_sha256"
            ],
            "coordinator_step15_completion_sha256": self.object_digest(completion),
            "transport_quiescence_sha256": "a" * 64,
            "resource_inventory_digest_sha256": self.complete[13][
                "reconciliation_inventory_digest_sha256"
            ],
            "effect_admission_freeze_intent_sha256": self.object_digest(admission),
            "effect_admission_operation_id": operation_id,
            "effect_admission_phase": "draining",
            "effect_admission_accepted_requests": 0,
            "effect_admission_active_requests": 0,
            "effect_admission_completed_requests": 0,
            "effect_admission_uncertain_requests": 0,
            "runtime_binding": runtime_binding,
            "services_to_stop": ["tunnel", "runtime"],
            "discord_effects_frozen": True,
        }
        freeze_path = finalization_directory / "finalization-freeze-intent.json"
        freeze_path.write_text(
            d2_run.canonical_json(freeze) + "\n", encoding="utf-8"
        )
        freeze_path.chmod(0o600)
        teardown_admission = {
            "schema_version": 1,
            "kind": "starring.d2.teardown-admission-intent.v1",
            "recorded_at": completion["observed_at"],
            "manifest_sha256": self.manifest_digest,
            "run_id": self.manifest["run_id"],
            "transport_instance_id": self.complete[3]["transport_instance_id"],
            "operation_id": operation_id,
            "finalization_freeze_intent_sha256": self.object_digest(freeze),
            "effect_admission_freeze_intent_sha256": self.object_digest(admission),
            "target_phase": "teardown_delete_only",
        }
        teardown_admission_path = (
            finalization_directory / "teardown-admission-intent.json"
        )
        teardown_admission_path.write_text(
            d2_run.canonical_json(teardown_admission) + "\n", encoding="utf-8"
        )
        teardown_admission_path.chmod(0o600)
        return freeze

    def step_sixteen_sources(self):
        receipts = self.receipts()
        completion = json.loads(
            d2_run.coordinator_completion_path(self.manifest_path, 15).read_text(
                encoding="utf-8"
            )
        )
        freeze = self.protected_freeze_intent(receipts, completion)
        installation_id = self.complete[4]["installation_id"]
        channel_id = self.complete[9]["channel_ids"][0]
        resources = [
            {
                "kind": "role",
                "resource_id": self.complete[9]["role_ids"][0],
            },
            {
                "kind": "channel",
                "resource_id": channel_id,
            },
            {
                "kind": "message",
                "resource_id": self.complete[9]["panel_message_ids"][0],
                "channel_id": channel_id,
            },
            {
                "kind": "message",
                "resource_id": self.complete[9]["panel_message_ids"][1],
                "channel_id": self.manifest["discord"]["hub_channel_id"],
            },
            {
                "kind": "role",
                "resource_id": self.complete[13]["output_role_id"],
            },
        ]
        resources.sort(key=lambda value: value["resource_id"])
        resource_ids = sorted(value["resource_id"] for value in resources)
        proxy_deletions = []
        direct_observations = []
        for resource in resources:
            channel = resource.get("channel_id")
            proxy_deletions.append(
                {
                    "resource_kind": resource["kind"],
                    "resource_id": resource["resource_id"],
                    "channel_id": channel,
                    "disposition": "preexisting_deleted",
                    "http_status": None,
                    "discord_code": None,
                }
            )
            if resource["kind"] == "role":
                http_status = 200
                discord_code = None
            elif resource["kind"] == "channel":
                http_status = 404
                discord_code = 10003
            else:
                http_status = 404
                discord_code = 10008
            direct_observations.append(
                {
                    "resource_kind": resource["kind"],
                    "resource_id": resource["resource_id"],
                    "channel_id": channel,
                    "http_status": http_status,
                    "discord_code": discord_code,
                    "exists": False,
                }
            )
        database = self.envelope(
            d2_run.DB_PRECLEANUP_KIND,
            installation_id=installation_id,
            scoped_installation_count=1,
            scoped_deployment_count=2,
            terminal_product_operation_count=2,
            unresolved_product_operation_count=0,
            unresolved_receipt_count=0,
            unresolved_journal_entry_count=0,
            unresolved_rollback_count=0,
            ready_for_cleanup=True,
        )
        teardown = {
            "schema_version": 1,
            "kind": d2_run.DISCORD_TEARDOWN_KIND,
            "manifest_sha256": self.manifest_digest,
            "run_id": self.manifest["run_id"],
            "recorded_at": OBSERVED_AT,
            "transport_instance_id": self.complete[3]["transport_instance_id"],
            "source_inventory_digest_sha256": self.complete[13][
                "reconciliation_inventory_digest_sha256"
            ],
            "final_inventory_digest_sha256": "e" * 64,
            "resource_union_sha256": self.object_digest(resources),
            "created_resources": resources,
            "deleted_resources": resources,
            "active_resources": [],
            "resource_ids": resource_ids,
            "message_ids": sorted(self.complete[9]["panel_message_ids"]),
            "channel_ids": sorted(self.complete[9]["channel_ids"]),
            "role_ids": sorted(
                self.complete[9]["role_ids"]
                + [self.complete[13]["output_role_id"]]
            ),
            "proxy_deletions": proxy_deletions,
            "direct_observations": direct_observations,
            "all_resources_absent": True,
            "finalization_freeze_intent_sha256": self.object_digest(freeze),
            "certification_step15_receipt_sha256": receipts[14]["receipt_sha256"],
            "coordinator_step15_completion_sha256": self.object_digest(completion),
            "freeze_resource_inventory_digest_sha256": self.complete[13][
                "reconciliation_inventory_digest_sha256"
            ],
        }
        finalization = self.envelope(
            d2_run.ORCHESTRATOR_FINALIZATION_KIND,
            manifest_sha256=self.manifest_digest,
            run_id=self.manifest["run_id"],
            installation_id=installation_id,
            precleanup_sha256=self.object_digest(database),
            database_absence_sha256="a" * 64,
            cleanup_sha256="b" * 64,
            discord_teardown_sha256=self.object_digest(teardown),
            database_drop_requested=True,
            database_absent=True,
            services_stopped=True,
            postgres_process_absent=True,
            candidate_launchd_jobs_absent=True,
            run_keychain_items_absent=True,
            isolated_root_absent=True,
            protected_staging_unchanged=True,
            external_credentials_preserved=True,
            discord_resource_ids_deleted=resource_ids,
            discord_active_resource_count=0,
        )
        return database, teardown, finalization

    def step_seventeen_sources(self, step_sixteen):
        receipts = self.receipts()
        completion = json.loads(
            d2_run.coordinator_completion_path(self.manifest_path, 16).read_text(
                encoding="utf-8"
            )
        )
        completed_at = datetime.datetime.fromisoformat(
            completion["observed_at"].replace("Z", "+00:00")
        )
        prefix_observed_at = (completed_at + datetime.timedelta(seconds=1)).isoformat(
            timespec="seconds"
        ).replace("+00:00", "Z")
        guild_observed_at = (completed_at + datetime.timedelta(seconds=2)).isoformat(
            timespec="seconds"
        ).replace("+00:00", "Z")
        orchestration_observed_at = (
            completed_at + datetime.timedelta(seconds=3)
        ).isoformat(timespec="seconds").replace("+00:00", "Z")
        database = self.envelope(
            d2_run.DB_ABSENCE_KIND,
            run_id=self.manifest["run_id"],
            installation_id=self.complete[4]["installation_id"],
            database_absent=True,
        )
        prefix_scan = self.envelope(
            d2_run.PREFIX_SCAN_KIND,
            observed_at=prefix_observed_at,
            guild_id=self.manifest["discord"]["guild_id"],
            resource_prefix=self.manifest["discord"]["resource_prefix"],
            guild_observation_http_status=200,
            role_count=0,
            channel_count=0,
            panel_count=0,
            resource_prefix_match_count=0,
        )
        guild = self.envelope(
            d2_run.GUILD_DELETION_KIND,
            observed_at=guild_observed_at,
            guild_id=self.manifest["discord"]["guild_id"],
            deletion_confirmed=True,
            guild_observation_http_status=404,
            discord_error_code=10004,
            confirmation_surface="chrome",
        )
        orchestration = self.envelope(
            d2_run.ORCHESTRATOR_ABSENCE_KIND,
            observed_at=orchestration_observed_at,
            manifest_sha256=self.manifest_digest,
            run_id=self.manifest["run_id"],
            installation_id=self.complete[4]["installation_id"],
            guild_id=self.manifest["discord"]["guild_id"],
            resource_prefix=self.manifest["discord"]["resource_prefix"],
            precleanup_sha256=self.object_digest(step_sixteen[0]),
            database_absence_sha256=self.object_digest(database),
            cleanup_sha256="b" * 64,
            discord_teardown_sha256=self.object_digest(step_sixteen[1]),
            prefix_scan_sha256=self.object_digest(prefix_scan),
            guild_deletion_sha256=self.object_digest(guild),
            step16_receipt_sha256=receipts[15]["receipt_sha256"],
            coordinator_step16_completion_sha256=self.object_digest(completion),
            coordinator_step16_completed_at=completion["observed_at"],
            unresolved_operation_count=0,
            unresolved_receipt_count=0,
            unresolved_journal_count=0,
            route_count=0,
            instance_count=0,
            role_count=0,
            channel_count=0,
            panel_count=0,
            resource_prefix_match_count=0,
            database_absent=True,
            postgres_process_absent=True,
            launchd_jobs_absent=True,
            keychain_items_absent=True,
            discord_child_resources_absent=True,
            protected_staging_unchanged=True,
            external_credentials_preserved=True,
        )
        return database, orchestration, prefix_scan, guild

    def append_raw_receipts(self, last_step):
        for step in range(1, last_step + 1):
            d2_run.append_step_receipt(
                self.verified_path,
                self.manifest,
                self.manifest_digest,
                step,
                self.complete[step],
                OBSERVED_AT,
            )

    def append_prior(self, last_step):
        self.append_raw_receipts(last_step)
        self.write_coordinator_records(last_step=last_step)

    def receipts(self):
        return d2_run.load_receipts(
            self.verified_path, self.manifest, self.manifest_digest
        )

    def write_coordinator_records(self, last_step=17, pending_step=None):
        d2_run.ensure_coordinator_directory(self.manifest_path)
        receipts = self.receipts()
        for step in range(1, last_step + 1):
            intent = {
                "schema_version": 1,
                "kind": d2_run.COORDINATOR_INTENT_KIND,
                "run_id": self.manifest["run_id"],
                "manifest_sha256": self.manifest_digest,
                "step": step,
                "code": d2_run.STEP_SPECS[step].code,
                "observed_at": OBSERVED_AT,
                "receipt_chain_head_sha256": (
                    d2_run.ZERO_DIGEST
                    if step == 1
                    else receipts[step - 2]["receipt_sha256"]
                ),
                "sources": [
                    {
                        "kind": specification["kind"],
                        "sha256": d2_run.sha256_bytes(
                            f"{step}:{specification['kind']}".encode("utf-8")
                        ),
                    }
                    for specification in d2_run.STEP_SOURCE_SPECS[step]
                ],
            }
            intent_path = d2_run.coordinator_intent_path(self.manifest_path, step)
            if d2_run.path_present(intent_path):
                intent = json.loads(intent_path.read_text(encoding="utf-8"))
            else:
                d2_run.write_private_json(intent_path, intent)
            if step == pending_step:
                continue
            completion = {
                "schema_version": 1,
                "kind": d2_run.COORDINATOR_COMPLETION_KIND,
                "run_id": self.manifest["run_id"],
                "manifest_sha256": self.manifest_digest,
                "step": step,
                "code": d2_run.STEP_SPECS[step].code,
                "observed_at": OBSERVED_AT,
                "intent_sha256": d2_run.intent_digest(intent),
                "receipt_sha256": receipts[step - 1]["receipt_sha256"],
                "receipt_disposition": "created",
            }
            completion_path = d2_run.coordinator_completion_path(
                self.manifest_path, step
            )
            if not d2_run.path_present(completion_path):
                d2_run.write_private_json(completion_path, completion)

    def rewrite_record(self, path, value):
        path.write_text(
            d2_run.canonical_json(value) + "\n", encoding="utf-8"
        )
        path.chmod(0o600)

    def write_candidate_start_retirement(self):
        path = d2_run.candidate_start_retirement_path(self.manifest_path)
        path.parent.mkdir(mode=0o700, exist_ok=True)
        path.parent.chmod(0o700)
        path.write_text("retired\n", encoding="utf-8")
        path.chmod(0o600)
        return path

    def write_abort_teardown_tombstone(self):
        path = d2_run.abort_teardown_tombstone_path(self.manifest_path)
        path.parent.mkdir(mode=0o700, exist_ok=True)
        path.parent.chmod(0o700)
        path.write_text("aborted\n", encoding="utf-8")
        path.chmod(0o600)
        return path

    def test_status_rejects_candidate_start_retirement(self):
        self.write_candidate_start_retirement()
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "candidate_start_transition_retirement_required",
        ):
            d2_run.next_certification_action(self.manifest_path)

    def test_status_rejects_retirement_created_inside_coordinator_lock(self):
        original = d2_run.coordinator_pending_step

        def retire_during_status(*arguments):
            result = original(*arguments)
            self.write_candidate_start_retirement()
            return result

        with mock.patch.object(
            d2_run,
            "coordinator_pending_step",
            side_effect=retire_during_status,
        ):
            with self.assertRaisesRegex(
                d2_run.CertificationError,
                "candidate_start_transition_retirement_required",
            ):
                d2_run.next_certification_action(self.manifest_path)

    def test_advance_rejects_candidate_start_retirement(self):
        source = self.direct_source(1, d2_run.ORCHESTRATOR_BOOTSTRAP_KIND)
        self.write_candidate_start_retirement()
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "candidate_start_transition_retirement_required",
        ):
            d2_run.advance_certification(
                self.manifest_path, 1, [str(source)]
            )

    def test_verify_rejects_candidate_start_retirement(self):
        self.append_prior(17)
        self.write_candidate_start_retirement()
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "candidate_start_transition_retirement_required",
        ):
            d2_run.verify_certification(self.manifest_path)

    def test_status_rejects_abort_teardown_tombstone(self):
        self.write_abort_teardown_tombstone()
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "candidate_start_transition_retirement_required",
        ):
            d2_run.next_certification_action(self.manifest_path)

    def test_advance_rejects_abort_teardown_tombstone(self):
        source = self.direct_source(1, d2_run.ORCHESTRATOR_BOOTSTRAP_KIND)
        self.write_abort_teardown_tombstone()
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "candidate_start_transition_retirement_required",
        ):
            d2_run.advance_certification(
                self.manifest_path, 1, [str(source)]
            )

    def test_verify_rejects_abort_teardown_tombstone(self):
        self.append_prior(17)
        self.write_abort_teardown_tombstone()
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "candidate_start_transition_retirement_required",
        ):
            d2_run.verify_certification(self.manifest_path)

    def test_status_exposes_exact_source_kinds_and_execution_modes(self):
        action = d2_run.next_certification_action(self.manifest_path)
        self.assertEqual(action["step"], 1)
        self.assertEqual(
            action["required_sources"],
            [
                {
                    "kind": d2_run.ORCHESTRATOR_BOOTSTRAP_KIND,
                    "mode": "machine",
                }
            ],
        )
        self.append_prior(3)
        action = d2_run.next_certification_action(self.manifest_path)
        self.assertEqual(action["step"], 4)
        self.assertEqual(
            action["required_sources"],
            [
                {
                    "kind": "starring.d2.browser-authentication-evidence.v1",
                    "mode": "chrome",
                },
                {
                    "kind": d2_run.ORCHESTRATOR_ONBOARDING_KIND,
                    "mode": "machine",
                },
            ],
        )
        d2_run.append_step_receipt(
            self.verified_path,
            self.manifest,
            self.manifest_digest,
            4,
            self.complete[4],
            OBSERVED_AT,
        )
        self.write_coordinator_records(last_step=4)
        action = d2_run.next_certification_action(self.manifest_path)
        self.assertEqual(action["step"], 5)
        self.assertEqual(
            action["required_sources"],
            [
                {
                    "kind": "starring.d2.browser-authoring-evidence.v1",
                    "mode": "chrome",
                },
                {
                    "kind": "starring.d2.worker-authoring-evidence.v1",
                    "mode": "machine",
                },
            ],
        )
        self.assertEqual(
            d2_run.STEP_SOURCE_SPECS[17][-2],
            {"kind": d2_run.PREFIX_SCAN_KIND, "mode": "chrome"},
        )
        self.assertEqual(
            d2_run.STEP_SOURCE_SPECS[17][-1],
            {"kind": d2_run.GUILD_DELETION_KIND, "mode": "chrome"},
        )
        self.assertEqual(
            d2_run.STEP_SOURCE_SPECS[15][-1],
            {"kind": d2_run.GATEWAY_HEALED_KIND, "mode": "machine"},
        )
        self.assertEqual(
            d2_run.STEP_SOURCE_SPECS[7],
            (
                {
                    "kind": "starring.d2.browser-product-decision-evidence.v2",
                    "mode": "chrome",
                },
            ),
        )
        self.assertEqual(
            d2_run.STEP_SOURCE_SPECS[9][0],
            {
                "kind": d2_run.DISCORD_INTERACTION_OBSERVATION_KIND,
                "mode": "chrome",
            },
        )

    def test_step_seven_action_exposes_the_durable_preview_completion_challenge(self):
        self.append_prior(6)
        completion = json.loads(
            d2_run.coordinator_completion_path(self.manifest_path, 6).read_text(
                encoding="utf-8"
            )
        )
        action = d2_run.next_certification_action(self.manifest_path)
        self.assertEqual(action["step"], 7)
        self.assertEqual(
            action["preview_completion_challenge_sha256"],
            d2_run.preview_completion_challenge(
                self.manifest,
                self.manifest_digest,
                completion,
            ),
        )

    def test_direct_machine_source_advances_and_replays_without_raw_evidence(self):
        source = self.direct_source(1, d2_run.ORCHESTRATOR_BOOTSTRAP_KIND)
        created = d2_run.advance_certification(
            self.manifest_path, 1, [str(source)]
        )
        replayed = d2_run.advance_certification(
            self.manifest_path, 1, [str(source)]
        )
        self.assertEqual(created["disposition"], "created")
        self.assertEqual(replayed["disposition"], "exact_replay")
        self.assertEqual(created["receipt_sha256"], replayed["receipt_sha256"])
        self.assertEqual(len(self.receipts()), 1)
        intent_path = d2_run.coordinator_intent_path(self.manifest_path, 1)
        completion_path = d2_run.coordinator_completion_path(
            self.manifest_path, 1
        )
        self.assertEqual(stat_mode(intent_path), 0o600)
        self.assertEqual(stat_mode(completion_path), 0o600)
        intent = json.loads(intent_path.read_text(encoding="utf-8"))
        completion = json.loads(completion_path.read_text(encoding="utf-8"))
        serialized = json.dumps({"intent": intent, "completion": completion})
        self.assertNotIn("database_system_identifier", serialized)
        self.assertNotIn('"evidence":', serialized)
        self.assertEqual(
            set(intent["sources"][0]), {"kind", "sha256"}
        )

    def test_direct_machine_source_requires_current_manifest_and_run(self):
        for field, value in (
            ("manifest_sha256", "f" * 64),
            ("run_id", "d2-20260804t010203z-ffffffffffff"),
        ):
            with self.subTest(field=field):
                source = self.direct_source(
                    1, d2_run.ORCHESTRATOR_BOOTSTRAP_KIND
                )
                envelope = json.loads(source.read_text(encoding="utf-8"))
                envelope[field] = value
                self.rewrite_record(source, envelope)
                with self.assertRaisesRegex(
                    d2_run.CertificationError,
                    "coordinator_direct_source_invalid",
                ):
                    d2_run.advance_certification(
                        self.manifest_path, 1, [str(source)]
                    )

    def test_raw_final_evidence_and_wrong_next_step_are_rejected(self):
        raw = self.write_source(self.complete[1])
        with self.assertRaisesRegex(
            d2_run.CertificationError, "coordinator_source_kind_invalid"
        ):
            d2_run.advance_certification(self.manifest_path, 1, [str(raw)])
        step_three = self.direct_source(3, d2_run.ORCHESTRATOR_CANDIDATE_KIND)
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "coordinator_step_out_of_order:expected_1",
        ):
            d2_run.advance_certification(
                self.manifest_path, 3, [str(step_three)]
            )

    def test_sources_require_private_regular_nonsymlink_strict_safe_json(self):
        source = self.direct_source(1, d2_run.ORCHESTRATOR_BOOTSTRAP_KIND)
        source.chmod(0o644)
        with self.assertRaisesRegex(
            d2_run.CertificationError, "coordinator_source_1_file_invalid"
        ):
            d2_run.advance_certification(self.manifest_path, 1, [str(source)])
        source.chmod(0o600)
        link = self.fixture.root / "source-link.json"
        link.symlink_to(source)
        with self.assertRaisesRegex(
            d2_run.CertificationError, "coordinator_source_1_unavailable"
        ):
            d2_run.advance_certification(self.manifest_path, 1, [str(link)])
        unsafe = self.envelope(
            d2_run.ORCHESTRATOR_BOOTSTRAP_KIND,
            evidence=self.complete[1],
            access_token="forbidden",
        )
        unsafe_source = self.write_source(unsafe)
        with self.assertRaisesRegex(
            d2_run.CertificationError, "evidence_forbidden_key"
        ):
            d2_run.advance_certification(
                self.manifest_path, 1, [str(unsafe_source)]
            )
        duplicate_source = self.write_source(
            {},
            raw=(
                '{"schema_version":1,"kind":"'
                + d2_run.ORCHESTRATOR_BOOTSTRAP_KIND
                + '","kind":"duplicate","observed_at":"'
                + OBSERVED_AT
                + '","evidence":{}}'
            ),
        )
        with self.assertRaisesRegex(
            d2_run.CertificationError, "evidence_duplicate_key"
        ):
            d2_run.advance_certification(
                self.manifest_path, 1, [str(duplicate_source)]
            )

    def test_source_digest_drift_after_durable_intent_is_rejected(self):
        source = self.direct_source(1, d2_run.ORCHESTRATOR_BOOTSTRAP_KIND)
        with mock.patch.object(
            d2_run,
            "append_step_receipt",
            side_effect=d2_run.CertificationError("injected_append_failure"),
        ):
            with self.assertRaisesRegex(
                d2_run.CertificationError, "injected_append_failure"
            ):
                d2_run.advance_certification(
                    self.manifest_path, 1, [str(source)]
                )
        changed = dict(self.complete[1])
        changed["database_system_identifier"] = "7667905772642692044"
        source.write_text(
            d2_run.canonical_json(
                self.envelope(
                    d2_run.ORCHESTRATOR_BOOTSTRAP_KIND,
                    evidence=changed,
                )
            )
            + "\n",
            encoding="utf-8",
        )
        source.chmod(0o600)
        with self.assertRaisesRegex(
            d2_run.CertificationError, "coordinator_source_digest_drift"
        ):
            d2_run.advance_certification(
                self.manifest_path, 1, [str(source)]
            )

    def test_receipt_before_completion_resumes_as_exact_replay(self):
        source = self.direct_source(1, d2_run.ORCHESTRATOR_BOOTSTRAP_KIND)
        writer = d2_run.write_private_json

        def interrupt_completion(path, value):
            if pathlib.Path(path).name.endswith("completion.json"):
                raise d2_run.CertificationError("injected_completion_failure")
            writer(path, value)

        with mock.patch.object(
            d2_run, "write_private_json", side_effect=interrupt_completion
        ):
            with self.assertRaisesRegex(
                d2_run.CertificationError, "injected_completion_failure"
            ):
                d2_run.advance_certification(
                    self.manifest_path, 1, [str(source)]
                )
        action = d2_run.next_certification_action(self.manifest_path)
        self.assertEqual(action["status"], "resume_step")
        self.assertEqual(action["step"], 1)
        resumed = d2_run.advance_certification(
            self.manifest_path, 1, [str(source)]
        )
        self.assertEqual(resumed["disposition"], "exact_replay")
        self.assertTrue(
            d2_run.coordinator_completion_path(self.manifest_path, 1).is_file()
        )

    def test_concurrent_advance_serializes_to_one_receipt(self):
        source = self.direct_source(1, d2_run.ORCHESTRATOR_BOOTSTRAP_KIND)

        def advance():
            return d2_run.advance_certification(
                self.manifest_path, 1, [str(source)]
            )

        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
            results = list(executor.map(lambda _index: advance(), range(2)))
        self.assertEqual(
            sorted(result["disposition"] for result in results),
            ["created", "exact_replay"],
        )
        self.assertEqual(len(self.receipts()), 1)

    def test_step_five_joins_browser_and_worker_sources(self):
        self.append_prior(4)
        final = self.complete[5]
        browser = self.envelope(
            "starring.d2.browser-authoring-evidence.v1",
            public_origin=final["public_origin"],
            authoring_http_status=final["authoring_http_status"],
            authoring_session_id=final["authoring_session_id"],
            authoring_generation=final["authoring_generation"],
            expected_generation=final["expected_generation"],
            authoring_disposition=final["authoring_disposition"],
            installation_id=final["installation_id"],
            one_shot=True,
            worker_request_id=final["worker_request_id"],
            worker_completion_sha256=final["worker_completion_sha256"],
        )
        browser_digest = d2_run.sha256_bytes(
            d2_evidence.canonical_json(browser).encode("utf-8")
        )
        worker = self.envelope(
            "starring.d2.worker-authoring-evidence.v1",
            manifest_sha256=self.manifest_digest,
            browser_evidence_sha256=browser_digest,
            browser_observed_at=OBSERVED_AT,
            worker_before_observed_at="2026-08-04T01:02:02Z",
            worker_after_observed_at="2026-08-04T01:02:04Z",
            provider=final["provider"],
            model=final["model"],
            reasoning_effort=final["reasoning_effort"],
            auth_mode=final["auth_mode"],
            codex_cli_version="1.0.0",
            worker_instance_id="worker-1",
            worker_source_sha256=self.manifest["source_trees"]["codex_worker"][
                "sha256"
            ],
            accepted_requests_before=4,
            accepted_requests_after=5,
            accepted_requests_delta=1,
            settled_requests_before=4,
            settled_requests_after=5,
            settled_requests_delta=1,
            active_requests_after=0,
            queued_requests_after=0,
            worker_request_id=final["worker_request_id"],
            worker_completion_sha256=final["worker_completion_sha256"],
        )
        result = d2_run.advance_certification(
            self.manifest_path,
            5,
            [str(self.write_source(worker)), str(self.write_source(browser))],
        )
        self.assertEqual(result["disposition"], "created")
        self.assertEqual(self.receipts()[4]["evidence"], final)

    def test_step_five_rejects_worker_from_forged_manifest_or_source_tree(self):
        browser = self.envelope("starring.d2.browser-authoring-evidence.v1")
        worker = self.envelope(
            "starring.d2.worker-authoring-evidence.v1",
            manifest_sha256="f" * 64,
            worker_source_sha256=self.manifest["source_trees"]["codex_worker"][
                "sha256"
            ],
        )
        with self.assertRaisesRegex(
            d2_run.CertificationError, "coordinator_worker_binding_invalid"
        ):
            d2_run.assemble_step_evidence(
                5,
                [
                    {"kind": browser["kind"], "value": browser},
                    {"kind": worker["kind"], "value": worker},
                ],
                self.receipts(),
                self.manifest,
                self.manifest_digest,
            )
        worker["manifest_sha256"] = self.manifest_digest
        worker["worker_source_sha256"] = "f" * 64
        with self.assertRaisesRegex(
            d2_run.CertificationError, "coordinator_worker_binding_invalid"
        ):
            d2_run.assemble_step_evidence(
                5,
                [
                    {"kind": browser["kind"], "value": browser},
                    {"kind": worker["kind"], "value": worker},
                ],
                self.receipts(),
                self.manifest,
                self.manifest_digest,
            )

    def test_step_six_joins_public_preview_to_exact_encrypted_generation(self):
        self.append_prior(5)
        final = self.complete[6]
        browser = self.envelope(
            d2_run.PREVIEW_READY_KIND,
            observed_at=final["preview_observed_at"],
            public_origin=final["public_origin"],
            installation_id=final["installation_id"],
            authoring_session_id=final["authoring_session_id"],
            authoring_generation=final["generation"],
            projection_state=final["projection_state"],
            candidate_ruleset_hash=final["candidate_ruleset_hash"],
            worker_request_id=final["worker_request_id"],
            worker_completion_sha256=final["worker_completion_sha256"],
        )
        database = self.envelope(
            "starring.d2.db-authoring-evidence.v1",
            generation_encrypted=final["generation_encrypted"],
            projection_state=final["projection_state"],
            generation=final["generation"],
            generation_count=final["generation_count"],
            payload_digest=final["candidate_ruleset_hash"],
            worker_request_id=final["worker_request_id"],
            worker_completion_sha256=final["worker_completion_sha256"],
            installation_id=final["installation_id"],
            authoring_session_id=final["authoring_session_id"],
            generation_created_at=final["generation_created_at"],
        )
        result = d2_run.advance_certification(
            self.manifest_path,
            6,
            [str(self.write_source(database)), str(self.write_source(browser))],
        )
        self.assertEqual(result["disposition"], "created")
        self.assertEqual(self.receipts()[5]["evidence"], final)

    def test_step_seven_must_present_the_exact_step_six_completion_challenge(self):
        self.append_prior(6)
        final = self.complete[7]
        completion = json.loads(
            d2_run.coordinator_completion_path(self.manifest_path, 6).read_text(
                encoding="utf-8"
            )
        )
        completed_at = datetime.datetime.fromisoformat(
            completion["observed_at"][:-1] + "+00:00"
        )
        before = (completed_at - datetime.timedelta(seconds=1)).isoformat(
            timespec="microseconds"
        ).replace("+00:00", "Z")
        expected_challenge = d2_run.preview_completion_challenge(
            self.manifest,
            self.manifest_digest,
            completion,
        )
        confirmation = self.envelope(
            "starring.d2.chrome-preview-confirmation.v2",
            observed_at=before,
            confirmation_surface="chrome_confirm",
            accepted=True,
            installation_id=final["installation_id"],
            promotion_id=final["promotion_id"],
            revision=1,
            payload_digest=final["payload_digest"],
            candidate_ruleset_hash=final["candidate_ruleset_hash"],
            target_content_hash=final["target_content_hash"],
            preview_completion_challenge_sha256="f" * 64,
            decision_command_sha256=final["decision_command_sha256"],
            summary={
                "panels": 1,
                "modals": 1,
                "rules": 4,
                "actions": 15,
                "target_version": 1,
                "required_approvals": 1,
            },
        )
        source = self.envelope(
            "starring.d2.browser-product-decision-evidence.v2",
            observed_at=before,
            public_origin=final["public_origin"],
            installation_id=final["installation_id"],
            promotion_id=final["promotion_id"],
            authoring_session_id=final["authoring_session_id"],
            authoring_generation=final["authoring_generation"],
            candidate_ruleset_hash=final["candidate_ruleset_hash"],
            target_content_hash=final["target_content_hash"],
            payload_digest=final["payload_digest"],
            preview_state=final["preview_state"],
            approval_state=final["approval_state"],
            apply_state=final["apply_state"],
            runtime_pending_observed=True,
            preview_completion_challenge_sha256="f" * 64,
            decision_command_sha256=final["decision_command_sha256"],
            chrome_confirmation=confirmation,
        )
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "coordinator_preview_completion_challenge_invalid",
        ):
            d2_run.advance_certification(
                self.manifest_path, 7, [str(self.write_source(source))]
            )
        source["preview_completion_challenge_sha256"] = expected_challenge
        source["chrome_confirmation"][
            "preview_completion_challenge_sha256"
        ] = expected_challenge
        result = d2_run.advance_certification(
            self.manifest_path, 7, [str(self.write_source(source))]
        )
        self.assertEqual(result["disposition"], "created")
        self.assertEqual(
            self.receipts()[6]["evidence"]["target_content_hash"],
            final["target_content_hash"],
        )
        self.assertEqual(
            self.receipts()[6]["evidence"][
                "preview_completion_challenge_sha256"
            ],
            expected_challenge,
        )

    def test_step_eight_binds_public_live_to_exact_database_runtime_identity(self):
        self.append_prior(7)
        browser, database = self.step_eight_sources()
        result = d2_run.advance_certification(
            self.manifest_path,
            8,
            [str(self.write_source(database)), str(self.write_source(browser))],
        )
        self.assertEqual(result["disposition"], "created")
        expected = dict(self.complete[8])
        expected["serving_lease_id"] = (
            d2_evidence.canonical_serving_identity_sha256(
                database["serving_identity"]
            )
        )
        self.assertEqual(self.receipts()[7]["evidence"], expected)

    def test_step_eight_rejects_public_database_process_drift(self):
        self.append_prior(7)
        browser, database = self.step_eight_sources()
        database["process_instance_id"] = (
            test_d2_certification.PROCESS_INSTANCE_NEW
        )
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "coordinator_evidence_assembly_failed:live_public_database_runtime_identity_mismatch",
        ):
            d2_run.assemble_step_evidence(
                8,
                [
                    {"kind": browser["kind"], "value": browser},
                    {"kind": database["kind"], "value": database},
                ],
                self.receipts(),
                self.manifest,
                self.manifest_digest,
            )

    def test_step_eight_rejects_a_live_registry_target_not_reviewed_at_step_seven(self):
        self.append_prior(7)
        browser, database = self.step_eight_sources()
        database["serving_identity"]["target_content_hash"] = "f" * 64
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "step_contract_failed:deployment_identity",
        ):
            d2_run.advance_certification(
                self.manifest_path,
                8,
                [str(self.write_source(database)), str(self.write_source(browser))],
            )

    def test_step_nine_binds_visible_join_to_durable_actor_role_and_resources(self):
        self.append_prior(8)
        database, transport, browser = self.step_nine_sources()
        result = d2_run.advance_certification(
            self.manifest_path,
            9,
            [
                str(self.write_source(transport)),
                str(self.write_source(browser)),
                str(self.write_source(database)),
            ],
        )
        self.assertEqual(result["disposition"], "created")
        self.assertEqual(self.receipts()[8]["evidence"], self.complete[9])

    def test_step_nine_rejects_durable_actor_role_or_ack_drift(self):
        self.append_prior(8)
        database, transport, browser = self.step_nine_sources()
        cases = {
            "actor_user_id": "1056857223529250907",
            "joined_role_id": "1532677575736819851",
            "ephemeral_count": 1,
        }
        for field, value in cases.items():
            with self.subTest(field=field):
                changed = dict(database)
                changed[field] = value
                with self.assertRaisesRegex(
                    d2_run.CertificationError,
                    "coordinator_evidence_assembly_failed:",
                ):
                    d2_run.assemble_step_evidence(
                        9,
                        [
                            {"kind": changed["kind"], "value": changed},
                            {"kind": transport["kind"], "value": transport},
                            {"kind": browser["kind"], "value": browser},
                        ],
                        self.receipts(),
                        self.manifest,
                        self.manifest_digest,
                    )

    def test_step_thirteen_correlates_database_effect_with_transport_digest(self):
        self.append_prior(12)
        interaction_id = "1532677575736819850"
        effect_identity = {
            "application_id": "1524810437118525552",
            "interaction_id": interaction_id,
            "action_index": 0,
        }
        output_role_id = "1532677575736819852"
        database = self.envelope(
            "starring.d2.db-reconciliation-evidence.v1",
            effect_identity=effect_identity,
            interaction_id=interaction_id,
            route_identity=test_d2_certification.route_identity(
                "deployment-1",
                test_d2_certification.PROCESS_INSTANCE_NEW,
                1,
                2,
                2,
            ),
            output_role_id=output_role_id,
            reconciliation_state="known_success",
            duplicate_external_effect_count=0,
            unsafe_deletion_count=0,
        )
        effect_id = d2_evidence.canonical_effect_identity_sha256(effect_identity)
        transport = self.envelope(
            "starring.d2.transport-indeterminate-evidence.v1",
            injected_outcome="indeterminate",
            transport_indeterminate_injections=1,
            transport_last_audit_reason_sha256=(
                d2_evidence.effect_audit_reason_sha256(effect_id)
            ),
            transport_last_upstream_status=201,
            transport_instance_id=test_d2_certification.TRANSPORT_INSTANCE_ID,
        )
        observation = self.envelope(
            d2_run.DISCORD_RECONCILIATION_OBSERVATION_KIND,
            transport_instance_id=test_d2_certification.TRANSPORT_INSTANCE_ID,
            inventory_digest_sha256="f" * 64,
            resource_kind="role",
            resource_id=output_role_id,
            channel_id=None,
            http_status=200,
            discord_code=None,
            exists=True,
        )
        result = d2_run.advance_certification(
            self.manifest_path,
            13,
            [
                str(self.write_source(transport)),
                str(self.write_source(observation)),
                str(self.write_source(database)),
            ],
        )
        self.assertEqual(result["disposition"], "created")
        evidence = self.receipts()[12]["evidence"]
        self.assertEqual(evidence["effect_id"], effect_id)
        self.assertNotIn("interaction_id", transport)
        self.assertEqual(
            evidence["transport_last_audit_reason_sha256"],
            d2_evidence.effect_audit_reason_sha256(effect_id),
        )
        self.assertEqual(evidence["output_role_id"], database["output_role_id"])
        self.assertEqual(
            evidence["reconciliation_inventory_digest_sha256"],
            observation["inventory_digest_sha256"],
        )
        mismatched = dict(observation)
        mismatched["resource_id"] = "1532677575736819853"
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "coordinator_evidence_assembly_failed:reconciliation_discord_observation_invalid",
        ):
            d2_run.assemble_step_evidence(
                13,
                [
                    {
                        "kind": database["kind"],
                        "value": database,
                    },
                    {
                        "kind": transport["kind"],
                        "value": transport,
                    },
                    {
                        "kind": mismatched["kind"],
                        "value": mismatched,
                    },
                ],
                self.receipts()[:12],
                self.manifest,
                self.manifest_digest,
            )

    def test_step_fifteen_binds_prior_step_fourteen_identity(self):
        self.append_prior(14)
        browser = self.envelope(
            "starring.d2.browser-live-loss-evidence.v1",
            public_origin=self.manifest["cloudflare"]["public_origin"],
            installation_id=self.complete[14]["installation_id"],
            promotion_id=self.complete[14]["replacement_promotion_id"],
            live_lost=True,
            deployment_http_status=200,
            operational_http_status=200,
            product_state="pending",
            operational_state="pending",
            runtime_phase="live",
            serving_state="disconnected",
            public_code="runtime_gateway_disconnected",
            retryable=True,
        )
        transport = self.envelope(
            "starring.d2.transport-gateway-loss-evidence.v1",
            gateway_disconnected=True,
            runtime_ready_status=503,
            transport_gateway_partitioned=True,
            transport_gateway_partition_events=1,
            transport_instance_id=test_d2_certification.TRANSPORT_INSTANCE_ID,
            partition_operation_id=(
                "d2:0123456789abcdef:0007:partition-gateway"
            ),
            partition_completion_sha256="c" * 64,
        )
        healed = self.gateway_healed_source()
        result = d2_run.advance_certification(
            self.manifest_path,
            15,
            [
                str(self.write_source(browser)),
                str(self.write_source(transport)),
                str(self.write_source(healed)),
            ],
        )
        self.assertEqual(result["disposition"], "created")
        evidence = self.receipts()[14]["evidence"]
        self.assertEqual(
            evidence["route_id"],
            self.complete[14]["replacement_route_id"],
        )
        self.assertEqual(
            evidence["installation_id"], self.complete[14]["installation_id"]
        )
        self.assertEqual(
            evidence["promotion_id"],
            self.complete[14]["replacement_promotion_id"],
        )
        self.assertNotIn("route_identity", transport)

    def test_step_fifteen_rejects_forged_replacement_product_identity(self):
        self.append_prior(14)
        browser = self.envelope(
            "starring.d2.browser-live-loss-evidence.v1",
            public_origin=self.manifest["cloudflare"]["public_origin"],
            installation_id="installation:forged",
            promotion_id="f" * 64,
            live_lost=True,
            deployment_http_status=200,
            operational_http_status=200,
            product_state="pending",
            operational_state="pending",
            runtime_phase="live",
            serving_state="disconnected",
            public_code="runtime_gateway_disconnected",
            retryable=True,
        )
        transport = self.envelope(
            "starring.d2.transport-gateway-loss-evidence.v1",
            gateway_disconnected=True,
            runtime_ready_status=503,
            transport_gateway_partitioned=True,
            transport_gateway_partition_events=1,
            transport_instance_id=test_d2_certification.TRANSPORT_INSTANCE_ID,
            partition_operation_id=(
                "d2:0123456789abcdef:0007:partition-gateway"
            ),
            partition_completion_sha256="c" * 64,
        )
        healed = self.gateway_healed_source()
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "coordinator_evidence_assembly_failed:live_loss_replacement_identity_mismatch",
        ):
            d2_run.advance_certification(
                self.manifest_path,
                15,
                [
                    str(self.write_source(browser)),
                    str(self.write_source(transport)),
                    str(self.write_source(healed)),
                ],
            )
        self.assertEqual(len(self.receipts()), 14)

    def test_step_fifteen_requires_healed_quiescent_exact_transport(self):
        self.append_prior(14)
        browser = self.envelope(
            "starring.d2.browser-live-loss-evidence.v1",
            public_origin=self.manifest["cloudflare"]["public_origin"],
            installation_id=self.complete[14]["installation_id"],
            promotion_id=self.complete[14]["replacement_promotion_id"],
            live_lost=True,
            deployment_http_status=200,
            operational_http_status=200,
            product_state="pending",
            operational_state="pending",
            runtime_phase="live",
            serving_state="disconnected",
            public_code="runtime_gateway_disconnected",
            retryable=True,
        )
        transport = self.envelope(
            "starring.d2.transport-gateway-loss-evidence.v1",
            gateway_disconnected=True,
            runtime_ready_status=503,
            transport_gateway_partitioned=True,
            transport_gateway_partition_events=1,
            transport_instance_id=test_d2_certification.TRANSPORT_INSTANCE_ID,
            partition_operation_id=(
                "d2:0123456789abcdef:0007:partition-gateway"
            ),
            partition_completion_sha256="c" * 64,
        )
        invalid = (
            {"gateway_connected": False},
            {"runtime_ready_status": 503},
            {"transport_gateway_partitioned": True},
            {"transport_gateway_partition_events": 2},
            {"transport_duplicate_claimed": True},
            {"transport_indeterminate_armed": True},
            {"transport_instance_id": "d2ti-fedcba9876543210fedcba9876543210"},
            {
                "partition_operation_id": (
                    "d2:0123456789abcdef:0009:partition-gateway"
                )
            },
            {"partition_completion_sha256": "e" * 64},
            {
                "heal_operation_id": (
                    "d2:0123456789abcdef:0008:partition-gateway"
                )
            },
            {"heal_completion_sha256": "invalid"},
        )
        for overrides in invalid:
            with self.subTest(overrides=overrides), self.assertRaisesRegex(
                d2_run.CertificationError,
                "coordinator_evidence_assembly_failed",
            ):
                d2_run.advance_certification(
                    self.manifest_path,
                    15,
                    [
                        str(self.write_source(browser)),
                        str(self.write_source(transport)),
                        str(
                            self.write_source(
                                self.gateway_healed_source(**overrides)
                            )
                        ),
                    ],
                )
        self.assertEqual(len(self.receipts()), 14)

        invalid_loss = (
            {
                "partition_operation_id": (
                    "d2:0123456789abcdef:0007:heal-gateway"
                )
            },
            {"partition_completion_sha256": "invalid"},
            {"transport_gateway_partition_events": 2},
            {
                "transport_instance_id": (
                    "d2ti-fedcba9876543210fedcba9876543210"
                )
            },
        )
        for overrides in invalid_loss:
            with self.subTest(loss=overrides), self.assertRaisesRegex(
                d2_run.CertificationError,
                "coordinator_evidence_assembly_failed",
            ):
                changed = dict(transport)
                changed.update(overrides)
                d2_run.advance_certification(
                    self.manifest_path,
                    15,
                    [
                        str(self.write_source(browser)),
                        str(self.write_source(changed)),
                        str(self.write_source(self.gateway_healed_source())),
                    ],
                )
        self.assertEqual(len(self.receipts()), 14)

    def test_finalization_extension_fails_closed_when_assembler_is_absent(self):
        self.append_prior(15)
        sources = [
            self.write_source(self.envelope(specification["kind"]))
            for specification in d2_run.STEP_SOURCE_SPECS[16]
        ]
        with mock.patch.object(
            d2_run.importlib,
            "import_module",
            side_effect=ImportError("not installed"),
        ):
            with self.assertRaisesRegex(
                d2_run.CertificationError, "finalization_assembler_unavailable"
            ):
                d2_run.advance_certification(
                    self.manifest_path,
                    16,
                    [str(path) for path in sources],
                )

    def test_steps_sixteen_and_seventeen_advance_and_verify_exact_chain(self):
        self.append_prior(15)
        self.write_coordinator_records(last_step=15)
        step_sixteen = self.step_sixteen_sources()
        created = d2_run.advance_certification(
            self.manifest_path,
            16,
            [str(self.write_source(value)) for value in step_sixteen],
        )
        self.assertEqual(created["disposition"], "created")
        receipt = self.receipts()[15]["evidence"]
        self.assertEqual(
            receipt["discord_resource_ids_deleted"],
            sorted(
                self.complete[9]["role_ids"]
                + self.complete[9]["channel_ids"]
                + self.complete[9]["panel_message_ids"]
                + [self.complete[13]["output_role_id"]]
            ),
        )
        self.assertEqual(
            receipt["precleanup_sha256"], self.object_digest(step_sixteen[0])
        )
        self.assertNotEqual(
            receipt["precleanup_sha256"],
            d2_run.sha256_bytes(
                (d2_run.canonical_json(step_sixteen[0]) + "\n").encode("utf-8")
            ),
        )
        step_seventeen = self.step_seventeen_sources(step_sixteen)
        incomplete_paths = [
            str(self.write_source(value))
            for value in (
                step_seventeen[0],
                step_seventeen[1],
                step_seventeen[3],
            )
        ]
        with self.assertRaisesRegex(
            d2_run.CertificationError, "coordinator_source_count_invalid"
        ):
            d2_run.advance_certification(
                self.manifest_path, 17, incomplete_paths
            )
        completed = d2_run.advance_certification(
            self.manifest_path,
            17,
            [str(self.write_source(value)) for value in step_seventeen],
        )
        self.assertEqual(completed["disposition"], "created")
        self.assertEqual(self.receipts()[16]["evidence"], self.complete[17])
        final = d2_run.verify_certification(self.manifest_path)
        self.assertEqual(final["kind"], d2_run.COORDINATOR_FINAL_KIND)
        self.assertEqual(final["status"], "passed")

    def test_step_sixteen_rejects_internally_consistent_forged_run_context(self):
        self.append_prior(15)
        database, teardown, finalization = self.step_sixteen_sources()
        database["installation_id"] = "installation:forged"
        teardown["manifest_sha256"] = "f" * 64
        teardown["run_id"] = "d2-forged-run"
        finalization["manifest_sha256"] = teardown["manifest_sha256"]
        finalization["run_id"] = teardown["run_id"]
        finalization["installation_id"] = database["installation_id"]
        finalization["precleanup_sha256"] = self.object_digest(database)
        finalization["discord_teardown_sha256"] = self.object_digest(teardown)
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "coordinator_evidence_assembly_failed:step16_source_evidence_invalid",
        ):
            d2_run.advance_certification(
                self.manifest_path,
                16,
                [
                    str(self.write_source(value))
                    for value in (database, teardown, finalization)
                ],
            )
        self.assertEqual(len(self.receipts()), 15)
        database, teardown, finalization = self.step_sixteen_sources()
        teardown["source_inventory_digest_sha256"] = "f" * 64
        teardown["freeze_resource_inventory_digest_sha256"] = "f" * 64
        finalization["discord_teardown_sha256"] = self.object_digest(teardown)
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "coordinator_evidence_assembly_failed:step16_source_evidence_invalid",
        ):
            d2_run.advance_certification(
                self.manifest_path,
                16,
                [
                    str(self.write_source(value))
                    for value in (database, teardown, finalization)
                ],
            )
        self.assertEqual(len(self.receipts()), 15)

    def test_step_sixteen_rejects_self_sourced_freeze_hash(self):
        self.append_prior(15)
        database, teardown, finalization = self.step_sixteen_sources()
        teardown["finalization_freeze_intent_sha256"] = "0" * 64
        finalization["discord_teardown_sha256"] = self.object_digest(teardown)
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "coordinator_evidence_assembly_failed:step16_source_evidence_invalid",
        ):
            d2_run.advance_certification(
                self.manifest_path,
                16,
                [
                    str(self.write_source(value))
                    for value in (database, teardown, finalization)
                ],
            )
        self.assertEqual(len(self.receipts()), 15)

    def test_step_sixteen_rejects_teardown_before_trusted_completion(self):
        self.append_prior(15)
        database, teardown, finalization = self.step_sixteen_sources()
        teardown["recorded_at"] = "2026-08-04T01:02:02Z"
        finalization["discord_teardown_sha256"] = self.object_digest(teardown)
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "coordinator_evidence_assembly_failed:step16_source_chronology_invalid",
        ):
            d2_run.advance_certification(
                self.manifest_path,
                16,
                [
                    str(self.write_source(value))
                    for value in (database, teardown, finalization)
                ],
            )
        self.assertEqual(len(self.receipts()), 15)

    def test_step_seventeen_rejects_forged_context_and_prior_digest_chain(self):
        self.append_prior(15)
        step_sixteen = self.step_sixteen_sources()
        d2_run.advance_certification(
            self.manifest_path,
            16,
            [str(self.write_source(value)) for value in step_sixteen],
        )
        database, orchestration, prefix_scan, guild = self.step_seventeen_sources(
            step_sixteen
        )
        forged_guild = "1532677575736819860"
        database["run_id"] = "d2-forged-run"
        database["installation_id"] = "installation:forged"
        guild["guild_id"] = forged_guild
        prefix_scan["guild_id"] = forged_guild
        prefix_scan["resource_prefix"] = "forged-prefix"
        orchestration["run_id"] = database["run_id"]
        orchestration["installation_id"] = database["installation_id"]
        orchestration["manifest_sha256"] = "f" * 64
        orchestration["guild_id"] = forged_guild
        orchestration["resource_prefix"] = "forged-prefix"
        orchestration["precleanup_sha256"] = "e" * 64
        orchestration["discord_teardown_sha256"] = "d" * 64
        orchestration["database_absence_sha256"] = self.object_digest(database)
        orchestration["prefix_scan_sha256"] = self.object_digest(prefix_scan)
        orchestration["guild_deletion_sha256"] = self.object_digest(guild)
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "coordinator_evidence_assembly_failed:step17_source_evidence_invalid",
        ):
            d2_run.advance_certification(
                self.manifest_path,
                17,
                [
                    str(self.write_source(value))
                    for value in (database, orchestration, prefix_scan, guild)
                ],
            )
        self.assertEqual(len(self.receipts()), 16)

    def test_step_seventeen_rejects_source_supplied_completion_time(self):
        self.append_prior(15)
        step_sixteen = self.step_sixteen_sources()
        d2_run.advance_certification(
            self.manifest_path,
            16,
            [str(self.write_source(value)) for value in step_sixteen],
        )
        database, orchestration, prefix_scan, guild = self.step_seventeen_sources(
            step_sixteen
        )
        completion = datetime.datetime.fromisoformat(
            orchestration["coordinator_step16_completed_at"].replace("Z", "+00:00")
        )
        orchestration["coordinator_step16_completed_at"] = (
            completion - datetime.timedelta(seconds=1)
        ).isoformat(timespec="seconds").replace("+00:00", "Z")
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "coordinator_evidence_assembly_failed:step17_source_evidence_invalid",
        ):
            d2_run.advance_certification(
                self.manifest_path,
                17,
                [
                    str(self.write_source(value))
                    for value in (database, orchestration, prefix_scan, guild)
                ],
            )
        self.assertEqual(len(self.receipts()), 16)

    def test_main_advance_outputs_only_receipt_metadata(self):
        source = self.direct_source(1, d2_run.ORCHESTRATOR_BOOTSTRAP_KIND)
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            status = d2_run.main(
                [
                    "advance",
                    "--manifest",
                    str(self.manifest_path),
                    "--step",
                    "1",
                    "--source",
                    str(source),
                ]
            )
        self.assertEqual(status, 0)
        result = json.loads(output.getvalue())
        self.assertEqual(
            set(result),
            {
                "schema_version",
                "kind",
                "run_id",
                "manifest_sha256",
                "step",
                "code",
                "disposition",
                "replayed",
                "receipt_sha256",
            },
        )

    def test_verify_rejects_legacy_raw_receipts_without_coordinator_ledger(self):
        self.append_raw_receipts(17)
        with self.assertRaisesRegex(
            d2_run.CertificationError, "coordinator_intent_missing:1"
        ):
            d2_run.verify_certification(self.manifest_path)

    def test_status_and_advance_reject_legacy_raw_receipt_prefix(self):
        self.append_raw_receipts(1)
        with self.assertRaisesRegex(
            d2_run.CertificationError, "coordinator_intent_missing:1"
        ):
            d2_run.next_certification_action(self.manifest_path)
        source = self.direct_source(2, d2_run.ORCHESTRATOR_PRIOR_ABSENCE_KIND)
        with self.assertRaisesRegex(
            d2_run.CertificationError, "coordinator_intent_missing:1"
        ):
            d2_run.advance_certification(
                self.manifest_path, 2, [str(source)]
            )

    def test_verify_emits_stable_domain_separated_final_record(self):
        self.append_prior(17)
        self.write_coordinator_records()
        first = d2_run.verify_certification(self.manifest_path)
        second = d2_run.verify_certification(self.manifest_path)
        self.assertEqual(first, second)
        self.assertEqual(first["kind"], d2_run.COORDINATOR_FINAL_KIND)
        self.assertEqual(first["status"], "passed")
        self.assertEqual(first["steps"], 17)
        self.assertRegex(first["coordinator_evidence_sha256"], r"^[0-9a-f]{64}$")
        self.assertNotEqual(
            first["coordinator_evidence_sha256"],
            first["receipt_chain_head_sha256"],
        )
        receipts = self.receipts()
        ledger_steps = []
        for step in d2_run.STEP_SPECS:
            intent = json.loads(
                d2_run.coordinator_intent_path(
                    self.manifest_path, step
                ).read_text(encoding="utf-8")
            )
            completion = json.loads(
                d2_run.coordinator_completion_path(
                    self.manifest_path, step
                ).read_text(encoding="utf-8")
            )
            ledger_steps.append(
                {
                    "step": step,
                    "code": d2_run.STEP_SPECS[step].code,
                    "intent_sha256": d2_run.coordinator_record_digest(intent),
                    "completion_sha256": d2_run.coordinator_record_digest(
                        completion
                    ),
                    "receipt_sha256": receipts[step - 1]["receipt_sha256"],
                    "sources": intent["sources"],
                }
            )
        ledger = {
            "schema_version": 1,
            "kind": d2_run.COORDINATOR_LEDGER_KIND,
            "run_id": self.manifest["run_id"],
            "manifest_sha256": self.manifest_digest,
            "receipt_chain_head_sha256": receipts[-1]["receipt_sha256"],
            "steps": ledger_steps,
        }
        self.assertEqual(
            first["coordinator_evidence_sha256"],
            d2_run.sha256_bytes(
                d2_run.COORDINATOR_LEDGER_DOMAIN
                + d2_run.canonical_json(ledger).encode("utf-8")
            ),
        )

    def test_verify_rejects_missing_or_forged_intent(self):
        self.append_prior(17)
        self.write_coordinator_records()
        intent_path = d2_run.coordinator_intent_path(self.manifest_path, 5)
        intent_path.unlink()
        with self.assertRaisesRegex(
            d2_run.CertificationError, "coordinator_intent_missing:5"
        ):
            d2_run.verify_certification(self.manifest_path)

    def test_verify_rejects_missing_or_forged_completion(self):
        self.append_prior(17)
        self.write_coordinator_records()
        completion_path = d2_run.coordinator_completion_path(
            self.manifest_path, 17
        )
        completion = json.loads(completion_path.read_text(encoding="utf-8"))
        completion["receipt_sha256"] = "f" * 64
        self.rewrite_record(completion_path, completion)
        with self.assertRaisesRegex(
            d2_run.CertificationError, "coordinator_completion_drift"
        ):
            d2_run.verify_certification(self.manifest_path)

    def test_verify_rejects_missing_completion_after_receipt(self):
        self.append_prior(17)
        self.write_coordinator_records()
        d2_run.coordinator_completion_path(self.manifest_path, 17).unlink()
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "coordinator_pending_step_unfinished:17",
        ):
            d2_run.verify_certification(self.manifest_path)

    def test_status_rejects_missing_completion_inside_receipt_prefix(self):
        self.append_prior(3)
        d2_run.coordinator_completion_path(self.manifest_path, 2).unlink()
        with self.assertRaisesRegex(
            d2_run.CertificationError, "coordinator_completion_missing:2"
        ):
            d2_run.next_certification_action(self.manifest_path)

    def test_verify_rejects_source_kind_digest_and_order_drift(self):
        self.append_prior(17)
        self.write_coordinator_records()
        intent_path = d2_run.coordinator_intent_path(self.manifest_path, 5)
        intent = json.loads(intent_path.read_text(encoding="utf-8"))
        wrong_kind = json.loads(json.dumps(intent))
        wrong_kind["sources"][0]["kind"] = "starring.d2.browser-wrong.v1"
        self.rewrite_record(intent_path, wrong_kind)
        with self.assertRaisesRegex(
            d2_run.CertificationError, "coordinator_intent_invalid"
        ):
            d2_run.verify_certification(self.manifest_path)
        intent["sources"] = list(reversed(intent["sources"]))
        self.rewrite_record(intent_path, intent)
        with self.assertRaisesRegex(
            d2_run.CertificationError, "coordinator_intent_invalid"
        ):
            d2_run.verify_certification(self.manifest_path)

    def test_verify_rejects_validly_shaped_source_digest_drift(self):
        self.append_prior(17)
        self.write_coordinator_records()
        intent_path = d2_run.coordinator_intent_path(self.manifest_path, 6)
        intent = json.loads(intent_path.read_text(encoding="utf-8"))
        intent["sources"][0]["sha256"] = "f" * 64
        self.rewrite_record(intent_path, intent)
        with self.assertRaisesRegex(
            d2_run.CertificationError, "coordinator_completion_drift"
        ):
            d2_run.verify_certification(self.manifest_path)

    def test_verify_rejects_pending_step_before_final_receipt(self):
        self.append_prior(16)
        self.write_coordinator_records(last_step=16)
        receipts = self.receipts()
        step = 17
        intent = {
            "schema_version": 1,
            "kind": d2_run.COORDINATOR_INTENT_KIND,
            "run_id": self.manifest["run_id"],
            "manifest_sha256": self.manifest_digest,
            "step": step,
            "code": d2_run.STEP_SPECS[step].code,
            "observed_at": OBSERVED_AT,
            "receipt_chain_head_sha256": receipts[-1]["receipt_sha256"],
            "sources": [
                {
                    "kind": specification["kind"],
                    "sha256": d2_run.sha256_bytes(
                        specification["kind"].encode("utf-8")
                    ),
                }
                for specification in d2_run.STEP_SOURCE_SPECS[step]
            ],
        }
        d2_run.write_private_json(
            d2_run.coordinator_intent_path(self.manifest_path, step), intent
        )
        with self.assertRaisesRegex(
            d2_run.CertificationError,
            "coordinator_pending_step_unfinished:17",
        ):
            d2_run.verify_certification(self.manifest_path)

    def test_main_verify_outputs_only_coordinator_final_record(self):
        self.append_prior(17)
        self.write_coordinator_records()
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            status = d2_run.main(
                ["verify", "--manifest", str(self.manifest_path)]
            )
        self.assertEqual(status, 0)
        result = json.loads(output.getvalue())
        self.assertEqual(
            set(result),
            {
                "schema_version",
                "kind",
                "run_id",
                "commit_sha",
                "manifest_sha256",
                "steps",
                "status",
                "resource_prefix",
                "receipt_chain_head_sha256",
                "coordinator_evidence_sha256",
            },
        )


def stat_mode(path):
    return os.stat(path, follow_symlinks=False).st_mode & 0o777


if __name__ == "__main__":
    unittest.main()
