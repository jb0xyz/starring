import datetime
import hashlib
import os
import pathlib
import re
import stat

from d2_certification import (
    canonical_json,
    fsync_directory,
    load_json_file,
    require_absolute_path,
    require_owned_mode,
)
from d2_orchestrator_contract import (
    append_journal,
    external_keychain_inventory,
    fail,
    keychain_inventory,
    load_state,
    standing_snapshot,
    utc_now,
    write_atomic,
)


FREEZE_STOP_ORDER = ("tunnel", "runtime")
FINAL_STOP_ORDER = ("api", "worker", "transport")
DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")
SNOWFLAKE_PATTERN = re.compile(r"^[1-9][0-9]{0,19}$")
UTC_PATTERN = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?Z$"
)
PRECLEANUP_KIND = "starring.d2.db-precleanup-evidence.v1"
DATABASE_DESTRUCTION_KIND = "starring.d2.database-destruction.v1"
DATABASE_ABSENCE_KIND = "starring.d2.db-absence-evidence.v1"
DISCORD_TEARDOWN_KIND = "starring.d2.discord-resource-teardown.v1"
FINALIZATION_INTENT_KIND = "starring.d2.database-finalization-intent.v1"
FINALIZATION_FREEZE_KIND = "starring.d2.finalization-freeze-intent.v1"
FINALIZATION_KIND = "starring.d2.orchestrator-finalization-evidence.v1"
PREFIX_SCAN_KIND = "starring.d2.browser-discord-resource-prefix-scan-evidence.v1"
ORCHESTRATION_ABSENCE_KIND = "starring.d2.orchestrator-total-absence-evidence.v1"
GUILD_DELETION_KIND = "starring.d2.browser-discord-guild-deletion-evidence.v1"


def _require_exact(value, fields, code):
    if not isinstance(value, dict) or set(value) != set(fields):
        fail(code)
    if "schema_version" in fields and type(value["schema_version"]) is not int:
        fail(code)
    return value


def _require_timestamp(value, code):
    if not isinstance(value, str) or not UTC_PATTERN.fullmatch(value):
        fail(code)
    try:
        datetime.datetime.fromisoformat(value[:-1] + "+00:00")
    except ValueError:
        fail(code)
    return value


def _parse_timestamp(value, code):
    _require_timestamp(value, code)
    try:
        return datetime.datetime.fromisoformat(value[:-1] + "+00:00")
    except ValueError:
        fail(code)


def _require_digest(value, code):
    if not isinstance(value, str) or not DIGEST_PATTERN.fullmatch(value):
        fail(code)
    return value


def _require_snowflake(value, code):
    if (
        not isinstance(value, str)
        or not SNOWFLAKE_PATTERN.fullmatch(value)
        or int(value) > 18446744073709551615
    ):
        fail(code)
    return value


def _digest(value):
    return hashlib.sha256(canonical_json(value).encode("utf-8")).hexdigest()


def _precise_utc_now():
    return (
        datetime.datetime.now(datetime.timezone.utc)
        .isoformat(timespec="microseconds")
        .replace("+00:00", "Z")
    )


def _load_private(path, label):
    require_owned_mode(path, 0o600, label)
    return load_json_file(path, label)


def _filesystem_entry_present(path, code):
    try:
        path.lstat()
    except FileNotFoundError:
        return False
    except OSError:
        fail(code)
    return True


def finalization_directory(context):
    return context.artifact_directory / "finalization"


def _require_owned_directory(path, code):
    try:
        metadata = path.lstat()
    except OSError:
        fail(code)
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or path.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        fail(code)
    return path


def ensure_finalization_directory(context):
    expected_artifact = context.run_directory / "orchestrator"
    if context.artifact_directory != expected_artifact:
        fail("artifact_directory_invalid")
    _require_owned_directory(context.artifact_directory, "artifact_directory_invalid")
    path = finalization_directory(context)
    try:
        path.lstat()
    except FileNotFoundError:
        try:
            os.mkdir(path, 0o700)
        except FileExistsError:
            pass
        except OSError:
            fail("finalization_directory_invalid")
        fsync_directory(context.artifact_directory, "finalization_directory_parent")
    except OSError:
        fail("finalization_directory_invalid")
    return _require_owned_directory(path, "finalization_directory_invalid")


def validate_mutation_roots(context):
    expected_root = pathlib.Path(f"/private/tmp/starring-d2-{context.manifest['run_id']}")
    if context.root != expected_root or context.cluster_root != expected_root / "postgres":
        fail("finalization_mutation_root_invalid")
    _require_owned_directory(context.root, "finalization_mutation_root_invalid")
    _require_owned_directory(
        context.cluster_root, "finalization_mutation_cluster_invalid"
    )


def freeze_intent_path(context):
    return finalization_directory(context) / "finalization-freeze-intent.json"


def abort_teardown_tombstone_path(context):
    return context.artifact_directory / "discord-resource-teardown-abort.json"


def abort_teardown_progress_path(context):
    return context.artifact_directory / "discord-resource-teardown-abort-progress.json"


def abort_teardown_evidence_path(context):
    return context.artifact_directory / "discord-resource-teardown-abort-evidence.json"


def require_certification_eligible_teardown(context):
    if any(
        path.exists() or path.is_symlink()
        for path in (
            abort_teardown_tombstone_path(context),
            abort_teardown_progress_path(context),
            abort_teardown_evidence_path(context),
        )
    ):
        fail("standalone_teardown_certification_disqualified")


def precleanup_path(context):
    return finalization_directory(context) / "database-precleanup.json"


def destroy_intent_path(context):
    return finalization_directory(context) / "database-destroy-intent.json"


def destroy_result_path(context):
    return finalization_directory(context) / "database-destroy-result.json"


def database_absence_path(context):
    return finalization_directory(context) / "database-absence.json"


def finalization_evidence_path(context):
    return finalization_directory(context) / "orchestrator-finalization.json"


def orchestration_absence_path(context):
    return finalization_directory(context) / "orchestrator-total-absence.json"


def step_sixteen_evidence_path(context):
    return context.artifact_directory / "step-16-evidence.json"


def step_seventeen_evidence_path(context):
    return context.artifact_directory / "step-17-evidence.json"


def validate_precleanup(context, value):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "installation_id",
        "scoped_installation_count",
        "scoped_deployment_count",
        "terminal_product_operation_count",
        "unresolved_product_operation_count",
        "unresolved_receipt_count",
        "unresolved_journal_entry_count",
        "unresolved_rollback_count",
        "ready_for_cleanup",
    }
    _require_exact(value, fields, "database_precleanup_evidence_invalid")
    expected_installation = f"installation:{context.manifest['discord']['resource_prefix']}"
    if (
        value["schema_version"] != 1
        or value["kind"] != PRECLEANUP_KIND
        or value["installation_id"] != expected_installation
        or type(value["scoped_installation_count"]) is not int
        or value["scoped_installation_count"] != 1
        or type(value["scoped_deployment_count"]) is not int
        or value["scoped_deployment_count"] <= 0
        or type(value["terminal_product_operation_count"]) is not int
        or value["terminal_product_operation_count"] < 0
        or value["ready_for_cleanup"] is not True
    ):
        fail("database_precleanup_evidence_invalid")
    _require_timestamp(value["observed_at"], "database_precleanup_evidence_invalid")
    for field in (
        "unresolved_product_operation_count",
        "unresolved_receipt_count",
        "unresolved_journal_entry_count",
        "unresolved_rollback_count",
    ):
        if type(value[field]) is not int or value[field] != 0:
            fail("database_precleanup_blocked")
    return value


def _resource_key(resource):
    return (
        resource["kind"],
        resource["resource_id"],
        resource.get("channel_id"),
    )


def _validate_resource(resource):
    kind = resource.get("kind") if isinstance(resource, dict) else None
    fields = {"kind", "resource_id", "channel_id"} if kind == "message" else {
        "kind",
        "resource_id",
    }
    _require_exact(resource, fields, "discord_teardown_evidence_invalid")
    if kind not in {"message", "channel", "role"}:
        fail("discord_teardown_evidence_invalid")
    _require_snowflake(resource["resource_id"], "discord_teardown_evidence_invalid")
    if kind == "message":
        _require_snowflake(resource["channel_id"], "discord_teardown_evidence_invalid")
    return resource


