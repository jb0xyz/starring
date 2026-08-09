import copy
import hashlib
import importlib.util
import json
import pathlib
import unittest


MODULE_PATH = pathlib.Path(__file__).with_name("d2_evidence.py")
SPEC = importlib.util.spec_from_file_location("d2_evidence_tests", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
LIVE_LOSS_GATEWAY_DISCONNECTED = json.loads(
    (MODULE_PATH.parent / "fixtures" / "live_loss_gateway_disconnected.v1.json").read_text(
        encoding="utf-8"
    )
)

OBSERVED_AT = "2026-08-04T01:02:03Z"
ORIGIN = "https://d2-api.starring.co.kr"
INSTALLATION_ID = "installation-1"
PROMOTION_ID = "b" * 64
ATTESTATION_ID = "c" * 64
PROCESS_OLD = "11111111111111111111111111111111"
PROCESS_NEW = "0123456789abcdef0123456789abcdef"


def route_identity(
    deployment_id="deployment-1", process=PROCESS_NEW, generation=1, lease_epoch=None
):
    lease = generation if lease_epoch is None else lease_epoch
    return {
        "deployment_id": deployment_id,
        "runtime_generation": generation,
        "route_controller_fencing_token": generation + 1,
        "route_incarnation": generation + 2,
        "origin_process_instance_id": process,
        "origin_serving_lease_epoch": lease,
        "origin_serving_revision": lease,
        "origin_gateway_shard_id": "shard-0",
        "origin_gateway_owner_lease_epoch": generation + 5,
        "origin_gateway_owner_revision": generation + 6,
    }


def serving_identity(deployment_id="deployment-1", process=PROCESS_NEW, lease_epoch=1):
    return {
        "guild_id": "1524810437118525551",
        "ruleset_key": "studyroom",
        "tenant_id": "tenant-1",
        "installation_id": INSTALLATION_ID,
        "deployment_id": deployment_id,
        "attestation_id": ATTESTATION_ID,
        "process_instance_id": process,
        "runtime_generation": 1,
        "target_version": 1,
        "target_content_hash": "d" * 64,
        "binding_revision": 1,
        "binding_fingerprint": "e" * 64,
        "lease_epoch": lease_epoch,
        "revision": lease_epoch,
    }


def effect_identity(interaction_id="1532677575736819846"):
    return {
        "application_id": "1524810437118525552",
        "interaction_id": interaction_id,
        "action_index": 0,
    }


def envelope(kind, **values):
    return {
        "schema_version": 1,
        "kind": kind,
        "observed_at": OBSERVED_AT,
        **values,
    }


def chrome_confirmation(
    challenge="d" * 64,
    payload_digest="a" * 64,
    target_content_hash="a" * 64,
):
    return envelope(
        "starring.d2.chrome-preview-confirmation.v1",
        confirmation_surface="chrome_confirm",
        accepted=True,
        installation_id=INSTALLATION_ID,
        promotion_id=PROMOTION_ID,
        revision=1,
        payload_digest=payload_digest,
        target_content_hash=target_content_hash,
        preview_completion_challenge_sha256=challenge,
        decision_command_sha256="f" * 64,
        summary={
            "panels": 1,
            "modals": 1,
            "rules": 4,
            "actions": 15,
            "target_version": 1,
            "required_approvals": 1,
        },
    )


class D2EvidenceTest(unittest.TestCase):
    def test_canonical_identities_have_stable_domain_separated_digests(self):
        route = route_identity()
        serving = serving_identity()
        effect = effect_identity()
        self.assertEqual(
            MODULE.canonical_route_identity_sha256(route),
            "b978f2ad1de572a9ced1d609984562effa92f6176c936224a66d000c2175ca18",
        )
        self.assertEqual(
            MODULE.canonical_serving_identity_sha256(serving),
            "6331228ee20481970db9de58c384c5393dfd8b95f90826b495dba79c10c30ba2",
        )
        self.assertEqual(
            MODULE.canonical_effect_identity_sha256(effect),
            "ffd64eff508caa4add5553394011dac57fe0a7126cbc21b9a476444b6d761d7a",
        )
        self.assertEqual(
            MODULE.effect_audit_reason_sha256(
                MODULE.canonical_effect_identity_sha256(effect)
            ),
            "e114d60e98b6bd84e24c86db85597ce7d45e07e98c91466fd6817321c8c9b02d",
        )
        reordered = dict(reversed(tuple(route.items())))
        self.assertEqual(
            MODULE.canonical_route_identity_sha256(reordered),
            MODULE.canonical_route_identity_sha256(route),
        )
        self.assertEqual(
            len(
                {
                    MODULE.canonical_route_identity_sha256(route),
                    MODULE.canonical_serving_identity_sha256(serving),
                    MODULE.canonical_effect_identity_sha256(effect),
                }
            ),
            3,
        )

    def test_canonical_identity_rejects_shape_and_type_drift(self):
        invalid_route = route_identity()
        invalid_route["route_incarnation"] = True
        invalid_serving = serving_identity()
        invalid_serving["expires_at"] = OBSERVED_AT
        invalid_effect = effect_identity()
        invalid_effect["action_index"] = 256
        for function, value in (
            (MODULE.canonical_route_identity_sha256, invalid_route),
            (MODULE.canonical_serving_identity_sha256, invalid_serving),
            (MODULE.canonical_effect_identity_sha256, invalid_effect),
        ):
            with self.subTest(function=function.__name__), self.assertRaises(
                MODULE.EvidenceContractError
            ):
                function(value)

    def test_strict_json_rejects_duplicate_and_secret_material(self):
        with self.assertRaisesRegex(MODULE.EvidenceContractError, "evidence_duplicate_key"):
            MODULE.load_strict_json('{"schema_version":1,"schema_version":1}')
        with self.assertRaisesRegex(MODULE.EvidenceContractError, "evidence_forbidden_key"):
            MODULE.load_strict_json(json.dumps({"nested": {"bot_token": "value"}}))
        with self.assertRaisesRegex(MODULE.EvidenceContractError, "evidence_forbidden_value"):
            MODULE.load_strict_json(json.dumps({"value": "postgres://user:pass@host/db"}))

    def test_authentication_adapter_excludes_unobservable_callback_status(self):
        browser = envelope(
            "starring.d2.browser-authentication-evidence.v1",
            public_origin=ORIGIN,
            me_status=200,
            principal_id="discord:1056857223529250906",
            installation_id=INSTALLATION_ID,
            guild_id="1524810437118525551",
            authority_check_status=204,
        )
        evidence = MODULE.assemble_authentication_evidence(browser)
        self.assertEqual(
            set(evidence),
            {
                "me_status",
                "principal_id",
                "installation_id",
                "guild_id",
                "authority_check_status",
                "public_origin",
            },
        )
        self.assertNotIn("oauth_callback_status", evidence)

    def test_live_adapter_joins_public_and_durable_witnesses(self):
        browser = envelope(
            "starring.d2.browser-live-evidence.v1",
            public_origin=ORIGIN,
            installation_id=INSTALLATION_ID,
            promotion_id=PROMOTION_ID,
            pending_observed=True,
            live_observed=True,
            attempts=2,
            product_state="live",
            operational_state="live",
            runtime_phase="live",
            serving_state="fresh",
            deployment_http_status=200,
            operational_http_status=200,
            deployment_observed_at="2026-08-04T01:01:50Z",
            deployment_attestation_revision=11,
            deployment_last_heartbeat_at="2026-08-04T01:01:40Z",
            deployment_lease_expires_at="2026-08-04T01:02:25Z",
            decision_observed_at="2026-08-04T01:01:52Z",
            runtime_observed_at="2026-08-04T01:01:55Z",
            current_attempt=1,
            attestation_revision=11,
            convergence_attempt=1,
            process_instance_id=PROCESS_NEW,
            last_heartbeat_at="2026-08-04T01:01:45Z",
            lease_expires_at="2026-08-04T01:02:30Z",
        )
        database = envelope(
            "starring.d2.db-live-evidence.v1",
            observed_at="2026-08-04T01:02:04Z",
            installation_id=INSTALLATION_ID,
            promotion_id=PROMOTION_ID,
            deployment_id="deployment-1",
            attestation_id=ATTESTATION_ID,
            deployment_revision=11,
            convergence_attempt=1,
            process_instance_id=PROCESS_NEW,
            last_heartbeat_at="2026-08-04T01:02:00Z",
            lease_expires_at="2026-08-04T01:02:45Z",
            route_identity=route_identity(),
            serving_identity=serving_identity(),
        )
        evidence = MODULE.assemble_live_evidence(browser, database)
        self.assertEqual(evidence["route_id"], MODULE.canonical_route_identity_sha256(route_identity()))
        self.assertEqual(
            evidence["serving_lease_id"],
            MODULE.canonical_serving_identity_sha256(serving_identity()),
        )
        self.assertEqual(evidence["deployment_revision"], 11)
        self.assertEqual(evidence["convergence_attempt"], 1)
        self.assertEqual(evidence["process_instance_id"], PROCESS_NEW)
        self.assertEqual(evidence["public_observed_at"], OBSERVED_AT)
        self.assertEqual(evidence["database_observed_at"], "2026-08-04T01:02:04Z")
        drifted = copy.deepcopy(database)
        drifted["promotion_id"] = "f" * 64
        with self.assertRaisesRegex(MODULE.EvidenceContractError, "live_promotion_id_mismatch"):
            MODULE.assemble_live_evidence(browser, drifted)

    def test_live_adapter_rejects_runtime_identity_and_lease_chronology_drift(self):
        browser = envelope(
            "starring.d2.browser-live-evidence.v1",
            public_origin=ORIGIN,
            installation_id=INSTALLATION_ID,
            promotion_id=PROMOTION_ID,
            pending_observed=True,
            live_observed=True,
            attempts=2,
            product_state="live",
            operational_state="live",
            runtime_phase="live",
            serving_state="fresh",
            deployment_http_status=200,
            operational_http_status=200,
            deployment_observed_at="2026-08-04T01:01:50Z",
            deployment_attestation_revision=11,
            deployment_last_heartbeat_at="2026-08-04T01:01:40Z",
            deployment_lease_expires_at="2026-08-04T01:02:25Z",
            decision_observed_at="2026-08-04T01:01:52Z",
            runtime_observed_at="2026-08-04T01:01:55Z",
            current_attempt=1,
            attestation_revision=11,
            convergence_attempt=1,
            process_instance_id=PROCESS_NEW,
            last_heartbeat_at="2026-08-04T01:01:45Z",
            lease_expires_at="2026-08-04T01:02:30Z",
        )
        database = envelope(
            "starring.d2.db-live-evidence.v1",
            observed_at="2026-08-04T01:02:04Z",
            installation_id=INSTALLATION_ID,
            promotion_id=PROMOTION_ID,
            deployment_id="deployment-1",
            attestation_id=ATTESTATION_ID,
            deployment_revision=11,
            convergence_attempt=1,
            process_instance_id=PROCESS_NEW,
            last_heartbeat_at="2026-08-04T01:02:00Z",
            lease_expires_at="2026-08-04T01:02:45Z",
            route_identity=route_identity(),
            serving_identity=serving_identity(),
        )
        cases = (
            ("deployment_revision", 12, "runtime_identity_mismatch"),
            ("convergence_attempt", 2, "runtime_identity_mismatch"),
            ("process_instance_id", PROCESS_OLD, "runtime_identity_mismatch"),
            ("last_heartbeat_at", "2026-08-04T01:01:44Z", "chronology_mismatch"),
            ("lease_expires_at", "2026-08-04T01:02:29Z", "chronology_mismatch"),
            ("observed_at", "2026-08-04T01:02:02Z", "chronology_mismatch"),
        )
        for field, value, code in cases:
            with self.subTest(field=field):
                drifted = copy.deepcopy(database)
                drifted[field] = value
                with self.assertRaisesRegex(MODULE.EvidenceContractError, code):
                    MODULE.assemble_live_evidence(browser, drifted)

    def test_authoring_preview_and_decision_adapters_are_exact(self):
        authoring = envelope(
            "starring.d2.browser-authoring-evidence.v1",
            public_origin=ORIGIN,
            authoring_http_status=201,
            authoring_session_id="session-1",
            authoring_generation=1,
            expected_generation=0,
            authoring_disposition="created",
            installation_id=INSTALLATION_ID,
            one_shot=True,
            worker_request_id="worker-request-1",
            worker_completion_sha256="b" * 64,
        )
        worker = envelope(
            "starring.d2.worker-authoring-evidence.v1",
            manifest_sha256="f" * 64,
            browser_evidence_sha256=hashlib.sha256(
                MODULE.canonical_json(authoring).encode("utf-8")
            ).hexdigest(),
            browser_observed_at=authoring["observed_at"],
            worker_before_observed_at="2026-08-04T01:02:02Z",
            worker_after_observed_at="2026-08-04T01:02:04Z",
            model="gpt-5.6-luna",
            provider="codex_chatgpt",
            reasoning_effort="medium",
            auth_mode="chatgpt",
            codex_cli_version="codex-cli 1.2.3",
            worker_instance_id="worker-0123456789abcdef",
            worker_source_sha256="e" * 64,
            accepted_requests_before=7,
            accepted_requests_after=8,
            accepted_requests_delta=1,
            settled_requests_before=7,
            settled_requests_after=8,
            settled_requests_delta=1,
            active_requests_after=0,
            queued_requests_after=0,
            worker_request_id="worker-request-1",
            worker_completion_sha256="b" * 64,
        )
        preview = envelope(
            "starring.d2.db-authoring-evidence.v1",
            generation_encrypted=True,
            projection_state="preview_ready",
            generation=1,
            generation_count=1,
            payload_digest="a" * 64,
            worker_request_id="worker-request-1",
            worker_completion_sha256="b" * 64,
            installation_id=INSTALLATION_ID,
            authoring_session_id="session-1",
            generation_created_at=OBSERVED_AT,
        )
        public_preview = envelope(
            "starring.d2.browser-preview-ready-evidence.v1",
            public_origin=ORIGIN,
            installation_id=INSTALLATION_ID,
            authoring_session_id="session-1",
            authoring_generation=1,
            projection_state="preview_ready",
            candidate_ruleset_hash="a" * 64,
            worker_request_id="worker-request-1",
            worker_completion_sha256="b" * 64,
        )
        decision = envelope(
            "starring.d2.browser-product-decision-evidence.v1",
            public_origin=ORIGIN,
            installation_id=INSTALLATION_ID,
            promotion_id=PROMOTION_ID,
            authoring_session_id="session-1",
            authoring_generation=1,
            target_content_hash="a" * 64,
            payload_digest="a" * 64,
            preview_state="pending_approval",
            approval_state="approved",
            apply_state="runtime_pending",
            runtime_pending_observed=True,
            preview_completion_challenge_sha256="d" * 64,
            decision_command_sha256="f" * 64,
            chrome_confirmation=chrome_confirmation(),
        )
        self.assertTrue(
            MODULE.assemble_authoring_evidence(authoring, worker)["one_shot"]
        )
        self.assertTrue(
            MODULE.assemble_preview_evidence(public_preview, preview)[
                "generation_encrypted"
            ]
        )
        self.assertEqual(
            MODULE.assemble_decision_evidence(decision)["apply_state"],
            "runtime_pending",
        )
        assembled_decision = MODULE.assemble_decision_evidence(decision)
        self.assertEqual(assembled_decision["authoring_session_id"], "session-1")
        self.assertEqual(assembled_decision["authoring_generation"], 1)
        self.assertEqual(assembled_decision["payload_digest"], "a" * 64)
        self.assertEqual(assembled_decision["decision_command_sha256"], "f" * 64)
        self.assertEqual(
            assembled_decision["chrome_confirmation_sha256"],
            hashlib.sha256(
                MODULE.canonical_json(decision["chrome_confirmation"]).encode("utf-8")
            ).hexdigest(),
        )
        self.assertEqual(assembled_decision["confirmation_surface"], "chrome_confirm")
        fast_live = copy.deepcopy(decision)
        fast_live["apply_state"] = "live"
        self.assertEqual(
            MODULE.assemble_decision_evidence(fast_live)["apply_state"],
            "runtime_pending",
        )
        missing_pending = copy.deepcopy(fast_live)
        missing_pending["runtime_pending_observed"] = False
        with self.assertRaisesRegex(
            MODULE.EvidenceContractError, "runtime_pending_observation_missing"
        ):
            MODULE.assemble_decision_evidence(missing_pending)
        mismatched_confirmation = copy.deepcopy(decision)
        mismatched_confirmation["chrome_confirmation"][
            "preview_completion_challenge_sha256"
        ] = "e" * 64
        with self.assertRaisesRegex(
            MODULE.EvidenceContractError, "chrome_preview_confirmation_binding_invalid"
        ):
            MODULE.assemble_decision_evidence(mismatched_confirmation)
        mismatched_command = copy.deepcopy(decision)
        mismatched_command["chrome_confirmation"]["decision_command_sha256"] = (
            "e" * 64
        )
        with self.assertRaisesRegex(
            MODULE.EvidenceContractError, "chrome_preview_confirmation_binding_invalid"
        ):
            MODULE.assemble_decision_evidence(mismatched_command)
        for field, path in (
            ("schema_version", None),
            ("revision", None),
            ("target_version", "summary"),
            ("panels", "summary"),
            ("required_approvals", "summary"),
        ):
            with self.subTest(field=field):
                boolean_integer = copy.deepcopy(decision)
                target = boolean_integer["chrome_confirmation"]
                if path is not None:
                    target = target[path]
                target[field] = True
                with self.assertRaises(MODULE.EvidenceContractError):
                    MODULE.assemble_decision_evidence(boolean_integer)

    def test_interaction_adapter_joins_database_and_transport_resources(self):
        database = envelope(
            "starring.d2.db-interaction-evidence.v1",
            create_interaction_id="1532677575736819845",
            join_interaction_id="1532677575736819846",
            actor_user_id="1056857223529250906",
            joined_role_id="1532677575736819847",
            deployment_id="deployment-1",
            route_identity=route_identity(),
            instance_id="instance-1",
            role_ids=["1532677575736819847"],
            channel_ids=["1532677575736819848"],
            panel_message_ids=[
                "1532677575736819849",
                "1532677575736819854",
            ],
            ephemeral_count=2,
        )
        transport = envelope(
            "starring.d2.transport-resource-evidence.v1",
            role_ids=["1532677575736819847"],
            channel_ids=["1532677575736819848"],
            panel_message_ids=[
                "1532677575736819849",
                "1532677575736819854",
            ],
            inventory_digest_sha256="b" * 64,
            transport_instance_id="d2ti-0123456789abcdef0123456789abcdef",
        )
        evidence = MODULE.assemble_interaction_evidence(database, transport)
        self.assertEqual(evidence["route_id"], MODULE.canonical_route_identity_sha256(route_identity()))
        drifted = copy.deepcopy(transport)
        drifted["role_ids"] = ["1532677575736819852"]
        with self.assertRaisesRegex(MODULE.EvidenceContractError, "interaction_role_ids_mismatch"):
            MODULE.assemble_interaction_evidence(database, drifted)
        for panel_message_ids in (
            ["1532677575736819849"],
            [
                "1532677575736819849",
                "1532677575736819854",
                "1532677575736819855",
            ],
        ):
            changed_database = copy.deepcopy(database)
            changed_transport = copy.deepcopy(transport)
            changed_database["panel_message_ids"] = panel_message_ids
            changed_transport["panel_message_ids"] = panel_message_ids
            with self.assertRaisesRegex(
                MODULE.EvidenceContractError,
                "interaction_panel_message_ids_cardinality_invalid",
            ):
                MODULE.assemble_interaction_evidence(
                    changed_database, changed_transport
                )

    def test_interaction_adapter_requires_exact_visible_discord_observation(self):
        database = envelope(
            "starring.d2.db-interaction-evidence.v1",
            create_interaction_id="1532677575736819845",
            join_interaction_id="1532677575736819846",
            actor_user_id="1056857223529250906",
            joined_role_id="1532677575736819847",
            deployment_id="deployment-1",
            route_identity=route_identity(),
            instance_id="instance-1",
            role_ids=["1532677575736819847"],
            channel_ids=["1532677575736819848"],
            panel_message_ids=[
                "1532677575736819849",
                "1532677575736819854",
            ],
            ephemeral_count=2,
        )
        transport = envelope(
            "starring.d2.transport-resource-evidence.v1",
            role_ids=["1532677575736819847"],
            channel_ids=["1532677575736819848"],
            panel_message_ids=[
                "1532677575736819849",
                "1532677575736819854",
            ],
            inventory_digest_sha256="b" * 64,
            transport_instance_id="d2ti-0123456789abcdef0123456789abcdef",
        )
        discord = envelope(
            "starring.d2.browser-discord-interaction-observation.v1",
            guild_id="1532677575736819800",
            resource_prefix="starring-d2-abcdef123456",
            actor_user_id="1056857223529250906",
            create_interaction_id="1532677575736819845",
            join_interaction_id="1532677575736819846",
            joined_role_id="1532677575736819847",
            role_ids=["1532677575736819847"],
            channel_ids=["1532677575736819848"],
            panel_message_ids=[
                "1532677575736819849",
                "1532677575736819854",
            ],
            create_response_observed=True,
            join_response_observed=True,
            private_channel_observed=True,
            role_assignment_observed=True,
            welcome_panel_observed=True,
            join_panel_observed=True,
            confirmation_surface="chrome_discord_web",
        )
        scope = {
            "guild_id": "1532677575736819800",
            "resource_prefix": "starring-d2-abcdef123456",
            "actor_user_id": "1056857223529250906",
        }
        evidence = MODULE.assemble_observed_interaction_evidence(
            database, transport, discord, scope
        )
        self.assertEqual(evidence["role_ids"], ["1532677575736819847"])
        mismatches = {
            "guild_id": "1532677575736819801",
            "actor_user_id": "1056857223529250907",
            "join_interaction_id": "1532677575736819850",
            "joined_role_id": "1532677575736819851",
            "role_ids": ["1532677575736819851"],
            "welcome_panel_observed": False,
            "join_panel_observed": False,
            "confirmation_surface": "manual_text",
        }
        for field, value in mismatches.items():
            with self.subTest(field=field):
                changed = copy.deepcopy(discord)
                changed[field] = value
                with self.assertRaises(MODULE.EvidenceContractError):
                    MODULE.assemble_observed_interaction_evidence(
                        database, transport, changed, scope
                    )

    def test_duplicate_adapter_requires_independent_exact_witnesses(self):
        database = envelope(
            "starring.d2.db-duplicate-evidence.v1",
            interaction_id="1532677575736819846",
            effect_identity=effect_identity(),
            external_effect_count=1,
            receipt_state="completed",
        )
        transport = envelope(
            "starring.d2.transport-duplicate-evidence.v1",
            interaction_id="1532677575736819846",
            delivery_count=2,
            transport_duplicate_injections=1,
            transport_duplicate_delivery_count=2,
            transport_last_duplicate_interaction_id="1532677575736819846",
            role_ids=["1532677575736819847"],
            channel_ids=["1532677575736819848"],
            panel_message_ids=[
                "1532677575736819849",
                "1532677575736819854",
            ],
            inventory_digest_sha256="b" * 64,
            transport_instance_id="d2ti-0123456789abcdef0123456789abcdef",
        )
        evidence = MODULE.assemble_duplicate_evidence(database, transport)
        self.assertEqual(evidence["external_effect_count"], 1)
        self.assertRegex(evidence["effect_id"], r"^[0-9a-f]{64}$")
        drifted = copy.deepcopy(transport)
        drifted["transport_duplicate_delivery_count"] = 3
        with self.assertRaisesRegex(
            MODULE.EvidenceContractError, "duplicate_transport_outcome_invalid"
        ):
            MODULE.assemble_duplicate_evidence(database, drifted)
        for panel_message_ids in (
            ["1532677575736819849"],
            [
                "1532677575736819849",
                "1532677575736819854",
                "1532677575736819855",
            ],
        ):
            changed = copy.deepcopy(transport)
            changed["panel_message_ids"] = panel_message_ids
            with self.assertRaisesRegex(
                MODULE.EvidenceContractError,
                "duplicate_panel_message_ids_cardinality_invalid",
            ):
                MODULE.assemble_duplicate_evidence(database, changed)

    def test_reconstruction_adapter_requires_rotated_process_bound_identity(self):
        database = envelope(
            "starring.d2.db-reconstruction-evidence.v1",
            route_reconstructed=True,
            instance_reconstructed=True,
            deployment_id="deployment-1",
            source_route_identity=route_identity(process=PROCESS_OLD, lease_epoch=1),
            reconstructed_route_identity=route_identity(process=PROCESS_NEW, lease_epoch=2),
            source_serving_identity=serving_identity(process=PROCESS_OLD, lease_epoch=1),
            reconstructed_serving_identity=serving_identity(
                process=PROCESS_NEW, lease_epoch=2
            ),
            instance_id="instance-1",
            pinned_ruleset_digest="f" * 64,
            probe_interaction_id="1532677575736819851",
            process_instance_id=PROCESS_NEW,
        )
        evidence = MODULE.assemble_reconstruction_evidence(database)
        self.assertNotEqual(evidence["source_route_id"], evidence["reconstructed_route_id"])
        self.assertNotEqual(
            evidence["source_serving_lease_id"],
            evidence["reconstructed_serving_lease_id"],
        )
        unrotated = copy.deepcopy(database)
        unrotated["reconstructed_route_identity"] = copy.deepcopy(
            unrotated["source_route_identity"]
        )
        with self.assertRaises(MODULE.EvidenceContractError):
            MODULE.assemble_reconstruction_evidence(unrotated)

    def test_reconciliation_adapter_requires_fault_and_durable_witnesses(self):
        interaction_id = "1532677575736819850"
        database = envelope(
            "starring.d2.db-reconciliation-evidence.v1",
            effect_identity=effect_identity(interaction_id),
            interaction_id=interaction_id,
            route_identity=route_identity(),
            reconciliation_state="known_success",
            duplicate_external_effect_count=0,
            unsafe_deletion_count=0,
            output_role_id="1532677575736819852",
        )
        effect_id = MODULE.canonical_effect_identity_sha256(
            database["effect_identity"]
        )
        transport = envelope(
            "starring.d2.transport-indeterminate-evidence.v1",
            injected_outcome="indeterminate",
            transport_indeterminate_injections=1,
            transport_last_audit_reason_sha256=MODULE.effect_audit_reason_sha256(
                effect_id
            ),
            transport_last_upstream_status=201,
            transport_instance_id="d2ti-0123456789abcdef0123456789abcdef",
        )
        discord = envelope(
            "starring.d2.discord-reconciliation-role-observation.v1",
            transport_instance_id="d2ti-0123456789abcdef0123456789abcdef",
            inventory_digest_sha256="b" * 64,
            resource_kind="role",
            resource_id="1532677575736819852",
            channel_id=None,
            http_status=200,
            discord_code=None,
            exists=True,
        )
        evidence = MODULE.assemble_reconciliation_evidence(
            database, transport, discord
        )
        self.assertEqual(evidence["reconciliation_state"], "known_success")
        self.assertEqual(evidence["output_role_id"], "1532677575736819852")
        self.assertEqual(evidence["reconciliation_inventory_digest_sha256"], "b" * 64)
        unsafe = copy.deepcopy(database)
        unsafe["unsafe_deletion_count"] = 1
        with self.assertRaisesRegex(MODULE.EvidenceContractError, "reconciliation_safety_invalid"):
            MODULE.assemble_reconciliation_evidence(unsafe, transport, discord)
        mismatched = copy.deepcopy(transport)
        mismatched["transport_last_audit_reason_sha256"] = "f" * 64
        with self.assertRaisesRegex(
            MODULE.EvidenceContractError,
            "reconciliation_audit_correlation_mismatch",
        ):
            MODULE.assemble_reconciliation_evidence(database, mismatched, discord)
        raw_interaction = copy.deepcopy(transport)
        raw_interaction["interaction_id"] = interaction_id
        with self.assertRaisesRegex(MODULE.EvidenceContractError, "fields_invalid"):
            MODULE.assemble_reconciliation_evidence(
                database, raw_interaction, discord
            )
        boolean_counter = copy.deepcopy(transport)
        boolean_counter["transport_indeterminate_injections"] = True
        with self.assertRaisesRegex(
            MODULE.EvidenceContractError, "indeterminate_injection_count_invalid"
        ):
            MODULE.assemble_reconciliation_evidence(
                database, boolean_counter, discord
            )
        boolean_safety = copy.deepcopy(database)
        boolean_safety["unsafe_deletion_count"] = False
        with self.assertRaisesRegex(
            MODULE.EvidenceContractError,
            "reconciliation_unsafe_deletion_count_invalid",
        ):
            MODULE.assemble_reconciliation_evidence(
                boolean_safety, transport, discord
            )
        missing = copy.deepcopy(discord)
        missing["exists"] = False
        missing["http_status"] = 404
        missing["discord_code"] = 10011
        with self.assertRaisesRegex(
            MODULE.EvidenceContractError,
            "reconciliation_discord_observation_invalid",
        ):
            MODULE.assemble_reconciliation_evidence(
                database, transport, missing
            )
        wrong_role = copy.deepcopy(discord)
        wrong_role["resource_id"] = "1532677575736819853"
        with self.assertRaisesRegex(
            MODULE.EvidenceContractError,
            "reconciliation_discord_observation_invalid",
        ):
            MODULE.assemble_reconciliation_evidence(
                database, transport, wrong_role
            )
        wrong_instance = copy.deepcopy(discord)
        wrong_instance["transport_instance_id"] = (
            "d2ti-fedcba9876543210fedcba9876543210"
        )
        with self.assertRaisesRegex(
            MODULE.EvidenceContractError,
            "reconciliation_discord_observation_invalid",
        ):
            MODULE.assemble_reconciliation_evidence(
                database, transport, wrong_instance
            )

    def test_replacement_and_live_loss_adapters_require_independent_sources(self):
        replacement_browser = envelope(
            "starring.d2.browser-replacement-evidence.v1",
            public_origin=ORIGIN,
            installation_id=INSTALLATION_ID,
            source_promotion_id=PROMOTION_ID,
            replacement_promotion_id="a" * 64,
            replacement_kind="update",
            preview_state="pending_approval",
            approval_state="approved",
            apply_state="runtime_pending",
            pending_observed=True,
            live_observed=True,
            product_state="live",
            operational_state="live",
            runtime_phase="live",
            serving_state="fresh",
            drain_conflict_observed=True,
            drain_attempts=1,
        )
        replacement_database = envelope(
            "starring.d2.db-replacement-evidence.v1",
            installation_id=INSTALLATION_ID,
            source_promotion_id=PROMOTION_ID,
            replacement_promotion_id="a" * 64,
            source_deployment_id="deployment-1",
            source_route_identity=route_identity(),
            replacement_deployment_id="deployment-2",
            replacement_route_identity=route_identity("deployment-2", generation=2),
            previous_target_drained=True,
            replacement_live=True,
            prior_route_absent=True,
        )
        replacement = MODULE.assemble_replacement_evidence(
            replacement_browser, replacement_database
        )
        loss_browser = copy.deepcopy(LIVE_LOSS_GATEWAY_DISCONNECTED)
        loss_transport = envelope(
            "starring.d2.transport-gateway-loss-evidence.v1",
            gateway_disconnected=True,
            runtime_ready_status=503,
            transport_gateway_partitioned=True,
            transport_gateway_partition_events=1,
            transport_instance_id="d2ti-0123456789abcdef0123456789abcdef",
            partition_operation_id=(
                "d2:0123456789abcdef:0007:partition-gateway"
            ),
            partition_completion_sha256="c" * 64,
        )
        loss = MODULE.assemble_live_loss_evidence(
            loss_browser,
            loss_transport,
            {
                "installation_id": replacement["installation_id"],
                "replacement_promotion_id": replacement[
                    "replacement_promotion_id"
                ],
                "replacement_route_id": replacement["replacement_route_id"],
            },
        )
        self.assertEqual(replacement["replacement_route_id"], loss["route_id"])
        self.assertEqual(loss["runtime_ready_status"], 503)
        healed_transport = envelope(
            "starring.d2.transport-gateway-healed-evidence.v1",
            gateway_connected=True,
            runtime_ready_status=200,
            transport_gateway_partitioned=False,
            transport_gateway_partition_events=1,
            transport_duplicate_armed=False,
            transport_duplicate_claimed=False,
            transport_indeterminate_armed=False,
            transport_indeterminate_claimed=False,
            transport_instance_id="d2ti-0123456789abcdef0123456789abcdef",
            partition_operation_id=(
                "d2:0123456789abcdef:0007:partition-gateway"
            ),
            partition_completion_sha256="c" * 64,
            heal_operation_id="d2:0123456789abcdef:0008:heal-gateway",
            heal_completion_sha256="d" * 64,
        )
        healed = MODULE.assemble_healed_live_loss_evidence(
            loss_browser,
            loss_transport,
            healed_transport,
            {
                "installation_id": replacement["installation_id"],
                "replacement_promotion_id": replacement[
                    "replacement_promotion_id"
                ],
                "replacement_route_id": replacement["replacement_route_id"],
            },
        )
        self.assertEqual(healed, loss)
        invalid_heals = {
            "runtime_ready_status": 503,
            "transport_gateway_partitioned": True,
            "transport_gateway_partition_events": 2,
            "transport_duplicate_armed": True,
            "transport_instance_id": "d2ti-fedcba9876543210fedcba9876543210",
            "partition_operation_id": (
                "d2:0123456789abcdef:0009:partition-gateway"
            ),
            "partition_completion_sha256": "e" * 64,
            "heal_operation_id": (
                "d2:0123456789abcdef:0008:partition-gateway"
            ),
            "heal_completion_sha256": "invalid",
        }
        for field, value in invalid_heals.items():
            with self.subTest(field=field):
                changed = copy.deepcopy(healed_transport)
                changed[field] = value
                with self.assertRaises(MODULE.EvidenceContractError):
                    MODULE.assemble_healed_live_loss_evidence(
                        loss_browser,
                        loss_transport,
                        changed,
                        {
                            "installation_id": replacement["installation_id"],
                            "replacement_promotion_id": replacement[
                                "replacement_promotion_id"
                            ],
                            "replacement_route_id": replacement[
                                "replacement_route_id"
                            ],
                        },
                    )
        for value in (
            "d2:0123456789abcdef:0006:heal-gateway",
            "d2:fedcba9876543210:0008:heal-gateway",
        ):
            changed = copy.deepcopy(healed_transport)
            changed["heal_operation_id"] = value
            with self.assertRaises(MODULE.EvidenceContractError):
                MODULE.assemble_healed_live_loss_evidence(
                    loss_browser,
                    loss_transport,
                    changed,
                    {
                        "installation_id": replacement["installation_id"],
                        "replacement_promotion_id": replacement[
                            "replacement_promotion_id"
                        ],
                        "replacement_route_id": replacement[
                            "replacement_route_id"
                        ],
                    },
                )
        same_digest = copy.deepcopy(healed_transport)
        same_digest["heal_completion_sha256"] = same_digest[
            "partition_completion_sha256"
        ]
        with self.assertRaises(MODULE.EvidenceContractError):
            MODULE.assemble_healed_live_loss_evidence(
                loss_browser,
                loss_transport,
                same_digest,
                {
                    "installation_id": replacement["installation_id"],
                    "replacement_promotion_id": replacement[
                        "replacement_promotion_id"
                    ],
                    "replacement_route_id": replacement[
                        "replacement_route_id"
                    ],
                },
            )
        unrelated_loss = copy.deepcopy(loss_transport)
        unrelated_loss["transport_gateway_partition_events"] = 2
        with self.assertRaisesRegex(
            MODULE.EvidenceContractError,
            "live_loss_partition_event_count_invalid",
        ):
            MODULE.assemble_live_loss_evidence(
                loss_browser,
                unrelated_loss,
                {
                    "installation_id": replacement["installation_id"],
                    "replacement_promotion_id": replacement[
                        "replacement_promotion_id"
                    ],
                    "replacement_route_id": replacement["replacement_route_id"],
                },
            )
        invalid_loss_bindings = {
            "partition_operation_id": "d2:0123456789abcdef:0007:heal-gateway",
            "partition_completion_sha256": "invalid",
        }
        for field, value in invalid_loss_bindings.items():
            with self.subTest(field=field):
                changed = copy.deepcopy(loss_transport)
                changed[field] = value
                with self.assertRaises(MODULE.EvidenceContractError):
                    MODULE.assemble_live_loss_evidence(
                        loss_browser,
                        changed,
                        {
                            "installation_id": replacement["installation_id"],
                            "replacement_promotion_id": replacement[
                                "replacement_promotion_id"
                            ],
                            "replacement_route_id": replacement[
                                "replacement_route_id"
                            ],
                        },
                    )
        raw_route = copy.deepcopy(loss_transport)
        raw_route["route_identity"] = route_identity(
            "deployment-2", generation=2
        )
        with self.assertRaisesRegex(MODULE.EvidenceContractError, "fields_invalid"):
            MODULE.assemble_live_loss_evidence(
                loss_browser,
                raw_route,
                {
                    "installation_id": replacement["installation_id"],
                    "replacement_promotion_id": replacement[
                        "replacement_promotion_id"
                    ],
                    "replacement_route_id": replacement["replacement_route_id"],
                },
            )
        with self.assertRaisesRegex(
            MODULE.EvidenceContractError, "live_loss_prior_route_id_invalid"
        ):
            MODULE.assemble_live_loss_evidence(
                loss_browser,
                loss_transport,
                {
                    "installation_id": replacement["installation_id"],
                    "replacement_promotion_id": replacement[
                        "replacement_promotion_id"
                    ],
                    "replacement_route_id": "route-raw",
                },
            )

    def test_envelopes_reject_extra_and_forbidden_nested_fields(self):
        browser = envelope(
            "starring.d2.browser-authentication-evidence.v1",
            public_origin=ORIGIN,
            me_status=200,
            principal_id="discord:1056857223529250906",
            installation_id=INSTALLATION_ID,
            guild_id="1524810437118525551",
            authority_check_status=204,
        )
        browser["operator_note"] = "extra"
        with self.assertRaisesRegex(MODULE.EvidenceContractError, "fields_invalid"):
            MODULE.assemble_authentication_evidence(browser)
        browser.pop("operator_note")
        browser["nested"] = {"session_cookie": "value"}
        with self.assertRaises(MODULE.EvidenceContractError):
            MODULE.assemble_authentication_evidence(browser)


if __name__ == "__main__":
    unittest.main()
