import os
import stat

from d2_certification import (
    STEP_SPECS,
    canonical_json,
    load_json_file,
    require_owned_mode,
    validate_utc_timestamp,
)
from d2_orchestrator_contract import fail, fsync_directory, write_atomic


SCHEMA_VERSION = 1
BOOTSTRAP_KIND = "starring.d2.orchestrator-bootstrap-evidence.v1"
PRIOR_ABSENCE_KIND = "starring.d2.orchestrator-prior-absence-evidence.v1"
CANDIDATE_KIND = "starring.d2.orchestrator-candidate-evidence.v1"
ONBOARDING_KIND = "starring.d2.orchestrator-onboarding-evidence.v1"
LIVE_RUNTIME_RESTART_KIND = "starring.d2.live-runtime-restart-evidence.v1"
PREFLIGHT_KIND = "starring.d2.preflight-absence-evidence.v1"
SOURCE_DIRECTORY_NAME = "coordinator-sources"


def source_directory(context):
    return context.artifact_directory / SOURCE_DIRECTORY_NAME


def ensure_source_directory(context):
    try:
        parent_metadata = context.artifact_directory.lstat()
    except OSError:
        fail("coordinator_source_directory_parent_invalid")
    if (
        not stat.S_ISDIR(parent_metadata.st_mode)
        or context.artifact_directory.is_symlink()
        or parent_metadata.st_uid != os.getuid()
        or stat.S_IMODE(parent_metadata.st_mode) != 0o700
    ):
        fail("coordinator_source_directory_parent_invalid")
    directory = source_directory(context)
    if not os.path.lexists(directory):
        try:
            directory.mkdir(mode=0o700)
        except OSError:
            fail("coordinator_source_directory_invalid")
        fsync_directory(
            context.artifact_directory, "coordinator_source_directory_parent"
        )
    try:
        metadata = directory.lstat()
    except OSError:
        fail("coordinator_source_directory_invalid")
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or directory.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        fail("coordinator_source_directory_invalid")
    return directory


def source_path(context, step, label):
    return source_directory(context) / f"step-{step:02d}-{label}.json"


def validate_direct_source(context, step, kind, value):
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
        or value["manifest_sha256"] != context.digest
        or value["run_id"] != context.manifest["run_id"]
        or not isinstance(value["evidence"], dict)
        or set(value["evidence"]) != set(STEP_SPECS[step].required)
    ):
        fail("coordinator_direct_source_invalid")
    return value


def read_private_source(context, path, step, kind):
    try:
        require_owned_mode(path, 0o600, "coordinator_source")
        value = load_json_file(path, "coordinator_source")
    except Exception as error:
        fail(f"coordinator_source_invalid:{error}")
    return validate_direct_source(context, step, kind, value)


def publish_direct_source(context, step, kind, label, evidence, observed_at):
    if not validate_utc_timestamp(observed_at):
        fail("coordinator_source_timestamp_invalid")
    if not isinstance(evidence, dict) or set(evidence) != set(
        STEP_SPECS[step].required
    ):
        fail("coordinator_source_evidence_invalid")
    directory = ensure_source_directory(context)
    path = source_path(context, step, label)
    value = {
        "schema_version": SCHEMA_VERSION,
        "kind": kind,
        "observed_at": observed_at,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "evidence": evidence,
    }
    validate_direct_source(context, step, kind, value)
    if os.path.lexists(path):
        recorded = read_private_source(context, path, step, kind)
        if recorded["evidence"] != evidence:
            fail("coordinator_source_replay_drift")
        return path
    write_atomic(path, canonical_json(value) + "\n")
    fsync_directory(directory, "coordinator_source_publish")
    read_private_source(context, path, step, kind)
    return path


def publish_bootstrap_source(context, evidence, observed_at):
    return publish_direct_source(
        context, 1, BOOTSTRAP_KIND, "bootstrap", evidence, observed_at
    )