def validate_discord_teardown(context, value):
    fields = {
        "schema_version",
        "kind",
        "manifest_sha256",
        "run_id",
        "recorded_at",
        "transport_instance_id",
        "source_inventory_digest_sha256",
        "final_inventory_digest_sha256",
        "resource_union_sha256",
        "created_resources",
        "deleted_resources",
        "active_resources",
        "resource_ids",
        "message_ids",
        "channel_ids",
        "role_ids",
        "proxy_deletions",
        "direct_observations",
        "all_resources_absent",
        "finalization_freeze_intent_sha256",
        "certification_step15_receipt_sha256",
        "coordinator_step15_completion_sha256",
        "freeze_resource_inventory_digest_sha256",
    }
    _require_exact(value, fields, "discord_teardown_evidence_invalid")
    if (
        value["schema_version"] != 1
        or value["kind"] != DISCORD_TEARDOWN_KIND
        or value["manifest_sha256"] != context.digest
        or value["run_id"] != context.manifest["run_id"]
        or value["all_resources_absent"] is not True
        or value["active_resources"] != []
        or not isinstance(value["created_resources"], list)
        or not value["created_resources"]
        or value["created_resources"] != value["deleted_resources"]
    ):
        fail("discord_teardown_evidence_invalid")
    _require_timestamp(value["recorded_at"], "discord_teardown_evidence_invalid")
    for field in (
        "source_inventory_digest_sha256",
        "final_inventory_digest_sha256",
        "resource_union_sha256",
        "finalization_freeze_intent_sha256",
        "certification_step15_receipt_sha256",
        "coordinator_step15_completion_sha256",
        "freeze_resource_inventory_digest_sha256",
    ):
        _require_digest(value[field], "discord_teardown_evidence_invalid")
    binding = certified_teardown_binding(context)
    if any(value[field] != binding[field] for field in binding) or value[
        "source_inventory_digest_sha256"
    ] != binding["freeze_resource_inventory_digest_sha256"]:
        fail("discord_teardown_evidence_invalid")
    resources = [_validate_resource(resource) for resource in value["created_resources"]]
    keys = [_resource_key(resource) for resource in resources]
    if len(keys) != len(set(keys)):
        fail("discord_teardown_evidence_invalid")
    expected = {
        "resource_ids": sorted(resource["resource_id"] for resource in resources),
        "message_ids": sorted(
            resource["resource_id"] for resource in resources if resource["kind"] == "message"
        ),
        "channel_ids": sorted(
            resource["resource_id"] for resource in resources if resource["kind"] == "channel"
        ),
        "role_ids": sorted(
            resource["resource_id"] for resource in resources if resource["kind"] == "role"
        ),
    }
    if any(value[field] != expected[field] for field in expected):
        fail("discord_teardown_evidence_invalid")
    if (
        not isinstance(value["proxy_deletions"], list)
        or len(value["proxy_deletions"]) != len(resources)
        or not isinstance(value["direct_observations"], list)
        or len(value["direct_observations"]) != len(resources)
    ):
        fail("discord_teardown_evidence_invalid")
    deleted = []
    for deletion in value["proxy_deletions"]:
        _require_exact(
            deletion,
            {
                "resource_kind",
                "resource_id",
                "channel_id",
                "disposition",
                "http_status",
                "discord_code",
            },
            "discord_teardown_evidence_invalid",
        )
        resource = {
            "kind": deletion["resource_kind"],
            "resource_id": deletion["resource_id"],
        }
        if deletion["resource_kind"] == "message":
            resource["channel_id"] = deletion["channel_id"]
        elif deletion["channel_id"] is not None:
            fail("discord_teardown_evidence_invalid")
        _validate_resource(resource)
        disposition = deletion["disposition"]
        success_status = {"message": 204, "channel": 200, "role": 204}[
            resource["kind"]
        ]
        if disposition in {"preexisting_deleted", "reconciled_deleted"}:
            valid_result = (
                deletion["http_status"] is None and deletion["discord_code"] is None
            )
        elif disposition == "deleted":
            valid_result = (
                deletion["http_status"] == success_status
                and deletion["discord_code"] is None
            )
        elif disposition == "already_absent":
            valid_result = (
                deletion["http_status"] == 404
                and type(deletion["discord_code"]) is int
            )
        else:
            valid_result = False
        if not valid_result:
            fail("discord_teardown_evidence_invalid")
        deleted.append(_resource_key(resource))
    if sorted(deleted) != sorted(keys):
        fail("discord_teardown_evidence_invalid")
    observed = []
    for observation in value["direct_observations"]:
        _require_exact(
            observation,
            {
                "resource_kind",
                "resource_id",
                "channel_id",
                "http_status",
                "discord_code",
                "exists",
            },
            "discord_teardown_evidence_invalid",
        )
        if observation["exists"] is not False:
            fail("discord_teardown_evidence_invalid")
        resource = {
            "kind": observation["resource_kind"],
            "resource_id": observation["resource_id"],
        }
        if observation["resource_kind"] == "message":
            resource["channel_id"] = observation["channel_id"]
        elif observation["channel_id"] is not None:
            fail("discord_teardown_evidence_invalid")
        _validate_resource(resource)
        absent = (
            observation["resource_kind"] == "role"
            and observation["http_status"] == 200
            and observation["discord_code"] is None
        ) or (
            observation["http_status"] == 404
            and observation["discord_code"]
            in {
                "role": {10011},
                "channel": {10003},
                "message": {10003, 10008},
            }[observation["resource_kind"]]
        )
        if not absent:
            fail("discord_teardown_evidence_invalid")
        observed.append(_resource_key(resource))
    if sorted(observed) != sorted(keys):
        fail("discord_teardown_evidence_invalid")
    return value


def validate_destroy_intent(context, value, precleanup, teardown, freeze):
    fields = {
        "schema_version",
        "kind",
        "recorded_at",
        "manifest_sha256",
        "run_id",
        "installation_id",
        "database_name",
        "candidate_launchd_labels",
        "freeze_sha256",
        "transport_quiescence_sha256",
        "precleanup_sha256",
        "discord_teardown_sha256",
        "live_inventory_digest_sha256",
        "database_drop_requested",
        "discord_effects_frozen",
        "zero_blockers_confirmed",
        "zero_child_resources_confirmed",
    }
    _require_exact(value, fields, "database_destroy_intent_invalid")
    labels = sorted(service["label"] for service in context.manifest["services"].values())
    if (
        value["schema_version"] != 1
        or value["kind"] != FINALIZATION_INTENT_KIND
        or value["manifest_sha256"] != context.digest
        or value["run_id"] != context.manifest["run_id"]
        or value["installation_id"] != precleanup["installation_id"]
        or value["database_name"] != "starring_runtime_staging"
        or value["candidate_launchd_labels"] != labels
        or value["freeze_sha256"] != _digest(freeze)
        or value["transport_quiescence_sha256"]
        != freeze["transport_quiescence_sha256"]
        or value["precleanup_sha256"] != _digest(precleanup)
        or value["discord_teardown_sha256"] != _digest(teardown)
        or value["live_inventory_digest_sha256"]
        != teardown["final_inventory_digest_sha256"]
        or value["database_drop_requested"] is not True
        or value["discord_effects_frozen"] is not True
        or value["zero_blockers_confirmed"] is not True
        or value["zero_child_resources_confirmed"] is not True
    ):
        fail("database_destroy_intent_invalid")
    _require_timestamp(value["recorded_at"], "database_destroy_intent_invalid")
    return value


def new_destroy_intent(context, precleanup, teardown, freeze):
    return {
        "schema_version": 1,
        "kind": FINALIZATION_INTENT_KIND,
        "recorded_at": utc_now(),
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "installation_id": precleanup["installation_id"],
        "database_name": "starring_runtime_staging",
        "candidate_launchd_labels": sorted(
            service["label"] for service in context.manifest["services"].values()
        ),
        "freeze_sha256": _digest(freeze),
        "transport_quiescence_sha256": freeze[
            "transport_quiescence_sha256"
        ],
        "precleanup_sha256": _digest(precleanup),
        "discord_teardown_sha256": _digest(teardown),
        "live_inventory_digest_sha256": teardown[
            "final_inventory_digest_sha256"
        ],
        "database_drop_requested": True,
        "discord_effects_frozen": True,
        "zero_blockers_confirmed": True,
        "zero_child_resources_confirmed": True,
    }


def validate_destroy_result(context, value):
    fields = {
        "schema_version",
        "kind",
        "outcome",
        "installation_id",
        "database_absent",
    }
    _require_exact(value, fields, "database_destroy_result_invalid")
    if (
        value["schema_version"] != 1
        or value["kind"] != DATABASE_DESTRUCTION_KIND
        or value["outcome"] not in {"destroyed", "exact_replay"}
        or value["installation_id"]
        != f"installation:{context.manifest['discord']['resource_prefix']}"
        or value["database_absent"] is not True
    ):
        fail("database_destroy_result_invalid")
    return value


def validate_database_absence(context, value):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "run_id",
        "installation_id",
        "database_absent",
    }
    _require_exact(value, fields, "database_absence_evidence_invalid")
    if (
        value["schema_version"] != 1
        or value["kind"] != DATABASE_ABSENCE_KIND
        or value["run_id"] != context.manifest["run_id"]
        or value["installation_id"]
        != f"installation:{context.manifest['discord']['resource_prefix']}"
        or value["database_absent"] is not True
    ):
        fail("database_absence_evidence_invalid")
    _require_timestamp(value["observed_at"], "database_absence_evidence_invalid")
    return value


def _invoke_sealed(context, platform, command, checkpoint=None):
    candidate = context.manifest["candidates"]["sealed_provisioner"]["path"]
    arguments = [candidate, command, "--manifest", context.manifest_path]
    if checkpoint is not None:
        arguments.extend(("--checkpoint", checkpoint))
    result = platform.run(
        arguments,
        timeout=300,
        environment={"PATH": "/usr/bin:/bin:/usr/sbin:/sbin"},
    )
    if result.returncode != 0 or len(result.stdout) > 64 * 1024:
        fail(f"sealed_{command}_failed")
    try:
        return load_json_file_from_bytes(result.stdout)
    except ValueError:
        fail(f"sealed_{command}_output_invalid")


def load_json_file_from_bytes(raw):
    import json

    def strict(pairs):
        value = {}
        for key, nested in pairs:
            if key in value:
                raise ValueError("duplicate_key")
            value[key] = nested
        return value

    try:
        return json.loads(raw, object_pairs_hook=strict)
    except (UnicodeDecodeError, json.JSONDecodeError):
        raise ValueError("json_invalid") from None


def _external_present(context, platform):
    return all(
        platform.keychain_present(service, account)
        for service, account in external_keychain_inventory(context)
    )


def _candidate_services_absent(context, platform):
    return all(
        not platform.launchd_loaded(service["label"])
        for service in context.manifest["services"].values()
    )


def _stop_services(context, platform, names):
    for name in names:
        label = context.manifest["services"][name]["label"]
        if not platform.launchd_loaded(label):
            continue
        append_journal(context, "finalization_bootout", "intent", label)
        platform.launchd_bootout(label)
        if platform.launchd_loaded(label):
            append_journal(context, "finalization_bootout", "failed", label)
            fail("candidate_service_stop_incomplete")
        append_journal(context, "finalization_bootout", "complete", label)


