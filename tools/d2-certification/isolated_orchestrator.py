#!/usr/bin/env python3

import argparse
import datetime
import errno
import hashlib
import json
import os
import pathlib
import plistlib
import re
import signal
import stat
import subprocess
import sys
import time
import unicodedata

import d2_run


D2A_TAINT_FIELDS = (
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
)
D2A_SESSION_LIFECYCLE_FIELDS = (
    "schema_version",
    "kind",
    "run_id",
    "manifest_sha256",
    "operation",
    "origin",
    "issuer_sha256",
    "issuer_source_sha256",
    "uid",
    "boot_identity",
    "process_group_id",
    "started_at",
    "status",
    "session_revoked",
    "revoked_at",
    "quarantined_at",
)
D2A_TEARDOWN_FENCE_FIELDS = (
    "kind",
    "manifest_sha256",
    "run_id",
    "schema_version",
    "status",
    "updated_at",
)
D2A_DIGEST = re.compile(r"^[0-9a-f]{64}$")
D2A_BOOT_IDENTITY = re.compile(
    r"^darwin-boottime:(?:[1-9][0-9]*):(?:0|[1-9][0-9]{0,5})$"
)
D2A_LIFECYCLE_TIMESTAMP = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{9}Z$"
)
D2A_MARKER_MAXIMUM_BYTES = 64 * 1024
D2A_SYSCTL_PATH = pathlib.Path("/usr/sbin/sysctl")
D2A_SYSCTL_BOOT_TIME = re.compile(
    rb"^\{ sec = ([1-9][0-9]*), usec = (0|[1-9][0-9]{0,5}) \} "
    rb"[A-Z][a-z]{2} [A-Z][a-z]{2} [ 0-9][0-9] "
    rb"[0-9]{2}:[0-9]{2}:[0-9]{2} [0-9]{4}\n$"
)

from d2_certification import (
    COMMIT_PATTERN,
    CertificationError,
    STEP_SPECS,
    canonical_json,
    fsync_directory,
    isolated_runtime_root,
    load_json_file,
    observe_audited_recovery_source_trees,
    require_absolute_path,
    require_owned_mode,
    sha256_file,
    validate_snowflake,
    validate_step_contract,
    validate_utc_timestamp,
    write_new_file,
)
from d2_orchestrator_composition import (
    compose_plists,
    configure_postgres,
    configure_postgres_bootstrap_network,
    configure_postgres_sealed_network,
    write_keychain_plan,
    write_plists,
)
from d2_orchestrator_contract import (
    GLOBAL_LOCK_PATH,
    OWNER_ACCOUNT,
    PROTECTED_PORTS,
    STANDING_DISCORD_IDENTITIES,
    STANDING_PUBLIC_ORIGIN,
    OrchestratorError,
    append_journal,
    claim_discord_ownership,
    external_keychain_inventory,
    fail,
    global_operation_lock,
    keychain_inventory,
    load_audited_recovery_context,
    load_json,
    load_context,
    load_state,
    owner_identities,
    release_discord_ownership,
    read_strict_journal_snapshot,
    require_discord_ownership_available,
    require_discord_ownership_claimed,
    require_discord_ownership_released,
    save_state,
    standing_snapshot,
    utc_now,
    validate_candidate_programs,
    validate_dedicated_discord_identity,
    validate_ports,
    validate_programs,
    write_atomic,
)
from d2_orchestrator_platform import Platform, rename_exclusive
from d2_drained_runtime_restart import (
    command_restart_drained_runtime as run_restart_drained_runtime,
    drained_runtime_restart_directory,
    drained_runtime_restart_identity,
    drained_runtime_restart_inventory,
    drained_runtime_restart_temporary_directory,
    require_bound_runtime_generation,
)
from d2_finalization import (
    abort_teardown_evidence_path,
    abort_teardown_progress_path,
    abort_teardown_tombstone_path,
    certified_teardown_binding,
    command_finalize_run as run_finalize_run,
    command_finalize_total_absence as run_finalize_total_absence,
    effect_admission_freeze_intent_path,
    freeze_intent_path,
    require_certified_teardown_snapshot,
    require_certification_eligible_teardown,
    validate_runtime_freeze_binding,
)
from d2_live_runtime_restart import (
    committed_live_runtime_restart_chain,
    command_certify_live_runtime_restart as run_certify_live_runtime_restart,
    live_runtime_restart_complete_path,
    live_runtime_restart_directory,
    live_runtime_restart_intent_path,
)
from d2_legacy_substrate_recovery import (
    command_recover as command_recover_legacy_substrate,
    command_status as command_legacy_substrate_status,
    load_legacy_context,
    load_lifecycle_journal,
)
from d2_source_contract import (
    CANDIDATE_KIND,
    publish_bootstrap_source,
    publish_candidate_source,
    publish_onboarding_source,
    read_private_source,
    source_path,
)
from d2_worker_evidence import capture_worker_authoring_checkpoint


SERVICE_START_ORDER = ("transport", "worker", "api", "runtime", "tunnel")
SERVICE_STOP_ORDER = tuple(reversed(SERVICE_START_ORDER))
TRANSPORT_INSTANCE_PATTERN = re.compile(r"^d2ti-[0-9a-f]{32}$")
DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")
DISCORD_RESOURCE_KIND_ORDER = {"message": 0, "channel": 1, "role": 2}
DISCORD_RESOURCE_UNKNOWN_CODES = {
    "role": {10011},
    "channel": {10003},
    "message": {10003, 10008},
}
DISCORD_RESOURCE_SUCCESS_STATUS = {"role": 204, "channel": 200, "message": 204}
DISCORD_TEARDOWN_PROGRESS_KIND = "starring.d2.discord-resource-teardown-progress.v1"
DISCORD_TEARDOWN_EVIDENCE_KIND = "starring.d2.discord-resource-teardown.v1"
DISCORD_TEARDOWN_ABORT_KIND = "starring.d2.discord-resource-teardown-abort.v1"
CLEANUP_ROOT_PROGRESS_KIND = "starring.d2.cleanup-root-progress.v1"
CLEANUP_ROOT_IDENTITY_KIND = "starring.d2.cleanup-root-identity.v1"
CLEANUP_KEYCHAIN_BASELINE_KIND = "starring.d2.cleanup-keychain-baseline.v1"
CANDIDATE_START_TRANSITION_KIND = "starring.d2.candidate-start-transition.v1"
CANDIDATE_START_RETIREMENT_KIND = "starring.d2.candidate-start-retirement.v1"
CANDIDATE_START_RETIREMENT_REASONS = {
    "state_drift",
    "transition_invalid",
    "candidate_service_drift",
    "candidate_health_drift",
    "protected_staging_drift",
    "candidate_identity_drift",
    "candidate_source_drift",
    "explicit_stop",
    "explicit_cleanup",
}
RECONCILIATION_DISCORD_OBSERVATION_KIND = (
    "starring.d2.discord-reconciliation-role-observation.v1"
)
TRANSPORT_EVIDENCE_KINDS = {
    "interaction": "starring.d2.transport-resource-evidence.v1",
    "duplicate": "starring.d2.transport-duplicate-evidence.v1",
    "reconciliation": "starring.d2.transport-indeterminate-evidence.v1",
    "gateway-loss": "starring.d2.transport-gateway-loss-evidence.v1",
    "gateway-healed": "starring.d2.transport-gateway-healed-evidence.v1",
}

AUDITED_PREISSUER_ROLLBACK_INTENT_KIND = (
    "starring.d2.audited-preissuer-rollback-recovery-intent.v1"
)
AUDITED_PREISSUER_ROLLBACK_EVIDENCE_KIND = (
    "starring.d2.audited-preissuer-rollback-recovery.v1"
)
AUDITED_RECOVERY_GIT_PATH = pathlib.Path("/usr/bin/git")
AUDITED_RECOVERY_REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[2]
AUDITED_QUARANTINED_RECOVERY_INTENT_SHA256 = (
    "319d84e19680bf15d20f094d9f24bf90c82e88d2bbf0133e56295774c6724a96"
)
AUDITED_QUARANTINED_RECOVERY_FROM_COMMIT = (
    "5153709b7b0fd5f1a1ed1a8aebd19fc865d14d4c"
)
AUDITED_QUARANTINED_RECOVERY_FROM_TREE = (
    "9cbfedaa35d56c65230406cae2680ba90d491c23"
)
AUDITED_QUARANTINED_RECOVERY_CLOSING_FENCE_SHA256 = (
    "ff8ce5b477b3ff49abc3afd0ee2234457161e418c4fbfac1444117d0a3845908"
)
AUDITED_QUARANTINED_RECOVERY_CHANGED_PATHS = (
    "tools/d2-certification/d2_orchestrator_platform.py",
    "tools/d2-certification/isolated_orchestrator.py",
    "tools/d2-certification/test_isolated_orchestrator.py",
    "tools/d2-maintenance/d2a_bootstrap.py",
    "tools/d2-maintenance/test_d2a_bootstrap.py",
)
AUDITED_QUARANTINED_RECOVERY_FROM_FILE_SHA256 = {
    "tools/d2-certification/d2_orchestrator_platform.py":
        "4e98f759b00c8e806376d6795016201b38ead9b6db51a0171d8a45cc2a54c5cd",
    "tools/d2-certification/isolated_orchestrator.py":
        "8fe495be9c2d920a64d2ce1d5a2c654ceebc396cd0a5b9f5615f02f3b5962337",
    "tools/d2-certification/test_isolated_orchestrator.py":
        "c19f0739ecdc7463395452efe51b8a1eca113257f3cd4fe0109dc8a37ed037fc",
    "tools/d2-maintenance/d2a_bootstrap.py":
        "65be8662f513d8b3af8a234ef3956478e1642f8c7b7c5ef2b80e565eef33b7fb",
    "tools/d2-maintenance/test_d2a_bootstrap.py":
        "198f1373a2c6d339c3a2da89563c6d8b365abbbb9fddc0a87da3791897d7a9ab",
}
AUDITED_QUARANTINED_RECOVERY_REASON_CODES = (
    "darwin_service_group_eperm_observation",
    "explicit_login_keychain_lookup_required",
    "zsh_readonly_status_assignment_fixed",
)
AUDITED_QUARANTINED_RECOVERY_V1_INTERLOCK_SHA256 = (
    "4e7687fb0b9db69b44e62c974a68318a0f85765292a9c3c5fc88978a61c442ca"
)
AUDITED_QUARANTINED_RECOVERY_V1_TRANSITION_SHA256 = (
    "d63408478cedc7ab39835797c1d76a00f7cf9703eda3b41db1b4d960ae1c900d"
)
AUDITED_QUARANTINED_RECOVERY_V1_TO_COMMIT = (
    "a323333061c76645d83539714250286c590960ef"
)
AUDITED_QUARANTINED_RECOVERY_V1_TO_TREE = (
    "8b238fd8153a57605d40739c79b182da9c52bec6"
)
AUDITED_QUARANTINED_RECOVERY_V1_TO_FILE_SHA256 = {
    "tools/d2-certification/d2_orchestrator_platform.py":
        "c860e105a8b5e697e673046e2e52cf7db382e3354741695215ee1881d8de66e8",
    "tools/d2-certification/isolated_orchestrator.py":
        "702a27aa7567ac7bd09823c72a79d4261a61c307b75ab765b1233d4640026ff6",
    "tools/d2-certification/test_isolated_orchestrator.py":
        "2bc8b80d259414576e42bd3d4a7c0e1d181394506e9e27a60dd53ba70aa0b96a",
    "tools/d2-maintenance/d2a_bootstrap.py":
        "4b51a9b3429d8fb6fe15959732d11b9bf8b49184b1c01a75ce3811658eec4714",
    "tools/d2-maintenance/test_d2a_bootstrap.py":
        "80e5981f2793985c140c575b6e7ec3f8e1aa902316e2001175ee28519c00c1e7",
}
AUDITED_QUARANTINED_RECOVERY_V2_REASON_CODES = (
    "postgres_inet_host_address_normalization_required",
)
AUDITED_QUARANTINED_RECOVERY_V2_STATIC_SQL_SHA256 = (
    "669c4adf949acd954a44ba1205ff19de18664f599ca830efc5d60367710ec5e8"
)
AUDITED_QUARANTINED_RECOVERY_V2_INTERLOCK_SHA256 = (
    "ab387a6e14beeccfb7ec94f4af7f694e2eb93553da723e3c16ed6a82faad54ca"
)
AUDITED_QUARANTINED_RECOVERY_V2_TRANSITION_SHA256 = (
    "4a843deda6ff27e82e0bdebe739b1c838b0eca24fff2a39d3118cf4726b0d22b"
)
AUDITED_QUARANTINED_RECOVERY_DATABASE_SHA256 = (
    "c76600c617e1abfdfbb83d5e10eb390a24e37b6ee05f78fbd10ecce204e6087d"
)
AUDITED_QUARANTINED_RECOVERY_RECONCILIATION_SHA256 = (
    "66e27795c9d8276a6ee49547f9d39f8c9649aa74e2c98f5d893bd6be9528286a"
)
AUDITED_QUARANTINED_RECOVERY_V2_TO_COMMIT = (
    "894a87c74d3db4ce06e314f87d11eb1d2d1392ec"
)
AUDITED_QUARANTINED_RECOVERY_V2_TO_TREE = (
    "dd7a71c3d967e8c1df66f60aff5e40d72fb67c4f"
)
AUDITED_QUARANTINED_RECOVERY_V2_TO_FILE_SHA256 = {
    "tools/d2-certification/d2_orchestrator_platform.py":
        "ce0f277ca87c0a2303f4a0226dc91bc9f33a131af6ecc0165e01421e692476e3",
    "tools/d2-certification/isolated_orchestrator.py":
        "38a76c0c5a9f0e0687dab99fdb459114d2d496d60a077e9f0c794d3e36ce3522",
    "tools/d2-certification/test_isolated_orchestrator.py":
        "996af253cf454de7b55619b63660a2cb335cd8a90a87b3c3031cdec53ef23595",
    "tools/d2-maintenance/d2a_bootstrap.py":
        "0ee3db8d635fe69a98fc0a3e41c67dc646d703bc4e1468e19c028cbc460ced40",
    "tools/d2-maintenance/test_d2a_bootstrap.py":
        "8cde2461d64af080a6cc8418a01b16b21df417764a16c4cb39d1d209ad50ed79",
}
AUDITED_QUARANTINED_RECOVERY_CLOSED_FENCE_SHA256 = (
    "712b23bd7edf50c5c56a334557185d21af738891dc88dd2d56450886ac4e24a1"
)
AUDITED_QUARANTINED_RECOVERY_CLEANUP_JOURNAL_SHA256 = (
    "3bfa38faa428a18aee1b92cb8eb04f868693650e3da4ab3c4f548efb1df14b3b"
)
AUDITED_QUARANTINED_RECOVERY_CLEANUP_JOURNAL_ROWS = 46
AUDITED_QUARANTINED_RECOVERY_ROOT_IDENTITY_SHA256 = (
    "a1ca6a17183f71ea88478671033bc9247facf13c8d0524a6e8b559845f546fa3"
)
AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_INVENTORY_SHA256 = (
    "8f0cb1f256b4ee59c4842b35d3a0409f604d90170728bfb7afcaad14847e90ae"
)
AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHOR_POLICY_KIND = (
    "starring.d2.keychain-persistent-reference-anchor-policy.v1"
)
AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHORS = (
    (
        "starring.d2.credentials", "discord.bot-token",
        "967a3764ce452acf80391d8cf151d23d6f9de65f171ddceeba3e0f9109e3ad8d",
    ),
    (
        "starring.d2.credentials", "discord.oauth-client-secret",
        "5552c98317f8c54368423fc3cbf2ab9dbd38db7d15ec8934aa0de1db3d645b05",
    ),
    (
        "starring.d2.credentials", "cloudflare.tunnel-token",
        "b307682daf62b6f5df76512dbb679ca6b1b8b4817f5616404eefd4820e21dc7f",
    ),
)
AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHOR_INVENTORY_SHA256 = (
    "a89603986948be5df6498edcdfb98ec3c9e28c130af7182c832cbeda173e4b08"
)
AUDITED_QUARANTINED_RECOVERY_CLEANUP_REASON_CODES = (
    "cleanup_keychain_owner_account_import_required",
)
AUDITED_QUARANTINED_LOGIN_KEYCHAIN_PATH = (
    "/Users/jungbogeon/Library/Keychains/login.keychain-db"
)
AUDITED_QUARANTINED_LOGIN_KEYCHAIN_POLICY_KIND = (
    "starring.d2.keychain-path-policy.v1"
)
AUDITED_QUARANTINED_LOGIN_KEYCHAIN_POLICY_SHA256 = (
    "95a270a538f67d82a235593400bc7baf9ee8f6cb6b40665e6388398389113b69"
)
AUDITED_BOOTSTRAP_STATE_FIELDS = tuple(
    sorted(
        {
            "schema_version",
            "kind",
            "bootstrap_id",
            "status",
            "phase",
            "operation",
            "config_path",
            "config_sha256",
            "candidate_spec_path",
            "candidate_spec_sha256",
            "candidate_provenance_path",
            "candidate_provenance_sha256",
            "candidate_dependency_record_sha256",
            "candidate_dependency_tree_sha256",
            "source_commit_sha",
            "source_tree_sha",
            "run_id",
            "manifest_path",
            "manifest_sha256",
            "onboarding_evidence_path",
            "onboarding_evidence_sha256",
            "resource_prefix",
            "tool_digests",
            "issuer_build_environment",
            "records",
            "last_session_operation",
            "candidate_started",
            "discord_teardown_complete",
            "cleanup_complete",
            "postconditions_complete",
            "persistent_sandbox_retained",
            "release_eligible",
            "last_error",
            "updated_at",
        }
    )
)

# Historical identity of the sole run affected by the pre-issuer rollback
# cleanup authorization bug.  This allowlist never authorizes a current source
# revision; the operator must separately confirm the clean current HEAD/tree,
# which is durably recorded in the recovery intent.
AUDITED_PREISSUER_ROLLBACK_ALLOWLIST = {
    (
        "d2-20260812t051209z-24ff1c8acd61",
        "5c5b387843ef5eaa8265f56ab5afaea01477c4e74866725ae3a1b12fd516351a",
    ): {
        "manifest_commit_sha": "ec9c9e1d5340b5e3681fa846f33cc68102a526d4",
        "historical_d2_toolchain_sha256": "a45dea01b7a82be133edb2af7fb58105480592250d5d8648f2c6131fd36d673f",
        "historical_transport_sha256": "a0351a4da7926b67941acb244a895a4140065f5dac9392699d5700bc054a9c6a",
        "historical_worker_sha256": "39421ca38caeaec5c3f1889f0e09118ebbe815e169a63da5daa7333ec1a2312d",
        "bootstrap_id": "d2ab-69011c2016465efe179b3ca5a283247e",
        "bootstrap_state_sha256": "6de083f61148698b7612ed2e00ce50a4a73ad0f95ab0b450a87714f1be391513",
        "bootstrap_config_sha256": "34988a102c346be941246d48622b742fc9d17c1c837d396fbca2b5077243da10",
        "candidate_spec_sha256": "64a9136252bb6f51ae44190f531c2c8816a3d3b8e5edcd05d6679740e5ad1558",
        "candidate_provenance_sha256": "e84b7a42e68fc6232f4c10297801e6816a17677a61ebc1c27ced7df7dbd61699",
        "candidate_dependency_record_sha256": "25523c5b7d5c6db57a440324c31c33a58e841c07c4c84593afbe6d8e32cfb421",
        "candidate_dependency_tree_sha256": "1ac4e636067f59abc9d339b9f3d4414a53b535ce7be5004e8681036438df581b",
        "source_tree_sha": "ca53ff3c8cf56c5c6b42523eea39b50b31b7a254",
        "issuer_sha256": "598377b6bdc4bdf80f9faa680d91656886cdee0bee3df9262e88f3eff02fc06d",
        "issuer_source_sha256": "6b4e6cefdbe789508283a72363a7a22d21fdfe9a5b840efe0c85e1abd552fdb1",
        "orchestrator_state_sha256": "5fc656c2bafb480d87781715287ba8b44b7f5aaeeb4a4dff9a7ecc55b09af1d2",
        "journal_sha256": "d7855e3bbaaf2157bcdf97612f5697efe89c80893d9c01e74de512cb0d323c08",
        "journal_rows": 43,
        "taint_sha256": "eef9fdbc5cab11b38f3b2d23d55597316c626db064f623cfb465f0fb958c28ee",
        "lifecycle_sha256": "9c88a096a7b6b57775a6329a429cf98c4d62eb3dacdd2275888c9b19dd98dd53",
    }
}
AUDITED_PREISSUER_ROLLBACK_INTENT_FIELDS = tuple(
    sorted(
        {
            "schema_version",
            "kind",
            "run_id",
            "manifest_sha256",
            "bootstrap_id",
            "bootstrap_state_path",
            "bootstrap_state_sha256",
            "historical_manifest_commit_sha",
            "historical_source_trees",
            "current_source",
            "orchestrator_state_sha256",
            "baseline_journal_sha256",
            "baseline_journal_rows",
            "taint_sha256",
            "lifecycle_sha256",
            "created_at",
        }
    )
)
AUDITED_PREISSUER_ROLLBACK_EVIDENCE_FIELDS = tuple(
    sorted(
        {
            "schema_version",
            "kind",
            "run_id",
            "manifest_sha256",
            "intent_sha256",
            "observed_at",
            "database_absent",
            "postgres_process_absent",
            "launchd_jobs_absent",
            "keychain_items_absent",
            "isolated_root_absent",
            "protected_staging_unchanged",
            "teardown_fence_sha256",
            "cleanup_evidence_sha256",
        }
    )
)

AUDITED_QUARANTINED_NO_ISSUE_INTENT_KIND = (
    "starring.d2.audited-quarantined-no-issue-recovery-intent.v1"
)
AUDITED_QUARANTINED_NO_ISSUE_RECONCILIATION_KIND = (
    "starring.d2.audited-quarantined-no-issue-reconciliation.v1"
)
AUDITED_QUARANTINED_NO_ISSUE_DATABASE_KIND = (
    "starring.d2.audited-quarantined-no-issue-database-absence.v1"
)
AUDITED_QUARANTINED_NO_ISSUE_EVIDENCE_KIND = (
    "starring.d2.audited-quarantined-no-issue-recovery.v1"
)
AUDITED_QUARANTINED_NO_ISSUE_OPERATION_ID = (
    "audited-quarantine-recovery:c52d220457d1"
)
AUDITED_QUARANTINED_NO_ISSUE_ALLOWLIST = {
    (
        "d2-20260812t082042z-c52d220457d1",
        "a522e5c316f58f54df8c0ea69ab0f6aebb9ed85e7bb6ce45a2c02b5e63823338",
    ): {
        "manifest_commit_sha": "ab9b36f25f52ad1e3d82aee1a4dba12b00080e83",
        "historical_d2_toolchain_sha256": "4a903d58c433d09d99f3ac359b14fdcf8189c3574600f0ebaa0c4db03621b976",
        "historical_transport_sha256": "79aa836e039687ddcb2b91305d2def26798dcec72de5703734416e6864e1a1a9",
        "historical_worker_sha256": "39421ca38caeaec5c3f1889f0e09118ebbe815e169a63da5daa7333ec1a2312d",
        "bootstrap_id": "d2ab-734673ef116c48fa3fc51ec0b13e02fc",
        "bootstrap_state_sha256": "49bf6c2fc87ee24fadec3d49707c8b2a438590a091eac7e2f96155155e507c85",
        "bootstrap_semantic_sha256": "e9497355f1d7e97f3dd3245f95c96a24bf70f774ea72989b37750c7b912ab952",
        "bootstrap_config_sha256": "34988a102c346be941246d48622b742fc9d17c1c837d396fbca2b5077243da10",
        "candidate_spec_sha256": "e931caed786f96caba8c13c421b8adcbe67dfde50f3101944dd8ebc2a5dcb074",
        "candidate_provenance_sha256": "7c1454e10462febaf05e96626c22ebdf3a5b2810733cd150290e2088a0d6ce16",
        "candidate_dependency_record_sha256": "25523c5b7d5c6db57a440324c31c33a58e841c07c4c84593afbe6d8e32cfb421",
        "candidate_dependency_tree_sha256": "1ac4e636067f59abc9d339b9f3d4414a53b535ce7be5004e8681036438df581b",
        "source_tree_sha": "b6dfe75f129f6fd8d6e796260359437ed5a27e82",
        "issuer_sha256": "769d2b16a3c85f578a0b470c07bd0aaf13aa9bf51a8dd646153a28d48b1ca5cb",
        "issuer_source_sha256": "69743522a2961c06547efc3221035e8f166e69a603188879ff92a5155614b234",
        "orchestrator_state_sha256": "ad80ebfbf5b2d3fa1424709b8ee07a5cf7859d6966df3ab1c6c7ed0183482016",
        "journal_sha256": "94167d8dcbe70e4c2e065472bdcc52b1bdabe77b2409e9985cdd5254480e0cd3",
        "journal_rows": 44,
        "taint_sha256": "0addc12e7574d261aaaa4978b7f9214b274ada5e371432e447d3b67a07b3cdac",
        "lifecycle_sha256": "a1de160581e7da4475f4b722b90a802ad8c4ef22009a99e57adfe3c64d940f55",
        "candidate_start_transition_sha256": "e6dbdfdeba98f8b0a7273a9319281ed36e9045e2b8862431706fcf81aa45d35f",
        "database_evidence_sha256": "47c3f2a24f5c62db72cf0310745b5026d6973455263b379af0a61d0a8354ce12",
        "step_03_evidence_sha256": "c739fdc3fc8f6efe5b9c66a8ad308902b0afeb6fbd9265e2ae190e1cdc163893",
        "database_system_identifier": "7673057195867924427",
        "transport_instance_id": "d2ti-f3616791d71f2fdd92ceb1ec14b60cd5",
        "empty_transport_inventory_sha256": "74f1658b2a815d8100552f69b0b58d6eaefae99e76be80e2297c8d9f6cf1ebc4",
        "safe_after": "2026-08-12T08:23:13Z",
        "zsh_sha256": "1f473d234dd65157f530b4f676686517ec97fe9aa64c76d82f2611674cc44314",
        "security_sha256": "2d2578ef40e1524f0572133e0e838479439027d51bb693a9ddfb57f20bb69e87",
        "psql_sha256": "880f676c397eb38415a83c25900a755502254f186dd7c9e18cb96b2c943b557b",
        "static_sql_sha256": "56cfdd24f7f8f50b73c0fc1decc8a38a4bff34fc4b1832f152fb8d5b2872607d",
        "receipts_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "coordinator_lock_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "coordinator_source_sha256": {
            "step-01-bootstrap.json": "08cec720eeef40c6bce9c6a40ad0dac8fa0cecc7901b7d70ff6d61f71d29c5af",
            "step-02-prior-absence.json": "cade6176cde4ded6e03e612bce126c63ee707aa58f1500f2f3659888277c824e",
            "step-03-candidate.json": "44b8865ff2b6431f068c0b58ca61f7a29dc61131408b6ccc8de633c54b5564c9",
        },
        "tunnel_script_sha256": "526c9b27c65a350a754221abae92450d052306872dc166503d9aff1127ce19a4",
        "plist_sha256": {
            "api": "b3eb9bf355855f40eef980fe9bf0132bbcf46f22b3457b57dc4c690be77c82a5",
            "runtime": "8918a467ec404418df5d89e3c44035c5b4aef0d14e8b0f2b0b210fd8ea66488a",
            "transport": "0a738931d2dde05d41aec4e9076fa6de4a35d5733fdaf711e9e78fa33fd15126",
            "tunnel": "14cdc74f88231b638c7ef0505c8ee267744a4f9313af203a81f72716f6177386",
            "worker": "38e60569fa74051fcc79f2e346da802fe8b9c6b72da2818e959332301ef9860a",
        },
    }
}

AUDITED_QUARANTINED_SERVICE_IDENTITIES = {
    "transport": {
        "pid": 30996, "process_group_id": 30996,
        "program": "/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/d2-certification-transport",
        "arguments": [
            "/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/d2-certification-transport",
            "--root", "/private/tmp/starring-d2-d2-20260812t082042z-c52d220457d1",
            "--run-id", "d2-20260812t082042z-c52d220457d1",
            "--guild-id", "1536845588954353676", "--hub-channel-id", "1536845619266846792",
            "--actor-id", "1056857223529250906", "--bot-user-id", "1533144492293754900",
            "--gateway-listen", "127.0.0.1:29101", "--http-listen", "127.0.0.1:29102",
        ],
        "candidate": "certification_transport", "start_time_seconds": 1786522855,
        "start_time_microseconds": 426871, "device": 16777230, "inode": 48539870,
        "size": 5854464,
    },
    "worker": {
        "pid": 31004, "process_group_id": 31004,
        "program": "/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/node",
        "arguments": [
            "/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/node",
            "/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/codex-worker/worker.mjs",
        ],
        "candidate": "node", "start_time_seconds": 1786522855,
        "start_time_microseconds": 969142, "device": 16777230, "inode": 48539879,
        "size": 292386560,
    },
    "api": {
        "pid": 31105, "process_group_id": 31105,
        "program": "/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/starring-api",
        "arguments": ["/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/starring-api"],
        "candidate": "api", "start_time_seconds": 1786522860,
        "start_time_microseconds": 212847, "device": 16777230, "inode": 48539866,
        "size": 29124432,
    },
    "runtime": {
        "pid": 31113, "process_group_id": 31113,
        "program": "/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/starring-runtime",
        "arguments": ["/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/starring-runtime"],
        "candidate": "runtime", "start_time_seconds": 1786522860,
        "start_time_microseconds": 239860, "device": 16777230, "inode": 48539867,
        "size": 28613808,
    },
    "tunnel": {
        "pid": 31121, "process_group_id": 31121, "program": "/bin/zsh",
        "arguments": [
            "/bin/zsh",
            "/Users/jungbogeon/Library/Application Support/Starring/release-certifications/d2-20260812t082042z-c52d220457d1/orchestrator/run-tunnel.zsh",
        ],
        "candidate": "cloudflared", "start_time_seconds": 1786522860,
        "start_time_microseconds": 262639, "device": 16777230, "inode": 48539880,
        "size": 38238338,
    },
}

AUDITED_QUARANTINED_INTENT_FIELDS = tuple(sorted({
    "schema_version", "kind", "run_id", "manifest_sha256", "bootstrap_id",
    "bootstrap_state_path", "baseline_bootstrap_state_sha256",
    "historical_manifest_commit_sha", "historical_source_trees", "current_source",
    "orchestrator_state_sha256", "baseline_journal_sha256", "baseline_journal_rows",
    "taint_sha256", "lifecycle_sha256", "candidate_start_transition_sha256",
    "database_evidence_sha256", "step_03_evidence_sha256", "transport_instance_id",
    "pre_intent_transport_inventory_sha256", "effect_admission_operation_id",
    "database_system_identifier", "safe_after", "service_identities", "audit_tools",
    "receipts_sha256", "coordinator_lock_sha256", "coordinator_source_sha256",
    "plist_sha256", "tunnel_script_sha256",
    "created_at",
}))
AUDITED_QUARANTINED_SOURCE_TRANSITION_INTERLOCK_KIND = (
    "starring.d2.audited-quarantined-no-issue-recovery-source-transition-interlock.v1"
)
AUDITED_QUARANTINED_SOURCE_TRANSITION_KIND = (
    "starring.d2.audited-quarantined-no-issue-recovery-source-transition.v1"
)
AUDITED_QUARANTINED_SOURCE_TRANSITION_BASE_FIELDS = tuple(sorted({
    "schema_version", "kind", "run_id", "manifest_sha256", "intent_sha256",
    "from_source", "to_source", "parent_commit_sha", "parent_count",
    "changed_paths", "file_sha256", "reason_codes", "audit_configuration",
    "bootstrap_state_semantic_sha256", "orchestrator_state_sha256",
    "baseline_journal_sha256", "baseline_journal_rows", "lifecycle_sha256",
    "teardown_fence_sha256", "transport_instance_id",
    "transport_inventory_sha256", "effect_admission_operation_id",
    "producer_launchd_jobs_absent", "issuer_process_group_absent",
    "transport_identity_verified", "transport_effect_admission_drained",
    "postgres_running", "protected_staging_unchanged",
    "database_absence_marker_absent", "reconciliation_marker_absent",
    "recovery_evidence_absent", "created_at",
}))
AUDITED_QUARANTINED_SOURCE_TRANSITION_FIELDS = tuple(sorted({
    *AUDITED_QUARANTINED_SOURCE_TRANSITION_BASE_FIELDS, "interlock_sha256",
}))
AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_INTERLOCK_KIND = (
    "starring.d2.audited-quarantined-no-issue-recovery-"
    "source-transition-interlock.v2"
)
AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_KIND = (
    "starring.d2.audited-quarantined-no-issue-recovery-source-transition.v2"
)
AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_BASE_FIELDS = tuple(sorted({
    *AUDITED_QUARANTINED_SOURCE_TRANSITION_BASE_FIELDS,
    "previous_source_transition_sha256",
}))
AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_FIELDS = tuple(sorted({
    *AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_BASE_FIELDS,
    "interlock_sha256",
}))
AUDITED_QUARANTINED_CLEANUP_TRANSITION_INTERLOCK_KIND = (
    "starring.d2.audited-quarantined-no-issue-recovery-"
    "cleanup-transition-interlock.v1"
)
AUDITED_QUARANTINED_CLEANUP_TRANSITION_KIND = (
    "starring.d2.audited-quarantined-no-issue-recovery-cleanup-transition.v1"
)
AUDITED_QUARANTINED_CLEANUP_TRANSITION_BASE_FIELDS = tuple(sorted({
    "schema_version", "kind", "run_id", "manifest_sha256", "intent_sha256",
    "previous_source_transition_sha256", "database_absence_sha256",
    "reconciliation_sha256", "from_source", "to_source",
    "parent_commit_sha", "parent_count", "changed_paths", "file_sha256",
    "reason_codes", "audit_configuration", "bootstrap_state_semantic_sha256",
    "orchestrator_state_sha256", "baseline_journal_sha256",
    "baseline_journal_rows", "lifecycle_sha256", "teardown_fence_sha256",
    "cleanup_journal_sha256", "cleanup_journal_rows",
    "producer_launchd_jobs_absent", "issuer_process_group_absent",
    "postgres_absent", "postgres_process_absent", "keychain_inventory_sha256",
    "keychain_item_count", "keychain_anchor_policy_kind",
    "keychain_anchor_inventory_sha256", "keychain_anchor_item_count",
    "cleanup_root_identity_sha256",
    "isolated_root_retained",
    "cleanup_keychain_baseline_absent", "cleanup_root_progress_absent",
    "cleanup_evidence_absent", "protected_staging_unchanged", "created_at",
}))
AUDITED_QUARANTINED_CLEANUP_TRANSITION_FIELDS = tuple(sorted({
    *AUDITED_QUARANTINED_CLEANUP_TRANSITION_BASE_FIELDS, "interlock_sha256",
}))
AUDITED_QUARANTINED_RECONCILIATION_FIELDS = tuple(sorted({
    "schema_version", "kind", "run_id", "manifest_sha256", "intent_sha256",
    "observed_at", "lifecycle_sha256", "transport_instance_id",
    "pre_intent_transport_inventory_sha256", "effect_admission_operation_id",
    "effect_admission_status", "producer_launchd_jobs_absent",
    "issuer_process_group_absent", "post_drain_transport_inventory_sha256",
    "database_absence_sha256", "final_transport_inventory_sha256",
    "postgres_running", "protected_staging_unchanged", "source_transition_sha256",
}))
AUDITED_QUARANTINED_DATABASE_FIELDS = tuple(sorted({
    "schema_version", "kind", "run_id", "manifest_sha256", "intent_sha256",
    "post_drain_transport_inventory_sha256", "observed_at", "database_name",
    "database_system_identifier", "control_plane_identity", "topology_verified",
    "tables_locked", "locked_tables", "transaction_committed",
    "process_group_quiescent", "oauth_flow_count", "auth_session_count",
    "principal_count", "tenant_count", "installation_count",
    "authority_version_count", "runtime_slot_writer_fence_count", "zsh_sha256",
    "security_sha256", "psql_sha256", "static_sql_sha256",
    "source_transition_sha256", "login_keychain_path",
    "login_keychain_policy_kind", "login_keychain_policy_sha256",
    "login_keychain_policy_verified",
}))
AUDITED_QUARANTINED_EVIDENCE_FIELDS = tuple(sorted({
    "schema_version", "kind", "run_id", "manifest_sha256", "intent_sha256",
    "reconciliation_sha256", "database_absence_sha256", "observed_at",
    "lifecycle_sha256", "teardown_fence_sha256", "cleanup_evidence_sha256",
    "cleanup_keychain_baseline_sha256", "cleanup_root_progress_sha256",
    "database_absent", "postgres_process_absent", "launchd_jobs_absent",
    "keychain_items_absent", "isolated_root_absent", "protected_staging_unchanged",
    "source_transition_sha256",
}))


def candidate_launchd_labels(context):
    return tuple(
        context.manifest["services"][name]["label"] for name in SERVICE_START_ORDER
    )


def candidate_launchd_absent(context, platform):
    labels = candidate_launchd_labels(context)
    return all(platform.launchd_absent(label) for label in labels) and (
        platform.launchd_overrides_absent(labels)
    )


def command_dry_run(context, platform):
    validate_programs(platform)
    validate_candidate_programs(context, platform)
    validate_dedicated_discord_identity(context)
    require_discord_ownership_available(context)
    validate_ports(context, platform, require_available=True)
    if context.root.exists():
        fail("isolated_root_busy")
    if not candidate_launchd_absent(context, platform):
        fail("isolated_launchd_label_busy")
    for service, account in keychain_inventory(context):
        if platform.keychain_present(service, account):
            fail("isolated_keychain_identity_busy")
    for service, account in external_keychain_inventory(context):
        if not platform.keychain_present(service, account):
            fail("external_keychain_identity_absent")
    snapshot = standing_snapshot(context, platform)
    return {
        "status": "ready",
        "manifest_sha256": context.digest,
        "standing_snapshot": snapshot,
        "standing_mutation_allowed": False,
    }


def command_prepare(context, platform):
    if context.state_path.exists():
        state = load_state(context)
        if state["phase"] in {"prepared", "substrate_started", "stopped"}:
            require_discord_ownership_claimed(context)
            root_identity = load_cleanup_root_identity(context)
            root_metadata = cleanup_path_metadata(
                context.root, "cleanup_root_invalid"
            )
            if (
                root_identity is None
                or not cleanup_root_identity_matches(
                    root_metadata, root_identity
                )
                or not (context.cluster_root / "PG_VERSION").is_file()
                or any(
                    not platform.keychain_owner_matches(
                        service, context.manifest["run_id"]
                    )
                    for service, _account in owner_identities(context)
                )
                or (
                    state["phase"] == "substrate_started"
                    and not platform.postgres_running(context.cluster_root)
                )
            ):
                fail("prepared_state_drift")
            return {"status": "already_prepared", "phase": state["phase"]}
        if state["phase"] == "preparing":
            fail("orchestrator_recovery_required")
        fail("orchestrator_already_cleaned")
    preflight = command_dry_run(context, platform)
    context.artifact_directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    save_state(context, "preparing", preflight["standing_snapshot"])
    try:
        claim_discord_ownership(context)
        append_journal(context, "discord_ownership", "complete", "identity")
        append_journal(context, "prepare", "intent", "run")
        append_journal(context, "root_create", "intent", "isolated_root")
        context.root.mkdir(mode=0o700)
        record_cleanup_root_identity(context)
        context.socket_directory.mkdir(mode=0o700)
        context.log_directory.mkdir(mode=0o700)
        append_journal(context, "root_create", "complete", "isolated_root")
        append_journal(context, "initdb", "intent", "cluster")
        platform.initdb(context.cluster_root)
        if not (context.cluster_root / "PG_VERSION").is_file():
            fail("postgres_cluster_incomplete")
        append_journal(context, "initdb", "complete", "cluster")
        configure_postgres(context)
        write_plists(context, platform)
        write_keychain_plan(context)
        for service, account in owner_identities(context):
            append_journal(context, "keychain_owner_create", "intent", service)
            platform.keychain_write_new(
                service, account, context.manifest["run_id"].encode("ascii")
            )
            append_journal(context, "keychain_owner_create", "complete", service)
        state = save_state(context, "prepared", preflight["standing_snapshot"])
        append_journal(context, "prepare", "complete", "run")
        return {"status": "prepared", "phase": state["phase"]}
    except BaseException:
        try:
            cleanup(context, platform, preflight["standing_snapshot"], from_failure=True)
        except BaseException:
            append_journal(context, "prepare_cleanup", "failed", "run")
        raise


def managed_keychain_inventory(context):
    return tuple(
        (service, account)
        for service, account in keychain_inventory(context)
        if account != OWNER_ACCOUNT
    )


def managed_keychain_presence(context, platform):
    inventory = managed_keychain_inventory(context)
    present = sum(
        1 for service, account in inventory if platform.keychain_present(service, account)
    )
    return present, len(inventory)


def service_plist_path(context, name):
    label = context.manifest["services"][name]["label"]
    path = context.plist_directory / f"{label}.plist"
    try:
        metadata = path.lstat()
    except OSError:
        fail("candidate_plist_absent")
    if not stat.S_ISREG(metadata.st_mode) or path.is_symlink():
        fail("candidate_plist_invalid")
    return path


def candidate_health(context, platform, wait):
    manifest = context.manifest
    def observe(probe):
        return platform.wait_for_status(probe, 200) if wait else probe()

    worker_status = observe(lambda: platform.worker_health_status(context))
    transport_status = observe(lambda: platform.transport_health_status(context))
    api_status = observe(
        lambda: platform.http_status(
            f"http://127.0.0.1:{manifest['services']['api']['port']}/health/ready",
            host_header=manifest["public_origin"].removeprefix("https://"),
        )
    )
    runtime_status = observe(
        lambda: platform.http_status(
            f"http://127.0.0.1:{manifest['services']['runtime']['port']}/health/ready"
        )
    )
    tunnel_status = observe(
        lambda: platform.http_status(f"{manifest['public_origin']}/health/live")
    )
    return {
        "worker": worker_status,
        "transport": transport_status,
        "api": api_status,
        "runtime": runtime_status,
        "tunnel": tunnel_status,
    }


def require_started_dependency(context, platform, name):
    if name == "transport":
        status = platform.wait_for_status(
            lambda: platform.transport_health_status(context), 200
        )
    elif name == "worker":
        status = platform.wait_for_status(
            lambda: platform.worker_health_status(context), 200
        )
    else:
        return
    if status != 200:
        fail("candidate_health_unready")


def rollback_candidate_services(context, platform):
    failures = []
    for name in SERVICE_STOP_ORDER:
        label = context.manifest["services"][name]["label"]
        try:
            platform.launchd_bootout(label)
        except BaseException:
            failures.append(name)
    if failures:
        fail("candidate_service_rollback_incomplete")


def recover_interrupted_start(context, platform, state):
    rollback_candidate_services(context, platform)
    platform.postgres_stop(context.cluster_root)
    if platform.postgres_running(context.cluster_root):
        fail("interrupted_start_recovery_failed")
    save_state(context, "stopped", state["standing_snapshot"])
    append_journal(context, "interrupted_start", "recovered", "run")
    return load_state(context, {"stopped"})


def write_database_evidence(context, database_evidence):
    write_atomic(
        context.artifact_directory / "database-evidence.json",
        canonical_json(database_evidence) + "\n",
    )
    step_one_evidence = {
        "database_system_identifier": database_evidence[
            "database_system_identifier"
        ],
        "migration_count": database_evidence["migration_count"],
        "migration_head": database_evidence["migration_head"],
        "migration_ledger_sha256": database_evidence["migration_ledger_sha256"],
        "discord_resource_prefix": context.manifest["discord"]["resource_prefix"],
    }
    write_atomic(
        context.artifact_directory / "step-01-evidence.json",
        canonical_json(step_one_evidence) + "\n",
    )
    return publish_bootstrap_source(context, step_one_evidence, utc_now())


def candidate_plist_identity(context, name):
    path = service_plist_path(context, name)
    flags = os.O_RDONLY
    if hasattr(os, "O_CLOEXEC"):
        flags |= os.O_CLOEXEC
    if not hasattr(os, "O_NOFOLLOW"):
        fail(f"candidate_{name}_plist_nofollow_unavailable")
    flags |= os.O_NOFOLLOW
    if hasattr(os, "O_NONBLOCK"):
        flags |= os.O_NONBLOCK
    try:
        descriptor = os.open(path, flags)
    except OSError:
        fail(f"candidate_{name}_plist_unavailable")
    try:
        before = os.fstat(descriptor)
        mode = stat.S_IMODE(before.st_mode)
        if (
            not stat.S_ISREG(before.st_mode)
            or before.st_uid != os.getuid()
            or before.st_nlink != 1
            or before.st_size <= 0
            or before.st_size > 256 * 1024
            or mode != 0o600
        ):
            fail(f"candidate_{name}_plist_identity_invalid")
        raw = bytearray()
        while len(raw) <= 256 * 1024:
            chunk = os.read(descriptor, 64 * 1024)
            if not chunk:
                break
            raw.extend(chunk)
        after = os.fstat(descriptor)
        try:
            named = os.stat(path, follow_symlinks=False)
        except OSError:
            fail(f"candidate_{name}_plist_path_changed")
    finally:
        os.close(descriptor)
    metadata = (
        before.st_dev,
        before.st_ino,
        before.st_mode,
        before.st_uid,
        before.st_nlink,
        before.st_size,
        before.st_mtime_ns,
        before.st_ctime_ns,
    )
    if (
        len(raw) != before.st_size
        or metadata
        != (
            after.st_dev,
            after.st_ino,
            after.st_mode,
            after.st_uid,
            after.st_nlink,
            after.st_size,
            after.st_mtime_ns,
            after.st_ctime_ns,
        )
        or metadata
        != (
            named.st_dev,
            named.st_ino,
            named.st_mode,
            named.st_uid,
            named.st_nlink,
            named.st_size,
            named.st_mtime_ns,
            named.st_ctime_ns,
        )
    ):
        fail(f"candidate_{name}_plist_changed_during_observation")
    expected = plistlib.dumps(
        compose_plists(context)[name], fmt=plistlib.FMT_XML, sort_keys=True
    )
    if bytes(raw) != expected:
        fail(f"candidate_{name}_plist_content_mismatch")
    return {
        "path": str(path),
        "sha256": hashlib.sha256(raw).hexdigest(),
        "size": len(raw),
        "mode": mode,
        "uid": before.st_uid,
        "device": before.st_dev,
        "inode": before.st_ino,
        "links": before.st_nlink,
    }


def candidate_ready_status(context, platform, name):
    service = context.manifest["services"][name]
    host_header = None
    if name == "api":
        host_header = context.manifest["public_origin"].removeprefix("https://")
    return platform.http_status(
        f"http://127.0.0.1:{service['port']}/health/ready",
        host_header=host_header,
    )


def observe_candidate_process(context, platform, name):
    manifest = context.manifest
    service = manifest["services"][name]
    candidate = manifest["candidates"][name]
    expected_plist = str(service_plist_path(context, name))
    expected_arguments = [candidate["path"]]
    first_plist = candidate_plist_identity(context, name)
    first_job = platform.launchd_job(service["label"])
    if (
        not isinstance(first_job, dict)
        or set(first_job)
        != {
            "pid",
            "program",
            "plist_path",
            "arguments",
            "runs",
            "state",
            "last_exit_code",
        }
        or type(first_job["pid"]) is not int
        or first_job["pid"] <= 0
        or first_job["program"] != candidate["path"]
        or first_job["plist_path"] != expected_plist
        or first_job["arguments"] != expected_arguments
        or type(first_job["runs"]) is not int
        or first_job["runs"] <= 0
        or first_job["state"] != "running"
        or first_job["last_exit_code"] is not None
    ):
        fail(f"candidate_{name}_launchd_identity_invalid")
    first_process = platform.candidate_process_identity(
        first_job["pid"], pathlib.Path(candidate["path"])
    )
    ready_status = candidate_ready_status(context, platform, name)
    if ready_status != 200:
        fail(f"candidate_{name}_health_identity_unready")
    health_identity = None
    if name == "runtime":
        health_identity = platform.runtime_process_identity(context)
        if (
            not isinstance(health_identity, dict)
            or health_identity.get("os_pid") != first_job["pid"]
        ):
            fail("candidate_runtime_health_identity_mismatch")
    second_process = platform.candidate_process_identity(
        first_job["pid"], pathlib.Path(candidate["path"])
    )
    second_job = platform.launchd_job(service["label"])
    second_plist = candidate_plist_identity(context, name)
    if first_process != second_process:
        fail(f"candidate_{name}_process_identity_drift")
    if first_job != second_job:
        fail(f"candidate_{name}_launchd_identity_drift")
    if first_plist != second_plist:
        fail(f"candidate_{name}_plist_identity_drift")
    if first_process["sha256"] != candidate["sha256"]:
        fail(f"candidate_{name}_process_digest_mismatch")
    evidence = {
        "launchd": {
            "pid": first_job["pid"],
            "program": first_job["program"],
            "plist_path": first_job["plist_path"],
            "arguments": first_job["arguments"],
            "runs": first_job["runs"],
            "state": first_job["state"],
        },
        "process": first_process,
        "plist": first_plist,
    }
    if health_identity is not None:
        evidence["runtime_health"] = health_identity
    return evidence, ready_status


def revalidate_candidate_process(
    context, platform, name, evidence, expected_ready_status=200
):
    revalidate_candidate_process_identity(
        context, platform, name, evidence
    )
    if type(expected_ready_status) is int:
        allowed_ready_statuses = (expected_ready_status,)
    elif (
        isinstance(expected_ready_status, tuple)
        and expected_ready_status
        and all(type(status) is int for status in expected_ready_status)
    ):
        allowed_ready_statuses = expected_ready_status
    else:
        fail("candidate_ready_status_contract_invalid")
    if candidate_ready_status(
        context, platform, name
    ) not in allowed_ready_statuses:
        fail(f"candidate_{name}_health_final_unready")
    if name == "runtime":
        health = platform.runtime_process_identity(context)
        if health != evidence["runtime_health"]:
            fail("candidate_runtime_health_final_identity_drift")


def revalidate_candidate_process_identity(context, platform, name, evidence):
    service = context.manifest["services"][name]
    job = platform.launchd_job(service["label"])
    expected_job = {
        **evidence["launchd"],
        "last_exit_code": None,
    }
    if job != expected_job:
        fail(f"candidate_{name}_launchd_final_identity_drift")
    process = platform.candidate_process_identity(
        job["pid"], pathlib.Path(context.manifest["candidates"][name]["path"])
    )
    if process != evidence["process"]:
        fail(f"candidate_{name}_process_final_identity_drift")
    if candidate_plist_identity(context, name) != evidence["plist"]:
        fail(f"candidate_{name}_plist_final_identity_drift")
    return job


def build_candidate_evidence(context, statuses, platform):
    manifest = context.manifest
    transport_snapshot = platform.transport_control(context, "snapshot")
    observations = {
        name: observe_candidate_process(context, platform, name)
        for name in ("api", "runtime")
    }
    process_identities = {
        "schema_version": 1,
        "api": observations["api"][0],
        "runtime": observations["runtime"][0],
    }
    if (
        process_identities["api"]["launchd"]["pid"]
        == process_identities["runtime"]["launchd"]["pid"]
    ):
        fail("candidate_process_pid_collision")
    for name in ("api", "runtime"):
        revalidate_candidate_process(
            context, platform, name, process_identities[name]
        )
    return {
        "api_sha256": manifest["candidates"]["api"]["sha256"],
        "runtime_sha256": manifest["candidates"]["runtime"]["sha256"],
        "codex_worker_sha256": manifest["source_trees"]["codex_worker"]["sha256"],
        "d2_toolchain_sha256": manifest["source_trees"]["d2_toolchain"]["sha256"],
        "certification_transport_sha256": manifest["candidates"][
            "certification_transport"
        ]["sha256"],
        "certification_transport_source_sha256": manifest["source_trees"][
            "certification_transport"
        ]["sha256"],
        "api_build_revision": manifest["commit_sha"],
        "runtime_build_revision": manifest["commit_sha"],
        "api_ready_status": observations["api"][1],
        "runtime_ready_status": observations["runtime"][1],
        "worker_ready_status": statuses["worker"],
        "cloudflare_tunnel_id": manifest["cloudflare"]["tunnel_id"],
        "public_origin": manifest["cloudflare"]["public_origin"],
        "origin_service": manifest["cloudflare"]["origin_service"],
        "transport_instance_id": transport_snapshot["instance_id"],
        "transport_ready": statuses["transport"] == 200,
        "tunnel_ready": statuses["tunnel"] == 200,
        "process_identities": process_identities,
    }


def candidate_start_transition_path(context):
    return context.artifact_directory / "candidate-start-transition.json"


def candidate_start_source_path(context):
    return source_path(context, 3, "candidate")


def candidate_start_retirement_path(context):
    return context.artifact_directory / "candidate-start-retirement.json"


def candidate_start_commitment_present(context):
    return (
        os.path.lexists(candidate_start_transition_path(context))
        or os.path.lexists(candidate_start_source_path(context))
        or os.path.lexists(candidate_start_retirement_path(context))
    )


def digest_json(value):
    return hashlib.sha256(canonical_json(value).encode("utf-8")).hexdigest()


def load_candidate_start_retirement(context):
    path = candidate_start_retirement_path(context)
    try:
        require_owned_mode(path, 0o600, "candidate_start_retirement")
    except CertificationError as error:
        fail(str(error))
    value = load_json(path, "candidate_start_retirement_invalid")
    if (
        not isinstance(value, dict)
        or set(value)
        != {
            "schema_version",
            "kind",
            "manifest_sha256",
            "run_id",
            "observed_at",
            "transition_sha256",
            "reason",
        }
        or type(value["schema_version"]) is not int
        or value["schema_version"] != 1
        or value["kind"] != CANDIDATE_START_RETIREMENT_KIND
        or value["manifest_sha256"] != context.digest
        or value["run_id"] != context.manifest["run_id"]
        or not validate_utc_timestamp(value["observed_at"])
        or not isinstance(value["transition_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(value["transition_sha256"])
        or not isinstance(value["reason"], str)
        or value["reason"] not in CANDIDATE_START_RETIREMENT_REASONS
    ):
        fail("candidate_start_retirement_invalid")
    return value


def persist_candidate_start_retirement(context, transition, reason):
    if reason not in CANDIDATE_START_RETIREMENT_REASONS:
        fail("candidate_start_retirement_reason_invalid")
    path = candidate_start_retirement_path(context)
    if os.path.lexists(path):
        return load_candidate_start_retirement(context)
    value = {
        "schema_version": 1,
        "kind": CANDIDATE_START_RETIREMENT_KIND,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "observed_at": utc_now(),
        "transition_sha256": (
            digest_json(transition) if transition is not None else "0" * 64
        ),
        "reason": reason,
    }
    write_atomic(path, canonical_json(value) + "\n")
    if load_candidate_start_retirement(context) != value:
        fail("candidate_start_retirement_replay_drift")
    return value


def retire_candidate_start(context, transition, reason):
    persist_candidate_start_retirement(context, transition, reason)
    fail("candidate_start_transition_retirement_required")


def persist_candidate_abort_retirement(context, state, reason):
    if not candidate_start_commitment_present(context):
        return
    transition = None
    if os.path.lexists(candidate_start_transition_path(context)):
        try:
            transition, _ = load_candidate_start_transition(
                context, state["standing_snapshot"]
            )
        except OrchestratorError:
            transition = None
    persist_candidate_start_retirement(context, transition, reason)


def require_candidate_start_not_retired(
    context, allow_abort_teardown=False
):
    if os.path.lexists(candidate_start_retirement_path(context)):
        load_candidate_start_retirement(context)
        fail("candidate_start_transition_retirement_required")
    if (
        not allow_abort_teardown
        and os.path.lexists(abort_teardown_tombstone_path(context))
    ):
        fail("candidate_start_transition_retirement_required")


def finalization_freeze_committed(context):
    if os.path.lexists(effect_admission_freeze_intent_path(context)):
        return True
    if not os.path.lexists(freeze_intent_path(context)):
        return False
    certified_teardown_binding(context)
    return True


def require_finalization_not_started(context):
    if finalization_freeze_committed(context):
        fail("orchestrator_phase_invalid")


def command_restart_drained_runtime(context, platform):
    require_candidate_start_not_retired(context)
    state = load_state(context, {"candidate_started"})
    require_finalization_not_started(context)
    _transition, evidence, _source = require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_transport_identity(context, platform, state)
    try:
        result = run_restart_drained_runtime(
            context,
            platform,
            evidence["process_identities"]["runtime"]["launchd"]["runs"],
        )
    except OrchestratorError as error:
        require_committed_candidate_processes(
            context, platform, state, ("api",)
        )
        require_committed_transport_identity(context, platform, state)
        if str(error) in {
            "drained_runtime_restart_generation_unjournaled",
            "drained_runtime_restart_process_identity_changed",
            "drained_runtime_restart_replay_drift",
            "drained_runtime_restart_sequence_exhausted",
            "drained_runtime_restart_unjournaled_pid",
            "transport_instance_changed",
        }:
            retire_candidate_start(
                context, _transition, "candidate_identity_drift"
            )
        raise
    except BaseException:
        require_committed_candidate_processes(
            context, platform, state, ("api",)
        )
        require_committed_transport_identity(context, platform, state)
        raise
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_transport_identity(context, platform, state)
    return result


def command_certify_live_runtime_restart(
    context, platform, confirmation_path=None
):
    require_candidate_start_not_retired(context)
    require_finalization_not_started(context)
    state = load_state(context, {"candidate_started"})
    if not os.path.lexists(live_runtime_restart_intent_path(context)):
        transition, _evidence, _source = require_initial_candidate_commitment(
            context, platform, state
        )
    else:
        transition, _evidence, _source = require_committed_candidate_processes(
            context, platform, state, ("api",)
        )
    require_committed_transport_identity(context, platform, state)
    try:
        result = run_certify_live_runtime_restart(
            context, platform, confirmation_path
        )
    except OrchestratorError as error:
        require_committed_candidate_processes(
            context, platform, state, ("api",)
        )
        require_committed_transport_identity(context, platform, state)
        if str(error) in {
            "transport_instance_changed",
            "live_runtime_restart_transport_changed",
        }:
            retire_candidate_start(
                context, transition, "candidate_identity_drift"
            )
        raise
    except BaseException:
        require_committed_candidate_processes(
            context, platform, state, ("api",)
        )
        require_committed_transport_identity(context, platform, state)
        raise
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_transport_identity(context, platform, state)
    return result


def command_finalize_run(context, platform, teardown_boundary):
    require_candidate_start_not_retired(context)
    if not os.path.lexists(freeze_intent_path(context)):
        state = load_state(context, {"candidate_started"})
        require_committed_candidate_processes(
            context, platform, state, ("api",)
        )
        require_committed_runtime_generation(context, platform, state)
        require_committed_transport_identity(context, platform, state)

    def committed_identity_boundary(
        boundary_context, boundary_platform, action, runtime_binding
    ):
        if boundary_context is not context or boundary_platform is not platform:
            fail("finalization_identity_boundary_invalid")
        if action not in {"capture", "suspend", "checkpoint"}:
            fail("finalization_identity_boundary_invalid")
        boundary_state = load_state(boundary_context, {"candidate_started"})
        transition, _evidence, _source = require_committed_candidate_processes(
            boundary_context,
            boundary_platform,
            boundary_state,
            ("api",),
        )
        require_committed_transport_identity(
            boundary_context, boundary_platform, boundary_state
        )
        if action == "capture":
            if runtime_binding is not None:
                fail("finalization_identity_boundary_invalid")
            require_committed_runtime_generation(
                boundary_context, boundary_platform, boundary_state
            )
            captured, _ready = observe_candidate_process(
                boundary_context, boundary_platform, "runtime"
            )
            require_committed_runtime_generation(
                boundary_context, boundary_platform, boundary_state
            )
            revalidate_candidate_process(
                boundary_context,
                boundary_platform,
                "runtime",
                captured,
            )
            return captured
        try:
            committed_transition, _committed_evidence, _committed_source = (
                require_committed_runtime_freeze_binding(
                    boundary_context, boundary_state, runtime_binding
                )
            )
            if committed_transition != transition:
                fail("candidate_runtime_freeze_binding_drift")
            revalidate_candidate_process_identity(
                boundary_context,
                boundary_platform,
                "runtime",
                runtime_binding,
            )
        except OrchestratorError:
            retire_candidate_start(
                boundary_context, transition, "candidate_identity_drift"
            )
        runtime_path = pathlib.Path(
            boundary_context.manifest["candidates"]["runtime"]["path"]
        )
        runtime_pid = runtime_binding["launchd"]["pid"]
        runtime_process = runtime_binding["process"]
        try:
            if action == "suspend":
                boundary_platform.candidate_process_suspend(
                    runtime_pid, runtime_path, runtime_process
                )
            if not boundary_platform.candidate_process_stopped(
                runtime_pid, runtime_path, runtime_process
            ):
                fail("candidate_runtime_freeze_suspend_incomplete")
            revalidate_candidate_process_identity(
                boundary_context,
                boundary_platform,
                "runtime",
                runtime_binding,
            )
        except OrchestratorError:
            retire_candidate_start(
                boundary_context, transition, "candidate_identity_drift"
            )

    def certified_cleanup_boundary(boundary_context, boundary_platform):
        if boundary_context is not context or boundary_platform is not platform:
            fail("certified_cleanup_boundary_invalid")
        return command_cleanup_internal(
            boundary_context, boundary_platform, retire_committed=False
        )

    return run_finalize_run(
        context,
        platform,
        certified_cleanup_boundary,
        teardown_boundary,
        committed_identity_boundary,
    )


def command_finalize_total_absence(
    context, platform, prefix_scan_evidence_path, guild_deletion_evidence_path
):
    require_candidate_start_not_retired(context)
    return run_finalize_total_absence(
        context,
        platform,
        prefix_scan_evidence_path,
        guild_deletion_evidence_path,
    )


def load_candidate_start_transition(context, snapshot):
    path = candidate_start_transition_path(context)
    try:
        require_owned_mode(path, 0o600, "candidate_start_transition")
    except CertificationError as error:
        fail(str(error))
    transition = load_json(path, "candidate_start_transition_invalid")
    if (
        not isinstance(transition, dict)
        or set(transition)
        != {
            "schema_version",
            "kind",
            "manifest_sha256",
            "run_id",
            "observed_at",
            "evidence_sha256",
            "standing_snapshot_sha256",
        }
        or type(transition["schema_version"]) is not int
        or transition["schema_version"] != 1
        or transition["kind"] != CANDIDATE_START_TRANSITION_KIND
        or transition["manifest_sha256"] != context.digest
        or transition["run_id"] != context.manifest["run_id"]
        or not validate_utc_timestamp(transition["observed_at"])
        or not isinstance(transition["evidence_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(transition["evidence_sha256"])
        or not isinstance(transition["standing_snapshot_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(transition["standing_snapshot_sha256"])
        or transition["standing_snapshot_sha256"] != digest_json(snapshot)
    ):
        fail("candidate_start_transition_invalid")
    evidence = load_step_evidence(context, 3)
    try:
        validate_step_contract(3, evidence, context.manifest, [])
    except CertificationError as error:
        fail(f"candidate_start_transition_evidence_invalid:{error}")
    if transition["evidence_sha256"] != digest_json(evidence):
        fail("candidate_start_transition_evidence_drift")
    return transition, evidence


def stage_candidate_start_transition(context, evidence, snapshot):
    if candidate_start_commitment_present(context):
        fail("candidate_start_transition_reentry_invalid")
    write_atomic(
        context.artifact_directory / "step-03-evidence.json",
        canonical_json(evidence) + "\n",
    )
    transition = {
        "schema_version": 1,
        "kind": CANDIDATE_START_TRANSITION_KIND,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "observed_at": utc_now(),
        "evidence_sha256": digest_json(evidence),
        "standing_snapshot_sha256": digest_json(snapshot),
    }
    write_atomic(
        candidate_start_transition_path(context),
        canonical_json(transition) + "\n",
    )
    recorded, recorded_evidence = load_candidate_start_transition(context, snapshot)
    if recorded != transition or recorded_evidence != evidence:
        fail("candidate_start_transition_replay_drift")
    return transition


def load_step_evidence(context, step):
    path = context.artifact_directory / f"step-{step:02d}-evidence.json"
    try:
        require_owned_mode(path, 0o600, f"step_{step:02d}_evidence")
    except CertificationError as error:
        fail(str(error))
    evidence = load_json(path, f"step_{step:02d}_evidence_invalid")
    if not isinstance(evidence, dict) or set(evidence) != set(
        STEP_SPECS[step].required
    ):
        fail(f"step_{step:02d}_evidence_invalid")
    return evidence


def candidate_start_result(bootstrap_source, candidate_source, status):
    return {
        "status": status,
        "phase": "candidate_started",
        "candidate_services_loaded": True,
        "database_schema_ready": True,
        "credentials_sealed": True,
        "coordinator_sources": {
            "1": str(bootstrap_source),
            "3": str(candidate_source),
        },
    }


def require_committed_candidate_identity(
    context, platform, state, transition, evidence
):
    if not platform.postgres_running(context.cluster_root) or any(
        not platform.launchd_loaded(context.manifest["services"][name]["label"])
        for name in SERVICE_START_ORDER
    ):
        retire_candidate_start(context, transition, "candidate_service_drift")
    statuses = candidate_health(context, platform, wait=True)
    if any(status != 200 for status in statuses.values()):
        retire_candidate_start(context, transition, "candidate_health_drift")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        retire_candidate_start(context, transition, "protected_staging_drift")
    try:
        for name in ("api", "runtime"):
            revalidate_candidate_process(
                context,
                platform,
                name,
                evidence["process_identities"][name],
            )
        transport_snapshot = platform.transport_control(context, "snapshot")
    except OrchestratorError:
        retire_candidate_start(context, transition, "candidate_identity_drift")
    if transport_snapshot["instance_id"] != evidence["transport_instance_id"]:
        retire_candidate_start(context, transition, "candidate_identity_drift")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        retire_candidate_start(context, transition, "protected_staging_drift")


def publish_committed_candidate_source(context, transition, evidence):
    candidate_path = candidate_start_source_path(context)
    source_present = os.path.lexists(candidate_path)
    try:
        candidate_path = publish_candidate_source(
            context, evidence, transition["observed_at"]
        )
        source = read_private_source(context, candidate_path, 3, CANDIDATE_KIND)
    except OrchestratorError:
        if source_present or os.path.lexists(candidate_path):
            retire_candidate_start(
                context, transition, "candidate_source_drift"
            )
        raise
    if (
        source["observed_at"] != transition["observed_at"]
        or source["evidence"] != evidence
    ):
        retire_candidate_start(context, transition, "candidate_source_drift")
    return candidate_path


def read_committed_candidate_source(context, transition, evidence):
    candidate_path = candidate_start_source_path(context)
    if not os.path.lexists(candidate_path):
        retire_candidate_start(context, transition, "candidate_source_drift")
    try:
        source = read_private_source(context, candidate_path, 3, CANDIDATE_KIND)
    except OrchestratorError:
        retire_candidate_start(context, transition, "candidate_source_drift")
    if (
        source["observed_at"] != transition["observed_at"]
        or source["evidence"] != evidence
    ):
        retire_candidate_start(context, transition, "candidate_source_drift")
    return candidate_path


def load_committed_candidate_artifacts(context, state):
    if not os.path.lexists(candidate_start_transition_path(context)):
        retire_candidate_start(context, None, "transition_invalid")
    try:
        transition, evidence = load_candidate_start_transition(
            context, state["standing_snapshot"]
        )
    except OrchestratorError:
        retire_candidate_start(context, None, "transition_invalid")
    candidate_source = read_committed_candidate_source(
        context, transition, evidence
    )
    return transition, evidence, candidate_source


def require_committed_candidate_processes(
    context, platform, state, names
):
    if not names or any(name not in {"api", "runtime"} for name in names):
        fail("candidate_process_selection_invalid")
    transition, evidence, candidate_source = load_committed_candidate_artifacts(
        context, state
    )
    try:
        for name in names:
            revalidate_candidate_process(
                context,
                platform,
                name,
                evidence["process_identities"][name],
            )
    except OrchestratorError:
        retire_candidate_start(
            context, transition, "candidate_identity_drift"
        )
    return transition, evidence, candidate_source


def require_committed_runtime_generation(
    context, platform, state, expected_ready_status=200
):
    transition, evidence, candidate_source = load_committed_candidate_artifacts(
        context, state
    )
    try:
        records, pending = drained_runtime_restart_inventory(context)
        live_chain_before = committed_live_runtime_restart_chain(context)
    except OrchestratorError:
        retire_candidate_start(
            context, transition, "candidate_identity_drift"
        )
    if pending is not None or live_chain_before["status"] in {
        "pending",
        "complete_unpublished",
    }:
        fail("candidate_restart_protocol_pending")
    if not records:
        try:
            revalidate_candidate_process(
                context,
                platform,
                "runtime",
                evidence["process_identities"]["runtime"],
                expected_ready_status,
            )
        except OrchestratorError:
            retire_candidate_start(
                context, transition, "candidate_identity_drift"
            )
        try:
            live_chain_after = committed_live_runtime_restart_chain(context)
        except OrchestratorError:
            retire_candidate_start(
                context, transition, "candidate_identity_drift"
            )
        if live_chain_after != live_chain_before:
            retire_candidate_start(
                context, transition, "candidate_identity_drift"
            )
        return transition, evidence, candidate_source
    if len(records) != 1 or "complete" not in records[0]:
        retire_candidate_start(
            context, transition, "candidate_identity_drift"
        )
    completion = records[0]["complete"]
    try:
        generation = require_bound_runtime_generation(
            context,
            platform,
            drained_runtime_restart_identity(context),
            completion["new_pid"],
            completion["new_runs"],
            "candidate_runtime_generation_drift",
            expected_ready_status,
        )
    except OrchestratorError:
        retire_candidate_start(
            context, transition, "candidate_identity_drift"
        )
    if (
        generation["process_identity"]
        != completion["new_process_identity"]
        or generation["runtime_health"]
        != completion["new_runtime_health"]
    ):
        retire_candidate_start(
            context, transition, "candidate_identity_drift"
        )
    try:
        live_chain_after = committed_live_runtime_restart_chain(context)
    except OrchestratorError:
        retire_candidate_start(
            context, transition, "candidate_identity_drift"
        )
    if live_chain_after != live_chain_before:
        retire_candidate_start(
            context, transition, "candidate_identity_drift"
        )
    return transition, evidence, candidate_source


def require_committed_runtime_freeze_binding(context, state, binding):
    validate_runtime_freeze_binding(context, binding)
    transition, evidence, candidate_source = load_committed_candidate_artifacts(
        context, state
    )
    records, pending = drained_runtime_restart_inventory(context)
    live_chain = committed_live_runtime_restart_chain(context)
    if pending is not None or live_chain["status"] in {
        "pending",
        "complete_unpublished",
    }:
        fail("candidate_restart_protocol_pending")
    if not records:
        if binding != evidence["process_identities"]["runtime"]:
            fail("candidate_runtime_freeze_binding_drift")
        return transition, evidence, candidate_source
    if len(records) != 1 or "complete" not in records[0]:
        fail("candidate_runtime_freeze_binding_drift")
    completion = records[0]["complete"]
    launchd = binding["launchd"]
    if (
        binding["process"] != completion["new_process_identity"]
        or binding["runtime_health"] != completion["new_runtime_health"]
        or launchd["pid"] != completion["new_pid"]
        or launchd["runs"] != completion["new_runs"]
    ):
        fail("candidate_runtime_freeze_binding_drift")
    return transition, evidence, candidate_source


def require_committed_transport_snapshot(context, state, snapshot):
    transition, evidence, candidate_source = load_committed_candidate_artifacts(
        context, state
    )
    if (
        not isinstance(snapshot, dict)
        or snapshot.get("instance_id")
        != evidence["transport_instance_id"]
    ):
        retire_candidate_start(
            context, transition, "candidate_identity_drift"
        )
    return transition, evidence, candidate_source


def require_committed_transport_identity(context, platform, state):
    snapshot = platform.transport_control(context, "snapshot")
    require_committed_transport_snapshot(context, state, snapshot)
    return snapshot


def require_initial_candidate_commitment(context, platform, state):
    transition, evidence, candidate_source = load_committed_candidate_artifacts(
        context, state
    )
    require_committed_candidate_identity(
        context, platform, state, transition, evidence
    )
    require_committed_candidate_identity(
        context, platform, state, transition, evidence
    )
    return transition, evidence, candidate_source


def candidate_restart_protocol_committed(context):
    live_chain = committed_live_runtime_restart_chain(context)
    if live_chain["status"] != "absent":
        return live_chain["status"]
    records, _pending = drained_runtime_restart_inventory(context)
    return "drained" if records else None


def recover_candidate_start_transition(context, platform, state):
    require_candidate_start_not_retired(context)
    if state["phase"] != "candidate_starting":
        retire_candidate_start(context, None, "state_drift")
    if not os.path.lexists(candidate_start_transition_path(context)):
        retire_candidate_start(context, None, "transition_invalid")
    try:
        transition, evidence = load_candidate_start_transition(
            context, state["standing_snapshot"]
        )
    except OrchestratorError:
        retire_candidate_start(context, None, "transition_invalid")
    require_committed_candidate_identity(
        context, platform, state, transition, evidence
    )
    candidate_path = publish_committed_candidate_source(
        context, transition, evidence
    )
    require_committed_candidate_identity(
        context, platform, state, transition, evidence
    )
    bootstrap_source = publish_bootstrap_source(
        context, load_step_evidence(context, 1), utc_now()
    )
    append_journal(
        context,
        "candidate_start_transition",
        "complete",
        transition["evidence_sha256"],
    )
    append_journal(context, "postgres_start", "complete", "cluster")
    require_committed_candidate_identity(
        context, platform, state, transition, evidence
    )
    save_state(context, "candidate_started", state["standing_snapshot"])
    require_committed_candidate_identity(
        context, platform, state, transition, evidence
    )
    return candidate_start_result(
        bootstrap_source, candidate_path, "candidate_start_recovered"
    )


def pinned_transport_instance_id(context):
    path = context.artifact_directory / "step-03-evidence.json"
    try:
        metadata = path.lstat()
    except OSError:
        fail("transport_instance_evidence_absent")
    if (
        not stat.S_ISREG(metadata.st_mode)
        or path.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o600
    ):
        fail("transport_instance_evidence_invalid")
    evidence = load_json(path, "transport_instance_evidence_invalid")
    instance_id = evidence.get("transport_instance_id") if isinstance(evidence, dict) else None
    if not isinstance(instance_id, str) or not TRANSPORT_INSTANCE_PATTERN.fullmatch(
        instance_id
    ):
        fail("transport_instance_evidence_invalid")
    return instance_id


def require_pinned_transport_snapshot(context, snapshot):
    if snapshot["instance_id"] != pinned_transport_instance_id(context):
        fail("transport_instance_changed")


def command_start(context, platform):
    state = load_state(context)
    require_candidate_start_not_retired(context)
    require_finalization_not_started(context)
    if state["phase"] in {"cleaned", "onboarding"}:
        fail("orchestrator_phase_invalid")
    if state["phase"] == "candidate_started":
        try:
            restart_protocol_committed = candidate_restart_protocol_committed(
                context
            )
        except OrchestratorError:
            transition, _evidence, _source = (
                load_committed_candidate_artifacts(context, state)
            )
            retire_candidate_start(
                context, transition, "candidate_identity_drift"
            )
        if restart_protocol_committed is not None:
            require_committed_candidate_processes(
                context, platform, state, ("api",)
            )
            if restart_protocol_committed != "pending":
                require_committed_runtime_generation(context, platform, state)
            fail("orchestrator_phase_invalid")
        _transition, _evidence, candidate_source = (
            require_initial_candidate_commitment(
                context, platform, state
            )
        )
        bootstrap_source = publish_bootstrap_source(
            context, load_step_evidence(context, 1), utc_now()
        )
        return {
            "status": "already_started",
            "phase": "candidate_started",
            "coordinator_sources": {
                "1": str(bootstrap_source),
                "3": str(candidate_source),
            },
        }
    if candidate_start_commitment_present(context):
        return recover_candidate_start_transition(context, platform, state)
    if state["phase"] in {
        "substrate_starting",
        "substrate_started",
        "credentials_sealing",
        "candidate_starting",
    }:
        state = recover_interrupted_start(context, platform, state)
    if state["phase"] not in {"prepared", "stopped"}:
        fail("orchestrator_phase_invalid")
    if platform.postgres_running(context.cluster_root):
        fail("postgres_state_drift")
    rollback_candidate_services(context, platform)
    if not candidate_launchd_absent(context, platform):
        fail("isolated_launchd_label_busy")
    validate_ports(context, platform, require_available=True)
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    save_state(context, "substrate_starting", state["standing_snapshot"])
    append_journal(context, "postgres_start", "intent", "cluster")
    try:
        configure_postgres_bootstrap_network(context)
        platform.postgres_start(context.cluster_root, context.postgres_log)
        if not platform.postgres_running(context.cluster_root):
            fail("postgres_start_unconfirmed")
        if not platform.port_available(context.manifest["database"]["port"]):
            fail("bootstrap_tcp_exposure_detected")
        append_journal(context, "database_bootstrap", "intent", "database")
        database_evidence = platform.bootstrap_database(context)
        bootstrap_source = write_database_evidence(context, database_evidence)
        append_journal(context, "database_bootstrap", "complete", "database")
        save_state(context, "credentials_sealing", state["standing_snapshot"])
        present, total = managed_keychain_presence(context, platform)
        if present != total:
            provisioning = platform.provision_credentials(context)
            if present == 0 and provisioning["outcome"] != "fresh":
                fail("sealed_provisioning_outcome_invalid")
            if present != 0:
                fail("partial_credentials_not_quarantined")
        configure_postgres_sealed_network(context)
        platform.postgres_stop(context.cluster_root)
        platform.postgres_start(context.cluster_root, context.postgres_log)
        if not platform.postgres_loopback_accepting(context):
            fail("sealed_postgres_unready")
        replay = platform.provision_credentials(context)
        if replay["outcome"] != "exact_replay":
            fail("sealed_replay_required")
        save_state(context, "candidate_starting", state["standing_snapshot"])
        for name in SERVICE_START_ORDER:
            label = context.manifest["services"][name]["label"]
            append_journal(context, "launchd_start", "intent", label)
            platform.launchd_start(label, service_plist_path(context, name))
            append_journal(context, "launchd_start", "complete", label)
            require_started_dependency(context, platform, name)
        statuses = candidate_health(context, platform, wait=True)
        if any(status != 200 for status in statuses.values()):
            fail("candidate_health_unready")
        if standing_snapshot(context, platform) != state["standing_snapshot"]:
            fail("protected_staging_state_changed")
        candidate_evidence = build_candidate_evidence(context, statuses, platform)
        if standing_snapshot(context, platform) != state["standing_snapshot"]:
            fail("protected_staging_state_changed")
        transition = stage_candidate_start_transition(
            context, candidate_evidence, state["standing_snapshot"]
        )
        require_committed_candidate_identity(
            context, platform, state, transition, candidate_evidence
        )
        candidate_source = publish_committed_candidate_source(
            context, transition, candidate_evidence
        )
        require_committed_candidate_identity(
            context, platform, state, transition, candidate_evidence
        )
        append_journal(
            context,
            "candidate_start_transition",
            "complete",
            transition["evidence_sha256"],
        )
        append_journal(context, "postgres_start", "complete", "cluster")
        require_committed_candidate_identity(
            context, platform, state, transition, candidate_evidence
        )
        save_state(context, "candidate_started", state["standing_snapshot"])
        require_committed_candidate_identity(
            context, platform, state, transition, candidate_evidence
        )
        return candidate_start_result(
            bootstrap_source, candidate_source, "candidate_started"
        )
    except BaseException:
        if candidate_start_commitment_present(context):
            raise
        try:
            rollback_candidate_services(context, platform)
            platform.postgres_stop(context.cluster_root)
            save_state(context, "stopped", state["standing_snapshot"])
            append_journal(context, "candidate_start", "rolled_back", "run")
        except BaseException:
            append_journal(context, "candidate_start", "rollback_failed", "run")
        raise


def command_stop(context, platform):
    state = load_state(
        context,
        {
            "prepared",
            "substrate_starting",
            "substrate_started",
            "credentials_sealing",
            "candidate_starting",
            "candidate_started",
            "onboarding",
            "stopped",
        },
    )
    persist_candidate_abort_retirement(context, state, "explicit_stop")
    failures = []
    for name in SERVICE_STOP_ORDER:
        label = context.manifest["services"][name]["label"]
        append_journal(context, "launchd_bootout", "intent", label)
        try:
            platform.launchd_bootout(label)
            append_journal(context, "launchd_bootout", "complete", label)
        except BaseException:
            failures.append(name)
            append_journal(context, "launchd_bootout", "failed", label)
    append_journal(context, "postgres_stop", "intent", "cluster")
    try:
        platform.postgres_stop(context.cluster_root)
    except BaseException:
        failures.append("postgres")
    try:
        if any(
            not platform.launchd_absent(service["label"])
            for service in context.manifest["services"].values()
        ):
            failures.append("launchd_absence")
    except BaseException:
        failures.append("launchd_observation")
    try:
        if not cleanup_postgres_absent(context, platform):
            failures.append("postgres_absence")
    except BaseException:
        failures.append("postgres_observation")
    if failures:
        fail("candidate_stop_incomplete")
    append_journal(context, "postgres_stop", "complete", "cluster")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    save_state(context, "stopped", state["standing_snapshot"])
    return {"status": "stopped", "phase": "stopped"}


def command_onboard(context, platform, principal_id, display_name):
    require_candidate_start_not_retired(context)
    require_finalization_not_started(context)
    state = load_state(context, {"candidate_started", "onboarding"})
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_runtime_generation(context, platform, state)
    require_committed_transport_identity(context, platform, state)
    if not principal_id.startswith("discord:"):
        fail("onboarding_principal_invalid")
    validate_snowflake(principal_id.removeprefix("discord:"), "onboarding_principal")
    if principal_id != f"discord:{context.manifest['discord']['actor_id']}":
        fail("onboarding_principal_invalid")
    if (
        not display_name
        or len(display_name.encode("utf-8")) > 512
        or len(display_name) > 128
        or display_name != display_name.strip()
        or any(unicodedata.category(character).startswith("C") for character in display_name)
    ):
        fail("onboarding_display_name_invalid")
    if not platform.postgres_running(context.cluster_root) or any(
        not platform.launchd_loaded(context.manifest["services"][name]["label"])
        for name in SERVICE_START_ORDER
    ):
        fail("candidate_state_drift")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    installation_id = (
        f"installation:{context.manifest['discord']['resource_prefix']}"
    )
    save_state(context, "onboarding", state["standing_snapshot"])
    append_journal(context, "installation_onboard", "intent", "installation")
    try:
        evidence = platform.onboard_installation(
            context, principal_id, display_name, installation_id
        )
        require_committed_candidate_processes(
            context, platform, state, ("api",)
        )
        require_committed_runtime_generation(context, platform, state)
        require_committed_transport_identity(context, platform, state)
        output = {
            "outcome": evidence["outcome"],
            "installation_id": evidence["installation_id"],
            "principal_id": evidence["principal_id"],
            "guild_id": context.manifest["discord"]["guild_id"],
            "discord_application_id": context.manifest["discord"]["application_id"],
            "binding_key": evidence["binding_key"],
            "hub_channel_id": evidence["hub_channel_id"],
        }
        write_atomic(
            context.artifact_directory / "onboarding-evidence.json",
            canonical_json(output) + "\n",
        )
        coordinator_source = publish_onboarding_source(
            context, output, utc_now()
        )
        require_committed_candidate_processes(
            context, platform, state, ("api",)
        )
        require_committed_runtime_generation(context, platform, state)
        require_committed_transport_identity(context, platform, state)
        append_journal(context, "installation_onboard", "complete", "installation")
        save_state(context, "candidate_started", state["standing_snapshot"])
        return {
            "status": "onboarded",
            **output,
            "coordinator_source": str(coordinator_source),
        }
    except BaseException:
        save_state(context, "candidate_started", state["standing_snapshot"])
        append_journal(context, "installation_onboard", "failed", "installation")
        raise


TRANSPORT_OPERATIONS = {
    "snapshot": "snapshot",
    "arm-next-duplicate": "arm_next_duplicate",
    "disarm-duplicate": "disarm_duplicate",
    "arm-next-indeterminate": "arm_next_create_role_indeterminate",
    "disarm-indeterminate": "disarm_indeterminate",
    "partition-gateway": "partition_gateway",
    "heal-gateway": "heal_gateway",
}
TRANSPORT_CONTROL_FILE_PATTERN = re.compile(
    r"^([0-9]{4})-([a-z-]+)-(intent|complete)\.json$"
)
TRANSPORT_OPERATION_ID_PATTERN = re.compile(r"^[a-z][a-z0-9_.:-]{7,95}$")
TRANSPORT_RECORDED_AT_PATTERN = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$"
)
EVIDENCE_RECORDED_AT_PATTERN = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?Z$"
)


def transport_control_directory(context):
    return context.artifact_directory / "transport-controls"


def transport_control_inventory(context):
    directory = transport_control_directory(context)
    if not directory.exists():
        return [], None
    metadata = directory.lstat()
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or directory.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
    ):
        fail("transport_evidence_directory_invalid")
    records = {}
    try:
        entries = list(directory.iterdir())
    except OSError:
        fail("transport_evidence_inventory_invalid")
    for entry in entries:
        match = TRANSPORT_CONTROL_FILE_PATTERN.fullmatch(entry.name)
        try:
            entry_metadata = entry.lstat()
        except OSError:
            fail("transport_evidence_inventory_invalid")
        if (
            match is None
            or not stat.S_ISREG(entry_metadata.st_mode)
            or entry.is_symlink()
            or entry_metadata.st_uid != os.getuid()
            or stat.S_IMODE(entry_metadata.st_mode) != 0o600
        ):
            fail("transport_evidence_inventory_invalid")
        sequence = int(match.group(1))
        operation = match.group(2)
        kind = match.group(3)
        if sequence == 0 or operation not in TRANSPORT_OPERATIONS:
            fail("transport_evidence_inventory_invalid")
        record = records.setdefault(sequence, {"operation": operation})
        if record["operation"] != operation or kind in record:
            fail("transport_evidence_inventory_invalid")
        record[kind] = load_json(entry, "transport_evidence_invalid")
    ordered = []
    for expected_sequence, sequence in enumerate(sorted(records), 1):
        if sequence != expected_sequence:
            fail("transport_evidence_inventory_invalid")
        record = records[sequence]
        if "intent" not in record:
            fail("transport_evidence_inventory_invalid")
        intent = record["intent"]
        expected_intent = {
            "schema_version",
            "manifest_sha256",
            "recorded_at",
            "sequence",
            "operation",
            "command",
            "operation_id",
        }
        if (
            not isinstance(intent, dict)
            or set(intent) != expected_intent
            or intent["schema_version"] != 1
            or intent["manifest_sha256"] != context.digest
            or intent["sequence"] != sequence
            or intent["operation"] != record["operation"]
            or intent["command"] != TRANSPORT_OPERATIONS[record["operation"]]
            or intent["operation_id"]
            != f"d2:{context.digest[:16]}:{sequence:04d}:{record['operation']}"
            or not isinstance(intent["recorded_at"], str)
            or not TRANSPORT_RECORDED_AT_PATTERN.fullmatch(intent["recorded_at"])
            or not isinstance(intent["operation_id"], str)
            or not TRANSPORT_OPERATION_ID_PATTERN.fullmatch(intent["operation_id"])
        ):
            fail("transport_evidence_invalid")
        complete = record.get("complete")
        if complete is not None:
            if (
                not isinstance(complete, dict)
                or set(complete)
                != {
                    "schema_version",
                    "manifest_sha256",
                    "recorded_at",
                    "sequence",
                    "operation",
                    "command",
                    "operation_id",
                    "response",
                    "snapshot",
                }
                or complete["schema_version"] != 1
                or complete["manifest_sha256"] != context.digest
                or complete["sequence"] != sequence
                or complete["operation"] != intent["operation"]
                or complete["command"] != intent["command"]
                or complete["operation_id"] != intent["operation_id"]
                or not isinstance(complete["recorded_at"], str)
                or not TRANSPORT_RECORDED_AT_PATTERN.fullmatch(
                    complete["recorded_at"]
                )
                or complete["response"] is not None
                and not isinstance(complete["response"], dict)
                or not isinstance(complete["snapshot"], dict)
            ):
                fail("transport_evidence_invalid")
        ordered.append({"sequence": sequence, **record})
    pending = [record for record in ordered if "complete" not in record]
    if len(pending) > 1 or pending and pending[0] is not ordered[-1]:
        fail("transport_evidence_inventory_invalid")
    return ordered, pending[0] if pending else None


def transport_operation_postcondition(operation, operation_id, response, snapshot):
    gateway = snapshot["gateway"]
    effect = snapshot["effect_http"]
    if operation == "snapshot":
        return
    if operation == "arm-next-duplicate":
        if response["disposition"] == "busy":
            fail("transport_operation_busy")
        if not (
            gateway["armed_duplicate_operation_id"] == operation_id
            or gateway["claimed_duplicate_operation_id"] == operation_id
            or gateway["last_duplicate_operation_id"] == operation_id
        ):
            fail("transport_operation_not_applied")
        return
    if operation == "arm-next-indeterminate":
        if response["disposition"] == "busy":
            fail("transport_operation_busy")
        if not (
            effect["armed_indeterminate_operation_id"] == operation_id
            or effect["claimed_indeterminate_operation_id"] == operation_id
            or effect["last_indeterminate_operation_id"] == operation_id
        ):
            fail("transport_operation_not_applied")
        return
    expected = {
        "disarm-duplicate": not gateway["duplicate_armed"]
        and not gateway["duplicate_claimed"],
        "disarm-indeterminate": not effect["indeterminate_armed"]
        and not effect["indeterminate_claimed"],
        "partition-gateway": gateway["partitioned"],
        "heal-gateway": not gateway["partitioned"],
    }[operation]
    if not expected:
        fail("transport_operation_not_applied")


def validate_transport_control_history(context, records):
    validator = Platform()
    pinned_instance_id = pinned_transport_instance_id(context)
    for record in records:
        complete = record.get("complete")
        if complete is None:
            continue
        response = complete["response"]
        operation = complete["operation"]
        if operation == "snapshot":
            if response is not None:
                fail("transport_evidence_invalid")
        elif operation in {"arm-next-duplicate", "arm-next-indeterminate"}:
            if (
                not isinstance(response, dict)
                or set(response) != {"changed", "disposition"}
                or type(response["changed"]) is not bool
                or response["disposition"] not in {"armed", "replayed"}
                or (response["disposition"] == "armed") != response["changed"]
            ):
                fail("transport_evidence_invalid")
        elif (
            not isinstance(response, dict)
            or set(response) != {"changed"}
            or type(response["changed"]) is not bool
        ):
            fail("transport_evidence_invalid")
        snapshot = complete["snapshot"]
        if (
            not validator._transport_snapshot_valid(context, snapshot)
            or snapshot["instance_id"] != pinned_instance_id
        ):
            fail("transport_evidence_invalid")
        transport_operation_postcondition(
            operation, complete["operation_id"], response, snapshot
        )


def gateway_control_completion_bindings(context, expected_operations):
    records, pending = transport_control_inventory(context)
    validate_transport_control_history(context, records)
    if pending is not None:
        fail("transport_gateway_operation_pending")
    gateway_records = [
        record
        for record in records
        if record["operation"] in {"partition-gateway", "heal-gateway"}
    ]
    if [record["operation"] for record in gateway_records] != list(
        expected_operations
    ):
        fail("transport_gateway_operation_history_invalid")
    bindings = []
    for record in gateway_records:
        complete = record.get("complete")
        if complete is None:
            fail("transport_gateway_operation_incomplete")
        bindings.append(
            {
                "operation_id": complete["operation_id"],
                "completion_sha256": hashlib.sha256(
                    canonical_json(complete).encode("utf-8")
                ).hexdigest(),
                "snapshot": complete["snapshot"],
            }
        )
    return bindings


def command_transport_control(context, platform, operation):
    require_candidate_start_not_retired(context)
    require_finalization_not_started(context)
    state = load_state(context, {"candidate_started"})
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_runtime_generation(
        context, platform, state, expected_ready_status=(200, 503)
    )
    if not platform.postgres_running(context.cluster_root) or any(
        not platform.launchd_loaded(context.manifest["services"][name]["label"])
        for name in SERVICE_START_ORDER
    ):
        fail("candidate_state_drift")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    command = TRANSPORT_OPERATIONS.get(operation)
    if command is None:
        fail("transport_operation_invalid")
    pre_snapshot = platform.transport_control(context, "snapshot")
    require_committed_transport_snapshot(context, state, pre_snapshot)
    records, pending = transport_control_inventory(context)
    validate_transport_control_history(context, records)
    if pending is not None:
        intent = pending["intent"]
        if intent["operation"] != operation:
            fail("transport_operation_pending")
        sequence = intent["sequence"]
        operation_id = intent["operation_id"]
    else:
        if operation == "arm-next-duplicate" and (
            pre_snapshot["gateway"]["duplicate_armed"]
            or pre_snapshot["gateway"]["duplicate_claimed"]
        ):
            fail("transport_operation_busy")
        if operation == "arm-next-indeterminate" and (
            pre_snapshot["effect_http"]["indeterminate_armed"]
            or pre_snapshot["effect_http"]["indeterminate_claimed"]
        ):
            fail("transport_operation_busy")
        sequence = len(records) + 1
        if sequence > 9999:
            fail("transport_evidence_capacity_exhausted")
        operation_id = f"d2:{context.digest[:16]}:{sequence:04d}:{operation}"
        intent = {
            "schema_version": 1,
            "manifest_sha256": context.digest,
            "recorded_at": utc_now(),
            "sequence": sequence,
            "operation": operation,
            "command": command,
            "operation_id": operation_id,
        }
        intent_path = transport_control_directory(context) / (
            f"{sequence:04d}-{operation}-intent.json"
        )
        write_atomic(intent_path, canonical_json(intent) + "\n")
        append_journal(
            context, "transport_control", "intent", operation_id.replace(":", "_")
        )
    response = None
    if operation != "snapshot":
        fields = (
            {"operation_id": operation_id}
            if operation in {"arm-next-duplicate", "arm-next-indeterminate"}
            else {}
        )
        response = platform.transport_control(context, command, fields)
    snapshot = platform.transport_control(context, "snapshot")
    require_committed_transport_snapshot(context, state, snapshot)
    transport_operation_postcondition(operation, operation_id, response, snapshot)
    evidence = {
        "schema_version": 1,
        "manifest_sha256": context.digest,
        "recorded_at": utc_now(),
        "sequence": sequence,
        "operation": operation,
        "command": command,
        "operation_id": operation_id,
        "response": response,
        "snapshot": snapshot,
    }
    evidence_path = transport_control_directory(context) / (
        f"{sequence:04d}-{operation}-complete.json"
    )
    write_atomic(evidence_path, canonical_json(evidence) + "\n")
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_runtime_generation(
        context, platform, state, expected_ready_status=(200, 503)
    )
    require_committed_transport_identity(context, platform, state)
    append_journal(
        context, "transport_control", "complete", operation_id.replace(":", "_")
    )
    return {
        "status": "controlled",
        "operation": operation,
        "operation_id": operation_id,
        "response": response,
        "evidence": str(evidence_path),
        "snapshot": snapshot,
    }


def require_candidate_certification_boundary(
    context, platform, allow_abort_teardown=False
):
    require_candidate_start_not_retired(context, allow_abort_teardown)
    require_finalization_not_started(context)
    state = load_state(context, {"candidate_started"})
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_runtime_generation(context, platform, state)
    if not platform.postgres_running(context.cluster_root) or any(
        not platform.launchd_loaded(context.manifest["services"][name]["label"])
        for name in SERVICE_START_ORDER
    ):
        fail("candidate_state_drift")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    statuses = candidate_health(context, platform, wait=False)
    if statuses != {
        "worker": 200,
        "transport": 200,
        "api": 200,
        "runtime": 200,
        "tunnel": 200,
    }:
        fail("candidate_health_unready")
    snapshot = platform.transport_control(context, "snapshot")
    require_committed_transport_snapshot(context, state, snapshot)
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_runtime_generation(context, platform, state)
    require_committed_transport_identity(context, platform, state)
    return state, snapshot


def require_frozen_discord_teardown_boundary(context, platform):
    require_candidate_start_not_retired(context)
    state = load_state(context, {"candidate_started"})
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    if not platform.postgres_running(context.cluster_root):
        fail("finalization_freeze_state_drift")
    required = ("transport", "worker", "api")
    stopped = ("runtime", "tunnel")
    if any(
        not platform.launchd_loaded(context.manifest["services"][name]["label"])
        for name in required
    ) or any(
        platform.launchd_loaded(context.manifest["services"][name]["label"])
        for name in stopped
    ):
        fail("finalization_freeze_state_drift")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    snapshot = platform.transport_control(context, "snapshot")
    require_committed_transport_snapshot(context, state, snapshot)
    require_certified_teardown_snapshot(context, snapshot)
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_transport_identity(context, platform, state)
    return state, snapshot


def command_resource_inventory(context, platform):
    _state, snapshot = require_candidate_certification_boundary(context, platform)
    inventory = platform.transport_control(context, "resource_inventory")
    if inventory["instance_id"] != snapshot["instance_id"]:
        fail("transport_instance_changed")
    return {
        "status": "observed",
        "phase": "candidate_started",
        "manifest_sha256": context.digest,
        "transport_instance_id": inventory["instance_id"],
        "inventory_digest_sha256": inventory["digest_sha256"],
        "created_count": len(inventory["created"]),
        "deleted_count": len(inventory["deleted"]),
        "active_count": len(inventory["active"]),
        "resource_inventory": inventory,
    }


def require_gateway_loss_certification_boundary(context, platform):
    require_candidate_start_not_retired(context)
    require_finalization_not_started(context)
    state = load_state(context, {"candidate_started"})
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_runtime_generation(
        context, platform, state, expected_ready_status=(200, 503)
    )
    if not platform.postgres_running(context.cluster_root) or any(
        not platform.launchd_loaded(context.manifest["services"][name]["label"])
        for name in SERVICE_START_ORDER
    ):
        fail("candidate_state_drift")
    if standing_snapshot(context, platform) != state["standing_snapshot"]:
        fail("protected_staging_state_changed")
    snapshot = platform.transport_control(context, "snapshot")
    require_committed_transport_snapshot(context, state, snapshot)
    runtime_status = platform.http_status(
        "http://127.0.0.1:"
        f"{context.manifest['services']['runtime']['port']}/health/ready"
    )
    if runtime_status != 503:
        fail("gateway_loss_runtime_readiness_invalid")
    require_committed_candidate_processes(
        context, platform, state, ("api",)
    )
    require_committed_runtime_generation(
        context, platform, state, expected_ready_status=(200, 503)
    )
    require_committed_transport_identity(context, platform, state)
    return snapshot, runtime_status


def require_gateway_healed_certification_boundary(context, platform):
    _state, snapshot = require_candidate_certification_boundary(context, platform)
    runtime_status = platform.http_status(
        "http://127.0.0.1:"
        f"{context.manifest['services']['runtime']['port']}/health/ready"
    )
    if runtime_status != 200:
        fail("gateway_healed_runtime_readiness_invalid")
    return snapshot, runtime_status


def transport_evidence_path(context, checkpoint):
    return context.artifact_directory / "transport-evidence" / f"{checkpoint}.json"


def validate_transport_evidence_payload(context, checkpoint, evidence):
    common = {"schema_version", "kind", "observed_at"}
    fields = {
        "interaction": {
            "role_ids",
            "channel_ids",
            "panel_message_ids",
            "inventory_digest_sha256",
            "transport_instance_id",
        },
        "duplicate": {
            "interaction_id",
            "delivery_count",
            "transport_duplicate_injections",
            "transport_duplicate_delivery_count",
            "transport_last_duplicate_interaction_id",
            "role_ids",
            "channel_ids",
            "panel_message_ids",
            "inventory_digest_sha256",
            "transport_instance_id",
        },
        "reconciliation": {
            "injected_outcome",
            "transport_indeterminate_injections",
            "transport_last_audit_reason_sha256",
            "transport_last_upstream_status",
            "transport_instance_id",
        },
        "gateway-loss": {
            "gateway_disconnected",
            "runtime_ready_status",
            "transport_gateway_partitioned",
            "transport_gateway_partition_events",
            "transport_instance_id",
            "partition_operation_id",
            "partition_completion_sha256",
        },
        "gateway-healed": {
            "gateway_connected",
            "runtime_ready_status",
            "transport_gateway_partitioned",
            "transport_gateway_partition_events",
            "transport_duplicate_armed",
            "transport_duplicate_claimed",
            "transport_indeterminate_armed",
            "transport_indeterminate_claimed",
            "transport_instance_id",
            "partition_operation_id",
            "partition_completion_sha256",
            "heal_operation_id",
            "heal_completion_sha256",
        },
    }
    if checkpoint not in TRANSPORT_EVIDENCE_KINDS:
        fail("transport_evidence_checkpoint_invalid")
    if (
        not isinstance(evidence, dict)
        or set(evidence) != common | fields[checkpoint]
        or evidence["schema_version"] != 1
        or evidence["kind"] != TRANSPORT_EVIDENCE_KINDS[checkpoint]
        or not isinstance(evidence["observed_at"], str)
        or not TRANSPORT_RECORDED_AT_PATTERN.fullmatch(evidence["observed_at"])
        or evidence["transport_instance_id"] != pinned_transport_instance_id(context)
    ):
        fail("transport_evidence_invalid")
    if checkpoint == "interaction":
        _require_transport_inventory_projection(evidence)
        _require_study_room_transport_inventory(
            evidence, "transport_interaction_inventory_invalid"
        )
    elif checkpoint == "duplicate":
        _require_transport_inventory_projection(evidence)
        _require_study_room_transport_inventory(
            evidence, "transport_duplicate_inventory_invalid"
        )
        validate_snowflake(evidence["interaction_id"], "transport_interaction_id")
        validate_snowflake(
            evidence["transport_last_duplicate_interaction_id"],
            "transport_last_duplicate_interaction_id",
        )
        if (
            evidence["interaction_id"]
            != evidence["transport_last_duplicate_interaction_id"]
            or type(evidence["delivery_count"]) is not int
            or evidence["delivery_count"] != 2
            or type(evidence["transport_duplicate_injections"]) is not int
            or evidence["transport_duplicate_injections"] != 1
            or type(evidence["transport_duplicate_delivery_count"]) is not int
            or evidence["transport_duplicate_delivery_count"] != 2
        ):
            fail("transport_duplicate_evidence_invalid")
    elif checkpoint == "reconciliation":
        if (
            evidence["injected_outcome"] != "indeterminate"
            or type(evidence["transport_indeterminate_injections"]) is not int
            or evidence["transport_indeterminate_injections"] != 1
            or not isinstance(
                evidence["transport_last_audit_reason_sha256"], str
            )
            or not DIGEST_PATTERN.fullmatch(
                evidence["transport_last_audit_reason_sha256"]
            )
            or type(evidence["transport_last_upstream_status"]) is not int
            or not 200 <= evidence["transport_last_upstream_status"] <= 299
        ):
            fail("transport_reconciliation_evidence_invalid")
    elif checkpoint == "gateway-loss":
        if (
            evidence["gateway_disconnected"] is not True
            or type(evidence["runtime_ready_status"]) is not int
            or evidence["runtime_ready_status"] != 503
            or evidence["transport_gateway_partitioned"] is not True
            or type(evidence["transport_gateway_partition_events"]) is not int
            or evidence["transport_gateway_partition_events"] != 1
            or not isinstance(evidence["partition_operation_id"], str)
            or not TRANSPORT_OPERATION_ID_PATTERN.fullmatch(
                evidence["partition_operation_id"]
            )
            or not evidence["partition_operation_id"].endswith(
                ":partition-gateway"
            )
            or not isinstance(evidence["partition_completion_sha256"], str)
            or not DIGEST_PATTERN.fullmatch(
                evidence["partition_completion_sha256"]
            )
        ):
            fail("transport_gateway_loss_evidence_invalid")
    elif (
        evidence["gateway_connected"] is not True
        or type(evidence["runtime_ready_status"]) is not int
        or evidence["runtime_ready_status"] != 200
        or evidence["transport_gateway_partitioned"] is not False
        or type(evidence["transport_gateway_partition_events"]) is not int
        or evidence["transport_gateway_partition_events"] != 1
        or not isinstance(evidence["partition_operation_id"], str)
        or not TRANSPORT_OPERATION_ID_PATTERN.fullmatch(
            evidence["partition_operation_id"]
        )
        or not evidence["partition_operation_id"].endswith(
            ":partition-gateway"
        )
        or not isinstance(evidence["partition_completion_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(evidence["partition_completion_sha256"])
        or not isinstance(evidence["heal_operation_id"], str)
        or not TRANSPORT_OPERATION_ID_PATTERN.fullmatch(
            evidence["heal_operation_id"]
        )
        or not evidence["heal_operation_id"].endswith(":heal-gateway")
        or evidence["heal_operation_id"] == evidence["partition_operation_id"]
        or not isinstance(evidence["heal_completion_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(evidence["heal_completion_sha256"])
        or any(
            type(evidence[field]) is not bool or evidence[field]
            for field in (
                "transport_duplicate_armed",
                "transport_duplicate_claimed",
                "transport_indeterminate_armed",
                "transport_indeterminate_claimed",
            )
        )
    ):
        fail("transport_gateway_healed_evidence_invalid")
    return evidence


def interaction_transport_evidence(context, platform, snapshot):
    inventory = platform.transport_control(context, "resource_inventory")
    if (
        inventory["instance_id"] != snapshot["instance_id"]
        or inventory["deleted"] != []
        or inventory["active"] != inventory["created"]
    ):
        fail("transport_interaction_inventory_invalid")
    values = {
        "role_ids": sorted(
            resource["resource_id"]
            for resource in inventory["active"]
            if resource["kind"] == "role"
        ),
        "channel_ids": sorted(
            resource["resource_id"]
            for resource in inventory["active"]
            if resource["kind"] == "channel"
        ),
        "panel_message_ids": sorted(
            resource["resource_id"]
            for resource in inventory["active"]
            if resource["kind"] == "message"
        ),
    }
    _require_study_room_transport_inventory(
        values, "transport_interaction_inventory_invalid"
    )
    return {
        "schema_version": 1,
        "kind": TRANSPORT_EVIDENCE_KINDS["interaction"],
        "observed_at": utc_now(),
        **values,
        "inventory_digest_sha256": inventory["digest_sha256"],
        "transport_instance_id": inventory["instance_id"],
    }


def _require_transport_inventory_projection(evidence):
    digest = evidence.get("inventory_digest_sha256")
    if not isinstance(digest, str) or not DIGEST_PATTERN.fullmatch(digest):
        fail("transport_inventory_projection_invalid")
    for field in ("role_ids", "channel_ids", "panel_message_ids"):
        values = evidence.get(field)
        if (
            not isinstance(values, list)
            or not values
            or values != sorted(values)
            or len(values) != len(set(values))
        ):
            fail("transport_inventory_projection_invalid")
        for value in values:
            validate_snowflake(value, f"transport_{field}")
    resource_ids = (
        evidence["role_ids"]
        + evidence["channel_ids"]
        + evidence["panel_message_ids"]
    )
    if len(resource_ids) != len(set(resource_ids)):
        fail("transport_inventory_projection_invalid")


def _require_study_room_transport_inventory(evidence, code):
    expected_cardinality = {
        "role_ids": 1,
        "channel_ids": 1,
        "panel_message_ids": 2,
    }
    if any(
        not isinstance(evidence.get(field), list)
        or len(evidence[field]) != cardinality
        for field, cardinality in expected_cardinality.items()
    ):
        fail(code)


def duplicate_transport_evidence(context, platform, snapshot):
    gateway = snapshot["gateway"]
    interaction_id = gateway["last_duplicate_interaction_id"]
    inventory = platform.transport_control(context, "resource_inventory")
    if (
        inventory["instance_id"] != snapshot["instance_id"]
        or inventory["deleted"] != []
        or inventory["active"] != inventory["created"]
    ):
        fail("transport_duplicate_inventory_invalid")
    evidence = {
        "schema_version": 1,
        "kind": TRANSPORT_EVIDENCE_KINDS["duplicate"],
        "observed_at": utc_now(),
        "interaction_id": interaction_id,
        "delivery_count": gateway["duplicate_delivery_count"],
        "transport_duplicate_injections": gateway["duplicate_injections"],
        "transport_duplicate_delivery_count": gateway[
            "duplicate_delivery_count"
        ],
        "transport_last_duplicate_interaction_id": interaction_id,
        "role_ids": sorted(
            resource["resource_id"]
            for resource in inventory["active"]
            if resource["kind"] == "role"
        ),
        "channel_ids": sorted(
            resource["resource_id"]
            for resource in inventory["active"]
            if resource["kind"] == "channel"
        ),
        "panel_message_ids": sorted(
            resource["resource_id"]
            for resource in inventory["active"]
            if resource["kind"] == "message"
        ),
        "inventory_digest_sha256": inventory["digest_sha256"],
        "transport_instance_id": snapshot["instance_id"],
    }
    _require_study_room_transport_inventory(
        evidence, "transport_duplicate_inventory_invalid"
    )
    return evidence


def reconciliation_transport_evidence(snapshot):
    effect = snapshot["effect_http"]
    return {
        "schema_version": 1,
        "kind": TRANSPORT_EVIDENCE_KINDS["reconciliation"],
        "observed_at": utc_now(),
        "injected_outcome": "indeterminate",
        "transport_indeterminate_injections": effect["indeterminate_injections"],
        "transport_last_audit_reason_sha256": effect[
            "last_indeterminate_audit_reason_sha256"
        ],
        "transport_last_upstream_status": effect[
            "last_indeterminate_upstream_status"
        ],
        "transport_instance_id": snapshot["instance_id"],
    }


def gateway_loss_transport_evidence(snapshot, runtime_status, partition_binding):
    gateway = snapshot["gateway"]
    return {
        "schema_version": 1,
        "kind": TRANSPORT_EVIDENCE_KINDS["gateway-loss"],
        "observed_at": utc_now(),
        "gateway_disconnected": gateway["partitioned"],
        "runtime_ready_status": runtime_status,
        "transport_gateway_partitioned": gateway["partitioned"],
        "transport_gateway_partition_events": gateway["partition_events"],
        "transport_instance_id": snapshot["instance_id"],
        "partition_operation_id": partition_binding["operation_id"],
        "partition_completion_sha256": partition_binding[
            "completion_sha256"
        ],
    }


def gateway_healed_transport_evidence(
    snapshot, runtime_status, partition_binding, heal_binding
):
    gateway = snapshot["gateway"]
    effect = snapshot["effect_http"]
    return {
        "schema_version": 1,
        "kind": TRANSPORT_EVIDENCE_KINDS["gateway-healed"],
        "observed_at": utc_now(),
        "gateway_connected": not gateway["partitioned"],
        "runtime_ready_status": runtime_status,
        "transport_gateway_partitioned": gateway["partitioned"],
        "transport_gateway_partition_events": gateway["partition_events"],
        "transport_duplicate_armed": gateway["duplicate_armed"],
        "transport_duplicate_claimed": gateway["duplicate_claimed"],
        "transport_indeterminate_armed": effect["indeterminate_armed"],
        "transport_indeterminate_claimed": effect["indeterminate_claimed"],
        "transport_instance_id": snapshot["instance_id"],
        "partition_operation_id": partition_binding["operation_id"],
        "partition_completion_sha256": partition_binding[
            "completion_sha256"
        ],
        "heal_operation_id": heal_binding["operation_id"],
        "heal_completion_sha256": heal_binding["completion_sha256"],
    }


def command_transport_evidence(context, platform, checkpoint):
    require_candidate_start_not_retired(context)
    if checkpoint not in TRANSPORT_EVIDENCE_KINDS:
        fail("transport_evidence_checkpoint_invalid")
    if checkpoint == "gateway-loss":
        snapshot, runtime_status = require_gateway_loss_certification_boundary(
            context, platform
        )
        bindings = gateway_control_completion_bindings(
            context, ("partition-gateway",)
        )
        partition_binding = bindings[0]
        if (
            partition_binding["snapshot"]["instance_id"]
            != snapshot["instance_id"]
            or partition_binding["snapshot"]["gateway"]["partitioned"]
            is not True
            or partition_binding["snapshot"]["gateway"]["partition_events"]
            != 1
        ):
            fail("transport_gateway_partition_binding_invalid")
        current = gateway_loss_transport_evidence(
            snapshot, runtime_status, partition_binding
        )
    elif checkpoint == "gateway-healed":
        snapshot, runtime_status = require_gateway_healed_certification_boundary(
            context, platform
        )
        loss_path = transport_evidence_path(context, "gateway-loss")
        if not loss_path.exists():
            fail("transport_gateway_loss_evidence_missing")
        loss = load_private_json(loss_path, "transport_evidence_gateway_loss")
        validate_transport_evidence_payload(context, "gateway-loss", loss)
        bindings = gateway_control_completion_bindings(
            context, ("partition-gateway", "heal-gateway")
        )
        partition_binding, heal_binding = bindings
        if (
            loss["partition_operation_id"] != partition_binding["operation_id"]
            or loss["partition_completion_sha256"]
            != partition_binding["completion_sha256"]
            or partition_binding["snapshot"]["instance_id"]
            != snapshot["instance_id"]
            or partition_binding["snapshot"]["gateway"]["partitioned"]
            is not True
            or partition_binding["snapshot"]["gateway"]["partition_events"]
            != 1
            or heal_binding["snapshot"]["instance_id"] != snapshot["instance_id"]
            or heal_binding["snapshot"]["gateway"]["partitioned"] is not False
            or heal_binding["snapshot"]["gateway"]["partition_events"] != 1
        ):
            fail("transport_gateway_heal_binding_invalid")
        current = gateway_healed_transport_evidence(
            snapshot,
            runtime_status,
            partition_binding,
            heal_binding,
        )
    else:
        _state, snapshot = require_candidate_certification_boundary(
            context, platform
        )
        if checkpoint == "interaction":
            current = interaction_transport_evidence(context, platform, snapshot)
        elif checkpoint == "duplicate":
            current = duplicate_transport_evidence(context, platform, snapshot)
        else:
            current = reconciliation_transport_evidence(snapshot)
    validate_transport_evidence_payload(context, checkpoint, current)
    path = transport_evidence_path(context, checkpoint)
    if path.exists():
        recorded = load_private_json(path, f"transport_evidence_{checkpoint}")
        validate_transport_evidence_payload(context, checkpoint, recorded)
        current_semantics = {
            key: value for key, value in current.items() if key != "observed_at"
        }
        recorded_semantics = {
            key: value for key, value in recorded.items() if key != "observed_at"
        }
        if current_semantics != recorded_semantics:
            fail("transport_evidence_replay_drift")
        status = "exact_replay"
        evidence = recorded
    else:
        append_journal(context, "transport_evidence", "intent", checkpoint)
        write_atomic(path, canonical_json(current) + "\n")
        evidence = load_private_json(path, f"transport_evidence_{checkpoint}")
        validate_transport_evidence_payload(context, checkpoint, evidence)
        append_journal(context, "transport_evidence", "complete", checkpoint)
        status = "recorded"
    return {
        "status": status,
        "phase": "candidate_started",
        "checkpoint": checkpoint,
        "kind": evidence["kind"],
        "transport_instance_id": evidence["transport_instance_id"],
        "evidence": str(path),
    }


def command_worker_authoring_evidence(
    context, platform, checkpoint, browser_evidence_path=None
):
    if checkpoint == "before" and browser_evidence_path is not None:
        fail("worker_browser_evidence_unexpected")
    if checkpoint == "after" and browser_evidence_path is None:
        fail("worker_browser_evidence_required")
    require_candidate_certification_boundary(context, platform)
    browser = None
    if browser_evidence_path is not None:
        path = require_absolute_path(
            browser_evidence_path, "worker_browser_evidence"
        )
        browser = load_private_json(path, "worker_browser_evidence")
    health = platform.worker_health_snapshot(context)
    return capture_worker_authoring_checkpoint(
        context, health, checkpoint, browser
    )


def reconciliation_discord_observation_path(context):
    return (
        context.artifact_directory
        / "discord-evidence"
        / "reconciliation-role.json"
    )


def validate_reconciliation_database_source(value):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "effect_identity",
        "interaction_id",
        "route_identity",
        "reconciliation_state",
        "duplicate_external_effect_count",
        "unsafe_deletion_count",
        "output_role_id",
    }
    if (
        not isinstance(value, dict)
        or set(value) != fields
        or value["schema_version"] != 1
        or value["kind"] != "starring.d2.db-reconciliation-evidence.v1"
        or not isinstance(value["observed_at"], str)
        or not EVIDENCE_RECORDED_AT_PATTERN.fullmatch(value["observed_at"])
        or value["reconciliation_state"] != "known_success"
        or value["duplicate_external_effect_count"] != 0
        or value["unsafe_deletion_count"] != 0
        or type(value["duplicate_external_effect_count"]) is not int
        or type(value["unsafe_deletion_count"]) is not int
        or not isinstance(value["effect_identity"], dict)
        or not isinstance(value["route_identity"], dict)
    ):
        fail("reconciliation_database_evidence_invalid")
    validate_snowflake(value["interaction_id"], "reconciliation_interaction_id")
    validate_snowflake(value["output_role_id"], "reconciliation_output_role_id")
    if value["effect_identity"].get("interaction_id") != value["interaction_id"]:
        fail("reconciliation_database_evidence_invalid")
    return value


def validate_reconciliation_discord_observation(context, value, inventory, role_id):
    fields = {
        "schema_version",
        "kind",
        "observed_at",
        "transport_instance_id",
        "inventory_digest_sha256",
        "resource_kind",
        "resource_id",
        "channel_id",
        "http_status",
        "discord_code",
        "exists",
    }
    if (
        not isinstance(value, dict)
        or set(value) != fields
        or value["schema_version"] != 1
        or value["kind"] != RECONCILIATION_DISCORD_OBSERVATION_KIND
        or not isinstance(value["observed_at"], str)
        or not EVIDENCE_RECORDED_AT_PATTERN.fullmatch(value["observed_at"])
        or value["transport_instance_id"] != inventory["instance_id"]
        or value["inventory_digest_sha256"] != inventory["digest_sha256"]
        or value["resource_kind"] != "role"
        or value["resource_id"] != role_id
        or value["channel_id"] is not None
        or value["http_status"] != 200
        or value["discord_code"] is not None
        or value["exists"] is not True
    ):
        fail("reconciliation_discord_observation_invalid")
    require_pinned_transport_snapshot(
        context, {"instance_id": value["transport_instance_id"]}
    )
    return value


def current_reconciliation_discord_observation(
    context, platform, database, inventory
):
    role_id = database["output_role_id"]
    resource = {"kind": "role", "resource_id": role_id}
    if resource not in inventory["active"]:
        fail("reconciliation_output_role_not_active")
    observed = platform.discord_observe_resource(context, resource, inventory)
    if (
        not isinstance(observed, dict)
        or set(observed)
        != {
            "schema_version",
            "kind",
            "transport_instance_id",
            "inventory_digest_sha256",
            "resource_kind",
            "resource_id",
            "channel_id",
            "http_status",
            "discord_code",
            "exists",
        }
        or observed["schema_version"] != 1
        or observed["kind"]
        != "starring.d2.discord-resource-observation.v1"
    ):
        fail("reconciliation_discord_observation_invalid")
    evidence = {
        **observed,
        "kind": RECONCILIATION_DISCORD_OBSERVATION_KIND,
        "observed_at": utc_now(),
    }
    return validate_reconciliation_discord_observation(
        context, evidence, inventory, role_id
    )


def command_reconciliation_discord_observation(
    context, platform, database_evidence_path
):
    _state, snapshot = require_candidate_certification_boundary(context, platform)
    database_path = require_absolute_path(
        database_evidence_path, "reconciliation_database_evidence"
    )
    database = validate_reconciliation_database_source(
        load_private_json(database_path, "reconciliation_database_evidence")
    )
    inventory = platform.transport_control(context, "resource_inventory")
    if inventory["instance_id"] != snapshot["instance_id"]:
        fail("transport_instance_changed")
    current = current_reconciliation_discord_observation(
        context, platform, database, inventory
    )
    path = reconciliation_discord_observation_path(context)
    if path.exists():
        recorded = validate_reconciliation_discord_observation(
            context,
            load_private_json(path, "reconciliation_discord_observation"),
            inventory,
            database["output_role_id"],
        )
        if {
            key: value for key, value in current.items() if key != "observed_at"
        } != {
            key: value for key, value in recorded.items() if key != "observed_at"
        }:
            fail("reconciliation_discord_observation_replay_drift")
        status = "exact_replay"
    else:
        append_journal(
            context, "reconciliation_discord_observation", "intent", "role"
        )
        write_atomic(path, canonical_json(current) + "\n")
        recorded = validate_reconciliation_discord_observation(
            context,
            load_private_json(path, "reconciliation_discord_observation"),
            inventory,
            database["output_role_id"],
        )
        append_journal(
            context, "reconciliation_discord_observation", "complete", "role"
        )
        status = "recorded"
    return {
        "status": status,
        "phase": "candidate_started",
        "kind": recorded["kind"],
        "transport_instance_id": recorded["transport_instance_id"],
        "resource_id": recorded["resource_id"],
        "evidence": str(path),
    }


def discord_resource_identity_key(resource):
    return (
        resource["kind"],
        resource["resource_id"],
        resource.get("channel_id"),
    )


def discord_resource_teardown_key(resource):
    return (
        DISCORD_RESOURCE_KIND_ORDER[resource["kind"]],
        resource.get("channel_id", ""),
        resource["resource_id"],
    )


def discord_resource_union_sha256(resources):
    return hashlib.sha256(canonical_json(resources).encode("utf-8")).hexdigest()


def discord_teardown_progress_path(context, frozen=False):
    if frozen:
        return context.artifact_directory / "discord-resource-teardown-progress.json"
    return abort_teardown_progress_path(context)


def discord_teardown_evidence_path(context, frozen=False):
    if frozen:
        return context.artifact_directory / "discord-resource-teardown-evidence.json"
    return abort_teardown_evidence_path(context)


def load_private_json(path, label):
    require_owned_mode(path, 0o600, label)
    return load_json_file(path, label)


def validate_abort_teardown_tombstone(context, value, inventory):
    if (
        not isinstance(value, dict)
        or set(value)
        != {
            "schema_version",
            "kind",
            "manifest_sha256",
            "run_id",
            "recorded_at",
            "transport_instance_id",
            "source_inventory_digest_sha256",
            "certification_permanently_disqualified",
        }
        or value["schema_version"] != 1
        or value["kind"] != DISCORD_TEARDOWN_ABORT_KIND
        or value["manifest_sha256"] != context.digest
        or value["run_id"] != context.manifest["run_id"]
        or not isinstance(value["recorded_at"], str)
        or not TRANSPORT_RECORDED_AT_PATTERN.fullmatch(value["recorded_at"])
        or value["transport_instance_id"] != inventory["instance_id"]
        or not isinstance(value["source_inventory_digest_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(value["source_inventory_digest_sha256"])
        or value["certification_permanently_disqualified"] is not True
    ):
        fail("discord_resource_teardown_abort_invalid")
    return value


def ensure_abort_teardown_tombstone(context, inventory):
    path = abort_teardown_tombstone_path(context)
    if path.exists():
        return validate_abort_teardown_tombstone(
            context,
            load_private_json(path, "discord_resource_teardown_abort"),
            inventory,
        )
    value = {
        "schema_version": 1,
        "kind": DISCORD_TEARDOWN_ABORT_KIND,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "recorded_at": utc_now(),
        "transport_instance_id": inventory["instance_id"],
        "source_inventory_digest_sha256": inventory["digest_sha256"],
        "certification_permanently_disqualified": True,
    }
    validate_abort_teardown_tombstone(context, value, inventory)
    write_atomic(path, canonical_json(value) + "\n")
    return value


def discord_teardown_record(resource, disposition, http_status=None, discord_code=None):
    return {
        "resource_kind": resource["kind"],
        "resource_id": resource["resource_id"],
        "channel_id": resource.get("channel_id"),
        "disposition": disposition,
        "http_status": http_status,
        "discord_code": discord_code,
    }


def discord_teardown_record_resource(record):
    resource = {
        "kind": record["resource_kind"],
        "resource_id": record["resource_id"],
    }
    if record["resource_kind"] == "message":
        resource["channel_id"] = record["channel_id"]
    return resource


def validate_discord_teardown_record(record, resources):
    if not isinstance(record, dict) or set(record) != {
        "resource_kind",
        "resource_id",
        "channel_id",
        "disposition",
        "http_status",
        "discord_code",
    }:
        fail("discord_resource_teardown_progress_invalid")
    kind = record["resource_kind"]
    if kind not in DISCORD_RESOURCE_KIND_ORDER:
        fail("discord_resource_teardown_progress_invalid")
    resource = discord_teardown_record_resource(record)
    if resource not in resources or (
        kind == "message" and not isinstance(record["channel_id"], str)
    ) or (kind != "message" and record["channel_id"] is not None):
        fail("discord_resource_teardown_progress_invalid")
    disposition = record["disposition"]
    if disposition in {"preexisting_deleted", "reconciled_deleted"}:
        if record["http_status"] is not None or record["discord_code"] is not None:
            fail("discord_resource_teardown_progress_invalid")
    elif disposition == "deleted":
        if (
            record["http_status"] != DISCORD_RESOURCE_SUCCESS_STATUS[kind]
            or record["discord_code"] is not None
        ):
            fail("discord_resource_teardown_progress_invalid")
    elif disposition == "already_absent":
        if (
            record["http_status"] != 404
            or record["discord_code"] not in DISCORD_RESOURCE_UNKNOWN_CODES[kind]
        ):
            fail("discord_resource_teardown_progress_invalid")
    else:
        fail("discord_resource_teardown_progress_invalid")
    return resource


def validate_discord_teardown_progress(context, progress, inventory):
    if not isinstance(progress, dict) or set(progress) != {
        "schema_version",
        "kind",
        "manifest_sha256",
        "run_id",
        "transport_instance_id",
        "source_inventory_digest_sha256",
        "resource_union_sha256",
        "created_resources",
        "deletions",
    }:
        fail("discord_resource_teardown_progress_invalid")
    resources = inventory["created"]
    if (
        progress["schema_version"] != 1
        or progress["kind"] != DISCORD_TEARDOWN_PROGRESS_KIND
        or progress["manifest_sha256"] != context.digest
        or progress["run_id"] != context.manifest["run_id"]
        or progress["transport_instance_id"] != inventory["instance_id"]
        or not isinstance(progress["source_inventory_digest_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(
            progress["source_inventory_digest_sha256"]
        )
        or progress["resource_union_sha256"]
        != discord_resource_union_sha256(resources)
        or progress["created_resources"] != resources
        or not isinstance(progress["deletions"], list)
    ):
        fail("discord_resource_teardown_progress_invalid")
    deleted = {
        discord_resource_identity_key(resource) for resource in inventory["deleted"]
    }
    observed = []
    for record in progress["deletions"]:
        resource = validate_discord_teardown_record(record, resources)
        key = discord_resource_identity_key(resource)
        if key not in deleted:
            fail("discord_resource_teardown_progress_mismatch")
        observed.append(key)
    expected_order = [
        discord_resource_identity_key(resource)
        for resource in sorted(
            (discord_teardown_record_resource(record) for record in progress["deletions"]),
            key=discord_resource_teardown_key,
        )
    ]
    if observed != expected_order or len(observed) != len(set(observed)):
        fail("discord_resource_teardown_progress_invalid")
    return progress


def new_discord_teardown_progress(context, inventory):
    deleted = {
        discord_resource_identity_key(resource) for resource in inventory["deleted"]
    }
    resources = inventory["created"]
    deletions = [
        discord_teardown_record(resource, "preexisting_deleted")
        for resource in sorted(resources, key=discord_resource_teardown_key)
        if discord_resource_identity_key(resource) in deleted
    ]
    return {
        "schema_version": 1,
        "kind": DISCORD_TEARDOWN_PROGRESS_KIND,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "transport_instance_id": inventory["instance_id"],
        "source_inventory_digest_sha256": inventory["digest_sha256"],
        "resource_union_sha256": discord_resource_union_sha256(resources),
        "created_resources": resources,
        "deletions": deletions,
    }


def write_discord_teardown_progress(context, progress, frozen=False):
    write_atomic(
        discord_teardown_progress_path(context, frozen), canonical_json(progress) + "\n"
    )


def reconcile_discord_teardown_progress(context, progress, inventory, frozen=False):
    completed = {
        discord_resource_identity_key(discord_teardown_record_resource(record))
        for record in progress["deletions"]
    }
    added = False
    for resource in sorted(inventory["deleted"], key=discord_resource_teardown_key):
        key = discord_resource_identity_key(resource)
        if key not in completed:
            progress["deletions"].append(
                discord_teardown_record(resource, "reconciled_deleted")
            )
            completed.add(key)
            added = True
    if added:
        progress["deletions"].sort(
            key=lambda record: discord_resource_teardown_key(
                discord_teardown_record_resource(record)
            )
        )
        write_discord_teardown_progress(context, progress, frozen)
    return progress


def normalize_proxy_deletion(inventory, resource, evidence):
    if not isinstance(evidence, dict) or set(evidence) != {
        "schema_version",
        "kind",
        "transport_instance_id",
        "inventory_digest_sha256",
        "resource_kind",
        "resource_id",
        "channel_id",
        "http_status",
        "discord_code",
        "deleted",
    }:
        fail("discord_resource_proxy_evidence_invalid")
    expected_status = DISCORD_RESOURCE_SUCCESS_STATUS[resource["kind"]]
    if (
        evidence["schema_version"] != 1
        or evidence["kind"]
        != "starring.d2.discord-resource-proxy-deletion.v1"
        or evidence["transport_instance_id"] != inventory["instance_id"]
        or evidence["inventory_digest_sha256"] != inventory["digest_sha256"]
        or evidence["resource_kind"] != resource["kind"]
        or evidence["resource_id"] != resource["resource_id"]
        or evidence["channel_id"] != resource.get("channel_id")
        or type(evidence["deleted"]) is not bool
    ):
        fail("discord_resource_proxy_evidence_invalid")
    if evidence["http_status"] == expected_status:
        if evidence["discord_code"] is not None or evidence["deleted"] is not True:
            fail("discord_resource_proxy_evidence_invalid")
        disposition = "deleted"
    elif evidence["http_status"] == 404:
        if (
            evidence["discord_code"]
            not in DISCORD_RESOURCE_UNKNOWN_CODES[resource["kind"]]
            or evidence["deleted"] is not False
        ):
            fail("discord_resource_proxy_evidence_invalid")
        disposition = "already_absent"
    else:
        fail("discord_resource_proxy_evidence_invalid")
    return discord_teardown_record(
        resource,
        disposition,
        evidence["http_status"],
        evidence["discord_code"],
    )


def normalize_direct_observation(inventory, resource, evidence):
    if not isinstance(evidence, dict) or set(evidence) != {
        "schema_version",
        "kind",
        "transport_instance_id",
        "inventory_digest_sha256",
        "resource_kind",
        "resource_id",
        "channel_id",
        "http_status",
        "discord_code",
        "exists",
    }:
        fail("discord_resource_observation_evidence_invalid")
    kind = resource["kind"]
    absent_status = (
        evidence["http_status"] == 200
        and kind == "role"
        and evidence["discord_code"] is None
    ) or (
        evidence["http_status"] == 404
        and evidence["discord_code"] in DISCORD_RESOURCE_UNKNOWN_CODES[kind]
    )
    if (
        evidence["schema_version"] != 1
        or evidence["kind"] != "starring.d2.discord-resource-observation.v1"
        or evidence["transport_instance_id"] != inventory["instance_id"]
        or evidence["inventory_digest_sha256"] != inventory["digest_sha256"]
        or evidence["resource_kind"] != kind
        or evidence["resource_id"] != resource["resource_id"]
        or evidence["channel_id"] != resource.get("channel_id")
        or evidence["exists"] is not False
        or not absent_status
    ):
        fail("discord_resource_absence_unconfirmed")
    return {
        "resource_kind": kind,
        "resource_id": resource["resource_id"],
        "channel_id": resource.get("channel_id"),
        "http_status": evidence["http_status"],
        "discord_code": evidence["discord_code"],
        "exists": False,
    }


def observe_absent_discord_resources(context, platform, inventory):
    observations = []
    for resource in sorted(inventory["created"], key=discord_resource_teardown_key):
        evidence = platform.discord_observe_resource(context, resource, inventory)
        observations.append(
            normalize_direct_observation(inventory, resource, evidence)
        )
    return observations


def discord_resource_id_lists(resources):
    return {
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


def validate_discord_teardown_evidence(
    context, evidence, inventory, certification_binding=None
):
    required = {
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
    }
    if certification_binding is not None:
        required.update(certification_binding)
    resources = inventory["created"]
    identifiers = discord_resource_id_lists(resources)
    if (
        not isinstance(evidence, dict)
        or set(evidence) != required
        or evidence["schema_version"] != 1
        or evidence["kind"] != DISCORD_TEARDOWN_EVIDENCE_KIND
        or evidence["manifest_sha256"] != context.digest
        or evidence["run_id"] != context.manifest["run_id"]
        or not isinstance(evidence["recorded_at"], str)
        or not TRANSPORT_RECORDED_AT_PATTERN.fullmatch(evidence["recorded_at"])
        or evidence["transport_instance_id"] != inventory["instance_id"]
        or not isinstance(evidence["source_inventory_digest_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(
            evidence["source_inventory_digest_sha256"]
        )
        or evidence["final_inventory_digest_sha256"]
        != inventory["digest_sha256"]
        or evidence["resource_union_sha256"]
        != discord_resource_union_sha256(resources)
        or evidence["created_resources"] != resources
        or evidence["deleted_resources"] != resources
        or evidence["active_resources"] != []
        or any(evidence[name] != value for name, value in identifiers.items())
        or evidence["all_resources_absent"] is not True
        or not isinstance(evidence["proxy_deletions"], list)
        or not isinstance(evidence["direct_observations"], list)
        or inventory["deleted"] != resources
        or inventory["active"] != []
    ):
        fail("discord_resource_teardown_evidence_invalid")
    if certification_binding is not None:
        if any(
            evidence[field] != value
            for field, value in certification_binding.items()
        ) or evidence["source_inventory_digest_sha256"] != (
            certification_binding["freeze_resource_inventory_digest_sha256"]
        ):
            fail("discord_resource_teardown_evidence_invalid")
    progress_view = {
        "schema_version": 1,
        "kind": DISCORD_TEARDOWN_PROGRESS_KIND,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "transport_instance_id": inventory["instance_id"],
        "source_inventory_digest_sha256": evidence[
            "source_inventory_digest_sha256"
        ],
        "resource_union_sha256": evidence["resource_union_sha256"],
        "created_resources": resources,
        "deletions": evidence["proxy_deletions"],
    }
    validate_discord_teardown_progress(context, progress_view, inventory)
    expected_resources = sorted(resources, key=discord_resource_teardown_key)
    observations = evidence["direct_observations"]
    if len(observations) != len(expected_resources):
        fail("discord_resource_teardown_evidence_invalid")
    for resource, observation in zip(expected_resources, observations):
        normalized = {
            "schema_version": 1,
            "kind": "starring.d2.discord-resource-observation.v1",
            "transport_instance_id": inventory["instance_id"],
            "inventory_digest_sha256": inventory["digest_sha256"],
            **observation,
        }
        normalize_direct_observation(inventory, resource, normalized)
    return evidence


def d2a_taint_path(context):
    return context.run_directory / "d2a-taint.json"


def d2a_session_lifecycle_path(context):
    return context.run_directory / "d2a-session-lifecycle.json"


def d2a_teardown_fence_path(context):
    return context.run_directory / "d2a-teardown-fence.json"


class D2aMarkerDecodeError(ValueError):
    pass


def strict_d2a_marker_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise D2aMarkerDecodeError("duplicate_key")
        value[key] = item
    return value


def d2a_marker_identity(metadata):
    return (
        metadata.st_dev,
        metadata.st_ino,
        metadata.st_mode,
        metadata.st_uid,
        metadata.st_gid,
        metadata.st_nlink,
        metadata.st_size,
        metadata.st_mtime_ns,
        metadata.st_ctime_ns,
    )


def load_strict_d2a_marker(path, code, fields, sorted_canonical=False):
    if not hasattr(os, "O_NOFOLLOW"):
        fail(code)
    flags = os.O_RDONLY | os.O_NOFOLLOW
    flags |= getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_NONBLOCK", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError:
        fail(code)
    try:
        before = os.fstat(descriptor)
        if (
            not stat.S_ISREG(before.st_mode)
            or before.st_uid != os.getuid()
            or stat.S_IMODE(before.st_mode) != 0o600
            or before.st_nlink != 1
            or before.st_size <= 0
            or before.st_size > D2A_MARKER_MAXIMUM_BYTES
        ):
            fail(code)
        raw = bytearray()
        while len(raw) <= D2A_MARKER_MAXIMUM_BYTES:
            chunk = os.read(descriptor, min(64 * 1024, D2A_MARKER_MAXIMUM_BYTES + 1 - len(raw)))
            if not chunk:
                break
            raw.extend(chunk)
        after = os.fstat(descriptor)
        try:
            named = os.stat(path, follow_symlinks=False)
        except OSError:
            fail(code)
    except OSError:
        fail(code)
    finally:
        os.close(descriptor)
    if (
        len(raw) != before.st_size
        or len(raw) > D2A_MARKER_MAXIMUM_BYTES
        or d2a_marker_identity(before) != d2a_marker_identity(after)
        or d2a_marker_identity(after) != d2a_marker_identity(named)
        or not stat.S_ISREG(named.st_mode)
    ):
        fail(code)
    try:
        observed = bytes(raw).decode("utf-8")
        if (
            not observed.endswith("\n")
            or observed.endswith("\n\n")
            or observed[:-1].endswith("\r")
        ):
            raise D2aMarkerDecodeError("newline")
        payload = observed[:-1]
        value = json.loads(payload, object_pairs_hook=strict_d2a_marker_object)
        if not isinstance(value, dict) or tuple(value) != tuple(fields):
            raise D2aMarkerDecodeError("fields")
        expected = (
            canonical_json(value)
            if sorted_canonical
            else json.dumps(value, ensure_ascii=False, separators=(",", ":"))
        )
        if payload != expected:
            raise D2aMarkerDecodeError("canonical")
    except (UnicodeDecodeError, json.JSONDecodeError, D2aMarkerDecodeError):
        fail(code)
    return value


def valid_d2a_digest(value):
    return isinstance(value, str) and D2A_DIGEST.fullmatch(value) is not None


def valid_d2a_boot_identity(value):
    if not isinstance(value, str) or D2A_BOOT_IDENTITY.fullmatch(value) is None:
        return False
    microseconds = int(value.rsplit(":", 1)[1])
    return microseconds < 1_000_000


def d2a_sysctl_executable_identity():
    for parent in (pathlib.Path("/usr"), pathlib.Path("/usr/sbin")):
        try:
            metadata = parent.lstat()
        except OSError:
            fail("d2a_boot_identity_invalid")
        if (
            not stat.S_ISDIR(metadata.st_mode)
            or parent.is_symlink()
            or metadata.st_uid != 0
            or stat.S_IMODE(metadata.st_mode) & 0o022
        ):
            fail("d2a_boot_identity_invalid")
    if not hasattr(os, "O_NOFOLLOW"):
        fail("d2a_boot_identity_invalid")
    try:
        descriptor = os.open(
            D2A_SYSCTL_PATH,
            os.O_RDONLY | os.O_NOFOLLOW | getattr(os, "O_CLOEXEC", 0),
        )
    except OSError:
        fail("d2a_boot_identity_invalid")
    try:
        opened = os.fstat(descriptor)
        named = os.stat(D2A_SYSCTL_PATH, follow_symlinks=False)
    except OSError:
        fail("d2a_boot_identity_invalid")
    finally:
        os.close(descriptor)
    if (
        not stat.S_ISREG(opened.st_mode)
        or opened.st_uid != 0
        or opened.st_nlink != 1
        or stat.S_IMODE(opened.st_mode) != 0o755
        or d2a_marker_identity(opened) != d2a_marker_identity(named)
    ):
        fail("d2a_boot_identity_invalid")
    return d2a_marker_identity(opened)


def current_darwin_boot_identity():
    before = d2a_sysctl_executable_identity()
    try:
        result = subprocess.run(
            [str(D2A_SYSCTL_PATH), "-n", "kern.boottime"],
            cwd="/",
            env={"LANG": "C", "LC_ALL": "C"},
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=5,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        fail("d2a_boot_identity_invalid")
    after = d2a_sysctl_executable_identity()
    if (
        before != after
        or result.returncode != 0
        or result.stderr
        or len(result.stdout) > 256
    ):
        fail("d2a_boot_identity_invalid")
    match = D2A_SYSCTL_BOOT_TIME.fullmatch(result.stdout)
    if match is None:
        fail("d2a_boot_identity_invalid")
    seconds = int(match.group(1))
    microseconds = int(match.group(2))
    if microseconds >= 1_000_000:
        fail("d2a_boot_identity_invalid")
    identity = f"darwin-boottime:{seconds}:{microseconds}"
    if not valid_d2a_boot_identity(identity):
        fail("d2a_boot_identity_invalid")
    return identity


def valid_d2a_lifecycle_timestamp(value):
    if not isinstance(value, str) or D2A_LIFECYCLE_TIMESTAMP.fullmatch(value) is None:
        return False
    try:
        # Python's datetime has microsecond precision.  Parse the calendar portion while
        # retaining the separately validated exact nine-digit nanosecond spelling.
        datetime.datetime.strptime(value[:19] + "Z", "%Y-%m-%dT%H:%M:%SZ")
    except ValueError:
        return False
    return True


def require_d2a_session_revoked(context):
    taint_path = d2a_taint_path(context)
    lifecycle_path = d2a_session_lifecycle_path(context)
    fence_path = d2a_teardown_fence_path(context)
    markers = tuple(
        os.path.lexists(path) for path in (taint_path, lifecycle_path, fence_path)
    )
    if not any(markers):
        return False
    if not os.path.lexists(taint_path) or not os.path.lexists(lifecycle_path):
        fail("manual_recovery_required")
    taint = load_strict_d2a_marker(
        taint_path, "d2a_taint_invalid", D2A_TAINT_FIELDS
    )
    if (
        type(taint.get("schema_version")) is not int
        or taint.get("schema_version") != 1
        or taint.get("kind") != "starring.d2a.run-taint.v1"
        or taint.get("run_id") != context.manifest["run_id"]
        or taint.get("manifest_sha256") != context.digest
        or taint.get("certification_class") != "automated_maintenance_v1"
        or taint.get("direct_auth_used") is not True
        or taint.get("release_eligible") is not False
        or any(
            not valid_d2a_digest(taint.get(field))
            for field in (
                "issuer_sha256",
                "issuer_source_sha256",
                "runner_sha256",
                "product_driver_sha256",
                "scenario_sha256",
            )
        )
    ):
        fail("manual_recovery_required")
    lifecycle = load_strict_d2a_marker(
        lifecycle_path,
        "d2a_session_lifecycle_invalid",
        D2A_SESSION_LIFECYCLE_FIELDS,
    )
    issuer_origin = lifecycle.get("origin") == "issuer"
    bootstrap_origin = lifecycle.get("origin") == "bootstrap"
    process_group_id = lifecycle.get("process_group_id")
    positive_process_group = (
        type(process_group_id) is int and 1 < process_group_id <= 2_147_483_647
    )
    revoked = (
        issuer_origin
        and positive_process_group
        and lifecycle.get("status") == "revoked"
        and lifecycle.get("session_revoked") is True
        and valid_d2a_lifecycle_timestamp(lifecycle.get("revoked_at"))
        and lifecycle.get("quarantined_at") is None
    )
    issuer_not_issued = (
        issuer_origin
        and positive_process_group
        and lifecycle.get("status") == "not_issued"
        and lifecycle.get("session_revoked") is False
        and lifecycle.get("revoked_at") is None
        and lifecycle.get("quarantined_at") is None
    )
    bootstrap_not_issued = (
        bootstrap_origin
        and lifecycle.get("operation") == "direct-onboard"
        and process_group_id is None
        and lifecycle.get("status") == "not_issued"
        and lifecycle.get("session_revoked") is False
        and lifecycle.get("revoked_at") is None
        and lifecycle.get("quarantined_at") is None
    )
    active = (
        issuer_origin
        and positive_process_group
        and lifecycle.get("status") == "active"
        and lifecycle.get("session_revoked") is False
        and lifecycle.get("revoked_at") is None
        and lifecycle.get("quarantined_at") is None
    )
    quarantined = (
        issuer_origin
        and positive_process_group
        and lifecycle.get("status") == "quarantined"
        and lifecycle.get("session_revoked") is False
        and lifecycle.get("revoked_at") is None
        and valid_d2a_lifecycle_timestamp(lifecycle.get("quarantined_at"))
    )
    if (
        type(lifecycle.get("schema_version")) is not int
        or lifecycle.get("schema_version") != 1
        or lifecycle.get("kind") != "starring.d2a.session-lifecycle.v1"
        or lifecycle.get("run_id") != context.manifest["run_id"]
        or lifecycle.get("manifest_sha256") != context.digest
        or lifecycle.get("operation") not in {"auth-smoke", "direct-onboard", "one-shot"}
        or lifecycle.get("origin") not in {"bootstrap", "issuer"}
        or lifecycle.get("issuer_sha256") != taint.get("issuer_sha256")
        or lifecycle.get("issuer_source_sha256") != taint.get("issuer_source_sha256")
        or not valid_d2a_digest(lifecycle.get("issuer_sha256"))
        or not valid_d2a_digest(lifecycle.get("issuer_source_sha256"))
        or type(lifecycle.get("uid")) is not int
        or lifecycle.get("uid") != os.getuid()
        or not valid_d2a_boot_identity(lifecycle.get("boot_identity"))
        or not valid_d2a_lifecycle_timestamp(lifecycle.get("started_at"))
        or not (
            active
            or issuer_not_issued
            or bootstrap_not_issued
            or revoked
            or quarantined
        )
    ):
        fail("manual_recovery_required")
    if not (revoked or issuer_not_issued or bootstrap_not_issued):
        fail("manual_recovery_required")
    if bootstrap_not_issued:
        return lifecycle
    current_boot_identity = current_darwin_boot_identity()
    if lifecycle["boot_identity"] == current_boot_identity:
        try:
            os.killpg(lifecycle["process_group_id"], 0)
        except ProcessLookupError:
            pass
        except PermissionError:
            fail("manual_recovery_required")
        else:
            # macOS has no pidfd-like group identity handle. Never signal this group;
            # live, EPERM, or numeric-identity reuse stays a manual boundary.
            fail("manual_recovery_required")
    # A different canonical boot identity proves every process group from the marker's
    # boot is gone. Do not even probe the stale numeric pgid: it may have been reused.
    return lifecycle


def validate_d2a_teardown_fence(context, fence):
    if (
        not isinstance(fence, dict)
        or set(fence) != set(D2A_TEARDOWN_FENCE_FIELDS)
        or type(fence.get("schema_version")) is not int
        or fence.get("schema_version") != 1
        or fence.get("kind") != "starring.d2a.teardown-fence.v1"
        or fence.get("run_id") != context.manifest["run_id"]
        or fence.get("manifest_sha256") != context.digest
        or fence.get("status") not in {"closing", "closed"}
        or not validate_utc_timestamp(fence.get("updated_at"))
    ):
        fail("d2a_teardown_fence_invalid")
    return fence


def transition_d2a_teardown_fence(context, status):
    if status not in {"closing", "closed"}:
        fail("d2a_teardown_fence_invalid")
    path = d2a_teardown_fence_path(context)
    if os.path.lexists(path):
        current = validate_d2a_teardown_fence(
            context,
            load_strict_d2a_marker(
                path,
                "d2a_teardown_fence_invalid",
                D2A_TEARDOWN_FENCE_FIELDS,
                sorted_canonical=True,
            ),
        )
        if current["status"] == "closed" and status == "closing":
            return True
    fence = {
        "schema_version": 1,
        "kind": "starring.d2a.teardown-fence.v1",
        "run_id": context.manifest["run_id"],
        "manifest_sha256": context.digest,
        "status": status,
        "updated_at": utc_now(),
    }
    validate_d2a_teardown_fence(context, fence)
    write_atomic(path, canonical_json(fence) + "\n")
    return True


def begin_d2a_teardown(context):
    lifecycle = require_d2a_session_revoked(context)
    if lifecycle:
        transition_d2a_teardown_fence(context, "closing")
        return True
    return False


def complete_d2a_teardown(context, automated):
    if automated:
        transition_d2a_teardown_fence(context, "closed")


def close_bootstrap_prestart_teardown_fence(context, platform, lifecycle):
    if not (
        lifecycle.get("origin") == "bootstrap"
        and lifecycle.get("operation") == "direct-onboard"
        and lifecycle.get("status") == "not_issued"
        and lifecycle.get("process_group_id") is None
        and lifecycle.get("session_revoked") is False
        and lifecycle.get("revoked_at") is None
        and lifecycle.get("quarantined_at") is None
    ):
        fail("manual_recovery_required")
    try:
        state = load_state(context, {"prepared"})
    except BaseException:
        fail("manual_recovery_required")
    mutation_paths = (
        candidate_start_transition_path(context),
        candidate_start_source_path(context),
        candidate_start_retirement_path(context),
        context.artifact_directory / "step-03-evidence.json",
        context.artifact_directory / "onboarding-evidence.json",
        context.artifact_directory / "transport-evidence",
        discord_teardown_progress_path(context),
        discord_teardown_progress_path(context, frozen=True),
        discord_teardown_evidence_path(context),
        discord_teardown_evidence_path(context, frozen=True),
        abort_teardown_tombstone_path(context),
    )
    try:
        services_absent = candidate_launchd_absent(context, platform)
        postgres_absent = not platform.postgres_running(context.cluster_root)
    except BaseException:
        fail("manual_recovery_required")
    if (
        state["phase"] != "prepared"
        or candidate_start_commitment_present(context)
        or not services_absent
        or not postgres_absent
        or any(os.path.lexists(path) for path in mutation_paths)
    ):
        fail("manual_recovery_required")
    transition_d2a_teardown_fence(context, "closed")


def require_d2a_cleanup_fence(context, platform):
    marker_paths = (
        d2a_taint_path(context),
        d2a_session_lifecycle_path(context),
        d2a_teardown_fence_path(context),
    )
    if not any(os.path.lexists(path) for path in marker_paths):
        return
    lifecycle = require_d2a_session_revoked(context)
    path = d2a_teardown_fence_path(context)
    if not os.path.lexists(path):
        if not isinstance(lifecycle, dict):
            fail("manual_recovery_required")
        close_bootstrap_prestart_teardown_fence(context, platform, lifecycle)
    fence = validate_d2a_teardown_fence(
        context,
        load_strict_d2a_marker(
            path,
            "d2a_teardown_fence_invalid",
            D2A_TEARDOWN_FENCE_FIELDS,
            sorted_canonical=True,
        ),
    )
    if fence["status"] != "closed":
        fail("manual_recovery_required")


def command_teardown_discord_resources(context, platform, frozen=False):
    if frozen:
        require_certification_eligible_teardown(context)
        boundary = require_frozen_discord_teardown_boundary
    else:
        def boundary(boundary_context, boundary_platform):
            return require_candidate_certification_boundary(
                boundary_context,
                boundary_platform,
                allow_abort_teardown=True,
            )
    _state, snapshot = boundary(context, platform)
    # Production dispatch holds the global D2 lock around this entire command.
    # Prove the candidate-start boundary before opening the durable teardown
    # transaction: a bootstrap sentinel for a merely prepared run must fail
    # here without leaving a `closing` fence that would brick its safe direct
    # cleanup path.  Once the boundary is proven, write `closing` before the
    # first inventory/tombstone/deletion mutation.
    automated = begin_d2a_teardown(context)
    inventory = platform.transport_control(context, "resource_inventory")
    if inventory["instance_id"] != snapshot["instance_id"]:
        fail("transport_instance_changed")
    certification_binding = certified_teardown_binding(context) if frozen else None
    if not frozen:
        ensure_abort_teardown_tombstone(context, inventory)
    evidence_path = discord_teardown_evidence_path(context, frozen)
    if evidence_path.exists():
        evidence = load_private_json(
            evidence_path, "discord_resource_teardown_evidence"
        )
        validate_discord_teardown_evidence(
            context, evidence, inventory, certification_binding
        )
        observe_absent_discord_resources(context, platform, inventory)
        final_inventory = platform.transport_control(context, "resource_inventory")
        if final_inventory["digest_sha256"] != inventory["digest_sha256"]:
            fail("discord_resource_teardown_replay_drift")
        boundary(context, platform)
        complete_d2a_teardown(context, automated)
        return {
            "status": "exact_replay",
            "phase": "candidate_started",
            "transport_instance_id": inventory["instance_id"],
            "inventory_digest_sha256": inventory["digest_sha256"],
            "resource_count": len(inventory["created"]),
            "all_resources_absent": True,
            "evidence": str(evidence_path),
        }
    progress_path = discord_teardown_progress_path(context, frozen)
    if progress_path.exists():
        progress = load_private_json(
            progress_path, "discord_resource_teardown_progress"
        )
        validate_discord_teardown_progress(context, progress, inventory)
        if certification_binding is not None and progress[
            "source_inventory_digest_sha256"
        ] != certification_binding["freeze_resource_inventory_digest_sha256"]:
            fail("discord_resource_teardown_progress_invalid")
    else:
        if certification_binding is not None and inventory["digest_sha256"] != (
            certification_binding["freeze_resource_inventory_digest_sha256"]
        ):
            fail("discord_teardown_live_inventory_drift")
        progress = new_discord_teardown_progress(context, inventory)
        append_journal(context, "discord_resource_teardown", "intent", "resources")
        write_discord_teardown_progress(context, progress, frozen)
    progress = reconcile_discord_teardown_progress(
        context, progress, inventory, frozen
    )
    completed = {
        discord_resource_identity_key(discord_teardown_record_resource(record))
        for record in progress["deletions"]
    }
    for resource in sorted(inventory["created"], key=discord_resource_teardown_key):
        key = discord_resource_identity_key(resource)
        if key in completed:
            continue
        current = platform.transport_control(context, "resource_inventory")
        if (
            current["instance_id"] != inventory["instance_id"]
            or current["created"] != inventory["created"]
        ):
            fail("discord_resource_teardown_inventory_drift")
        if resource not in current["active"]:
            progress["deletions"].append(
                discord_teardown_record(resource, "reconciled_deleted")
            )
        else:
            deletion = platform.discord_delete_resource_through_transport(
                context, resource, current
            )
            record = normalize_proxy_deletion(current, resource, deletion)
            refreshed = platform.transport_control(context, "resource_inventory")
            if (
                refreshed["instance_id"] != inventory["instance_id"]
                or refreshed["created"] != inventory["created"]
                or resource in refreshed["active"]
                or resource not in refreshed["deleted"]
            ):
                fail("discord_resource_lifecycle_not_deleted")
            progress["deletions"].append(record)
        progress["deletions"].sort(
            key=lambda value: discord_resource_teardown_key(
                discord_teardown_record_resource(value)
            )
        )
        write_discord_teardown_progress(context, progress, frozen)
        completed.add(key)
    final_inventory = platform.transport_control(context, "resource_inventory")
    if (
        final_inventory["instance_id"] != inventory["instance_id"]
        or final_inventory["created"] != inventory["created"]
        or final_inventory["deleted"] != inventory["created"]
        or final_inventory["active"] != []
    ):
        fail("discord_resource_teardown_incomplete")
    validate_discord_teardown_progress(context, progress, final_inventory)
    if len(progress["deletions"]) != len(final_inventory["created"]):
        fail("discord_resource_teardown_incomplete")
    observations = observe_absent_discord_resources(
        context, platform, final_inventory
    )
    _state, final_snapshot = boundary(context, platform)
    confirmed_inventory = platform.transport_control(context, "resource_inventory")
    if (
        final_snapshot["instance_id"] != final_inventory["instance_id"]
        or confirmed_inventory["digest_sha256"]
        != final_inventory["digest_sha256"]
    ):
        fail("discord_resource_teardown_final_drift")
    evidence = {
        "schema_version": 1,
        "kind": DISCORD_TEARDOWN_EVIDENCE_KIND,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "recorded_at": utc_now(),
        "transport_instance_id": final_inventory["instance_id"],
        "source_inventory_digest_sha256": progress[
            "source_inventory_digest_sha256"
        ],
        "final_inventory_digest_sha256": final_inventory["digest_sha256"],
        "resource_union_sha256": progress["resource_union_sha256"],
        "created_resources": final_inventory["created"],
        "deleted_resources": final_inventory["deleted"],
        "active_resources": final_inventory["active"],
        **discord_resource_id_lists(final_inventory["created"]),
        "proxy_deletions": progress["deletions"],
        "direct_observations": observations,
        "all_resources_absent": True,
        **(certification_binding or {}),
    }
    validate_discord_teardown_evidence(
        context, evidence, final_inventory, certification_binding
    )
    write_atomic(evidence_path, canonical_json(evidence) + "\n")
    append_journal(context, "discord_resource_teardown", "complete", "resources")
    complete_d2a_teardown(context, automated)
    return {
        "status": "torn_down",
        "phase": "candidate_started",
        "transport_instance_id": final_inventory["instance_id"],
        "inventory_digest_sha256": final_inventory["digest_sha256"],
        "resource_count": len(final_inventory["created"]),
        "all_resources_absent": True,
        "evidence": str(evidence_path),
    }


def cleanup_root_quarantine_name(context):
    return f".{context.root.name}.cleanup-{context.digest[:16]}"


def cleanup_root_quarantine_path(context):
    return context.root.parent / cleanup_root_quarantine_name(context)


def cleanup_root_progress_path(context):
    return context.artifact_directory / "cleanup-root-progress.json"


def cleanup_root_identity_path(context):
    return context.artifact_directory / "cleanup-root-identity.json"


def cleanup_path_metadata(path, code):
    try:
        return path.lstat()
    except FileNotFoundError:
        return None
    except OSError:
        fail(code)


def validate_cleanup_root_directory(context, root):
    expected = isolated_runtime_root(context.manifest["run_id"])
    if context.root != expected or context.root.parent != pathlib.Path("/private/tmp"):
        fail("cleanup_root_guard_failed")
    metadata = cleanup_path_metadata(root, "cleanup_root_invalid")
    if metadata is None:
        return None
    parent = cleanup_path_metadata(root.parent, "cleanup_root_invalid")
    if parent is None:
        fail("cleanup_root_invalid")
    if (
        not stat.S_ISDIR(parent.st_mode)
        or root.parent.is_symlink()
        or not stat.S_ISDIR(metadata.st_mode)
        or root.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
        or metadata.st_dev != parent.st_dev
    ):
        fail("cleanup_root_invalid")
    cluster_root = root / "postgres"
    cluster = cleanup_path_metadata(cluster_root, "cleanup_cluster_invalid")
    if cluster is not None and (
        not stat.S_ISDIR(cluster.st_mode)
        or cluster_root.is_symlink()
        or cluster.st_uid != os.getuid()
        or stat.S_IMODE(cluster.st_mode) != 0o700
        or cluster.st_dev != metadata.st_dev
    ):
        fail("cleanup_cluster_invalid")
    try:
        for directory, names, files in os.walk(root, followlinks=False):
            for name in names + files:
                item = (pathlib.Path(directory) / name).lstat()
                if item.st_dev != metadata.st_dev:
                    fail("cleanup_mount_boundary_invalid")
    except OSError:
        fail("cleanup_root_invalid")
    return metadata


def validate_cleanup_root_identity(context, identity):
    if (
        not isinstance(identity, dict)
        or set(identity)
        != {
            "schema_version",
            "kind",
            "manifest_sha256",
            "run_id",
            "root_path",
            "root_device",
            "root_inode",
            "parent_device",
            "owner_uid",
        }
        or identity.get("schema_version") != 1
        or identity.get("kind") != CLEANUP_ROOT_IDENTITY_KIND
        or identity.get("manifest_sha256") != context.digest
        or identity.get("run_id") != context.manifest["run_id"]
        or identity.get("root_path") != str(context.root)
        or type(identity.get("root_device")) is not int
        or identity["root_device"] < 0
        or type(identity.get("root_inode")) is not int
        or identity["root_inode"] <= 0
        or type(identity.get("parent_device")) is not int
        or identity["parent_device"] < 0
        or identity["root_device"] != identity["parent_device"]
        or identity.get("owner_uid") != os.getuid()
    ):
        fail("cleanup_root_identity_invalid")
    return identity


def load_cleanup_root_identity(context):
    path = cleanup_root_identity_path(context)
    metadata = cleanup_path_metadata(path, "cleanup_root_identity_invalid")
    if metadata is None:
        return None
    require_owned_mode(path, 0o600, "cleanup_root_identity")
    return validate_cleanup_root_identity(
        context, load_json(path, "cleanup_root_identity_invalid")
    )


def record_cleanup_root_identity(context):
    if cleanup_path_metadata(
        cleanup_root_identity_path(context), "cleanup_root_identity_invalid"
    ) is not None:
        fail("cleanup_root_identity_busy")
    metadata = validate_cleanup_root_directory(context, context.root)
    parent = cleanup_path_metadata(context.root.parent, "cleanup_root_invalid")
    if metadata is None or parent is None:
        fail("cleanup_root_identity_invalid")
    identity = {
        "schema_version": 1,
        "kind": CLEANUP_ROOT_IDENTITY_KIND,
        "manifest_sha256": context.digest,
        "run_id": context.manifest["run_id"],
        "root_path": str(context.root),
        "root_device": metadata.st_dev,
        "root_inode": metadata.st_ino,
        "parent_device": parent.st_dev,
        "owner_uid": os.getuid(),
    }
    validate_cleanup_root_identity(context, identity)
    write_atomic(
        cleanup_root_identity_path(context), canonical_json(identity) + "\n"
    )
    return identity


def cleanup_root_identity_matches(metadata, identity):
    return (
        metadata is not None
        and stat.S_ISDIR(metadata.st_mode)
        and metadata.st_uid == identity["owner_uid"]
        and metadata.st_dev == identity["root_device"]
        and metadata.st_ino == identity["root_inode"]
    )


def load_cleanup_root_progress(context, identity=None):
    path = cleanup_root_progress_path(context)
    metadata = cleanup_path_metadata(path, "cleanup_root_progress_invalid")
    if metadata is None:
        return None
    if identity is None:
        identity = load_cleanup_root_identity(context)
    if identity is None:
        fail("cleanup_root_progress_invalid")
    require_owned_mode(path, 0o600, "cleanup_root_progress")
    progress = load_json(path, "cleanup_root_progress_invalid")
    if (
        not isinstance(progress, dict)
        or set(progress)
        != {
            "schema_version",
            "kind",
            "manifest_sha256",
            "run_id",
            "root_device",
            "root_inode",
            "quarantine_name",
            "phase",
        }
        or type(progress.get("schema_version")) is not int
        or progress.get("schema_version") != 1
        or progress.get("kind") != CLEANUP_ROOT_PROGRESS_KIND
        or progress.get("manifest_sha256") != context.digest
        or progress.get("run_id") != context.manifest["run_id"]
        or type(progress.get("root_device")) is not int
        or progress["root_device"] < 0
        or type(progress.get("root_inode")) is not int
        or progress["root_inode"] <= 0
        or progress.get("quarantine_name") != cleanup_root_quarantine_name(context)
        or progress.get("phase") not in {"planned", "quarantined", "deleted"}
        or progress.get("root_device") != identity["root_device"]
        or progress.get("root_inode") != identity["root_inode"]
    ):
        fail("cleanup_root_progress_invalid")
    return progress


def save_cleanup_root_progress(context, progress, phase):
    updated = {**progress, "phase": phase}
    write_atomic(
        cleanup_root_progress_path(context), canonical_json(updated) + "\n"
    )
    return updated


def cleanup_root_metadata_matches(metadata, progress):
    return (
        metadata is not None
        and stat.S_ISDIR(metadata.st_mode)
        and metadata.st_uid == os.getuid()
        and metadata.st_dev == progress["root_device"]
        and metadata.st_ino == progress["root_inode"]
    )


def remove_cleanup_tree_contents(descriptor, expected_device):
    try:
        entries = list(os.scandir(descriptor))
    except OSError:
        fail("cleanup_root_delete_failed")
    for entry in entries:
        try:
            before = os.stat(
                entry.name, dir_fd=descriptor, follow_symlinks=False
            )
        except OSError:
            fail("cleanup_root_swap_detected")
        if before.st_dev != expected_device:
            fail("cleanup_mount_boundary_invalid")
        if stat.S_ISDIR(before.st_mode):
            try:
                child = os.open(
                    entry.name,
                    os.O_RDONLY
                    | getattr(os, "O_DIRECTORY", 0)
                    | getattr(os, "O_NOFOLLOW", 0),
                    dir_fd=descriptor,
                )
            except OSError:
                fail("cleanup_root_swap_detected")
            try:
                opened = os.fstat(child)
                if (
                    opened.st_dev != before.st_dev
                    or opened.st_ino != before.st_ino
                ):
                    fail("cleanup_root_swap_detected")
                remove_cleanup_tree_contents(child, expected_device)
            finally:
                os.close(child)
            try:
                after = os.stat(
                    entry.name, dir_fd=descriptor, follow_symlinks=False
                )
                if after.st_dev != before.st_dev or after.st_ino != before.st_ino:
                    fail("cleanup_root_swap_detected")
                os.rmdir(entry.name, dir_fd=descriptor)
            except OSError:
                fail("cleanup_root_swap_detected")
        else:
            try:
                after = os.stat(
                    entry.name, dir_fd=descriptor, follow_symlinks=False
                )
                if after.st_dev != before.st_dev or after.st_ino != before.st_ino:
                    fail("cleanup_root_swap_detected")
                os.unlink(entry.name, dir_fd=descriptor)
            except OSError:
                fail("cleanup_root_swap_detected")


def remove_cleanup_quarantine(context, progress):
    flags = (
        os.O_RDONLY
        | getattr(os, "O_DIRECTORY", 0)
        | getattr(os, "O_NOFOLLOW", 0)
    )
    try:
        parent = os.open(context.root.parent, flags)
    except OSError:
        fail("cleanup_root_invalid")
    try:
        try:
            before = os.stat(
                progress["quarantine_name"],
                dir_fd=parent,
                follow_symlinks=False,
            )
        except OSError:
            fail("cleanup_root_swap_detected")
        if not cleanup_root_metadata_matches(before, progress):
            fail("cleanup_root_swap_detected")
        try:
            root = os.open(
                progress["quarantine_name"], flags, dir_fd=parent
            )
        except OSError:
            fail("cleanup_root_swap_detected")
        try:
            opened = os.fstat(root)
            if not cleanup_root_metadata_matches(opened, progress):
                fail("cleanup_root_swap_detected")
            remove_cleanup_tree_contents(root, progress["root_device"])
        finally:
            os.close(root)
        try:
            after = os.stat(
                progress["quarantine_name"],
                dir_fd=parent,
                follow_symlinks=False,
            )
            if not cleanup_root_metadata_matches(after, progress):
                fail("cleanup_root_swap_detected")
            os.rmdir(progress["quarantine_name"], dir_fd=parent)
            os.fsync(parent)
        except OSError:
            fail("cleanup_root_swap_detected")
    finally:
        os.close(parent)


def require_quarantined_cleanup_substrate_inert(context, platform):
    if not cleanup_postgres_absent(context, platform):
        fail("cleanup_postgres_active_after_quarantine")
    if not candidate_launchd_absent(context, platform):
        fail("cleanup_launchd_active_after_quarantine")


def guarded_remove_root(context, platform):
    expected = isolated_runtime_root(context.manifest["run_id"])
    if context.root != expected or context.root.parent != pathlib.Path("/private/tmp"):
        fail("cleanup_root_guard_failed")
    identity = load_cleanup_root_identity(context)
    root_metadata = cleanup_path_metadata(context.root, "cleanup_root_invalid")
    quarantined = cleanup_root_quarantine_path(context)
    quarantine_metadata = cleanup_path_metadata(quarantined, "cleanup_root_invalid")
    progress = load_cleanup_root_progress(context, identity)
    if root_metadata is not None and quarantine_metadata is not None:
        fail("cleanup_root_swap_detected")
    if (
        identity is None
        and (
            root_metadata is not None
            or quarantine_metadata is not None
            or progress is not None
        )
    ):
        fail("cleanup_root_identity_invalid")
    if identity is None:
        return
    if progress is not None and progress["phase"] == "deleted" and (
        root_metadata is not None or quarantine_metadata is not None
    ):
        fail("cleanup_root_swap_detected")
    if root_metadata is not None:
        validated = validate_cleanup_root_directory(context, context.root)
        if not cleanup_root_identity_matches(validated, identity):
            fail("cleanup_root_swap_detected")
        if progress is None:
            progress = {
                "schema_version": 1,
                "kind": CLEANUP_ROOT_PROGRESS_KIND,
                "manifest_sha256": context.digest,
                "run_id": context.manifest["run_id"],
                "root_device": identity["root_device"],
                "root_inode": identity["root_inode"],
                "quarantine_name": cleanup_root_quarantine_name(context),
                "phase": "planned",
            }
            write_atomic(
                cleanup_root_progress_path(context),
                canonical_json(progress) + "\n",
            )
        if not cleanup_root_metadata_matches(validated, progress):
            fail("cleanup_root_swap_detected")
        if progress["phase"] != "planned":
            fail("cleanup_root_swap_detected")
        flags = (
            os.O_RDONLY
            | getattr(os, "O_DIRECTORY", 0)
            | getattr(os, "O_NOFOLLOW", 0)
        )
        try:
            parent = os.open(context.root.parent, flags)
        except OSError:
            fail("cleanup_root_invalid")
        try:
            try:
                before = os.stat(
                    context.root.name, dir_fd=parent, follow_symlinks=False
                )
                if not cleanup_root_metadata_matches(before, progress):
                    fail("cleanup_root_swap_detected")
                rename_exclusive(
                    parent,
                    context.root.name,
                    parent,
                    progress["quarantine_name"],
                )
                after = os.stat(
                    progress["quarantine_name"],
                    dir_fd=parent,
                    follow_symlinks=False,
                )
                if not cleanup_root_metadata_matches(after, progress):
                    fail("cleanup_root_swap_detected")
                os.fsync(parent)
            except OSError:
                fail("cleanup_root_swap_detected")
        finally:
            os.close(parent)
        require_quarantined_cleanup_substrate_inert(context, platform)
        progress = save_cleanup_root_progress(context, progress, "quarantined")
    elif quarantine_metadata is not None:
        if progress is None or not cleanup_root_metadata_matches(
            quarantine_metadata, progress
        ):
            fail("cleanup_root_swap_detected")
        if progress["phase"] not in {"planned", "quarantined"}:
            fail("cleanup_root_swap_detected")
        validate_cleanup_root_directory(context, quarantined)
        require_quarantined_cleanup_substrate_inert(context, platform)
        progress = save_cleanup_root_progress(context, progress, "quarantined")
    elif progress is not None:
        if progress["phase"] == "quarantined":
            save_cleanup_root_progress(context, progress, "deleted")
            return
        if progress["phase"] == "deleted":
            return
        fail("cleanup_root_loss_unproven")
    else:
        fail("cleanup_root_loss_unproven")
    remove_cleanup_quarantine(context, progress)
    save_cleanup_root_progress(context, progress, "deleted")


def validate_cleanup_mutation_roots(context):
    expected = isolated_runtime_root(context.manifest["run_id"])
    if context.root != expected or context.cluster_root != expected / "postgres":
        fail("cleanup_root_guard_failed")
    root_metadata = cleanup_path_metadata(context.root, "cleanup_root_invalid")
    quarantined = cleanup_root_quarantine_path(context)
    quarantine_metadata = cleanup_path_metadata(quarantined, "cleanup_root_invalid")
    identity = load_cleanup_root_identity(context)
    progress = load_cleanup_root_progress(context, identity)
    if root_metadata is not None and quarantine_metadata is not None:
        fail("cleanup_root_swap_detected")
    if (
        identity is None
        and (
            root_metadata is not None
            or quarantine_metadata is not None
            or progress is not None
        )
    ):
        fail("cleanup_root_identity_invalid")
    if root_metadata is not None:
        validated = validate_cleanup_root_directory(context, context.root)
        if not cleanup_root_identity_matches(validated, identity):
            fail("cleanup_root_swap_detected")
        if progress is not None and progress["phase"] != "planned":
            fail("cleanup_root_swap_detected")
    if quarantine_metadata is not None:
        if progress is None or not cleanup_root_metadata_matches(
            quarantine_metadata, progress
        ):
            fail("cleanup_root_swap_detected")
        if progress["phase"] not in {"planned", "quarantined"}:
            fail("cleanup_root_swap_detected")
        validate_cleanup_root_directory(context, quarantined)


def filesystem_entry_present(path, code):
    try:
        path.lstat()
    except FileNotFoundError:
        return False
    except OSError:
        fail(code)
    return True


def cleanup_postgres_absent(context, platform):
    original = context.cluster_root
    quarantined = cleanup_root_quarantine_path(context) / "postgres"
    original_present = filesystem_entry_present(
        original, "cleanup_cluster_invalid"
    )
    quarantine_present = filesystem_entry_present(
        quarantined, "cleanup_cluster_invalid"
    )
    if original_present and quarantine_present:
        fail("cleanup_root_swap_detected")
    if original_present:
        return platform.postgres_absent(original)
    if quarantine_present:
        return platform.postgres_absent(
            quarantined
        ) and platform.postgres_process_path_absent(original)
    return platform.postgres_process_path_absent(
        original
    ) and platform.postgres_process_path_absent(quarantined)


def cleanup_absence(
    context, platform, expected_snapshot, *, audited_keychain=False
):
    root_present = filesystem_entry_present(
        context.root, "cleanup_root_invalid"
    ) or filesystem_entry_present(
        cleanup_root_quarantine_path(context), "cleanup_root_invalid"
    )
    if audited_keychain:
        keychain_observed = platform.audited_keychain_item_identities(
            tuple(keychain_inventory(context)),
            AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHORS,
        )
        keychain_items_absent = all(
            identity is None for identity in keychain_observed.values()
        )
    else:
        keychain_items_absent = all(
            not platform.keychain_present(service, account)
            for service, account in keychain_inventory(context)
        )
    return {
        "database_absent": not root_present,
        "postgres_process_absent": cleanup_postgres_absent(context, platform),
        "launchd_jobs_absent": candidate_launchd_absent(context, platform),
        "keychain_items_absent": keychain_items_absent,
        "isolated_root_absent": not root_present,
        "protected_staging_unchanged": standing_snapshot(context, platform)
        == expected_snapshot,
    }


def new_cleanup_evidence(context, absence):
    evidence = {
        "schema_version": 1,
        "manifest_sha256": context.digest,
        "observed_at": utc_now(),
        **absence,
    }
    return validate_cleanup_evidence(context, evidence)


def validate_cleanup_evidence(context, evidence):
    boolean_fields = {
        "database_absent",
        "postgres_process_absent",
        "launchd_jobs_absent",
        "keychain_items_absent",
        "isolated_root_absent",
        "protected_staging_unchanged",
    }
    if (
        not isinstance(evidence, dict)
        or set(evidence)
        != {
            "schema_version",
            "manifest_sha256",
            "observed_at",
            *boolean_fields,
        }
        or type(evidence.get("schema_version")) is not int
        or evidence.get("schema_version") != 1
        or evidence.get("manifest_sha256") != context.digest
        or not isinstance(evidence.get("observed_at"), str)
        or not EVIDENCE_RECORDED_AT_PATTERN.fullmatch(evidence["observed_at"])
        or any(evidence.get(field) is not True for field in boolean_fields)
    ):
        fail("cleanup_evidence_invalid")
    return evidence


def require_terminal_cleanup_root_progress(context):
    root = cleanup_path_metadata(context.root, "cleanup_root_invalid")
    quarantine = cleanup_path_metadata(
        cleanup_root_quarantine_path(context), "cleanup_root_invalid"
    )
    if root is not None or quarantine is not None:
        fail("cleanup_root_not_deleted")
    identity = load_cleanup_root_identity(context)
    progress = load_cleanup_root_progress(context, identity)
    if identity is None:
        if progress is not None:
            fail("cleanup_root_progress_invalid")
        return None
    if progress is None:
        fail("cleanup_root_progress_invalid")
    if progress["phase"] != "deleted":
        fail("cleanup_root_progress_not_terminal")
    return progress


def cleanup_journal_rows(context):
    try:
        rows = load_lifecycle_journal(context)
    except BaseException:
        fail("cleanup_journal_invalid")
    for row in rows:
        if row["action"] == "cleanup" and (
            row["target"] != "run"
            or row["status"] not in {"intent", "failed", "complete"}
        ):
            fail("cleanup_journal_invalid")
    return rows


def ensure_cleanup_journal_complete(context):
    rows = cleanup_journal_rows(context)
    cleanup_rows = [
        (index, row)
        for index, row in enumerate(rows)
        if row["action"] == "cleanup" and row["target"] == "run"
    ]
    intents = [index for index, row in cleanup_rows if row["status"] == "intent"]
    if not intents:
        fail("cleanup_journal_incomplete")
    last_intent = intents[-1]
    if (
        cleanup_rows[-1][0] <= last_intent
        or cleanup_rows[-1][1]["status"] != "complete"
    ):
        append_journal(context, "cleanup", "complete", "run")
        rows = cleanup_journal_rows(context)
        cleanup_rows = [
            (index, row)
            for index, row in enumerate(rows)
            if row["action"] == "cleanup" and row["target"] == "run"
        ]
        if (
            cleanup_rows[-1][0] <= last_intent
            or cleanup_rows[-1][1]["status"] != "complete"
        ):
            fail("cleanup_journal_incomplete")


def cleanup_keychain_baseline_path(context):
    return context.artifact_directory / "cleanup-keychain-baseline.json"


def validate_cleanup_keychain_baseline(context, baseline):
    inventory = tuple(keychain_inventory(context))
    if (
        not isinstance(baseline, dict)
        or set(baseline)
        != {
            "schema_version",
            "kind",
            "manifest_sha256",
            "run_id",
            "inventory",
        }
        or type(baseline.get("schema_version")) is not int
        or baseline.get("schema_version") != 1
        or baseline.get("kind") != CLEANUP_KEYCHAIN_BASELINE_KIND
        or baseline.get("manifest_sha256") != context.digest
        or baseline.get("run_id") != context.manifest["run_id"]
        or not isinstance(baseline.get("inventory"), list)
        or len(baseline["inventory"]) != len(inventory)
    ):
        fail("cleanup_keychain_baseline_invalid")
    observed_inventory = []
    identities = {}
    for entry in baseline["inventory"]:
        if (
            not isinstance(entry, dict)
            or set(entry) != {"service", "account", "identity_sha256"}
            or not isinstance(entry.get("service"), str)
            or not isinstance(entry.get("account"), str)
            or (
                entry.get("identity_sha256") is not None
                and (
                    not isinstance(entry["identity_sha256"], str)
                    or not DIGEST_PATTERN.fullmatch(entry["identity_sha256"])
                )
            )
        ):
            fail("cleanup_keychain_baseline_invalid")
        identity = (entry["service"], entry["account"])
        if identity in identities:
            fail("cleanup_keychain_baseline_invalid")
        observed_inventory.append(identity)
        identities[identity] = entry["identity_sha256"]
    if tuple(observed_inventory) != inventory:
        fail("cleanup_keychain_baseline_invalid")
    for service in {service for service, _account in inventory}:
        service_identities = {
            account: identities[(service, account)]
            for item_service, account in inventory
            if item_service == service
        }
        if any(value is not None for value in service_identities.values()) and (
            service_identities.get(OWNER_ACCOUNT) is None
        ):
            fail("cleanup_keychain_baseline_invalid")
    return identities


def observe_cleanup_keychain_inventory(context, platform):
    observed = {}
    for service, account in keychain_inventory(context):
        identity = platform.keychain_item_identity(service, account)
        if identity is not None and (
            not isinstance(identity, str)
            or not DIGEST_PATTERN.fullmatch(identity)
        ):
            fail("cleanup_keychain_identity_invalid")
        observed[(service, account)] = identity
    return observed


def load_cleanup_keychain_baseline(context):
    path = cleanup_keychain_baseline_path(context)
    if cleanup_path_metadata(path, "cleanup_keychain_baseline_invalid") is None:
        fail("cleanup_keychain_baseline_invalid")
    require_owned_mode(path, 0o600, "cleanup_keychain_baseline")
    return validate_cleanup_keychain_baseline(
        context, load_json(path, "cleanup_keychain_baseline_invalid")
    )


def load_or_create_cleanup_keychain_baseline(context, platform):
    path = cleanup_keychain_baseline_path(context)
    metadata = cleanup_path_metadata(path, "cleanup_keychain_baseline_invalid")
    if metadata is None:
        observed = observe_cleanup_keychain_inventory(context, platform)
        baseline = {
            "schema_version": 1,
            "kind": CLEANUP_KEYCHAIN_BASELINE_KIND,
            "manifest_sha256": context.digest,
            "run_id": context.manifest["run_id"],
            "inventory": [
                {
                    "service": service,
                    "account": account,
                    "identity_sha256": observed[(service, account)],
                }
                for service, account in keychain_inventory(context)
            ],
        }
        identities = validate_cleanup_keychain_baseline(context, baseline)
        for service in {service for service, _account in identities}:
            if any(
                identity is not None
                for (item_service, _account), identity in identities.items()
                if item_service == service
            ) and not platform.keychain_owner_matches(
                service, context.manifest["run_id"]
            ):
                fail("cleanup_keychain_ownership_invalid")
        write_atomic(path, canonical_json(baseline) + "\n")
    return load_cleanup_keychain_baseline(context)


def load_or_create_audited_cleanup_keychain_baseline(
    context, platform, expected_inventory_sha256
):
    path = cleanup_keychain_baseline_path(context)
    metadata = cleanup_path_metadata(
        path, "cleanup_keychain_baseline_invalid"
    )
    created = metadata is None
    if created:
        inventory, inventory_sha256 = (
            audited_quarantined_cleanup_keychain_inventory(context, platform)
        )
        if inventory_sha256 != expected_inventory_sha256:
            fail("audited_quarantined_cleanup_keychain_boundary_invalid")
        baseline = {
            "schema_version": 1,
            "kind": CLEANUP_KEYCHAIN_BASELINE_KIND,
            "manifest_sha256": context.digest,
            "run_id": context.manifest["run_id"],
            "inventory": inventory,
        }
        validate_cleanup_keychain_baseline(context, baseline)
        write_atomic(path, canonical_json(baseline) + "\n")
    baseline = load_cleanup_keychain_baseline(context)
    canonical_inventory = [
        {
            "service": service,
            "account": account,
            "identity_sha256": identity_sha256,
        }
        for (service, account), identity_sha256 in baseline.items()
    ]
    inventory_sha256 = hashlib.sha256(
        json.dumps(
            canonical_inventory, ensure_ascii=False, sort_keys=True,
            separators=(",", ":"),
        ).encode("utf-8")
    ).hexdigest()
    if inventory_sha256 != expected_inventory_sha256:
        fail("audited_quarantined_cleanup_keychain_boundary_invalid")
    current = validate_audited_cleanup_keychain_replay(
        context, platform, baseline
    )
    if created and any(
        current[identity] != original for identity, original in baseline.items()
    ):
        fail("audited_quarantined_cleanup_keychain_boundary_invalid")
    return baseline


def audited_cleanup_keychain_inventory(
    context, platform, expected_inventory_sha256
):
    baseline = load_or_create_audited_cleanup_keychain_baseline(
        context, platform, expected_inventory_sha256
    )
    audited_cleanup_keychain_inventory_from_baseline(
        context, platform, baseline
    )


def require_audited_cleanup_keychain_policy(platform):
    expected = {
        "login_keychain_path": AUDITED_QUARANTINED_LOGIN_KEYCHAIN_PATH,
        "login_keychain_policy_kind": (
            AUDITED_QUARANTINED_LOGIN_KEYCHAIN_POLICY_KIND
        ),
        "login_keychain_policy_sha256": (
            AUDITED_QUARANTINED_LOGIN_KEYCHAIN_POLICY_SHA256
        ),
        "login_keychain_policy_verified": True,
    }
    if platform.quarantined_recovery_login_keychain_policy() != expected:
        fail("audited_quarantined_cleanup_keychain_policy_invalid")


def observe_audited_cleanup_keychain_inventory(context, platform):
    require_audited_cleanup_keychain_policy(platform)
    observed = platform.audited_keychain_item_identities(
        tuple(keychain_inventory(context)),
        AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHORS,
    )
    if any(
        identity is not None and (
            not isinstance(identity, str)
            or DIGEST_PATTERN.fullmatch(identity) is None
        )
        for identity in observed.values()
    ):
        fail("audited_quarantined_cleanup_keychain_boundary_invalid")
    require_audited_cleanup_keychain_policy(platform)
    return observed


def validate_audited_cleanup_keychain_replay(context, platform, baseline):
    current = observe_audited_cleanup_keychain_inventory(context, platform)
    for identity, original in baseline.items():
        observed = current[identity]
        if original is None:
            if observed is not None:
                fail("audited_quarantined_cleanup_keychain_identity_drift")
        elif observed not in {None, original}:
            fail("audited_quarantined_cleanup_keychain_identity_drift")
    for service in {service for service, _account in baseline}:
        pending = {
            account
            for (item_service, account), original in baseline.items()
            if item_service == service
            and original is not None
            and current[(item_service, account)] == original
        }
        if pending and (
            OWNER_ACCOUNT not in pending
            or not platform.audited_keychain_owner_matches(
                service, context.manifest["run_id"]
            )
        ):
            fail("audited_quarantined_cleanup_keychain_ownership_invalid")
    # The owner lookup is a separate explicit-path Security invocation. Reopen
    # the fixed keychain and prove the immutable anchors again before returning.
    platform.audited_keychain_item_identities(
        (), AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHORS
    )
    require_audited_cleanup_keychain_policy(platform)
    return current


def audited_cleanup_keychain_baseline_sha256(
    context, expected_inventory_sha256
):
    baseline_path = cleanup_keychain_baseline_path(context)
    raw = audited_private_file_bytes(
        baseline_path, {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_cleanup_keychain_boundary_invalid",
    )
    baseline_object = load_json(
        baseline_path,
        "audited_quarantined_cleanup_keychain_boundary_invalid",
    )
    if raw != (canonical_json(baseline_object) + "\n").encode("utf-8"):
        fail("audited_quarantined_cleanup_keychain_boundary_invalid")
    baseline = load_cleanup_keychain_baseline(context)
    inventory = [
        {
            "service": service,
            "account": account,
            "identity_sha256": identity_sha256,
        }
        for (service, account), identity_sha256 in baseline.items()
    ]
    inventory_sha256 = hashlib.sha256(
        json.dumps(
            inventory, ensure_ascii=False, sort_keys=True,
            separators=(",", ":"),
        ).encode("utf-8")
    ).hexdigest()
    if inventory_sha256 != expected_inventory_sha256:
        fail("audited_quarantined_cleanup_keychain_boundary_invalid")
    return hashlib.sha256(raw).hexdigest()


def audited_cleanup_root_progress_sha256(context):
    progress_path = cleanup_root_progress_path(context)
    raw = audited_private_file_bytes(
        progress_path, {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_cleanup_root_progress_invalid",
    )
    progress_object = load_json(
        progress_path, "audited_quarantined_cleanup_root_progress_invalid"
    )
    if raw != (canonical_json(progress_object) + "\n").encode("utf-8"):
        fail("audited_quarantined_cleanup_root_progress_invalid")
    identity = load_cleanup_root_identity(context)
    progress = load_cleanup_root_progress(context, identity)
    if (
        identity is None
        or progress is None
        or progress.get("phase") != "deleted"
        or progress.get("root_device") != identity["root_device"]
        or progress.get("root_inode") != identity["root_inode"]
    ):
        fail("audited_quarantined_cleanup_root_progress_invalid")
    return hashlib.sha256(raw).hexdigest()


def audited_cleanup_keychain_inventory_from_baseline(
    context, platform, baseline
):
    """Delete only sealed run-owned items from the fixed login keychain."""
    for service in sorted({service for service, _account in baseline}):
        accounts = sorted(
            (
                account
                for (item_service, account), original in baseline.items()
                if item_service == service and original is not None
            ),
            key=lambda account: account == OWNER_ACCOUNT,
        )
        for account in accounts:
            current = validate_audited_cleanup_keychain_replay(
                context, platform, baseline
            )
            target = (service, account)
            if current[target] is None:
                continue
            if not platform.audited_keychain_owner_matches(
                service, context.manifest["run_id"]
            ):
                fail("audited_quarantined_cleanup_keychain_ownership_invalid")
            platform.audited_keychain_item_identities(
                (), AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHORS
            )
            require_audited_cleanup_keychain_policy(platform)
            platform.audited_keychain_delete_exact(
                service,
                account,
                baseline[target],
                AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHORS,
            )
            require_audited_cleanup_keychain_policy(platform)
    current = validate_audited_cleanup_keychain_replay(
        context, platform, baseline
    )
    if any(identity is not None for identity in current.values()):
        fail("audited_quarantined_cleanup_keychain_delete_unconfirmed")


def cleanup_keychain_inventory_from_baseline(context, platform, baseline):
    """Delete only the exact identities sealed in a validated baseline."""
    for service in sorted({service for service, _account in baseline}):
        accounts = sorted(
            (
                account
                for (item_service, account), original in baseline.items()
                if item_service == service and original is not None
            ),
            key=lambda account: account == OWNER_ACCOUNT,
        )
        for account in accounts:
            current = validate_cleanup_keychain_replay(context, platform, baseline)
            target = (service, account)
            if current[target] is None:
                continue
            if not platform.keychain_owner_matches(
                service, context.manifest["run_id"]
            ):
                fail("cleanup_keychain_ownership_invalid")
            platform.keychain_delete_exact(service, account, baseline[target])
            if platform.keychain_item_identity(service, account) is not None:
                fail("cleanup_keychain_delete_unconfirmed")
    current = validate_cleanup_keychain_replay(context, platform, baseline)
    if any(identity is not None for identity in current.values()):
        fail("cleanup_keychain_delete_unconfirmed")


def validate_cleanup_keychain_replay(context, platform, baseline):
    current = observe_cleanup_keychain_inventory(context, platform)
    for identity, original in baseline.items():
        observed = current[identity]
        if original is None:
            if observed is not None:
                fail("cleanup_keychain_identity_drift")
        elif observed not in {None, original}:
            fail("cleanup_keychain_identity_drift")
    for service in {service for service, _account in baseline}:
        pending = {
            account
            for (item_service, account), original in baseline.items()
            if item_service == service
            and original is not None
            and current[(item_service, account)] == original
        }
        if pending and (
            OWNER_ACCOUNT not in pending
            or not platform.keychain_owner_matches(
                service, context.manifest["run_id"]
            )
        ):
            fail("cleanup_keychain_ownership_invalid")
    return current


def cleanup_keychain_inventory(context, platform):
    baseline = load_or_create_cleanup_keychain_baseline(context, platform)
    for service in sorted({service for service, _account in baseline}):
        accounts = sorted(
            (
                account
                for (item_service, account), original in baseline.items()
                if item_service == service and original is not None
            ),
            key=lambda account: account == OWNER_ACCOUNT,
        )
        for account in accounts:
            current = validate_cleanup_keychain_replay(
                context, platform, baseline
            )
            target = (service, account)
            if current[target] is None:
                continue
            if not platform.keychain_owner_matches(
                service, context.manifest["run_id"]
            ):
                fail("cleanup_keychain_ownership_invalid")
            platform.keychain_delete_exact(service, account, baseline[target])
            if platform.keychain_item_identity(service, account) is not None:
                fail("cleanup_keychain_delete_unconfirmed")
    current = validate_cleanup_keychain_replay(context, platform, baseline)
    if any(identity is not None for identity in current.values()):
        fail("cleanup_keychain_delete_unconfirmed")


def require_cleanup_keychain_baseline_absent(context, platform):
    baseline = load_cleanup_keychain_baseline(context)
    current = validate_cleanup_keychain_replay(context, platform, baseline)
    if any(identity is not None for identity in current.values()):
        fail("cleanup_keychain_delete_unconfirmed")


def require_audited_cleanup_keychain_baseline_absent(context, platform):
    baseline = load_cleanup_keychain_baseline(context)
    current = validate_audited_cleanup_keychain_replay(
        context, platform, baseline
    )
    if any(identity is not None for identity in current.values()):
        fail("audited_quarantined_cleanup_keychain_delete_unconfirmed")


def cleanup(
    context, platform, expected_snapshot, from_failure=False,
    audited_keychain_inventory_sha256=None,
):
    validate_cleanup_mutation_roots(context)
    append_journal(context, "cleanup", "intent", "run")
    failures = []
    for name in SERVICE_STOP_ORDER:
        label = context.manifest["services"][name]["label"]
        try:
            platform.launchd_bootout(label)
        except BaseException:
            failures.append(f"launchd:{name}")
    try:
        platform.postgres_stop(context.cluster_root)
    except BaseException:
        failures.append("postgres")
    try:
        if audited_keychain_inventory_sha256 is None:
            cleanup_keychain_inventory(context, platform)
        else:
            audited_cleanup_keychain_inventory(
                context, platform, audited_keychain_inventory_sha256
            )
    except BaseException:
        failures.append("keychain")
    try:
        postgres_inert = cleanup_postgres_absent(context, platform)
    except BaseException:
        failures.append("postgres_observation")
        postgres_inert = False
    try:
        launchd_inert = candidate_launchd_absent(context, platform)
    except BaseException:
        failures.append("launchd_observation")
        launchd_inert = False
    if postgres_inert and launchd_inert and not failures:
        try:
            guarded_remove_root(context, platform)
        except BaseException:
            failures.append("root")
    else:
        failures.append("root_removal_blocked")
    try:
        absence = cleanup_absence(
            context,
            platform,
            expected_snapshot,
            audited_keychain=audited_keychain_inventory_sha256 is not None,
        )
    except BaseException:
        failures.append("absence_observation")
        absence = {
            "database_absent": False,
            "postgres_process_absent": False,
            "launchd_jobs_absent": False,
            "keychain_items_absent": False,
            "isolated_root_absent": False,
            "protected_staging_unchanged": False,
        }
    if not all(absence.values()):
        failures.append("absence_verification")
    if failures:
        append_journal(context, "cleanup", "failed", "run")
        fail("cleanup_incomplete")
    require_terminal_cleanup_root_progress(context)
    try:
        release_discord_ownership(context)
    except BaseException:
        append_journal(context, "cleanup", "failed", "run")
        fail("cleanup_incomplete")
    save_state(context, "cleaned", expected_snapshot)
    evidence = new_cleanup_evidence(context, absence)
    write_atomic(
        context.artifact_directory / "cleanup-evidence.json",
        canonical_json(evidence) + "\n",
    )
    append_journal(context, "cleanup", "complete", "run")
    ensure_cleanup_journal_complete(context)
    return {
        "status": "cleaned_after_failure" if from_failure else "cleaned",
        "phase": "cleaned",
        "database_absent": True,
        "postgres_process_absent": True,
        "launchd_jobs_absent": True,
        "keychain_items_absent": True,
        "isolated_root_absent": True,
        "protected_staging_unchanged": True,
    }


def command_cleanup_internal(
    context, platform, retire_committed,
    audited_keychain_inventory_sha256=None,
):
    state = load_state(context)
    if retire_committed and state["phase"] != "cleaned":
        persist_candidate_abort_retirement(
            context, state, "explicit_cleanup"
        )
    if state["phase"] == "cleaned":
        require_discord_ownership_released(context)
        require_terminal_cleanup_root_progress(context)
        if audited_keychain_inventory_sha256 is not None:
            load_or_create_audited_cleanup_keychain_baseline(
                context, platform, audited_keychain_inventory_sha256
            )
        if audited_keychain_inventory_sha256 is None:
            require_cleanup_keychain_baseline_absent(context, platform)
        else:
            require_audited_cleanup_keychain_baseline_absent(context, platform)
        absence = cleanup_absence(
            context,
            platform,
            state["standing_snapshot"],
            audited_keychain=audited_keychain_inventory_sha256 is not None,
        )
        if not all(absence.values()):
            fail("cleanup_incomplete")
        path = context.artifact_directory / "cleanup-evidence.json"
        if path.exists() or path.is_symlink():
            require_owned_mode(path, 0o600, "cleanup_evidence")
            validate_cleanup_evidence(
                context, load_json(path, "cleanup_evidence_invalid")
            )
        else:
            write_atomic(
                path,
                canonical_json(new_cleanup_evidence(context, absence)) + "\n",
            )
        ensure_cleanup_journal_complete(context)
        return {
            "status": "already_cleaned",
            "phase": "cleaned",
            **absence,
        }
    return cleanup(
        context, platform, state["standing_snapshot"],
        audited_keychain_inventory_sha256=audited_keychain_inventory_sha256,
    )


def command_cleanup(context, platform):
    require_d2a_cleanup_fence(context, platform)
    return command_cleanup_internal(context, platform, retire_committed=True)


def audited_preissuer_rollback_intent_path(context):
    return context.artifact_directory / "audited-preissuer-rollback-recovery-intent.json"


def audited_preissuer_rollback_evidence_path(context):
    return context.artifact_directory / "audited-preissuer-rollback-recovery.json"


def audited_private_file_bytes(
    path, modes, maximum_bytes, code, *, allow_empty=False
):
    path = require_absolute_path(str(path), code)
    try:
        descriptor = os.open(
            path,
            os.O_RDONLY
            | getattr(os, "O_NOFOLLOW", 0)
            | getattr(os, "O_CLOEXEC", 0),
        )
    except OSError:
        fail(code)
    try:
        before = os.fstat(descriptor)
        if (
            not stat.S_ISREG(before.st_mode)
            or before.st_uid != os.getuid()
            or before.st_nlink != 1
            or stat.S_IMODE(before.st_mode) not in modes
            or (before.st_size <= 0 and not allow_empty)
            or before.st_size > maximum_bytes
        ):
            fail(code)
        raw = bytearray()
        while len(raw) <= maximum_bytes:
            chunk = os.read(descriptor, min(64 * 1024, maximum_bytes + 1 - len(raw)))
            if not chunk:
                break
            raw.extend(chunk)
        after = os.fstat(descriptor)
        try:
            named = os.stat(path, follow_symlinks=False)
        except OSError:
            fail(code)
    finally:
        os.close(descriptor)
    if (
        len(raw) != before.st_size
        or len(raw) > maximum_bytes
        or d2a_marker_identity(before) != d2a_marker_identity(after)
        or d2a_marker_identity(after) != d2a_marker_identity(named)
    ):
        fail(code)
    return bytes(raw)


def require_audited_manifest_unchanged(context):
    manifest_raw = audited_private_file_bytes(
        context.manifest_path,
        {0o600},
        256 * 1024,
        "audited_recovery_manifest_changed",
    )
    digest_raw = audited_private_file_bytes(
        context.manifest_path.with_name("manifest.sha256"),
        {0o600},
        1024,
        "audited_recovery_manifest_changed",
    )
    if (
        manifest_raw != (canonical_json(context.manifest) + "\n").encode("utf-8")
        or digest_raw != (context.digest + "\n").encode("ascii")
    ):
        fail("audited_recovery_manifest_changed")


def audited_bootstrap_state(context, raw_state_path, quarantined_recovery=False):
    state_path = require_absolute_path(raw_state_path, "audited_recovery_bootstrap_state")
    if state_path.name != f"bootstrap-{context.manifest['run_id']}.json":
        fail("audited_recovery_bootstrap_state_invalid")
    try:
        parent = state_path.parent.lstat()
        resolved = state_path.resolve(strict=True)
    except OSError:
        fail("audited_recovery_bootstrap_state_invalid")
    if (
        resolved != state_path
        or not stat.S_ISDIR(parent.st_mode)
        or state_path.parent.is_symlink()
        or parent.st_uid != os.getuid()
        or stat.S_IMODE(parent.st_mode) != 0o700
    ):
        fail("audited_recovery_bootstrap_state_invalid")
    state = load_strict_d2a_marker(
        state_path,
        "audited_recovery_bootstrap_state_invalid",
        AUDITED_BOOTSTRAP_STATE_FIELDS,
        sorted_canonical=True,
    )
    baseline_quarantined = (
        state.get("status") == "recovery_required"
        and state.get("phase") == "direct_onboard"
    )
    replay_quarantined = (
        state.get("status") == "failed" and state.get("phase") == "complete"
    )
    raw = audited_private_file_bytes(
        state_path, {0o600}, D2A_MARKER_MAXIMUM_BYTES, "audited_recovery_bootstrap_state_invalid"
    )
    if (
        type(state.get("schema_version")) is not int
        or state.get("schema_version") != 1
        or state.get("kind") != "starring.d2a.bootstrap-state.v1"
        or (
            not quarantined_recovery
            and (
                state.get("status") != "recovery_required"
                or state.get("phase") != "cleanup"
            )
        )
        or quarantined_recovery
        and not (baseline_quarantined or replay_quarantined)
        or state.get("operation") != "one-shot"
        or state.get("run_id") != context.manifest["run_id"]
        or state.get("manifest_path") != str(context.manifest_path)
        or state.get("manifest_sha256") != context.digest
        or state.get("source_commit_sha") != context.manifest["commit_sha"]
        or not isinstance(state.get("source_tree_sha"), str)
        or COMMIT_PATTERN.fullmatch(state["source_tree_sha"]) is None
        or state.get("candidate_started") is not quarantined_recovery
        or state.get("discord_teardown_complete")
        is not (False if quarantined_recovery else True)
        or state.get("cleanup_complete") is not False
        or state.get("postconditions_complete") is not False
        or state.get("records") != []
        or state.get("onboarding_evidence_path") is not None
        or state.get("onboarding_evidence_sha256") is not None
        or state.get("last_session_operation") != "direct-onboard"
        or state.get("last_error")
        != ("direct_onboard_failed" if quarantined_recovery else "start_failed")
        or state.get("persistent_sandbox_retained") is not True
        or state.get("release_eligible") is not False
        or not validate_utc_timestamp(state.get("updated_at"))
        or not isinstance(state.get("bootstrap_id"), str)
        or not re.fullmatch(r"d2ab-[0-9a-f]{32}", state["bootstrap_id"])
        or state.get("resource_prefix")
        != context.manifest["discord"]["resource_prefix"]
    ):
        fail("audited_recovery_bootstrap_state_invalid")
    if quarantined_recovery and any(
        state.get(name) is not False
        for name in (
            "discord_teardown_complete",
            "cleanup_complete",
            "postconditions_complete",
        )
    ):
        fail("audited_recovery_bootstrap_state_invalid")
    tool_digests = state.get("tool_digests")
    if (
        not isinstance(tool_digests, dict)
        or any(not valid_d2a_digest(value) for value in tool_digests.values())
        or any(
            field not in tool_digests
            for field in (
                "issuer_sha256",
                "issuer_source_sha256",
                "runner_sha256",
                "product_driver_sha256",
                "scenario_sha256",
            )
        )
    ):
        fail("audited_recovery_bootstrap_state_invalid")
    for field, mode, maximum in (
        ("config_path", 0o600, 256 * 1024),
        ("candidate_spec_path", 0o400, 256 * 1024),
        ("candidate_provenance_path", 0o400, 1024 * 1024),
    ):
        recorded_digest = state.get(field.replace("_path", "_sha256"))
        if not valid_d2a_digest(recorded_digest):
            fail("audited_recovery_bootstrap_state_invalid")
        payload = audited_private_file_bytes(
            pathlib.Path(state[field]),
            {mode},
            maximum,
            "audited_recovery_bootstrap_dependency_invalid",
        )
        if hashlib.sha256(payload).hexdigest() != recorded_digest:
            fail("audited_recovery_bootstrap_dependency_invalid")
    bundle = pathlib.Path(context.manifest["candidates"]["api"]["path"]).parent
    if (
        pathlib.Path(state["candidate_spec_path"]) != bundle / "candidate-spec.json"
        or pathlib.Path(state["candidate_provenance_path"]) != bundle / "provenance.json"
    ):
        fail("audited_recovery_bootstrap_state_invalid")
    return state_path, state, hashlib.sha256(raw).hexdigest()


def audited_quarantined_bootstrap_semantic_sha256(state):
    mutable_completion_fields = {
        "status", "phase", "discord_teardown_complete", "cleanup_complete",
        "postconditions_complete", "updated_at",
    }
    return hashlib.sha256(canonical_json({
        name: value for name, value in state.items()
        if name not in mutable_completion_fields
    }).encode("utf-8")).hexdigest()


def audited_orchestrator_state(context):
    fields = tuple(
        sorted(
            {
                "schema_version",
                "manifest_sha256",
                "run_id",
                "phase",
                "updated_at",
                "standing_snapshot",
            }
        )
    )
    state = load_strict_d2a_marker(
        context.state_path,
        "audited_recovery_orchestrator_state_invalid",
        fields,
        sorted_canonical=True,
    )
    raw = audited_private_file_bytes(
        context.state_path,
        {0o600},
        D2A_MARKER_MAXIMUM_BYTES,
        "audited_recovery_orchestrator_state_invalid",
    )
    if (
        type(state.get("schema_version")) is not int
        or state.get("schema_version") != 1
        or state.get("manifest_sha256") != context.digest
        or state.get("run_id") != context.manifest["run_id"]
        or state.get("phase") not in {"stopped", "cleaned"}
        or not validate_utc_timestamp(state.get("updated_at"))
        or not isinstance(state.get("standing_snapshot"), dict)
    ):
        fail("audited_recovery_orchestrator_state_invalid")
    return state, hashlib.sha256(raw).hexdigest()


def audited_git_executable_sha256():
    for parent in (pathlib.Path("/usr"), pathlib.Path("/usr/bin")):
        try:
            metadata = parent.lstat()
        except OSError:
            fail("audited_recovery_git_invalid")
        if (
            not stat.S_ISDIR(metadata.st_mode)
            or parent.is_symlink()
            or metadata.st_uid != 0
            or stat.S_IMODE(metadata.st_mode) & 0o022
        ):
            fail("audited_recovery_git_invalid")
    try:
        metadata = AUDITED_RECOVERY_GIT_PATH.lstat()
    except OSError:
        fail("audited_recovery_git_invalid")
    if (
        not stat.S_ISREG(metadata.st_mode)
        or AUDITED_RECOVERY_GIT_PATH.is_symlink()
        or metadata.st_uid != 0
        or metadata.st_nlink < 1
        or not stat.S_IMODE(metadata.st_mode) & 0o111
        or stat.S_IMODE(metadata.st_mode) & 0o022
    ):
        fail("audited_recovery_git_invalid")
    return sha256_file(AUDITED_RECOVERY_GIT_PATH)


def audited_git_command(arguments, allow_dirty_exit=False):
    try:
        completed = subprocess.run(
            [str(AUDITED_RECOVERY_GIT_PATH), "-C", str(AUDITED_RECOVERY_REPOSITORY_ROOT), *arguments],
            cwd="/",
            env={
                "GIT_CONFIG_GLOBAL": "/dev/null",
                "GIT_CONFIG_NOSYSTEM": "1",
                "GIT_TERMINAL_PROMPT": "0",
                "GIT_OPTIONAL_LOCKS": "0",
                "HOME": "/",
                "LANG": "C",
                "LC_ALL": "C",
                "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            },
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=10,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        fail("audited_recovery_git_invalid")
    if (
        len(completed.stdout) > 1024 * 1024
        or len(completed.stderr) > 1024 * 1024
        or completed.stderr
        or completed.returncode not in ({0, 1} if allow_dirty_exit else {0})
    ):
        fail("audited_recovery_git_invalid")
    return completed


def current_clean_recovery_source():
    root = AUDITED_RECOVERY_REPOSITORY_ROOT
    try:
        metadata = root.lstat()
        resolved = root.resolve(strict=True)
    except OSError:
        fail("audited_recovery_source_invalid")
    if (
        root != resolved
        or not stat.S_ISDIR(metadata.st_mode)
        or root.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) & 0o022
    ):
        fail("audited_recovery_source_invalid")
    git_sha256 = audited_git_executable_sha256()

    def text(arguments):
        raw = audited_git_command(arguments).stdout
        try:
            value = raw.decode("utf-8").strip()
        except UnicodeDecodeError:
            fail("audited_recovery_git_invalid")
        if not value or "\n" in value:
            fail("audited_recovery_git_invalid")
        return value

    if pathlib.Path(text(["rev-parse", "--show-toplevel"])) != root:
        fail("audited_recovery_source_invalid")
    commit_sha = text(["rev-parse", "--verify", "HEAD"])
    tree_sha = text(["rev-parse", "--verify", "HEAD^{tree}"])
    if (
        COMMIT_PATTERN.fullmatch(commit_sha) is None
        or COMMIT_PATTERN.fullmatch(tree_sha) is None
    ):
        fail("audited_recovery_source_invalid")
    status = audited_git_command(
        ["status", "--porcelain=v1", "--untracked-files=all"]
    )
    unstaged = audited_git_command(
        ["diff", "--no-ext-diff", "--quiet"], allow_dirty_exit=True
    )
    staged = audited_git_command(
        ["diff", "--cached", "--no-ext-diff", "--quiet"],
        allow_dirty_exit=True,
    )
    if status.stdout or unstaged.returncode != 0 or staged.returncode != 0:
        fail("audited_recovery_source_dirty")
    return {
        "repository_root": str(root),
        "commit_sha": commit_sha,
        "tree_sha": tree_sha,
        "git_path": str(AUDITED_RECOVERY_GIT_PATH),
        "git_sha256": git_sha256,
    }


def validate_audited_source_observations(context, observations):
    if not isinstance(observations, dict) or set(observations) != {
        "codex_worker",
        "d2_toolchain",
        "certification_transport",
    }:
        fail("audited_recovery_source_observation_invalid")
    for name, observation in observations.items():
        historical = context.manifest["source_trees"][name]
        if (
            not isinstance(observation, dict)
            or set(observation)
            != {
                "root",
                "historical_sha256",
                "observed_sha256",
                "matches_historical",
            }
            or observation.get("root") != historical["root"]
            or observation.get("historical_sha256") != historical["sha256"]
            or not valid_d2a_digest(observation.get("observed_sha256"))
            or type(observation.get("matches_historical")) is not bool
            or observation["matches_historical"]
            != (observation["observed_sha256"] == historical["sha256"])
        ):
            fail("audited_recovery_source_observation_invalid")
    if not observations["codex_worker"]["matches_historical"]:
        fail("audited_recovery_source_observation_invalid")
    return observations


def audited_recovery_journal(context, baseline_rows, baseline_sha256):
    rows, raw = read_strict_journal_snapshot(context)
    if len(rows) < baseline_rows:
        fail("audited_recovery_journal_invalid")
    lines = raw.splitlines(keepends=True)
    prefix = b"".join(lines[:baseline_rows])
    if hashlib.sha256(prefix).hexdigest() != baseline_sha256:
        fail("audited_recovery_journal_invalid")
    baseline_last = rows[baseline_rows - 1]
    if any(
        baseline_last.get(field) != expected
        for field, expected in (
            ("action", "candidate_start"),
            ("status", "rolled_back"),
            ("target", "run"),
        )
    ):
        fail("audited_recovery_journal_invalid")
    if any(
        row["action"] != "cleanup"
        or row["status"] not in {"intent", "failed", "complete"}
        or row["target"] != "run"
        for row in rows[baseline_rows:]
    ):
        fail("audited_recovery_journal_invalid")
    return rows, raw


def audited_recovery_forbidden_paths(context):
    return (
        candidate_start_transition_path(context),
        candidate_start_source_path(context),
        candidate_start_retirement_path(context),
        context.artifact_directory / "step-03-evidence.json",
        context.artifact_directory / "onboarding-evidence.json",
        context.artifact_directory / "transport-evidence",
        discord_teardown_progress_path(context),
        discord_teardown_progress_path(context, frozen=True),
        discord_teardown_evidence_path(context),
        discord_teardown_evidence_path(context, frozen=True),
        abort_teardown_tombstone_path(context),
        effect_admission_freeze_intent_path(context),
        freeze_intent_path(context),
    )


def require_audited_preissuer_artifact_boundary(context, intent_exists):
    receipts = audited_private_file_bytes(
        context.manifest_path.with_name("receipts.jsonl"),
        {0o600},
        8 * 1024 * 1024,
        "audited_recovery_artifact_invalid",
        allow_empty=True,
    )
    if receipts:
        fail("audited_recovery_artifact_invalid")
    coordinator = context.artifact_directory / "coordinator-sources"
    try:
        metadata = coordinator.lstat()
        entries = {entry.name: entry for entry in coordinator.iterdir()}
    except OSError:
        fail("audited_recovery_artifact_invalid")
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or coordinator.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
        or not set(entries).issubset(
            {"step-01-bootstrap.json", "step-02-prior-absence.json"}
        )
    ):
        fail("audited_recovery_artifact_invalid")
    for path in entries.values():
        audited_private_file_bytes(
            path,
            {0o600},
            D2A_MARKER_MAXIMUM_BYTES,
            "audited_recovery_artifact_invalid",
        )
    try:
        artifact_entries = tuple(context.artifact_directory.iterdir())
    except OSError:
        fail("audited_recovery_artifact_invalid")
    if any(
        entry.name.startswith("step-")
        and entry.name != "step-01-evidence.json"
        for entry in artifact_entries
    ):
        fail("audited_recovery_artifact_invalid")
    if not intent_exists and any(
        os.path.lexists(path)
        for path in (
            cleanup_keychain_baseline_path(context),
            cleanup_root_progress_path(context),
            context.artifact_directory / "cleanup-evidence.json",
            audited_preissuer_rollback_evidence_path(context),
        )
    ):
        fail("audited_recovery_artifact_invalid")


def require_audited_recovery_inert_boundary(context, platform, state, lifecycle):
    if (
        lifecycle.get("origin") != "bootstrap"
        or lifecycle.get("operation") != "direct-onboard"
        or lifecycle.get("status") != "not_issued"
        or lifecycle.get("process_group_id") is not None
        or lifecycle.get("session_revoked") is not False
        or lifecycle.get("revoked_at") is not None
        or lifecycle.get("quarantined_at") is not None
        or candidate_start_commitment_present(context)
        or any(os.path.lexists(path) for path in audited_recovery_forbidden_paths(context))
    ):
        fail("audited_recovery_boundary_invalid")
    try:
        launchd_absent = candidate_launchd_absent(context, platform)
        postgres_absent = cleanup_postgres_absent(context, platform)
        protected_unchanged = (
            standing_snapshot(context, platform) == state["standing_snapshot"]
        )
    except BaseException:
        fail("audited_recovery_boundary_invalid")
    if not launchd_absent or not postgres_absent or not protected_unchanged:
        fail("audited_recovery_boundary_invalid")
    return {
        "launchd_jobs_absent": True,
        "postgres_process_absent": True,
        "protected_staging_unchanged": True,
    }


def close_audited_preissuer_rollback_teardown_fence(
    context, platform, expected_intent, allowlist
):
    """Close the teardown fence only for the allowlisted stopped rollback.

    Normal cleanup deliberately remains limited to the prepared pre-issuer
    sentinel.  This gate is reachable only after the explicit recovery command
    has durably published its exact intent under the global operation lock.
    Every mutable historical boundary is re-read without journal repair before
    the first cleanup-authorizing mutation.
    """
    intent_path = audited_preissuer_rollback_intent_path(context)
    observed_intent = load_strict_d2a_marker(
        intent_path,
        "audited_recovery_intent_invalid",
        AUDITED_PREISSUER_ROLLBACK_INTENT_FIELDS,
        sorted_canonical=True,
    )
    if observed_intent != expected_intent:
        fail("audited_recovery_intent_invalid")
    state, state_sha256 = audited_orchestrator_state(context)
    if (
        state["phase"] != "stopped"
        or state_sha256 != allowlist["orchestrator_state_sha256"]
    ):
        fail("audited_recovery_orchestrator_state_invalid")
    rows, journal_raw = audited_recovery_journal(
        context, allowlist["journal_rows"], allowlist["journal_sha256"]
    )
    if (
        len(rows) != allowlist["journal_rows"]
        or hashlib.sha256(journal_raw).hexdigest() != allowlist["journal_sha256"]
    ):
        fail("audited_recovery_journal_invalid")
    taint_raw = audited_private_file_bytes(
        d2a_taint_path(context),
        {0o600},
        D2A_MARKER_MAXIMUM_BYTES,
        "audited_recovery_taint_invalid",
    )
    lifecycle_raw = audited_private_file_bytes(
        d2a_session_lifecycle_path(context),
        {0o600},
        D2A_MARKER_MAXIMUM_BYTES,
        "audited_recovery_lifecycle_invalid",
    )
    if (
        hashlib.sha256(taint_raw).hexdigest() != allowlist["taint_sha256"]
        or hashlib.sha256(lifecycle_raw).hexdigest()
        != allowlist["lifecycle_sha256"]
    ):
        fail("audited_recovery_replay_drift")
    lifecycle = require_d2a_session_revoked(context)
    require_audited_preissuer_artifact_boundary(context, True)
    require_audited_recovery_inert_boundary(
        context, platform, state, lifecycle
    )
    if os.path.lexists(d2a_teardown_fence_path(context)):
        fail("audited_recovery_fence_invalid")
    transition_d2a_teardown_fence(context, "closed")


def validate_audited_recovery_allowlist(
    context,
    bootstrap_state,
    bootstrap_state_sha256,
    orchestrator_state_sha256,
    observations,
    journal_raw,
    taint_sha256,
    lifecycle_sha256,
):
    entry = AUDITED_PREISSUER_ROLLBACK_ALLOWLIST.get(
        (context.manifest["run_id"], context.digest)
    )
    if entry is None:
        fail("audited_recovery_identity_not_allowlisted")
    actual = {
        "manifest_commit_sha": context.manifest["commit_sha"],
        "historical_d2_toolchain_sha256": observations["d2_toolchain"]["historical_sha256"],
        "historical_transport_sha256": observations["certification_transport"]["historical_sha256"],
        "historical_worker_sha256": observations["codex_worker"]["historical_sha256"],
        "bootstrap_id": bootstrap_state["bootstrap_id"],
        "bootstrap_state_sha256": bootstrap_state_sha256,
        "bootstrap_config_sha256": bootstrap_state["config_sha256"],
        "candidate_spec_sha256": bootstrap_state["candidate_spec_sha256"],
        "candidate_provenance_sha256": bootstrap_state["candidate_provenance_sha256"],
        "candidate_dependency_record_sha256": bootstrap_state["candidate_dependency_record_sha256"],
        "candidate_dependency_tree_sha256": bootstrap_state["candidate_dependency_tree_sha256"],
        "source_tree_sha": bootstrap_state["source_tree_sha"],
        "issuer_sha256": bootstrap_state["tool_digests"]["issuer_sha256"],
        "issuer_source_sha256": bootstrap_state["tool_digests"]["issuer_source_sha256"],
        "orchestrator_state_sha256": orchestrator_state_sha256,
        "journal_sha256": hashlib.sha256(journal_raw).hexdigest(),
        "journal_rows": len(journal_raw.splitlines()),
        "taint_sha256": taint_sha256,
        "lifecycle_sha256": lifecycle_sha256,
    }
    if actual != entry:
        fail("audited_recovery_identity_not_allowlisted")
    return entry


def audited_recovery_current_source(context, observations, revision):
    validate_audited_source_observations(context, observations)
    if (
        not isinstance(revision, dict)
        or set(revision)
        != {"repository_root", "commit_sha", "tree_sha", "git_path", "git_sha256"}
        or revision.get("repository_root") != str(AUDITED_RECOVERY_REPOSITORY_ROOT)
        or revision.get("git_path") != str(AUDITED_RECOVERY_GIT_PATH)
        or COMMIT_PATTERN.fullmatch(revision.get("commit_sha", "")) is None
        or COMMIT_PATTERN.fullmatch(revision.get("tree_sha", "")) is None
        or not valid_d2a_digest(revision.get("git_sha256"))
    ):
        fail("audited_recovery_source_invalid")
    if revision["commit_sha"] == context.manifest["commit_sha"] or not any(
        not observations[name]["matches_historical"]
        for name in ("d2_toolchain", "certification_transport")
    ):
        fail("audited_recovery_source_not_changed")
    return {**revision, "source_trees": observations}


def audited_recovery_intent(
    context,
    bootstrap_state_path,
    bootstrap_state,
    bootstrap_state_sha256,
    source_record,
    allowlist,
    created_at,
):
    return {
        "schema_version": 1,
        "kind": AUDITED_PREISSUER_ROLLBACK_INTENT_KIND,
        "run_id": context.manifest["run_id"],
        "manifest_sha256": context.digest,
        "bootstrap_id": bootstrap_state["bootstrap_id"],
        "bootstrap_state_path": str(bootstrap_state_path),
        "bootstrap_state_sha256": bootstrap_state_sha256,
        "historical_manifest_commit_sha": context.manifest["commit_sha"],
        "historical_source_trees": {
            name: context.manifest["source_trees"][name]["sha256"]
            for name in sorted(context.manifest["source_trees"])
        },
        "current_source": source_record,
        "orchestrator_state_sha256": allowlist["orchestrator_state_sha256"],
        "baseline_journal_sha256": allowlist["journal_sha256"],
        "baseline_journal_rows": allowlist["journal_rows"],
        "taint_sha256": allowlist["taint_sha256"],
        "lifecycle_sha256": allowlist["lifecycle_sha256"],
        "created_at": created_at,
    }


def load_or_create_audited_recovery_intent(context, expected):
    path = audited_preissuer_rollback_intent_path(context)
    if os.path.lexists(path):
        observed = load_strict_d2a_marker(
            path,
            "audited_recovery_intent_invalid",
            AUDITED_PREISSUER_ROLLBACK_INTENT_FIELDS,
            sorted_canonical=True,
        )
        if not validate_utc_timestamp(observed.get("created_at")):
            fail("audited_recovery_intent_invalid")
        replay_expected = {**expected, "created_at": observed["created_at"]}
        if observed != replay_expected:
            fail("audited_recovery_intent_invalid")
        return path, observed
    try:
        write_new_file(path, canonical_json(expected) + "\n")
        fsync_directory(context.artifact_directory, "audited_recovery_intent_parent")
    except (OSError, CertificationError):
        fail("audited_recovery_intent_write_failed")
    observed = load_strict_d2a_marker(
        path,
        "audited_recovery_intent_invalid",
        AUDITED_PREISSUER_ROLLBACK_INTENT_FIELDS,
        sorted_canonical=True,
    )
    if observed != expected:
        fail("audited_recovery_intent_invalid")
    return path, observed


def audited_recovery_evidence(
    context, intent_sha256, fence_sha256, cleanup_evidence_sha256, observed_at
):
    return {
        "schema_version": 1,
        "kind": AUDITED_PREISSUER_ROLLBACK_EVIDENCE_KIND,
        "run_id": context.manifest["run_id"],
        "manifest_sha256": context.digest,
        "intent_sha256": intent_sha256,
        "observed_at": observed_at,
        "database_absent": True,
        "postgres_process_absent": True,
        "launchd_jobs_absent": True,
        "keychain_items_absent": True,
        "isolated_root_absent": True,
        "protected_staging_unchanged": True,
        "teardown_fence_sha256": fence_sha256,
        "cleanup_evidence_sha256": cleanup_evidence_sha256,
    }


def validate_audited_recovery_evidence(context, evidence, expected):
    if (
        not isinstance(evidence, dict)
        or tuple(sorted(evidence)) != AUDITED_PREISSUER_ROLLBACK_EVIDENCE_FIELDS
        or type(evidence.get("schema_version")) is not int
        or evidence.get("schema_version") != 1
        or evidence.get("kind") != AUDITED_PREISSUER_ROLLBACK_EVIDENCE_KIND
        or not validate_utc_timestamp(evidence.get("observed_at"))
        or evidence != {**expected, "observed_at": evidence["observed_at"]}
    ):
        fail("audited_recovery_evidence_invalid")
    return evidence


def command_recover_audited_preissuer_rollback(
    context,
    platform,
    initial_observations,
    bootstrap_state_path,
    confirmed_current_commit,
    confirmed_current_tree,
    confirmed_run_id,
    confirmed_manifest_sha256,
):
    if (
        COMMIT_PATTERN.fullmatch(confirmed_current_commit or "") is None
        or COMMIT_PATTERN.fullmatch(confirmed_current_tree or "") is None
        or confirmed_run_id != context.manifest["run_id"]
        or confirmed_manifest_sha256 != context.digest
    ):
        fail("audited_recovery_confirmation_mismatch")
    require_audited_manifest_unchanged(context)
    observations = validate_audited_source_observations(
        context, initial_observations
    )
    revision = current_clean_recovery_source()
    if (
        revision["commit_sha"] != confirmed_current_commit
        or revision["tree_sha"] != confirmed_current_tree
    ):
        fail("audited_recovery_confirmation_mismatch")
    source_record = audited_recovery_current_source(
        context, observations, revision
    )
    bootstrap_path, bootstrap_state, bootstrap_sha256 = audited_bootstrap_state(
        context, bootstrap_state_path
    )
    state, state_sha256 = audited_orchestrator_state(context)
    rows, journal_raw = read_strict_journal_snapshot(context)
    taint_raw = audited_private_file_bytes(
        d2a_taint_path(context),
        {0o600},
        D2A_MARKER_MAXIMUM_BYTES,
        "audited_recovery_taint_invalid",
    )
    lifecycle_raw = audited_private_file_bytes(
        d2a_session_lifecycle_path(context),
        {0o600},
        D2A_MARKER_MAXIMUM_BYTES,
        "audited_recovery_lifecycle_invalid",
    )
    taint_sha256 = hashlib.sha256(taint_raw).hexdigest()
    lifecycle_sha256 = hashlib.sha256(lifecycle_raw).hexdigest()
    intent_path = audited_preissuer_rollback_intent_path(context)
    evidence_path = audited_preissuer_rollback_evidence_path(context)
    intent_exists = os.path.lexists(intent_path)
    allowlist = AUDITED_PREISSUER_ROLLBACK_ALLOWLIST.get(
        (context.manifest["run_id"], context.digest)
    )
    if allowlist is None:
        fail("audited_recovery_identity_not_allowlisted")
    if intent_exists:
        audited_recovery_journal(
            context, allowlist["journal_rows"], allowlist["journal_sha256"]
        )
        if state["phase"] == "stopped" and state_sha256 != allowlist["orchestrator_state_sha256"]:
            fail("audited_recovery_orchestrator_state_invalid")
        if (
            bootstrap_sha256 != allowlist["bootstrap_state_sha256"]
            or taint_sha256 != allowlist["taint_sha256"]
            or lifecycle_sha256 != allowlist["lifecycle_sha256"]
        ):
            fail("audited_recovery_replay_drift")
    else:
        if state["phase"] != "stopped":
            fail("audited_recovery_boundary_invalid")
        allowlist = validate_audited_recovery_allowlist(
            context,
            bootstrap_state,
            bootstrap_sha256,
            state_sha256,
            observations,
            journal_raw,
            taint_sha256,
            lifecycle_sha256,
        )
        if os.path.lexists(d2a_teardown_fence_path(context)) or os.path.lexists(evidence_path):
            fail("audited_recovery_boundary_invalid")
    rows, _journal_raw = audited_recovery_journal(
        context, allowlist["journal_rows"], allowlist["journal_sha256"]
    )
    if not intent_exists and len(rows) != allowlist["journal_rows"]:
        fail("audited_recovery_journal_invalid")
    lifecycle = require_d2a_session_revoked(context)
    require_audited_preissuer_artifact_boundary(context, intent_exists)
    require_audited_recovery_inert_boundary(context, platform, state, lifecycle)
    if os.path.lexists(d2a_teardown_fence_path(context)):
        fence = validate_d2a_teardown_fence(
            context,
            load_strict_d2a_marker(
                d2a_teardown_fence_path(context),
                "audited_recovery_fence_invalid",
                D2A_TEARDOWN_FENCE_FIELDS,
                sorted_canonical=True,
            ),
        )
        if not intent_exists or fence["status"] != "closed":
            fail("audited_recovery_fence_invalid")
    # Re-read every mutable source input immediately before the first durable
    # recovery mutation.  The working tree must still be the exact confirmed
    # clean commit/tree, and candidate/source observations must be unchanged.
    second_revision = current_clean_recovery_source()
    second_observations = validate_audited_source_observations(
        context, observe_audited_recovery_source_trees(context.manifest)
    )
    if second_revision != revision or second_observations != observations:
        fail("audited_recovery_source_changed")
    require_audited_manifest_unchanged(context)
    expected_intent = audited_recovery_intent(
        context,
        bootstrap_path,
        bootstrap_state,
        bootstrap_sha256,
        source_record,
        allowlist,
        utc_now(),
    )
    intent_path, intent = load_or_create_audited_recovery_intent(
        context, expected_intent
    )
    intent_raw = audited_private_file_bytes(
        intent_path,
        {0o600},
        D2A_MARKER_MAXIMUM_BYTES,
        "audited_recovery_intent_invalid",
    )
    intent_sha256 = hashlib.sha256(intent_raw).hexdigest()
    if os.path.lexists(evidence_path):
        if state["phase"] != "cleaned" or not os.path.lexists(
            d2a_teardown_fence_path(context)
        ):
            fail("audited_recovery_evidence_invalid")
        cleanup_evidence_path = context.artifact_directory / "cleanup-evidence.json"
        validate_cleanup_evidence(
            context,
            load_json(cleanup_evidence_path, "cleanup_evidence_invalid"),
        )
        absence = cleanup_absence(
            context, platform, state["standing_snapshot"]
        )
        if not all(absence.values()):
            fail("audited_recovery_incomplete")
        fence_raw = audited_private_file_bytes(
            d2a_teardown_fence_path(context),
            {0o600},
            D2A_MARKER_MAXIMUM_BYTES,
            "audited_recovery_fence_invalid",
        )
        cleanup_evidence_raw = audited_private_file_bytes(
            cleanup_evidence_path,
            {0o600},
            D2A_MARKER_MAXIMUM_BYTES,
            "audited_recovery_evidence_invalid",
        )
        expected_evidence = audited_recovery_evidence(
            context,
            intent_sha256,
            hashlib.sha256(fence_raw).hexdigest(),
            hashlib.sha256(cleanup_evidence_raw).hexdigest(),
            utc_now(),
        )
        evidence = load_strict_d2a_marker(
            evidence_path,
            "audited_recovery_evidence_invalid",
            AUDITED_PREISSUER_ROLLBACK_EVIDENCE_FIELDS,
            sorted_canonical=True,
        )
        validate_audited_recovery_evidence(context, evidence, expected_evidence)
        final_revision = current_clean_recovery_source()
        final_observations = validate_audited_source_observations(
            context, observe_audited_recovery_source_trees(context.manifest)
        )
        if final_revision != revision or final_observations != observations:
            fail("audited_recovery_source_changed")
        require_audited_manifest_unchanged(context)
        return {
            "status": "exact_replay",
            "phase": "cleaned",
            "run_id": context.manifest["run_id"],
            "manifest_sha256": context.digest,
            "intent": str(intent_path),
            "evidence": str(evidence_path),
            **absence,
            "source_drift_observed": True,
            "cleanup_status": "already_cleaned",
        }
    if not os.path.lexists(d2a_teardown_fence_path(context)):
        # The intent is the first durable recovery mutation.  Re-prove the
        # current clean source and historical immutable observations after it,
        # then use the audited-only stopped-rollback fence gate.  General
        # cleanup never accepts a stopped bootstrap sentinel.
        third_revision = current_clean_recovery_source()
        third_observations = validate_audited_source_observations(
            context, observe_audited_recovery_source_trees(context.manifest)
        )
        if third_revision != revision or third_observations != observations:
            fail("audited_recovery_source_changed")
        require_audited_manifest_unchanged(context)
        close_audited_preissuer_rollback_teardown_fence(
            context, platform, intent, allowlist
        )
    require_d2a_cleanup_fence(context, platform)
    cleanup_result = command_cleanup_internal(
        context, platform, retire_committed=False
    )
    validate_cleanup_evidence(
        context,
        load_json(
            context.artifact_directory / "cleanup-evidence.json",
            "cleanup_evidence_invalid",
        ),
    )
    final_state = load_state(context, {"cleaned"})
    absence = cleanup_absence(context, platform, final_state["standing_snapshot"])
    if not all(absence.values()):
        fail("audited_recovery_incomplete")
    final_revision = current_clean_recovery_source()
    final_observations = validate_audited_source_observations(
        context, observe_audited_recovery_source_trees(context.manifest)
    )
    if final_revision != revision or final_observations != observations:
        fail("audited_recovery_source_changed")
    require_audited_manifest_unchanged(context)
    fence_raw = audited_private_file_bytes(
        d2a_teardown_fence_path(context),
        {0o600},
        D2A_MARKER_MAXIMUM_BYTES,
        "audited_recovery_fence_invalid",
    )
    cleanup_evidence_raw = audited_private_file_bytes(
        context.artifact_directory / "cleanup-evidence.json",
        {0o600},
        D2A_MARKER_MAXIMUM_BYTES,
        "audited_recovery_evidence_invalid",
    )
    expected_evidence = audited_recovery_evidence(
        context,
        intent_sha256,
        hashlib.sha256(fence_raw).hexdigest(),
        hashlib.sha256(cleanup_evidence_raw).hexdigest(),
        utc_now(),
    )
    write_atomic(evidence_path, canonical_json(expected_evidence) + "\n")
    evidence = load_strict_d2a_marker(
        evidence_path,
        "audited_recovery_evidence_invalid",
        AUDITED_PREISSUER_ROLLBACK_EVIDENCE_FIELDS,
        sorted_canonical=True,
    )
    if evidence != expected_evidence:
        fail("audited_recovery_evidence_invalid")
    return {
        "status": "recovered",
        "phase": "cleaned",
        "run_id": context.manifest["run_id"],
        "manifest_sha256": context.digest,
        "intent": str(intent_path),
        "evidence": str(evidence_path),
        **absence,
        "source_drift_observed": True,
        "cleanup_status": cleanup_result["status"],
    }


def audited_quarantined_recovery_paths(context):
    return {
        "intent": context.artifact_directory
        / "audited-quarantined-no-issue-recovery-intent.json",
        "source_transition_interlock": context.run_directory
        / "coordinator"
        / "audited-quarantined-no-issue-recovery-source-transition-interlock.json",
        "source_transition": context.artifact_directory
        / "audited-quarantined-no-issue-recovery-source-transition.json",
        "source_transition_v2_interlock": context.run_directory
        / "coordinator"
        / (
            "audited-quarantined-no-issue-recovery-source-transition-"
            "v2-interlock.json"
        ),
        "source_transition_v2": context.artifact_directory
        / "audited-quarantined-no-issue-recovery-source-transition-v2.json",
        "cleanup_transition_interlock": context.run_directory
        / "coordinator"
        / (
            "audited-quarantined-no-issue-recovery-cleanup-transition-"
            "interlock.json"
        ),
        "cleanup_transition": context.artifact_directory
        / "audited-quarantined-no-issue-recovery-cleanup-transition.json",
        "database_absence": context.artifact_directory
        / "audited-quarantined-no-issue-database-absence.json",
        "reconciliation": context.artifact_directory
        / "audited-quarantined-no-issue-reconciliation.json",
        "evidence": context.artifact_directory
        / "audited-quarantined-no-issue-recovery.json",
    }


def audited_quarantined_source_transition_git_facts(
    revision,
    parent_commit=AUDITED_QUARANTINED_RECOVERY_FROM_COMMIT,
    from_file_sha256=AUDITED_QUARANTINED_RECOVERY_FROM_FILE_SHA256,
):
    def lines(arguments, code):
        raw = audited_git_command(arguments).stdout
        try:
            values = raw.decode("utf-8").splitlines()
        except UnicodeDecodeError:
            fail(code)
        if not values or any(not value or "\r" in value for value in values):
            fail(code)
        return values

    parents = lines(
        ["rev-list", "--parents", "-n", "1", "HEAD"],
        "audited_quarantined_source_parent_invalid",
    )
    if len(parents) != 1:
        fail("audited_quarantined_source_parent_invalid")
    parent_fields = parents[0].split(" ")
    if (
        len(parent_fields) != 2
        or parent_fields[0] != revision["commit_sha"]
        or parent_fields[1] != parent_commit
    ):
        fail("audited_quarantined_source_parent_invalid")
    changed_paths = lines(
        [
            "diff-tree", "--no-commit-id", "--name-only", "-r",
            parent_commit,
            revision["commit_sha"],
        ],
        "audited_quarantined_source_diff_invalid",
    )
    if changed_paths != list(AUDITED_QUARANTINED_RECOVERY_CHANGED_PATHS):
        fail("audited_quarantined_source_diff_invalid")
    file_sha256 = {}
    for relative_path in AUDITED_QUARANTINED_RECOVERY_CHANGED_PATHS:
        path = AUDITED_RECOVERY_REPOSITORY_ROOT / relative_path
        observed = sha256_file(path)
        committed = audited_git_command([
            "show", f"{revision['commit_sha']}:{relative_path}",
        ]).stdout
        committed_sha256 = hashlib.sha256(committed).hexdigest()
        if observed != committed_sha256:
            fail("audited_quarantined_source_file_invalid")
        historical = audited_git_command([
            "show", f"{parent_commit}:{relative_path}",
        ]).stdout
        from_sha256 = hashlib.sha256(historical).hexdigest()
        if from_sha256 != from_file_sha256[relative_path]:
            fail("audited_quarantined_source_file_invalid")
        file_sha256[relative_path] = {
            "from_sha256": from_sha256, "to_sha256": committed_sha256,
        }
    return {
        "parent_commit_sha": parent_commit,
        "parent_count": 1,
        "changed_paths": list(AUDITED_QUARANTINED_RECOVERY_CHANGED_PATHS),
        "file_sha256": file_sha256,
    }


def audited_quarantined_source_transition_boundary(
    context, platform, allowlist, paths, state
):
    fence_raw = audited_private_file_bytes(
        d2a_teardown_fence_path(context), {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_source_transition_boundary_invalid",
    )
    if (
        hashlib.sha256(fence_raw).hexdigest()
        != AUDITED_QUARANTINED_RECOVERY_CLOSING_FENCE_SHA256
        or any(os.path.lexists(paths[name]) for name in (
            "database_absence", "reconciliation", "evidence",
        ))
        or any(not platform.launchd_absent(
            context.manifest["services"][name]["label"]
        ) for name in ("tunnel", "runtime", "api", "worker"))
        or not platform.launchd_overrides_absent(candidate_launchd_labels(context))
        or not audited_quarantined_service_identity(
            context, platform, "transport", allowlist
        )
        or not platform.postgres_running(context.cluster_root)
        or standing_snapshot(context, platform) != state["standing_snapshot"]
    ):
        fail("audited_quarantined_source_transition_boundary_invalid")
    for name in ("tunnel", "runtime", "api", "worker"):
        require_audited_process_group_absent(
            AUDITED_QUARANTINED_SERVICE_IDENTITIES[name]["process_group_id"],
            "audited_quarantined_source_transition_boundary_invalid",
        )
    require_audited_quarantined_lifecycle(context, allowlist)
    snapshot = platform.transport_control(context, "snapshot")
    effect = snapshot.get("effect_http", {})
    inventory = require_empty_audited_transport_inventory(
        context, platform, allowlist
    )
    if (
        effect.get("phase") != "draining"
        or effect.get("operation_id") != AUDITED_QUARANTINED_NO_ISSUE_OPERATION_ID
        or effect.get("active_requests") != 0
        or effect.get("uncertain_requests") != 0
        or effect.get("accepted_requests") != effect.get("completed_requests")
    ):
        fail("audited_quarantined_source_transition_boundary_invalid")
    return {
        "teardown_fence_sha256": hashlib.sha256(fence_raw).hexdigest(),
        "transport_instance_id": allowlist["transport_instance_id"],
        "transport_inventory_sha256": inventory["digest_sha256"],
        "effect_admission_operation_id": AUDITED_QUARANTINED_NO_ISSUE_OPERATION_ID,
        "producer_launchd_jobs_absent": True,
        "issuer_process_group_absent": True,
        "transport_identity_verified": True,
        "transport_effect_admission_drained": True,
        "postgres_running": True,
        "protected_staging_unchanged": True,
        "database_absence_marker_absent": True,
        "reconciliation_marker_absent": True,
        "recovery_evidence_absent": True,
    }


def audited_quarantined_marker(path, fields, code):
    return load_strict_d2a_marker(
        path, code, fields, sorted_canonical=True
    )


def audited_write_once_marker(path, expected, fields, code):
    if os.path.lexists(path):
        observed = audited_quarantined_marker(path, fields, code)
        timestamp = "created_at" if "created_at" in expected else "observed_at"
        if not validate_utc_timestamp(observed.get(timestamp)):
            fail(code)
        if observed != {**expected, timestamp: observed[timestamp]}:
            fail(code)
        return observed
    try:
        write_new_file(path, canonical_json(expected) + "\n")
        fsync_directory(path.parent, f"{code}_parent")
    except (OSError, CertificationError):
        fail(f"{code}_write_failed")
    observed = audited_quarantined_marker(path, fields, code)
    if observed != expected:
        fail(code)
    return observed


def audited_quarantined_load_original_intent(context, paths):
    raw = audited_private_file_bytes(
        paths["intent"], {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_intent_invalid",
    )
    if hashlib.sha256(raw).hexdigest() != AUDITED_QUARANTINED_RECOVERY_INTENT_SHA256:
        fail("audited_quarantined_intent_invalid")
    intent = audited_quarantined_marker(
        paths["intent"], AUDITED_QUARANTINED_INTENT_FIELDS,
        "audited_quarantined_intent_invalid",
    )
    source = intent.get("current_source")
    if (
        intent.get("kind") != AUDITED_QUARANTINED_NO_ISSUE_INTENT_KIND
        or intent.get("run_id") != context.manifest["run_id"]
        or intent.get("manifest_sha256") != context.digest
        or not isinstance(source, dict)
        or source.get("commit_sha") != AUDITED_QUARANTINED_RECOVERY_FROM_COMMIT
        or source.get("tree_sha") != AUDITED_QUARANTINED_RECOVERY_FROM_TREE
    ):
        fail("audited_quarantined_intent_invalid")
    return intent, raw


def audited_quarantined_v1_audit_configuration():
    return {
        "login_keychain_path": AUDITED_QUARANTINED_LOGIN_KEYCHAIN_PATH,
        "login_keychain_policy_kind": AUDITED_QUARANTINED_LOGIN_KEYCHAIN_POLICY_KIND,
        "login_keychain_policy_sha256": AUDITED_QUARANTINED_LOGIN_KEYCHAIN_POLICY_SHA256,
        "login_keychain_policy_verified": True,
    }


def audited_quarantined_v2_audit_configuration():
    return {
        **audited_quarantined_v1_audit_configuration(),
        "static_sql_sha256": AUDITED_QUARANTINED_RECOVERY_V2_STATIC_SQL_SHA256,
    }


def validate_audited_quarantined_v1_source_transition(
    context, paths, intent, intent_raw, bootstrap_state, allowlist
):
    interlock = audited_quarantined_marker(
        paths["source_transition_interlock"],
        AUDITED_QUARANTINED_SOURCE_TRANSITION_BASE_FIELDS,
        "audited_quarantined_v1_source_transition_interlock_invalid",
    )
    interlock_raw = audited_private_file_bytes(
        paths["source_transition_interlock"], {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_v1_source_transition_interlock_invalid",
    )
    transition = audited_quarantined_marker(
        paths["source_transition"], AUDITED_QUARANTINED_SOURCE_TRANSITION_FIELDS,
        "audited_quarantined_v1_source_transition_invalid",
    )
    transition_raw = audited_private_file_bytes(
        paths["source_transition"], {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_v1_source_transition_invalid",
    )
    expected_interlock = {
        name: transition[name]
        for name in AUDITED_QUARANTINED_SOURCE_TRANSITION_BASE_FIELDS
    }
    expected_interlock["kind"] = (
        AUDITED_QUARANTINED_SOURCE_TRANSITION_INTERLOCK_KIND
    )
    file_sha256 = transition.get("file_sha256")
    if (
        hashlib.sha256(interlock_raw).hexdigest()
        != AUDITED_QUARANTINED_RECOVERY_V1_INTERLOCK_SHA256
        or hashlib.sha256(transition_raw).hexdigest()
        != AUDITED_QUARANTINED_RECOVERY_V1_TRANSITION_SHA256
        or interlock != expected_interlock
        or transition.get("interlock_sha256")
        != AUDITED_QUARANTINED_RECOVERY_V1_INTERLOCK_SHA256
        or transition.get("schema_version") != 1
        or transition.get("kind") != AUDITED_QUARANTINED_SOURCE_TRANSITION_KIND
        or transition.get("run_id") != context.manifest["run_id"]
        or transition.get("manifest_sha256") != context.digest
        or transition.get("intent_sha256") != hashlib.sha256(intent_raw).hexdigest()
        or transition.get("from_source") != intent["current_source"]
        or not isinstance(transition.get("to_source"), dict)
        or transition["to_source"].get("commit_sha")
        != AUDITED_QUARANTINED_RECOVERY_V1_TO_COMMIT
        or transition["to_source"].get("tree_sha")
        != AUDITED_QUARANTINED_RECOVERY_V1_TO_TREE
        or transition.get("parent_commit_sha")
        != AUDITED_QUARANTINED_RECOVERY_FROM_COMMIT
        or transition.get("parent_count") != 1
        or transition.get("changed_paths")
        != list(AUDITED_QUARANTINED_RECOVERY_CHANGED_PATHS)
        or not isinstance(file_sha256, dict)
        or set(file_sha256) != set(AUDITED_QUARANTINED_RECOVERY_CHANGED_PATHS)
        or any(
            file_sha256[path]
            != {
                "from_sha256": AUDITED_QUARANTINED_RECOVERY_FROM_FILE_SHA256[path],
                "to_sha256": AUDITED_QUARANTINED_RECOVERY_V1_TO_FILE_SHA256[path],
            }
            for path in AUDITED_QUARANTINED_RECOVERY_CHANGED_PATHS
        )
        or transition.get("reason_codes")
        != list(AUDITED_QUARANTINED_RECOVERY_REASON_CODES)
        or transition.get("audit_configuration")
        != audited_quarantined_v1_audit_configuration()
        or transition.get("bootstrap_state_semantic_sha256")
        != audited_quarantined_bootstrap_semantic_sha256(bootstrap_state)
        or transition.get("orchestrator_state_sha256")
        != allowlist["orchestrator_state_sha256"]
        or transition.get("baseline_journal_sha256") != allowlist["journal_sha256"]
        or transition.get("baseline_journal_rows") != allowlist["journal_rows"]
        or transition.get("lifecycle_sha256") != allowlist["lifecycle_sha256"]
        or transition.get("teardown_fence_sha256")
        != AUDITED_QUARANTINED_RECOVERY_CLOSING_FENCE_SHA256
        or transition.get("transport_instance_id") != allowlist["transport_instance_id"]
        or transition.get("transport_inventory_sha256")
        != allowlist["empty_transport_inventory_sha256"]
        or transition.get("effect_admission_operation_id")
        != AUDITED_QUARANTINED_NO_ISSUE_OPERATION_ID
        or not validate_utc_timestamp(transition.get("created_at"))
        or any(transition.get(name) is not True for name in (
            "producer_launchd_jobs_absent", "issuer_process_group_absent",
            "transport_identity_verified", "transport_effect_admission_drained",
            "postgres_running", "protected_staging_unchanged",
            "database_absence_marker_absent", "reconciliation_marker_absent",
            "recovery_evidence_absent",
        ))
    ):
        fail("audited_quarantined_v1_source_transition_invalid")
    return transition, hashlib.sha256(transition_raw).hexdigest()


def audited_quarantined_source_transition(
    context, platform, allowlist, paths, intent, intent_raw, source_record,
    revision, bootstrap_state, state, boundary
):
    interlock_path = paths["source_transition_interlock"]
    transition_path = paths["source_transition"]
    git_facts = audited_quarantined_source_transition_git_facts(revision)
    expected_audit_configuration = {
        "login_keychain_path": AUDITED_QUARANTINED_LOGIN_KEYCHAIN_PATH,
        "login_keychain_policy_kind": AUDITED_QUARANTINED_LOGIN_KEYCHAIN_POLICY_KIND,
        "login_keychain_policy_sha256": AUDITED_QUARANTINED_LOGIN_KEYCHAIN_POLICY_SHA256,
        "login_keychain_policy_verified": True,
    }
    if (
        platform.quarantined_recovery_login_keychain_policy()
        != expected_audit_configuration
    ):
        fail("audited_quarantined_source_transition_audit_policy_invalid")
    base = {
        "schema_version": 1,
        "kind": AUDITED_QUARANTINED_SOURCE_TRANSITION_INTERLOCK_KIND,
        "run_id": context.manifest["run_id"],
        "manifest_sha256": context.digest,
        "intent_sha256": hashlib.sha256(intent_raw).hexdigest(),
        "from_source": intent["current_source"],
        "to_source": source_record,
        **git_facts,
        "reason_codes": list(AUDITED_QUARANTINED_RECOVERY_REASON_CODES),
        "audit_configuration": expected_audit_configuration,
        "bootstrap_state_semantic_sha256": (
            audited_quarantined_bootstrap_semantic_sha256(bootstrap_state)
        ),
        "orchestrator_state_sha256": allowlist["orchestrator_state_sha256"],
        "baseline_journal_sha256": allowlist["journal_sha256"],
        "baseline_journal_rows": allowlist["journal_rows"],
        "lifecycle_sha256": allowlist["lifecycle_sha256"],
        **boundary,
        "created_at": utc_now(),
    }
    interlock = audited_write_once_marker(
        interlock_path, base, AUDITED_QUARANTINED_SOURCE_TRANSITION_BASE_FIELDS,
        "audited_quarantined_source_transition_interlock_invalid",
    )
    interlock_raw = audited_private_file_bytes(
        interlock_path, {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_source_transition_interlock_invalid",
    )
    second_revision = current_clean_recovery_source()
    second_observations = validate_audited_source_observations(
        context, observe_audited_recovery_source_trees(context.manifest)
    )
    second_source_record = audited_recovery_current_source(
        context, second_observations, second_revision
    )
    second_boundary = audited_quarantined_source_transition_boundary(
        context, platform, allowlist, paths, state
    )
    if (
        second_revision != revision
        or second_source_record != source_record
        or second_boundary != boundary
        or audited_quarantined_source_transition_git_facts(second_revision)
        != git_facts
        or platform.quarantined_recovery_login_keychain_policy()
        != expected_audit_configuration
    ):
        fail("audited_quarantined_source_transition_changed")
    expected_transition = {
        **interlock,
        "kind": AUDITED_QUARANTINED_SOURCE_TRANSITION_KIND,
        "interlock_sha256": hashlib.sha256(interlock_raw).hexdigest(),
    }
    transition = audited_write_once_marker(
        transition_path, expected_transition,
        AUDITED_QUARANTINED_SOURCE_TRANSITION_FIELDS,
        "audited_quarantined_source_transition_invalid",
    )
    transition_raw = audited_private_file_bytes(
        transition_path, {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_source_transition_invalid",
    )
    return validate_audited_quarantined_source_transition(
        context, paths, intent, intent_raw, source_record, revision,
        bootstrap_state, allowlist
    )


def validate_audited_quarantined_source_transition(
    context, paths, intent, intent_raw, source_record, revision,
    bootstrap_state, allowlist
):
    interlock = audited_quarantined_marker(
        paths["source_transition_interlock"],
        AUDITED_QUARANTINED_SOURCE_TRANSITION_BASE_FIELDS,
        "audited_quarantined_source_transition_interlock_invalid",
    )
    interlock_raw = audited_private_file_bytes(
        paths["source_transition_interlock"], {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_source_transition_interlock_invalid",
    )
    transition = audited_quarantined_marker(
        paths["source_transition"], AUDITED_QUARANTINED_SOURCE_TRANSITION_FIELDS,
        "audited_quarantined_source_transition_invalid",
    )
    transition_raw = audited_private_file_bytes(
        paths["source_transition"], {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_source_transition_invalid",
    )
    expected_interlock = {
        name: transition[name]
        for name in AUDITED_QUARANTINED_SOURCE_TRANSITION_BASE_FIELDS
    }
    expected_interlock["kind"] = (
        AUDITED_QUARANTINED_SOURCE_TRANSITION_INTERLOCK_KIND
    )
    if (
        interlock != expected_interlock
        or transition.get("interlock_sha256")
        != hashlib.sha256(interlock_raw).hexdigest()
        or transition.get("intent_sha256") != hashlib.sha256(intent_raw).hexdigest()
        or transition.get("from_source") != intent["current_source"]
        or transition.get("to_source") != source_record
        or transition.get("reason_codes")
        != list(AUDITED_QUARANTINED_RECOVERY_REASON_CODES)
        or transition.get("audit_configuration")
        != {
            "login_keychain_path": AUDITED_QUARANTINED_LOGIN_KEYCHAIN_PATH,
            "login_keychain_policy_kind": AUDITED_QUARANTINED_LOGIN_KEYCHAIN_POLICY_KIND,
            "login_keychain_policy_sha256": AUDITED_QUARANTINED_LOGIN_KEYCHAIN_POLICY_SHA256,
            "login_keychain_policy_verified": True,
        }
        or transition.get("schema_version") != 1
        or transition.get("kind") != AUDITED_QUARANTINED_SOURCE_TRANSITION_KIND
        or transition.get("run_id") != context.manifest["run_id"]
        or transition.get("manifest_sha256") != context.digest
        or transition.get("bootstrap_state_semantic_sha256")
        != audited_quarantined_bootstrap_semantic_sha256(bootstrap_state)
        or transition.get("orchestrator_state_sha256")
        != allowlist["orchestrator_state_sha256"]
        or transition.get("baseline_journal_sha256") != allowlist["journal_sha256"]
        or transition.get("baseline_journal_rows") != allowlist["journal_rows"]
        or transition.get("lifecycle_sha256") != allowlist["lifecycle_sha256"]
        or transition.get("teardown_fence_sha256")
        != AUDITED_QUARANTINED_RECOVERY_CLOSING_FENCE_SHA256
        or transition.get("transport_instance_id") != allowlist["transport_instance_id"]
        or transition.get("transport_inventory_sha256")
        != allowlist["empty_transport_inventory_sha256"]
        or transition.get("effect_admission_operation_id")
        != AUDITED_QUARANTINED_NO_ISSUE_OPERATION_ID
        or not validate_utc_timestamp(transition.get("created_at"))
        or any(transition.get(name) is not True for name in (
            "producer_launchd_jobs_absent", "issuer_process_group_absent",
            "transport_identity_verified", "transport_effect_admission_drained",
            "postgres_running", "protected_staging_unchanged",
            "database_absence_marker_absent", "reconciliation_marker_absent",
            "recovery_evidence_absent",
        ))
    ):
        fail("audited_quarantined_source_transition_invalid")
    facts = audited_quarantined_source_transition_git_facts(revision)
    if any(transition.get(name) != value for name, value in facts.items()):
        fail("audited_quarantined_source_transition_invalid")
    return transition, hashlib.sha256(transition_raw).hexdigest()


def require_audited_quarantined_source_transition_baseline(
    context, bootstrap_path, allowlist
):
    bootstrap_raw = audited_private_file_bytes(
        bootstrap_path, {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_source_transition_baseline_invalid",
    )
    state, state_sha256 = audited_quarantined_state(context)
    rows, journal_raw = audited_quarantined_journal(context, allowlist)
    if (
        hashlib.sha256(bootstrap_raw).hexdigest()
        != allowlist["bootstrap_state_sha256"]
        or state_sha256 != allowlist["orchestrator_state_sha256"]
        or state["phase"] != "candidate_started"
        or len(rows) != allowlist["journal_rows"]
        or hashlib.sha256(journal_raw).hexdigest() != allowlist["journal_sha256"]
    ):
        fail("audited_quarantined_source_transition_baseline_invalid")
    return state


def audited_quarantined_source_transition_v2(
    context, platform, allowlist, paths, intent, intent_raw,
    previous_transition, source_record, revision, bootstrap_path,
    bootstrap_state, state, boundary
):
    interlock_path = paths["source_transition_v2_interlock"]
    transition_path = paths["source_transition_v2"]
    git_facts = audited_quarantined_source_transition_git_facts(
        revision,
        AUDITED_QUARANTINED_RECOVERY_V1_TO_COMMIT,
        AUDITED_QUARANTINED_RECOVERY_V1_TO_FILE_SHA256,
    )
    expected_audit_configuration = (
        audited_quarantined_v2_audit_configuration()
    )
    if (
        platform.quarantined_recovery_login_keychain_policy()
        != audited_quarantined_v1_audit_configuration()
        or platform.quarantined_recovery_static_sql_sha256()
        != AUDITED_QUARANTINED_RECOVERY_V2_STATIC_SQL_SHA256
    ):
        fail("audited_quarantined_source_transition_v2_audit_policy_invalid")
    base = {
        "schema_version": 2,
        "kind": AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_INTERLOCK_KIND,
        "run_id": context.manifest["run_id"],
        "manifest_sha256": context.digest,
        "intent_sha256": hashlib.sha256(intent_raw).hexdigest(),
        "previous_source_transition_sha256": (
            AUDITED_QUARANTINED_RECOVERY_V1_TRANSITION_SHA256
        ),
        "from_source": previous_transition["to_source"],
        "to_source": source_record,
        **git_facts,
        "reason_codes": list(AUDITED_QUARANTINED_RECOVERY_V2_REASON_CODES),
        "audit_configuration": expected_audit_configuration,
        "bootstrap_state_semantic_sha256": (
            audited_quarantined_bootstrap_semantic_sha256(bootstrap_state)
        ),
        "orchestrator_state_sha256": allowlist["orchestrator_state_sha256"],
        "baseline_journal_sha256": allowlist["journal_sha256"],
        "baseline_journal_rows": allowlist["journal_rows"],
        "lifecycle_sha256": allowlist["lifecycle_sha256"],
        **boundary,
        "created_at": utc_now(),
    }
    interlock = audited_write_once_marker(
        interlock_path, base,
        AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_BASE_FIELDS,
        "audited_quarantined_source_transition_v2_interlock_invalid",
    )
    interlock_raw = audited_private_file_bytes(
        interlock_path, {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_source_transition_v2_interlock_invalid",
    )
    require_audited_manifest_unchanged(context)
    require_audited_quarantined_source_transition_baseline(
        context, bootstrap_path, allowlist
    )
    checked_previous, checked_previous_sha256 = (
        validate_audited_quarantined_v1_source_transition(
            context, paths, intent, intent_raw, bootstrap_state, allowlist
        )
    )
    second_revision = current_clean_recovery_source()
    second_observations = validate_audited_source_observations(
        context, observe_audited_recovery_source_trees(context.manifest)
    )
    second_source_record = audited_recovery_current_source(
        context, second_observations, second_revision
    )
    second_boundary = audited_quarantined_source_transition_boundary(
        context, platform, allowlist, paths, state
    )
    if (
        checked_previous != previous_transition
        or checked_previous_sha256
        != AUDITED_QUARANTINED_RECOVERY_V1_TRANSITION_SHA256
        or second_revision != revision
        or second_source_record != source_record
        or second_boundary != boundary
        or audited_quarantined_source_transition_git_facts(
            second_revision,
            AUDITED_QUARANTINED_RECOVERY_V1_TO_COMMIT,
            AUDITED_QUARANTINED_RECOVERY_V1_TO_FILE_SHA256,
        ) != git_facts
        or platform.quarantined_recovery_login_keychain_policy()
        != audited_quarantined_v1_audit_configuration()
        or platform.quarantined_recovery_static_sql_sha256()
        != AUDITED_QUARANTINED_RECOVERY_V2_STATIC_SQL_SHA256
    ):
        fail("audited_quarantined_source_transition_v2_changed")
    expected_transition = {
        **interlock,
        "kind": AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_KIND,
        "interlock_sha256": hashlib.sha256(interlock_raw).hexdigest(),
    }
    audited_write_once_marker(
        transition_path, expected_transition,
        AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_FIELDS,
        "audited_quarantined_source_transition_v2_invalid",
    )
    return validate_audited_quarantined_source_transition_v2(
        context, paths, intent, intent_raw, previous_transition,
        source_record, revision, bootstrap_state, allowlist
    )


def validate_audited_quarantined_source_transition_v2(
    context, paths, intent, intent_raw, previous_transition,
    source_record, revision, bootstrap_state, allowlist
):
    interlock = audited_quarantined_marker(
        paths["source_transition_v2_interlock"],
        AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_BASE_FIELDS,
        "audited_quarantined_source_transition_v2_interlock_invalid",
    )
    interlock_raw = audited_private_file_bytes(
        paths["source_transition_v2_interlock"], {0o600},
        D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_source_transition_v2_interlock_invalid",
    )
    transition = audited_quarantined_marker(
        paths["source_transition_v2"],
        AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_FIELDS,
        "audited_quarantined_source_transition_v2_invalid",
    )
    transition_raw = audited_private_file_bytes(
        paths["source_transition_v2"], {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_source_transition_v2_invalid",
    )
    expected_interlock = {
        name: transition[name]
        for name in AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_BASE_FIELDS
    }
    expected_interlock["kind"] = (
        AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_INTERLOCK_KIND
    )
    if (
        interlock != expected_interlock
        or transition.get("interlock_sha256")
        != hashlib.sha256(interlock_raw).hexdigest()
        or transition.get("previous_source_transition_sha256")
        != AUDITED_QUARANTINED_RECOVERY_V1_TRANSITION_SHA256
        or transition.get("intent_sha256") != hashlib.sha256(intent_raw).hexdigest()
        or transition.get("from_source") != previous_transition["to_source"]
        or transition.get("to_source") != source_record
        or transition.get("reason_codes")
        != list(AUDITED_QUARANTINED_RECOVERY_V2_REASON_CODES)
        or transition.get("audit_configuration")
        != audited_quarantined_v2_audit_configuration()
        or transition.get("schema_version") != 2
        or transition.get("kind") != AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_KIND
        or transition.get("run_id") != context.manifest["run_id"]
        or transition.get("manifest_sha256") != context.digest
        or transition.get("bootstrap_state_semantic_sha256")
        != audited_quarantined_bootstrap_semantic_sha256(bootstrap_state)
        or transition.get("orchestrator_state_sha256")
        != allowlist["orchestrator_state_sha256"]
        or transition.get("baseline_journal_sha256") != allowlist["journal_sha256"]
        or transition.get("baseline_journal_rows") != allowlist["journal_rows"]
        or transition.get("lifecycle_sha256") != allowlist["lifecycle_sha256"]
        or transition.get("teardown_fence_sha256")
        != AUDITED_QUARANTINED_RECOVERY_CLOSING_FENCE_SHA256
        or transition.get("transport_instance_id") != allowlist["transport_instance_id"]
        or transition.get("transport_inventory_sha256")
        != allowlist["empty_transport_inventory_sha256"]
        or transition.get("effect_admission_operation_id")
        != AUDITED_QUARANTINED_NO_ISSUE_OPERATION_ID
        or not validate_utc_timestamp(transition.get("created_at"))
        or any(transition.get(name) is not True for name in (
            "producer_launchd_jobs_absent", "issuer_process_group_absent",
            "transport_identity_verified", "transport_effect_admission_drained",
            "postgres_running", "protected_staging_unchanged",
            "database_absence_marker_absent", "reconciliation_marker_absent",
            "recovery_evidence_absent",
        ))
    ):
        fail("audited_quarantined_source_transition_v2_invalid")
    facts = audited_quarantined_source_transition_git_facts(
        revision,
        AUDITED_QUARANTINED_RECOVERY_V1_TO_COMMIT,
        AUDITED_QUARANTINED_RECOVERY_V1_TO_FILE_SHA256,
    )
    if any(transition.get(name) != value for name, value in facts.items()):
        fail("audited_quarantined_source_transition_v2_invalid")
    return transition, hashlib.sha256(transition_raw).hexdigest()


def validate_audited_quarantined_historical_source_transition_v2(
    context, paths, intent_raw, bootstrap_state, allowlist, previous_transition
):
    interlock = audited_quarantined_marker(
        paths["source_transition_v2_interlock"],
        AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_BASE_FIELDS,
        "audited_quarantined_historical_source_transition_v2_invalid",
    )
    interlock_raw = audited_private_file_bytes(
        paths["source_transition_v2_interlock"], {0o600},
        D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_historical_source_transition_v2_invalid",
    )
    transition = audited_quarantined_marker(
        paths["source_transition_v2"],
        AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_FIELDS,
        "audited_quarantined_historical_source_transition_v2_invalid",
    )
    transition_raw = audited_private_file_bytes(
        paths["source_transition_v2"], {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_historical_source_transition_v2_invalid",
    )
    expected_interlock = {
        name: transition[name]
        for name in AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_BASE_FIELDS
    }
    expected_interlock["kind"] = (
        AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_INTERLOCK_KIND
    )
    file_sha256 = transition.get("file_sha256")
    if (
        hashlib.sha256(interlock_raw).hexdigest()
        != AUDITED_QUARANTINED_RECOVERY_V2_INTERLOCK_SHA256
        or hashlib.sha256(transition_raw).hexdigest()
        != AUDITED_QUARANTINED_RECOVERY_V2_TRANSITION_SHA256
        or interlock != expected_interlock
        or transition.get("interlock_sha256")
        != AUDITED_QUARANTINED_RECOVERY_V2_INTERLOCK_SHA256
        or transition.get("schema_version") != 2
        or transition.get("kind") != AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_KIND
        or transition.get("run_id") != context.manifest["run_id"]
        or transition.get("manifest_sha256") != context.digest
        or transition.get("intent_sha256") != hashlib.sha256(intent_raw).hexdigest()
        or transition.get("previous_source_transition_sha256")
        != AUDITED_QUARANTINED_RECOVERY_V1_TRANSITION_SHA256
        or transition.get("from_source") != previous_transition["to_source"]
        or not isinstance(transition.get("to_source"), dict)
        or transition["to_source"].get("commit_sha")
        != AUDITED_QUARANTINED_RECOVERY_V2_TO_COMMIT
        or transition["to_source"].get("tree_sha")
        != AUDITED_QUARANTINED_RECOVERY_V2_TO_TREE
        or transition.get("parent_commit_sha")
        != AUDITED_QUARANTINED_RECOVERY_V1_TO_COMMIT
        or transition.get("parent_count") != 1
        or transition.get("changed_paths")
        != list(AUDITED_QUARANTINED_RECOVERY_CHANGED_PATHS)
        or not isinstance(file_sha256, dict)
        or set(file_sha256) != set(AUDITED_QUARANTINED_RECOVERY_CHANGED_PATHS)
        or any(
            file_sha256[path]
            != {
                "from_sha256": AUDITED_QUARANTINED_RECOVERY_V1_TO_FILE_SHA256[path],
                "to_sha256": AUDITED_QUARANTINED_RECOVERY_V2_TO_FILE_SHA256[path],
            }
            for path in AUDITED_QUARANTINED_RECOVERY_CHANGED_PATHS
        )
        or transition.get("reason_codes")
        != list(AUDITED_QUARANTINED_RECOVERY_V2_REASON_CODES)
        or transition.get("audit_configuration")
        != audited_quarantined_v2_audit_configuration()
        or transition.get("bootstrap_state_semantic_sha256")
        != audited_quarantined_bootstrap_semantic_sha256(bootstrap_state)
        or transition.get("orchestrator_state_sha256")
        != allowlist["orchestrator_state_sha256"]
        or transition.get("baseline_journal_sha256") != allowlist["journal_sha256"]
        or transition.get("baseline_journal_rows") != allowlist["journal_rows"]
        or transition.get("lifecycle_sha256") != allowlist["lifecycle_sha256"]
        or transition.get("teardown_fence_sha256")
        != AUDITED_QUARANTINED_RECOVERY_CLOSING_FENCE_SHA256
        or not validate_utc_timestamp(transition.get("created_at"))
        or any(transition.get(name) is not True for name in (
            "producer_launchd_jobs_absent", "issuer_process_group_absent",
            "transport_identity_verified", "transport_effect_admission_drained",
            "postgres_running", "protected_staging_unchanged",
            "database_absence_marker_absent", "reconciliation_marker_absent",
            "recovery_evidence_absent",
        ))
    ):
        fail("audited_quarantined_historical_source_transition_v2_invalid")
    return transition, hashlib.sha256(transition_raw).hexdigest()


def audited_quarantined_cleanup_keychain_inventory(context, platform):
    anchor_names = tuple(
        (service, account)
        for service, account, _identity_sha256
        in AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHORS
    )
    if (
        tuple(external_keychain_inventory(context)) != anchor_names
        or set(anchor_names).intersection(keychain_inventory(context))
    ):
        fail("audited_quarantined_cleanup_keychain_boundary_invalid")
    anchor_inventory = [
        {
            "service": service,
            "account": account,
            "identity_sha256": identity_sha256,
        }
        for service, account, identity_sha256
        in AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHORS
    ]
    anchor_raw = json.dumps(
        anchor_inventory, ensure_ascii=False, sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    if hashlib.sha256(anchor_raw).hexdigest() != (
        AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHOR_INVENTORY_SHA256
    ):
        fail("audited_quarantined_cleanup_keychain_boundary_invalid")
    require_audited_cleanup_keychain_policy(platform)
    observed = platform.audited_keychain_item_identities(
        tuple(keychain_inventory(context)),
        AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHORS,
    )
    inventory = []
    services = set()
    for service, account in keychain_inventory(context):
        identity_sha256 = observed[(service, account)]
        if (
            not isinstance(identity_sha256, str)
            or DIGEST_PATTERN.fullmatch(identity_sha256) is None
        ):
            fail("audited_quarantined_cleanup_keychain_boundary_invalid")
        inventory.append({
            "service": service,
            "account": account,
            "identity_sha256": identity_sha256,
        })
        services.add(service)
    if (
        len(inventory) != 29
        or any(
            not platform.audited_keychain_owner_matches(
                service, context.manifest["run_id"]
            )
            for service in services
        )
    ):
        fail("audited_quarantined_cleanup_keychain_boundary_invalid")
    platform.audited_keychain_item_identities(
        (), AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHORS
    )
    require_audited_cleanup_keychain_policy(platform)
    raw = json.dumps(
        inventory, ensure_ascii=False, sort_keys=True, separators=(",", ":")
    ).encode("utf-8")
    return inventory, hashlib.sha256(raw).hexdigest()


def audited_quarantined_cleanup_transition_boundary(
    context, platform, allowlist, paths, state, rows, journal_raw
):
    fence_raw = audited_private_file_bytes(
        d2a_teardown_fence_path(context), {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_cleanup_transition_boundary_invalid",
    )
    root_identity_raw = audited_private_file_bytes(
        cleanup_root_identity_path(context), {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_cleanup_transition_boundary_invalid",
    )
    root_identity = load_cleanup_root_identity(context)
    root_metadata = validate_cleanup_root_directory(context, context.root)
    cleanup_rows = rows[allowlist["journal_rows"]:]
    if (
        hashlib.sha256(fence_raw).hexdigest()
        != AUDITED_QUARANTINED_RECOVERY_CLOSED_FENCE_SHA256
        or state["phase"] != "candidate_started"
        or len(rows) != AUDITED_QUARANTINED_RECOVERY_CLEANUP_JOURNAL_ROWS
        or hashlib.sha256(journal_raw).hexdigest()
        != AUDITED_QUARANTINED_RECOVERY_CLEANUP_JOURNAL_SHA256
        or len(cleanup_rows) != 2
        or [row.get("action") for row in cleanup_rows] != ["cleanup", "cleanup"]
        or [row.get("status") for row in cleanup_rows] != ["intent", "failed"]
        or [row.get("target") for row in cleanup_rows] != ["run", "run"]
        or [row.get("sequence") for row in cleanup_rows] != [45, 46]
        or any(
            not platform.launchd_absent(
                context.manifest["services"][name]["label"]
            )
            for name in SERVICE_START_ORDER
        )
        or not platform.launchd_overrides_absent(candidate_launchd_labels(context))
        or platform.postgres_running(context.cluster_root)
        or not cleanup_postgres_absent(context, platform)
        or root_identity is None
        or root_metadata is None
        or not cleanup_root_identity_matches(root_metadata, root_identity)
        or cleanup_path_metadata(
            cleanup_root_quarantine_path(context),
            "audited_quarantined_cleanup_transition_boundary_invalid",
        ) is not None
        or hashlib.sha256(root_identity_raw).hexdigest()
        != AUDITED_QUARANTINED_RECOVERY_ROOT_IDENTITY_SHA256
        or any(os.path.lexists(path) for path in (
            cleanup_keychain_baseline_path(context),
            cleanup_root_progress_path(context),
            context.artifact_directory / "cleanup-evidence.json",
            paths["evidence"],
        ))
        or standing_snapshot(context, platform) != state["standing_snapshot"]
    ):
        fail("audited_quarantined_cleanup_transition_boundary_invalid")
    for name in SERVICE_START_ORDER:
        require_audited_process_group_absent(
            AUDITED_QUARANTINED_SERVICE_IDENTITIES[name]["process_group_id"],
            "audited_quarantined_cleanup_transition_boundary_invalid",
        )
    require_audited_quarantined_lifecycle(context, allowlist)
    inventory, inventory_sha256 = (
        audited_quarantined_cleanup_keychain_inventory(context, platform)
    )
    if (
        inventory_sha256
        != AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_INVENTORY_SHA256
    ):
        fail("audited_quarantined_cleanup_keychain_boundary_invalid")
    return {
        "teardown_fence_sha256": hashlib.sha256(fence_raw).hexdigest(),
        "cleanup_journal_sha256": hashlib.sha256(journal_raw).hexdigest(),
        "cleanup_journal_rows": len(rows),
        "producer_launchd_jobs_absent": True,
        "issuer_process_group_absent": True,
        "postgres_absent": True,
        "postgres_process_absent": True,
        "keychain_inventory_sha256": (
            AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_INVENTORY_SHA256
        ),
        "keychain_item_count": len(inventory),
        "keychain_anchor_policy_kind": (
            AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHOR_POLICY_KIND
        ),
        "keychain_anchor_inventory_sha256": (
            AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHOR_INVENTORY_SHA256
        ),
        "keychain_anchor_item_count": len(
            AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHORS
        ),
        "cleanup_root_identity_sha256": hashlib.sha256(
            root_identity_raw
        ).hexdigest(),
        "isolated_root_retained": True,
        "cleanup_keychain_baseline_absent": True,
        "cleanup_root_progress_absent": True,
        "cleanup_evidence_absent": True,
        "protected_staging_unchanged": True,
    }


def validate_audited_quarantined_cleanup_replay_journal(
    context, allowlist, paths, transition, state
):
    rows, raw = audited_quarantined_journal(context, allowlist)
    baseline_rows = transition["cleanup_journal_rows"]
    if (
        baseline_rows != AUDITED_QUARANTINED_RECOVERY_CLEANUP_JOURNAL_ROWS
        or len(rows) < baseline_rows
        or hashlib.sha256(
            b"".join(
                (canonical_json(row) + "\n").encode("utf-8")
                for row in rows[:baseline_rows]
            )
        ).hexdigest() != transition["cleanup_journal_sha256"]
    ):
        fail("audited_quarantined_cleanup_replay_journal_invalid")
    suffix = rows[baseline_rows:]
    if any(
        row.get("action") != "cleanup"
        or row.get("target") != "run"
        or row.get("status") not in {"intent", "failed", "complete"}
        or row.get("sequence") != baseline_rows + index + 1
        for index, row in enumerate(suffix)
    ):
        fail("audited_quarantined_cleanup_replay_journal_invalid")
    for index, row in enumerate(suffix):
        if row["status"] == "intent":
            continue
        if index == 0 or suffix[index - 1]["status"] != "intent":
            fail("audited_quarantined_cleanup_replay_journal_invalid")
    if any(
        row["status"] == "complete" and index != len(suffix) - 1
        for index, row in enumerate(suffix)
    ):
        fail("audited_quarantined_cleanup_replay_journal_invalid")
    cleanup_evidence_present = os.path.lexists(
        context.artifact_directory / "cleanup-evidence.json"
    )
    final_evidence_present = os.path.lexists(paths["evidence"])
    keychain_baseline_present = os.path.lexists(
        cleanup_keychain_baseline_path(context)
    )
    root_progress_present = os.path.lexists(cleanup_root_progress_path(context))
    if (
        (not suffix and (keychain_baseline_present or root_progress_present))
        or (root_progress_present and not keychain_baseline_present)
        or (
            state["phase"] == "cleaned"
            and (not keychain_baseline_present or not root_progress_present)
        )
    ):
        fail("audited_quarantined_cleanup_replay_journal_invalid")
    if (
        state["phase"] == "candidate_started"
        and (
            cleanup_evidence_present
            or final_evidence_present
            or any(row["status"] == "complete" for row in suffix)
        )
    ):
        fail("audited_quarantined_cleanup_replay_journal_invalid")
    if state["phase"] == "cleaned" and (
        not suffix
        or suffix[-1]["status"] not in {"intent", "complete"}
    ):
        fail("audited_quarantined_cleanup_replay_journal_invalid")
    if final_evidence_present and (
        state["phase"] != "cleaned"
        or not cleanup_evidence_present
        or not suffix
        or suffix[-1]["status"] != "complete"
    ):
        fail("audited_quarantined_cleanup_replay_journal_invalid")
    return rows, raw


def audited_quarantined_cleanup_transition(
    context, platform, allowlist, paths, intent_raw, previous_transition,
    source_transition_v1, source_record, revision, bootstrap_state, state,
    rows, journal_raw, boundary
):
    git_facts = audited_quarantined_source_transition_git_facts(
        revision,
        AUDITED_QUARANTINED_RECOVERY_V2_TO_COMMIT,
        AUDITED_QUARANTINED_RECOVERY_V2_TO_FILE_SHA256,
    )
    base = {
        "schema_version": 1,
        "kind": AUDITED_QUARANTINED_CLEANUP_TRANSITION_INTERLOCK_KIND,
        "run_id": context.manifest["run_id"],
        "manifest_sha256": context.digest,
        "intent_sha256": hashlib.sha256(intent_raw).hexdigest(),
        "previous_source_transition_sha256": (
            AUDITED_QUARANTINED_RECOVERY_V2_TRANSITION_SHA256
        ),
        "database_absence_sha256": (
            AUDITED_QUARANTINED_RECOVERY_DATABASE_SHA256
        ),
        "reconciliation_sha256": (
            AUDITED_QUARANTINED_RECOVERY_RECONCILIATION_SHA256
        ),
        "from_source": previous_transition["to_source"],
        "to_source": source_record,
        **git_facts,
        "reason_codes": list(
            AUDITED_QUARANTINED_RECOVERY_CLEANUP_REASON_CODES
        ),
        "audit_configuration": audited_quarantined_v2_audit_configuration(),
        "bootstrap_state_semantic_sha256": (
            audited_quarantined_bootstrap_semantic_sha256(bootstrap_state)
        ),
        "orchestrator_state_sha256": allowlist["orchestrator_state_sha256"],
        "baseline_journal_sha256": allowlist["journal_sha256"],
        "baseline_journal_rows": allowlist["journal_rows"],
        "lifecycle_sha256": allowlist["lifecycle_sha256"],
        **boundary,
        "created_at": utc_now(),
    }
    interlock = audited_write_once_marker(
        paths["cleanup_transition_interlock"], base,
        AUDITED_QUARANTINED_CLEANUP_TRANSITION_BASE_FIELDS,
        "audited_quarantined_cleanup_transition_interlock_invalid",
    )
    interlock_raw = audited_private_file_bytes(
        paths["cleanup_transition_interlock"], {0o600},
        D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_cleanup_transition_interlock_invalid",
    )
    require_audited_manifest_unchanged(context)
    checked_previous, checked_previous_sha256 = (
        validate_audited_quarantined_historical_source_transition_v2(
            context, paths, intent_raw, bootstrap_state, allowlist,
            source_transition_v1
        )
    )
    second_revision = current_clean_recovery_source()
    second_observations = validate_audited_source_observations(
        context, observe_audited_recovery_source_trees(context.manifest)
    )
    second_source_record = audited_recovery_current_source(
        context, second_observations, second_revision
    )
    checked_state, _state_sha256 = audited_quarantined_state(context)
    checked_rows, checked_journal_raw = audited_quarantined_journal(
        context, allowlist
    )
    second_boundary = audited_quarantined_cleanup_transition_boundary(
        context, platform, allowlist, paths, checked_state,
        checked_rows, checked_journal_raw,
    )
    if (
        checked_previous != previous_transition
        or checked_previous_sha256
        != AUDITED_QUARANTINED_RECOVERY_V2_TRANSITION_SHA256
        or second_revision != revision
        or second_source_record != source_record
        or second_boundary != boundary
        or audited_quarantined_source_transition_git_facts(
            second_revision,
            AUDITED_QUARANTINED_RECOVERY_V2_TO_COMMIT,
            AUDITED_QUARANTINED_RECOVERY_V2_TO_FILE_SHA256,
        ) != git_facts
        or platform.quarantined_recovery_login_keychain_policy()
        != audited_quarantined_v1_audit_configuration()
        or platform.quarantined_recovery_static_sql_sha256()
        != AUDITED_QUARANTINED_RECOVERY_V2_STATIC_SQL_SHA256
    ):
        fail("audited_quarantined_cleanup_transition_changed")
    validate_audited_quarantined_database_reconciliation_chain(
        context, paths, AUDITED_QUARANTINED_RECOVERY_V2_TRANSITION_SHA256
    )
    expected_transition = {
        **interlock,
        "kind": AUDITED_QUARANTINED_CLEANUP_TRANSITION_KIND,
        "interlock_sha256": hashlib.sha256(interlock_raw).hexdigest(),
    }
    audited_write_once_marker(
        paths["cleanup_transition"], expected_transition,
        AUDITED_QUARANTINED_CLEANUP_TRANSITION_FIELDS,
        "audited_quarantined_cleanup_transition_invalid",
    )
    return validate_audited_quarantined_cleanup_transition(
        context, paths, intent_raw, previous_transition, source_record,
        revision, bootstrap_state, allowlist
    )


def validate_audited_quarantined_cleanup_transition(
    context, paths, intent_raw, previous_transition, source_record,
    revision, bootstrap_state, allowlist
):
    interlock = audited_quarantined_marker(
        paths["cleanup_transition_interlock"],
        AUDITED_QUARANTINED_CLEANUP_TRANSITION_BASE_FIELDS,
        "audited_quarantined_cleanup_transition_interlock_invalid",
    )
    interlock_raw = audited_private_file_bytes(
        paths["cleanup_transition_interlock"], {0o600},
        D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_cleanup_transition_interlock_invalid",
    )
    transition = audited_quarantined_marker(
        paths["cleanup_transition"],
        AUDITED_QUARANTINED_CLEANUP_TRANSITION_FIELDS,
        "audited_quarantined_cleanup_transition_invalid",
    )
    transition_raw = audited_private_file_bytes(
        paths["cleanup_transition"], {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_cleanup_transition_invalid",
    )
    expected_interlock = {
        name: transition[name]
        for name in AUDITED_QUARANTINED_CLEANUP_TRANSITION_BASE_FIELDS
    }
    expected_interlock["kind"] = (
        AUDITED_QUARANTINED_CLEANUP_TRANSITION_INTERLOCK_KIND
    )
    if (
        interlock != expected_interlock
        or transition.get("interlock_sha256")
        != hashlib.sha256(interlock_raw).hexdigest()
        or transition.get("schema_version") != 1
        or transition.get("kind") != AUDITED_QUARANTINED_CLEANUP_TRANSITION_KIND
        or transition.get("run_id") != context.manifest["run_id"]
        or transition.get("manifest_sha256") != context.digest
        or transition.get("intent_sha256") != hashlib.sha256(intent_raw).hexdigest()
        or transition.get("previous_source_transition_sha256")
        != AUDITED_QUARANTINED_RECOVERY_V2_TRANSITION_SHA256
        or transition.get("database_absence_sha256")
        != AUDITED_QUARANTINED_RECOVERY_DATABASE_SHA256
        or transition.get("reconciliation_sha256")
        != AUDITED_QUARANTINED_RECOVERY_RECONCILIATION_SHA256
        or transition.get("from_source") != previous_transition["to_source"]
        or transition.get("to_source") != source_record
        or transition.get("reason_codes")
        != list(AUDITED_QUARANTINED_RECOVERY_CLEANUP_REASON_CODES)
        or transition.get("audit_configuration")
        != audited_quarantined_v2_audit_configuration()
        or transition.get("bootstrap_state_semantic_sha256")
        != audited_quarantined_bootstrap_semantic_sha256(bootstrap_state)
        or transition.get("orchestrator_state_sha256")
        != allowlist["orchestrator_state_sha256"]
        or transition.get("baseline_journal_sha256") != allowlist["journal_sha256"]
        or transition.get("baseline_journal_rows") != allowlist["journal_rows"]
        or transition.get("lifecycle_sha256") != allowlist["lifecycle_sha256"]
        or transition.get("teardown_fence_sha256")
        != AUDITED_QUARANTINED_RECOVERY_CLOSED_FENCE_SHA256
        or transition.get("cleanup_journal_sha256")
        != AUDITED_QUARANTINED_RECOVERY_CLEANUP_JOURNAL_SHA256
        or transition.get("cleanup_journal_rows")
        != AUDITED_QUARANTINED_RECOVERY_CLEANUP_JOURNAL_ROWS
        or transition.get("keychain_item_count") != 29
        or transition.get("keychain_inventory_sha256")
        != AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_INVENTORY_SHA256
        or transition.get("keychain_anchor_policy_kind")
        != AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHOR_POLICY_KIND
        or transition.get("keychain_anchor_inventory_sha256")
        != AUDITED_QUARANTINED_RECOVERY_KEYCHAIN_ANCHOR_INVENTORY_SHA256
        or transition.get("keychain_anchor_item_count") != 3
        or transition.get("cleanup_root_identity_sha256")
        != AUDITED_QUARANTINED_RECOVERY_ROOT_IDENTITY_SHA256
        or not validate_utc_timestamp(transition.get("created_at"))
        or any(transition.get(name) is not True for name in (
            "producer_launchd_jobs_absent", "issuer_process_group_absent",
            "postgres_absent", "postgres_process_absent",
            "isolated_root_retained", "cleanup_keychain_baseline_absent",
            "cleanup_root_progress_absent", "cleanup_evidence_absent",
            "protected_staging_unchanged",
        ))
    ):
        fail("audited_quarantined_cleanup_transition_invalid")
    facts = audited_quarantined_source_transition_git_facts(
        revision,
        AUDITED_QUARANTINED_RECOVERY_V2_TO_COMMIT,
        AUDITED_QUARANTINED_RECOVERY_V2_TO_FILE_SHA256,
    )
    if any(transition.get(name) != value for name, value in facts.items()):
        fail("audited_quarantined_cleanup_transition_invalid")
    return transition, hashlib.sha256(transition_raw).hexdigest()


def audited_quarantined_state(context):
    fields = tuple(sorted({
        "schema_version", "manifest_sha256", "run_id", "phase", "updated_at",
        "standing_snapshot",
    }))
    state = load_strict_d2a_marker(
        context.state_path,
        "audited_quarantined_orchestrator_state_invalid",
        fields,
        sorted_canonical=True,
    )
    raw = audited_private_file_bytes(
        context.state_path, {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_orchestrator_state_invalid",
    )
    if (
        state.get("schema_version") != 1
        or state.get("manifest_sha256") != context.digest
        or state.get("run_id") != context.manifest["run_id"]
        or state.get("phase") not in {"candidate_started", "cleaned"}
        or not validate_utc_timestamp(state.get("updated_at"))
        or not isinstance(state.get("standing_snapshot"), dict)
    ):
        fail("audited_quarantined_orchestrator_state_invalid")
    return state, hashlib.sha256(raw).hexdigest()


def audited_quarantined_journal(context, allowlist):
    rows, raw = read_strict_journal_snapshot(context)
    baseline_rows = allowlist["journal_rows"]
    if len(rows) < baseline_rows:
        fail("audited_quarantined_journal_invalid")
    lines = raw.splitlines(keepends=True)
    if hashlib.sha256(b"".join(lines[:baseline_rows])).hexdigest() != allowlist[
        "journal_sha256"
    ]:
        fail("audited_quarantined_journal_invalid")
    if any(
        row.get("action") != "cleanup"
        or row.get("status") not in {"intent", "failed", "complete"}
        or row.get("target") != "run"
        for row in rows[baseline_rows:]
    ):
        fail("audited_quarantined_journal_invalid")
    return rows, raw


def require_audited_quarantined_lifecycle(context, allowlist):
    taint_raw = audited_private_file_bytes(
        d2a_taint_path(context), {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_taint_invalid",
    )
    lifecycle_raw = audited_private_file_bytes(
        d2a_session_lifecycle_path(context), {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_lifecycle_invalid",
    )
    if (
        hashlib.sha256(taint_raw).hexdigest() != allowlist["taint_sha256"]
        or hashlib.sha256(lifecycle_raw).hexdigest()
        != allowlist["lifecycle_sha256"]
    ):
        fail("audited_quarantined_lifecycle_drift")
    taint = load_strict_d2a_marker(
        d2a_taint_path(context), "audited_quarantined_taint_invalid",
        D2A_TAINT_FIELDS,
    )
    lifecycle = load_strict_d2a_marker(
        d2a_session_lifecycle_path(context),
        "audited_quarantined_lifecycle_invalid", D2A_SESSION_LIFECYCLE_FIELDS,
    )
    if (
        taint.get("run_id") != context.manifest["run_id"]
        or taint.get("manifest_sha256") != context.digest
        or taint.get("release_eligible") is not False
        or lifecycle
        != {
            "schema_version": 1,
            "kind": "starring.d2a.session-lifecycle.v1",
            "run_id": context.manifest["run_id"],
            "manifest_sha256": context.digest,
            "operation": "direct-onboard",
            "origin": "issuer",
            "issuer_sha256": allowlist["issuer_sha256"],
            "issuer_source_sha256": allowlist["issuer_source_sha256"],
            "uid": os.getuid(),
            "boot_identity": "darwin-boottime:1786435163:174871",
            "process_group_id": 31359,
            "started_at": "2026-08-12T08:21:11.229463000Z",
            "status": "quarantined",
            "session_revoked": False,
            "revoked_at": None,
            "quarantined_at": "2026-08-12T08:21:12.175186000Z",
        }
    ):
        fail("audited_quarantined_lifecycle_invalid")
    try:
        now = datetime.datetime.strptime(utc_now(), "%Y-%m-%dT%H:%M:%SZ").replace(
            tzinfo=datetime.timezone.utc
        )
        safe_after = datetime.datetime.strptime(
            allowlist["safe_after"], "%Y-%m-%dT%H:%M:%SZ"
        ).replace(tzinfo=datetime.timezone.utc)
    except ValueError:
        fail("audited_quarantined_safe_after_invalid")
    if now < safe_after:
        fail("audited_quarantined_safe_after_not_elapsed")
    if lifecycle["boot_identity"] == current_darwin_boot_identity():
        try:
            os.killpg(lifecycle["process_group_id"], 0)
        except ProcessLookupError:
            pass
        except PermissionError:
            fail("audited_quarantined_process_group_present")
        else:
            fail("audited_quarantined_process_group_present")
    return lifecycle


def require_empty_audited_transport_inventory(context, platform, allowlist):
    inventory = platform.transport_control(context, "resource_inventory")
    if (
        inventory.get("instance_id") != allowlist["transport_instance_id"]
        or inventory.get("digest_sha256")
        != allowlist["empty_transport_inventory_sha256"]
        or inventory.get("history") != []
        or inventory.get("created") != []
        or inventory.get("deleted") != []
        or inventory.get("active") != []
    ):
        fail("audited_quarantined_transport_inventory_invalid")
    return inventory


def require_audited_quarantined_coordinator_baseline(context, allowlist):
    receipts = audited_private_file_bytes(
        context.manifest_path.with_name("receipts.jsonl"), {0o600}, 1024,
        "audited_quarantined_receipts_invalid", allow_empty=True,
    )
    coordinator = context.run_directory / "coordinator"
    try:
        metadata = coordinator.lstat()
        entries = {path.name: path for path in coordinator.iterdir()}
    except OSError:
        fail("audited_quarantined_coordinator_invalid")
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or coordinator.is_symlink()
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != 0o700
        or set(entries) not in (
            {
                "coordinator.lock",
                "audited-quarantined-no-issue-recovery-source-transition-"
                "interlock.json",
            },
            {
                "coordinator.lock",
                "audited-quarantined-no-issue-recovery-source-transition-"
                "interlock.json",
                "audited-quarantined-no-issue-recovery-source-transition-"
                "v2-interlock.json",
            },
            {
                "coordinator.lock",
                "audited-quarantined-no-issue-recovery-source-transition-"
                "interlock.json",
                "audited-quarantined-no-issue-recovery-source-transition-"
                "v2-interlock.json",
                "audited-quarantined-no-issue-recovery-cleanup-transition-"
                "interlock.json",
            },
        )
        or hashlib.sha256(receipts).hexdigest() != allowlist["receipts_sha256"]
    ):
        fail("audited_quarantined_coordinator_invalid")
    lock_raw = audited_private_file_bytes(
        entries["coordinator.lock"], {0o600}, 1024,
        "audited_quarantined_coordinator_invalid", allow_empty=True,
    )
    if hashlib.sha256(lock_raw).hexdigest() != allowlist["coordinator_lock_sha256"]:
        fail("audited_quarantined_coordinator_invalid")
    source_directory = context.artifact_directory / "coordinator-sources"
    try:
        source_metadata = source_directory.lstat()
        source_entries = {
            path.name: path for path in source_directory.iterdir()
        }
    except OSError:
        fail("audited_quarantined_coordinator_source_invalid")
    expected = allowlist["coordinator_source_sha256"]
    if (
        not stat.S_ISDIR(source_metadata.st_mode)
        or source_directory.is_symlink()
        or source_metadata.st_uid != os.getuid()
        or stat.S_IMODE(source_metadata.st_mode) != 0o700
        or set(source_entries) != set(expected)
    ):
        fail("audited_quarantined_coordinator_source_invalid")
    for name, digest in expected.items():
        raw = audited_private_file_bytes(
            source_entries[name], {0o600}, D2A_MARKER_MAXIMUM_BYTES,
            "audited_quarantined_coordinator_source_invalid",
        )
        if hashlib.sha256(raw).hexdigest() != digest:
            fail("audited_quarantined_coordinator_source_invalid")
    return {
        "receipts_sha256": allowlist["receipts_sha256"],
        "coordinator_lock_sha256": allowlist["coordinator_lock_sha256"],
        "coordinator_source_sha256": expected,
        "source_transition_interlock_present": (
            "audited-quarantined-no-issue-recovery-source-transition-"
            "interlock.json" in entries
        ),
        "source_transition_v2_interlock_present": (
            "audited-quarantined-no-issue-recovery-source-transition-"
            "v2-interlock.json" in entries
        ),
        "cleanup_transition_interlock_present": (
            "audited-quarantined-no-issue-recovery-cleanup-transition-"
            "interlock.json" in entries
        ),
    }


def audited_quarantined_service_identity(context, platform, name, allowlist):
    label = context.manifest["services"][name]["label"]
    job = platform.launchd_job(label)
    if job is None:
        return False
    expected = AUDITED_QUARANTINED_SERVICE_IDENTITIES[name]
    expected_job = {
        "pid": expected["pid"], "program": expected["program"],
        "plist_path": str(service_plist_path(context, name)),
        "arguments": expected["arguments"], "runs": 1, "state": "running",
        "last_exit_code": None,
    }
    plist_raw = audited_private_file_bytes(
        service_plist_path(context, name), {0o600}, 256 * 1024,
        "audited_quarantined_plist_invalid",
    )
    if name == "tunnel":
        tunnel_script = context.artifact_directory / "run-tunnel.zsh"
        tunnel_raw = audited_private_file_bytes(
            tunnel_script, {0o700}, 64 * 1024,
            "audited_quarantined_tunnel_script_invalid",
        )
        if hashlib.sha256(tunnel_raw).hexdigest() != allowlist[
            "tunnel_script_sha256"
        ]:
            fail("audited_quarantined_tunnel_script_invalid")
    candidate = context.manifest["candidates"][expected["candidate"]]
    process = platform.candidate_process_identity(
        expected["pid"], pathlib.Path(candidate["path"])
    )
    try:
        process_group_id = os.getpgid(expected["pid"])
    except OSError:
        fail("audited_quarantined_service_identity_invalid")
    expected_process = {
        "pid": expected["pid"], "path": candidate["path"],
        "sha256": candidate["sha256"], "size": expected["size"],
        "mode": 0o555, "uid": os.getuid(), "device": expected["device"],
        "inode": expected["inode"], "links": 1,
        "start_time_seconds": expected["start_time_seconds"],
        "start_time_microseconds": expected["start_time_microseconds"],
    }
    if (
        job != expected_job
        or process_group_id != expected["process_group_id"]
        or process != expected_process
        or hashlib.sha256(plist_raw).hexdigest()
        != allowlist["plist_sha256"][name]
    ):
        fail("audited_quarantined_service_identity_invalid")
    return True


def require_audited_process_group_absent(process_group_id, code):
    deadline = time.monotonic() + 10
    while True:
        try:
            os.killpg(process_group_id, 0)
        except ProcessLookupError:
            return
        except PermissionError:
            pass
        except OSError as error:
            if error.errno == errno.ESRCH:
                return
            if error.errno != errno.EPERM:
                fail(code)
        if time.monotonic() >= deadline:
            fail(code)
        time.sleep(0.05)


def audited_quarantined_stop_exact(context, platform, name, allowlist):
    label = context.manifest["services"][name]["label"]
    if audited_quarantined_service_identity(context, platform, name, allowlist):
        platform.launchd_bootout(label)
    if not platform.launchd_absent(label):
        fail("audited_quarantined_service_stop_incomplete")
    identity = AUDITED_QUARANTINED_SERVICE_IDENTITIES[name]
    expected_pid = identity["pid"]
    expected_process_group_id = identity["process_group_id"]
    require_audited_process_group_absent(
        expected_process_group_id, "audited_quarantined_service_process_present"
    )


def audited_quarantined_drain_transport(context, platform):
    operation_id = AUDITED_QUARANTINED_NO_ISSUE_OPERATION_ID
    response = platform.transport_effect_admission(
        context, "close_effect_admission", operation_id
    )
    if response["disposition"] not in {"transitioned", "replayed"}:
        fail("audited_quarantined_effect_admission_invalid")
    deadline = time.monotonic() + 10
    while True:
        snapshot = platform.transport_control(context, "snapshot")
        effect = snapshot["effect_http"]
        if (
            effect.get("phase") == "draining"
            and effect.get("operation_id") == operation_id
            and effect.get("active_requests") == 0
            and effect.get("uncertain_requests") == 0
            and effect.get("accepted_requests")
            == effect.get("completed_requests")
        ):
            return snapshot
        if time.monotonic() >= deadline:
            fail("audited_quarantined_effect_drain_incomplete")
        time.sleep(0.1)


def audited_quarantined_postconditions(context, platform, state):
    labels = candidate_launchd_labels(context)
    loaded = sum(1 for label in labels if platform.launchd_loaded(label))
    result = {
        "status": "observed", "phase": "cleaned",
        "postgres_running": context.cluster_root.exists()
        and platform.postgres_running(context.cluster_root),
        "candidate_launchd_jobs_loaded": loaded,
        "candidate_launchd_overrides_absent": platform.launchd_overrides_absent(labels),
        "protected_staging_unchanged": standing_snapshot(context, platform)
        == state["standing_snapshot"],
    }
    if result != {
        "status": "observed", "phase": "cleaned", "postgres_running": False,
        "candidate_launchd_jobs_loaded": 0,
        "candidate_launchd_overrides_absent": True,
        "protected_staging_unchanged": True,
    }:
        fail("audited_quarantined_postconditions_invalid")
    return result


def audited_quarantined_database_payload(
    context, allowlist, intent_sha256, source_transition_sha256,
    inventory_sha256, observed_at, audit_configuration
):
    return {
        "schema_version": 1,
        "kind": AUDITED_QUARANTINED_NO_ISSUE_DATABASE_KIND,
        "run_id": context.manifest["run_id"],
        "manifest_sha256": context.digest,
        "intent_sha256": intent_sha256,
        "source_transition_sha256": source_transition_sha256,
        "post_drain_transport_inventory_sha256": inventory_sha256,
        "observed_at": observed_at,
        "database_name": "starring_runtime_staging",
        "database_system_identifier": allowlist["database_system_identifier"],
        "control_plane_identity": "run_owned_cluster_admin_tcp_v1",
        "topology_verified": True,
        "tables_locked": True,
        "locked_tables": [
            "public.product_oauth_flows",
            "public.product_auth_sessions",
            "public.product_principals",
            "public.product_tenants",
            "public.automation_installations",
            "public.automation_installation_authority_versions",
            "public.runtime_slot_writer_fences_v2",
            "public.product_control_plane_identity",
        ],
        "transaction_committed": True,
        "process_group_quiescent": True,
        "oauth_flow_count": 0,
        "auth_session_count": 0,
        "principal_count": 0,
        "tenant_count": 0,
        "installation_count": 0,
        "authority_version_count": 0,
        "runtime_slot_writer_fence_count": 0,
        "zsh_sha256": allowlist["zsh_sha256"],
        "security_sha256": allowlist["security_sha256"],
        "psql_sha256": allowlist["psql_sha256"],
        "static_sql_sha256": allowlist["static_sql_sha256"],
        **audit_configuration,
    }


def audited_quarantined_database_projection(database_marker):
    excluded = {
        "schema_version", "kind", "run_id", "manifest_sha256", "intent_sha256",
        "source_transition_sha256",
        "post_drain_transport_inventory_sha256", "observed_at",
    }
    return {
        name: database_marker[name]
        for name in database_marker
        if name not in excluded
    }


def validate_audited_quarantined_database_reconciliation_chain(
    context, paths, source_transition_sha256
):
    database = audited_quarantined_marker(
        paths["database_absence"], AUDITED_QUARANTINED_DATABASE_FIELDS,
        "audited_quarantined_database_evidence_invalid",
    )
    database_raw = audited_private_file_bytes(
        paths["database_absence"], {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_database_evidence_invalid",
    )
    database_sha256 = hashlib.sha256(database_raw).hexdigest()
    reconciliation = audited_quarantined_marker(
        paths["reconciliation"], AUDITED_QUARANTINED_RECONCILIATION_FIELDS,
        "audited_quarantined_reconciliation_invalid",
    )
    reconciliation_raw = audited_private_file_bytes(
        paths["reconciliation"], {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_reconciliation_invalid",
    )
    reconciliation_sha256 = hashlib.sha256(reconciliation_raw).hexdigest()
    if (
        database_sha256 != AUDITED_QUARANTINED_RECOVERY_DATABASE_SHA256
        or reconciliation_sha256
        != AUDITED_QUARANTINED_RECOVERY_RECONCILIATION_SHA256
        or database.get("source_transition_sha256") != source_transition_sha256
        or reconciliation.get("source_transition_sha256")
        != source_transition_sha256
        or reconciliation.get("database_absence_sha256") != database_sha256
        or database.get("intent_sha256") != reconciliation.get("intent_sha256")
        or database.get("run_id") != context.manifest["run_id"]
        or reconciliation.get("run_id") != context.manifest["run_id"]
        or database.get("manifest_sha256") != context.digest
        or reconciliation.get("manifest_sha256") != context.digest
    ):
        fail("audited_quarantined_database_reconciliation_chain_invalid")
    return database, database_sha256, reconciliation, reconciliation_sha256


def command_recover_audited_quarantined_no_issue(
    context,
    platform,
    initial_observations,
    bootstrap_state_path,
    confirmed_current_commit,
    confirmed_current_tree,
    confirmed_run_id,
    confirmed_manifest_sha256,
):
    if (
        COMMIT_PATTERN.fullmatch(confirmed_current_commit or "") is None
        or COMMIT_PATTERN.fullmatch(confirmed_current_tree or "") is None
        or confirmed_run_id != context.manifest["run_id"]
        or confirmed_manifest_sha256 != context.digest
    ):
        fail("audited_quarantined_confirmation_mismatch")
    allowlist = AUDITED_QUARANTINED_NO_ISSUE_ALLOWLIST.get(
        (context.manifest["run_id"], context.digest)
    )
    if allowlist is None:
        fail("audited_quarantined_identity_not_allowlisted")
    paths = audited_quarantined_recovery_paths(context)
    intent_exists = os.path.lexists(paths["intent"])
    if not intent_exists:
        fail("audited_quarantined_intent_invalid")
    require_audited_manifest_unchanged(context)
    coordinator_baseline = require_audited_quarantined_coordinator_baseline(
        context, allowlist
    )
    observations = validate_audited_source_observations(
        context, initial_observations
    )
    revision = current_clean_recovery_source()
    if (
        revision["commit_sha"] != confirmed_current_commit
        or revision["tree_sha"] != confirmed_current_tree
    ):
        fail("audited_quarantined_confirmation_mismatch")
    source_record = audited_recovery_current_source(
        context, observations, revision
    )
    bootstrap_path, bootstrap_state, bootstrap_sha256 = audited_bootstrap_state(
        context, bootstrap_state_path, quarantined_recovery=True
    )
    if audited_quarantined_bootstrap_semantic_sha256(
        bootstrap_state
    ) != allowlist["bootstrap_semantic_sha256"]:
        fail("audited_recovery_bootstrap_state_invalid")
    state, state_sha256 = audited_quarantined_state(context)
    rows, journal_raw = audited_quarantined_journal(context, allowlist)
    if not os.path.lexists(paths["reconciliation"]) and (
        bootstrap_sha256 != allowlist["bootstrap_state_sha256"]
        or state_sha256 != allowlist["orchestrator_state_sha256"]
        or state["phase"] != "candidate_started"
        or len(rows) != allowlist["journal_rows"]
        or hashlib.sha256(journal_raw).hexdigest() != allowlist["journal_sha256"]
    ):
        fail("audited_quarantined_source_transition_baseline_invalid")
    require_audited_quarantined_lifecycle(context, allowlist)
    for name, expected in (
        ("candidate-start-transition.json", allowlist["candidate_start_transition_sha256"]),
        ("database-evidence.json", allowlist["database_evidence_sha256"]),
        ("step-03-evidence.json", allowlist["step_03_evidence_sha256"]),
    ):
        raw = audited_private_file_bytes(
            context.artifact_directory / name, {0o600}, 1024 * 1024,
            "audited_quarantined_historical_artifact_invalid",
        )
        if hashlib.sha256(raw).hexdigest() != expected:
            fail("audited_quarantined_historical_artifact_invalid")
    initial_inventory = {
        "digest_sha256": allowlist["empty_transport_inventory_sha256"]
    }
    # Re-read all mutable source inputs immediately before any recovery mutation.
    second_revision = current_clean_recovery_source()
    second_observations = validate_audited_source_observations(
        context, observe_audited_recovery_source_trees(context.manifest)
    )
    if second_revision != revision or second_observations != observations:
        fail("audited_quarantined_source_changed")
    require_audited_manifest_unchanged(context)
    intent, intent_raw = audited_quarantined_load_original_intent(context, paths)
    intent_sha256 = hashlib.sha256(intent_raw).hexdigest()
    # From this point a partial stop is accepted only through the exact intent.
    require_audited_quarantined_lifecycle(context, allowlist)
    fence_path = d2a_teardown_fence_path(context)
    if not os.path.lexists(fence_path):
        fail("audited_quarantined_fence_invalid")
    fence = validate_d2a_teardown_fence(
        context,
        load_strict_d2a_marker(
            fence_path, "audited_quarantined_fence_invalid",
            D2A_TEARDOWN_FENCE_FIELDS, sorted_canonical=True,
        ),
    )
    previous_transition, previous_transition_sha256 = (
        validate_audited_quarantined_v1_source_transition(
            context, paths, intent, intent_raw, bootstrap_state, allowlist
        )
    )
    if (
        previous_transition_sha256
        != AUDITED_QUARANTINED_RECOVERY_V1_TRANSITION_SHA256
    ):
        fail("audited_quarantined_v1_source_transition_invalid")
    transition, database_source_transition_sha256 = (
        validate_audited_quarantined_historical_source_transition_v2(
            context, paths, intent_raw, bootstrap_state, allowlist,
            previous_transition
        )
    )
    third_revision = current_clean_recovery_source()
    third_observations = validate_audited_source_observations(
        context, observe_audited_recovery_source_trees(context.manifest)
    )
    if third_revision != revision or third_observations != observations:
        fail("audited_quarantined_source_changed")
    database_marker = audited_quarantined_marker(
        paths["database_absence"], AUDITED_QUARANTINED_DATABASE_FIELDS,
        "audited_quarantined_database_evidence_invalid",
    )
    database_raw = audited_private_file_bytes(
        paths["database_absence"], {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_database_evidence_invalid",
    )
    database_sha256 = hashlib.sha256(database_raw).hexdigest()
    expected_database = audited_quarantined_database_payload(
        context, allowlist, intent_sha256, database_source_transition_sha256,
        allowlist["empty_transport_inventory_sha256"],
        database_marker.get("observed_at"), transition["audit_configuration"],
    )
    if (
        database_sha256 != AUDITED_QUARANTINED_RECOVERY_DATABASE_SHA256
        or database_marker != expected_database
    ):
        fail("audited_quarantined_database_evidence_invalid")
    reconciliation = audited_quarantined_marker(
        paths["reconciliation"], AUDITED_QUARANTINED_RECONCILIATION_FIELDS,
        "audited_quarantined_reconciliation_invalid",
    )
    reconciliation_raw = audited_private_file_bytes(
        paths["reconciliation"], {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_reconciliation_invalid",
    )
    reconciliation_sha256 = hashlib.sha256(reconciliation_raw).hexdigest()
    if (
        reconciliation_sha256
        != AUDITED_QUARANTINED_RECOVERY_RECONCILIATION_SHA256
        or reconciliation.get("source_transition_sha256")
        != database_source_transition_sha256
        or reconciliation.get("database_absence_sha256") != database_sha256
    ):
        fail("audited_quarantined_reconciliation_invalid")
    require_audited_quarantined_lifecycle(context, allowlist)
    cleanup_transition_exists = os.path.lexists(paths["cleanup_transition"])
    cleanup_interlock_exists = os.path.lexists(
        paths["cleanup_transition_interlock"]
    )
    if state["phase"] != "cleaned" and not cleanup_transition_exists:
        cleanup_boundary = audited_quarantined_cleanup_transition_boundary(
            context, platform, allowlist, paths, state, rows, journal_raw
        )
        cleanup_transition, source_transition_sha256 = (
            audited_quarantined_cleanup_transition(
                context, platform, allowlist, paths, intent_raw, transition,
                previous_transition, source_record, revision, bootstrap_state,
                state, rows, journal_raw, cleanup_boundary,
            )
        )
    elif cleanup_transition_exists and cleanup_interlock_exists:
        cleanup_transition, source_transition_sha256 = (
            validate_audited_quarantined_cleanup_transition(
                context, paths, intent_raw, transition, source_record,
                revision, bootstrap_state, allowlist
            )
        )
        validate_audited_quarantined_cleanup_replay_journal(
            context, allowlist, paths, cleanup_transition, state
        )
    else:
        fail("audited_quarantined_cleanup_transition_invalid")
    cleanup_result = command_cleanup_internal(
        context, platform, retire_committed=False,
        audited_keychain_inventory_sha256=cleanup_transition[
            "keychain_inventory_sha256"
        ],
    )
    final_state = load_state(context, {"cleaned"})
    absence = cleanup_absence(
        context,
        platform,
        final_state["standing_snapshot"],
        audited_keychain=True,
    )
    if not all(absence.values()):
        fail("audited_quarantined_cleanup_incomplete")
    require_audited_quarantined_lifecycle(context, allowlist)
    final_previous_transition, final_previous_transition_sha256 = (
        validate_audited_quarantined_historical_source_transition_v2(
            context, paths, intent_raw, bootstrap_state, allowlist,
            previous_transition,
        )
    )
    final_cleanup_transition, final_source_transition_sha256 = (
        validate_audited_quarantined_cleanup_transition(
            context, paths, intent_raw, final_previous_transition,
            source_record, revision, bootstrap_state, allowlist,
        )
    )
    (
        final_database_marker,
        final_database_sha256,
        final_reconciliation,
        final_reconciliation_sha256,
    ) = validate_audited_quarantined_database_reconciliation_chain(
        context, paths, final_previous_transition_sha256
    )
    if (
        final_previous_transition != transition
        or final_previous_transition_sha256 != database_source_transition_sha256
        or final_cleanup_transition != cleanup_transition
        or final_source_transition_sha256 != source_transition_sha256
        or final_database_marker != database_marker
        or final_database_sha256 != database_sha256
        or final_reconciliation != reconciliation
        or final_reconciliation_sha256 != reconciliation_sha256
    ):
        fail("audited_quarantined_cleanup_artifact_drift")
    final_revision = current_clean_recovery_source()
    final_observations = validate_audited_source_observations(
        context, observe_audited_recovery_source_trees(context.manifest)
    )
    if final_revision != revision or final_observations != observations:
        fail("audited_quarantined_source_changed")
    fence_raw = audited_private_file_bytes(
        fence_path, {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_fence_invalid",
    )
    fence_sha256 = hashlib.sha256(fence_raw).hexdigest()
    if fence_sha256 != cleanup_transition["teardown_fence_sha256"]:
        fail("audited_quarantined_fence_invalid")
    cleanup_path = context.artifact_directory / "cleanup-evidence.json"
    cleanup_raw = audited_private_file_bytes(
        cleanup_path, {0o600}, D2A_MARKER_MAXIMUM_BYTES,
        "audited_quarantined_cleanup_evidence_invalid",
    )
    cleanup_evidence = validate_cleanup_evidence(
        context,
        load_json(
            cleanup_path, "audited_quarantined_cleanup_evidence_invalid"
        ),
    )
    if cleanup_raw != (
        canonical_json(cleanup_evidence) + "\n"
    ).encode("utf-8"):
        fail("audited_quarantined_cleanup_evidence_invalid")
    cleanup_keychain_baseline_sha256 = (
        audited_cleanup_keychain_baseline_sha256(
            context, cleanup_transition["keychain_inventory_sha256"]
        )
    )
    cleanup_root_progress_sha256 = audited_cleanup_root_progress_sha256(
        context
    )
    expected_evidence = {
        "schema_version": 1, "kind": AUDITED_QUARANTINED_NO_ISSUE_EVIDENCE_KIND,
        "run_id": context.manifest["run_id"], "manifest_sha256": context.digest,
        "intent_sha256": intent_sha256,
        "source_transition_sha256": source_transition_sha256,
        "reconciliation_sha256": reconciliation_sha256,
        "database_absence_sha256": database_sha256, "observed_at": utc_now(),
        "lifecycle_sha256": allowlist["lifecycle_sha256"],
        "teardown_fence_sha256": fence_sha256,
        "cleanup_evidence_sha256": hashlib.sha256(cleanup_raw).hexdigest(),
        "cleanup_keychain_baseline_sha256": (
            cleanup_keychain_baseline_sha256
        ),
        "cleanup_root_progress_sha256": cleanup_root_progress_sha256,
        **absence,
    }
    evidence = audited_write_once_marker(
        paths["evidence"], expected_evidence, AUDITED_QUARANTINED_EVIDENCE_FIELDS,
        "audited_quarantined_evidence_invalid",
    )
    postconditions = audited_quarantined_postconditions(
        context, platform, final_state
    )
    return {
        "status": "exact_replay" if intent_exists and os.path.lexists(paths["evidence"])
        and cleanup_result["status"] == "already_cleaned" else "recovered",
        "phase": "cleaned", "run_id": context.manifest["run_id"],
        "manifest_sha256": context.digest, "intent": str(paths["intent"]),
        "source_transition": str(paths["cleanup_transition"]),
        "source_transition_sha256": source_transition_sha256,
        "reconciliation": str(paths["reconciliation"]),
        "database_absence_evidence": str(paths["database_absence"]),
        "evidence": str(paths["evidence"]),
        "transport_instance_id": allowlist["transport_instance_id"],
        "transport_inventory_sha256": allowlist["empty_transport_inventory_sha256"],
        "database_absence_sha256": database_sha256,
        "database_absence": audited_quarantined_database_projection(database_marker),
        **absence, "source_drift_observed": True,
        "cleanup_status": cleanup_result["status"], "postconditions": postconditions,
    }


def command_status(context, platform):
    state = load_state(context)
    labels = candidate_launchd_labels(context)
    return {
        "status": "observed",
        "phase": state["phase"],
        "postgres_running": context.cluster_root.exists()
        and platform.postgres_running(context.cluster_root),
        "candidate_launchd_jobs_loaded": sum(
            1
            for service in context.manifest["services"].values()
            if platform.launchd_loaded(service["label"])
        ),
        "candidate_launchd_overrides_absent": platform.launchd_overrides_absent(
            labels
        ),
        "protected_staging_unchanged": standing_snapshot(context, platform)
        == state["standing_snapshot"],
    }


def build_parser():
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    for command in (
        "dry-run",
        "prepare",
        "start",
        "restart-drained-runtime",
        "resource-inventory",
        "teardown-discord-resources",
        "finalize-run",
        "stop",
        "cleanup",
        "status",
    ):
        child = subparsers.add_parser(command)
        child.add_argument("--manifest", required=True)
    live_restart = subparsers.add_parser("certify-live-runtime-restart")
    live_restart.add_argument("--manifest", required=True)
    live_restart.add_argument("--confirmation-file")
    onboard = subparsers.add_parser("onboard")
    onboard.add_argument("--manifest", required=True)
    onboard.add_argument("--principal-id", required=True)
    onboard.add_argument("--display-name", required=True)
    transport = subparsers.add_parser("transport-control")
    transport.add_argument("--manifest", required=True)
    transport.add_argument(
        "--operation",
        required=True,
        choices=(
            "snapshot",
            "arm-next-duplicate",
            "disarm-duplicate",
            "arm-next-indeterminate",
            "disarm-indeterminate",
            "partition-gateway",
            "heal-gateway",
        ),
    )
    evidence = subparsers.add_parser("transport-evidence")
    evidence.add_argument("--manifest", required=True)
    evidence.add_argument(
        "--checkpoint",
        required=True,
        choices=tuple(TRANSPORT_EVIDENCE_KINDS),
    )
    worker_evidence = subparsers.add_parser("worker-authoring-evidence")
    worker_evidence.add_argument("--manifest", required=True)
    worker_evidence.add_argument(
        "--checkpoint", required=True, choices=("before", "after")
    )
    worker_evidence.add_argument("--browser-evidence")
    reconciliation_observation = subparsers.add_parser(
        "reconciliation-discord-observation"
    )
    reconciliation_observation.add_argument("--manifest", required=True)
    reconciliation_observation.add_argument(
        "--database-evidence", required=True
    )
    total_absence = subparsers.add_parser("finalize-total-absence")
    total_absence.add_argument("--manifest", required=True)
    total_absence.add_argument("--prefix-scan-evidence", required=True)
    total_absence.add_argument("--guild-deletion-evidence", required=True)
    legacy_status = subparsers.add_parser("legacy-substrate-status")
    legacy_status.add_argument("--manifest", required=True)
    legacy_recovery = subparsers.add_parser("recover-legacy-substrate")
    legacy_recovery.add_argument("--manifest", required=True)
    legacy_recovery.add_argument("--confirm-run-id", required=True)
    legacy_recovery.add_argument("--confirm-manifest-sha256", required=True)
    audited_recovery = subparsers.add_parser(
        "recover-audited-preissuer-rollback"
    )
    audited_recovery.add_argument("--manifest", required=True)
    audited_recovery.add_argument("--bootstrap-state", required=True)
    audited_recovery.add_argument("--confirm-current-commit", required=True)
    audited_recovery.add_argument("--confirm-current-tree", required=True)
    audited_recovery.add_argument("--confirm-run-id", required=True)
    audited_recovery.add_argument("--confirm-manifest-sha256", required=True)
    quarantined_recovery = subparsers.add_parser(
        "recover-audited-quarantined-no-issue"
    )
    quarantined_recovery.add_argument("--manifest", required=True)
    quarantined_recovery.add_argument("--bootstrap-state", required=True)
    quarantined_recovery.add_argument("--confirm-current-commit", required=True)
    quarantined_recovery.add_argument("--confirm-current-tree", required=True)
    quarantined_recovery.add_argument("--confirm-run-id", required=True)
    quarantined_recovery.add_argument(
        "--confirm-manifest-sha256", required=True
    )
    return parser


def main():
    arguments = build_parser().parse_args()
    try:
        audited_source_observations = None
        if arguments.command in {
            "legacy-substrate-status",
            "recover-legacy-substrate",
        }:
            context, legacy_state = load_legacy_context(
                require_absolute_path(arguments.manifest, "manifest")
            )
        elif arguments.command in {
            "recover-audited-preissuer-rollback",
            "recover-audited-quarantined-no-issue",
        }:
            context, audited_source_observations = load_audited_recovery_context(
                require_absolute_path(arguments.manifest, "manifest")
            )
            legacy_state = None
        else:
            context = load_context(
                require_absolute_path(arguments.manifest, "manifest")
            )
            legacy_state = None
        platform = Platform()
        handlers = {
            "dry-run": command_dry_run,
            "prepare": command_prepare,
            "start": command_start,
            "restart-drained-runtime": command_restart_drained_runtime,
            "resource-inventory": command_resource_inventory,
            "teardown-discord-resources": command_teardown_discord_resources,
            "stop": command_stop,
            "cleanup": command_cleanup,
            "status": command_status,
        }
        with global_operation_lock():
            if arguments.command not in {
                "legacy-substrate-status",
                "recover-legacy-substrate",
                "recover-audited-preissuer-rollback",
                "recover-audited-quarantined-no-issue",
                "finalize-run",
                "finalize-total-absence",
                "stop",
                "cleanup",
                "status",
            } and finalization_freeze_committed(context):
                fail("orchestrator_phase_invalid")
            if arguments.command not in {
                "legacy-substrate-status",
                "recover-legacy-substrate",
                "recover-audited-preissuer-rollback",
                "recover-audited-quarantined-no-issue",
                "teardown-discord-resources",
                "stop",
                "cleanup",
                "status",
            }:
                require_candidate_start_not_retired(context)
            if arguments.command == "legacy-substrate-status":
                result = command_legacy_substrate_status(
                    context, legacy_state, platform
                )
            elif arguments.command == "recover-legacy-substrate":
                result = command_recover_legacy_substrate(
                    context,
                    legacy_state,
                    platform,
                    arguments.confirm_run_id,
                    arguments.confirm_manifest_sha256,
                )
            elif arguments.command == "recover-audited-preissuer-rollback":
                result = command_recover_audited_preissuer_rollback(
                    context,
                    platform,
                    audited_source_observations,
                    arguments.bootstrap_state,
                    arguments.confirm_current_commit,
                    arguments.confirm_current_tree,
                    arguments.confirm_run_id,
                    arguments.confirm_manifest_sha256,
                )
            elif arguments.command == "recover-audited-quarantined-no-issue":
                with d2_run.coordinator_lock(context.manifest_path, True):
                    result = command_recover_audited_quarantined_no_issue(
                        context,
                        platform,
                        audited_source_observations,
                        arguments.bootstrap_state,
                        arguments.confirm_current_commit,
                        arguments.confirm_current_tree,
                        arguments.confirm_run_id,
                        arguments.confirm_manifest_sha256,
                    )
            elif arguments.command == "onboard":
                result = command_onboard(
                    context, platform, arguments.principal_id, arguments.display_name
                )
            elif arguments.command == "transport-control":
                result = command_transport_control(
                    context,
                    platform,
                    arguments.operation,
                )
            elif arguments.command == "transport-evidence":
                result = command_transport_evidence(
                    context,
                    platform,
                    arguments.checkpoint,
                )
            elif arguments.command == "worker-authoring-evidence":
                result = command_worker_authoring_evidence(
                    context,
                    platform,
                    arguments.checkpoint,
                    arguments.browser_evidence,
                )
            elif arguments.command == "reconciliation-discord-observation":
                result = command_reconciliation_discord_observation(
                    context,
                    platform,
                    arguments.database_evidence,
                )
            elif arguments.command == "finalize-run":
                result = command_finalize_run(
                    context,
                    platform,
                    command_teardown_discord_resources,
                )
            elif arguments.command == "finalize-total-absence":
                result = command_finalize_total_absence(
                    context,
                    platform,
                    arguments.prefix_scan_evidence,
                    arguments.guild_deletion_evidence,
                )
            elif arguments.command == "certify-live-runtime-restart":
                confirmation_path = (
                    None
                    if arguments.confirmation_file is None
                    else require_absolute_path(
                        arguments.confirmation_file, "confirmation_file"
                    )
                )
                result = command_certify_live_runtime_restart(
                    context, platform, confirmation_path
                )
            else:
                result = handlers[arguments.command](context, platform)
        print(canonical_json(result))
    except (CertificationError, OrchestratorError) as error:
        print(canonical_json({"status": "failed", "code": str(error)}), file=sys.stderr)
        raise SystemExit(1)
    except KeyboardInterrupt:
        print(
            canonical_json({"status": "failed", "code": "d2_operation_interrupted"}),
            file=sys.stderr,
        )
        raise SystemExit(130)


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, lambda _signal, _frame: (_ for _ in ()).throw(KeyboardInterrupt()))
    main()