def publish_prior_absence_source(context, preflight):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "manifest_sha256",
        "prior_runtime_owner_count",
        "prior_smoke_process_count",
        "standing_snapshot_sha256",
        "external_credential_count",
    }
    if (
        not isinstance(preflight, dict)
        or set(preflight) != fields
        or preflight["kind"] != PREFLIGHT_KIND
        or preflight["manifest_sha256"] != context.digest
    ):
        fail("preflight_source_invalid")
    evidence = {
        "prior_runtime_owner_count": preflight["prior_runtime_owner_count"],
        "prior_smoke_process_count": preflight["prior_smoke_process_count"],
    }
    return publish_direct_source(
        context,
        2,
        PRIOR_ABSENCE_KIND,
        "prior-absence",
        evidence,
        preflight["observed_at"],
    )


def publish_candidate_source(context, evidence, observed_at):
    return publish_direct_source(
        context, 3, CANDIDATE_KIND, "candidate", evidence, observed_at
    )


def publish_live_runtime_restart_source(context, evidence, observed_at):
    return publish_direct_source(
        context,
        11,
        LIVE_RUNTIME_RESTART_KIND,
        "live-runtime-restart",
        evidence,
        observed_at,
    )


def validate_onboarding_source(context, value):
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
    manifest = context.manifest
    if (
        not isinstance(value, dict)
        or set(value) != fields
        or type(value["schema_version"]) is not int
        or value["schema_version"] != SCHEMA_VERSION
        or value["kind"] != ONBOARDING_KIND
        or not validate_utc_timestamp(value["observed_at"])
        or value["manifest_sha256"] != context.digest
        or value["run_id"] != manifest["run_id"]
        or value["outcome"] not in {"fresh", "exact_replay"}
        or value["installation_id"]
        != f"installation:{manifest['discord']['resource_prefix']}"
        or value["principal_id"] != f"discord:{manifest['discord']['actor_id']}"
        or value["guild_id"] != manifest["discord"]["guild_id"]
        or value["discord_application_id"]
        != manifest["discord"]["application_id"]
        or value["binding_key"] != "community_hub"
        or value["hub_channel_id"] != manifest["discord"]["hub_channel_id"]
    ):
        fail("onboarding_source_invalid")
    return value


def onboarding_source_path(context):
    return source_path(context, 4, "onboarding")


def publish_onboarding_source(context, onboarding, observed_at):
    if not validate_utc_timestamp(observed_at):
        fail("coordinator_source_timestamp_invalid")
    onboarding_fields = {
        "outcome",
        "installation_id",
        "principal_id",
        "guild_id",
        "discord_application_id",
        "binding_key",
        "hub_channel_id",
    }
    if not isinstance(onboarding, dict) or set(onboarding) != onboarding_fields:
        fail("onboarding_source_invalid")
    ensure_source_directory(context)
    value = {
        "schema_version": SCHEMA_VERSION,
        "kind": ONBOARDING_KIND,
        "observed_at": observed_at,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "outcome": onboarding["outcome"],
        "installation_id": onboarding["installation_id"],
        "principal_id": onboarding["principal_id"],
        "guild_id": onboarding["guild_id"],
        "discord_application_id": onboarding["discord_application_id"],
        "binding_key": onboarding["binding_key"],
        "hub_channel_id": onboarding["hub_channel_id"],
    }
    validate_onboarding_source(context, value)
    path = onboarding_source_path(context)
    if os.path.lexists(path):
        try:
            require_owned_mode(path, 0o600, "onboarding_source")
            recorded = load_json_file(path, "onboarding_source")
        except Exception as error:
            fail(f"onboarding_source_invalid:{error}")
        validate_onboarding_source(context, recorded)
        stable_fields = set(value) - {"observed_at", "outcome"}
        if any(recorded[field] != value[field] for field in stable_fields):
            fail("onboarding_source_replay_drift")
        return path
    write_atomic(path, canonical_json(value) + "\n")
    require_owned_mode(path, 0o600, "onboarding_source")
    recorded = load_json_file(path, "onboarding_source")
    validate_onboarding_source(context, recorded)
    return path