def _pinned_transport_instance_id(context):
    evidence = _load_private(
        context.artifact_directory / "step-03-evidence.json",
        "transport_instance_evidence",
    )
    value = evidence.get("transport_instance_id") if isinstance(evidence, dict) else None
    if not isinstance(value, str) or not value:
        fail("transport_instance_evidence_invalid")
    return value


def require_transport_quiescent(context, platform, expected_instance_id):
    snapshot = platform.transport_control(context, "snapshot")
    if not isinstance(snapshot, dict) or snapshot.get("instance_id") != expected_instance_id:
        fail("transport_instance_changed")
    gateway = snapshot.get("gateway")
    effect = snapshot.get("effect_http")
    if not isinstance(gateway, dict) or not isinstance(effect, dict):
        fail("finalization_transport_quiescence_invalid")
    projection = {
        "gateway_partitioned": gateway.get("partitioned"),
        "duplicate_armed": gateway.get("duplicate_armed"),
        "armed_duplicate_operation_id": gateway.get("armed_duplicate_operation_id"),
        "duplicate_claimed": gateway.get("duplicate_claimed"),
        "claimed_duplicate_operation_id": gateway.get(
            "claimed_duplicate_operation_id"
        ),
        "indeterminate_armed": effect.get("indeterminate_armed"),
        "armed_indeterminate_operation_id": effect.get(
            "armed_indeterminate_operation_id"
        ),
        "indeterminate_claimed": effect.get("indeterminate_claimed"),
        "claimed_indeterminate_operation_id": effect.get(
            "claimed_indeterminate_operation_id"
        ),
    }
    if projection != {
        "gateway_partitioned": False,
        "duplicate_armed": False,
        "armed_duplicate_operation_id": None,
        "duplicate_claimed": False,
        "claimed_duplicate_operation_id": None,
        "indeterminate_armed": False,
        "armed_indeterminate_operation_id": None,
        "indeterminate_claimed": False,
        "claimed_indeterminate_operation_id": None,
    }:
        fail("finalization_transport_quiescence_invalid")
    return _digest(projection)


def require_certification_prefix(context):
    import d2_run

    receipts = d2_run.load_receipts(
        context.manifest_path, context.manifest, context.digest
    )
    if len(receipts) not in {15, 16, 17}:
        fail("finalization_certification_prefix_incomplete")
    pending = d2_run.coordinator_pending_step(
        context.manifest_path, context.manifest, context.digest, receipts
    )
    if pending is not None and pending <= 15:
        fail("finalization_certification_prefix_incomplete")
    completion = None
    for step in range(1, 16):
        path = d2_run.coordinator_completion_path(context.manifest_path, step)
        if not d2_run.path_present(path):
            fail("finalization_certification_prefix_incomplete")
        completion = d2_run.validate_completion(
            d2_run.load_coordinator_record(path, "coordinator_completion"),
            context.manifest,
            context.digest,
            step,
        )
        if completion["receipt_sha256"] != receipts[step - 1]["receipt_sha256"]:
            fail("finalization_certification_prefix_invalid")
    return {
        "step15_receipt_sha256": receipts[14]["receipt_sha256"],
        "step15_completion_sha256": _digest(completion),
        "step15_completed_at": completion["observed_at"],
        "reconciliation_inventory_digest_sha256": receipts[12]["evidence"][
            "reconciliation_inventory_digest_sha256"
        ],
    }


def require_certification_step_sixteen(context):
    import d2_run

    receipts = d2_run.load_receipts(
        context.manifest_path, context.manifest, context.digest
    )
    if len(receipts) not in {16, 17}:
        fail("total_absence_step16_completion_missing")
    d2_run.coordinator_pending_step(
        context.manifest_path, context.manifest, context.digest, receipts
    )
    path = d2_run.coordinator_completion_path(context.manifest_path, 16)
    if not d2_run.path_present(path):
        fail("total_absence_step16_completion_missing")
    completion = d2_run.validate_completion(
        d2_run.load_coordinator_record(path, "coordinator_completion"),
        context.manifest,
        context.digest,
        16,
    )
    if completion["receipt_sha256"] != receipts[15]["receipt_sha256"]:
        fail("total_absence_step16_completion_invalid")
    return {
        "step16_receipt_sha256": receipts[15]["receipt_sha256"],
        "step16_completion_sha256": _digest(completion),
        "step16_completed_at": completion["observed_at"],
    }


def validate_freeze_intent(context, value):
    fields = {
        "schema_version",
        "kind",
        "recorded_at",
        "manifest_sha256",
        "run_id",
        "transport_instance_id",
        "certification_step15_receipt_sha256",
        "coordinator_step15_completion_sha256",
        "transport_quiescence_sha256",
        "resource_inventory_digest_sha256",
        "services_to_stop",
        "discord_effects_frozen",
    }
    _require_exact(value, fields, "finalization_freeze_intent_invalid")
    if (
        value["schema_version"] != 1
        or value["kind"] != FINALIZATION_FREEZE_KIND
        or value["manifest_sha256"] != context.digest
        or value["run_id"] != context.manifest["run_id"]
        or value["transport_instance_id"] != _pinned_transport_instance_id(context)
        or value["services_to_stop"] != list(FREEZE_STOP_ORDER)
        or value["discord_effects_frozen"] is not True
    ):
        fail("finalization_freeze_intent_invalid")
    _require_digest(
        value["certification_step15_receipt_sha256"],
        "finalization_freeze_intent_invalid",
    )
    _require_digest(
        value["coordinator_step15_completion_sha256"],
        "finalization_freeze_intent_invalid",
    )
    _require_digest(
        value["transport_quiescence_sha256"],
        "finalization_freeze_intent_invalid",
    )
    _require_digest(
        value["resource_inventory_digest_sha256"],
        "finalization_freeze_intent_invalid",
    )
    _require_timestamp(value["recorded_at"], "finalization_freeze_intent_invalid")
    return value


def validate_freeze_certification(intent, certification):
    if (
        not isinstance(certification, dict)
        or set(certification)
        != {
            "step15_receipt_sha256",
            "step15_completion_sha256",
            "step15_completed_at",
            "reconciliation_inventory_digest_sha256",
        }
        or intent["certification_step15_receipt_sha256"]
        != certification["step15_receipt_sha256"]
        or intent["coordinator_step15_completion_sha256"]
        != certification["step15_completion_sha256"]
        or intent["resource_inventory_digest_sha256"]
        != certification["reconciliation_inventory_digest_sha256"]
    ):
        fail("finalization_freeze_certification_invalid")
    _require_digest(
        certification["step15_receipt_sha256"],
        "finalization_freeze_certification_invalid",
    )
    _require_digest(
        certification["step15_completion_sha256"],
        "finalization_freeze_certification_invalid",
    )
    _require_digest(
        certification["reconciliation_inventory_digest_sha256"],
        "finalization_freeze_certification_invalid",
    )
    if _parse_timestamp(
        certification["step15_completed_at"],
        "finalization_freeze_certification_invalid",
    ) > _parse_timestamp(
        intent["recorded_at"], "finalization_freeze_certification_invalid"
    ):
        fail("finalization_freeze_chronology_invalid")
    return intent


def certified_teardown_binding(context):
    freeze = validate_freeze_intent(
        context,
        _load_private(freeze_intent_path(context), "finalization_freeze_intent"),
    )
    return {
        "finalization_freeze_intent_sha256": _digest(freeze),
        "certification_step15_receipt_sha256": freeze[
            "certification_step15_receipt_sha256"
        ],
        "coordinator_step15_completion_sha256": freeze[
            "coordinator_step15_completion_sha256"
        ],
        "freeze_resource_inventory_digest_sha256": freeze[
            "resource_inventory_digest_sha256"
        ],
    }


def _initial_freeze_boundary(context, platform):
    state = load_state(context, {"candidate_started"})
    validate_mutation_roots(context)
    if not platform.postgres_running(context.cluster_root) or any(
        not platform.launchd_loaded(context.manifest["services"][name]["label"])
        for name in FREEZE_STOP_ORDER + FINAL_STOP_ORDER
    ):
        fail("finalization_freeze_state_drift")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    snapshot = platform.transport_control(context, "snapshot")
    instance_id = snapshot.get("instance_id") if isinstance(snapshot, dict) else None
    if instance_id != _pinned_transport_instance_id(context):
        fail("transport_instance_changed")
    quiescence_sha256 = require_transport_quiescent(context, platform, instance_id)
    inventory = platform.transport_control(context, "resource_inventory")
    if (
        not isinstance(inventory, dict)
        or inventory.get("instance_id") != instance_id
        or not isinstance(inventory.get("digest_sha256"), str)
        or not DIGEST_PATTERN.fullmatch(inventory["digest_sha256"])
    ):
        fail("finalization_freeze_inventory_invalid")
    return state, instance_id, quiescence_sha256, inventory["digest_sha256"]


