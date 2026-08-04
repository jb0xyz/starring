import argparse
import contextlib
import datetime
import fcntl
import hashlib
import importlib
import json
import os
import pathlib
import re
import stat
import sys

import d2_evidence
from d2_certification import (
    CertificationError,
    STEP_SPECS,
    ZERO_DIGEST,
    append_step_receipt,
    fsync_directory,
    load_receipts_from_handle,
    load_verified_manifest,
    open_locked_receipts,
    require_owned_mode,
    validate_utc_timestamp,
)
from d2_orchestrator_contract import OrchestratorError


SCHEMA_VERSION = 1
SOURCE_MAXIMUM_BYTES = 256 * 1024
COORDINATOR_INTENT_KIND = "starring.d2.coordinator-step-intent.v1"
COORDINATOR_COMPLETION_KIND = "starring.d2.coordinator-step-completion.v1"
COORDINATOR_LEDGER_KIND = "starring.d2.coordinator-evidence-ledger.v1"
COORDINATOR_FINAL_KIND = "starring.d2.coordinator-final-record.v1"
COORDINATOR_LEDGER_DOMAIN = b"starring.d2.coordinator-evidence-ledger.v1\x00"
DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")
HUMAN_BOUNDARIES = {
    4: "complete_discord_oauth",
    7: "confirm_product_preview",
    9: "execute_real_discord_interactions",
    14: "confirm_replacement_preview",
    17: "delete_disposable_discord_guild",
}
ORCHESTRATOR_BOOTSTRAP_KIND = "starring.d2.orchestrator-bootstrap-evidence.v1"
ORCHESTRATOR_PRIOR_ABSENCE_KIND = (
    "starring.d2.orchestrator-prior-absence-evidence.v1"
)
ORCHESTRATOR_CANDIDATE_KIND = "starring.d2.orchestrator-candidate-evidence.v1"
ORCHESTRATOR_ONBOARDING_KIND = "starring.d2.orchestrator-onboarding-evidence.v1"
LIVE_RUNTIME_RESTART_KIND = "starring.d2.live-runtime-restart-evidence.v1"
DB_PRECLEANUP_KIND = "starring.d2.db-precleanup-evidence.v1"
DISCORD_TEARDOWN_KIND = "starring.d2.discord-resource-teardown.v1"
ORCHESTRATOR_FINALIZATION_KIND = (
    "starring.d2.orchestrator-finalization-evidence.v1"
)
DB_ABSENCE_KIND = "starring.d2.db-absence-evidence.v1"
ORCHESTRATOR_ABSENCE_KIND = (
    "starring.d2.orchestrator-total-absence-evidence.v1"
)
PREFIX_SCAN_KIND = "starring.d2.browser-discord-resource-prefix-scan-evidence.v1"
GUILD_DELETION_KIND = (
    "starring.d2.browser-discord-guild-deletion-evidence.v1"
)
DISCORD_RECONCILIATION_OBSERVATION_KIND = (
    "starring.d2.discord-reconciliation-role-observation.v1"
)


def source_spec(kind, mode):
    return {"kind": kind, "mode": mode}