def _ensure_effect_freeze(context, platform):
    ensure_finalization_directory(context)
    certification = require_certification_prefix(context)
    path = freeze_intent_path(context)
    if path.exists():
        intent = validate_freeze_intent(
            context, _load_private(path, "finalization_freeze_intent")
        )
        validate_freeze_certification(intent, certification)
        if any(
            platform.launchd_loaded(context.manifest["services"][name]["label"])
            for name in FREEZE_STOP_ORDER
        ):
            validate_mutation_roots(context)
    else:
        (
            _state,
            instance_id,
            quiescence_sha256,
            resource_inventory_digest_sha256,
        ) = _initial_freeze_boundary(context, platform)
        intent = {
            "schema_version": 1,
            "kind": FINALIZATION_FREEZE_KIND,
            "recorded_at": utc_now(),
            "manifest_sha256": context.digest,
            "run_id": context.manifest["run_id"],
            "transport_instance_id": instance_id,
            "certification_step15_receipt_sha256": certification[
                "step15_receipt_sha256"
            ],
            "coordinator_step15_completion_sha256": certification[
                "step15_completion_sha256"
            ],
            "transport_quiescence_sha256": quiescence_sha256,
            "resource_inventory_digest_sha256": resource_inventory_digest_sha256,
            "services_to_stop": list(FREEZE_STOP_ORDER),
            "discord_effects_frozen": True,
        }
        validate_freeze_intent(context, intent)
        validate_freeze_certification(intent, certification)
        write_atomic(path, canonical_json(intent) + "\n")
    _stop_services(context, platform, FREEZE_STOP_ORDER)
    if any(
        platform.launchd_loaded(context.manifest["services"][name]["label"])
        for name in FREEZE_STOP_ORDER
    ):
        fail("finalization_freeze_incomplete")
    return intent


def _load_teardown(context):
    path = context.artifact_directory / "discord-resource-teardown-evidence.json"
    return validate_discord_teardown(
        context, _load_private(path, "discord_resource_teardown_evidence")
    )


def require_live_teardown_inventory(context, platform, teardown):
    inventory = platform.transport_control(context, "resource_inventory")
    if (
        inventory["instance_id"] != teardown["transport_instance_id"]
        or inventory["digest_sha256"]
        != teardown["final_inventory_digest_sha256"]
        or inventory["created"] != teardown["created_resources"]
        or inventory["deleted"] != teardown["deleted_resources"]
        or inventory["active"] != []
    ):
        fail("discord_teardown_live_inventory_drift")
    return inventory


def _load_or_capture_precleanup(context, platform):
    path = precleanup_path(context)
    if path.exists():
        return validate_precleanup(
            context, _load_private(path, "database_precleanup_evidence")
        )
    evidence = validate_precleanup(
        context, _invoke_sealed(context, platform, "inspect", "precleanup")
    )
    write_atomic(path, canonical_json(evidence) + "\n")
    return evidence


def _validate_cleanup(context, value):
    fields = {
        "schema_version",
        "manifest_sha256",
        "observed_at",
        "database_absent",
        "postgres_process_absent",
        "launchd_jobs_absent",
        "keychain_items_absent",
        "isolated_root_absent",
        "protected_staging_unchanged",
    }
    _require_exact(value, fields, "cleanup_evidence_invalid")
    if (
        value["schema_version"] != 1
        or value["manifest_sha256"] != context.digest
        or any(
            value[field] is not True
            for field in fields
            - {"schema_version", "manifest_sha256", "observed_at"}
        )
    ):
        fail("cleanup_evidence_invalid")
    _require_timestamp(value["observed_at"], "cleanup_evidence_invalid")
    return value


def _cleanup_path(context):
    return context.artifact_directory / "cleanup-evidence.json"


def _load_database_artifacts(context):
    ensure_finalization_directory(context)
    freeze = validate_freeze_intent(
        context, _load_private(freeze_intent_path(context), "finalization_freeze_intent")
    )
    precleanup = validate_precleanup(
        context, _load_private(precleanup_path(context), "database_precleanup_evidence")
    )
    teardown = _load_teardown(context)
    intent = validate_destroy_intent(
        context,
        _load_private(destroy_intent_path(context), "database_destroy_intent"),
        precleanup,
        teardown,
        freeze,
    )
    result = validate_destroy_result(
        context, _load_private(destroy_result_path(context), "database_destroy_result")
    )
    absence = validate_database_absence(
        context, _load_private(database_absence_path(context), "database_absence_evidence")
    )
    return precleanup, teardown, intent, result, absence


def command_finalize_database(context, platform):
    require_certification_eligible_teardown(context)
    ensure_finalization_directory(context)
    state = load_state(context)
    if state["phase"] not in {"candidate_started", "cleaned"}:
        fail("finalization_phase_invalid")
    cleanup_exists = _cleanup_path(context).exists()
    if database_absence_path(context).exists():
        precleanup, teardown, _intent, result, absence = _load_database_artifacts(context)
        if cleanup_exists:
            _validate_cleanup(
                context, _load_private(_cleanup_path(context), "cleanup_evidence")
            )
        elif platform.postgres_running(context.cluster_root):
            replayed = validate_database_absence(
                context, _invoke_sealed(context, platform, "inspect", "absence")
            )
            if replayed["run_id"] != absence["run_id"]:
                fail("database_absence_replay_mismatch")
        if not _candidate_services_absent(context, platform):
            fail("candidate_services_still_loaded")
        if standing_snapshot(context, platform) != state["standing_snapshot"]:
            fail("protected_staging_state_changed")
        if not _external_present(context, platform):
            fail("external_keychain_identity_absent")
        return {
            "status": "exact_replay",
            "phase": "database_absent",
            "database_absent": True,
            "services_stopped": True,
            "destroy_outcome": result["outcome"],
            "discord_resource_count": len(teardown["resource_ids"]),
            "zero_blockers_confirmed": precleanup["ready_for_cleanup"],
        }
    if state["phase"] != "candidate_started":
        fail("finalization_artifacts_incomplete")
    validate_mutation_roots(context)
    if not platform.postgres_running(context.cluster_root):
        fail("candidate_database_unavailable")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    if not _external_present(context, platform):
        fail("external_keychain_identity_absent")
    freeze = _ensure_effect_freeze(context, platform)
    teardown = _load_teardown(context)
    intent_path = destroy_intent_path(context)
    if intent_path.exists():
        precleanup = validate_precleanup(
            context,
            _load_private(precleanup_path(context), "database_precleanup_evidence"),
        )
        intent = validate_destroy_intent(
            context,
            _load_private(intent_path, "database_destroy_intent"),
            precleanup,
            teardown,
            freeze,
        )
    else:
        transport_label = context.manifest["services"]["transport"]["label"]
        if not platform.launchd_loaded(transport_label):
            fail("finalization_transport_stopped_before_freeze_proof")
        require_live_teardown_inventory(context, platform, teardown)
        _stop_services(context, platform, ("api", "worker"))
        if any(
            platform.launchd_loaded(context.manifest["services"][name]["label"])
            for name in ("api", "worker")
        ):
            fail("candidate_service_stop_incomplete")
        precleanup = _load_or_capture_precleanup(context, platform)
        require_live_teardown_inventory(context, platform, teardown)
        if require_transport_quiescent(
            context, platform, freeze["transport_instance_id"]
        ) != freeze["transport_quiescence_sha256"]:
            fail("finalization_transport_quiescence_drift")
        intent = new_destroy_intent(context, precleanup, teardown, freeze)
        append_journal(context, "database_destroy", "intent", "database")
        write_atomic(intent_path, canonical_json(intent) + "\n")
    _stop_services(context, platform, ("api", "worker"))
    if platform.launchd_loaded(context.manifest["services"]["transport"]["label"]):
        require_live_teardown_inventory(context, platform, teardown)
    _stop_services(context, platform, ("transport",))
    if not _candidate_services_absent(context, platform):
        fail("candidate_service_stop_incomplete")
    if not platform.postgres_running(context.cluster_root):
        fail("postgres_stopped_before_database_destroy")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    if not _external_present(context, platform):
        fail("external_keychain_identity_absent")
    result_path = destroy_result_path(context)
    if result_path.exists():
        result = validate_destroy_result(
            context, _load_private(result_path, "database_destroy_result")
        )
    else:
        result = validate_destroy_result(
            context, _invoke_sealed(context, platform, "destroy")
        )
        write_atomic(result_path, canonical_json(result) + "\n")
        append_journal(context, "database_destroy", "complete", "database")
    absence = validate_database_absence(
        context, _invoke_sealed(context, platform, "inspect", "absence")
    )
    write_atomic(database_absence_path(context), canonical_json(absence) + "\n")
    if not platform.postgres_running(context.cluster_root):
        fail("postgres_stopped_during_database_destroy")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    if not _external_present(context, platform):
        fail("external_keychain_identity_absent")
    return {
        "status": "database_destroyed",
        "phase": "database_absent",
        "database_absent": True,
        "services_stopped": True,
        "destroy_outcome": result["outcome"],
        "discord_resource_count": len(teardown["resource_ids"]),
        "zero_blockers_confirmed": precleanup["ready_for_cleanup"],
    }


def validate_finalization(context, value, precleanup, teardown, absence, cleanup):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "manifest_sha256",
        "run_id",
        "installation_id",
        "precleanup_sha256",
        "database_absence_sha256",
        "cleanup_sha256",
        "discord_teardown_sha256",
        "database_drop_requested",
        "database_absent",
        "services_stopped",
        "postgres_process_absent",
        "candidate_launchd_jobs_absent",
        "run_keychain_items_absent",
        "isolated_root_absent",
        "protected_staging_unchanged",
        "external_credentials_preserved",
        "discord_resource_ids_deleted",
        "discord_active_resource_count",
    }
    _require_exact(value, fields, "orchestrator_finalization_evidence_invalid")
    if (
        value["schema_version"] != 1
        or value["kind"] != FINALIZATION_KIND
        or value["manifest_sha256"] != context.digest
        or value["run_id"] != context.manifest["run_id"]
        or value["installation_id"] != absence["installation_id"]
        or value["precleanup_sha256"] != _digest(precleanup)
        or value["database_absence_sha256"] != _digest(absence)
        or value["cleanup_sha256"] != _digest(cleanup)
        or value["discord_teardown_sha256"] != _digest(teardown)
        or any(
            value[field] is not True
            for field in (
                "database_drop_requested",
                "database_absent",
                "services_stopped",
                "postgres_process_absent",
                "candidate_launchd_jobs_absent",
                "run_keychain_items_absent",
                "isolated_root_absent",
                "protected_staging_unchanged",
                "external_credentials_preserved",
            )
        )
        or value["discord_resource_ids_deleted"] != teardown["resource_ids"]
        or value["discord_active_resource_count"] != 0
    ):
        fail("orchestrator_finalization_evidence_invalid")
    _require_timestamp(value["observed_at"], "orchestrator_finalization_evidence_invalid")
    return value


def new_finalization_evidence(context, precleanup, teardown, absence, cleanup):
    return {
        "schema_version": 1,
        "kind": FINALIZATION_KIND,
        "observed_at": utc_now(),
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "installation_id": absence["installation_id"],
        "precleanup_sha256": _digest(precleanup),
        "database_absence_sha256": _digest(absence),
        "cleanup_sha256": _digest(cleanup),
        "discord_teardown_sha256": _digest(teardown),
        "database_drop_requested": True,
        "database_absent": True,
        "services_stopped": True,
        "postgres_process_absent": True,
        "candidate_launchd_jobs_absent": True,
        "run_keychain_items_absent": True,
        "isolated_root_absent": True,
        "protected_staging_unchanged": True,
        "external_credentials_preserved": True,
        "discord_resource_ids_deleted": teardown["resource_ids"],
        "discord_active_resource_count": 0,
    }


def _validate_precleanup_source(value):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "installation_id",
        "scoped_installation_count",
        "scoped_deployment_count",
        "terminal_product_operation_count",
        "unresolved_product_operation_count",
        "unresolved_receipt_count",
        "unresolved_journal_entry_count",
        "unresolved_rollback_count",
        "ready_for_cleanup",
    }
    _require_exact(value, fields, "step16_database_source_invalid")
    if (
        value["schema_version"] != 1
        or value["kind"] != PRECLEANUP_KIND
        or not isinstance(value["installation_id"], str)
        or not value["installation_id"].startswith("installation:")
        or type(value["scoped_installation_count"]) is not int
        or value["scoped_installation_count"] != 1
        or type(value["scoped_deployment_count"]) is not int
        or value["scoped_deployment_count"] <= 0
        or type(value["terminal_product_operation_count"]) is not int
        or value["terminal_product_operation_count"] < 0
        or value["ready_for_cleanup"] is not True
    ):
        fail("step16_database_source_invalid")
    _require_timestamp(value["observed_at"], "step16_database_source_invalid")
    for field in (
        "unresolved_product_operation_count",
        "unresolved_receipt_count",
        "unresolved_journal_entry_count",
        "unresolved_rollback_count",
    ):
        if type(value[field]) is not int or value[field] != 0:
            fail("step16_database_source_invalid")
    return value


def _validate_teardown_source(value):
    fields = {
        "schema_version",
        "kind",
        "manifest_sha256",
        "run_id",
        "recorded_at",
        "transport_instance_id",
        "source_inventory_digest_sha256",
        "final_inventory_digest_sha256",
        "resource_union_sha256",
        "created_resources",
        "deleted_resources",
        "active_resources",
        "resource_ids",
        "message_ids",
        "channel_ids",
        "role_ids",
        "proxy_deletions",
        "direct_observations",
        "all_resources_absent",
        "finalization_freeze_intent_sha256",
        "certification_step15_receipt_sha256",
        "coordinator_step15_completion_sha256",
        "freeze_resource_inventory_digest_sha256",
    }
    _require_exact(value, fields, "step16_teardown_source_invalid")
    if (
        value["schema_version"] != 1
        or value["kind"] != DISCORD_TEARDOWN_KIND
        or not isinstance(value["run_id"], str)
        or not value["run_id"]
        or not isinstance(value["transport_instance_id"], str)
        or not value["transport_instance_id"]
        or value["all_resources_absent"] is not True
        or value["active_resources"] != []
        or not isinstance(value["created_resources"], list)
        or not value["created_resources"]
        or value["deleted_resources"] != value["created_resources"]
        or not isinstance(value["proxy_deletions"], list)
        or len(value["proxy_deletions"]) != len(value["created_resources"])
        or not isinstance(value["direct_observations"], list)
        or len(value["direct_observations"]) != len(value["created_resources"])
    ):
        fail("step16_teardown_source_invalid")
    _require_timestamp(value["recorded_at"], "step16_teardown_source_invalid")
    for field in (
        "manifest_sha256",
        "source_inventory_digest_sha256",
        "final_inventory_digest_sha256",
        "resource_union_sha256",
        "finalization_freeze_intent_sha256",
        "certification_step15_receipt_sha256",
        "coordinator_step15_completion_sha256",
        "freeze_resource_inventory_digest_sha256",
    ):
        _require_digest(value[field], "step16_teardown_source_invalid")
    resources = [_validate_resource(resource) for resource in value["created_resources"]]
    if (
        value["source_inventory_digest_sha256"]
        != value["freeze_resource_inventory_digest_sha256"]
    ):
        fail("step16_teardown_source_invalid")
    resource_keys = [_resource_key(resource) for resource in resources]
    if (
        len(resource_keys) != len(set(resource_keys))
        or value["resource_union_sha256"] != _digest(resources)
    ):
        fail("step16_teardown_source_invalid")
    expected = {
        "resource_ids": sorted(resource["resource_id"] for resource in resources),
        "message_ids": sorted(
            resource["resource_id"]
            for resource in resources
            if resource["kind"] == "message"
        ),
        "channel_ids": sorted(
            resource["resource_id"]
            for resource in resources
            if resource["kind"] == "channel"
        ),
        "role_ids": sorted(
            resource["resource_id"]
            for resource in resources
            if resource["kind"] == "role"
        ),
    }
    if any(value[field] != expected[field] for field in expected):
        fail("step16_teardown_source_invalid")
    deleted_keys = []
    for deletion in value["proxy_deletions"]:
        _require_exact(
            deletion,
            {
                "resource_kind",
                "resource_id",
                "channel_id",
                "disposition",
                "http_status",
                "discord_code",
            },
            "step16_teardown_source_invalid",
        )
        resource = {
            "kind": deletion["resource_kind"],
            "resource_id": deletion["resource_id"],
        }
        if deletion["resource_kind"] == "message":
            resource["channel_id"] = deletion["channel_id"]
        elif deletion["channel_id"] is not None:
            fail("step16_teardown_source_invalid")
        _validate_resource(resource)
        success = {"message": 204, "channel": 200, "role": 204}[resource["kind"]]
        unknown = {
            "message": {10003, 10008},
            "channel": {10003},
            "role": {10011},
        }[resource["kind"]]
        disposition = deletion["disposition"]
        if disposition in {"preexisting_deleted", "reconciled_deleted"}:
            valid = deletion["http_status"] is None and deletion["discord_code"] is None
        elif disposition == "deleted":
            valid = deletion["http_status"] == success and deletion["discord_code"] is None
        elif disposition == "already_absent":
            valid = deletion["http_status"] == 404 and deletion["discord_code"] in unknown
        else:
            valid = False
        if not valid:
            fail("step16_teardown_source_invalid")
        deleted_keys.append(_resource_key(resource))
    if sorted(deleted_keys) != sorted(resource_keys):
        fail("step16_teardown_source_invalid")
    observed_keys = []
    for observation in value["direct_observations"]:
        _require_exact(
            observation,
            {
                "resource_kind",
                "resource_id",
                "channel_id",
                "http_status",
                "discord_code",
                "exists",
            },
            "step16_teardown_source_invalid",
        )
        resource = {
            "kind": observation["resource_kind"],
            "resource_id": observation["resource_id"],
        }
        if observation["resource_kind"] == "message":
            resource["channel_id"] = observation["channel_id"]
        elif observation["channel_id"] is not None:
            fail("step16_teardown_source_invalid")
        _validate_resource(resource)
        unknown = {
            "message": {10003, 10008},
            "channel": {10003},
            "role": {10011},
        }[resource["kind"]]
        absent = observation["exists"] is False and (
            (
                resource["kind"] == "role"
                and observation["http_status"] == 200
                and observation["discord_code"] is None
            )
            or (
                observation["http_status"] == 404
                and observation["discord_code"] in unknown
            )
        )
        if not absent:
            fail("step16_teardown_source_invalid")
        observed_keys.append(_resource_key(resource))
    if sorted(observed_keys) != sorted(resource_keys):
        fail("step16_teardown_source_invalid")
    return value


def _validate_finalization_source(value):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "manifest_sha256",
        "run_id",
        "installation_id",
        "precleanup_sha256",
        "database_absence_sha256",
        "cleanup_sha256",
        "discord_teardown_sha256",
        "database_drop_requested",
        "database_absent",
        "services_stopped",
        "postgres_process_absent",
        "candidate_launchd_jobs_absent",
        "run_keychain_items_absent",
        "isolated_root_absent",
        "protected_staging_unchanged",
        "external_credentials_preserved",
        "discord_resource_ids_deleted",
        "discord_active_resource_count",
    }
    _require_exact(value, fields, "step16_finalization_source_invalid")
    if (
        value["schema_version"] != 1
        or value["kind"] != FINALIZATION_KIND
        or not isinstance(value["run_id"], str)
        or not value["run_id"]
        or not isinstance(value["installation_id"], str)
        or not value["installation_id"].startswith("installation:")
        or any(
            value[field] is not True
            for field in (
                "database_drop_requested",
                "database_absent",
                "services_stopped",
                "postgres_process_absent",
                "candidate_launchd_jobs_absent",
                "run_keychain_items_absent",
                "isolated_root_absent",
                "protected_staging_unchanged",
                "external_credentials_preserved",
            )
        )
        or value["discord_active_resource_count"] != 0
        or type(value["discord_active_resource_count"]) is not int
        or not isinstance(value["discord_resource_ids_deleted"], list)
        or not value["discord_resource_ids_deleted"]
    ):
        fail("step16_finalization_source_invalid")
    _require_timestamp(value["observed_at"], "step16_finalization_source_invalid")
    for field in (
        "manifest_sha256",
        "precleanup_sha256",
        "database_absence_sha256",
        "cleanup_sha256",
        "discord_teardown_sha256",
    ):
        _require_digest(value[field], "step16_finalization_source_invalid")
    for resource_id in value["discord_resource_ids_deleted"]:
        _require_snowflake(resource_id, "step16_finalization_source_invalid")
    if len(value["discord_resource_ids_deleted"]) != len(
        set(value["discord_resource_ids_deleted"])
    ):
        fail("step16_finalization_source_invalid")
    return value