STEP_SOURCE_SPECS = {
    1: (source_spec(ORCHESTRATOR_BOOTSTRAP_KIND, "machine"),),
    2: (source_spec(ORCHESTRATOR_PRIOR_ABSENCE_KIND, "machine"),),
    3: (source_spec(ORCHESTRATOR_CANDIDATE_KIND, "machine"),),
    4: (
        source_spec("starring.d2.browser-authentication-evidence.v1", "chrome"),
        source_spec(ORCHESTRATOR_ONBOARDING_KIND, "machine"),
    ),
    5: (
        source_spec("starring.d2.browser-authoring-evidence.v1", "chrome"),
        source_spec("starring.d2.worker-authoring-evidence.v1", "machine"),
    ),
    6: (source_spec("starring.d2.db-authoring-evidence.v1", "machine"),),
    7: (
        source_spec(
            "starring.d2.browser-product-decision-evidence.v1", "chrome"
        ),
    ),
    8: (
        source_spec("starring.d2.browser-live-evidence.v1", "chrome"),
        source_spec("starring.d2.db-live-evidence.v1", "machine"),
    ),
    9: (
        source_spec("starring.d2.db-interaction-evidence.v1", "machine"),
        source_spec("starring.d2.transport-resource-evidence.v1", "machine"),
    ),
    10: (
        source_spec("starring.d2.db-duplicate-evidence.v1", "machine"),
        source_spec("starring.d2.transport-duplicate-evidence.v1", "machine"),
    ),
    11: (source_spec(LIVE_RUNTIME_RESTART_KIND, "machine"),),
    12: (
        source_spec("starring.d2.db-reconstruction-evidence.v1", "machine"),
    ),
    13: (
        source_spec("starring.d2.db-reconciliation-evidence.v1", "machine"),
        source_spec("starring.d2.transport-indeterminate-evidence.v1", "machine"),
        source_spec(DISCORD_RECONCILIATION_OBSERVATION_KIND, "machine"),
    ),
    14: (
        source_spec("starring.d2.browser-replacement-evidence.v1", "chrome"),
        source_spec("starring.d2.db-replacement-evidence.v1", "machine"),
    ),
    15: (
        source_spec("starring.d2.browser-live-loss-evidence.v1", "chrome"),
        source_spec("starring.d2.transport-gateway-loss-evidence.v1", "machine"),
    ),
    16: (
        source_spec(DB_PRECLEANUP_KIND, "machine"),
        source_spec(DISCORD_TEARDOWN_KIND, "machine"),
        source_spec(ORCHESTRATOR_FINALIZATION_KIND, "machine"),
    ),
    17: (
        source_spec(DB_ABSENCE_KIND, "machine"),
        source_spec(ORCHESTRATOR_ABSENCE_KIND, "machine"),
        source_spec(PREFIX_SCAN_KIND, "chrome"),
        source_spec(GUILD_DELETION_KIND, "chrome"),
    ),
}


def canonical_json(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def fail(code):
    raise CertificationError(code)


def utc_now():
    return (
        datetime.datetime.now(datetime.timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z")
    )


def sha256_bytes(value):
    return hashlib.sha256(value).hexdigest()


def coordinator_directory(manifest_path):
    return pathlib.Path(manifest_path).parent / "coordinator"


def coordinator_lock_path(manifest_path):
    return coordinator_directory(manifest_path) / "coordinator.lock"


def coordinator_intent_path(manifest_path, step):
    return coordinator_directory(manifest_path) / f"step-{step:02d}-intent.json"


def coordinator_completion_path(manifest_path, step):
    return coordinator_directory(manifest_path) / f"step-{step:02d}-completion.json"


def path_present(path):
    return os.path.lexists(path)


def ensure_coordinator_directory(manifest_path):
    directory = coordinator_directory(manifest_path)
    if not path_present(directory):
        try:
            directory.mkdir(mode=0o700)
        except FileExistsError:
            pass
        except OSError:
            fail("coordinator_directory_unavailable")
        fsync_directory(directory.parent, "coordinator_directory_parent")
    try:
        metadata = directory.lstat()
    except OSError:
        fail("coordinator_directory_unavailable")
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or directory.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        fail("coordinator_directory_invalid")
    return directory


@contextlib.contextmanager
def coordinator_lock(manifest_path, exclusive):
    directory = ensure_coordinator_directory(manifest_path)
    path = coordinator_lock_path(manifest_path)
    created = not path_present(path)
    flags = os.O_RDWR | os.O_CREAT
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags, 0o600)
    except OSError:
        fail("coordinator_lock_unavailable")
    try:
        if created:
            os.fchmod(descriptor, 0o600)
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
        ):
            fail("coordinator_lock_invalid")
        if created:
            fsync_directory(directory, "coordinator_lock_parent")
        fcntl.flock(
            descriptor,
            fcntl.LOCK_EX if exclusive else fcntl.LOCK_SH,
        )
        yield
    finally:
        os.close(descriptor)


def read_private_json(path, label):
    path = pathlib.Path(path)
    if not path.is_absolute():
        fail(f"{label}_path_not_absolute")
    flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags)
    except OSError:
        fail(f"{label}_unavailable")
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
            or metadata.st_size <= 0
            or metadata.st_size > SOURCE_MAXIMUM_BYTES
        ):
            fail(f"{label}_file_invalid")
        with os.fdopen(descriptor, "rb", closefd=False) as handle:
            raw = handle.read(SOURCE_MAXIMUM_BYTES + 1)
        if len(raw) != metadata.st_size:
            fail(f"{label}_read_drift")
    finally:
        os.close(descriptor)
    try:
        value = d2_evidence.load_strict_json(raw)
    except d2_evidence.EvidenceContractError as error:
        fail(f"{label}_json_invalid:{error}")
    if not isinstance(value, dict):
        fail(f"{label}_object_required")
    return value, sha256_bytes(raw)