def _validate_step16_binding(value):
    fields = {
        "manifest_sha256",
        "run_id",
        "installation_id",
        "transport_instance_id",
        "expected_discord_resource_ids",
        "reconciliation_inventory_digest_sha256",
        "finalization_freeze_intent_sha256",
        "certification_step15_receipt_sha256",
        "coordinator_step15_completion_sha256",
    }
    _require_exact(value, fields, "step16_binding_invalid")
    _require_digest(value["manifest_sha256"], "step16_binding_invalid")
    _require_digest(
        value["reconciliation_inventory_digest_sha256"],
        "step16_binding_invalid",
    )
    _require_digest(
        value["finalization_freeze_intent_sha256"],
        "step16_binding_invalid",
    )
    _require_digest(
        value["certification_step15_receipt_sha256"],
        "step16_binding_invalid",
    )
    _require_digest(
        value["coordinator_step15_completion_sha256"],
        "step16_binding_invalid",
    )
    if (
        not isinstance(value["run_id"], str)
        or not value["run_id"]
        or not isinstance(value["installation_id"], str)
        or not value["installation_id"].startswith("installation:")
        or not isinstance(value["transport_instance_id"], str)
        or not value["transport_instance_id"]
        or not isinstance(value["expected_discord_resource_ids"], list)
        or not value["expected_discord_resource_ids"]
    ):
        fail("step16_binding_invalid")
    for resource_id in value["expected_discord_resource_ids"]:
        _require_snowflake(resource_id, "step16_binding_invalid")
    if (
        value["expected_discord_resource_ids"]
        != sorted(value["expected_discord_resource_ids"])
        or len(value["expected_discord_resource_ids"])
        != len(set(value["expected_discord_resource_ids"]))
    ):
        fail("step16_binding_invalid")
    return value


def assemble_teardown_evidence(database, teardown, finalization, binding):
    database = _validate_precleanup_source(database)
    teardown = _validate_teardown_source(teardown)
    finalization = _validate_finalization_source(finalization)
    binding = _validate_step16_binding(binding)
    if (
        finalization["manifest_sha256"] != binding["manifest_sha256"]
        or teardown["manifest_sha256"] != binding["manifest_sha256"]
        or finalization["run_id"] != binding["run_id"]
        or teardown["run_id"] != binding["run_id"]
        or finalization["installation_id"] != binding["installation_id"]
        or database["installation_id"] != binding["installation_id"]
        or teardown["transport_instance_id"] != binding["transport_instance_id"]
        or teardown["resource_ids"] != binding["expected_discord_resource_ids"]
        or teardown["source_inventory_digest_sha256"]
        != binding["reconciliation_inventory_digest_sha256"]
        or teardown["finalization_freeze_intent_sha256"]
        != binding["finalization_freeze_intent_sha256"]
        or teardown["certification_step15_receipt_sha256"]
        != binding["certification_step15_receipt_sha256"]
        or teardown["coordinator_step15_completion_sha256"]
        != binding["coordinator_step15_completion_sha256"]
        or finalization["precleanup_sha256"] != _digest(database)
        or finalization["discord_teardown_sha256"] != _digest(teardown)
        or finalization["discord_resource_ids_deleted"] != teardown["resource_ids"]
    ):
        fail("step16_source_evidence_invalid")
    return {
        "teardown_started": True,
        "discord_resource_ids_deleted": teardown["resource_ids"],
        "database_drop_requested": True,
        "services_stopped": True,
        "precleanup_sha256": _digest(database),
        "discord_teardown_sha256": _digest(teardown),
    }


def local_step16_binding(context, teardown):
    return {
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "installation_id": (
            f"installation:{context.manifest['discord']['resource_prefix']}"
        ),
        "transport_instance_id": teardown["transport_instance_id"],
        "expected_discord_resource_ids": teardown["resource_ids"],
        "reconciliation_inventory_digest_sha256": teardown[
            "source_inventory_digest_sha256"
        ],
        "finalization_freeze_intent_sha256": teardown[
            "finalization_freeze_intent_sha256"
        ],
        "certification_step15_receipt_sha256": teardown[
            "certification_step15_receipt_sha256"
        ],
        "coordinator_step15_completion_sha256": teardown[
            "coordinator_step15_completion_sha256"
        ],
    }


def command_finalize_run(
    context, platform, cleanup_boundary, teardown_boundary
):
    require_certification_eligible_teardown(context)
    ensure_finalization_directory(context)
    if not _external_present(context, platform):
        fail("external_keychain_identity_absent")
    _ensure_effect_freeze(context, platform)
    step_path = step_sixteen_evidence_path(context)
    if step_path.exists():
        precleanup, teardown, _intent, _result, absence = _load_database_artifacts(context)
        cleanup = _validate_cleanup(
            context, _load_private(_cleanup_path(context), "cleanup_evidence")
        )
        finalization = validate_finalization(
            context,
            _load_private(finalization_evidence_path(context), "orchestrator_finalization"),
            precleanup,
            teardown,
            absence,
            cleanup,
        )
        step = _load_private(step_path, "step_16_evidence")
        if step != assemble_teardown_evidence(
            precleanup,
            teardown,
            finalization,
            local_step16_binding(context, teardown),
        ):
            fail("step16_evidence_replay_mismatch")
        if not _candidate_services_absent(context, platform):
            fail("candidate_services_still_loaded")
        if any(
            platform.keychain_present(service, account)
            for service, account in keychain_inventory(context)
        ):
            fail("run_keychain_items_still_present")
        if _filesystem_entry_present(
            context.root, "isolated_runtime_absence_invalid"
        ) or platform.postgres_running(context.cluster_root):
            fail("isolated_runtime_still_present")
        if not _external_present(context, platform):
            fail("external_keychain_identity_absent")
        return {"status": "exact_replay", "phase": "cleaned", "step": 16}
    if not _external_present(context, platform):
        fail("external_keychain_identity_absent")
    teardown_path = context.artifact_directory / "discord-resource-teardown-evidence.json"
    if not teardown_path.exists():
        teardown_boundary(context, platform, frozen=True)
    command_finalize_database(context, platform)
    state = load_state(context)
    if not _cleanup_path(context).exists():
        if _filesystem_entry_present(
            context.root, "isolated_runtime_absence_invalid"
        ):
            validate_mutation_roots(context)
        cleanup_boundary(context, platform)
    cleanup = _validate_cleanup(
        context, _load_private(_cleanup_path(context), "cleanup_evidence")
    )
    if not _candidate_services_absent(context, platform):
        fail("candidate_services_still_loaded")
    if platform.postgres_running(context.cluster_root) or _filesystem_entry_present(
        context.root, "isolated_runtime_absence_invalid"
    ):
        fail("isolated_runtime_still_present")
    if any(
        platform.keychain_present(service, account)
        for service, account in keychain_inventory(context)
    ):
        fail("run_keychain_items_still_present")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    if not _external_present(context, platform):
        fail("external_keychain_identity_absent")
    precleanup, teardown, _intent, _result, absence = _load_database_artifacts(context)
    finalization_path = finalization_evidence_path(context)
    if finalization_path.exists():
        finalization = validate_finalization(
            context,
            _load_private(finalization_path, "orchestrator_finalization"),
            precleanup,
            teardown,
            absence,
            cleanup,
        )
    else:
        finalization = new_finalization_evidence(
            context, precleanup, teardown, absence, cleanup
        )
        validate_finalization(
            context, finalization, precleanup, teardown, absence, cleanup
        )
        write_atomic(finalization_path, canonical_json(finalization) + "\n")
    step = assemble_teardown_evidence(
        precleanup,
        teardown,
        finalization,
        local_step16_binding(context, teardown),
    )
    write_atomic(step_path, canonical_json(step) + "\n")
    return {
        "status": "finalized",
        "phase": "cleaned",
        "step": 16,
        "database_absent": True,
        "services_stopped": True,
        "discord_resource_count": len(teardown["resource_ids"]),
    }


def validate_prefix_scan(context, value):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "guild_id",
        "resource_prefix",
        "guild_observation_http_status",
        "role_count",
        "channel_count",
        "panel_count",
        "resource_prefix_match_count",
    }
    _require_exact(value, fields, "discord_prefix_scan_evidence_invalid")
    if (
        value["schema_version"] != 1
        or value["kind"] != PREFIX_SCAN_KIND
        or value["guild_id"] != context.manifest["discord"]["guild_id"]
        or value["resource_prefix"] != context.manifest["discord"]["resource_prefix"]
        or value["guild_observation_http_status"] != 200
        or any(
            type(value[field]) is not int or value[field] != 0
            for field in (
                "role_count",
                "channel_count",
                "panel_count",
                "resource_prefix_match_count",
            )
        )
    ):
        fail("discord_prefix_scan_evidence_invalid")
    _require_timestamp(value["observed_at"], "discord_prefix_scan_evidence_invalid")
    return value


def validate_guild_deletion(context, value):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "guild_id",
        "deletion_confirmed",
        "guild_observation_http_status",
        "discord_error_code",
        "confirmation_surface",
    }
    _require_exact(value, fields, "discord_guild_deletion_evidence_invalid")
    if (
        value["schema_version"] != 1
        or value["kind"] != GUILD_DELETION_KIND
        or value["guild_id"] != context.manifest["discord"]["guild_id"]
        or value["deletion_confirmed"] is not True
        or value["guild_observation_http_status"] != 404
        or value["discord_error_code"] != 10004
        or value["confirmation_surface"] != "chrome"
    ):
        fail("discord_guild_deletion_evidence_invalid")
    _require_timestamp(value["observed_at"], "discord_guild_deletion_evidence_invalid")
    return value


def validate_orchestration_absence(
    context,
    value,
    precleanup,
    teardown,
    absence,
    cleanup,
    prefix_scan,
    guild,
    step16_completion,
):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "manifest_sha256",
        "run_id",
        "installation_id",
        "guild_id",
        "resource_prefix",
        "precleanup_sha256",
        "database_absence_sha256",
        "cleanup_sha256",
        "discord_teardown_sha256",
        "prefix_scan_sha256",
        "guild_deletion_sha256",
        "step16_receipt_sha256",
        "coordinator_step16_completion_sha256",
        "coordinator_step16_completed_at",
        "unresolved_operation_count",
        "unresolved_receipt_count",
        "unresolved_journal_count",
        "route_count",
        "instance_count",
        "role_count",
        "channel_count",
        "panel_count",
        "resource_prefix_match_count",
        "database_absent",
        "postgres_process_absent",
        "launchd_jobs_absent",
        "keychain_items_absent",
        "discord_child_resources_absent",
        "protected_staging_unchanged",
        "external_credentials_preserved",
    }
    _require_exact(value, fields, "orchestrator_total_absence_evidence_invalid")
    zero_fields = (
        "unresolved_operation_count",
        "unresolved_receipt_count",
        "unresolved_journal_count",
        "route_count",
        "instance_count",
        "role_count",
        "channel_count",
        "panel_count",
        "resource_prefix_match_count",
    )
    true_fields = (
        "database_absent",
        "postgres_process_absent",
        "launchd_jobs_absent",
        "keychain_items_absent",
        "discord_child_resources_absent",
        "protected_staging_unchanged",
        "external_credentials_preserved",
    )
    if (
        value["schema_version"] != 1
        or value["kind"] != ORCHESTRATION_ABSENCE_KIND
        or value["manifest_sha256"] != context.digest
        or value["run_id"] != context.manifest["run_id"]
        or value["installation_id"] != absence["installation_id"]
        or value["guild_id"] != context.manifest["discord"]["guild_id"]
        or value["resource_prefix"] != context.manifest["discord"]["resource_prefix"]
        or value["precleanup_sha256"] != _digest(precleanup)
        or value["database_absence_sha256"] != _digest(absence)
        or value["cleanup_sha256"] != _digest(cleanup)
        or value["discord_teardown_sha256"] != _digest(teardown)
        or value["prefix_scan_sha256"] != _digest(prefix_scan)
        or value["guild_deletion_sha256"] != _digest(guild)
        or value["step16_receipt_sha256"]
        != step16_completion["step16_receipt_sha256"]
        or value["coordinator_step16_completion_sha256"]
        != step16_completion["step16_completion_sha256"]
        or value["coordinator_step16_completed_at"]
        != step16_completion["step16_completed_at"]
        or any(type(value[field]) is not int or value[field] != 0 for field in zero_fields)
        or any(value[field] is not True for field in true_fields)
    ):
        fail("orchestrator_total_absence_evidence_invalid")
    _require_timestamp(value["observed_at"], "orchestrator_total_absence_evidence_invalid")
    _require_digest(
        value["step16_receipt_sha256"],
        "orchestrator_total_absence_evidence_invalid",
    )
    _require_digest(
        value["coordinator_step16_completion_sha256"],
        "orchestrator_total_absence_evidence_invalid",
    )
    _require_timestamp(
        value["coordinator_step16_completed_at"],
        "orchestrator_total_absence_evidence_invalid",
    )
    if _parse_timestamp(
        step16_completion["step16_completed_at"],
        "orchestrator_total_absence_evidence_invalid",
    ) >= _parse_timestamp(
        prefix_scan["observed_at"], "orchestrator_total_absence_evidence_invalid"
    ):
        fail("step16_absence_chronology_invalid")
    if _parse_timestamp(
        prefix_scan["observed_at"], "orchestrator_total_absence_evidence_invalid"
    ) >= _parse_timestamp(
        guild["observed_at"], "orchestrator_total_absence_evidence_invalid"
    ):
        fail("discord_absence_chronology_invalid")
    if _parse_timestamp(
        guild["observed_at"], "orchestrator_total_absence_evidence_invalid"
    ) >= _parse_timestamp(
        value["observed_at"], "orchestrator_total_absence_evidence_invalid"
    ):
        fail("total_absence_observation_chronology_invalid")
    return value


def new_orchestration_absence(
    context,
    precleanup,
    teardown,
    absence,
    cleanup,
    prefix_scan,
    guild,
    step16_completion,
):
    return {
        "schema_version": 1,
        "kind": ORCHESTRATION_ABSENCE_KIND,
        "observed_at": _precise_utc_now(),
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "installation_id": absence["installation_id"],
        "guild_id": context.manifest["discord"]["guild_id"],
        "resource_prefix": context.manifest["discord"]["resource_prefix"],
        "precleanup_sha256": _digest(precleanup),
        "database_absence_sha256": _digest(absence),
        "cleanup_sha256": _digest(cleanup),
        "discord_teardown_sha256": _digest(teardown),
        "prefix_scan_sha256": _digest(prefix_scan),
        "guild_deletion_sha256": _digest(guild),
        "step16_receipt_sha256": step16_completion["step16_receipt_sha256"],
        "coordinator_step16_completion_sha256": step16_completion[
            "step16_completion_sha256"
        ],
        "coordinator_step16_completed_at": step16_completion[
            "step16_completed_at"
        ],
        "unresolved_operation_count": precleanup["unresolved_product_operation_count"],
        "unresolved_receipt_count": precleanup["unresolved_receipt_count"],
        "unresolved_journal_count": precleanup["unresolved_journal_entry_count"],
        "route_count": 0,
        "instance_count": 0,
        "role_count": prefix_scan["role_count"],
        "channel_count": prefix_scan["channel_count"],
        "panel_count": prefix_scan["panel_count"],
        "resource_prefix_match_count": prefix_scan["resource_prefix_match_count"],
        "database_absent": True,
        "postgres_process_absent": True,
        "launchd_jobs_absent": True,
        "keychain_items_absent": True,
        "discord_child_resources_absent": teardown["all_resources_absent"],
        "protected_staging_unchanged": True,
        "external_credentials_preserved": True,
    }


def _validate_database_absence_source(value):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "run_id",
        "installation_id",
        "database_absent",
    }
    _require_exact(value, fields, "step17_database_source_invalid")
    if (
        value["schema_version"] != 1
        or value["kind"] != DATABASE_ABSENCE_KIND
        or not isinstance(value["run_id"], str)
        or not value["run_id"]
        or not isinstance(value["installation_id"], str)
        or not value["installation_id"].startswith("installation:")
        or value["database_absent"] is not True
    ):
        fail("step17_database_source_invalid")
    _require_timestamp(value["observed_at"], "step17_database_source_invalid")
    return value


def _validate_prefix_scan_source(value):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "guild_id",
        "resource_prefix",
        "guild_observation_http_status",
        "role_count",
        "channel_count",
        "panel_count",
        "resource_prefix_match_count",
    }
    _require_exact(value, fields, "step17_prefix_scan_source_invalid")
    if (
        value["schema_version"] != 1
        or value["kind"] != PREFIX_SCAN_KIND
        or value["guild_observation_http_status"] != 200
        or not isinstance(value["resource_prefix"], str)
        or not value["resource_prefix"]
        or any(
            type(value[field]) is not int or value[field] != 0
            for field in (
                "role_count",
                "channel_count",
                "panel_count",
                "resource_prefix_match_count",
            )
        )
    ):
        fail("step17_prefix_scan_source_invalid")
    _require_snowflake(value["guild_id"], "step17_prefix_scan_source_invalid")
    _require_timestamp(value["observed_at"], "step17_prefix_scan_source_invalid")
    return value


def _validate_orchestration_absence_source(value):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "manifest_sha256",
        "run_id",
        "installation_id",
        "guild_id",
        "resource_prefix",
        "precleanup_sha256",
        "database_absence_sha256",
        "cleanup_sha256",
        "discord_teardown_sha256",
        "prefix_scan_sha256",
        "guild_deletion_sha256",
        "step16_receipt_sha256",
        "coordinator_step16_completion_sha256",
        "coordinator_step16_completed_at",
        "unresolved_operation_count",
        "unresolved_receipt_count",
        "unresolved_journal_count",
        "route_count",
        "instance_count",
        "role_count",
        "channel_count",
        "panel_count",
        "resource_prefix_match_count",
        "database_absent",
        "postgres_process_absent",
        "launchd_jobs_absent",
        "keychain_items_absent",
        "discord_child_resources_absent",
        "protected_staging_unchanged",
        "external_credentials_preserved",
    }
    _require_exact(value, fields, "step17_orchestration_source_invalid")
    zero_fields = (
        "unresolved_operation_count",
        "unresolved_receipt_count",
        "unresolved_journal_count",
        "route_count",
        "instance_count",
        "role_count",
        "channel_count",
        "panel_count",
        "resource_prefix_match_count",
    )
    true_fields = (
        "database_absent",
        "postgres_process_absent",
        "launchd_jobs_absent",
        "keychain_items_absent",
        "discord_child_resources_absent",
        "protected_staging_unchanged",
        "external_credentials_preserved",
    )
    if (
        value["schema_version"] != 1
        or value["kind"] != ORCHESTRATION_ABSENCE_KIND
        or not isinstance(value["run_id"], str)
        or not value["run_id"]
        or not isinstance(value["installation_id"], str)
        or not value["installation_id"].startswith("installation:")
        or not isinstance(value["guild_id"], str)
        or not isinstance(value["resource_prefix"], str)
        or not value["resource_prefix"]
        or any(type(value[field]) is not int or value[field] != 0 for field in zero_fields)
        or any(value[field] is not True for field in true_fields)
    ):
        fail("step17_orchestration_source_invalid")
    _require_snowflake(value["guild_id"], "step17_orchestration_source_invalid")
    _require_timestamp(value["observed_at"], "step17_orchestration_source_invalid")
    _require_timestamp(
        value["coordinator_step16_completed_at"],
        "step17_orchestration_source_invalid",
    )
    for field in (
        "manifest_sha256",
        "precleanup_sha256",
        "database_absence_sha256",
        "cleanup_sha256",
        "discord_teardown_sha256",
        "prefix_scan_sha256",
        "guild_deletion_sha256",
        "step16_receipt_sha256",
        "coordinator_step16_completion_sha256",
    ):
        _require_digest(value[field], "step17_orchestration_source_invalid")
    return value