def write_private_json(path, value):
    path = pathlib.Path(path)
    payload = (canonical_json(value) + "\n").encode("utf-8")
    if len(payload) > SOURCE_MAXIMUM_BYTES:
        fail("coordinator_record_too_large")
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(temporary, flags, 0o600)
    except OSError:
        fail("coordinator_record_unavailable")
    try:
        os.fchmod(descriptor, 0o600)
        written = 0
        while written < len(payload):
            count = os.write(descriptor, payload[written:])
            if count <= 0:
                fail("coordinator_record_write_failed")
            written += count
        os.fsync(descriptor)
    except BaseException:
        try:
            temporary.unlink()
        except OSError:
            pass
        raise
    finally:
        os.close(descriptor)
    if path_present(path):
        temporary.unlink()
        fail("coordinator_record_already_exists")
    os.replace(temporary, path)
    fsync_directory(path.parent, "coordinator_record_parent")


def source_records(sources):
    return [
        {"kind": source["kind"], "sha256": source["sha256"]}
        for source in sources
    ]


def validate_source_mapping(step, raw_paths):
    expected = STEP_SOURCE_SPECS[step]
    if not isinstance(raw_paths, list) or len(raw_paths) != len(expected):
        fail("coordinator_source_count_invalid")
    paths = [pathlib.Path(raw) for raw in raw_paths]
    if len(set(paths)) != len(paths):
        fail("coordinator_source_path_duplicate")
    loaded = {}
    for index, path in enumerate(paths, 1):
        value, digest = read_private_json(path, f"coordinator_source_{index}")
        kind = value.get("kind")
        if not isinstance(kind, str):
            fail("coordinator_source_kind_invalid")
        if kind in loaded:
            fail("coordinator_source_kind_duplicate")
        loaded[kind] = {
            "kind": kind,
            "value": value,
            "sha256": digest,
        }
    expected_kinds = tuple(specification["kind"] for specification in expected)
    if set(loaded) != set(expected_kinds):
        fail("coordinator_source_kind_not_allowed")
    return [loaded[kind] for kind in expected_kinds]


def validate_direct_source(step, kind, value, manifest, digest):
    if (
        not isinstance(value, dict)
        or set(value)
        != {
            "schema_version",
            "kind",
            "observed_at",
            "manifest_sha256",
            "run_id",
            "evidence",
        }
        or type(value["schema_version"]) is not int
        or value["schema_version"] != SCHEMA_VERSION
        or value["kind"] != kind
        or not validate_utc_timestamp(value["observed_at"])
        or value["manifest_sha256"] != digest
        or value["run_id"] != manifest["run_id"]
        or not isinstance(value["evidence"], dict)
        or set(value["evidence"]) != set(STEP_SPECS[step].required)
    ):
        fail("coordinator_direct_source_invalid")
    return dict(value["evidence"])


def validate_onboarding_source(value, manifest, digest):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "manifest_sha256",
        "run_id",
        "outcome",
        "installation_id",
        "principal_id",
        "guild_id",
        "discord_application_id",
        "binding_key",
        "hub_channel_id",
    }
    discord = manifest["discord"]
    if (
        not isinstance(value, dict)
        or set(value) != fields
        or type(value["schema_version"]) is not int
        or value["schema_version"] != SCHEMA_VERSION
        or value["kind"] != ORCHESTRATOR_ONBOARDING_KIND
        or not validate_utc_timestamp(value["observed_at"])
        or value["manifest_sha256"] != digest
        or value["run_id"] != manifest["run_id"]
        or value["outcome"] not in {"fresh", "exact_replay"}
        or value["installation_id"]
        != f"installation:{discord['resource_prefix']}"
        or value["principal_id"] != f"discord:{discord['actor_id']}"
        or value["guild_id"] != discord["guild_id"]
        or value["discord_application_id"] != discord["application_id"]
        or value["binding_key"] != "community_hub"
        or value["hub_channel_id"] != discord["hub_channel_id"]
    ):
        fail("coordinator_onboarding_source_invalid")
    return value


def finalization_assembler(name):
    try:
        module = importlib.import_module("d2_finalization")
    except ImportError:
        fail("finalization_assembler_unavailable")
    assembler = getattr(module, name, None)
    if not callable(assembler):
        fail("finalization_assembler_unavailable")
    return assembler