def _validate_guild_deletion_source(value):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "guild_id",
        "deletion_confirmed",
        "guild_observation_http_status",
        "discord_error_code",
        "confirmation_surface",
    }
    _require_exact(value, fields, "step17_guild_source_invalid")
    if (
        value["schema_version"] != 1
        or value["kind"] != GUILD_DELETION_KIND
        or value["deletion_confirmed"] is not True
        or value["guild_observation_http_status"] != 404
        or value["discord_error_code"] != 10004
        or value["confirmation_surface"] != "chrome"
    ):
        fail("step17_guild_source_invalid")
    _require_snowflake(value["guild_id"], "step17_guild_source_invalid")
    _require_timestamp(value["observed_at"], "step17_guild_source_invalid")
    return value


def _validate_step17_binding(value):
    fields = {
        "manifest_sha256",
        "run_id",
        "installation_id",
        "guild_id",
        "resource_prefix",
        "precleanup_sha256",
        "discord_teardown_sha256",
        "step16_receipt_sha256",
        "coordinator_step16_completion_sha256",
    }
    _require_exact(value, fields, "step17_binding_invalid")
    _require_digest(value["manifest_sha256"], "step17_binding_invalid")
    _require_digest(value["precleanup_sha256"], "step17_binding_invalid")
    _require_digest(value["discord_teardown_sha256"], "step17_binding_invalid")
    _require_digest(value["step16_receipt_sha256"], "step17_binding_invalid")
    _require_digest(
        value["coordinator_step16_completion_sha256"],
        "step17_binding_invalid",
    )
    _require_snowflake(value["guild_id"], "step17_binding_invalid")
    if (
        not isinstance(value["run_id"], str)
        or not value["run_id"]
        or not isinstance(value["installation_id"], str)
        or not value["installation_id"].startswith("installation:")
        or not isinstance(value["resource_prefix"], str)
        or not value["resource_prefix"]
    ):
        fail("step17_binding_invalid")
    return value


def assemble_absence_evidence(database, orchestration, prefix_scan, guild, binding):
    database = _validate_database_absence_source(database)
    orchestration = _validate_orchestration_absence_source(orchestration)
    prefix_scan = _validate_prefix_scan_source(prefix_scan)
    guild = _validate_guild_deletion_source(guild)
    binding = _validate_step17_binding(binding)
    if (
        database["run_id"] != binding["run_id"]
        or orchestration["run_id"] != binding["run_id"]
        or database["installation_id"] != binding["installation_id"]
        or orchestration["installation_id"] != binding["installation_id"]
        or orchestration["manifest_sha256"] != binding["manifest_sha256"]
        or orchestration["guild_id"] != binding["guild_id"]
        or prefix_scan["guild_id"] != binding["guild_id"]
        or guild["guild_id"] != binding["guild_id"]
        or orchestration["resource_prefix"] != binding["resource_prefix"]
        or prefix_scan["resource_prefix"] != binding["resource_prefix"]
        or orchestration["precleanup_sha256"] != binding["precleanup_sha256"]
        or orchestration["discord_teardown_sha256"]
        != binding["discord_teardown_sha256"]
        or orchestration["step16_receipt_sha256"]
        != binding["step16_receipt_sha256"]
        or orchestration["coordinator_step16_completion_sha256"]
        != binding["coordinator_step16_completion_sha256"]
        or orchestration["database_absence_sha256"] != _digest(database)
        or orchestration["prefix_scan_sha256"] != _digest(prefix_scan)
        or orchestration["guild_deletion_sha256"] != _digest(guild)
    ):
        fail("step17_source_evidence_invalid")
    if _parse_timestamp(
        orchestration["coordinator_step16_completed_at"],
        "step17_source_evidence_invalid",
    ) >= _parse_timestamp(
        prefix_scan["observed_at"], "step17_source_evidence_invalid"
    ) or _parse_timestamp(
        prefix_scan["observed_at"], "step17_source_evidence_invalid"
    ) >= _parse_timestamp(guild["observed_at"], "step17_source_evidence_invalid"):
        fail("step17_source_chronology_invalid")
    if _parse_timestamp(
        guild["observed_at"], "step17_source_evidence_invalid"
    ) >= _parse_timestamp(
        orchestration["observed_at"], "step17_source_evidence_invalid"
    ):
        fail("step17_source_chronology_invalid")
    fields = {
        name: orchestration[name]
        for name in (
            "unresolved_operation_count",
            "unresolved_receipt_count",
            "unresolved_journal_count",
            "route_count",
            "instance_count",
            "role_count",
            "channel_count",
            "panel_count",
            "resource_prefix_match_count",
            "postgres_process_absent",
            "launchd_jobs_absent",
            "keychain_items_absent",
        )
    }
    return {
        **fields,
        "database_absent": True,
        "discord_guild_deleted": True,
    }


def local_step17_binding(context, precleanup, teardown, step16_completion):
    return {
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "installation_id": (
            f"installation:{context.manifest['discord']['resource_prefix']}"
        ),
        "guild_id": context.manifest["discord"]["guild_id"],
        "resource_prefix": context.manifest["discord"]["resource_prefix"],
        "precleanup_sha256": _digest(precleanup),
        "discord_teardown_sha256": _digest(teardown),
        "step16_receipt_sha256": step16_completion["step16_receipt_sha256"],
        "coordinator_step16_completion_sha256": step16_completion[
            "step16_completion_sha256"
        ],
    }


def command_finalize_total_absence(
    context, platform, prefix_scan_evidence_path, guild_deletion_evidence_path
):
    require_certification_eligible_teardown(context)
    step16_completion = require_certification_step_sixteen(context)
    ensure_finalization_directory(context)
    state = load_state(context, {"cleaned"})
    precleanup, teardown, _intent, _result, absence = _load_database_artifacts(context)
    cleanup = _validate_cleanup(
        context, _load_private(_cleanup_path(context), "cleanup_evidence")
    )
    finalization = validate_finalization(
        context,
        _load_private(finalization_evidence_path(context), "orchestrator_finalization"),
        precleanup,
        teardown,
        absence,
        cleanup,
    )
    if finalization["discord_active_resource_count"] != 0:
        fail("discord_child_resources_still_active")
    prefix_scan = validate_prefix_scan(
        context,
        _load_private(
            require_absolute_path(
                prefix_scan_evidence_path, "discord_prefix_scan_evidence"
            ),
            "discord_prefix_scan_evidence",
        ),
    )
    guild = validate_guild_deletion(
        context,
        _load_private(
            require_absolute_path(
                guild_deletion_evidence_path, "discord_guild_deletion_evidence"
            ),
            "discord_guild_deletion_evidence",
        ),
    )
    if not _candidate_services_absent(context, platform):
        fail("candidate_services_still_loaded")
    if platform.postgres_running(context.cluster_root) or _filesystem_entry_present(
        context.root, "isolated_runtime_absence_invalid"
    ):
        fail("isolated_runtime_still_present")
    if any(
        platform.keychain_present(service, account)
        for service, account in keychain_inventory(context)
    ):
        fail("run_keychain_items_still_present")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    if not _external_present(context, platform):
        fail("external_keychain_identity_absent")
    orchestration_path = orchestration_absence_path(context)
    if orchestration_path.exists():
        orchestration = validate_orchestration_absence(
            context,
            _load_private(orchestration_path, "orchestrator_total_absence"),
            precleanup,
            teardown,
            absence,
            cleanup,
            prefix_scan,
            guild,
            step16_completion,
        )
    else:
        orchestration = new_orchestration_absence(
            context,
            precleanup,
            teardown,
            absence,
            cleanup,
            prefix_scan,
            guild,
            step16_completion,
        )
        validate_orchestration_absence(
            context,
            orchestration,
            precleanup,
            teardown,
            absence,
            cleanup,
            prefix_scan,
            guild,
            step16_completion,
        )
        write_atomic(orchestration_path, canonical_json(orchestration) + "\n")
    step = assemble_absence_evidence(
        absence,
        orchestration,
        prefix_scan,
        guild,
        local_step17_binding(context, precleanup, teardown, step16_completion),
    )
    step_path = step_seventeen_evidence_path(context)
    if step_path.exists():
        if _load_private(step_path, "step_17_evidence") != step:
            fail("step17_evidence_replay_mismatch")
        status = "exact_replay"
    else:
        write_atomic(step_path, canonical_json(step) + "\n")
        status = "total_absence_confirmed"
    return {
        "status": status,
        "phase": "cleaned",
        "step": 17,
        "total_absence_confirmed": True,
        "discord_guild_deleted": True,
    }