def step_binding(step, prior_receipts, manifest, digest):
    if step not in {16, 17}:
        return None
    if len(prior_receipts) != step - 1:
        fail("coordinator_prior_receipt_missing")
    installation_id = prior_receipts[3]["evidence"]["installation_id"]
    if step == 16:
        created = prior_receipts[8]["evidence"]
        expected = sorted(
            set(
                created["role_ids"]
                + created["channel_ids"]
                + created["panel_message_ids"]
                + [prior_receipts[12]["evidence"]["output_role_id"]]
            )
        )
        return {
            "manifest_sha256": digest,
            "run_id": manifest["run_id"],
            "installation_id": installation_id,
            "transport_instance_id": prior_receipts[2]["evidence"][
                "transport_instance_id"
            ],
            "reconciliation_inventory_digest_sha256": prior_receipts[12][
                "evidence"
            ]["reconciliation_inventory_digest_sha256"],
            "expected_discord_resource_ids": expected,
        }
    teardown = prior_receipts[15]["evidence"]
    return {
        "manifest_sha256": digest,
        "run_id": manifest["run_id"],
        "installation_id": installation_id,
        "guild_id": manifest["discord"]["guild_id"],
        "resource_prefix": manifest["discord"]["resource_prefix"],
        "precleanup_sha256": teardown["precleanup_sha256"],
        "discord_teardown_sha256": teardown["discord_teardown_sha256"],
    }


def assemble_step_evidence(
    step,
    sources,
    prior_receipts,
    manifest,
    digest,
):
    values = {source["kind"]: source["value"] for source in sources}
    try:
        if step == 1:
            return validate_direct_source(
                step,
                ORCHESTRATOR_BOOTSTRAP_KIND,
                values[ORCHESTRATOR_BOOTSTRAP_KIND],
                manifest,
                digest,
            )
        if step == 2:
            return validate_direct_source(
                step,
                ORCHESTRATOR_PRIOR_ABSENCE_KIND,
                values[ORCHESTRATOR_PRIOR_ABSENCE_KIND],
                manifest,
                digest,
            )
        if step == 3:
            return validate_direct_source(
                step,
                ORCHESTRATOR_CANDIDATE_KIND,
                values[ORCHESTRATOR_CANDIDATE_KIND],
                manifest,
                digest,
            )
        if step == 4:
            authentication = d2_evidence.assemble_authentication_evidence(
                values["starring.d2.browser-authentication-evidence.v1"]
            )
            onboarding = validate_onboarding_source(
                values[ORCHESTRATOR_ONBOARDING_KIND], manifest, digest
            )
            if (
                authentication["principal_id"] != onboarding["principal_id"]
                or authentication["installation_id"]
                != onboarding["installation_id"]
                or authentication["guild_id"] != onboarding["guild_id"]
                or authentication["public_origin"]
                != manifest["cloudflare"]["public_origin"]
            ):
                fail("coordinator_onboarding_binding_invalid")
            return authentication
        if step == 5:
            worker = values["starring.d2.worker-authoring-evidence.v1"]
            if (
                worker.get("manifest_sha256") != digest
                or worker.get("worker_source_sha256")
                != manifest["source_trees"]["codex_worker"]["sha256"]
            ):
                fail("coordinator_worker_binding_invalid")
            return d2_evidence.assemble_authoring_evidence(
                values["starring.d2.browser-authoring-evidence.v1"],
                worker,
            )
        if step == 6:
            return d2_evidence.assemble_preview_evidence(
                values["starring.d2.db-authoring-evidence.v1"]
            )
        if step == 7:
            return d2_evidence.assemble_decision_evidence(
                values["starring.d2.browser-product-decision-evidence.v1"]
            )
        if step == 8:
            return d2_evidence.assemble_live_evidence(
                values["starring.d2.browser-live-evidence.v1"],
                values["starring.d2.db-live-evidence.v1"],
            )
        if step == 9:
            return d2_evidence.assemble_interaction_evidence(
                values["starring.d2.db-interaction-evidence.v1"],
                values["starring.d2.transport-resource-evidence.v1"],
            )
        if step == 10:
            return d2_evidence.assemble_duplicate_evidence(
                values["starring.d2.db-duplicate-evidence.v1"],
                values["starring.d2.transport-duplicate-evidence.v1"],
            )
        if step == 11:
            return validate_direct_source(
                step,
                LIVE_RUNTIME_RESTART_KIND,
                values[LIVE_RUNTIME_RESTART_KIND],
                manifest,
                digest,
            )
        if step == 12:
            return d2_evidence.assemble_reconstruction_evidence(
                values["starring.d2.db-reconstruction-evidence.v1"]
            )
        if step == 13:
            return d2_evidence.assemble_reconciliation_evidence(
                values["starring.d2.db-reconciliation-evidence.v1"],
                values["starring.d2.transport-indeterminate-evidence.v1"],
                values[DISCORD_RECONCILIATION_OBSERVATION_KIND],
            )
        if step == 14:
            return d2_evidence.assemble_replacement_evidence(
                values["starring.d2.browser-replacement-evidence.v1"],
                values["starring.d2.db-replacement-evidence.v1"],
            )
        if step == 15:
            if len(prior_receipts) != 14:
                fail("coordinator_prior_receipt_missing")
            return d2_evidence.assemble_live_loss_evidence(
                values["starring.d2.browser-live-loss-evidence.v1"],
                values["starring.d2.transport-gateway-loss-evidence.v1"],
                {
                    "installation_id": prior_receipts[13]["evidence"][
                        "installation_id"
                    ],
                    "replacement_promotion_id": prior_receipts[13]["evidence"][
                        "replacement_promotion_id"
                    ],
                    "replacement_route_id": prior_receipts[13]["evidence"][
                        "replacement_route_id"
                    ],
                },
            )
        if step == 16:
            return finalization_assembler("assemble_teardown_evidence")(
                values[DB_PRECLEANUP_KIND],
                values[DISCORD_TEARDOWN_KIND],
                values[ORCHESTRATOR_FINALIZATION_KIND],
                step_binding(step, prior_receipts, manifest, digest),
            )
        if step == 17:
            return finalization_assembler("assemble_absence_evidence")(
                values[DB_ABSENCE_KIND],
                values[ORCHESTRATOR_ABSENCE_KIND],
                values[PREFIX_SCAN_KIND],
                values[GUILD_DELETION_KIND],
                step_binding(
                    step,
                    prior_receipts,
                    manifest,
                    digest,
                ),
            )
    except (
        d2_evidence.EvidenceContractError,
        OrchestratorError,
        KeyError,
        TypeError,
    ) as error:
        fail(f"coordinator_evidence_assembly_failed:{error}")
    fail("coordinator_step_assembler_unavailable")


def load_receipts(manifest_path, manifest, digest):
    receipts_path = pathlib.Path(manifest_path).with_name("receipts.jsonl")
    require_owned_mode(receipts_path, 0o600, "receipts")
    with open_locked_receipts(receipts_path, False) as handle:
        return load_receipts_from_handle(handle, manifest, digest)


def validate_intent(intent, manifest, digest, step):
    if (
        not isinstance(intent, dict)
        or set(intent)
        != {
            "schema_version",
            "kind",
            "run_id",
            "manifest_sha256",
            "step",
            "code",
            "observed_at",
            "receipt_chain_head_sha256",
            "sources",
        }
        or type(intent["schema_version"]) is not int
        or intent["schema_version"] != SCHEMA_VERSION
        or intent["kind"] != COORDINATOR_INTENT_KIND
        or intent["run_id"] != manifest["run_id"]
        or intent["manifest_sha256"] != digest
        or type(intent["step"]) is not int
        or intent["step"] != step
        or intent["code"] != STEP_SPECS[step].code
        or not validate_utc_timestamp(intent["observed_at"])
        or not isinstance(intent["receipt_chain_head_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(intent["receipt_chain_head_sha256"])
        or not isinstance(intent["sources"], list)
    ):
        fail("coordinator_intent_invalid")
    expected = STEP_SOURCE_SPECS[step]
    if len(intent["sources"]) != len(expected):
        fail("coordinator_intent_invalid")
    for source, specification in zip(intent["sources"], expected):
        if (
            not isinstance(source, dict)
            or set(source) != {"kind", "sha256"}
            or source["kind"] != specification["kind"]
            or not isinstance(source["sha256"], str)
            or not DIGEST_PATTERN.fullmatch(source["sha256"])
        ):
            fail("coordinator_intent_invalid")
    return intent


def validate_completion(completion, manifest, digest, step):
    if (
        not isinstance(completion, dict)
        or set(completion)
        != {
            "schema_version",
            "kind",
            "run_id",
            "manifest_sha256",
            "step",
            "code",
            "observed_at",
            "intent_sha256",
            "receipt_sha256",
            "receipt_disposition",
        }
        or type(completion["schema_version"]) is not int
        or completion["schema_version"] != SCHEMA_VERSION
        or completion["kind"] != COORDINATOR_COMPLETION_KIND
        or completion["run_id"] != manifest["run_id"]
        or completion["manifest_sha256"] != digest
        or type(completion["step"]) is not int
        or completion["step"] != step
        or completion["code"] != STEP_SPECS[step].code
        or not validate_utc_timestamp(completion["observed_at"])
        or not isinstance(completion["intent_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(completion["intent_sha256"])
        or not isinstance(completion["receipt_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(completion["receipt_sha256"])
        or completion["receipt_disposition"] not in {"created", "exact_replay"}
    ):
        fail("coordinator_completion_invalid")
    return completion


def load_coordinator_record(path, label):
    value, _digest = read_private_json(path, label)
    return value


def intent_digest(intent):
    return sha256_bytes(canonical_json(intent).encode("utf-8"))


def coordinator_record_digest(record):
    return sha256_bytes(canonical_json(record).encode("utf-8"))


def coordinator_pending_step(manifest_path, manifest, digest, receipts):
    pending = []
    completed_steps = len(receipts)
    for step in STEP_SPECS:
        intent_path = coordinator_intent_path(manifest_path, step)
        completion_path = coordinator_completion_path(manifest_path, step)
        has_intent = path_present(intent_path)
        has_completion = path_present(completion_path)
        if step <= completed_steps and not has_intent:
            fail(f"coordinator_intent_missing:{step}")
        if step < completed_steps and not has_completion:
            fail(f"coordinator_completion_missing:{step}")
        if has_completion and not has_intent:
            fail("coordinator_completion_without_intent")
        if not has_intent:
            continue
        intent = validate_intent(
            load_coordinator_record(intent_path, "coordinator_intent"),
            manifest,
            digest,
            step,
        )
        expected_previous = (
            ZERO_DIGEST
            if step == 1
            else receipts[step - 2]["receipt_sha256"]
            if step - 1 <= completed_steps
            else None
        )
        if (
            expected_previous is not None
            and intent["receipt_chain_head_sha256"] != expected_previous
        ):
            fail("coordinator_receipt_chain_drift")
        if has_completion:
            completion = validate_completion(
                load_coordinator_record(
                    completion_path, "coordinator_completion"
                ),
                manifest,
                digest,
                step,
            )
            if (
                completion["intent_sha256"] != intent_digest(intent)
                or step > completed_steps
                or completion["receipt_sha256"]
                != receipts[step - 1]["receipt_sha256"]
            ):
                fail("coordinator_completion_drift")
        else:
            pending.append(step)
    if len(pending) > 1:
        fail("coordinator_multiple_pending_steps")
    if pending and pending[0] not in {completed_steps, completed_steps + 1}:
        fail("coordinator_pending_step_drift")
    return None if not pending else pending[0]


def next_certification_action(manifest_path):
    verified_path, manifest, digest = load_verified_manifest(manifest_path)
    with coordinator_lock(verified_path, False):
        receipts = load_receipts(verified_path, manifest, digest)
        pending_step = coordinator_pending_step(
            verified_path, manifest, digest, receipts
        )
    completed_steps = len(receipts)
    chain_head = ZERO_DIGEST if not receipts else receipts[-1]["receipt_sha256"]
    base = {
        "schema_version": SCHEMA_VERSION,
        "kind": "starring.d2.certification-next-action.v1",
        "run_id": manifest["run_id"],
        "manifest_sha256": digest,
        "completed_steps": completed_steps,
        "receipt_chain_head_sha256": chain_head,
    }
    if pending_step is not None:
        specification = STEP_SPECS[pending_step]
        result = {
            **base,
            "status": "resume_step",
            "step": pending_step,
            "code": specification.code,
            "required_evidence_fields": list(specification.required),
            "required_sources": [dict(value) for value in STEP_SOURCE_SPECS[pending_step]],
        }
        if pending_step in HUMAN_BOUNDARIES:
            result["boundary"] = HUMAN_BOUNDARIES[pending_step]
        return result
    if completed_steps == len(STEP_SPECS):
        return {
            **base,
            "status": "complete",
            "steps": len(STEP_SPECS),
        }
    step = completed_steps + 1
    specification = STEP_SPECS[step]
    result = {
        **base,
        "status": (
            "awaiting_human_boundary" if step in HUMAN_BOUNDARIES else "next_step"
        ),
        "step": step,
        "code": specification.code,
        "required_evidence_fields": list(specification.required),
        "required_sources": [dict(value) for value in STEP_SOURCE_SPECS[step]],
    }
    if step in HUMAN_BOUNDARIES:
        result["boundary"] = HUMAN_BOUNDARIES[step]
    return result


def advance_certification(manifest_path, step, raw_sources):
    verified_path, manifest, digest = load_verified_manifest(manifest_path)
    if step not in STEP_SPECS:
        fail("coordinator_step_invalid")
    with coordinator_lock(verified_path, True):
        receipts = load_receipts(verified_path, manifest, digest)
        completed_steps = len(receipts)
        pending_step = coordinator_pending_step(
            verified_path, manifest, digest, receipts
        )
        if pending_step is not None:
            if step != pending_step:
                fail(f"coordinator_step_out_of_order:expected_{pending_step}")
        elif step == completed_steps:
            completion_path = coordinator_completion_path(verified_path, step)
            if step == 0 or not path_present(completion_path):
                expected = min(completed_steps + 1, len(STEP_SPECS))
                fail(f"coordinator_step_out_of_order:expected_{expected}")
        elif step != completed_steps + 1:
            expected = min(completed_steps + 1, len(STEP_SPECS))
            fail(f"coordinator_step_out_of_order:expected_{expected}")
        sources = validate_source_mapping(step, raw_sources)
        records = source_records(sources)
        intent_path = coordinator_intent_path(verified_path, step)
        completion_path = coordinator_completion_path(verified_path, step)
        prior_receipts = receipts[: step - 1]
        existing_receipt = receipts[step - 1] if len(receipts) >= step else None
        expected_previous = (
            ZERO_DIGEST
            if not prior_receipts
            else prior_receipts[-1]["receipt_sha256"]
        )
        if path_present(intent_path):
            intent = validate_intent(
                load_coordinator_record(intent_path, "coordinator_intent"),
                manifest,
                digest,
                step,
            )
            if intent["sources"] != records:
                fail("coordinator_source_digest_drift")
            if intent["receipt_chain_head_sha256"] != expected_previous:
                fail("coordinator_receipt_chain_drift")
            evidence = assemble_step_evidence(
                step,
                sources,
                prior_receipts,
                manifest,
                digest,
            )
        else:
            if step != completed_steps + 1:
                fail("coordinator_replay_intent_missing")
            evidence = assemble_step_evidence(
                step,
                sources,
                prior_receipts,
                manifest,
                digest,
            )
            intent = {
                "schema_version": SCHEMA_VERSION,
                "kind": COORDINATOR_INTENT_KIND,
                "run_id": manifest["run_id"],
                "manifest_sha256": digest,
                "step": step,
                "code": STEP_SPECS[step].code,
                "observed_at": utc_now(),
                "receipt_chain_head_sha256": (
                    ZERO_DIGEST
                    if not receipts
                    else receipts[-1]["receipt_sha256"]
                ),
                "sources": records,
            }
            write_private_json(intent_path, intent)
        receipt, replayed = append_step_receipt(
            verified_path, manifest, digest, step, evidence
        )
        if existing_receipt is not None and receipt != existing_receipt:
            fail("coordinator_receipt_replay_drift")
        completion = {
            "schema_version": SCHEMA_VERSION,
            "kind": COORDINATOR_COMPLETION_KIND,
            "run_id": manifest["run_id"],
            "manifest_sha256": digest,
            "step": step,
            "code": STEP_SPECS[step].code,
            "observed_at": utc_now(),
            "intent_sha256": intent_digest(intent),
            "receipt_sha256": receipt["receipt_sha256"],
            "receipt_disposition": "exact_replay" if replayed else "created",
        }
        if path_present(completion_path):
            recorded = validate_completion(
                load_coordinator_record(
                    completion_path, "coordinator_completion"
                ),
                manifest,
                digest,
                step,
            )
            if any(
                recorded[field] != completion[field]
                for field in (
                    "intent_sha256",
                    "receipt_sha256",
                )
            ):
                fail("coordinator_completion_drift")
        else:
            write_private_json(completion_path, completion)
        return {
            "schema_version": SCHEMA_VERSION,
            "kind": "starring.d2.certification-advance-result.v1",
            "run_id": manifest["run_id"],
            "manifest_sha256": digest,
            "step": step,
            "code": STEP_SPECS[step].code,
            "disposition": "exact_replay" if replayed else "created",
            "replayed": replayed,
            "receipt_sha256": receipt["receipt_sha256"],
        }


def verify_certification(manifest_path):
    verified_path, manifest, digest = load_verified_manifest(manifest_path)
    with coordinator_lock(verified_path, False):
        receipts = load_receipts(verified_path, manifest, digest)
        pending_step = coordinator_pending_step(
            verified_path, manifest, digest, receipts
        )
        if pending_step is not None:
            fail(f"coordinator_pending_step_unfinished:{pending_step}")
        if len(receipts) != len(STEP_SPECS):
            fail(
                f"coordinator_certification_incomplete:{len(receipts)}_of_"
                f"{len(STEP_SPECS)}"
            )
        ledger_steps = []
        for step in STEP_SPECS:
            intent_path = coordinator_intent_path(verified_path, step)
            completion_path = coordinator_completion_path(verified_path, step)
            if not path_present(intent_path):
                fail(f"coordinator_intent_missing:{step}")
            if not path_present(completion_path):
                fail(f"coordinator_completion_missing:{step}")
            intent = validate_intent(
                load_coordinator_record(intent_path, "coordinator_intent"),
                manifest,
                digest,
                step,
            )
            completion = validate_completion(
                load_coordinator_record(
                    completion_path, "coordinator_completion"
                ),
                manifest,
                digest,
                step,
            )
            receipt = receipts[step - 1]
            expected_previous = (
                ZERO_DIGEST
                if step == 1
                else receipts[step - 2]["receipt_sha256"]
            )
            if intent["receipt_chain_head_sha256"] != expected_previous:
                fail(f"coordinator_intent_chain_drift:{step}")
            if completion["intent_sha256"] != intent_digest(intent):
                fail(f"coordinator_completion_intent_drift:{step}")
            if completion["receipt_sha256"] != receipt["receipt_sha256"]:
                fail(f"coordinator_completion_receipt_drift:{step}")
            ledger_steps.append(
                {
                    "step": step,
                    "code": STEP_SPECS[step].code,
                    "intent_sha256": coordinator_record_digest(intent),
                    "completion_sha256": coordinator_record_digest(completion),
                    "receipt_sha256": receipt["receipt_sha256"],
                    "sources": intent["sources"],
                }
            )
        ledger = {
            "schema_version": SCHEMA_VERSION,
            "kind": COORDINATOR_LEDGER_KIND,
            "run_id": manifest["run_id"],
            "manifest_sha256": digest,
            "receipt_chain_head_sha256": receipts[-1]["receipt_sha256"],
            "steps": ledger_steps,
        }
        coordinator_evidence_sha256 = sha256_bytes(
            COORDINATOR_LEDGER_DOMAIN + canonical_json(ledger).encode("utf-8")
        )
    return {
        "schema_version": SCHEMA_VERSION,
        "kind": COORDINATOR_FINAL_KIND,
        "run_id": manifest["run_id"],
        "commit_sha": manifest["commit_sha"],
        "manifest_sha256": digest,
        "steps": len(receipts),
        "status": "passed",
        "resource_prefix": manifest["discord"]["resource_prefix"],
        "receipt_chain_head_sha256": receipts[-1]["receipt_sha256"],
        "coordinator_evidence_sha256": coordinator_evidence_sha256,
    }


def parser():
    root = argparse.ArgumentParser(prog="d2-run")
    commands = root.add_subparsers(dest="command", required=True)
    status = commands.add_parser("status")
    status.add_argument("--manifest", required=True)
    verify = commands.add_parser("verify")
    verify.add_argument("--manifest", required=True)
    advance = commands.add_parser("advance")
    advance.add_argument("--manifest", required=True)
    advance.add_argument("--step", required=True, type=int)
    advance.add_argument("--source", action="append", required=True)
    return root


def main(argv=None):
    try:
        arguments = parser().parse_args(argv)
        if arguments.command == "status":
            result = next_certification_action(arguments.manifest)
        elif arguments.command == "verify":
            result = verify_certification(arguments.manifest)
        elif arguments.command == "advance":
            result = advance_certification(
                arguments.manifest, arguments.step, arguments.source
            )
        else:
            fail("coordinator_command_invalid")
        print(canonical_json(result))
        return 0
    except CertificationError as error:
        print(str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
