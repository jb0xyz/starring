#!/usr/bin/env python3

"""Create, exercise, and retire one isolated D2A candidate.

This is deliberately a bootstrap around the existing D2 tools, not another
orchestrator.  It never invokes the commercial coordinator or D3.  A valid D2
manifest is permanently tainted before the first orchestrator observation and
all Discord cleanup is delegated to the run-owned resource teardown command.
"""

import argparse
import contextlib
import datetime
import fcntl
import hashlib
import importlib.util
import json
import os
import pathlib
import re
import selectors
import secrets
import shutil
import signal
import stat
import subprocess
import sys
import time
import unicodedata
import uuid


TOOL_ROOT = pathlib.Path(__file__).resolve().parent
CERTIFICATION_ROOT = TOOL_ROOT.parent / "d2-certification"
_D2A_SPEC = importlib.util.spec_from_file_location("starring_d2a_controller", TOOL_ROOT / "d2a.py")
D2A = importlib.util.module_from_spec(_D2A_SPEC)
_D2A_SPEC.loader.exec_module(D2A)

SCHEMA_VERSION = 1
CONFIG_KIND = "starring.d2a.persistent-sandbox-config.v1"
CANDIDATE_SCHEMA_VERSION = 2
CANDIDATE_KIND = "starring.d2a.candidate-spec.v2"
CANDIDATE_PROVENANCE_KIND = "starring.d2a.candidate-provenance.v1"
CANDIDATE_DEPENDENCY_SNAPSHOT_KIND = "starring.d2a.candidate-dependency-snapshot.v1"
STATE_KIND = "starring.d2a.bootstrap-state.v1"
RESULT_KIND = "starring.d2a.bootstrap-result.v1"
GLOBAL_LOCK_PATH = pathlib.Path("/private/tmp/starring-d2a-bootstrap.lock")
ISSUER_GLOBAL_D2_LOCK_PATH = pathlib.Path("/private/tmp/starring-d2-certification.lock")
DEFAULT_RUST_TOOLCHAIN_BIN = (
    pathlib.Path.home()
    / ".rustup"
    / "toolchains"
    / "stable-aarch64-apple-darwin"
    / "bin"
)
DEFAULT_RELEASE_RUN_ROOT = (
    pathlib.Path.home()
    / "Library"
    / "Application Support"
    / "Starring"
    / "release-certifications"
)
FIXED_GIT = pathlib.Path("/usr/bin/git")
FIXED_XCRUN = pathlib.Path("/usr/bin/xcrun")
FIXED_XCODE_SELECT = pathlib.Path("/usr/bin/xcode-select")
FIXED_SW_VERS = pathlib.Path("/usr/bin/sw_vers")
FIXED_SYSCTL = pathlib.Path("/usr/sbin/sysctl")
FIXED_LINKERS = tuple(
    pathlib.Path(path)
    for path in ("/usr/bin/cc", "/usr/bin/clang", "/usr/bin/ld", "/usr/bin/ar", "/usr/bin/ranlib")
)
MAX_INPUT_BYTES = 64 * 1024
MAX_OUTPUT_BYTES = 1024 * 1024
MAX_COMMAND_ERROR_BYTES = 4 * 1024
COMMAND_ERROR_KIND = "starring.d2a.command-error.v1"
COMMAND_ERROR_FIELDS = {
    "error_code",
    "kind",
    "operation",
    "schema_version",
}
ONE_SHOT_COMMAND_ERROR_CODES = frozenset({
    "d2a_one_shot_dependency_timeout",
    "d2a_one_shot_dependency_unavailable",
    "d2a_one_shot_product_request_failed",
})
TEARDOWN_DIAGNOSTIC_FIELDS = {"code", "status"}
TEARDOWN_DIAGNOSTIC_CODES = frozenset({
    "candidate_health_unready",
    "transport_control_unavailable",
    "discord_resource_teardown_final_drift",
})
MAX_CARGO_MANIFEST_BYTES = 1024 * 1024
MAX_CARGO_LOCK_BYTES = 16 * 1024 * 1024
MAX_CANDIDATE_DEPENDENCY_ENTRIES = 500000
MAX_CANDIDATE_DEPENDENCY_BYTES = 4 * 1024 * 1024 * 1024
BUILD_TIMEOUT_SECONDS = 60 * 60
COMMAND_TIMEOUT_SECONDS = 20 * 60
TOOL_TIMEOUT_SECONDS = 60
PROCESS_GROUP_GRACE_SECONDS = 2
RUN_ID = re.compile(r"^d2-[0-9]{8}t[0-9]{6}z-[0-9a-f]{12}$")
DIGEST = re.compile(r"^[0-9a-f]{64}$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")
SNOWFLAKE = re.compile(r"^[1-9][0-9]{0,19}$")
IDENTIFIER = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,191}$")
SANDBOX_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
ERROR_CODE = re.compile(r"^[a-z][a-z0-9_]{0,95}$")
UTC_TIMESTAMP = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?Z$"
)

CONFIG_FIELDS = {
    "schema_version",
    "kind",
    "sandbox_id",
    "guild_lifecycle",
    "discord",
    "credential_refs",
    "cloudflare",
    "ports",
    "release_run_root",
    "d2a_result_root",
    "bootstrap_state_root",
}
DISCORD_FIELDS = {
    "guild_id",
    "hub_channel_id",
    "application_id",
    "bot_user_id",
    "actor_id",
    "actor_display_name",
}
CREDENTIAL_FIELDS = {"discord_oauth", "discord_bot", "cloudflare_tunnel"}
CLOUDFLARE_FIELDS = {"tunnel_id", "public_origin"}
PORT_FIELDS = {
    "postgres",
    "api",
    "runtime",
    "worker",
    "transport_gateway",
    "transport_http",
}
CANDIDATE_FIELDS = {
    "schema_version",
    "kind",
    "commit_sha",
    "source_tree_sha",
    "bundle",
    "provenance_sha256",
    "candidates",
}
CANDIDATE_RECORD_FIELDS = {"path", "sha256"}
CANDIDATE_PROVENANCE_FIELDS = {
    "schema_version", "kind", "status", "release_eligible",
    "commercial_certification", "source", "commands", "environment",
    "dependencies", "toolchain", "artifacts", "worker", "operators",
    "bundle", "builder", "built_at",
}
CANDIDATE_PROVENANCE_SOURCE_FIELDS = {"root", "commit", "tree", "clean", "git"}
CANDIDATE_PROVENANCE_ENVIRONMENT_FIELDS = {
    "AR", "CARGO_BUILD_JOBS", "CARGO_HOME", "CARGO_INCREMENTAL",
    "CARGO_NET_OFFLINE", "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER",
    "CC", "CXX", "GIT_CONFIG_NOSYSTEM", "GIT_TERMINAL_PROMPT", "HOME", "LC_ALL", "PATH",
    "RANLIB", "RUSTC", "SDKROOT", "STARRING_RUNTIME_BUILD_REVISION", "TMPDIR",
}
CANDIDATE_IDENTITY_FIELDS = {"path", "sha256", "size", "mode", "uid", "links"}
CANDIDATE_DEPENDENCY_RECORD_FIELDS = {
    "schema_version", "kind", "gate_runtime_sha256", "entries",
    "total_bytes", "tree_sha256", "record_sha256",
}
CANDIDATE_DEPENDENCY_SNAPSHOT_FIELDS = {
    "schema_version", "kind", "bootstrap_root", "record",
    "gate_runtime_sha256", "record_sha256", "tree_sha256", "entries",
    "total_bytes", "workspace", "transport", "source_inputs",
}
CANDIDATE_DEPENDENCY_SOURCE_FIELDS = {"vendor_root", "cargo_config"}
CANDIDATE_DEPENDENCY_SOURCE_INPUTS = {
    "workspace_manifest": pathlib.Path("Cargo.toml"),
    "workspace_lock": pathlib.Path("Cargo.lock"),
    "transport_manifest": pathlib.Path("tools/d2-certification-transport/Cargo.toml"),
    "transport_lock": pathlib.Path("tools/d2-certification-transport/Cargo.lock"),
}
CANDIDATE_DEPENDENCY_SOURCE_LIMITS = {
    "workspace_manifest": MAX_CARGO_MANIFEST_BYTES,
    "workspace_lock": MAX_CARGO_LOCK_BYTES,
    "transport_manifest": MAX_CARGO_MANIFEST_BYTES,
    "transport_lock": MAX_CARGO_LOCK_BYTES,
}
CANDIDATE_TOOL_IDENTITY_FIELDS = CANDIDATE_IDENTITY_FIELDS | {"version"}
CANDIDATE_DARWIN_SELECTED_FIELDS = {
    "selected_path", "selected_link_target", "resolved_path", "sha256",
    "size", "mode",
}
CANDIDATE_DARWIN_FIXED_FIELDS = CANDIDATE_IDENTITY_FIELDS
CANDIDATE_ARTIFACT_NAMES = {
    "api",
    "certification_transport",
    "db_bootstrap",
    "runtime",
    "sealed_provisioner",
}
CANDIDATE_OPERATOR_NAMES = {"cloudflared", "codex", "node"}
CANDIDATE_RELATIVE_PATHS = {
    "api": pathlib.Path("starring-api"),
    "certification_transport": pathlib.Path("d2-certification-transport"),
    "cloudflared": pathlib.Path("cloudflared"),
    "codex": pathlib.Path("codex"),
    "codex_worker": pathlib.Path("codex-worker/worker.mjs"),
    "db_bootstrap": pathlib.Path("starring-d2-db-bootstrap"),
    "node": pathlib.Path("node"),
    "runtime": pathlib.Path("starring-runtime"),
    "sealed_provisioner": pathlib.Path("starring-d2-sealed-provisioner"),
}
CODEX_WORKER_FILES = {
    "admission-registry.mjs",
    "codex-runner.mjs",
    "metrics-log.mjs",
    "protocol.mjs",
    "request-timeline.mjs",
    "scheduler.mjs",
    "worker.mjs",
}
TOOL_DIGEST_FIELDS = {
    "cargo_sha256",
    "rustc_sha256",
    "rust_sysroot_sha256",
    "rust_linkers_sha256",
    "darwin_toolchain_sha256",
    "macos_sdk_sha256",
    "issuer_build_environment_sha256",
    "issuer_sha256",
    "issuer_source_sha256",
    "runner_sha256",
    "product_driver_sha256",
    "scenario_sha256",
}
ISSUER_BUILD_ENVIRONMENT_FIELDS = {
    "AR", "CARGO_HOME", "CARGO_INCREMENTAL", "CARGO_NET_OFFLINE",
    "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER", "CC", "CXX",
    "GIT_CONFIG_NOSYSTEM", "GIT_TERMINAL_PROMPT", "HOME", "LC_ALL",
    "PATH", "RANLIB", "RUSTC", "SDKROOT",
}
RECORD_FIELDS = {"operation", "path", "sha256", "verified"}
DIRECT_ONBOARDING_FIELDS = {
    "schema_version",
    "kind",
    "certification_class",
    "operation",
    "observed_at",
    "run_id",
    "manifest_sha256",
    "principal_id",
    "guild_id",
    "discord_application_id",
    "hub_channel_id",
    "binding_key",
    "installation_id",
    "outcome",
    "provisioner_sha256",
    "issuer_sha256",
    "issuer_source_sha256",
    "discord_hub_preflight",
    "direct_auth_used",
    "session_revoked",
    "release_eligible",
}
COMMERCIAL_ONBOARDING_ARTIFACTS = (
    pathlib.Path("orchestrator/onboarding-evidence.json"),
    pathlib.Path("orchestrator/coordinator-sources/step-04-onboarding.json"),
)
SESSION_LIFECYCLE_FIELDS = (
    "schema_version", "kind", "run_id", "manifest_sha256", "operation",
    "origin", "issuer_sha256", "issuer_source_sha256", "uid", "boot_identity",
    "process_group_id", "started_at", "status", "session_revoked",
    "revoked_at", "quarantined_at",
)
BOOT_IDENTITY = re.compile(r"^darwin-boottime:[0-9]+:[0-9]+$")
SESSION_LIFECYCLE_TIMESTAMP = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{9}Z$"
)
AUDITED_QUARANTINED_RECOVERY_COMMAND = (
    "recover-audited-quarantined-no-issue"
)
AUDITED_QUARANTINED_RECOVERY_FILENAMES = {
    "intent": "audited-quarantined-no-issue-recovery-intent.json",
    "source_transition": (
        "audited-quarantined-no-issue-recovery-cleanup-transition.json"
    ),
    "reconciliation": "audited-quarantined-no-issue-reconciliation.json",
    "database_absence": "audited-quarantined-no-issue-database-absence.json",
    "evidence": "audited-quarantined-no-issue-recovery.json",
}
AUDITED_QUARANTINED_RECOVERY_KINDS = {
    "intent": "starring.d2.audited-quarantined-no-issue-recovery-intent.v1",
    "source_transition": (
        "starring.d2.audited-quarantined-no-issue-recovery-cleanup-transition.v1"
    ),
    "reconciliation": "starring.d2.audited-quarantined-no-issue-reconciliation.v1",
    "database_absence": "starring.d2.audited-quarantined-no-issue-database-absence.v1",
    "evidence": "starring.d2.audited-quarantined-no-issue-recovery.v1",
}
AUDITED_QUARANTINED_SOURCE_TRANSITION_V1_FILENAME = (
    "audited-quarantined-no-issue-recovery-source-transition.json"
)
AUDITED_QUARANTINED_SOURCE_TRANSITION_V1_KIND = (
    "starring.d2.audited-quarantined-no-issue-recovery-source-transition.v1"
)
AUDITED_QUARANTINED_SOURCE_INTERLOCK_V1_FILENAME = (
    "audited-quarantined-no-issue-recovery-source-transition-interlock.json"
)
AUDITED_QUARANTINED_SOURCE_INTERLOCK_V1_KIND = (
    "starring.d2.audited-quarantined-no-issue-recovery-source-transition-"
    "interlock.v1"
)
AUDITED_QUARANTINED_SOURCE_INTERLOCK_V2_FILENAME = (
    "audited-quarantined-no-issue-recovery-source-transition-v2-interlock.json"
)
AUDITED_QUARANTINED_SOURCE_INTERLOCK_V2_KIND = (
    "starring.d2.audited-quarantined-no-issue-recovery-source-transition-"
    "interlock.v2"
)
AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_FILENAME = (
    "audited-quarantined-no-issue-recovery-source-transition-v2.json"
)
AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_KIND = (
    "starring.d2.audited-quarantined-no-issue-recovery-source-transition.v2"
)
AUDITED_QUARANTINED_SOURCE_INTERLOCK_FILENAME = (
    "audited-quarantined-no-issue-recovery-cleanup-transition-interlock.json"
)
AUDITED_QUARANTINED_SOURCE_INTERLOCK_KIND = (
    "starring.d2.audited-quarantined-no-issue-recovery-cleanup-transition-"
    "interlock.v1"
)
AUDITED_QUARANTINED_INTENT_FIELDS = {
    "schema_version", "kind", "run_id", "manifest_sha256", "bootstrap_id",
    "bootstrap_state_path", "baseline_bootstrap_state_sha256",
    "historical_manifest_commit_sha", "historical_source_trees",
    "current_source", "orchestrator_state_sha256",
    "baseline_journal_sha256", "baseline_journal_rows", "taint_sha256",
    "lifecycle_sha256", "candidate_start_transition_sha256",
    "database_evidence_sha256", "step_03_evidence_sha256",
    "transport_instance_id", "pre_intent_transport_inventory_sha256",
    "effect_admission_operation_id", "database_system_identifier",
    "safe_after", "service_identities", "audit_tools", "created_at",
    "receipts_sha256", "coordinator_lock_sha256", "coordinator_source_sha256",
    "plist_sha256", "tunnel_script_sha256",
}
AUDITED_QUARANTINED_RECONCILIATION_FIELDS = {
    "schema_version", "kind", "run_id", "manifest_sha256", "intent_sha256",
    "source_transition_sha256",
    "observed_at", "lifecycle_sha256", "transport_instance_id",
    "pre_intent_transport_inventory_sha256", "effect_admission_operation_id",
    "effect_admission_status", "producer_launchd_jobs_absent",
    "issuer_process_group_absent", "post_drain_transport_inventory_sha256",
    "database_absence_sha256", "final_transport_inventory_sha256",
    "postgres_running", "protected_staging_unchanged",
}
AUDITED_QUARANTINED_DATABASE_ABSENCE_FIELDS = {
    "schema_version", "kind", "run_id", "manifest_sha256", "intent_sha256",
    "source_transition_sha256",
    "observed_at", "post_drain_transport_inventory_sha256", "database_name",
    "database_system_identifier", "control_plane_identity", "topology_verified",
    "tables_locked", "locked_tables", "transaction_committed",
    "process_group_quiescent", "zsh_sha256", "security_sha256",
    "psql_sha256", "static_sql_sha256", "oauth_flow_count",
    "auth_session_count", "principal_count", "tenant_count",
    "installation_count", "authority_version_count",
    "runtime_slot_writer_fence_count",
    "login_keychain_path", "login_keychain_policy_kind",
    "login_keychain_policy_sha256", "login_keychain_policy_verified",
}
AUDITED_QUARANTINED_DATABASE_PROJECTION_FIELDS = {
    "database_name", "database_system_identifier", "control_plane_identity",
    "topology_verified", "tables_locked", "locked_tables", "transaction_committed",
    "process_group_quiescent", "zsh_sha256", "security_sha256",
    "psql_sha256", "static_sql_sha256",
    "oauth_flow_count", "auth_session_count", "principal_count", "tenant_count",
    "installation_count", "authority_version_count",
    "runtime_slot_writer_fence_count",
    "login_keychain_path", "login_keychain_policy_kind",
    "login_keychain_policy_sha256", "login_keychain_policy_verified",
}
AUDITED_QUARANTINED_EVIDENCE_FIELDS = {
    "schema_version", "kind", "run_id", "manifest_sha256", "intent_sha256",
    "source_transition_sha256",
    "reconciliation_sha256", "database_absence_sha256", "observed_at",
    "lifecycle_sha256", "teardown_fence_sha256", "cleanup_evidence_sha256",
    "cleanup_keychain_baseline_sha256", "cleanup_root_progress_sha256",
    "database_absent", "postgres_process_absent", "launchd_jobs_absent",
    "keychain_items_absent", "isolated_root_absent",
    "protected_staging_unchanged",
}
AUDITED_QUARANTINED_OUTPUT_FIELDS = {
    "status", "phase", "run_id", "manifest_sha256", "intent",
    "source_transition", "source_transition_sha256",
    "reconciliation", "database_absence_evidence", "evidence", "transport_instance_id",
    "transport_inventory_sha256", "database_absence_sha256",
    "database_absence", "postconditions", "database_absent", "postgres_process_absent",
    "launchd_jobs_absent", "keychain_items_absent", "isolated_root_absent",
    "protected_staging_unchanged", "source_drift_observed", "cleanup_status",
}
AUDITED_QUARANTINED_SOURCE_TRANSITION_V1_BASE_FIELDS = {
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
}
AUDITED_QUARANTINED_SOURCE_INTERLOCK_V1_FIELDS = set(
    AUDITED_QUARANTINED_SOURCE_TRANSITION_V1_BASE_FIELDS
)
AUDITED_QUARANTINED_SOURCE_TRANSITION_V1_FIELDS = {
    *AUDITED_QUARANTINED_SOURCE_TRANSITION_V1_BASE_FIELDS,
    "interlock_sha256",
}
AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_BASE_FIELDS = {
    *AUDITED_QUARANTINED_SOURCE_TRANSITION_V1_BASE_FIELDS,
    "previous_source_transition_sha256",
}
AUDITED_QUARANTINED_SOURCE_INTERLOCK_V2_FIELDS = set(
    AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_BASE_FIELDS
)
AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_FIELDS = {
    *AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_BASE_FIELDS,
    "interlock_sha256",
}
AUDITED_QUARANTINED_SOURCE_TRANSITION_BASE_FIELDS = {
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
    "isolated_root_retained", "cleanup_keychain_baseline_absent",
    "cleanup_root_progress_absent", "cleanup_evidence_absent",
    "protected_staging_unchanged", "created_at",
}
AUDITED_QUARANTINED_SOURCE_INTERLOCK_FIELDS = set(
    AUDITED_QUARANTINED_SOURCE_TRANSITION_BASE_FIELDS
)
AUDITED_QUARANTINED_SOURCE_TRANSITION_FIELDS = {
    *AUDITED_QUARANTINED_SOURCE_TRANSITION_BASE_FIELDS,
    "interlock_sha256",
}
AUDITED_QUARANTINED_CHANGED_PATHS = [
    "tools/d2-certification/d2_orchestrator_platform.py",
    "tools/d2-certification/isolated_orchestrator.py",
    "tools/d2-certification/test_isolated_orchestrator.py",
    "tools/d2-maintenance/d2a_bootstrap.py",
    "tools/d2-maintenance/test_d2a_bootstrap.py",
]
AUDITED_QUARANTINED_CHANGED_PATH_FROM_SHA256 = {
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
AUDITED_QUARANTINED_REASON_CODES_V1 = [
    "darwin_service_group_eperm_observation",
    "explicit_login_keychain_lookup_required",
    "zsh_readonly_status_assignment_fixed",
]
AUDITED_QUARANTINED_REASON_CODES_V2 = [
    "postgres_inet_host_address_normalization_required",
]
AUDITED_QUARANTINED_REASON_CODES = [
    "cleanup_keychain_owner_account_import_required",
]
AUDITED_QUARANTINED_LOGIN_KEYCHAIN_PATH = (
    "/Users/jungbogeon/Library/Keychains/login.keychain-db"
)
AUDITED_QUARANTINED_LOGIN_KEYCHAIN_POLICY_KIND = (
    "starring.d2.keychain-path-policy.v1"
)
AUDITED_QUARANTINED_LOGIN_KEYCHAIN_POLICY_SHA256 = (
    "95a270a538f67d82a235593400bc7baf9ee8f6cb6b40665e6388398389113b69"
)
AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_STATIC_SQL_SHA256 = (
    "669c4adf949acd954a44ba1205ff19de18664f599ca830efc5d60367710ec5e8"
)
AUDITED_QUARANTINED_KEYCHAIN_ANCHOR_POLICY_KIND = (
    "starring.d2.keychain-persistent-reference-anchor-policy.v1"
)
AUDITED_QUARANTINED_KEYCHAIN_ANCHORS = [
    {
        "service": "starring.d2.credentials",
        "account": "discord.bot-token",
        "identity_sha256": (
            "967a3764ce452acf80391d8cf151d23d6f9de65f171ddceeba3e0f9109e3ad8d"
        ),
    },
    {
        "service": "starring.d2.credentials",
        "account": "discord.oauth-client-secret",
        "identity_sha256": (
            "5552c98317f8c54368423fc3cbf2ab9dbd38db7d15ec8934aa0de1db3d645b05"
        ),
    },
    {
        "service": "starring.d2.credentials",
        "account": "cloudflare.tunnel-token",
        "identity_sha256": (
            "b307682daf62b6f5df76512dbb679ca6b1b8b4817f5616404eefd4820e21dc7f"
        ),
    },
]
AUDITED_QUARANTINED_KEYCHAIN_ANCHOR_INVENTORY_SHA256 = (
    "a89603986948be5df6498edcdfb98ec3c9e28c130af7182c832cbeda173e4b08"
)
AUDITED_QUARANTINED_KEYCHAIN_ACCOUNTS = {
    "api": (
        "database.oauth-flow-writer", "database.session-issuer",
        "database.session-api", "database.security-revoker",
        "database.installation-authority-reader",
        "database.authorized-snapshot-reader", "database.promotion-executor",
        "database.decision-reader", "database.approval-executor",
        "database.rejection-executor", "database.apply-executor",
        "database.cancellation-executor", "database.deployment-status-reader",
        "database.operational-deployment-status-reader",
        "database.authoring-session-writer", "keyring.product-action",
        "keyring.snapshot-envelope", "lifecycle-owner",
    ),
    "runtime": (
        "database.execution", "database.exact-target", "database.panel",
        "database.serving", "database.interaction",
        "interaction.token-envelope-keyring", "lifecycle-owner",
    ),
    "postgres": ("database.cluster-admin", "lifecycle-owner"),
    "worker": ("authoring.bearer-token", "lifecycle-owner"),
}
AUDITED_QUARANTINED_ABSENCE_FIELDS = {
    "database_absent", "postgres_process_absent", "launchd_jobs_absent",
    "keychain_items_absent", "isolated_root_absent",
    "protected_staging_unchanged",
}
TRANSPORT_INSTANCE_ID = re.compile(r"^d2ti-[0-9a-f]{32}$")
DATABASE_SYSTEM_IDENTIFIER = re.compile(r"^[1-9][0-9]{0,19}$")
SAFE_RECOVERY_TIMESTAMP = re.compile(
    r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?Z$"
)
AUDITED_QUARANTINED_LOCKED_TABLES = [
    "public.product_oauth_flows",
    "public.product_auth_sessions",
    "public.product_principals",
    "public.product_tenants",
    "public.automation_installations",
    "public.automation_installation_authority_versions",
    "public.runtime_slot_writer_fences_v2",
    "public.product_control_plane_identity",
]
AUDITED_QUARANTINED_INCIDENT = {
    "run_id": "d2-20260812t082042z-c52d220457d1",
    "manifest_sha256": "a522e5c316f58f54df8c0ea69ab0f6aebb9ed85e7bb6ce45a2c02b5e63823338",
    "intent_sha256": "319d84e19680bf15d20f094d9f24bf90c82e88d2bbf0133e56295774c6724a96",
    "intent_source_commit_sha": "5153709b7b0fd5f1a1ed1a8aebd19fc865d14d4c",
    "intent_source_tree_sha": "9cbfedaa35d56c65230406cae2680ba90d491c23",
    "intent_source_sha256": "6b17a41e22d458e831ee8d0d3d25d85cd640b289412598430499152fec4fba45",
    "source_transition_v1_interlock_sha256": "4e7687fb0b9db69b44e62c974a68318a0f85765292a9c3c5fc88978a61c442ca",
    "source_transition_v1_sha256": "d63408478cedc7ab39835797c1d76a00f7cf9703eda3b41db1b4d960ae1c900d",
    "source_transition_v1_commit_sha": "a323333061c76645d83539714250286c590960ef",
    "source_transition_v1_tree_sha": "8b238fd8153a57605d40739c79b182da9c52bec6",
    "source_transition_v1_to_file_sha256": {
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
    },
    "source_transition_v2_static_sql_sha256": (
        AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_STATIC_SQL_SHA256
    ),
    "source_transition_v2_interlock_sha256": (
        "ab387a6e14beeccfb7ec94f4af7f694e2eb93553da723e3c16ed6a82faad54ca"
    ),
    "source_transition_v2_sha256": (
        "4a843deda6ff27e82e0bdebe739b1c838b0eca24fff2a39d3118cf4726b0d22b"
    ),
    "database_absence_sha256": (
        "c76600c617e1abfdfbb83d5e10eb390a24e37b6ee05f78fbd10ecce204e6087d"
    ),
    "reconciliation_sha256": (
        "66e27795c9d8276a6ee49547f9d39f8c9649aa74e2c98f5d893bd6be9528286a"
    ),
    "source_transition_v2_commit_sha": (
        "894a87c74d3db4ce06e314f87d11eb1d2d1392ec"
    ),
    "source_transition_v2_tree_sha": (
        "dd7a71c3d967e8c1df66f60aff5e40d72fb67c4f"
    ),
    "source_transition_v2_to_file_sha256": {
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
    },
    "closed_teardown_fence_sha256": (
        "712b23bd7edf50c5c56a334557185d21af738891dc88dd2d56450886ac4e24a1"
    ),
    "cleanup_journal_sha256": (
        "3bfa38faa428a18aee1b92cb8eb04f868693650e3da4ab3c4f548efb1df14b3b"
    ),
    "cleanup_journal_rows": 46,
    "cleanup_root_identity_sha256": (
        "a1ca6a17183f71ea88478671033bc9247facf13c8d0524a6e8b559845f546fa3"
    ),
    "keychain_inventory_sha256": (
        "8f0cb1f256b4ee59c4842b35d3a0409f604d90170728bfb7afcaad14847e90ae"
    ),
    "keychain_item_count": 29,
    "keychain_anchor_policy_kind": (
        AUDITED_QUARANTINED_KEYCHAIN_ANCHOR_POLICY_KIND
    ),
    "keychain_anchor_inventory_sha256": (
        AUDITED_QUARANTINED_KEYCHAIN_ANCHOR_INVENTORY_SHA256
    ),
    "keychain_anchor_item_count": 3,
    "bootstrap_state_sha256": "49bf6c2fc87ee24fadec3d49707c8b2a438590a091eac7e2f96155155e507c85",
    "bootstrap_state_semantic_sha256": "e9497355f1d7e97f3dd3245f95c96a24bf70f774ea72989b37750c7b912ab952",
    "orchestrator_state_sha256": "ad80ebfbf5b2d3fa1424709b8ee07a5cf7859d6966df3ab1c6c7ed0183482016",
    "journal_sha256": "94167d8dcbe70e4c2e065472bdcc52b1bdabe77b2409e9985cdd5254480e0cd3",
    "journal_rows": 44,
    "taint_sha256": "0addc12e7574d261aaaa4978b7f9214b274ada5e371432e447d3b67a07b3cdac",
    "lifecycle_sha256": "a1de160581e7da4475f4b722b90a802ad8c4ef22009a99e57adfe3c64d940f55",
    "closing_teardown_fence_sha256": "ff8ce5b477b3ff49abc3afd0ee2234457161e418c4fbfac1444117d0a3845908",
    "candidate_start_transition_sha256": "e6dbdfdeba98f8b0a7273a9319281ed36e9045e2b8862431706fcf81aa45d35f",
    "database_evidence_sha256": "47c3f2a24f5c62db72cf0310745b5026d6973455263b379af0a61d0a8354ce12",
    "step_03_evidence_sha256": "c739fdc3fc8f6efe5b9c66a8ad308902b0afeb6fbd9265e2ae190e1cdc163893",
    "transport_instance_id": "d2ti-f3616791d71f2fdd92ceb1ec14b60cd5",
    "empty_transport_inventory_sha256": "74f1658b2a815d8100552f69b0b58d6eaefae99e76be80e2297c8d9f6cf1ebc4",
    "database_system_identifier": "7673057195867924427",
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
    "plist_sha256": {
        "api": "b3eb9bf355855f40eef980fe9bf0132bbcf46f22b3457b57dc4c690be77c82a5",
        "runtime": "8918a467ec404418df5d89e3c44035c5b4aef0d14e8b0f2b0b210fd8ea66488a",
        "transport": "0a738931d2dde05d41aec4e9076fa6de4a35d5733fdaf711e9e78fa33fd15126",
        "tunnel": "14cdc74f88231b638c7ef0505c8ee267744a4f9313af203a81f72716f6177386",
        "worker": "38e60569fa74051fcc79f2e346da802fe8b9c6b72da2818e959332301ef9860a",
    },
    "tunnel_script_sha256": "526c9b27c65a350a754221abae92450d052306872dc166503d9aff1127ce19a4",
}
AUDITED_QUARANTINED_SERVICE_IDENTITIES = {
    "transport": {
        "pid": 30996,
        "process_group_id": 30996,
        "program": "/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/d2-certification-transport",
        "arguments": [
            "/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/d2-certification-transport",
            "--root", "/private/tmp/starring-d2-d2-20260812t082042z-c52d220457d1",
            "--run-id", "d2-20260812t082042z-c52d220457d1",
            "--guild-id", "1536845588954353676",
            "--hub-channel-id", "1536845619266846792",
            "--actor-id", "1056857223529250906",
            "--bot-user-id", "1533144492293754900",
            "--gateway-listen", "127.0.0.1:29101",
            "--http-listen", "127.0.0.1:29102",
        ],
        "candidate": "certification_transport",
        "start_time_seconds": 1786522855,
        "start_time_microseconds": 426871,
        "device": 16777230,
        "inode": 48539870,
        "size": 5854464,
    },
    "worker": {
        "pid": 31004,
        "process_group_id": 31004,
        "program": "/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/node",
        "arguments": [
            "/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/node",
            "/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/codex-worker/worker.mjs",
        ],
        "candidate": "node",
        "start_time_seconds": 1786522855,
        "start_time_microseconds": 969142,
        "device": 16777230,
        "inode": 48539879,
        "size": 292386560,
    },
    "api": {
        "pid": 31105,
        "process_group_id": 31105,
        "program": "/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/starring-api",
        "arguments": [
            "/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/starring-api"
        ],
        "candidate": "api",
        "start_time_seconds": 1786522860,
        "start_time_microseconds": 212847,
        "device": 16777230,
        "inode": 48539866,
        "size": 29124432,
    },
    "runtime": {
        "pid": 31113,
        "process_group_id": 31113,
        "program": "/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/starring-runtime",
        "arguments": [
            "/Users/jungbogeon/Library/Application Support/Starring/d2a-candidates/candidate-20260812T080002Z-ab9b36f25f52-eae50f9a5847/starring-runtime"
        ],
        "candidate": "runtime",
        "start_time_seconds": 1786522860,
        "start_time_microseconds": 239860,
        "device": 16777230,
        "inode": 48539867,
        "size": 28613808,
    },
    "tunnel": {
        "pid": 31121,
        "process_group_id": 31121,
        "program": "/bin/zsh",
        "arguments": [
            "/bin/zsh",
            "/Users/jungbogeon/Library/Application Support/Starring/release-certifications/d2-20260812t082042z-c52d220457d1/orchestrator/run-tunnel.zsh",
        ],
        "candidate": "cloudflared",
        "start_time_seconds": 1786522860,
        "start_time_microseconds": 262639,
        "device": 16777230,
        "inode": 48539880,
        "size": 38238338,
    },
}
ISSUER_BUILD_LIFECYCLE_FIELDS = {
    "schema_version", "kind", "build_id", "status", "source_commit",
    "source_tree", "target_dir", "process_group_id",
    "process_group_quiescent", "build_environment",
    "build_environment_sha256", "started_at", "completed_at", "error_code",
}
STATE_FIELDS = {
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
RESULT_FIELDS = {
    "schema_version",
    "kind",
    "status",
    "error_code",
    "operation",
    "run_id",
    "manifest",
    "state",
    "records",
    "onboarding_evidence",
    "source_revision",
    "candidate_dependencies",
    "issuer_toolchain",
    "release_eligible",
    "persistent_sandbox_retained",
    "discord_teardown_complete",
    "cleanup_complete",
    "total_local_absence",
    "protected_staging_unchanged",
}
DIRECT_ONBOARD_ISSUER_ERROR_CODES = frozenset({
    "api_loopback_origin_invalid",
    "api_loopback_connect_failed",
    "api_loopback_write_failed",
    "api_loopback_read_failed",
    "api_loopback_response_empty",
    "api_loopback_status_invalid",
    "discord_hub_preflight_failed",
    "session_lifecycle_binary_invalid",
    "session_lifecycle_source_invalid",
    "session_lifecycle_boot_identity_invalid",
    "session_lifecycle_existing_marker_invalid",
    "session_lifecycle_handoff_invalid",
    "session_lifecycle_reentry_invalid",
    "session_lifecycle_cas_failed",
})
PHASES = {
    "building_issuer",
    "initialized",
    "preparing_manifest",
    "tainted",
    "dry_run",
    "preflight",
    "prepare",
    "start",
    "direct_onboard",
    "auth_smoke",
    "requested_operation",
    "offline_verify",
    "discord_teardown",
    "cleanup",
    "postconditions",
    "complete",
}


class BootstrapError(Exception):
    def __init__(self, code):
        super().__init__(code)
        self.code = code if isinstance(code, str) and ERROR_CODE.fullmatch(code) else "bootstrap_internal_error"


def fail(code):
    raise BootstrapError(code)


def process_group_exists(process_group):
    try:
        os.killpg(process_group, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def terminate_process_group(process, process_group):
    for signal_value in (signal.SIGTERM, signal.SIGKILL):
        if not process_group_exists(process_group):
            break
        try:
            os.killpg(process_group, signal_value)
        except ProcessLookupError:
            break
        deadline = time.monotonic() + PROCESS_GROUP_GRACE_SECONDS
        while process_group_exists(process_group) and time.monotonic() < deadline:
            try:
                process.wait(timeout=0.05)
            except subprocess.TimeoutExpired:
                pass
            time.sleep(0.01)
    try:
        process.wait(timeout=PROCESS_GROUP_GRACE_SECONDS)
    except subprocess.TimeoutExpired:
        pass
    return not process_group_exists(process_group)


def bounded_subprocess(
    argv,
    cwd,
    environment,
    timeout_seconds,
    maximum=MAX_OUTPUT_BYTES,
    on_spawn=None,
):
    try:
        process = subprocess.Popen(
            argv,
            cwd=cwd,
            env=environment,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
            close_fds=True,
            bufsize=0,
        )
    except OSError:
        fail("subprocess_start_failed")
    process_group = process.pid
    if on_spawn is not None:
        try:
            on_spawn(process_group)
        except BaseException:
            terminate_process_group(process, process_group)
            for stream in (process.stdout, process.stderr):
                try:
                    stream.close()
                except OSError:
                    pass
            raise
    stdout_stream = process.stdout
    stderr_stream = process.stderr
    streams = {stdout_stream: bytearray(), stderr_stream: bytearray()}
    exceeded = {stdout_stream: False, stderr_stream: False}
    selector = selectors.DefaultSelector()
    for stream in streams:
        os.set_blocking(stream.fileno(), False)
        selector.register(stream, selectors.EVENT_READ)
    deadline = time.monotonic() + timeout_seconds
    timed_out = False
    output_exceeded = False
    clean_group = False
    try:
        while selector.get_map():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                timed_out = True
                break
            events = selector.select(min(0.25, remaining))
            if not events and process.poll() is not None:
                events = [(key, selectors.EVENT_READ) for key in selector.get_map().values()]
            for key, _mask in events:
                stream = key.fileobj
                try:
                    chunk = os.read(stream.fileno(), 64 * 1024)
                except BlockingIOError:
                    continue
                except OSError:
                    chunk = b""
                if not chunk:
                    try:
                        selector.unregister(stream)
                    except Exception:
                        pass
                    continue
                retained = max(0, maximum + 1 - len(streams[stream]))
                streams[stream].extend(chunk[:retained])
                if len(chunk) > retained or len(streams[stream]) > maximum:
                    exceeded[stream] = True
                    output_exceeded = True
                    break
            if output_exceeded:
                clean_group = terminate_process_group(process, process_group)
                if not clean_group:
                    fail("subprocess_group_not_quiescent")
                break
            if process.poll() is not None and process_group_exists(process_group):
                clean_group = terminate_process_group(process, process_group)
                if not clean_group:
                    fail("subprocess_group_not_quiescent")
                timed_out = True
                break
        if process.poll() is None:
            try:
                process.wait(timeout=max(0.1, deadline - time.monotonic()))
            except subprocess.TimeoutExpired:
                timed_out = True
        if timed_out or process_group_exists(process_group):
            clean_group = terminate_process_group(process, process_group)
            if not clean_group:
                fail("subprocess_group_not_quiescent")
            if not timed_out and process.returncode == 0:
                timed_out = True
        else:
            clean_group = True
    except BaseException:
        terminate_process_group(process, process_group)
        raise
    finally:
        selector.close()
        for stream in streams:
            try:
                stream.close()
            except OSError:
                pass
    result = subprocess.CompletedProcess(
        argv,
        process.returncode if process.returncode is not None else -signal.SIGKILL,
        bytes(streams[stdout_stream]),
        bytes(streams[stderr_stream]),
    )
    result.timed_out = timed_out
    result.output_exceeded = output_exceeded or exceeded[stdout_stream] or exceeded[stderr_stream]
    result.process_group_quiescent = clean_group
    return result


def canonical_json(value):
    return json.dumps(value, ensure_ascii=False, allow_nan=False, sort_keys=True, separators=(",", ":"))


def sha256_bytes(value):
    return hashlib.sha256(value).hexdigest()


def write_all(descriptor, payload, label):
    offset = 0
    while offset < len(payload):
        try:
            written = os.write(descriptor, payload[offset:])
        except OSError:
            fail(f"{label}_write_failed")
        if type(written) is not int or written <= 0:
            fail(f"{label}_write_failed")
        offset += written


def valid_boot_identity(value):
    if not isinstance(value, str) or BOOT_IDENTITY.fullmatch(value) is None:
        return False
    _prefix, seconds, microseconds = value.split(":")
    return int(seconds) > 0 and 0 <= int(microseconds) < 1_000_000


def current_boot_identity():
    sysctl = absolute_normal_path(FIXED_SYSCTL, "boot_identity_tool", True)
    digest, metadata = stable_file_digest(
        sysctl, "boot_identity_tool", expected_uid=0
    )
    del digest
    if stat.S_IMODE(metadata.st_mode) & 0o111 == 0:
        fail("boot_identity_unavailable")
    completed = bounded_subprocess(
        [str(sysctl), "-n", "kern.boottime"],
        pathlib.Path("/"),
        {
            "HOME": "/var/empty",
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            "LC_ALL": "C",
        },
        10,
        maximum=16 * 1024,
    )
    stdout = completed.stdout
    stderr = completed.stderr
    if (
        completed.returncode != 0
        or completed.timed_out
        or completed.output_exceeded
        or completed.process_group_quiescent is not True
        or stderr
    ):
        fail("boot_identity_unavailable")
    try:
        value = stdout.decode("ascii")
    except UnicodeDecodeError:
        fail("boot_identity_unavailable")
    match = re.fullmatch(
        r"\{ sec = ([0-9]+), usec = ([0-9]+) \}(?: [^\r\n]*)?\n?",
        value,
    )
    if match is None:
        fail("boot_identity_unavailable")
    seconds, microseconds = (int(item) for item in match.groups())
    if seconds <= 0 or not 0 <= microseconds < 1_000_000:
        fail("boot_identity_unavailable")
    return f"darwin-boottime:{seconds}:{microseconds}"


def stable_file_digest(path, label, maximum=1024 * 1024 * 1024, expected_uid=None):
    try:
        before = path.lstat()
    except OSError:
        fail(f"{label}_unavailable")
    if (
        path.is_symlink()
        or not stat.S_ISREG(before.st_mode)
        or (expected_uid is not None and before.st_uid != expected_uid)
        or before.st_nlink < 1
        or stat.S_IMODE(before.st_mode) & 0o022
        or before.st_size > maximum
    ):
        fail(f"{label}_invalid")
    digest = hashlib.sha256()
    observed = 0
    try:
        with path.open("rb", buffering=0) as handle:
            while True:
                chunk = handle.read(1024 * 1024)
                if not chunk:
                    break
                observed += len(chunk)
                if observed > maximum:
                    fail(f"{label}_invalid")
                digest.update(chunk)
            after = os.fstat(handle.fileno())
    except OSError:
        fail(f"{label}_unavailable")
    identity = (
        before.st_dev, before.st_ino, before.st_uid, before.st_gid,
        before.st_mode, before.st_nlink, before.st_size, before.st_mtime_ns,
    )
    if identity != (
        after.st_dev, after.st_ino, after.st_uid, after.st_gid,
        after.st_mode, after.st_nlink, after.st_size, after.st_mtime_ns,
    ):
        fail(f"{label}_changed")
    return digest.hexdigest(), before


def rust_toolchain_manifest(bin_root):
    sysroot = absolute_normal_path(bin_root.parent, "rust_sysroot", must_exist=True)
    records = []
    try:
        paths = [sysroot, *sorted(sysroot.rglob("*"), key=lambda item: str(item.relative_to(sysroot)))]
    except OSError:
        fail("rust_sysroot_invalid")
    for path in paths:
        relative = "." if path == sysroot else path.relative_to(sysroot).as_posix()
        try:
            metadata = path.lstat()
        except OSError:
            fail("rust_sysroot_invalid")
        if path.is_symlink() or metadata.st_uid != os.getuid() or stat.S_IMODE(metadata.st_mode) & 0o022:
            fail("rust_sysroot_invalid")
        if stat.S_ISDIR(metadata.st_mode):
            records.append({"path": relative, "kind": "directory", "mode": stat.S_IMODE(metadata.st_mode)})
        elif stat.S_ISREG(metadata.st_mode):
            digest, stable = stable_file_digest(path, "rust_sysroot_file", expected_uid=os.getuid())
            if stable.st_nlink != 1:
                fail("rust_sysroot_invalid")
            records.append({
                "path": relative,
                "kind": "file",
                "mode": stat.S_IMODE(stable.st_mode),
                "size": stable.st_size,
                "sha256": digest,
            })
        else:
            fail("rust_sysroot_invalid")
    linker_records = []
    digest_cache = {}
    for path in FIXED_LINKERS:
        fixed = absolute_normal_path(path, "rust_linker", must_exist=True)
        try:
            observed = fixed.lstat()
        except OSError:
            fail("rust_linker_unavailable")
        key = (observed.st_dev, observed.st_ino, observed.st_size, observed.st_mtime_ns)
        if key not in digest_cache:
            digest_cache[key], observed = stable_file_digest(
                fixed, "rust_linker", expected_uid=0
            )
        if (
            fixed.is_symlink()
            or not stat.S_ISREG(observed.st_mode)
            or observed.st_uid != 0
            or stat.S_IMODE(observed.st_mode) & 0o022
            or stat.S_IMODE(observed.st_mode) & 0o111 == 0
        ):
            fail("rust_linker_invalid")
        linker_records.append({
            "path": str(fixed),
            "mode": stat.S_IMODE(observed.st_mode),
            "uid": observed.st_uid,
            "nlink": observed.st_nlink,
            "size": observed.st_size,
            "sha256": digest_cache[key],
        })
    return {
        "sysroot": str(sysroot),
        "files": records,
        "sha256": sha256_bytes(canonical_json(records).encode("utf-8")),
        "linkers": linker_records,
        "linkers_sha256": sha256_bytes(canonical_json(linker_records).encode("utf-8")),
    }


def rooted_tree_digest(root, label):
    root = absolute_normal_path(root, label, must_exist=True)
    digest = hashlib.sha256(f"starring.d2a.{label}.v1\0".encode("ascii"))
    ancestors = []
    for directory in (root, *root.parents):
        try:
            metadata = directory.lstat()
        except OSError:
            fail(f"{label}_invalid")
        if (
            directory.is_symlink()
            or not stat.S_ISDIR(metadata.st_mode)
            or metadata.st_uid != 0
            or stat.S_IMODE(metadata.st_mode) & 0o022
        ):
            fail(f"{label}_invalid")
        ancestors.append({
            "path": str(directory),
            "mode": stat.S_IMODE(metadata.st_mode),
            "uid": metadata.st_uid,
        })
    root_identity = ancestors[0]
    encoded_root = canonical_json(root_identity).encode("utf-8")
    digest.update(len(encoded_root).to_bytes(8, "big"))
    digest.update(encoded_root)
    count = 0
    try:
        paths = sorted(root.rglob("*"), key=lambda item: item.relative_to(root).as_posix())
    except OSError:
        fail(f"{label}_invalid")
    for path in paths:
        relative = path.relative_to(root).as_posix()
        try:
            metadata = path.lstat()
        except OSError:
            fail(f"{label}_invalid")
        if metadata.st_uid != 0 or (
            not stat.S_ISLNK(metadata.st_mode)
            and stat.S_IMODE(metadata.st_mode) & 0o022
        ):
            fail(f"{label}_invalid")
        if stat.S_ISDIR(metadata.st_mode):
            record = {"path": relative, "kind": "directory", "mode": stat.S_IMODE(metadata.st_mode)}
        elif stat.S_ISLNK(metadata.st_mode):
            try:
                target = os.readlink(path)
                resolved = path.resolve(strict=True)
            except OSError:
                fail(f"{label}_invalid")
            if resolved != root and root not in resolved.parents:
                fail(f"{label}_invalid")
            record = {
                "path": relative,
                "kind": "symlink",
                "target": target,
                "resolved_path": resolved.relative_to(root).as_posix(),
            }
        elif stat.S_ISREG(metadata.st_mode):
            file_digest, stable = stable_file_digest(
                path, f"{label}_file", expected_uid=0
            )
            record = {
                "path": relative,
                "kind": "file",
                "mode": stat.S_IMODE(stable.st_mode),
                "size": stable.st_size,
                "sha256": file_digest,
            }
        else:
            fail(f"{label}_invalid")
        encoded = canonical_json(record).encode("utf-8")
        digest.update(len(encoded).to_bytes(8, "big"))
        digest.update(encoded)
        count += 1
    return {
        "root": str(root),
        "root_identity": root_identity,
        "ancestors": ancestors,
        "entries": count,
        "sha256": digest.hexdigest(),
    }


def reject_cargo_configuration(source_root):
    current = absolute_normal_path(source_root, "source_root", must_exist=True)
    while True:
        for name in ("config", "config.toml"):
            if os.path.lexists(current / ".cargo" / name):
                fail("cargo_config_present")
        if current.parent == current:
            break
        current = current.parent


def prepare_isolated_cargo_home(parent, identifier):
    parent = absolute_normal_path(parent, "issuer_target_root", must_exist=True)
    cargo_home = parent / f".cargo-home-{identifier}"
    try:
        cargo_home.mkdir(mode=0o700)
    except OSError:
        fail("cargo_home_create_failed")
    source_home = pathlib.Path.home() / ".cargo"
    for name in ("registry", "git"):
        source = source_home / name
        if not source.exists():
            continue
        source = absolute_normal_path(source, f"cargo_cache_{name}", must_exist=True)
        try:
            metadata = source.lstat()
        except OSError:
            fail("cargo_cache_invalid")
        if (
            source.is_symlink()
            or not stat.S_ISDIR(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) & 0o022
        ):
            fail("cargo_cache_invalid")
        try:
            (cargo_home / name).symlink_to(source, target_is_directory=True)
        except OSError:
            fail("cargo_home_create_failed")
    return cargo_home


def utc_now():
    return (
        datetime.datetime.now(datetime.timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z")
    )


def lifecycle_timestamp():
    now = time.time_ns()
    seconds, nanoseconds = divmod(now, 1_000_000_000)
    calendar = datetime.datetime.fromtimestamp(
        seconds, datetime.timezone.utc
    ).strftime("%Y-%m-%dT%H:%M:%S")
    return f"{calendar}.{nanoseconds:09d}Z"


def strict_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate_key")
        result[key] = value
    return result


def reject_constant(_value):
    raise ValueError("non_finite")


def absolute_normal_path(raw, label, must_exist=False):
    if not isinstance(raw, (str, os.PathLike)):
        fail(f"{label}_path_invalid")
    path = pathlib.Path(raw)
    if not path.is_absolute() or path != pathlib.Path(os.path.normpath(path)):
        fail(f"{label}_path_invalid")
    try:
        resolved = path.resolve(strict=must_exist)
    except OSError:
        fail(f"{label}_path_invalid")
    if resolved != path:
        fail(f"{label}_path_invalid")
    return path


def require_owned(path, label, mode, directory=False, allow_empty=False):
    try:
        metadata = path.lstat()
    except OSError:
        fail(f"{label}_unavailable")
    expected = stat.S_ISDIR if directory else stat.S_ISREG
    if (
        path.is_symlink()
        or not expected(metadata.st_mode)
        or metadata.st_uid != os.getuid()
        or stat.S_IMODE(metadata.st_mode) != mode
        or (not directory and metadata.st_nlink != 1)
        or (not directory and not allow_empty and metadata.st_size < 1)
    ):
        fail(f"{label}_invalid")
    return metadata


def read_owned_json(raw_path, label, mode, maximum):
    path = absolute_normal_path(raw_path, label, must_exist=True)
    expected = require_owned(path, label, mode)
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError:
        fail(f"{label}_unavailable")
    try:
        before = os.fstat(descriptor)
        if (
            (before.st_dev, before.st_ino) != (expected.st_dev, expected.st_ino)
            or not stat.S_ISREG(before.st_mode)
            or before.st_uid != os.getuid()
            or stat.S_IMODE(before.st_mode) != mode
            or before.st_nlink != 1
        ):
            fail(f"{label}_invalid")
        raw = b""
        while len(raw) <= maximum:
            chunk = os.read(descriptor, min(16384, maximum + 1 - len(raw)))
            if not chunk:
                break
            raw += chunk
        after = os.fstat(descriptor)
        if (
            len(raw) < 1
            or len(raw) > maximum
            or (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)
            != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
        ):
            fail(f"{label}_invalid")
    finally:
        os.close(descriptor)
    try:
        value = json.loads(
            raw,
            object_pairs_hook=strict_object,
            parse_constant=reject_constant,
        )
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
        fail(f"{label}_invalid")
    return path, value, sha256_bytes(raw)


def read_private_json(raw_path, label):
    return read_owned_json(raw_path, label, 0o600, MAX_INPUT_BYTES)


def read_canonical_private_json(raw_path, label):
    path = absolute_normal_path(raw_path, label, must_exist=True)
    raw = private_file_bytes(path, label)
    if len(raw) > MAX_INPUT_BYTES:
        fail(f"{label}_invalid")
    try:
        value = json.loads(
            raw,
            object_pairs_hook=strict_object,
            parse_constant=reject_constant,
        )
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
        fail(f"{label}_invalid")
    if raw != (canonical_json(value) + "\n").encode("utf-8"):
        fail(f"{label}_invalid")
    return path, value, sha256_bytes(raw)


def read_immutable_json(raw_path, label, maximum=MAX_INPUT_BYTES):
    return read_owned_json(raw_path, label, 0o400, maximum)


def private_file_bytes(path, label, allow_empty=False, mode=0o600):
    path = absolute_normal_path(path, label, must_exist=True)
    expected = require_owned(path, label, mode, allow_empty=allow_empty)
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError:
        fail(f"{label}_unavailable")
    try:
        before = os.fstat(descriptor)
        if (
            (before.st_dev, before.st_ino) != (expected.st_dev, expected.st_ino)
            or not stat.S_ISREG(before.st_mode)
            or before.st_uid != os.getuid()
            or stat.S_IMODE(before.st_mode) != mode
            or before.st_nlink != 1
        ):
            fail(f"{label}_invalid")
        raw = b""
        while len(raw) <= MAX_OUTPUT_BYTES:
            chunk = os.read(descriptor, min(65536, MAX_OUTPUT_BYTES + 1 - len(raw)))
            if not chunk:
                break
            raw += chunk
        after = os.fstat(descriptor)
        try:
            named = os.stat(path, follow_symlinks=False)
        except OSError:
            fail(f"{label}_invalid")
        identity = lambda item: (
            item.st_dev,
            item.st_ino,
            item.st_uid,
            item.st_gid,
            item.st_mode,
            item.st_nlink,
            item.st_size,
            item.st_mtime_ns,
        )
        if (
            (not allow_empty and not raw)
            or len(raw) > MAX_OUTPUT_BYTES
            or identity(before) != identity(after)
            or identity(after) != identity(named)
        ):
            fail(f"{label}_invalid")
        return raw
    finally:
        os.close(descriptor)


def validate_snowflake(value, label):
    if (
        not isinstance(value, str)
        or not SNOWFLAKE.fullmatch(value)
        or int(value) > 18_446_744_073_709_551_615
    ):
        fail(f"{label}_invalid")


def validate_keychain_ref(value, label):
    if not isinstance(value, str):
        fail(f"{label}_invalid")
    service, separator, account = value.partition(":")
    if not separator or ":" in account or not IDENTIFIER.fullmatch(service) or not IDENTIFIER.fullmatch(account):
        fail(f"{label}_invalid")
    return {"service": service, "account": account}


def validate_root_path(value, label):
    return absolute_normal_path(value, label, must_exist=False)


def validate_config(value):
    if (
        not isinstance(value, dict)
        or set(value) != CONFIG_FIELDS
        or type(value.get("schema_version")) is not int
        or value.get("schema_version") != SCHEMA_VERSION
        or value.get("kind") != CONFIG_KIND
        or not isinstance(value.get("sandbox_id"), str)
        or not SANDBOX_ID.fullmatch(value["sandbox_id"])
        or value.get("guild_lifecycle") != "persistent_reuse_no_delete_v1"
    ):
        fail("sandbox_config_schema_invalid")
    discord = value.get("discord")
    if not isinstance(discord, dict) or set(discord) != DISCORD_FIELDS:
        fail("sandbox_config_discord_invalid")
    for field in ("guild_id", "hub_channel_id", "application_id", "bot_user_id", "actor_id"):
        validate_snowflake(discord.get(field), f"sandbox_{field}")
    if (
        discord["application_id"] != discord["bot_user_id"]
        or len({
            discord["guild_id"],
            discord["hub_channel_id"],
            discord["actor_id"],
            discord["application_id"],
        }) != 4
    ):
        fail("sandbox_discord_identity_overlap")
    display_name = discord.get("actor_display_name")
    if (
        not isinstance(display_name, str)
        or display_name != display_name.strip()
        or not display_name
        or len(display_name) > 128
        or len(display_name.encode("utf-8")) > 512
        or any(unicodedata.category(character).startswith("C") for character in display_name)
    ):
        fail("sandbox_actor_display_name_invalid")
    credentials = value.get("credential_refs")
    if not isinstance(credentials, dict) or set(credentials) != CREDENTIAL_FIELDS:
        fail("sandbox_credential_refs_invalid")
    for field in sorted(CREDENTIAL_FIELDS):
        validate_keychain_ref(credentials[field], f"sandbox_{field}_keychain")
    cloudflare = value.get("cloudflare")
    if not isinstance(cloudflare, dict) or set(cloudflare) != CLOUDFLARE_FIELDS:
        fail("sandbox_cloudflare_invalid")
    try:
        tunnel_id = uuid.UUID(cloudflare.get("tunnel_id", ""))
    except (ValueError, TypeError, AttributeError):
        fail("sandbox_cloudflare_invalid")
    if (
        tunnel_id.version != 4
        or str(tunnel_id) != cloudflare["tunnel_id"]
        or cloudflare["tunnel_id"] != "57c22e8a-0ec2-4f67-a882-2c355b0348df"
        or cloudflare.get("public_origin") != D2A.D2_PUBLIC_ORIGIN
    ):
        fail("sandbox_cloudflare_invalid")
    ports = value.get("ports")
    if (
        not isinstance(ports, dict)
        or set(ports) != PORT_FIELDS
        or any(type(ports[field]) is not int or not 1024 <= ports[field] <= 65535 for field in PORT_FIELDS)
        or len(set(ports.values())) != len(PORT_FIELDS)
        or ports["api"] != 28080
    ):
        fail("sandbox_ports_invalid")
    roots = [
        validate_root_path(value[field], field)
        for field in ("release_run_root", "d2a_result_root", "bootstrap_state_root")
    ]
    if len(set(roots)) != 3 or any(
        left in right.parents or right in left.parents
        for index, left in enumerate(roots)
        for right in roots[index + 1 :]
    ):
        fail("sandbox_roots_invalid")
    return value


def candidate_file_identity(
    raw,
    label,
    expected_mode,
    maximum=512 * 1024 * 1024,
    *,
    immutable_parent=True,
):
    path = absolute_normal_path(raw, label, must_exist=True)
    try:
        metadata = path.lstat()
        parent = path.parent.lstat()
    except OSError:
        fail(f"{label}_invalid")
    if (
        path.is_symlink()
        or not stat.S_ISREG(metadata.st_mode)
        or metadata.st_uid != os.getuid()
        or metadata.st_nlink != 1
        or stat.S_IMODE(metadata.st_mode) != expected_mode
        or metadata.st_size < 1
        or metadata.st_size > maximum
        or path.parent.is_symlink()
        or not stat.S_ISDIR(parent.st_mode)
        or parent.st_uid != os.getuid()
        or (immutable_parent and stat.S_IMODE(parent.st_mode) & 0o222)
    ):
        fail(f"{label}_invalid")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError:
        fail(f"{label}_unavailable")
    try:
        before = os.fstat(descriptor)
        if (before.st_dev, before.st_ino) != (metadata.st_dev, metadata.st_ino):
            fail(f"{label}_invalid")
        digest = hashlib.sha256()
        size = 0
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            size += len(chunk)
            if size > maximum:
                fail(f"{label}_invalid")
            digest.update(chunk)
        after = os.fstat(descriptor)
        if (
            (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)
            != (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
        ):
            fail(f"{label}_changed")
        return path, {
            "path": str(path),
            "sha256": digest.hexdigest(),
            "size": before.st_size,
            "mode": stat.S_IMODE(before.st_mode),
            "uid": before.st_uid,
            "links": before.st_nlink,
        }
    finally:
        os.close(descriptor)


def validate_candidate_spec(value):
    if (
        not isinstance(value, dict)
        or set(value) != CANDIDATE_FIELDS
        or type(value.get("schema_version")) is not int
        or value.get("schema_version") != CANDIDATE_SCHEMA_VERSION
        or value.get("kind") != CANDIDATE_KIND
        or not isinstance(value.get("commit_sha"), str)
        or not COMMIT.fullmatch(value["commit_sha"])
        or not isinstance(value.get("source_tree_sha"), str)
        or not COMMIT.fullmatch(value["source_tree_sha"])
        or not isinstance(value.get("provenance_sha256"), str)
        or not DIGEST.fullmatch(value["provenance_sha256"])
        or not isinstance(value.get("candidates"), dict)
        or set(value["candidates"]) != D2A.CANDIDATE_KEYS
    ):
        fail("candidate_spec_schema_invalid")
    absolute_normal_path(value.get("bundle"), "candidate_bundle", must_exist=False)
    for name in sorted(D2A.CANDIDATE_KEYS):
        record = value["candidates"][name]
        if (
            not isinstance(record, dict)
            or set(record) != CANDIDATE_RECORD_FIELDS
            or not isinstance(record.get("path"), str)
            or not isinstance(record.get("sha256"), str)
            or not DIGEST.fullmatch(record["sha256"])
        ):
            fail("candidate_spec_schema_invalid")
        absolute_normal_path(record["path"], f"candidate_{name}", must_exist=False)
    return value


def candidate_provenance_record(provenance, name):
    if name in CANDIDATE_ARTIFACT_NAMES:
        records = provenance.get("artifacts")
        if not isinstance(records, dict) or set(records) != CANDIDATE_ARTIFACT_NAMES:
            fail("candidate_provenance_invalid")
        return records.get(name)
    if name in CANDIDATE_OPERATOR_NAMES:
        records = provenance.get("operators")
        if not isinstance(records, dict) or set(records) != CANDIDATE_OPERATOR_NAMES:
            fail("candidate_provenance_invalid")
        return records.get(name)
    worker = provenance.get("worker")
    files = worker.get("files") if isinstance(worker, dict) else None
    if not isinstance(files, dict) or set(files) != CODEX_WORKER_FILES:
        fail("candidate_provenance_invalid")
    return files.get("worker.mjs")


def valid_candidate_identity(value, *, tool=False):
    expected = CANDIDATE_TOOL_IDENTITY_FIELDS if tool else CANDIDATE_IDENTITY_FIELDS
    return (
        isinstance(value, dict)
        and set(value) == expected
        and isinstance(value.get("path"), str)
        and pathlib.Path(value["path"]).is_absolute()
        and isinstance(value.get("sha256"), str)
        and bool(DIGEST.fullmatch(value["sha256"]))
        and type(value.get("size")) is int
        and value["size"] >= 0
        and type(value.get("mode")) is int
        and 0 <= value["mode"] <= 0o7777
        and type(value.get("uid")) is int
        and value["uid"] >= 0
        and type(value.get("links")) is int
        and value["links"] >= 1
        and (
            not tool
            or (isinstance(value.get("version"), str) and bool(value["version"]))
        )
    )


def workspace_dependency_cargo_configuration(bootstrap_root):
    workspace = bootstrap_root / "vendor" / "workspace"
    transport = bootstrap_root / "vendor" / "transport"
    return (
        '[source.crates-io]\nreplace-with = "workspace-vendored-sources"\n\n'
        '[source."git+https://github.com/twilight-rs/twilight.git?rev='
        'b4ce13b727e7731b917576ad977300ab6926bb6b"]\n'
        'git = "https://github.com/twilight-rs/twilight.git"\n'
        'rev = "b4ce13b727e7731b917576ad977300ab6926bb6b"\n'
        'replace-with = "twilight-vendored-sources"\n\n'
        '[source.workspace-vendored-sources]\n'
        f'directory = "{workspace}"\n\n'
        '[source.twilight-vendored-sources]\n'
        f'directory = "{transport}"\n\n'
        '[net]\noffline = true\n'
    ).encode("utf-8")


def transport_dependency_cargo_configuration(bootstrap_root):
    transport = bootstrap_root / "vendor" / "transport"
    return (
        '[source.crates-io]\nreplace-with = "vendored-sources"\n\n'
        '[source."git+https://github.com/twilight-rs/twilight.git?rev='
        'b4ce13b727e7731b917576ad977300ab6926bb6b"]\n'
        'git = "https://github.com/twilight-rs/twilight.git"\n'
        'rev = "b4ce13b727e7731b917576ad977300ab6926bb6b"\n'
        'replace-with = "vendored-sources"\n\n'
        '[source.vendored-sources]\n'
        f'directory = "{transport}"\n\n'
        '[net]\noffline = true\n'
    ).encode("utf-8")


def candidate_dependency_file(raw, label, expected_mode, maximum=MAX_INPUT_BYTES):
    path = absolute_normal_path(raw, label, must_exist=True)
    try:
        expected = path.lstat()
    except OSError:
        fail(f"{label}_invalid")
    if (
        path.is_symlink()
        or not stat.S_ISREG(expected.st_mode)
        or expected.st_uid != os.getuid()
        or expected.st_nlink != 1
        or stat.S_IMODE(expected.st_mode) != expected_mode
        or not 0 < expected.st_size <= maximum
    ):
        fail(f"{label}_invalid")
    try:
        descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    except OSError:
        fail(f"{label}_unavailable")
    try:
        before = os.fstat(descriptor)
        if (before.st_dev, before.st_ino) != (expected.st_dev, expected.st_ino):
            fail(f"{label}_invalid")
        payload = bytearray()
        digest = hashlib.sha256()
        while True:
            chunk = os.read(descriptor, min(1024 * 1024, maximum + 1 - len(payload)))
            if not chunk:
                break
            payload.extend(chunk)
            digest.update(chunk)
            if len(payload) > maximum:
                fail(f"{label}_invalid")
        after = os.fstat(descriptor)
        if (
            len(payload) != before.st_size
            or (
                before.st_dev, before.st_ino, before.st_uid, before.st_gid,
                before.st_mode, before.st_nlink, before.st_size,
                before.st_mtime_ns, before.st_ctime_ns,
            )
            != (
                after.st_dev, after.st_ino, after.st_uid, after.st_gid,
                after.st_mode, after.st_nlink, after.st_size,
                after.st_mtime_ns, after.st_ctime_ns,
            )
        ):
            fail(f"{label}_changed")
        return bytes(payload), {
            "path": str(path),
            "sha256": digest.hexdigest(),
            "size": before.st_size,
            "mode": stat.S_IMODE(before.st_mode),
            "uid": before.st_uid,
            "links": before.st_nlink,
        }
    finally:
        os.close(descriptor)


def candidate_dependency_tree_identity(raw_root):
    root = absolute_normal_path(raw_root, "candidate_dependency_bootstrap", must_exist=True)
    try:
        root_before = root.lstat()
    except OSError:
        fail("candidate_dependency_bootstrap_invalid")
    if (
        root.name != "gate-bootstrap"
        or root.is_symlink()
        or not stat.S_ISDIR(root_before.st_mode)
        or root_before.st_uid != os.getuid()
        or stat.S_IMODE(root_before.st_mode) != 0o555
    ):
        fail("candidate_dependency_bootstrap_invalid")
    try:
        paths = sorted(root.rglob("*"), key=lambda path: path.relative_to(root).as_posix())
    except OSError:
        fail("candidate_dependency_tree_unavailable")
    digest = hashlib.sha256()
    entries = 0
    total_bytes = 0
    inventory = []
    for path in paths:
        relative = path.relative_to(root).as_posix()
        inventory.append(relative)
        try:
            before = path.lstat()
        except OSError:
            fail("candidate_dependency_tree_unavailable")
        mode = stat.S_IMODE(before.st_mode)
        if stat.S_ISLNK(before.st_mode) or before.st_uid != os.getuid() or mode & 0o222:
            fail("candidate_dependency_tree_invalid")
        entries += 1
        if entries > MAX_CANDIDATE_DEPENDENCY_ENTRIES:
            fail("candidate_dependency_tree_too_large")
        if stat.S_ISDIR(before.st_mode):
            kind = "directory"
            size = 0
        elif stat.S_ISREG(before.st_mode) and before.st_nlink == 1:
            kind = "file"
            size = before.st_size
        else:
            fail("candidate_dependency_tree_invalid")
        header = canonical_json(
            {"path": relative, "kind": kind, "mode": mode, "size": size}
        ).encode("utf-8")
        digest.update(len(header).to_bytes(8, "big"))
        digest.update(header)
        if kind == "directory":
            continue
        total_bytes += size
        if total_bytes > MAX_CANDIDATE_DEPENDENCY_BYTES:
            fail("candidate_dependency_tree_too_large")
        try:
            descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
        except OSError:
            fail("candidate_dependency_tree_unavailable")
        observed = 0
        try:
            while True:
                chunk = os.read(descriptor, 1024 * 1024)
                if not chunk:
                    break
                observed += len(chunk)
                if observed > size:
                    fail("candidate_dependency_tree_changed")
                digest.update(chunk)
            after = os.fstat(descriptor)
        finally:
            os.close(descriptor)
        if (
            observed != size
            or (
                before.st_dev, before.st_ino, before.st_uid, before.st_gid,
                before.st_mode, before.st_nlink, before.st_size,
                before.st_mtime_ns, before.st_ctime_ns,
            )
            != (
                after.st_dev, after.st_ino, after.st_uid, after.st_gid,
                after.st_mode, after.st_nlink, after.st_size,
                after.st_mtime_ns, after.st_ctime_ns,
            )
        ):
            fail("candidate_dependency_tree_changed")
    try:
        final_inventory = [
            path.relative_to(root).as_posix()
            for path in sorted(root.rglob("*"), key=lambda path: path.relative_to(root).as_posix())
        ]
        root_after = root.lstat()
    except OSError:
        fail("candidate_dependency_tree_unavailable")
    if (
        inventory != final_inventory
        or (
            root_before.st_dev, root_before.st_ino, root_before.st_uid,
            root_before.st_gid, root_before.st_mode, root_before.st_mtime_ns,
            root_before.st_ctime_ns,
        )
        != (
            root_after.st_dev, root_after.st_ino, root_after.st_uid,
            root_after.st_gid, root_after.st_mode, root_after.st_mtime_ns,
            root_after.st_ctime_ns,
        )
    ):
        fail("candidate_dependency_tree_changed")
    return {
        "entries": entries,
        "total_bytes": total_bytes,
        "tree_sha256": digest.hexdigest(),
    }


def validate_candidate_dependencies(value, source_root):
    if (
        not isinstance(value, dict)
        or set(value) != CANDIDATE_DEPENDENCY_SNAPSHOT_FIELDS
        or type(value.get("schema_version")) is not int
        or value.get("schema_version") != 1
        or value.get("kind") != CANDIDATE_DEPENDENCY_SNAPSHOT_KIND
        or not valid_candidate_identity(value.get("record"))
        or not isinstance(value.get("workspace"), dict)
        or set(value["workspace"]) != CANDIDATE_DEPENDENCY_SOURCE_FIELDS
        or not isinstance(value.get("transport"), dict)
        or set(value["transport"]) != CANDIDATE_DEPENDENCY_SOURCE_FIELDS
        or not isinstance(value.get("source_inputs"), dict)
        or set(value["source_inputs"]) != set(CANDIDATE_DEPENDENCY_SOURCE_INPUTS)
        or not valid_candidate_identity(value["workspace"].get("cargo_config"))
        or not valid_candidate_identity(value["transport"].get("cargo_config"))
        or any(not valid_candidate_identity(record) for record in value["source_inputs"].values())
        or not DIGEST.fullmatch(value.get("gate_runtime_sha256", ""))
        or not DIGEST.fullmatch(value.get("record_sha256", ""))
        or not DIGEST.fullmatch(value.get("tree_sha256", ""))
        or type(value.get("entries")) is not int
        or not 0 < value["entries"] <= MAX_CANDIDATE_DEPENDENCY_ENTRIES
        or type(value.get("total_bytes")) is not int
        or not 0 < value["total_bytes"] <= MAX_CANDIDATE_DEPENDENCY_BYTES
    ):
        fail("candidate_dependency_snapshot_invalid")
    bootstrap = absolute_normal_path(
        value.get("bootstrap_root"), "candidate_dependency_bootstrap", must_exist=True
    )
    require_owned(bootstrap.parent, "candidate_dependency_state_root", 0o700, directory=True)
    record_path = bootstrap.parent / "gate-bootstrap.json"
    if pathlib.Path(value["record"]["path"]) != record_path:
        fail("candidate_dependency_record_invalid")
    raw_record, observed_record_identity = candidate_dependency_file(
        record_path, "candidate_dependency_record", 0o600
    )
    if observed_record_identity != value["record"]:
        fail("candidate_dependency_snapshot_changed")
    try:
        record = json.loads(
            raw_record, object_pairs_hook=strict_object, parse_constant=reject_constant
        )
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
        fail("candidate_dependency_record_invalid")
    if (
        not isinstance(record, dict)
        or set(record) != CANDIDATE_DEPENDENCY_RECORD_FIELDS
        or type(record.get("schema_version")) is not int
        or record.get("schema_version") != 1
        or record.get("kind") != "starring.d3.gate-bootstrap.v1"
        or not DIGEST.fullmatch(record.get("gate_runtime_sha256", ""))
        or type(record.get("entries")) is not int
        or not 0 < record["entries"] <= MAX_CANDIDATE_DEPENDENCY_ENTRIES
        or type(record.get("total_bytes")) is not int
        or not 0 < record["total_bytes"] <= MAX_CANDIDATE_DEPENDENCY_BYTES
        or not DIGEST.fullmatch(record.get("tree_sha256", ""))
        or not DIGEST.fullmatch(record.get("record_sha256", ""))
    ):
        fail("candidate_dependency_record_invalid")
    record_payload = dict(record)
    record_seal = record_payload.pop("record_sha256")
    if (
        sha256_bytes(canonical_json(record_payload).encode("utf-8")) != record_seal
        or raw_record != (canonical_json(record) + "\n").encode("utf-8")
    ):
        fail("candidate_dependency_record_invalid")
    tree = candidate_dependency_tree_identity(bootstrap)
    if any(tree[name] != record[name] for name in tree):
        fail("candidate_dependency_tree_mismatch")
    if any(
        value[name] != record[name]
        for name in (
            "gate_runtime_sha256", "record_sha256", "tree_sha256",
            "entries", "total_bytes",
        )
    ):
        fail("candidate_dependency_snapshot_changed")
    workspace_vendor = bootstrap / "vendor" / "workspace"
    transport_vendor = bootstrap / "vendor" / "transport"
    require_owned(workspace_vendor, "candidate_dependency_workspace_vendor", 0o500, directory=True)
    require_owned(transport_vendor, "candidate_dependency_transport_vendor", 0o500, directory=True)
    workspace_config = bootstrap / "native-cargo-config.toml"
    transport_config = bootstrap / "native-transport-cargo-config.toml"
    workspace_raw, workspace_identity = candidate_dependency_file(
        workspace_config, "candidate_dependency_workspace_config", 0o400
    )
    transport_raw, transport_identity = candidate_dependency_file(
        transport_config, "candidate_dependency_transport_config", 0o400
    )
    if (
        workspace_raw != workspace_dependency_cargo_configuration(bootstrap)
        or transport_raw != transport_dependency_cargo_configuration(bootstrap)
        or value["workspace"] != {
            "vendor_root": str(workspace_vendor), "cargo_config": workspace_identity,
        }
        or value["transport"] != {
            "vendor_root": str(transport_vendor), "cargo_config": transport_identity,
        }
    ):
        fail("candidate_dependency_config_invalid")
    source = absolute_normal_path(source_root, "candidate_dependency_source", must_exist=True)
    observed_inputs = {}
    for name, relative in CANDIDATE_DEPENDENCY_SOURCE_INPUTS.items():
        _raw, observed_inputs[name] = candidate_dependency_file(
            source / relative,
            f"candidate_dependency_{name}",
            0o644,
            CANDIDATE_DEPENDENCY_SOURCE_LIMITS[name],
        )
    if observed_inputs != value["source_inputs"]:
        fail("candidate_dependency_source_changed")
    return value


def validate_candidate_recipe(provenance):
    source = provenance["source"]
    toolchain = provenance["toolchain"]
    tools = toolchain["tools"]
    cargo = tools["cargo"]["path"]
    commands = provenance["commands"]
    dependencies = provenance["dependencies"]
    workspace_config = dependencies["workspace"]["cargo_config"]["path"]
    transport_config = dependencies["transport"]["cargo_config"]["path"]
    first_target = pathlib.Path(commands[0][7]) if len(commands[0]) > 7 else pathlib.Path(".")
    build_root = first_target.parent
    workspace_target = build_root / "workspace-target"
    transport_target = build_root / "transport-target"
    expected = [
        [cargo, "--config", workspace_config, *(
            str(workspace_target) if item == "{workspace_target}" else item
            for item in command
        )]
        for command in (
            ("build", "--frozen", "--release", "--target-dir", "{workspace_target}", "-p", "starring-api", "--bin", "starring-api"),
            ("build", "--frozen", "--release", "--target-dir", "{workspace_target}", "-p", "starring-runtime", "--bin", "starring-runtime"),
            ("build", "--frozen", "--release", "--target-dir", "{workspace_target}", "-p", "starring-db-bootstrap", "--bin", "starring-d2-db-bootstrap"),
            ("build", "--frozen", "--release", "--target-dir", "{workspace_target}", "-p", "starring-staging-provisioner", "--bin", "starring-d2-sealed-provisioner"),
        )
    ]
    expected.append([
        cargo, "--config", transport_config, "build", "--frozen", "--release", "--manifest-path",
        "tools/d2-certification-transport/Cargo.toml", "--target-dir",
        str(transport_target),
    ])
    darwin = toolchain["darwin"]
    selected = darwin["selected_tools"]
    environment = provenance["environment"]
    expected_environment = {
        "HOME": str(pathlib.Path.home()),
        "PATH": f"{toolchain['root']}:/usr/bin:/bin:/usr/sbin:/sbin",
        "RUSTC": tools["rustc"]["path"],
        "CC": selected["clang"]["resolved_path"],
        "CXX": selected["clang"]["resolved_path"],
        "AR": selected["ar"]["resolved_path"],
        "RANLIB": selected["ranlib"]["resolved_path"],
        "SDKROOT": darwin["sdk"]["root"],
        "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER": selected["clang"]["resolved_path"],
        "CARGO_HOME": str(build_root / "cargo-home"),
        "CARGO_INCREMENTAL": "0",
        "CARGO_BUILD_JOBS": "1",
        "CARGO_NET_OFFLINE": "true",
        "GIT_TERMINAL_PROMPT": "0",
        "GIT_CONFIG_NOSYSTEM": "1",
        "LC_ALL": "C",
        "STARRING_RUNTIME_BUILD_REVISION": source["commit"],
        "TMPDIR": str(build_root / "tmp"),
    }
    bundle = pathlib.Path(provenance["bundle"])
    if (
        commands != expected
        or environment != expected_environment
        or not build_root.is_absolute()
        or not re.fullmatch(r"\.build-d2ac-[0-9a-f]{32}", build_root.name)
        or build_root.parent != bundle.parent
    ):
        fail("candidate_provenance_recipe_invalid")


def validate_candidate_toolchain(provenance):
    toolchain = provenance["toolchain"]
    tools = toolchain.get("tools")
    sysroot = toolchain.get("sysroot_manifest")
    darwin = toolchain.get("darwin")
    selected = darwin.get("selected_tools") if isinstance(darwin, dict) else None
    fixed = darwin.get("fixed_tools") if isinstance(darwin, dict) else None
    sdk = darwin.get("sdk") if isinstance(darwin, dict) else None
    if (
        set(toolchain) != {"target", "root", "rustc_verbose_version", "tools", "sysroot_manifest", "darwin"}
        or toolchain.get("target") != "aarch64-apple-darwin"
        or not isinstance(toolchain.get("root"), str)
        or not pathlib.Path(toolchain["root"]).is_absolute()
        or not isinstance(toolchain.get("rustc_verbose_version"), list)
        or not all(isinstance(line, str) for line in toolchain["rustc_verbose_version"])
        or not isinstance(tools, dict)
        or set(tools) != {"cargo", "rustc"}
        or not all(valid_candidate_identity(tools[name], tool=True) for name in tools)
        or pathlib.Path(tools["cargo"]["path"]) != pathlib.Path(toolchain["root"]) / "cargo"
        or pathlib.Path(tools["rustc"]["path"]) != pathlib.Path(toolchain["root"]) / "rustc"
        or not tools["cargo"]["version"].startswith("cargo 1.97.0 ")
        or not tools["rustc"]["version"].startswith("rustc 1.97.0 ")
        or not isinstance(sysroot, dict)
        or set(sysroot) != {"sysroot", "files", "sha256", "linkers", "linkers_sha256"}
        or not DIGEST.fullmatch(sysroot.get("sha256", ""))
        or not DIGEST.fullmatch(sysroot.get("linkers_sha256", ""))
        or not isinstance(sysroot.get("files"), list)
        or not isinstance(sysroot.get("linkers"), list)
        or not isinstance(darwin, dict)
        or set(darwin) != {"fixed_tools", "developer_root", "selected_tools", "sdk", "os_build_version"}
        or not isinstance(fixed, dict)
        or set(fixed) != {"xcrun", "xcode_select", "sw_vers"}
        or any(
            not isinstance(record, dict)
            or set(record) != CANDIDATE_DARWIN_FIXED_FIELDS
            or not valid_candidate_identity(record)
            for record in fixed.values()
        )
        or not isinstance(selected, dict)
        or set(selected) != {"clang", "ld", "ar", "ranlib", "otool"}
        or any(
            not isinstance(record, dict)
            or set(record) != CANDIDATE_DARWIN_SELECTED_FIELDS
            or not isinstance(record.get("selected_path"), str)
            or not pathlib.Path(record["selected_path"]).is_absolute()
            or (
                record.get("selected_link_target") is not None
                and not isinstance(record.get("selected_link_target"), str)
            )
            or not isinstance(record.get("resolved_path"), str)
            or not pathlib.Path(record["resolved_path"]).is_absolute()
            or not DIGEST.fullmatch(record.get("sha256", ""))
            or type(record.get("size")) is not int
            or record["size"] < 1
            or type(record.get("mode")) is not int
            or record["mode"] & 0o111 == 0
            for record in selected.values()
        )
        or not isinstance(sdk, dict)
        or set(sdk) != {
            "root", "root_identity", "ancestors", "entries", "sha256",
            "selected_path", "selected_link_target",
        }
        or not DIGEST.fullmatch(sdk.get("sha256", ""))
        or type(sdk.get("entries")) is not int
        or sdk["entries"] < 1
        or not isinstance(sdk.get("root"), str)
        or not pathlib.Path(sdk["root"]).is_absolute()
        or not isinstance(sdk.get("root_identity"), dict)
        or set(sdk["root_identity"]) != {"path", "mode", "uid"}
        or sdk["root_identity"].get("path") != sdk["root"]
        or sdk["root_identity"].get("uid") != 0
        or type(sdk["root_identity"].get("mode")) is not int
        or sdk["root_identity"]["mode"] & 0o022
        or not isinstance(sdk.get("ancestors"), list)
        or not sdk["ancestors"]
        or sdk["ancestors"][0] != sdk["root_identity"]
        or any(
            not isinstance(record, dict)
            or set(record) != {"path", "mode", "uid"}
            or not isinstance(record.get("path"), str)
            or not pathlib.Path(record["path"]).is_absolute()
            or record.get("uid") != 0
            or type(record.get("mode")) is not int
            or record["mode"] & 0o022
            for record in sdk["ancestors"]
        )
        or not isinstance(sdk.get("selected_path"), str)
        or not pathlib.Path(sdk["selected_path"]).is_absolute()
        or (
            sdk.get("selected_link_target") is not None
            and not isinstance(sdk.get("selected_link_target"), str)
        )
        or not isinstance(darwin.get("developer_root"), str)
        or not pathlib.Path(darwin["developer_root"]).is_absolute()
        or not isinstance(darwin.get("os_build_version"), str)
        or not darwin["os_build_version"]
    ):
        fail("candidate_provenance_toolchain_invalid")
    for record in sysroot["files"]:
        if not isinstance(record, dict) or record.get("kind") not in {
            "directory", "file", "symlink"
        }:
            fail("candidate_provenance_toolchain_invalid")
        expected = {
            "directory": {"path", "kind", "mode"},
            "file": {"path", "kind", "mode", "size", "sha256"},
            "symlink": {"path", "kind", "target"},
        }[record["kind"]]
        if set(record) != expected or not isinstance(record.get("path"), str):
            fail("candidate_provenance_toolchain_invalid")
        if record["kind"] == "file" and (
            type(record.get("size")) is not int
            or record["size"] < 0
            or not DIGEST.fullmatch(record.get("sha256", ""))
        ):
            fail("candidate_provenance_toolchain_invalid")
    for record in sysroot["linkers"]:
        if (
            not isinstance(record, dict)
            or set(record) != {"path", "mode", "uid", "nlink", "size", "sha256"}
            or not isinstance(record.get("path"), str)
            or not pathlib.Path(record["path"]).is_absolute()
            or type(record.get("mode")) is not int
            or type(record.get("uid")) is not int
            or type(record.get("nlink")) is not int
            or record["nlink"] < 1
            or type(record.get("size")) is not int
            or record["size"] < 1
            or not DIGEST.fullmatch(record.get("sha256", ""))
        ):
            fail("candidate_provenance_toolchain_invalid")
    if sha256_bytes(canonical_json(sysroot["files"]).encode("utf-8")) != sysroot["sha256"]:
        fail("candidate_provenance_toolchain_invalid")
    if sha256_bytes(canonical_json(sysroot["linkers"]).encode("utf-8")) != sysroot["linkers_sha256"]:
        fail("candidate_provenance_toolchain_invalid")


def validate_candidate_publication(
    candidate_spec_path,
    candidate_spec,
    provenance_path,
    provenance,
    provenance_digest,
):
    validate_candidate_spec(candidate_spec)
    bundle = absolute_normal_path(candidate_spec["bundle"], "candidate_bundle", must_exist=True)
    require_owned(bundle, "candidate_bundle", 0o555, directory=True)
    if (
        candidate_spec_path != bundle / "candidate-spec.json"
        or provenance_path != bundle / "provenance.json"
        or candidate_spec["provenance_sha256"] != provenance_digest
    ):
        fail("candidate_publication_binding_invalid")
    if (
        not isinstance(provenance, dict)
        or set(provenance) != CANDIDATE_PROVENANCE_FIELDS
        or type(provenance.get("schema_version")) is not int
        or provenance.get("schema_version") != 1
        or provenance.get("kind") != CANDIDATE_PROVENANCE_KIND
        or provenance.get("status") != "built"
        or provenance.get("release_eligible") is not False
        or provenance.get("commercial_certification") is not False
        or provenance.get("bundle") != str(bundle)
    ):
        fail("candidate_provenance_invalid")
    source = provenance.get("source")
    environment = provenance.get("environment")
    if (
        not isinstance(source, dict)
        or set(source) != CANDIDATE_PROVENANCE_SOURCE_FIELDS
        or source.get("commit") != candidate_spec["commit_sha"]
        or source.get("tree") != candidate_spec["source_tree_sha"]
        or source.get("clean") is not True
        or not isinstance(source.get("root"), str)
        or not pathlib.Path(source["root"]).is_absolute()
        or not valid_candidate_identity(source.get("git"))
        or not isinstance(environment, dict)
        or set(environment) != CANDIDATE_PROVENANCE_ENVIRONMENT_FIELDS
        or environment.get("STARRING_RUNTIME_BUILD_REVISION")
        != candidate_spec["commit_sha"]
    ):
        fail("candidate_provenance_binding_invalid")
    commands = provenance.get("commands")
    toolchain = provenance.get("toolchain")
    builder = provenance.get("builder")
    if (
        not isinstance(commands, list)
        or len(commands) != 5
        or any(
            not isinstance(command, list)
            or not command
            or any(not isinstance(item, str) or not item for item in command)
            for command in commands
        )
        or not isinstance(toolchain, dict)
        or set(toolchain) != {
            "target", "root", "rustc_verbose_version", "tools",
            "sysroot_manifest", "darwin",
        }
        or toolchain.get("target") != "aarch64-apple-darwin"
        or not isinstance(toolchain.get("tools"), dict)
        or set(toolchain["tools"]) != {"cargo", "rustc"}
        or not isinstance(toolchain.get("sysroot_manifest"), dict)
        or not isinstance(toolchain.get("darwin"), dict)
        or not isinstance(builder, dict)
        or set(builder) != {"path", "sha256", "size", "mode", "uid", "links"}
        or not DIGEST.fullmatch(builder.get("sha256", ""))
        or type(builder.get("size")) is not int
        or type(builder.get("mode")) is not int
        or type(builder.get("uid")) is not int
        or type(builder.get("links")) is not int
        or not isinstance(provenance.get("built_at"), str)
        or not UTC_TIMESTAMP.fullmatch(provenance["built_at"])
    ):
        fail("candidate_provenance_binding_invalid")
    validate_candidate_dependencies(provenance.get("dependencies"), source["root"])
    validate_candidate_toolchain(provenance)
    validate_candidate_recipe(provenance)
    expected_records = {
        "artifacts": CANDIDATE_ARTIFACT_NAMES,
        "operators": CANDIDATE_OPERATOR_NAMES,
    }
    for collection, names in expected_records.items():
        records = provenance.get(collection)
        if not isinstance(records, dict) or set(records) != names:
            fail("candidate_provenance_record_invalid")
        for record in records.values():
            if (
                not isinstance(record, dict)
                or set(record) != {"source", "artifact"}
                or not valid_candidate_identity(record.get("source"))
                or not valid_candidate_identity(record.get("artifact"))
            ):
                fail("candidate_provenance_record_invalid")
    worker = provenance.get("worker")
    worker_files = worker.get("files") if isinstance(worker, dict) else None
    if (
        not isinstance(worker, dict)
        or set(worker) != {"tree_sha256", "files"}
        or not DIGEST.fullmatch(worker.get("tree_sha256", ""))
        or not isinstance(worker_files, dict)
        or set(worker_files) != CODEX_WORKER_FILES
    ):
        fail("candidate_provenance_record_invalid")
    for record in worker_files.values():
        if (
            not isinstance(record, dict)
            or set(record) != {"source", "artifact"}
            or not valid_candidate_identity(record.get("source"))
            or not valid_candidate_identity(record.get("artifact"))
        ):
            fail("candidate_provenance_record_invalid")
    builder = provenance["builder"]
    expected_builder = pathlib.Path(source["root"]) / "tools" / "d2-maintenance" / "d2a_candidate.py"
    if pathlib.Path(builder["path"]) != expected_builder:
        fail("candidate_provenance_builder_invalid")
    _builder, observed_builder = candidate_file_identity(
        expected_builder,
        "candidate_builder",
        builder["mode"],
        4 * 1024 * 1024,
        immutable_parent=False,
    )
    if observed_builder != builder:
        fail("candidate_provenance_builder_invalid")
    expected_inventory = {
        "candidate-spec.json",
        "provenance.json",
        "codex-worker",
        *(relative.name for name, relative in CANDIDATE_RELATIVE_PATHS.items() if name != "codex_worker"),
    }
    try:
        if {path.name for path in bundle.iterdir()} != expected_inventory:
            fail("candidate_bundle_inventory_invalid")
    except OSError:
        fail("candidate_bundle_inventory_invalid")
    worker_root = bundle / "codex-worker"
    require_owned(worker_root, "candidate_worker_root", 0o555, directory=True)
    try:
        if {path.name for path in worker_root.iterdir()} != CODEX_WORKER_FILES:
            fail("candidate_worker_inventory_invalid")
    except OSError:
        fail("candidate_worker_inventory_invalid")

    identities = {}
    seen_paths = set()
    for name in sorted(D2A.CANDIDATE_KEYS):
        candidate = candidate_spec["candidates"][name]
        expected_path = bundle / CANDIDATE_RELATIVE_PATHS[name]
        if pathlib.Path(candidate["path"]) != expected_path or expected_path in seen_paths:
            fail("candidate_publication_binding_invalid")
        seen_paths.add(expected_path)
        expected_mode = 0o444 if name == "codex_worker" else 0o555
        path, identity = candidate_file_identity(
            expected_path,
            f"candidate_{name}",
            expected_mode,
        )
        if candidate != {"path": str(path), "sha256": identity["sha256"]}:
            fail("candidate_digest_mismatch")
        provenance_record = candidate_provenance_record(provenance, name)
        if (
            not isinstance(provenance_record, dict)
            or provenance_record.get("artifact") != identity
        ):
            fail("candidate_provenance_record_invalid")
        identities[name] = identity

    worker_files = provenance["worker"]["files"]
    for name in sorted(CODEX_WORKER_FILES - {"worker.mjs"}):
        _path, identity = candidate_file_identity(
            worker_root / name,
            f"candidate_worker_{name}",
            0o444,
            4 * 1024 * 1024,
        )
        record = worker_files.get(name)
        if not isinstance(record, dict) or record.get("artifact") != identity:
            fail("candidate_provenance_record_invalid")
    return identities


def load_candidate_publication(raw_candidate_spec_path):
    candidate_spec_path, candidate_spec, candidate_spec_digest = read_immutable_json(
        raw_candidate_spec_path,
        "candidate_spec",
    )
    provenance_path, provenance, provenance_digest = read_immutable_json(
        candidate_spec_path.with_name("provenance.json"),
        "candidate_provenance",
        MAX_OUTPUT_BYTES,
    )
    validate_candidate_publication(
        candidate_spec_path,
        candidate_spec,
        provenance_path,
        provenance,
        provenance_digest,
    )
    return (
        candidate_spec_path,
        candidate_spec,
        candidate_spec_digest,
        provenance_path,
        provenance,
        provenance_digest,
    )


def ensure_private_directory(path, label):
    path = absolute_normal_path(path, label, must_exist=False)
    if not path.exists():
        try:
            path.mkdir(mode=0o700, parents=True)
        except OSError:
            fail(f"{label}_unavailable")
    absolute_normal_path(path, label, must_exist=True)
    require_owned(path, label, 0o700, directory=True)
    return path


def fsync_directory(path, label):
    flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
        os.fsync(descriptor)
    except OSError:
        fail(f"{label}_fsync_failed")
    finally:
        if "descriptor" in locals():
            os.close(descriptor)


def validate_state(value):
    if (
        not isinstance(value, dict)
        or set(value) != STATE_FIELDS
        or type(value.get("schema_version")) is not int
        or value.get("schema_version") != SCHEMA_VERSION
        or value.get("kind") != STATE_KIND
        or not isinstance(value.get("bootstrap_id"), str)
        or not value["bootstrap_id"].startswith("d2ab-")
        or value.get("status") not in {"running", "recovery_required", "failed", "passed"}
        or value.get("phase") not in PHASES
        or value.get("operation") not in D2A.ALLOWED_OPERATIONS
        or not isinstance(value.get("run_id"), str)
        or not RUN_ID.fullmatch(value["run_id"])
        or value.get("resource_prefix") != f"starring-d2-{value['run_id'][3:11]}-{value['run_id'].rsplit('-', 1)[1]}"
        or value.get("release_eligible") is not False
        or value.get("persistent_sandbox_retained") is not True
        or value.get("last_session_operation") not in {
            None, "auth-smoke", "direct-onboard", "one-shot"
        }
        or any(type(value.get(field)) is not bool for field in (
            "candidate_started", "discord_teardown_complete", "cleanup_complete", "postconditions_complete"
        ))
    ):
        fail("bootstrap_state_invalid")
    for field in (
        "config_path",
        "candidate_spec_path",
        "candidate_provenance_path",
        "manifest_path",
    ):
        absolute_normal_path(value.get(field), f"state_{field}", must_exist=False)
    if (
        pathlib.Path(value["candidate_spec_path"]).name != "candidate-spec.json"
        or pathlib.Path(value["candidate_provenance_path"])
        != pathlib.Path(value["candidate_spec_path"]).with_name("provenance.json")
    ):
        fail("bootstrap_state_invalid")
    manifest_path = pathlib.Path(value["manifest_path"])
    if manifest_path.name != "manifest.json" or manifest_path.parent.name != value["run_id"]:
        fail("bootstrap_state_invalid")
    for field in (
        "config_sha256",
        "candidate_spec_sha256",
        "candidate_provenance_sha256",
        "candidate_dependency_record_sha256",
        "candidate_dependency_tree_sha256",
    ):
        if not isinstance(value.get(field), str) or not DIGEST.fullmatch(value[field]):
            fail("bootstrap_state_invalid")
    for field in ("source_commit_sha", "source_tree_sha"):
        if not isinstance(value.get(field), str) or not COMMIT.fullmatch(value[field]):
            fail("bootstrap_state_invalid")
    if value["manifest_sha256"] is not None and (
        not isinstance(value["manifest_sha256"], str) or not DIGEST.fullmatch(value["manifest_sha256"])
    ):
        fail("bootstrap_state_invalid")
    onboarding_path = value.get("onboarding_evidence_path")
    onboarding_digest = value.get("onboarding_evidence_sha256")
    if (onboarding_path is None) != (onboarding_digest is None):
        fail("bootstrap_state_invalid")
    if onboarding_path is not None:
        evidence_path = absolute_normal_path(
            onboarding_path,
            "state_onboarding_evidence_path",
            must_exist=False,
        )
        if (
            evidence_path
            != manifest_path.with_name("d2a-onboarding-evidence.json")
            or value["manifest_sha256"] is None
            or not isinstance(onboarding_digest, str)
            or not DIGEST.fullmatch(onboarding_digest)
        ):
            fail("bootstrap_state_invalid")
    if (
        (value.get("status") == "passed" or value.get("records"))
        and onboarding_path is None
    ):
        fail("bootstrap_state_invalid")
    tool_digests = value.get("tool_digests")
    if (
        not isinstance(tool_digests, dict)
        or set(tool_digests) != TOOL_DIGEST_FIELDS
        or any(not isinstance(tool_digests[field], str) or not DIGEST.fullmatch(tool_digests[field]) for field in TOOL_DIGEST_FIELDS)
    ):
        fail("bootstrap_state_invalid")
    issuer_environment = value.get("issuer_build_environment")
    if (
        not isinstance(issuer_environment, dict)
        or set(issuer_environment) != ISSUER_BUILD_ENVIRONMENT_FIELDS
        or any(
            not isinstance(item, str) or not item
            for item in issuer_environment.values()
        )
        or sha256_bytes(canonical_json(issuer_environment).encode("utf-8"))
        != tool_digests["issuer_build_environment_sha256"]
    ):
        fail("bootstrap_state_invalid")
    records = value.get("records")
    if not isinstance(records, list) or len(records) > 2:
        fail("bootstrap_state_invalid")
    seen = set()
    for record in records:
        if (
            not isinstance(record, dict)
            or set(record) != RECORD_FIELDS
            or record.get("operation") not in D2A.ALLOWED_OPERATIONS
            or record["operation"] in seen
            or not isinstance(record.get("sha256"), str)
            or not DIGEST.fullmatch(record["sha256"])
            or type(record.get("verified")) is not bool
        ):
            fail("bootstrap_state_invalid")
        absolute_normal_path(record.get("path"), "state_record", must_exist=False)
        seen.add(record["operation"])
    error = value.get("last_error")
    if error is not None and (not isinstance(error, str) or not ERROR_CODE.fullmatch(error)):
        fail("bootstrap_state_invalid")
    if not isinstance(value.get("updated_at"), str) or not value["updated_at"].endswith("Z"):
        fail("bootstrap_state_invalid")
    return value


def write_state(path, value):
    validate_state(value)
    parent = ensure_private_directory(path.parent, "bootstrap_state_root")
    if path.exists() or path.is_symlink():
        absolute_normal_path(path, "bootstrap_state", must_exist=True)
        require_owned(path, "bootstrap_state", 0o600)
    payload = (canonical_json(value) + "\n").encode("utf-8")
    temporary = parent / f".{path.name}.tmp-{secrets.token_hex(8)}"
    descriptor = None
    try:
        descriptor = os.open(
            temporary,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
            0o600,
        )
        write_all(descriptor, payload, "bootstrap_state")
        os.fsync(descriptor)
        os.close(descriptor)
        descriptor = None
        os.replace(temporary, path)
        fsync_directory(parent, "bootstrap_state_root")
    except OSError:
        fail("bootstrap_state_write_failed")
    finally:
        if descriptor is not None:
            os.close(descriptor)
        try:
            if temporary.exists() or temporary.is_symlink():
                temporary.unlink()
        except OSError:
            pass
    require_owned(path, "bootstrap_state", 0o600)


def validate_issuer_build_environment(value, target_root):
    if (
        not isinstance(value, dict)
        or set(value) != ISSUER_BUILD_ENVIRONMENT_FIELDS
        or any(not isinstance(item, str) or not item for item in value.values())
        or value.get("CARGO_INCREMENTAL") != "0"
        or value.get("CARGO_NET_OFFLINE") != "true"
        or value.get("GIT_CONFIG_NOSYSTEM") != "1"
        or value.get("GIT_TERMINAL_PROMPT") != "0"
        or value.get("LC_ALL") != "C"
    ):
        fail("issuer_build_environment_invalid")
    for field in (
        "HOME", "RUSTC", "CC", "CXX", "AR", "RANLIB", "SDKROOT",
        "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER",
    ):
        path = pathlib.Path(value[field])
        if not path.is_absolute() or path != pathlib.Path(os.path.normpath(path)):
            fail("issuer_build_environment_invalid")
    cargo_home = pathlib.Path(value["CARGO_HOME"])
    if (
        not cargo_home.is_absolute()
        or cargo_home.parent != target_root
        or not cargo_home.name.startswith(".cargo-home-")
    ):
        fail("issuer_build_environment_invalid")
    return value


def validate_issuer_build_lifecycle(value, state_root):
    if (
        not isinstance(value, dict)
        or set(value) != ISSUER_BUILD_LIFECYCLE_FIELDS
        or type(value.get("schema_version")) is not int
        or value.get("schema_version") != 1
        or value.get("kind") != "starring.d2a.issuer-build-lifecycle.v1"
        or not isinstance(value.get("build_id"), str)
        or not re.fullmatch(r"d2aib-[0-9a-f]{32}", value["build_id"])
        or value.get("status") not in {"active", "passed", "failed", "quarantined"}
        or not COMMIT.fullmatch(value.get("source_commit", ""))
        or not COMMIT.fullmatch(value.get("source_tree", ""))
        or not DIGEST.fullmatch(value.get("build_environment_sha256", ""))
        or type(value.get("process_group_quiescent")) is not bool
        or not isinstance(value.get("started_at"), str)
        or not UTC_TIMESTAMP.fullmatch(value["started_at"])
    ):
        fail("issuer_build_lifecycle_invalid")
    target = absolute_normal_path(
        value.get("target_dir"), "issuer_build_target", must_exist=False
    )
    if target.parent != state_root or target.name != f".issuer-build-{value['build_id']}":
        fail("issuer_build_lifecycle_invalid")
    environment = validate_issuer_build_environment(
        value.get("build_environment"), target
    )
    if sha256_bytes(canonical_json(environment).encode("utf-8")) != value[
        "build_environment_sha256"
    ]:
        fail("issuer_build_lifecycle_invalid")
    process_group = value.get("process_group_id")
    if process_group is not None and (type(process_group) is not int or process_group <= 0):
        fail("issuer_build_lifecycle_invalid")
    completed = value.get("completed_at")
    error = value.get("error_code")
    if value["status"] == "active":
        if value["process_group_quiescent"] or completed is not None or error is not None:
            fail("issuer_build_lifecycle_invalid")
    elif value["status"] in {"passed", "failed"}:
        if (
            not value["process_group_quiescent"]
            or not isinstance(completed, str)
            or not UTC_TIMESTAMP.fullmatch(completed)
            or (value["status"] == "passed" and error is not None)
            or (
                value["status"] == "failed"
                and (not isinstance(error, str) or not ERROR_CODE.fullmatch(error))
            )
        ):
            fail("issuer_build_lifecycle_invalid")
    elif (
        value["process_group_quiescent"]
        or process_group is None
        or not isinstance(completed, str)
        or not UTC_TIMESTAMP.fullmatch(completed)
        or not isinstance(error, str)
        or not ERROR_CODE.fullmatch(error)
    ):
        fail("issuer_build_lifecycle_invalid")
    return value


def write_private_marker(path, value, label, *, sorted_canonical=True):
    parent = ensure_private_directory(path.parent, f"{label}_root")
    payload = (
        (
            canonical_json(value)
            if sorted_canonical
            else json.dumps(
                value,
                ensure_ascii=False,
                allow_nan=False,
                separators=(",", ":"),
            )
        )
        + "\n"
    ).encode("utf-8")
    temporary = parent / f".{path.name}.tmp-{secrets.token_hex(8)}"
    descriptor = None
    try:
        descriptor = os.open(
            temporary,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
            0o600,
        )
        write_all(descriptor, payload, label)
        os.fsync(descriptor)
        os.close(descriptor)
        descriptor = None
        os.replace(temporary, path)
        fsync_directory(parent, f"{label}_root")
    except OSError:
        fail(f"{label}_write_failed")
    finally:
        if descriptor is not None:
            os.close(descriptor)
        try:
            if temporary.exists() or temporary.is_symlink():
                temporary.unlink()
        except OSError:
            pass
    require_owned(path, label, 0o600)


def write_new_private_marker(path, value, label, *, sorted_canonical=False):
    parent = ensure_private_directory(path.parent, f"{label}_root")
    payload = (
        (
            canonical_json(value)
            if sorted_canonical
            else json.dumps(
                value,
                ensure_ascii=False,
                allow_nan=False,
                separators=(",", ":"),
            )
        )
        + "\n"
    ).encode("utf-8")
    descriptor = None
    try:
        descriptor = os.open(
            path,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL
            | getattr(os, "O_NOFOLLOW", 0),
            0o600,
        )
        write_all(descriptor, payload, label)
        os.fsync(descriptor)
        os.close(descriptor)
        descriptor = None
        fsync_directory(parent, f"{label}_root")
    except FileExistsError:
        fail("manual_recovery_required")
    except OSError:
        fail(f"{label}_write_failed")
    finally:
        if descriptor is not None:
            os.close(descriptor)
    require_owned(path, label, 0o600)


def remove_issuer_build_target(target, state_root):
    target = pathlib.Path(target)
    if target.parent != state_root or not target.name.startswith(".issuer-build-d2aib-"):
        fail("issuer_build_cleanup_invalid")
    if not os.path.lexists(target):
        return
    target = absolute_normal_path(target, "issuer_build_target", must_exist=True)
    try:
        metadata = target.lstat()
    except OSError:
        fail("issuer_build_cleanup_invalid")
    if target.is_symlink() or not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != os.getuid():
        fail("issuer_build_cleanup_invalid")
    try:
        shutil.rmtree(target)
    except OSError:
        fail("issuer_build_cleanup_failed")
    fsync_directory(state_root, "bootstrap_state_root")


def publish_issuer_binary(source, destination):
    source = absolute_normal_path(source, "issuer_build_output", must_exist=True)
    digest = D2A.sha256_file(source, "issuer_build_output", 512 * 1024 * 1024)
    destination = absolute_normal_path(
        destination, "issuer_publish_destination", must_exist=False
    )
    base = destination.parents[2]
    try:
        base_metadata = base.lstat()
    except OSError:
        fail("issuer_publish_root_invalid")
    if (
        base.is_symlink()
        or not stat.S_ISDIR(base_metadata.st_mode)
        or base_metadata.st_uid != os.getuid()
        or stat.S_IMODE(base_metadata.st_mode) != 0o755
    ):
        fail("issuer_publish_root_invalid")
    directory_flags = (
        os.O_RDONLY
        | getattr(os, "O_DIRECTORY", 0)
        | getattr(os, "O_NOFOLLOW", 0)
    )
    descriptors = []
    try:
        current = os.open(base, directory_flags)
        descriptors.append(current)
        for component in ("target", "release"):
            try:
                os.mkdir(component, mode=0o755, dir_fd=current)
                os.fsync(current)
            except FileExistsError:
                pass
            child = os.open(component, directory_flags, dir_fd=current)
            metadata = os.fstat(child)
            descriptors.append(child)
            if (
                not stat.S_ISDIR(metadata.st_mode)
                or metadata.st_uid != os.getuid()
                or stat.S_IMODE(metadata.st_mode) != 0o755
            ):
                for descriptor in reversed(descriptors):
                    os.close(descriptor)
                fail("issuer_publish_root_invalid")
            current = child
        release_descriptor = current
    except OSError:
        for descriptor in reversed(descriptors):
            os.close(descriptor)
        fail("issuer_publish_root_invalid")
    temporary_name = f".{destination.name}.tmp-{secrets.token_hex(8)}"
    source_descriptor = destination_descriptor = None
    try:
        source_descriptor = os.open(source, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
        destination_descriptor = os.open(
            temporary_name,
            os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
            0o700,
            dir_fd=release_descriptor,
        )
        while True:
            chunk = os.read(source_descriptor, 1024 * 1024)
            if not chunk:
                break
            view = memoryview(chunk)
            while view:
                written = os.write(destination_descriptor, view)
                if written <= 0:
                    fail("issuer_publish_failed")
                view = view[written:]
        os.fchmod(destination_descriptor, 0o755)
        os.fsync(destination_descriptor)
        os.close(destination_descriptor)
        destination_descriptor = None
        os.close(source_descriptor)
        source_descriptor = None
        os.replace(
            temporary_name,
            destination.name,
            src_dir_fd=release_descriptor,
            dst_dir_fd=release_descriptor,
        )
        os.fsync(release_descriptor)
    except OSError:
        fail("issuer_publish_failed")
    finally:
        for descriptor in (source_descriptor, destination_descriptor):
            if descriptor is not None:
                os.close(descriptor)
        try:
            os.unlink(temporary_name, dir_fd=release_descriptor)
        except FileNotFoundError:
            pass
        except OSError:
            pass
        for descriptor in reversed(descriptors):
            os.close(descriptor)
    if require_tool(
        destination, "issuer", 0o755, 512 * 1024 * 1024
    ) != digest:
        fail("issuer_publish_failed")
    return digest


def load_state_with_digest(path):
    path, value, digest = read_private_json(path, "bootstrap_state")
    validate_state(value)
    if path.name != f"bootstrap-{value['run_id']}.json":
        fail("bootstrap_state_invalid")
    return path, value, digest


def load_state(path):
    path, value, _digest = load_state_with_digest(path)
    return path, value


def recovery_manifest_bindings(state):
    manifest_path = pathlib.Path(state["manifest_path"])
    _path, manifest, _raw_digest = read_canonical_private_json(
        manifest_path, "manifest"
    )
    if (
        not isinstance(manifest, dict)
        or set(manifest) != D2A.COMMERCIAL_MANIFEST_FIELDS
        or type(manifest.get("schema_version")) is not int
        or manifest.get("schema_version") != 1
        or manifest.get("certification_class") != D2A.COMMERCIAL_CLASS
        or manifest.get("run_id") != state["run_id"]
        or manifest.get("commit_sha") != state["source_commit_sha"]
        or manifest.get("database", {}).get("name")
        != "starring_runtime_staging"
    ):
        fail("audited_recovery_manifest_invalid")
    source_trees = manifest.get("source_trees")
    if (
        not isinstance(source_trees, dict)
        or set(source_trees)
        != {"codex_worker", "d2_toolchain", "certification_transport"}
        or any(
            not isinstance(source_trees[name], dict)
            or not DIGEST.fullmatch(source_trees[name].get("sha256", ""))
            for name in source_trees
        )
    ):
        fail("audited_recovery_manifest_invalid")
    observed = sha256_bytes(canonical_json(manifest).encode("utf-8"))
    if observed != state["manifest_sha256"]:
        fail("audited_recovery_manifest_invalid")
    digest_raw = private_file_bytes(
        manifest_path.with_name("manifest.sha256"),
        "audited_recovery_manifest_digest",
    )
    if digest_raw != (observed + "\n").encode("ascii"):
        fail("audited_recovery_manifest_invalid")
    return manifest, {
        name: source_trees[name]["sha256"] for name in sorted(source_trees)
    }


def valid_recovery_timestamp(value):
    if not isinstance(value, str) or SAFE_RECOVERY_TIMESTAMP.fullmatch(value) is None:
        return False
    try:
        datetime.datetime.fromisoformat(value[:-1] + "+00:00")
    except ValueError:
        return False
    return True


def audited_recovery_artifact_paths(state):
    root = pathlib.Path(state["manifest_path"]).parent / "orchestrator"
    return {
        name: root / filename
        for name, filename in AUDITED_QUARANTINED_RECOVERY_FILENAMES.items()
    }


def audited_recovery_source_interlock_path(state):
    return (
        pathlib.Path(state["manifest_path"]).parent
        / "coordinator"
        / AUDITED_QUARANTINED_SOURCE_INTERLOCK_FILENAME
    )


def audited_recovery_source_interlock_v1_path(state):
    return (
        pathlib.Path(state["manifest_path"]).parent
        / "coordinator"
        / AUDITED_QUARANTINED_SOURCE_INTERLOCK_V1_FILENAME
    )


def audited_recovery_source_interlock_v2_path(state):
    return (
        pathlib.Path(state["manifest_path"]).parent
        / "coordinator"
        / AUDITED_QUARANTINED_SOURCE_INTERLOCK_V2_FILENAME
    )


def audited_recovery_source_transition_v1_path(state):
    return (
        pathlib.Path(state["manifest_path"]).parent
        / "orchestrator"
        / AUDITED_QUARANTINED_SOURCE_TRANSITION_V1_FILENAME
    )


def audited_recovery_source_transition_v2_path(state):
    return (
        pathlib.Path(state["manifest_path"]).parent
        / "orchestrator"
        / AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_FILENAME
    )


def audited_recovery_bootstrap_semantic_sha256(state):
    mutable = {
        "status",
        "phase",
        "discord_teardown_complete",
        "cleanup_complete",
        "postconditions_complete",
        "updated_at",
    }
    return sha256_bytes(
        canonical_json(
            {name: value for name, value in state.items() if name not in mutable}
        ).encode("utf-8")
    )


def read_quarantined_recovery_lifecycle(state):
    path = pathlib.Path(state["manifest_path"]).with_name(
        "d2a-session-lifecycle.json"
    )
    raw = private_file_bytes(path, "audited_recovery_lifecycle")
    try:
        lifecycle = json.loads(
            raw,
            object_pairs_hook=strict_object,
            parse_constant=reject_constant,
        )
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
        fail("audited_recovery_lifecycle_invalid")
    digest = sha256_bytes(raw)
    expected_raw = (
        json.dumps(
            lifecycle,
            ensure_ascii=False,
            allow_nan=False,
            separators=(",", ":"),
        )
        + "\n"
    ).encode("utf-8")
    if (
        not isinstance(lifecycle, dict)
        or tuple(lifecycle) != SESSION_LIFECYCLE_FIELDS
        or raw != expected_raw
        or sha256_bytes(raw) != digest
        or type(lifecycle.get("schema_version")) is not int
        or lifecycle.get("schema_version") != 1
        or lifecycle.get("kind") != "starring.d2a.session-lifecycle.v1"
        or lifecycle.get("run_id") != state["run_id"]
        or lifecycle.get("manifest_sha256") != state["manifest_sha256"]
        or lifecycle.get("operation") != "direct-onboard"
        or lifecycle.get("origin") != "issuer"
        or lifecycle.get("issuer_sha256")
        != state["tool_digests"]["issuer_sha256"]
        or lifecycle.get("issuer_source_sha256")
        != state["tool_digests"]["issuer_source_sha256"]
        or lifecycle.get("uid") != os.getuid()
        or not valid_boot_identity(lifecycle.get("boot_identity"))
        or type(lifecycle.get("process_group_id")) is not int
        or not 1 < lifecycle["process_group_id"] <= 2_147_483_647
        or SESSION_LIFECYCLE_TIMESTAMP.fullmatch(
            lifecycle.get("started_at", "")
        )
        is None
        or lifecycle.get("status") != "quarantined"
        or lifecycle.get("session_revoked") is not False
        or lifecycle.get("revoked_at") is not None
        or SESSION_LIFECYCLE_TIMESTAMP.fullmatch(
            lifecycle.get("quarantined_at", "")
        )
        is None
    ):
        fail("audited_recovery_lifecycle_invalid")
    return lifecycle, digest


def validate_audited_recovery_source(
    value,
    manifest,
    historical_source_trees,
    current_commit,
    current_tree,
    source_root,
):
    if (
        not isinstance(value, dict)
        or set(value)
        != {
            "repository_root",
            "commit_sha",
            "tree_sha",
            "git_path",
            "git_sha256",
            "source_trees",
        }
        or value.get("repository_root") != str(source_root)
        or value.get("commit_sha") != current_commit
        or value.get("tree_sha") != current_tree
        or value.get("git_path") != str(FIXED_GIT)
        or DIGEST.fullmatch(value.get("git_sha256", "")) is None
    ):
        fail("audited_recovery_intent_invalid")
    observations = value.get("source_trees")
    if not isinstance(observations, dict) or set(observations) != set(
        historical_source_trees
    ):
        fail("audited_recovery_intent_invalid")
    for name, observation in observations.items():
        historical = manifest["source_trees"][name]
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
            or observation.get("historical_sha256")
            != historical_source_trees[name]
            or DIGEST.fullmatch(observation.get("observed_sha256", ""))
            is None
            or type(observation.get("matches_historical")) is not bool
            or observation["matches_historical"]
            != (
                observation["observed_sha256"]
                == observation["historical_sha256"]
            )
        ):
            fail("audited_recovery_intent_invalid")
    if (
        observations["codex_worker"]["matches_historical"] is not True
        or all(
            observations[name]["matches_historical"]
            for name in ("d2_toolchain", "certification_transport")
        )
    ):
        fail("audited_recovery_intent_invalid")


def validate_audited_transition_source(
    value,
    manifest,
    historical_source_trees,
    current_commit,
    current_tree,
    source_root,
    code,
):
    try:
        validate_audited_recovery_source(
            value,
            manifest,
            historical_source_trees,
            current_commit,
            current_tree,
            source_root,
        )
    except BootstrapError:
        fail(code)


def audited_recovery_git_bytes(source_root, arguments, label):
    argv = [str(FIXED_GIT), "-C", str(source_root), *arguments]
    try:
        completed = subprocess.run(
            argv,
            cwd="/",
            env={
                "HOME": "/",
                "LANG": "C",
                "LC_ALL": "C",
                "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
                "GIT_CONFIG_GLOBAL": "/dev/null",
                "GIT_CONFIG_NOSYSTEM": "1",
                "GIT_TERMINAL_PROMPT": "0",
                "GIT_OPTIONAL_LOCKS": "0",
            },
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=TOOL_TIMEOUT_SECONDS,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        fail(label)
    if (
        completed.returncode != 0
        or completed.stderr
        or len(completed.stdout) > MAX_OUTPUT_BYTES
    ):
        fail(label)
    return completed.stdout


def audited_recovery_git_output(source_root, arguments, label):
    try:
        return audited_recovery_git_bytes(
            source_root, arguments, label
        ).decode("utf-8").strip()
    except UnicodeDecodeError:
        fail(label)


def validate_audited_source_transition_git(
    transition,
    current_commit,
    current_tree,
    source_root,
    *,
    expected_parent_commit,
    expected_from_file_sha256,
):
    code = "audited_recovery_source_transition_invalid"
    if (
        transition.get("parent_commit_sha")
        != expected_parent_commit
        or transition.get("parent_count") != 1
    ):
        fail(code)
    parent_line = audited_recovery_git_output(
        source_root,
        ["rev-list", "--parents", "-n", "1", current_commit],
        code,
    ).split()
    if parent_line != [current_commit, transition["parent_commit_sha"]]:
        fail(code)
    observed_tree = audited_recovery_git_output(
        source_root,
        ["rev-parse", "--verify", f"{current_commit}^{{tree}}"],
        code,
    )
    if observed_tree != current_tree:
        fail(code)
    changed_paths = audited_recovery_git_output(
        source_root,
        [
            "diff-tree", "--no-commit-id", "--name-only", "-r",
            transition["parent_commit_sha"], current_commit,
        ],
        code,
    ).splitlines()
    if changed_paths != AUDITED_QUARANTINED_CHANGED_PATHS:
        fail(code)
    file_sha256 = transition.get("file_sha256")
    if not isinstance(file_sha256, dict) or list(file_sha256) != sorted(
        AUDITED_QUARANTINED_CHANGED_PATHS
    ):
        fail(code)
    for path in AUDITED_QUARANTINED_CHANGED_PATHS:
        entry = file_sha256.get(path)
        if (
            not isinstance(entry, dict)
            or set(entry) != {"from_sha256", "to_sha256"}
            or entry.get("from_sha256")
            != expected_from_file_sha256[path]
            or DIGEST.fullmatch(entry.get("to_sha256", "")) is None
        ):
            fail(code)
        blob = audited_recovery_git_bytes(
            source_root,
            ["show", f"{current_commit}:{path}"],
            code,
        )
        if sha256_bytes(blob) != entry["to_sha256"]:
            fail(code)


def audited_recovery_transition_audit_configuration(static_sql_sha256=None):
    value = {
        "login_keychain_path": AUDITED_QUARANTINED_LOGIN_KEYCHAIN_PATH,
        "login_keychain_policy_kind": (
            AUDITED_QUARANTINED_LOGIN_KEYCHAIN_POLICY_KIND
        ),
        "login_keychain_policy_sha256": (
            AUDITED_QUARANTINED_LOGIN_KEYCHAIN_POLICY_SHA256
        ),
        "login_keychain_policy_verified": True,
    }
    if static_sql_sha256 is not None:
        value["static_sql_sha256"] = static_sql_sha256
    return value


def audited_recovery_keychain_anchor_inventory_sha256():
    return sha256_bytes(
        canonical_json(AUDITED_QUARANTINED_KEYCHAIN_ANCHORS).encode("utf-8")
    )


def audited_recovery_keychain_inventory(manifest):
    services = manifest.get("keychain_services")
    suffix = manifest.get("run_id", "").rsplit("-", 1)[-1]
    if (
        not isinstance(services, dict)
        or set(services) != set(AUDITED_QUARANTINED_KEYCHAIN_ACCOUNTS)
        or any(
            services.get(name) != f"starring.d2.{suffix}.{name}"
            for name in AUDITED_QUARANTINED_KEYCHAIN_ACCOUNTS
        )
    ):
        fail("audited_recovery_cleanup_keychain_baseline_invalid")
    return [
        (services[name], account)
        for name, accounts in AUDITED_QUARANTINED_KEYCHAIN_ACCOUNTS.items()
        for account in accounts
    ]


def validate_audited_transition_common(
    transition,
    intent,
    intent_sha256,
    state,
    lifecycle_sha256,
    expected_schema_version,
    expected_kind,
    expected_from_source,
    expected_parent_commit,
    expected_from_file_sha256,
    expected_reason_codes,
    expected_audit_configuration,
    previous_source_transition_sha256=None,
):
    boolean_fields = {
        "producer_launchd_jobs_absent",
        "issuer_process_group_absent",
        "transport_identity_verified",
        "transport_effect_admission_drained",
        "postgres_running",
        "protected_staging_unchanged",
        "database_absence_marker_absent",
        "reconciliation_marker_absent",
        "recovery_evidence_absent",
    }
    file_sha256 = transition.get("file_sha256")
    if (
        transition.get("schema_version") != expected_schema_version
        or transition.get("kind") != expected_kind
        or transition.get("run_id") != state["run_id"]
        or transition.get("manifest_sha256") != state["manifest_sha256"]
        or transition.get("intent_sha256") != intent_sha256
        or transition.get("from_source") != expected_from_source
        or transition.get("to_source", {}).get("git_sha256")
        != intent["current_source"]["git_sha256"]
        or transition.get("parent_commit_sha") != expected_parent_commit
        or transition.get("parent_count") != 1
        or transition.get("changed_paths") != AUDITED_QUARANTINED_CHANGED_PATHS
        or not isinstance(file_sha256, dict)
        or list(file_sha256) != sorted(AUDITED_QUARANTINED_CHANGED_PATHS)
        or any(
            not isinstance(file_sha256.get(path), dict)
            or set(file_sha256[path]) != {"from_sha256", "to_sha256"}
            or file_sha256[path].get("from_sha256")
            != expected_from_file_sha256[path]
            or DIGEST.fullmatch(file_sha256[path].get("to_sha256", "")) is None
            for path in AUDITED_QUARANTINED_CHANGED_PATHS
        )
        or transition.get("reason_codes") != expected_reason_codes
        or transition.get("audit_configuration")
        != expected_audit_configuration
        or transition.get("bootstrap_state_semantic_sha256")
        != AUDITED_QUARANTINED_INCIDENT["bootstrap_state_semantic_sha256"]
        or transition.get("orchestrator_state_sha256")
        != AUDITED_QUARANTINED_INCIDENT["orchestrator_state_sha256"]
        or transition.get("baseline_journal_sha256")
        != AUDITED_QUARANTINED_INCIDENT["journal_sha256"]
        or transition.get("baseline_journal_rows")
        != AUDITED_QUARANTINED_INCIDENT["journal_rows"]
        or transition.get("lifecycle_sha256") != lifecycle_sha256
        or transition.get("teardown_fence_sha256")
        != AUDITED_QUARANTINED_INCIDENT["closing_teardown_fence_sha256"]
        or transition.get("transport_instance_id")
        != AUDITED_QUARANTINED_INCIDENT["transport_instance_id"]
        or transition.get("transport_inventory_sha256")
        != AUDITED_QUARANTINED_INCIDENT["empty_transport_inventory_sha256"]
        or transition.get("effect_admission_operation_id")
        != intent["effect_admission_operation_id"]
        or any(transition.get(field) is not True for field in boolean_fields)
        or not valid_recovery_timestamp(transition.get("created_at"))
        or (
            previous_source_transition_sha256 is not None
            and transition.get("previous_source_transition_sha256")
            != previous_source_transition_sha256
        )
    ):
        fail("audited_recovery_source_transition_invalid")


def validate_audited_cleanup_journal(state):
    path = pathlib.Path(state["manifest_path"]).parent / "orchestrator" / "lifecycle.jsonl"
    raw = private_file_bytes(path, "audited_recovery_cleanup_journal")
    lines = raw.splitlines(keepends=True)
    boundary_rows = AUDITED_QUARANTINED_INCIDENT["cleanup_journal_rows"]
    baseline_rows = AUDITED_QUARANTINED_INCIDENT["journal_rows"]
    if (
        len(lines) < boundary_rows
        or boundary_rows != baseline_rows + 2
        or any(not line.endswith(b"\n") for line in lines)
        or sha256_bytes(b"".join(lines[:baseline_rows]))
        != AUDITED_QUARANTINED_INCIDENT["journal_sha256"]
        or sha256_bytes(b"".join(lines[:boundary_rows]))
        != AUDITED_QUARANTINED_INCIDENT["cleanup_journal_sha256"]
    ):
        fail("audited_recovery_cleanup_journal_invalid")
    rows = []
    for index, line in enumerate(lines, 1):
        try:
            row = json.loads(
                line,
                object_pairs_hook=strict_object,
                parse_constant=reject_constant,
            )
        except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
            fail("audited_recovery_cleanup_journal_invalid")
        if (
            not isinstance(row, dict)
            or set(row)
            != {
                "schema_version", "sequence", "manifest_sha256", "action",
                "status", "target", "recorded_at",
            }
            or line != (canonical_json(row) + "\n").encode("utf-8")
            or row.get("schema_version") != 1
            or row.get("sequence") != index
            or row.get("manifest_sha256") != state["manifest_sha256"]
            or not valid_recovery_timestamp(row.get("recorded_at"))
        ):
            fail("audited_recovery_cleanup_journal_invalid")
        rows.append(row)
    expected_boundary = (("intent", 45), ("failed", 46))
    if any(
        rows[sequence - 1].get("action") != "cleanup"
        or rows[sequence - 1].get("status") != status
        or rows[sequence - 1].get("target") != "run"
        for status, sequence in expected_boundary
    ):
        fail("audited_recovery_cleanup_journal_invalid")
    suffix = rows[boundary_rows:]
    for index, row in enumerate(suffix):
        if (
            row.get("action") != "cleanup"
            or row.get("status") not in {"intent", "failed", "complete"}
            or row.get("target") != "run"
            or (
                row.get("status") != "intent"
                and (
                    index == 0
                    or suffix[index - 1].get("status") != "intent"
                )
            )
            or (
                row.get("status") == "complete"
                and index != len(suffix) - 1
            )
        ):
            fail("audited_recovery_cleanup_journal_invalid")
    if not suffix or suffix[-1].get("status") != "complete":
        fail("audited_recovery_cleanup_journal_invalid")
    return AUDITED_QUARANTINED_INCIDENT["cleanup_journal_sha256"]


def validate_audited_cleanup_root_identity(state, manifest):
    path = (
        pathlib.Path(state["manifest_path"]).parent
        / "orchestrator"
        / "cleanup-root-identity.json"
    )
    _path, identity, digest = read_canonical_private_json(
        path, "audited_recovery_cleanup_root_identity"
    )
    expected_root = str(pathlib.Path(manifest["database"]["cluster_root"]).parent)
    if (
        digest
        != AUDITED_QUARANTINED_INCIDENT["cleanup_root_identity_sha256"]
        or not isinstance(identity, dict)
        or set(identity)
        != {
            "schema_version", "kind", "manifest_sha256", "run_id",
            "root_path", "root_device", "root_inode", "parent_device",
            "owner_uid",
        }
        or identity.get("schema_version") != 1
        or identity.get("kind") != "starring.d2.cleanup-root-identity.v1"
        or identity.get("manifest_sha256") != state["manifest_sha256"]
        or identity.get("run_id") != state["run_id"]
        or identity.get("root_path") != expected_root
        or type(identity.get("root_device")) is not int
        or identity["root_device"] < 0
        or type(identity.get("root_inode")) is not int
        or identity["root_inode"] <= 0
        or type(identity.get("parent_device")) is not int
        or identity["parent_device"] != identity["root_device"]
        or identity.get("owner_uid") != os.getuid()
    ):
        fail("audited_recovery_cleanup_root_identity_invalid")
    return identity, digest


def validate_audited_cleanup_terminal_artifacts(
    state, manifest, transition, root_identity, evidence
):
    artifact_directory = pathlib.Path(state["manifest_path"]).parent / "orchestrator"
    baseline_path = artifact_directory / "cleanup-keychain-baseline.json"
    _path, baseline, baseline_sha256 = read_canonical_private_json(
        baseline_path, "audited_recovery_cleanup_keychain_baseline"
    )
    expected_inventory = audited_recovery_keychain_inventory(manifest)
    inventory = baseline.get("inventory") if isinstance(baseline, dict) else None
    if (
        not isinstance(baseline, dict)
        or set(baseline)
        != {"schema_version", "kind", "manifest_sha256", "run_id", "inventory"}
        or type(baseline.get("schema_version")) is not int
        or baseline.get("schema_version") != 1
        or baseline.get("kind")
        != "starring.d2.cleanup-keychain-baseline.v1"
        or baseline.get("manifest_sha256") != state["manifest_sha256"]
        or baseline.get("run_id") != state["run_id"]
        or not isinstance(inventory, list)
        or len(inventory) != transition["keychain_item_count"]
        or len(inventory) != len(expected_inventory)
        or any(
            not isinstance(entry, dict)
            or set(entry) != {"service", "account", "identity_sha256"}
            or (entry.get("service"), entry.get("account"))
            != expected_inventory[index]
            or DIGEST.fullmatch(entry.get("identity_sha256", "")) is None
            for index, entry in enumerate(inventory)
        )
        or sha256_bytes(canonical_json(inventory).encode("utf-8"))
        != transition["keychain_inventory_sha256"]
        or evidence.get("cleanup_keychain_baseline_sha256")
        != baseline_sha256
    ):
        fail("audited_recovery_cleanup_keychain_baseline_invalid")

    progress_path = artifact_directory / "cleanup-root-progress.json"
    _path, progress, progress_sha256 = read_canonical_private_json(
        progress_path, "audited_recovery_cleanup_root_progress"
    )
    root = pathlib.Path(manifest["database"]["cluster_root"]).parent
    expected_root = pathlib.Path("/private/tmp") / f"starring-d2-{state['run_id']}"
    quarantine_name = f".{root.name}.cleanup-{state['manifest_sha256'][:16]}"
    quarantine = root.parent / quarantine_name
    if (
        root != expected_root
        or not isinstance(progress, dict)
        or set(progress)
        != {
            "schema_version", "kind", "manifest_sha256", "run_id",
            "root_device", "root_inode", "quarantine_name", "phase",
        }
        or type(progress.get("schema_version")) is not int
        or progress.get("schema_version") != 1
        or progress.get("kind") != "starring.d2.cleanup-root-progress.v1"
        or progress.get("manifest_sha256") != state["manifest_sha256"]
        or progress.get("run_id") != state["run_id"]
        or type(progress.get("root_device")) is not int
        or progress.get("root_device") != root_identity["root_device"]
        or type(progress.get("root_inode")) is not int
        or progress.get("root_inode") != root_identity["root_inode"]
        or progress.get("quarantine_name") != quarantine_name
        or progress.get("phase") != "deleted"
        or evidence.get("cleanup_root_progress_sha256") != progress_sha256
        or os.path.lexists(root)
        or os.path.lexists(quarantine)
    ):
        fail("audited_recovery_cleanup_root_progress_invalid")
    return baseline_sha256, progress_sha256


def validate_audited_quarantined_recovery_output(
    value,
    state,
    state_path,
    current_commit,
    current_tree,
    source_root,
):
    if not isinstance(value, dict) or set(value) != AUDITED_QUARANTINED_OUTPUT_FIELDS:
        fail("audited_recovery_output_invalid")
    paths = audited_recovery_artifact_paths(state)
    expected_paths = {name: str(path) for name, path in paths.items()}
    if (
        value.get("status") not in {"recovered", "exact_replay"}
        or value.get("phase") != "cleaned"
        or value.get("run_id") != state["run_id"]
        or value.get("manifest_sha256") != state["manifest_sha256"]
        or value.get("intent") != expected_paths["intent"]
        or value.get("source_transition") != expected_paths["source_transition"]
        or DIGEST.fullmatch(value.get("source_transition_sha256", "")) is None
        or value.get("reconciliation") != expected_paths["reconciliation"]
        or value.get("database_absence_evidence")
        != expected_paths["database_absence"]
        or value.get("evidence") != expected_paths["evidence"]
        or TRANSPORT_INSTANCE_ID.fullmatch(
            value.get("transport_instance_id", "")
        )
        is None
        or DIGEST.fullmatch(value.get("transport_inventory_sha256", ""))
        is None
        or value.get("transport_inventory_sha256")
        != AUDITED_QUARANTINED_INCIDENT["empty_transport_inventory_sha256"]
        or DIGEST.fullmatch(value.get("database_absence_sha256", ""))
        is None
        or value.get("source_drift_observed") is not True
        or value.get("cleanup_status")
        != ("cleaned" if value["status"] == "recovered" else "already_cleaned")
        or any(value.get(field) is not True for field in AUDITED_QUARANTINED_ABSENCE_FIELDS)
    ):
        fail("audited_recovery_output_invalid")

    manifest, historical_source_trees = recovery_manifest_bindings(state)
    lifecycle, lifecycle_sha256 = read_quarantined_recovery_lifecycle(state)
    artifacts = {}
    artifact_digests = {}
    fields = {
        "intent": AUDITED_QUARANTINED_INTENT_FIELDS,
        "source_transition": AUDITED_QUARANTINED_SOURCE_TRANSITION_FIELDS,
        "reconciliation": AUDITED_QUARANTINED_RECONCILIATION_FIELDS,
        "database_absence": AUDITED_QUARANTINED_DATABASE_ABSENCE_FIELDS,
        "evidence": AUDITED_QUARANTINED_EVIDENCE_FIELDS,
    }
    for name in (
        "intent", "source_transition", "database_absence",
        "reconciliation", "evidence",
    ):
        _path, artifact, digest = read_canonical_private_json(
            paths[name], f"audited_recovery_{name}"
        )
        if (
            not isinstance(artifact, dict)
            or set(artifact) != fields[name]
            or type(artifact.get("schema_version")) is not int
            or artifact.get("schema_version") != 1
            or artifact.get("kind") != AUDITED_QUARANTINED_RECOVERY_KINDS[name]
            or artifact.get("run_id") != state["run_id"]
            or artifact.get("manifest_sha256") != state["manifest_sha256"]
        ):
            fail(f"audited_recovery_{name}_invalid")
        artifacts[name] = artifact
        artifact_digests[name] = digest

    intent = artifacts["intent"]
    digest_fields = {
        "baseline_bootstrap_state_sha256",
        "orchestrator_state_sha256",
        "baseline_journal_sha256",
        "taint_sha256",
        "lifecycle_sha256",
        "candidate_start_transition_sha256",
        "database_evidence_sha256",
        "step_03_evidence_sha256",
        "pre_intent_transport_inventory_sha256",
    }
    if (
        intent.get("bootstrap_id") != state["bootstrap_id"]
        or intent.get("bootstrap_state_path") != str(state_path)
        or intent.get("baseline_bootstrap_state_sha256")
        != AUDITED_QUARANTINED_INCIDENT["bootstrap_state_sha256"]
        or intent.get("historical_manifest_commit_sha")
        != manifest["commit_sha"]
        or intent.get("historical_source_trees") != historical_source_trees
        or any(
            DIGEST.fullmatch(intent.get(field, "")) is None
            for field in digest_fields
        )
        or intent.get("orchestrator_state_sha256")
        != AUDITED_QUARANTINED_INCIDENT["orchestrator_state_sha256"]
        or intent.get("baseline_journal_sha256")
        != AUDITED_QUARANTINED_INCIDENT["journal_sha256"]
        or intent.get("baseline_journal_rows")
        != AUDITED_QUARANTINED_INCIDENT["journal_rows"]
        or intent.get("taint_sha256")
        != AUDITED_QUARANTINED_INCIDENT["taint_sha256"]
        or intent.get("lifecycle_sha256")
        != AUDITED_QUARANTINED_INCIDENT["lifecycle_sha256"]
        or intent.get("lifecycle_sha256") != lifecycle_sha256
        or intent.get("candidate_start_transition_sha256")
        != AUDITED_QUARANTINED_INCIDENT[
            "candidate_start_transition_sha256"
        ]
        or intent.get("database_evidence_sha256")
        != AUDITED_QUARANTINED_INCIDENT["database_evidence_sha256"]
        or intent.get("step_03_evidence_sha256")
        != AUDITED_QUARANTINED_INCIDENT["step_03_evidence_sha256"]
        or intent.get("transport_instance_id")
        != AUDITED_QUARANTINED_INCIDENT["transport_instance_id"]
        or intent.get("transport_instance_id") != value["transport_instance_id"]
        or intent.get("pre_intent_transport_inventory_sha256")
        != AUDITED_QUARANTINED_INCIDENT[
            "empty_transport_inventory_sha256"
        ]
        or intent.get("effect_admission_operation_id")
        != f"audited-quarantine-recovery:{state['run_id'].rsplit('-', 1)[1]}"
        or intent.get("database_system_identifier")
        != AUDITED_QUARANTINED_INCIDENT["database_system_identifier"]
        or intent.get("safe_after")
        != AUDITED_QUARANTINED_INCIDENT["safe_after"]
        or intent.get("service_identities")
        != AUDITED_QUARANTINED_SERVICE_IDENTITIES
        or intent.get("audit_tools")
        != {
            field: AUDITED_QUARANTINED_INCIDENT[field]
            for field in (
                "zsh_sha256",
                "security_sha256",
                "psql_sha256",
                "static_sql_sha256",
            )
        }
        or intent.get("receipts_sha256")
        != AUDITED_QUARANTINED_INCIDENT["receipts_sha256"]
        or intent.get("coordinator_lock_sha256")
        != AUDITED_QUARANTINED_INCIDENT["coordinator_lock_sha256"]
        or intent.get("coordinator_source_sha256")
        != AUDITED_QUARANTINED_INCIDENT["coordinator_source_sha256"]
        or intent.get("plist_sha256")
        != AUDITED_QUARANTINED_INCIDENT["plist_sha256"]
        or intent.get("tunnel_script_sha256")
        != AUDITED_QUARANTINED_INCIDENT["tunnel_script_sha256"]
        or not valid_recovery_timestamp(intent.get("created_at"))
    ):
        fail("audited_recovery_intent_invalid")
    if (
        artifact_digests["intent"]
        != AUDITED_QUARANTINED_INCIDENT["intent_sha256"]
        or sha256_bytes(
            canonical_json(intent.get("current_source")).encode("utf-8")
        ) != AUDITED_QUARANTINED_INCIDENT["intent_source_sha256"]
    ):
        fail("audited_recovery_intent_invalid")
    validate_audited_recovery_source(
        intent.get("current_source"),
        manifest,
        historical_source_trees,
        AUDITED_QUARANTINED_INCIDENT["intent_source_commit_sha"],
        AUDITED_QUARANTINED_INCIDENT["intent_source_tree_sha"],
        source_root,
    )

    transition_v1_interlock_path = audited_recovery_source_interlock_v1_path(
        state
    )
    _path, transition_v1_interlock, transition_v1_interlock_sha256 = (
        read_canonical_private_json(
            transition_v1_interlock_path,
            "audited_recovery_source_interlock_v1",
        )
    )
    transition_v1_path = audited_recovery_source_transition_v1_path(state)
    _path, transition_v1, transition_v1_sha256 = read_canonical_private_json(
        transition_v1_path, "audited_recovery_source_transition_v1"
    )
    transition_v1_without_interlock = {
        name: transition_v1[name]
        for name in transition_v1
        if name != "interlock_sha256"
    }
    if (
        transition_v1_interlock_sha256
        != AUDITED_QUARANTINED_INCIDENT[
            "source_transition_v1_interlock_sha256"
        ]
        or transition_v1_sha256
        != AUDITED_QUARANTINED_INCIDENT["source_transition_v1_sha256"]
        or not isinstance(transition_v1_interlock, dict)
        or set(transition_v1_interlock)
        != AUDITED_QUARANTINED_SOURCE_INTERLOCK_V1_FIELDS
        or not isinstance(transition_v1, dict)
        or set(transition_v1) != AUDITED_QUARANTINED_SOURCE_TRANSITION_V1_FIELDS
        or transition_v1_interlock.get("kind")
        != AUDITED_QUARANTINED_SOURCE_INTERLOCK_V1_KIND
        or transition_v1.get("kind")
        != AUDITED_QUARANTINED_SOURCE_TRANSITION_V1_KIND
        or transition_v1.get("interlock_sha256")
        != transition_v1_interlock_sha256
        or transition_v1_interlock
        != {
            **transition_v1_without_interlock,
            "kind": AUDITED_QUARANTINED_SOURCE_INTERLOCK_V1_KIND,
        }
    ):
        fail("audited_recovery_source_transition_invalid")
    validate_audited_transition_common(
        transition_v1,
        intent,
        artifact_digests["intent"],
        state,
        lifecycle_sha256,
        1,
        AUDITED_QUARANTINED_SOURCE_TRANSITION_V1_KIND,
        intent["current_source"],
        AUDITED_QUARANTINED_INCIDENT["intent_source_commit_sha"],
        AUDITED_QUARANTINED_CHANGED_PATH_FROM_SHA256,
        AUDITED_QUARANTINED_REASON_CODES_V1,
        audited_recovery_transition_audit_configuration(),
    )
    if any(
        transition_v1["file_sha256"][path]["to_sha256"]
        != AUDITED_QUARANTINED_INCIDENT[
            "source_transition_v1_to_file_sha256"
        ][path]
        for path in AUDITED_QUARANTINED_CHANGED_PATHS
    ):
        fail("audited_recovery_source_transition_invalid")
    validate_audited_transition_source(
        transition_v1.get("to_source"),
        manifest,
        historical_source_trees,
        AUDITED_QUARANTINED_INCIDENT["source_transition_v1_commit_sha"],
        AUDITED_QUARANTINED_INCIDENT["source_transition_v1_tree_sha"],
        source_root,
        "audited_recovery_source_transition_invalid",
    )
    validate_audited_source_transition_git(
        transition_v1,
        AUDITED_QUARANTINED_INCIDENT["source_transition_v1_commit_sha"],
        AUDITED_QUARANTINED_INCIDENT["source_transition_v1_tree_sha"],
        source_root,
        expected_parent_commit=(
            AUDITED_QUARANTINED_INCIDENT["intent_source_commit_sha"]
        ),
        expected_from_file_sha256=(
            AUDITED_QUARANTINED_CHANGED_PATH_FROM_SHA256
        ),
    )

    transition_v2_interlock_path = audited_recovery_source_interlock_v2_path(
        state
    )
    _path, transition_v2_interlock, transition_v2_interlock_sha256 = (
        read_canonical_private_json(
            transition_v2_interlock_path,
            "audited_recovery_source_interlock_v2",
        )
    )
    transition_v2_path = audited_recovery_source_transition_v2_path(state)
    _path, transition_v2, transition_v2_sha256 = read_canonical_private_json(
        transition_v2_path, "audited_recovery_source_transition_v2"
    )
    transition_v2_without_interlock = {
        name: transition_v2[name]
        for name in transition_v2
        if name != "interlock_sha256"
    }
    audit_configuration = audited_recovery_transition_audit_configuration(
        AUDITED_QUARANTINED_INCIDENT[
            "source_transition_v2_static_sql_sha256"
        ]
    )
    if (
        transition_v2_interlock_sha256
        != AUDITED_QUARANTINED_INCIDENT[
            "source_transition_v2_interlock_sha256"
        ]
        or transition_v2_sha256
        != AUDITED_QUARANTINED_INCIDENT["source_transition_v2_sha256"]
        or not isinstance(transition_v2_interlock, dict)
        or set(transition_v2_interlock)
        != AUDITED_QUARANTINED_SOURCE_INTERLOCK_V2_FIELDS
        or not isinstance(transition_v2, dict)
        or set(transition_v2) != AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_FIELDS
        or transition_v2_interlock.get("kind")
        != AUDITED_QUARANTINED_SOURCE_INTERLOCK_V2_KIND
        or transition_v2.get("kind")
        != AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_KIND
        or transition_v2.get("interlock_sha256")
        != transition_v2_interlock_sha256
        or transition_v2_interlock != {
            **transition_v2_without_interlock,
            "kind": AUDITED_QUARANTINED_SOURCE_INTERLOCK_V2_KIND,
        }
    ):
        fail("audited_recovery_source_transition_invalid")
    validate_audited_transition_common(
        transition_v2,
        intent,
        artifact_digests["intent"],
        state,
        lifecycle_sha256,
        2,
        AUDITED_QUARANTINED_SOURCE_TRANSITION_V2_KIND,
        transition_v1["to_source"],
        AUDITED_QUARANTINED_INCIDENT["source_transition_v1_commit_sha"],
        AUDITED_QUARANTINED_INCIDENT["source_transition_v1_to_file_sha256"],
        AUDITED_QUARANTINED_REASON_CODES_V2,
        audit_configuration,
        transition_v1_sha256,
    )
    if any(
        transition_v2["file_sha256"][path]["to_sha256"]
        != AUDITED_QUARANTINED_INCIDENT[
            "source_transition_v2_to_file_sha256"
        ][path]
        for path in AUDITED_QUARANTINED_CHANGED_PATHS
    ):
        fail("audited_recovery_source_transition_invalid")
    validate_audited_transition_source(
        transition_v2.get("to_source"), manifest, historical_source_trees,
        AUDITED_QUARANTINED_INCIDENT["source_transition_v2_commit_sha"],
        AUDITED_QUARANTINED_INCIDENT["source_transition_v2_tree_sha"],
        source_root,
        "audited_recovery_source_transition_invalid",
    )
    validate_audited_source_transition_git(
        transition_v2,
        AUDITED_QUARANTINED_INCIDENT["source_transition_v2_commit_sha"],
        AUDITED_QUARANTINED_INCIDENT["source_transition_v2_tree_sha"],
        source_root,
        expected_parent_commit=(
            AUDITED_QUARANTINED_INCIDENT["source_transition_v1_commit_sha"]
        ),
        expected_from_file_sha256=(
            AUDITED_QUARANTINED_INCIDENT[
                "source_transition_v1_to_file_sha256"
            ]
        ),
    )
    if (
        artifact_digests["database_absence"]
        != AUDITED_QUARANTINED_INCIDENT["database_absence_sha256"]
        or artifact_digests["reconciliation"]
        != AUDITED_QUARANTINED_INCIDENT["reconciliation_sha256"]
    ):
        fail("audited_recovery_source_transition_invalid")
    cleanup_journal_sha256 = validate_audited_cleanup_journal(state)
    cleanup_root_identity, cleanup_root_identity_sha256 = (
        validate_audited_cleanup_root_identity(state, manifest)
    )

    transition = artifacts["source_transition"]
    interlock_path = audited_recovery_source_interlock_path(state)
    _path, interlock, interlock_sha256 = read_canonical_private_json(
        interlock_path, "audited_recovery_source_interlock"
    )
    transition_without_interlock = {
        name: transition[name]
        for name in transition
        if name != "interlock_sha256"
    }
    file_sha256 = transition.get("file_sha256")
    boundary_boolean_fields = {
        "producer_launchd_jobs_absent", "issuer_process_group_absent",
        "postgres_absent", "postgres_process_absent", "isolated_root_retained",
        "cleanup_keychain_baseline_absent", "cleanup_root_progress_absent",
        "cleanup_evidence_absent", "protected_staging_unchanged",
    }
    if (
        not isinstance(interlock, dict)
        or set(interlock) != AUDITED_QUARANTINED_SOURCE_INTERLOCK_FIELDS
        or interlock.get("kind") != AUDITED_QUARANTINED_SOURCE_INTERLOCK_KIND
        or transition.get("interlock_sha256") != interlock_sha256
        or value.get("source_transition_sha256")
        != artifact_digests["source_transition"]
        or interlock != {
            **transition_without_interlock,
            "kind": AUDITED_QUARANTINED_SOURCE_INTERLOCK_KIND,
        }
        or transition.get("schema_version") != 1
        or transition.get("kind")
        != AUDITED_QUARANTINED_RECOVERY_KINDS["source_transition"]
        or transition.get("run_id") != state["run_id"]
        or transition.get("manifest_sha256") != state["manifest_sha256"]
        or transition.get("intent_sha256") != artifact_digests["intent"]
        or transition.get("previous_source_transition_sha256")
        != transition_v2_sha256
        or transition.get("database_absence_sha256")
        != artifact_digests["database_absence"]
        or transition.get("reconciliation_sha256")
        != artifact_digests["reconciliation"]
        or transition.get("from_source") != transition_v2["to_source"]
        or transition.get("to_source", {}).get("git_sha256")
        != intent["current_source"]["git_sha256"]
        or transition.get("parent_commit_sha")
        != AUDITED_QUARANTINED_INCIDENT["source_transition_v2_commit_sha"]
        or transition.get("parent_count") != 1
        or transition.get("changed_paths") != AUDITED_QUARANTINED_CHANGED_PATHS
        or not isinstance(file_sha256, dict)
        or list(file_sha256) != sorted(AUDITED_QUARANTINED_CHANGED_PATHS)
        or any(
            not isinstance(file_sha256.get(path), dict)
            or set(file_sha256[path]) != {"from_sha256", "to_sha256"}
            or file_sha256[path].get("from_sha256")
            != AUDITED_QUARANTINED_INCIDENT[
                "source_transition_v2_to_file_sha256"
            ][path]
            or DIGEST.fullmatch(file_sha256[path].get("to_sha256", "")) is None
            for path in AUDITED_QUARANTINED_CHANGED_PATHS
        )
        or transition.get("reason_codes") != AUDITED_QUARANTINED_REASON_CODES
        or transition.get("audit_configuration") != audit_configuration
        or transition.get("bootstrap_state_semantic_sha256")
        != AUDITED_QUARANTINED_INCIDENT["bootstrap_state_semantic_sha256"]
        or transition.get("orchestrator_state_sha256")
        != AUDITED_QUARANTINED_INCIDENT["orchestrator_state_sha256"]
        or transition.get("baseline_journal_sha256")
        != AUDITED_QUARANTINED_INCIDENT["journal_sha256"]
        or transition.get("baseline_journal_rows")
        != AUDITED_QUARANTINED_INCIDENT["journal_rows"]
        or transition.get("lifecycle_sha256") != lifecycle_sha256
        or transition.get("teardown_fence_sha256")
        != AUDITED_QUARANTINED_INCIDENT["closed_teardown_fence_sha256"]
        or transition.get("cleanup_journal_sha256")
        != cleanup_journal_sha256
        or transition.get("cleanup_journal_rows")
        != AUDITED_QUARANTINED_INCIDENT["cleanup_journal_rows"]
        or transition.get("keychain_inventory_sha256")
        != AUDITED_QUARANTINED_INCIDENT["keychain_inventory_sha256"]
        or transition.get("keychain_item_count")
        != AUDITED_QUARANTINED_INCIDENT["keychain_item_count"]
        or transition.get("keychain_anchor_policy_kind")
        != AUDITED_QUARANTINED_INCIDENT["keychain_anchor_policy_kind"]
        or transition.get("keychain_anchor_inventory_sha256")
        != AUDITED_QUARANTINED_INCIDENT[
            "keychain_anchor_inventory_sha256"
        ]
        or transition.get("keychain_anchor_inventory_sha256")
        != audited_recovery_keychain_anchor_inventory_sha256()
        or transition.get("keychain_anchor_item_count")
        != AUDITED_QUARANTINED_INCIDENT["keychain_anchor_item_count"]
        or transition.get("keychain_anchor_item_count")
        != len(AUDITED_QUARANTINED_KEYCHAIN_ANCHORS)
        or transition.get("cleanup_root_identity_sha256")
        != cleanup_root_identity_sha256
        or any(transition.get(field) is not True for field in boundary_boolean_fields)
        or not valid_recovery_timestamp(transition.get("created_at"))
    ):
        fail("audited_recovery_source_transition_invalid")
    validate_audited_transition_source(
        transition.get("to_source"), manifest, historical_source_trees,
        current_commit, current_tree, source_root,
        "audited_recovery_source_transition_invalid",
    )
    validate_audited_source_transition_git(
        transition,
        current_commit,
        current_tree,
        source_root,
        expected_parent_commit=(
            AUDITED_QUARANTINED_INCIDENT["source_transition_v2_commit_sha"]
        ),
        expected_from_file_sha256=(
            AUDITED_QUARANTINED_INCIDENT[
                "source_transition_v2_to_file_sha256"
            ]
        ),
    )
    fixed_inputs = {
        "candidate_start_transition_sha256": paths["intent"].parent
        / "candidate-start-transition.json",
        "database_evidence_sha256": paths["intent"].parent
        / "database-evidence.json",
        "step_03_evidence_sha256": paths["intent"].parent
        / "step-03-evidence.json",
        "taint_sha256": pathlib.Path(state["manifest_path"]).with_name(
            "d2a-taint.json"
        ),
        "receipts_sha256": pathlib.Path(state["manifest_path"]).with_name(
            "receipts.jsonl"
        ),
        "coordinator_lock_sha256": pathlib.Path(state["manifest_path"]).parent
        / "coordinator"
        / "coordinator.lock",
    }
    for field, path in fixed_inputs.items():
        if (
            sha256_bytes(
                private_file_bytes(
                    path,
                    f"audited_recovery_{field}",
                    allow_empty=field
                    in {"receipts_sha256", "coordinator_lock_sha256"},
                )
            )
            != intent[field]
        ):
            fail("audited_recovery_intent_invalid")
    coordinator_directory = pathlib.Path(state["manifest_path"]).parent / "coordinator"
    require_owned(
        coordinator_directory,
        "audited_recovery_coordinator_directory",
        0o700,
        directory=True,
    )
    try:
        if {path.name for path in coordinator_directory.iterdir()} != {
            "coordinator.lock",
            AUDITED_QUARANTINED_SOURCE_INTERLOCK_V1_FILENAME,
            AUDITED_QUARANTINED_SOURCE_INTERLOCK_V2_FILENAME,
            AUDITED_QUARANTINED_SOURCE_INTERLOCK_FILENAME,
        }:
            fail("audited_recovery_intent_invalid")
    except OSError:
        fail("audited_recovery_intent_invalid")
    coordinator_root = paths["intent"].parent / "coordinator-sources"
    require_owned(
        coordinator_root,
        "audited_recovery_coordinator_source_directory",
        0o700,
        directory=True,
    )
    try:
        coordinator_entries = {
            path.name: path for path in coordinator_root.iterdir()
        }
    except OSError:
        fail("audited_recovery_intent_invalid")
    if set(coordinator_entries) != set(
        AUDITED_QUARANTINED_INCIDENT["coordinator_source_sha256"]
    ):
        fail("audited_recovery_intent_invalid")
    for name, expected_digest in AUDITED_QUARANTINED_INCIDENT[
        "coordinator_source_sha256"
    ].items():
        if sha256_bytes(
            private_file_bytes(
                coordinator_entries[name], "audited_recovery_coordinator_source"
            )
        ) != expected_digest:
            fail("audited_recovery_intent_invalid")
    launchd_root = paths["intent"].parent / "launchd"
    require_owned(
        launchd_root,
        "audited_recovery_launchd_directory",
        0o700,
        directory=True,
    )
    suffix = state["run_id"].rsplit("-", 1)[1]
    expected_plists = {
        f"local.starring.d2.{suffix}.{name}.plist"
        for name in AUDITED_QUARANTINED_INCIDENT["plist_sha256"]
    }
    try:
        if {path.name for path in launchd_root.iterdir()} != expected_plists:
            fail("audited_recovery_intent_invalid")
    except OSError:
        fail("audited_recovery_intent_invalid")
    for name, expected_digest in AUDITED_QUARANTINED_INCIDENT[
        "plist_sha256"
    ].items():
        plist = launchd_root / f"local.starring.d2.{suffix}.{name}.plist"
        if sha256_bytes(
            private_file_bytes(plist, "audited_recovery_launchd_plist")
        ) != expected_digest:
            fail("audited_recovery_intent_invalid")
    if sha256_bytes(
        private_file_bytes(
            paths["intent"].parent / "run-tunnel.zsh",
            "audited_recovery_run_tunnel",
            mode=0o700,
        )
    ) != AUDITED_QUARANTINED_INCIDENT["tunnel_script_sha256"]:
        fail("audited_recovery_intent_invalid")

    database = artifacts["database_absence"]
    count_fields = {
        "oauth_flow_count",
        "auth_session_count",
        "principal_count",
        "tenant_count",
        "installation_count",
        "authority_version_count",
        "runtime_slot_writer_fence_count",
    }
    database_tool_digests = {
        "zsh_sha256", "security_sha256", "psql_sha256", "static_sql_sha256"
    }
    immutable_database_tool_digests = {
        "zsh_sha256", "security_sha256", "psql_sha256"
    }
    if (
        database.get("intent_sha256") != artifact_digests["intent"]
        or database.get("source_transition_sha256")
        != transition_v2_sha256
        or database.get("post_drain_transport_inventory_sha256")
        != artifacts["reconciliation"].get(
            "post_drain_transport_inventory_sha256"
        )
        or database.get("post_drain_transport_inventory_sha256")
        != AUDITED_QUARANTINED_INCIDENT["empty_transport_inventory_sha256"]
        or database.get("database_name") != manifest["database"]["name"]
        or database.get("database_system_identifier")
        != intent["database_system_identifier"]
        or DATABASE_SYSTEM_IDENTIFIER.fullmatch(
            database.get("database_system_identifier", "")
        )
        is None
        or database.get("control_plane_identity")
        != "run_owned_cluster_admin_tcp_v1"
        or any(
            database.get(field) is not True
            for field in (
                "topology_verified",
                "tables_locked",
                "transaction_committed",
                "process_group_quiescent",
            )
        )
        or database.get("locked_tables") != AUDITED_QUARANTINED_LOCKED_TABLES
        or any(type(database.get(field)) is not int or database[field] != 0 for field in count_fields)
        or any(DIGEST.fullmatch(database.get(field, "")) is None for field in database_tool_digests)
        or any(
            database.get(field) != AUDITED_QUARANTINED_INCIDENT[field]
            for field in immutable_database_tool_digests
        )
        or any(
            database.get(field) != intent["audit_tools"].get(field)
            for field in immutable_database_tool_digests
        )
        or database.get("static_sql_sha256")
        != audit_configuration["static_sql_sha256"]
        or database.get("login_keychain_path")
        != audit_configuration["login_keychain_path"]
        or database.get("login_keychain_policy_kind")
        != audit_configuration["login_keychain_policy_kind"]
        or database.get("login_keychain_policy_sha256")
        != audit_configuration["login_keychain_policy_sha256"]
        or database.get("login_keychain_policy_verified") is not True
        or not valid_recovery_timestamp(database.get("observed_at"))
    ):
        fail("audited_recovery_database_absence_invalid")

    reconciliation = artifacts["reconciliation"]
    if (
        reconciliation.get("intent_sha256") != artifact_digests["intent"]
        or reconciliation.get("source_transition_sha256")
        != transition_v2_sha256
        or reconciliation.get("lifecycle_sha256") != lifecycle_sha256
        or reconciliation.get("transport_instance_id")
        != value["transport_instance_id"]
        or reconciliation.get("pre_intent_transport_inventory_sha256")
        != intent["pre_intent_transport_inventory_sha256"]
        or reconciliation.get("effect_admission_operation_id")
        != intent["effect_admission_operation_id"]
        or reconciliation.get("effect_admission_status") != "drained"
        or reconciliation.get("producer_launchd_jobs_absent") is not True
        or reconciliation.get("issuer_process_group_absent") is not True
        or reconciliation.get("postgres_running") is not True
        or reconciliation.get("protected_staging_unchanged") is not True
        or reconciliation.get("database_absence_sha256")
        != artifact_digests["database_absence"]
        or reconciliation.get("final_transport_inventory_sha256")
        != value["transport_inventory_sha256"]
        or reconciliation.get("post_drain_transport_inventory_sha256")
        != AUDITED_QUARANTINED_INCIDENT["empty_transport_inventory_sha256"]
        or not valid_recovery_timestamp(reconciliation.get("observed_at"))
    ):
        fail("audited_recovery_reconciliation_invalid")

    fence_path = pathlib.Path(state["manifest_path"]).with_name(
        "d2a-teardown-fence.json"
    )
    _path, fence, fence_sha256 = read_canonical_private_json(
        fence_path, "audited_recovery_teardown_fence"
    )
    cleanup_path = paths["evidence"].parent / "cleanup-evidence.json"
    _path, cleanup, cleanup_sha256 = read_canonical_private_json(
        cleanup_path, "audited_recovery_cleanup_evidence"
    )
    if (
        fence != {
            "kind": "starring.d2a.teardown-fence.v1",
            "manifest_sha256": state["manifest_sha256"],
            "run_id": state["run_id"],
            "schema_version": 1,
            "status": "closed",
            "updated_at": fence.get("updated_at"),
        }
        or not valid_recovery_timestamp(fence.get("updated_at"))
        or fence_sha256 != transition["teardown_fence_sha256"]
    ):
        fail("audited_recovery_teardown_fence_invalid")
    if (
        not isinstance(cleanup, dict)
        or set(cleanup)
        != {
            "schema_version",
            "manifest_sha256",
            "observed_at",
            *AUDITED_QUARANTINED_ABSENCE_FIELDS,
        }
        or type(cleanup.get("schema_version")) is not int
        or cleanup.get("schema_version") != 1
        or cleanup.get("manifest_sha256") != state["manifest_sha256"]
        or not valid_recovery_timestamp(cleanup.get("observed_at"))
        or any(cleanup.get(field) is not True for field in AUDITED_QUARANTINED_ABSENCE_FIELDS)
    ):
        fail("audited_recovery_cleanup_evidence_invalid")

    evidence = artifacts["evidence"]
    validate_audited_cleanup_terminal_artifacts(
        state, manifest, transition, cleanup_root_identity, evidence
    )
    if (
        evidence.get("intent_sha256") != artifact_digests["intent"]
        or evidence.get("source_transition_sha256")
        != artifact_digests["source_transition"]
        or evidence.get("reconciliation_sha256")
        != artifact_digests["reconciliation"]
        or evidence.get("database_absence_sha256")
        != artifact_digests["database_absence"]
        or evidence.get("lifecycle_sha256") != lifecycle_sha256
        or evidence.get("teardown_fence_sha256") != fence_sha256
        or evidence.get("cleanup_evidence_sha256") != cleanup_sha256
        or any(evidence.get(field) is not True for field in AUDITED_QUARANTINED_ABSENCE_FIELDS)
        or not valid_recovery_timestamp(evidence.get("observed_at"))
        or value["database_absence_sha256"]
        != artifact_digests["database_absence"]
        or value.get("database_absence")
        != {
            field: database[field]
            for field in AUDITED_QUARANTINED_DATABASE_PROJECTION_FIELDS
        }
    ):
        fail("audited_recovery_evidence_invalid")

    expected_postconditions = {
        "status": "observed",
        "phase": "cleaned",
        "postgres_running": False,
        "candidate_launchd_jobs_loaded": 0,
        "candidate_launchd_overrides_absent": True,
        "protected_staging_unchanged": True,
    }
    if value.get("postconditions") != expected_postconditions:
        fail("audited_recovery_postconditions_invalid")
    return value


def require_tool(path, label, mode, maximum):
    path = absolute_normal_path(path, label, must_exist=True)
    require_owned(path, label, mode)
    return D2A.sha256_file(path, label, maximum)


def collect_tool_digests(tool_root=TOOL_ROOT, certification_root=CERTIFICATION_ROOT):
    tool_root = absolute_normal_path(tool_root, "tool_root", must_exist=True)
    certification_root = absolute_normal_path(certification_root, "certification_root", must_exist=True)
    issuer = tool_root / "session-issuer" / "target" / "release" / "starring-d2-session-issuer"
    runner = tool_root / "headless_product_runner.mjs"
    scenario = tool_root / "scenarios" / "study-room.v1.json"
    product_driver = certification_root / "product_driver.js"
    issuer_sha = require_tool(issuer, "issuer", 0o755, 512 * 1024 * 1024)
    return {
        "issuer_sha256": issuer_sha,
        "issuer_source_sha256": D2A.issuer_source_sha256(tool_root),
        "runner_sha256": require_tool(runner, "runner", 0o644, 4 * 1024 * 1024),
        "product_driver_sha256": require_tool(product_driver, "product_driver", 0o644, 4 * 1024 * 1024),
        "scenario_sha256": require_tool(scenario, "scenario", 0o644, 49_152),
    }


def split_keychain_ref(value):
    service, _separator, account = value.partition(":")
    return {"service": service, "account": account}


def reject_commercial_onboarding_artifacts(manifest_path):
    manifest_path = absolute_normal_path(
        manifest_path,
        "direct_onboard_manifest",
        must_exist=True,
    )
    run_directory = manifest_path.parent
    for relative in COMMERCIAL_ONBOARDING_ARTIFACTS:
        if os.path.lexists(run_directory / relative):
            fail("commercial_onboarding_artifact_rejected")


def require_revoked_session_lifecycle(state):
    lifecycle_path = pathlib.Path(state["manifest_path"]).with_name(
        "d2a-session-lifecycle.json"
    )
    if not os.path.lexists(lifecycle_path):
        fail("manual_recovery_required")
    lifecycle_path, lifecycle, lifecycle_digest = read_private_json(
        lifecycle_path, "session_lifecycle"
    )
    if not isinstance(lifecycle, dict):
        fail("manual_recovery_required")
    expected_lifecycle = (
        json.dumps(
            lifecycle,
            ensure_ascii=False,
            allow_nan=False,
            separators=(",", ":"),
        )
        + "\n"
    ).encode("utf-8")
    if lifecycle_digest != sha256_bytes(expected_lifecycle):
        fail("manual_recovery_required")
    expected_operation = state["last_session_operation"]
    if expected_operation not in {"auth-smoke", "direct-onboard", "one-shot"}:
        fail("manual_recovery_required")
    issuer_revoked = (
        lifecycle.get("origin") == "issuer"
        and lifecycle.get("status") == "revoked"
        and lifecycle.get("session_revoked") is True
        and bool(
            SESSION_LIFECYCLE_TIMESTAMP.fullmatch(
                lifecycle.get("revoked_at", "")
            )
        )
        and lifecycle.get("quarantined_at") is None
    )
    issuer_not_issued = (
        lifecycle.get("origin") == "issuer"
        and lifecycle.get("status") == "not_issued"
        and lifecycle.get("session_revoked") is False
        and lifecycle.get("revoked_at") is None
        and lifecycle.get("quarantined_at") is None
    )
    bootstrap_not_issued = (
        lifecycle.get("origin") == "bootstrap"
        and lifecycle.get("operation") == "direct-onboard"
        and lifecycle.get("process_group_id") is None
        and lifecycle.get("status") == "not_issued"
        and lifecycle.get("session_revoked") is False
        and lifecycle.get("revoked_at") is None
        and lifecycle.get("quarantined_at") is None
    )
    if (
        not isinstance(lifecycle, dict)
        or tuple(lifecycle) != SESSION_LIFECYCLE_FIELDS
        or type(lifecycle.get("schema_version")) is not int
        or lifecycle.get("schema_version") != 1
        or lifecycle.get("kind") != "starring.d2a.session-lifecycle.v1"
        or lifecycle.get("run_id") != state["run_id"]
        or lifecycle.get("manifest_sha256") != state["manifest_sha256"]
        or lifecycle.get("operation") != expected_operation
        or lifecycle.get("origin") not in {"bootstrap", "issuer"}
        or lifecycle.get("issuer_sha256") != state["tool_digests"]["issuer_sha256"]
        or lifecycle.get("issuer_source_sha256") != state["tool_digests"]["issuer_source_sha256"]
        or lifecycle.get("uid") != os.getuid()
        or not valid_boot_identity(lifecycle.get("boot_identity"))
        or not SESSION_LIFECYCLE_TIMESTAMP.fullmatch(
            lifecycle.get("started_at", "")
        )
        or not (
            bootstrap_not_issued
            or (
                (issuer_revoked or issuer_not_issued)
                and type(lifecycle.get("process_group_id")) is int
                and 1 < lifecycle["process_group_id"] <= 2_147_483_647
            )
        )
    ):
        fail("manual_recovery_required")
    if bootstrap_not_issued:
        return lifecycle
    # A process group cannot survive a reboot.  Never probe an old numeric pgid
    # on a later boot because macOS can reuse it for an unrelated process.
    if lifecycle["boot_identity"] != current_boot_identity():
        return lifecycle
    try:
        os.killpg(lifecycle["process_group_id"], 0)
    except ProcessLookupError:
        return lifecycle
    except PermissionError:
        fail("manual_recovery_required")
    else:
        # On the same boot, presence or reuse remains a manual boundary.
        fail("manual_recovery_required")


def validate_direct_onboarding_evidence(
    stdout_evidence,
    state,
    config,
    candidate_spec,
):
    manifest_path = pathlib.Path(state["manifest_path"])
    reject_commercial_onboarding_artifacts(manifest_path)
    evidence_path = manifest_path.with_name("d2a-onboarding-evidence.json")
    evidence_path, persisted, evidence_digest = read_private_json(
        evidence_path,
        "direct_onboarding_evidence",
    )
    persisted_payload = private_file_bytes(
        evidence_path,
        "direct_onboarding_evidence",
    )
    if (
        sha256_bytes(persisted_payload) != evidence_digest
        or persisted_payload != (canonical_json(persisted) + "\n").encode("utf-8")
    ):
        fail("direct_onboarding_evidence_not_canonical")
    if stdout_evidence != persisted:
        fail("direct_onboarding_stdout_mismatch")
    discord = config["discord"]
    expected = {
        "schema_version": 1,
        "kind": "starring.d2a.direct-onboarding-evidence.v1",
        "certification_class": D2A.AUTOMATED_CLASS,
        "operation": "direct-onboard",
        "run_id": state["run_id"],
        "manifest_sha256": state["manifest_sha256"],
        "principal_id": f"discord:{discord['actor_id']}",
        "guild_id": discord["guild_id"],
        "discord_application_id": discord["application_id"],
        "hub_channel_id": discord["hub_channel_id"],
        "binding_key": "community_hub",
        "installation_id": f"installation:{state['resource_prefix']}",
        "provisioner_sha256": candidate_spec["candidates"]["sealed_provisioner"]["sha256"],
        "issuer_sha256": state["tool_digests"]["issuer_sha256"],
        "issuer_source_sha256": state["tool_digests"]["issuer_source_sha256"],
        "discord_hub_preflight": True,
        "direct_auth_used": True,
        "session_revoked": True,
        "release_eligible": False,
    }
    if (
        not isinstance(persisted, dict)
        or set(persisted) != DIRECT_ONBOARDING_FIELDS
        or any(persisted.get(key) != value for key, value in expected.items())
        or persisted.get("outcome") not in {"fresh", "exact_replay"}
        or not isinstance(persisted.get("observed_at"), str)
        or not UTC_TIMESTAMP.fullmatch(persisted["observed_at"])
        or type(persisted.get("schema_version")) is not int
        or any(
            type(persisted.get(field)) is not bool
            for field in (
                "discord_hub_preflight",
                "direct_auth_used",
                "session_revoked",
                "release_eligible",
            )
        )
    ):
        fail("direct_onboarding_evidence_invalid")
    try:
        datetime.datetime.strptime(
            persisted["observed_at"][:-1].split(".", 1)[0],
            "%Y-%m-%dT%H:%M:%S",
        )
    except ValueError:
        fail("direct_onboarding_evidence_invalid")
    reject_commercial_onboarding_artifacts(manifest_path)
    return evidence_path, evidence_digest


def load_manifest_for_bootstrap(state, config, candidate_spec, allow_missing_digest=False):
    manifest_path = pathlib.Path(state["manifest_path"])
    _path, manifest, _raw_digest = read_private_json(manifest_path, "manifest")
    if (
        not isinstance(manifest, dict)
        or set(manifest) != D2A.COMMERCIAL_MANIFEST_FIELDS
        or type(manifest.get("schema_version")) is not int
        or manifest.get("schema_version") != 1
        or manifest.get("certification_class") != D2A.COMMERCIAL_CLASS
        or manifest.get("run_id") != state["run_id"]
        or manifest.get("commit_sha") != candidate_spec["commit_sha"]
        or manifest.get("public_origin") != config["cloudflare"]["public_origin"]
    ):
        fail("bootstrap_manifest_invalid")
    expected_discord = config["discord"]
    discord = manifest.get("discord")
    if (
        not isinstance(discord, dict)
        or discord.get("guild_id") != expected_discord["guild_id"]
        or discord.get("hub_channel_id") != expected_discord["hub_channel_id"]
        or discord.get("application_id") != expected_discord["application_id"]
        or discord.get("bot_user_id") != expected_discord["bot_user_id"]
        or discord.get("actor_id") != expected_discord["actor_id"]
        or discord.get("resource_prefix") != state["resource_prefix"]
        or discord.get("disposable_guild_required") is not True
    ):
        fail("bootstrap_manifest_binding_invalid")
    candidates = manifest.get("candidates")
    if not isinstance(candidates, dict) or set(candidates) != D2A.CANDIDATE_KEYS:
        fail("bootstrap_manifest_binding_invalid")
    for name, expected_candidate in candidate_spec["candidates"].items():
        entry = candidates.get(name)
        if (
            not isinstance(entry, dict)
            or set(entry) != {"path", "sha256"}
            or entry != expected_candidate
        ):
            fail("bootstrap_manifest_binding_invalid")
    credentials = config["credential_refs"]
    if manifest.get("external_keychain") != {
        "discord_oauth_client_secret": split_keychain_ref(credentials["discord_oauth"]),
        "discord_bot_token": split_keychain_ref(credentials["discord_bot"]),
        "tunnel_token": split_keychain_ref(credentials["cloudflare_tunnel"]),
    }:
        fail("bootstrap_manifest_binding_invalid")
    services = manifest.get("services")
    ports = config["ports"]
    if (
        not isinstance(services, dict)
        or services.get("api", {}).get("port") != ports["api"]
        or services.get("runtime", {}).get("port") != ports["runtime"]
        or services.get("worker", {}).get("port") != ports["worker"]
        or services.get("transport", {}).get("gateway_port") != ports["transport_gateway"]
        or services.get("transport", {}).get("http_port") != ports["transport_http"]
        or manifest.get("database", {}).get("port") != ports["postgres"]
    ):
        fail("bootstrap_manifest_binding_invalid")
    observed = sha256_bytes(canonical_json(manifest).encode("utf-8"))
    digest_path = manifest_path.with_name("manifest.sha256")
    if digest_path.exists() or digest_path.is_symlink():
        digest_raw = private_file_bytes(digest_path, "manifest_digest")
        try:
            recorded = digest_raw.decode("ascii").strip()
        except UnicodeDecodeError:
            fail("bootstrap_manifest_digest_invalid")
        if recorded != observed:
            fail("bootstrap_manifest_digest_invalid")
    elif not allow_missing_digest:
        fail("bootstrap_manifest_digest_invalid")
    return manifest, observed


def taint_payload(state, manifest_digest):
    digests = state["tool_digests"]
    return D2A.build_taint_marker(
        state["run_id"],
        manifest_digest,
        digests["issuer_sha256"],
        digests["issuer_source_sha256"],
        digests["runner_sha256"],
        digests["product_driver_sha256"],
        digests["scenario_sha256"],
    )[1]


def preissuer_lifecycle_marker(state, manifest_digest):
    digests = state["tool_digests"]
    return {
        "schema_version": 1,
        "kind": "starring.d2a.session-lifecycle.v1",
        "run_id": state["run_id"],
        "manifest_sha256": manifest_digest,
        "operation": "direct-onboard",
        "origin": "bootstrap",
        "issuer_sha256": digests["issuer_sha256"],
        "issuer_source_sha256": digests["issuer_source_sha256"],
        "uid": os.getuid(),
        "boot_identity": current_boot_identity(),
        "process_group_id": None,
        "started_at": lifecycle_timestamp(),
        "status": "not_issued",
        "session_revoked": False,
        "revoked_at": None,
        "quarantined_at": None,
    }


def valid_preissuer_lifecycle(value, state, manifest_digest):
    return (
        isinstance(value, dict)
        and tuple(value) == SESSION_LIFECYCLE_FIELDS
        and type(value.get("schema_version")) is int
        and value.get("schema_version") == 1
        and value.get("kind") == "starring.d2a.session-lifecycle.v1"
        and value.get("run_id") == state["run_id"]
        and value.get("manifest_sha256") == manifest_digest
        and value.get("operation") == "direct-onboard"
        and value.get("origin") == "bootstrap"
        and value.get("issuer_sha256")
        == state["tool_digests"]["issuer_sha256"]
        and value.get("issuer_source_sha256")
        == state["tool_digests"]["issuer_source_sha256"]
        and value.get("uid") == os.getuid()
        and valid_boot_identity(value.get("boot_identity"))
        and value.get("process_group_id") is None
        and bool(
            SESSION_LIFECYCLE_TIMESTAMP.fullmatch(value.get("started_at", ""))
        )
        and value.get("status") == "not_issued"
        and value.get("session_revoked") is False
        and value.get("revoked_at") is None
        and value.get("quarantined_at") is None
    )


def persist_preissuer_lifecycle(
    state,
    manifest_digest,
    *,
    allow_existing_transition=False,
    allow_missing=True,
):
    path = pathlib.Path(state["manifest_path"]).with_name(
        "d2a-session-lifecycle.json"
    )
    if os.path.lexists(path):
        if not allow_existing_transition:
            fail("manual_recovery_required")
        _path, observed, observed_digest = read_private_json(
            path, "session_lifecycle"
        )
        expected = (
            json.dumps(
                observed,
                ensure_ascii=False,
                allow_nan=False,
                separators=(",", ":"),
            )
            + "\n"
        ).encode("utf-8")
        if observed_digest != sha256_bytes(expected):
            fail("manual_recovery_required")
        return observed
    if not allow_missing:
        fail("manual_recovery_required")
    marker = preissuer_lifecycle_marker(state, manifest_digest)
    if not valid_preissuer_lifecycle(marker, state, manifest_digest):
        fail("session_lifecycle_invalid")
    # The global lock excludes issuer takeover and teardown.  Never replace an
    # existing lifecycle: the issuer is the only component permitted to CAS an
    # exact bootstrap sentinel into its active marker.
    if os.path.lexists(path):
        fail("manual_recovery_required")
    write_new_private_marker(
        path,
        marker,
        "session_lifecycle",
        sorted_canonical=False,
    )
    _path, observed, _digest = read_private_json(path, "session_lifecycle")
    if observed != marker:
        fail("session_lifecycle_write_failed")
    return marker


def persist_taint(state, manifest_digest, *, resume=False):
    with d2_global_marker_lock():
        payload = taint_payload(state, manifest_digest)
        path = pathlib.Path(state["manifest_path"]).with_name("d2a-taint.json")
        if path.exists() or path.is_symlink():
            observed = private_file_bytes(path, "d2a_taint")
            if observed != payload:
                fail("early_taint_replay_mismatch")
        else:
            descriptor = None
            try:
                descriptor = os.open(
                    path,
                    os.O_WRONLY | os.O_CREAT | os.O_EXCL
                    | getattr(os, "O_NOFOLLOW", 0),
                    0o600,
                )
                write_all(descriptor, payload, "early_taint")
                os.fsync(descriptor)
                os.close(descriptor)
                descriptor = None
            except OSError:
                fail("early_taint_write_failed")
            finally:
                if descriptor is not None:
                    try:
                        os.close(descriptor)
                    except OSError:
                        pass
            fsync_directory(path.parent, "run_directory")
            if private_file_bytes(path, "d2a_taint") != payload:
                fail("early_taint_write_failed")
        persist_preissuer_lifecycle(
            state,
            manifest_digest,
            allow_existing_transition=resume,
            # A missing lifecycle is recoverable only while the durable state
            # still proves that bootstrap died inside the initial taint write.
            # After that boundary, absence could be deletion of an issuer
            # marker and must remain fail-closed.
            allow_missing=(
                not resume
                or state.get("phase") in {"initialized", "preparing_manifest"}
            ),
        )
    if not resume or state.get("phase") in {"initialized", "preparing_manifest"}:
        state["manifest_sha256"] = manifest_digest
        state["last_session_operation"] = "direct-onboard"
        state["phase"] = "tainted"
    state["updated_at"] = utc_now()
    return state


def write_early_taint(state, config, candidate_spec):
    _manifest, manifest_digest = load_manifest_for_bootstrap(
        state, config, candidate_spec, allow_missing_digest=True
    )
    return persist_taint(state, manifest_digest)


def ensure_resume_taint(state):
    manifest_path = pathlib.Path(state["manifest_path"])
    _path, manifest, _raw_digest = read_private_json(manifest_path, "manifest")
    if (
        not isinstance(manifest, dict)
        or set(manifest) != D2A.COMMERCIAL_MANIFEST_FIELDS
        or type(manifest.get("schema_version")) is not int
        or manifest.get("schema_version") != 1
        or manifest.get("certification_class") != D2A.COMMERCIAL_CLASS
        or manifest.get("run_id") != state["run_id"]
    ):
        fail("bootstrap_manifest_invalid")
    observed = sha256_bytes(canonical_json(manifest).encode("utf-8"))
    if state["manifest_sha256"] is not None and state["manifest_sha256"] != observed:
        fail("bootstrap_manifest_changed")
    digest_path = manifest_path.with_name("manifest.sha256")
    if digest_path.exists() or digest_path.is_symlink():
        try:
            recorded = private_file_bytes(digest_path, "manifest_digest").decode("ascii").strip()
        except UnicodeDecodeError:
            fail("bootstrap_manifest_digest_invalid")
        if recorded != observed:
            fail("bootstrap_manifest_digest_invalid")
    return persist_taint(state, observed, resume=True)


class CommandExecutor:
    def __call__(
        self,
        argv,
        *,
        cwd=None,
        environment=None,
        timeout_seconds=None,
        on_spawn=None,
    ):
        if environment is None:
            environment = {
                "HOME": str(pathlib.Path.home()),
                "PATH": f"{DEFAULT_RUST_TOOLCHAIN_BIN}:/usr/bin:/bin:/usr/sbin:/sbin",
                "GIT_CONFIG_NOSYSTEM": "1",
                "GIT_TERMINAL_PROMPT": "0",
                "LC_ALL": "C",
            }
        if timeout_seconds is None:
            timeout_seconds = COMMAND_TIMEOUT_SECONDS
        return bounded_subprocess(
            argv, cwd, environment, timeout_seconds, on_spawn=on_spawn
        )


def command_identity(argv):
    if not isinstance(argv, list) or len(argv) < 2 or any(not isinstance(item, str) for item in argv):
        fail("subprocess_contract_invalid")
    if pathlib.Path(argv[0]).name == "cargo" and argv[1:] == ["--version"]:
        return "cargo_version"
    if pathlib.Path(argv[0]).name == "rustc" and argv[1:] == ["--version", "--verbose"]:
        return "rustc_verbose_version"
    if (
        pathlib.Path(argv[0]).name in {"otool", "llvm-otool"}
        and len(argv) == 3
        and argv[1] == "-L"
    ):
        absolute_normal_path(argv[2], "issuer_linkage_input", must_exist=True)
        return "issuer_linkage"
    if pathlib.Path(argv[0]).name == "git" and len(argv) >= 5 and argv[1] == "-C":
        command = argv[3:]
        identities = {
            ("rev-parse", "--show-toplevel"): "source_root",
            ("rev-parse", "--verify", "HEAD"): "source_commit",
            ("rev-parse", "--verify", "HEAD^{tree}"): "source_tree",
            ("status", "--porcelain=v1", "--untracked-files=all"): "source_status",
        }
        identity = identities.get(tuple(command))
        if identity is None:
            fail("subprocess_contract_invalid")
        return identity
    if pathlib.Path(argv[0]).name == "cargo" and argv[1:4] == ["build", "--locked", "--release"]:
        if (
            len(argv) != 8
            or argv[4] != "--manifest-path"
            or argv[6] != "--target-dir"
        ):
            fail("subprocess_contract_invalid")
        return "issuer_build"
    if pathlib.Path(argv[0]).name == "starring-d2-session-issuer":
        if (
            len(argv) != 7
            or argv[1] != "--manifest"
            or argv[3:6] != ["--operation", "direct-onboard", "--display-name"]
        ):
            fail("subprocess_contract_invalid")
        absolute_normal_path(argv[2], "direct_onboard_manifest", must_exist=True)
        if not argv[6]:
            fail("subprocess_contract_invalid")
        return "direct_onboard"
    script = pathlib.Path(argv[1]).name
    if script == "d2_certification.py" and len(argv) > 2 and argv[2] == "prepare":
        return "manifest_prepare"
    if script == "d2_preflight_evidence.py":
        return "preflight"
    if script == "isolated_orchestrator.py" and len(argv) > 2 and argv[2] in {
        "dry-run", "prepare", "start", "teardown-discord-resources", "cleanup", "status"
    }:
        return argv[2].replace("-", "_")
    if (
        script == "isolated_orchestrator.py"
        and len(argv) > 2
        and argv[2] == AUDITED_QUARANTINED_RECOVERY_COMMAND
    ):
        flags = (
            "--manifest",
            "--bootstrap-state",
            "--confirm-current-commit",
            "--confirm-current-tree",
            "--confirm-run-id",
            "--confirm-manifest-sha256",
        )
        if len(argv) != 15 or tuple(argv[3::2]) != flags:
            fail("subprocess_contract_invalid")
        absolute_normal_path(
            argv[4], "audited_recovery_manifest", must_exist=True
        )
        absolute_normal_path(
            argv[6], "audited_recovery_bootstrap_state", must_exist=True
        )
        if (
            COMMIT.fullmatch(argv[8]) is None
            or COMMIT.fullmatch(argv[10]) is None
            or RUN_ID.fullmatch(argv[12]) is None
            or DIGEST.fullmatch(argv[14]) is None
        ):
            fail("subprocess_contract_invalid")
        return "recover_audited_quarantined_no_issue"
    if script == "d2a.py" and len(argv) > 2 and argv[2] in {"run", "verify"}:
        if argv[2] == "verify":
            return "d2a_verify"
        try:
            operation = argv[argv.index("--operation") + 1]
        except (ValueError, IndexError):
            fail("subprocess_contract_invalid")
        if operation not in D2A.ALLOWED_OPERATIONS:
            fail("subprocess_contract_invalid")
        return f"d2a_{operation.replace('-', '_')}"
    fail("forbidden_subprocess")


def parse_one_shot_command_error(stdout):
    if (
        type(stdout) is not bytes
        or not stdout
        or len(stdout) > MAX_COMMAND_ERROR_BYTES
        or not stdout.isascii()
    ):
        return None
    try:
        value = json.loads(
            stdout,
            object_pairs_hook=strict_object,
            parse_constant=reject_constant,
        )
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
        return None
    if (
        not isinstance(value, dict)
        or set(value) != COMMAND_ERROR_FIELDS
        or type(value.get("schema_version")) is not int
        or value["schema_version"] != 1
        or value.get("kind") != COMMAND_ERROR_KIND
        or value.get("operation") != "one-shot"
        or value.get("error_code") not in ONE_SHOT_COMMAND_ERROR_CODES
        or stdout != (canonical_json(value) + "\n").encode("ascii")
    ):
        return None
    return value["error_code"]


def parse_teardown_diagnostic(stderr):
    if (
        type(stderr) is not bytes
        or not stderr
        or len(stderr) > MAX_COMMAND_ERROR_BYTES
        or not stderr.isascii()
    ):
        return None
    try:
        value = json.loads(
            stderr,
            object_pairs_hook=strict_object,
            parse_constant=reject_constant,
        )
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
        return None
    if (
        not isinstance(value, dict)
        or set(value) != TEARDOWN_DIAGNOSTIC_FIELDS
        or value.get("status") != "failed"
        or value.get("code") not in TEARDOWN_DIAGNOSTIC_CODES
        or stderr != (canonical_json(value) + "\n").encode("ascii")
    ):
        return None
    code = f"teardown_discord_resources_{value['code']}"
    return code if ERROR_CODE.fullmatch(code) is not None else None


def parse_child_json(completed, label):
    returncode = getattr(completed, "returncode", None)
    stdout = getattr(completed, "stdout", b"")
    stderr = getattr(completed, "stderr", b"")
    if isinstance(stdout, str):
        stdout = stdout.encode("utf-8")
    if isinstance(stderr, str):
        stderr = stderr.encode("utf-8")
    if getattr(completed, "timed_out", False):
        fail(f"{label}_timeout")
    if getattr(completed, "output_exceeded", False):
        if label in {"d2a_one_shot", "teardown_discord_resources"} and returncode != 0:
            fail(f"{label}_failed")
        fail(f"{label}_output_invalid")
    if getattr(completed, "process_group_quiescent", True) is not True:
        fail(f"{label}_process_group_active")
    if type(returncode) is not int or returncode != 0:
        if (
            label == "d2a_one_shot"
            and type(returncode) is int
            and returncode == 1
            and not stderr
        ):
            code = parse_one_shot_command_error(stdout)
            if code is not None:
                fail(code)
        if (
            label == "direct_onboard"
            and type(returncode) is int
            and returncode == 1
            and not stdout
        ):
            try:
                diagnostic = stderr.decode("ascii")
            except UnicodeDecodeError:
                diagnostic = ""
            prefix = "error: "
            if diagnostic.startswith(prefix) and diagnostic.endswith("\n"):
                code = diagnostic[len(prefix):-1]
                if "\n" not in code and code in DIRECT_ONBOARD_ISSUER_ERROR_CODES:
                    fail(code)
        if (
            label == "teardown_discord_resources"
            and type(returncode) is int
            and returncode == 1
            and not stdout
        ):
            code = parse_teardown_diagnostic(stderr)
            if code is not None:
                fail(code)
        fail(f"{label}_failed")
    if not stdout or len(stdout) > MAX_OUTPUT_BYTES or stderr:
        fail(f"{label}_output_invalid")
    try:
        value = json.loads(
            stdout,
            object_pairs_hook=strict_object,
            parse_constant=reject_constant,
        )
    except (UnicodeDecodeError, json.JSONDecodeError, ValueError):
        fail(f"{label}_output_invalid")
    if not isinstance(value, dict):
        fail(f"{label}_output_invalid")
    return value


@contextlib.contextmanager
def bootstrap_lock(path=GLOBAL_LOCK_PATH):
    path = absolute_normal_path(path, "bootstrap_lock", must_exist=False)
    if path == ISSUER_GLOBAL_D2_LOCK_PATH:
        fail("bootstrap_lock_conflict")
    flags = os.O_RDWR | os.O_CREAT | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags, 0o600)
    except OSError:
        fail("bootstrap_lock_unavailable")
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or stat.S_IMODE(metadata.st_mode) != 0o600
        ):
            fail("bootstrap_lock_invalid")
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            fail("bootstrap_lock_busy")
        yield
    finally:
        try:
            fcntl.flock(descriptor, fcntl.LOCK_UN)
        except OSError:
            pass
        os.close(descriptor)


@contextlib.contextmanager
def d2_global_marker_lock(path=ISSUER_GLOBAL_D2_LOCK_PATH):
    path = absolute_normal_path(path, "d2_global_lock", must_exist=False)
    flags = os.O_RDWR | os.O_CREAT | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags, 0o600)
    except OSError:
        fail("d2_global_lock_unavailable")
    try:
        metadata = os.fstat(descriptor)
        if (
            not stat.S_ISREG(metadata.st_mode)
            or metadata.st_uid != os.getuid()
            or metadata.st_nlink != 1
            or stat.S_IMODE(metadata.st_mode) != 0o600
        ):
            fail("d2_global_lock_invalid")
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            fail("d2_global_lock_busy")
        yield
    finally:
        try:
            fcntl.flock(descriptor, fcntl.LOCK_UN)
        except OSError:
            pass
        os.close(descriptor)


class BootstrapController:
    def __init__(
        self,
        executor=None,
        tool_root=TOOL_ROOT,
        certification_root=CERTIFICATION_ROOT,
        lock_path=GLOBAL_LOCK_PATH,
        rust_toolchain_bin=DEFAULT_RUST_TOOLCHAIN_BIN,
        expected_release_root=DEFAULT_RELEASE_RUN_ROOT,
        source_root=None,
        git_path=FIXED_GIT,
        darwin_toolchain_provider=None,
    ):
        self.executor = executor or CommandExecutor()
        self.tool_root = pathlib.Path(tool_root)
        self.certification_root = pathlib.Path(certification_root)
        self.lock_path = pathlib.Path(lock_path)
        self.rust_toolchain_bin = pathlib.Path(rust_toolchain_bin)
        self.expected_release_root = pathlib.Path(expected_release_root)
        self.source_root = pathlib.Path(
            source_root if source_root is not None else self.tool_root.parents[1]
        )
        self.git_path = pathlib.Path(git_path)
        self.darwin_toolchain_provider = darwin_toolchain_provider
        self.python = sys.executable

    def save(self, state_path, state, **changes):
        state.update(changes)
        state["updated_at"] = utc_now()
        write_state(state_path, state)

    def execute(
        self,
        argv,
        *,
        cwd=None,
        environment=None,
        timeout_seconds=None,
        on_spawn=None,
    ):
        if isinstance(self.executor, CommandExecutor):
            return self.executor(
                argv,
                cwd=cwd,
                environment=environment,
                timeout_seconds=timeout_seconds,
                on_spawn=on_spawn,
            )
        completed = self.executor(argv)
        if not hasattr(completed, "process_group_quiescent"):
            completed.process_group_quiescent = True
        return completed

    def invoke(self, argv):
        label = command_identity(argv)
        completed = self.execute(argv)
        return label, parse_child_json(completed, label)

    def source_revision(self, candidate_spec):
        source_root = absolute_normal_path(
            self.source_root,
            "source_root",
            must_exist=True,
        )
        try:
            root_metadata = source_root.lstat()
        except OSError:
            fail("source_root_invalid")
        if (
            source_root.is_symlink()
            or not stat.S_ISDIR(root_metadata.st_mode)
            or root_metadata.st_uid != os.getuid()
            or source_root not in self.tool_root.parents
            or source_root not in self.certification_root.parents
        ):
            fail("source_root_invalid")
        git = absolute_normal_path(self.git_path, "git", must_exist=True)
        try:
            git_metadata = git.lstat()
        except OSError:
            fail("git_invalid")
        expected_uid = 0 if git == FIXED_GIT else os.getuid()
        if (
            git.is_symlink()
            or not stat.S_ISREG(git_metadata.st_mode)
            or git_metadata.st_uid != expected_uid
            or (git != FIXED_GIT and git_metadata.st_nlink != 1)
            or not stat.S_IMODE(git_metadata.st_mode) & 0o111
            or stat.S_IMODE(git_metadata.st_mode) & 0o022
        ):
            fail("git_invalid")

        def git_output(arguments, label, allow_empty=False):
            argv = [str(git), "-C", str(source_root), *arguments]
            if command_identity(argv) != label:
                fail("source_git_contract_invalid")
            completed = self.execute(argv)
            stdout = getattr(completed, "stdout", b"")
            stderr = getattr(completed, "stderr", b"")
            if isinstance(stdout, str):
                stdout = stdout.encode("utf-8")
            if isinstance(stderr, str):
                stderr = stderr.encode("utf-8")
            if (
                getattr(completed, "returncode", None) != 0
                or (not allow_empty and not stdout)
                or len(stdout) > 1024 * 1024
                or stderr
            ):
                fail(f"{label}_invalid")
            try:
                return stdout.decode("utf-8").strip()
            except UnicodeDecodeError:
                fail(f"{label}_invalid")

        observed_root = git_output(
            ["rev-parse", "--show-toplevel"],
            "source_root",
        )
        commit = git_output(["rev-parse", "--verify", "HEAD"], "source_commit")
        tree = git_output(
            ["rev-parse", "--verify", "HEAD^{tree}"],
            "source_tree",
        )
        status = git_output(
            ["status", "--porcelain=v1", "--untracked-files=all"],
            "source_status",
            allow_empty=True,
        )
        if (
            observed_root != str(source_root)
            or commit != candidate_spec["commit_sha"]
            or tree != candidate_spec["source_tree_sha"]
            or status
        ):
            fail("source_revision_mismatch")
        return {"commit": commit, "tree": tree, "clean": True}

    def require_sealed_source_revision(self, candidate_spec, sealed_revision):
        try:
            observed = self.source_revision(candidate_spec)
        except BootstrapError:
            fail("source_changed")
        if observed != sealed_revision:
            fail("source_changed")

    def strict_tool_output(self, argv, label):
        completed = self.execute(argv, timeout_seconds=TOOL_TIMEOUT_SECONDS)
        stdout = getattr(completed, "stdout", b"")
        stderr = getattr(completed, "stderr", b"")
        if isinstance(stdout, str):
            stdout = stdout.encode("utf-8")
        if isinstance(stderr, str):
            stderr = stderr.encode("utf-8")
        if (
            getattr(completed, "timed_out", False)
            or getattr(completed, "output_exceeded", False)
            or getattr(completed, "returncode", None) != 0
            or not stdout
            or stderr
            or len(stdout) > 16 * 1024
        ):
            fail(f"{label}_invalid")
        try:
            value = stdout.decode("utf-8").strip()
        except UnicodeDecodeError:
            fail(f"{label}_invalid")
        if not value or "\n" in value or "\r" in value:
            fail(f"{label}_invalid")
        return value

    def inspect_darwin_toolchain(self):
        if self.darwin_toolchain_provider is not None:
            return self.darwin_toolchain_provider()
        fixed = {}
        for name, path in (
            ("xcrun", FIXED_XCRUN),
            ("xcode_select", FIXED_XCODE_SELECT),
            ("sw_vers", FIXED_SW_VERS),
        ):
            digest, metadata = stable_file_digest(
                path, f"darwin_{name}", expected_uid=0
            )
            if stat.S_IMODE(metadata.st_mode) & 0o111 == 0:
                fail(f"darwin_{name}_invalid")
            fixed[name] = {"path": str(path), "sha256": digest, "size": metadata.st_size}
        developer_root_raw = self.strict_tool_output(
            [str(FIXED_XCODE_SELECT), "-p"], "xcode_select"
        )
        developer_root_path = pathlib.Path(developer_root_raw)
        if not developer_root_path.is_absolute():
            fail("xcode_select_invalid")
        try:
            developer_root = developer_root_path.resolve(strict=True)
            developer_metadata = developer_root.lstat()
        except OSError:
            fail("xcode_select_invalid")
        if (
            not stat.S_ISDIR(developer_metadata.st_mode)
            or developer_metadata.st_uid != 0
            or stat.S_IMODE(developer_metadata.st_mode) & 0o022
        ):
            fail("xcode_select_invalid")
        selected = {}
        for name in ("clang", "ld", "ar", "ranlib", "otool"):
            raw = self.strict_tool_output(
                [str(FIXED_XCRUN), "--find", name], f"xcrun_{name}"
            )
            selector = pathlib.Path(raw)
            if not selector.is_absolute() or selector != pathlib.Path(os.path.normpath(selector)):
                fail("xcrun_tool_invalid")
            try:
                selector_metadata = selector.lstat()
                resolved = selector.resolve(strict=True)
            except OSError:
                fail("xcrun_tool_invalid")
            if developer_root not in resolved.parents:
                fail("xcrun_tool_invalid")
            link_target = os.readlink(selector) if stat.S_ISLNK(selector_metadata.st_mode) else None
            digest, metadata = stable_file_digest(
                resolved, "xcrun_tool", expected_uid=0
            )
            if stat.S_IMODE(metadata.st_mode) & 0o111 == 0:
                fail("xcrun_tool_invalid")
            selected[name] = {
                "selected_path": str(selector),
                "selected_link_target": link_target,
                "resolved_path": str(resolved),
                "sha256": digest,
                "size": metadata.st_size,
                "mode": stat.S_IMODE(metadata.st_mode),
            }
        sdk_raw = self.strict_tool_output(
            [str(FIXED_XCRUN), "--sdk", "macosx", "--show-sdk-path"],
            "xcrun_sdk",
        )
        sdk_selector = pathlib.Path(sdk_raw)
        if not sdk_selector.is_absolute() or sdk_selector != pathlib.Path(os.path.normpath(sdk_selector)):
            fail("xcrun_sdk_invalid")
        try:
            sdk_metadata = sdk_selector.lstat()
            sdk_resolved = sdk_selector.resolve(strict=True)
        except OSError:
            fail("xcrun_sdk_invalid")
        if developer_root not in sdk_resolved.parents or not sdk_resolved.is_dir():
            fail("xcrun_sdk_invalid")
        sdk = rooted_tree_digest(sdk_resolved, "macos_sdk")
        sdk.update({
            "selected_path": str(sdk_selector),
            "selected_link_target": os.readlink(sdk_selector)
            if stat.S_ISLNK(sdk_metadata.st_mode)
            else None,
        })
        os_build = self.strict_tool_output(
            [str(FIXED_SW_VERS), "-buildVersion"], "os_build_version"
        )
        if not re.fullmatch(r"[0-9A-Za-z.]+", os_build):
            fail("os_build_version_invalid")
        return {
            "fixed_tools": fixed,
            "developer_root": str(developer_root),
            "selected_tools": selected,
            "sdk": sdk,
            "os_build_version": os_build,
        }

    def build_issuer(self, state_root, source_revision, candidate_spec):
        state_root = ensure_private_directory(
            pathlib.Path(state_root), "bootstrap_state_root"
        )
        lifecycle_path = state_root / "issuer-build-lifecycle.json"
        if os.path.lexists(lifecycle_path):
            _path, previous, _digest = read_private_json(
                lifecycle_path, "issuer_build_lifecycle"
            )
            validate_issuer_build_lifecycle(previous, state_root)
            if (
                previous["status"] in {"active", "quarantined"}
                or previous["process_group_quiescent"] is not True
            ):
                fail("manual_recovery_required")
            remove_issuer_build_target(previous["target_dir"], state_root)
        manifest = absolute_normal_path(
            self.tool_root / "session-issuer" / "Cargo.toml",
            "issuer_manifest",
            must_exist=True,
        )
        toolchain_bin = absolute_normal_path(
            self.rust_toolchain_bin,
            "rust_toolchain_bin",
            must_exist=True,
        )
        require_owned(toolchain_bin, "rust_toolchain_bin", 0o755, directory=True)
        reject_cargo_configuration(manifest.parent)
        executables = {}
        toolchain_digests = {}
        for name in ("cargo", "rustc"):
            path = absolute_normal_path(
                toolchain_bin / name,
                f"rust_{name}",
                must_exist=True,
            )
            try:
                metadata = path.lstat()
            except OSError:
                fail(f"rust_{name}_unavailable")
            if (
                path.is_symlink()
                or not stat.S_ISREG(metadata.st_mode)
                or metadata.st_uid != os.getuid()
                or metadata.st_nlink != 1
                or not stat.S_IMODE(metadata.st_mode) & 0o111
                or stat.S_IMODE(metadata.st_mode) & 0o022
            ):
                fail(f"rust_{name}_invalid")
            executables[name] = path
            toolchain_digests[f"{name}_sha256"] = D2A.sha256_file(
                path,
                f"rust_{name}",
                512 * 1024 * 1024,
            )
        manifest_before = rust_toolchain_manifest(toolchain_bin)
        darwin_before = self.inspect_darwin_toolchain()
        toolchain_digests["rust_sysroot_sha256"] = manifest_before["sha256"]
        toolchain_digests["rust_linkers_sha256"] = manifest_before["linkers_sha256"]
        toolchain_digests["darwin_toolchain_sha256"] = sha256_bytes(
            canonical_json(darwin_before).encode("utf-8")
        )
        toolchain_digests["macos_sdk_sha256"] = darwin_before["sdk"]["sha256"]
        cargo_result = self.execute(
            [str(executables["cargo"]), "--version"],
            timeout_seconds=TOOL_TIMEOUT_SECONDS,
        )
        cargo_stdout = self.parse_tool_version_output(cargo_result, "cargo_version")
        if not cargo_stdout.startswith("cargo 1.97.0 ") or "\n" in cargo_stdout:
            fail("cargo_version_invalid")
        rustc_result = self.execute([
            str(executables["rustc"]),
            "--version",
            "--verbose",
        ], timeout_seconds=TOOL_TIMEOUT_SECONDS)
        rustc_stdout = self.parse_tool_version_output(
            rustc_result,
            "rustc_verbose_version",
        )
        rustc_fields = {
            key: value
            for line in rustc_stdout.splitlines()
            if ": " in line
            for key, value in [line.split(": ", 1)]
        }
        if (
            not rustc_stdout.splitlines()
            or not rustc_stdout.splitlines()[0].startswith("rustc 1.97.0 ")
            or rustc_fields.get("release") != "1.97.0"
            or rustc_fields.get("host") != "aarch64-apple-darwin"
        ):
            fail("rustc_version_invalid")
        build_id = f"d2aib-{secrets.token_hex(16)}"
        target_root = state_root / f".issuer-build-{build_id}"
        try:
            target_root.mkdir(mode=0o700)
        except OSError:
            fail("issuer_target_unavailable")
        lifecycle = {
            "schema_version": 1,
            "kind": "starring.d2a.issuer-build-lifecycle.v1",
            "build_id": build_id,
            "status": "active",
            "source_commit": source_revision["commit"],
            "source_tree": source_revision["tree"],
            "target_dir": str(target_root),
            "process_group_id": None,
            "process_group_quiescent": False,
            "build_environment": None,
            "build_environment_sha256": "0" * 64,
            "started_at": utc_now(),
            "completed_at": None,
            "error_code": None,
        }
        argv = [
            str(executables["cargo"]),
            "build",
            "--locked",
            "--release",
            "--manifest-path",
            str(manifest),
            "--target-dir",
            str(target_root),
        ]
        if command_identity(argv) != "issuer_build":
            fail("issuer_build_contract_invalid")
        try:
            cargo_home = prepare_isolated_cargo_home(
                target_root, secrets.token_hex(16)
            )
        except BaseException:
            remove_issuer_build_target(target_root, state_root)
            raise
        build_environment = {
            "HOME": str(pathlib.Path.home()),
            "PATH": f"{toolchain_bin}:/usr/bin:/bin:/usr/sbin:/sbin",
            "CARGO_HOME": str(cargo_home),
            "CARGO_INCREMENTAL": "0",
            "CARGO_NET_OFFLINE": "true",
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_TERMINAL_PROMPT": "0",
            "LC_ALL": "C",
            "RUSTC": str(executables["rustc"]),
            "CC": darwin_before["selected_tools"]["clang"]["resolved_path"],
            "CXX": darwin_before["selected_tools"]["clang"]["resolved_path"],
            "AR": darwin_before["selected_tools"]["ar"]["resolved_path"],
            "RANLIB": darwin_before["selected_tools"]["ranlib"]["resolved_path"],
            "SDKROOT": darwin_before["sdk"]["root"],
            "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER": darwin_before["selected_tools"]["clang"]["resolved_path"],
        }
        build_environment_sha256 = sha256_bytes(
            canonical_json(build_environment).encode("utf-8")
        )
        lifecycle["build_environment_sha256"] = build_environment_sha256
        lifecycle["build_environment"] = dict(build_environment)
        toolchain_digests["issuer_build_environment_sha256"] = (
            build_environment_sha256
        )
        validate_issuer_build_lifecycle(lifecycle, state_root)
        completed = None

        def record_process_group(process_group):
            lifecycle["process_group_id"] = process_group
            write_private_marker(
                lifecycle_path, lifecycle, "issuer_build_lifecycle"
            )

        write_private_marker(
            lifecycle_path, lifecycle, "issuer_build_lifecycle"
        )
        try:
            completed = self.execute(
                argv,
                cwd=manifest.parent,
                environment=build_environment,
                timeout_seconds=BUILD_TIMEOUT_SECONDS,
                on_spawn=record_process_group,
            )
            stdout = getattr(completed, "stdout", b"")
            stderr = getattr(completed, "stderr", b"")
            if isinstance(stdout, str):
                stdout = stdout.encode("utf-8")
            if isinstance(stderr, str):
                stderr = stderr.encode("utf-8")
            if getattr(completed, "timed_out", False):
                fail("issuer_build_timeout")
            if (
                getattr(completed, "output_exceeded", False)
                or getattr(completed, "process_group_quiescent", True) is not True
                or getattr(completed, "returncode", None) != 0
                or len(stdout) > MAX_OUTPUT_BYTES
                or len(stderr) > MAX_OUTPUT_BYTES
            ):
                fail("issuer_build_failed")
        except BaseException as error:
            group_quiescent = (
                getattr(completed, "process_group_quiescent", False) is True
                if completed is not None
                else (
                    lifecycle["process_group_id"] is None
                    or not process_group_exists(lifecycle["process_group_id"])
                )
            )
            lifecycle.update(
                status="failed" if group_quiescent else "quarantined",
                process_group_quiescent=group_quiescent,
                completed_at=utc_now(),
                error_code=(
                    error.code
                    if isinstance(error, BootstrapError)
                    else "bootstrap_interrupted"
                    if isinstance(error, KeyboardInterrupt)
                    else "bootstrap_internal_error"
                ),
            )
            write_private_marker(
                lifecycle_path, lifecycle, "issuer_build_lifecycle"
            )
            if group_quiescent:
                try:
                    metadata = cargo_home.lstat()
                    if (
                        cargo_home.is_symlink()
                        or not stat.S_ISDIR(metadata.st_mode)
                        or metadata.st_uid != os.getuid()
                        or cargo_home.parent != target_root
                        or not cargo_home.name.startswith(".cargo-home-")
                    ):
                        fail("cargo_home_cleanup_invalid")
                    shutil.rmtree(cargo_home)
                except FileNotFoundError:
                    pass
                except OSError:
                    fail("cargo_home_cleanup_failed")
            raise
        try:
            metadata = cargo_home.lstat()
            if (
                cargo_home.is_symlink()
                or not stat.S_ISDIR(metadata.st_mode)
                or metadata.st_uid != os.getuid()
                or cargo_home.parent != target_root
                or not cargo_home.name.startswith(".cargo-home-")
            ):
                fail("cargo_home_cleanup_invalid")
            shutil.rmtree(cargo_home)
            for name, path in executables.items():
                try:
                    metadata = path.lstat()
                except OSError:
                    fail("rust_toolchain_changed")
                if (
                    path.is_symlink()
                    or not stat.S_ISREG(metadata.st_mode)
                    or metadata.st_uid != os.getuid()
                    or metadata.st_nlink != 1
                    or not stat.S_IMODE(metadata.st_mode) & 0o111
                    or stat.S_IMODE(metadata.st_mode) & 0o022
                    or D2A.sha256_file(path, f"rust_{name}", 512 * 1024 * 1024)
                    != toolchain_digests[f"{name}_sha256"]
                ):
                    fail("rust_toolchain_changed")
            manifest_after = rust_toolchain_manifest(toolchain_bin)
            darwin_after = self.inspect_darwin_toolchain()
            if manifest_after != manifest_before or darwin_after != darwin_before:
                fail("rust_toolchain_changed")
            reject_cargo_configuration(manifest.parent)
            post_cargo_stdout = self.parse_tool_version_output(
                self.execute(
                    [str(executables["cargo"]), "--version"],
                    timeout_seconds=TOOL_TIMEOUT_SECONDS,
                ),
                "cargo_version",
            )
            post_rustc_stdout = self.parse_tool_version_output(
                self.execute(
                    [str(executables["rustc"]), "--version", "--verbose"],
                    timeout_seconds=TOOL_TIMEOUT_SECONDS,
                ),
                "rustc_verbose_version",
            )
            if post_cargo_stdout != cargo_stdout or post_rustc_stdout != rustc_stdout:
                fail("rust_toolchain_changed")
            built_issuer = (
                target_root / "release" / "starring-d2-session-issuer"
            )
            self.validate_macho_linkage(built_issuer, darwin_before)
            # The credential-capable fixed path is the final mutation.  The
            # repository is checked immediately on both sides of the atomic
            # replacement, while every binary/linker/toolchain check is made
            # against the private target first.
            self.require_sealed_source_revision(candidate_spec, source_revision)
            published_issuer = (
                manifest.parent
                / "target"
                / "release"
                / "starring-d2-session-issuer"
            )
            publish_issuer_binary(built_issuer, published_issuer)
            self.require_sealed_source_revision(candidate_spec, source_revision)
        except BaseException as error:
            lifecycle.update(
                status="failed",
                process_group_quiescent=True,
                completed_at=utc_now(),
                error_code=(
                    error.code
                    if isinstance(error, BootstrapError)
                    else "bootstrap_interrupted"
                    if isinstance(error, KeyboardInterrupt)
                    else "bootstrap_internal_error"
                ),
            )
            write_private_marker(
                lifecycle_path, lifecycle, "issuer_build_lifecycle"
            )
            raise
        lifecycle.update(
            status="passed",
            process_group_quiescent=True,
            completed_at=utc_now(),
            error_code=None,
        )
        write_private_marker(
            lifecycle_path, lifecycle, "issuer_build_lifecycle"
        )
        return toolchain_digests, dict(build_environment)

    def validate_macho_linkage(self, path, darwin_toolchain):
        path = absolute_normal_path(path, "issuer_linkage_input", must_exist=True)
        argv = [
            darwin_toolchain["selected_tools"]["otool"]["resolved_path"],
            "-L",
            str(path),
        ]
        if command_identity(argv) != "issuer_linkage":
            fail("issuer_linkage_invalid")
        completed = self.execute(argv, timeout_seconds=TOOL_TIMEOUT_SECONDS)
        stdout = getattr(completed, "stdout", b"")
        stderr = getattr(completed, "stderr", b"")
        if isinstance(stdout, str):
            stdout = stdout.encode("utf-8")
        if isinstance(stderr, str):
            stderr = stderr.encode("utf-8")
        if (
            getattr(completed, "returncode", None) != 0
            or getattr(completed, "timed_out", False)
            or getattr(completed, "output_exceeded", False)
            or stderr
        ):
            fail("issuer_linkage_invalid")
        try:
            lines = stdout.decode("utf-8").splitlines()
        except UnicodeDecodeError:
            fail("issuer_linkage_invalid")
        if not lines or lines[0] != f"{path}:" or len(lines) < 2:
            fail("issuer_linkage_invalid")
        for line in lines[1:]:
            dependency = line.strip().split(" ", 1)[0] if line.startswith("\t") else ""
            if not dependency.startswith(
                (
                    "/usr/lib/",
                    "/System/Library/Frameworks/",
                    "/System/Library/PrivateFrameworks/",
                )
            ):
                fail("issuer_linkage_invalid")

    @staticmethod
    def parse_tool_version_output(completed, label):
        stdout = getattr(completed, "stdout", b"")
        stderr = getattr(completed, "stderr", b"")
        if isinstance(stdout, str):
            stdout = stdout.encode("utf-8")
        if isinstance(stderr, str):
            stderr = stderr.encode("utf-8")
        if (
            getattr(completed, "timed_out", False)
            or getattr(completed, "output_exceeded", False)
            or getattr(completed, "process_group_quiescent", True) is not True
            or getattr(completed, "returncode", None) != 0
            or not stdout
            or len(stdout) > 16 * 1024
            or stderr
        ):
            fail(f"{label}_invalid")
        try:
            return stdout.decode("ascii").strip()
        except UnicodeDecodeError:
            fail(f"{label}_invalid")

    def orchestrator(self, command, manifest, *extra):
        return self.invoke([
            self.python,
            str(self.certification_root / "isolated_orchestrator.py"),
            command,
            "--manifest",
            str(manifest),
            *extra,
        ])[1]

    def direct_onboard(self, state_path, state, config, candidate_spec):
        manifest_path = pathlib.Path(state["manifest_path"])
        reject_commercial_onboarding_artifacts(manifest_path)
        evidence_path = manifest_path.with_name("d2a-onboarding-evidence.json")
        if os.path.lexists(evidence_path):
            fail("direct_onboarding_evidence_collision")
        issuer = absolute_normal_path(
            self.tool_root
            / "session-issuer"
            / "target"
            / "release"
            / "starring-d2-session-issuer",
            "issuer",
            must_exist=True,
        )
        if (
            require_tool(issuer, "issuer", 0o755, 512 * 1024 * 1024)
            != state["tool_digests"]["issuer_sha256"]
        ):
            fail("issuer_changed")
        _label, stdout_evidence = self.invoke([
            str(issuer),
            "--manifest",
            str(manifest_path),
            "--operation",
            "direct-onboard",
            "--display-name",
            config["discord"]["actor_display_name"],
        ])
        evidence_path, evidence_digest = validate_direct_onboarding_evidence(
            stdout_evidence,
            state,
            config,
            candidate_spec,
        )
        self.save(
            state_path,
            state,
            onboarding_evidence_path=str(evidence_path),
            onboarding_evidence_sha256=evidence_digest,
        )

    def prepare_argv(self, state, config, candidate_spec):
        discord = config["discord"]
        credentials = config["credential_refs"]
        argv = [
            self.python,
            str(self.certification_root / "d2_certification.py"),
            "prepare",
            "--output-root", config["release_run_root"],
            "--commit", candidate_spec["commit_sha"],
            "--run-id", state["run_id"],
            "--discord-guild-id", discord["guild_id"],
            "--discord-hub-channel-id", discord["hub_channel_id"],
            "--discord-application-id", discord["application_id"],
            "--discord-bot-user-id", discord["bot_user_id"],
            "--discord-actor-id", discord["actor_id"],
            "--discord-oauth-keychain", credentials["discord_oauth"],
            "--discord-bot-keychain", credentials["discord_bot"],
            "--tunnel-token-keychain", credentials["cloudflare_tunnel"],
            "--cloudflare-tunnel-id", config["cloudflare"]["tunnel_id"],
            "--public-origin", config["cloudflare"]["public_origin"],
        ]
        for name in sorted(D2A.CANDIDATE_KEYS):
            argv.extend((
                "--candidate",
                f"{name}={candidate_spec['candidates'][name]['path']}",
            ))
        for name in sorted(PORT_FIELDS):
            argv.extend(("--port", f"{name}={config['ports'][name]}"))
        return argv

    def prepare_manifest(self, state_path, state, config, candidate_spec):
        self.save(state_path, state, phase="preparing_manifest")
        command_error = None
        output = None
        try:
            _label, output = self.invoke(self.prepare_argv(state, config, candidate_spec))
        except BaseException as error:
            command_error = error
        manifest_path = pathlib.Path(state["manifest_path"])
        if manifest_path.exists() or manifest_path.is_symlink():
            write_early_taint(state, config, candidate_spec)
            self.save(state_path, state)
        if command_error is not None:
            raise command_error
        expected = {
            "run_id": state["run_id"],
            "manifest": state["manifest_path"],
            "receipts": str(manifest_path.with_name("receipts.jsonl")),
            "resource_prefix": state["resource_prefix"],
        }
        if output != expected:
            fail("manifest_prepare_output_invalid")
        private_file_bytes(manifest_path.with_name("receipts.jsonl"), "receipts", allow_empty=True)
        load_manifest_for_bootstrap(state, config, candidate_spec)
        if state["phase"] != "tainted":
            fail("early_taint_missing")

    def validate_stage(self, stage, value, state, config):
        if stage == "dry_run":
            if value.get("status") != "ready" or value.get("standing_mutation_allowed") is not False:
                fail("dry_run_result_invalid")
        elif stage == "preflight":
            if value.get("status") not in {"recorded", "exact_replay"} or value.get("manifest_sha256") != state["manifest_sha256"]:
                fail("preflight_result_invalid")
        elif stage == "prepare":
            if value.get("status") not in {"prepared", "already_prepared"} or value.get("phase") not in {"prepared", "substrate_started", "stopped"}:
                fail("prepare_result_invalid")
        elif stage == "start":
            if value.get("status") not in {"candidate_started", "already_started", "recovered_started"} or value.get("phase") != "candidate_started":
                fail("start_result_invalid")

    def run_d2a(self, operation, state, config):
        _label, output = self.invoke([
            self.python,
            str(self.tool_root / "d2a.py"),
            "run",
            "--manifest", state["manifest_path"],
            "--operation", operation,
            "--output-root", config["d2a_result_root"],
        ])
        if set(output) != {"status", "release_eligible", "result", "evidence_sha256"} or output.get("status") != "passed" or output.get("release_eligible") is not False:
            fail(f"d2a_{operation.replace('-', '_')}_result_invalid")
        record_path = absolute_normal_path(output.get("result"), "d2a_record", must_exist=True)
        result_root = pathlib.Path(config["d2a_result_root"])
        if record_path == result_root or result_root not in record_path.parents:
            fail("d2a_record_path_invalid")
        payload = private_file_bytes(record_path, "d2a_record")
        if not isinstance(output.get("evidence_sha256"), str) or not DIGEST.fullmatch(output["evidence_sha256"]):
            fail("d2a_record_invalid")
        return {
            "operation": operation,
            "path": str(record_path),
            "sha256": sha256_bytes(payload),
            "verified": False,
        }

    def verify_records(self, state_path, state):
        for record in state["records"]:
            if record["verified"]:
                continue
            payload = private_file_bytes(pathlib.Path(record["path"]), "d2a_record")
            if sha256_bytes(payload) != record["sha256"]:
                fail("d2a_record_changed")
            _label, output = self.invoke([
                self.python,
                str(self.tool_root / "d2a.py"),
                "verify",
                "--record", record["path"],
            ])
            if output != {"status": "verified", "release_eligible": False}:
                fail("offline_verify_result_invalid")
            record["verified"] = True
            self.save(state_path, state)

    def validate_teardown(self, value):
        if (
            value.get("status") not in {"torn_down", "exact_replay"}
            or value.get("phase") != "candidate_started"
            or value.get("all_resources_absent") is not True
            or type(value.get("resource_count")) is not int
            or value["resource_count"] < 0
        ):
            fail("discord_teardown_result_invalid")

    def validate_cleanup(self, value):
        required_true = {
            "database_absent",
            "postgres_process_absent",
            "launchd_jobs_absent",
            "keychain_items_absent",
            "isolated_root_absent",
            "protected_staging_unchanged",
        }
        if (
            value.get("status") not in {"cleaned", "cleaned_after_failure", "already_cleaned"}
            or value.get("phase") != "cleaned"
            or any(value.get(field) is not True for field in required_true)
        ):
            fail("cleanup_result_invalid")

    def validate_postconditions(self, value, state):
        if value != {
            "status": "observed",
            "phase": "cleaned",
            "postgres_running": False,
            "candidate_launchd_jobs_loaded": 0,
            "candidate_launchd_overrides_absent": True,
            "protected_staging_unchanged": True,
        }:
            fail("postconditions_invalid")
        runtime_root = pathlib.Path("/private/tmp") / f"starring-d2-{state['run_id']}"
        if os.path.lexists(runtime_root):
            fail("isolated_runtime_still_present")

    def teardown(self, state_path, state):
        require_revoked_session_lifecycle(state)
        self.save(state_path, state, phase="discord_teardown")
        output = self.orchestrator("teardown-discord-resources", pathlib.Path(state["manifest_path"]))
        self.validate_teardown(output)
        self.save(state_path, state, discord_teardown_complete=True)

    def cleanup(self, state_path, state):
        if state["candidate_started"] and not state["discord_teardown_complete"]:
            fail("cleanup_before_discord_teardown")
        if state["candidate_started"]:
            require_revoked_session_lifecycle(state)
        self.save(state_path, state, phase="cleanup")
        output = self.orchestrator("cleanup", pathlib.Path(state["manifest_path"]))
        self.validate_cleanup(output)
        self.save(state_path, state, cleanup_complete=True)
        self.save(state_path, state, phase="postconditions")
        status = self.orchestrator("status", pathlib.Path(state["manifest_path"]))
        self.validate_postconditions(status, state)
        self.save(state_path, state, postconditions_complete=True)

    def recover(self, state_path, state):
        manifest = pathlib.Path(state["manifest_path"])
        if manifest.exists() or manifest.is_symlink():
            ensure_resume_taint(state)
            self.save(state_path, state)
            orchestrator_state = manifest.parent / "orchestrator" / "state.json"
            if orchestrator_state.exists() or orchestrator_state.is_symlink():
                if state["candidate_started"] and not state["discord_teardown_complete"]:
                    self.teardown(state_path, state)
                elif not state["candidate_started"]:
                    self.save(state_path, state, discord_teardown_complete=True)
                if not state["cleanup_complete"] or not state["postconditions_complete"]:
                    self.cleanup(state_path, state)
            else:
                # Preserve lifecycle-tamper diagnostics ahead of the missing
                # observer-state boundary, but never convert either case into
                # cleanup or absence proof.
                require_revoked_session_lifecycle(state)
                if state["last_error"] == "manual_recovery_required":
                    fail("manual_recovery_required")
                fail("orchestrator_state_missing")
        else:
            runtime_root = pathlib.Path("/private/tmp") / f"starring-d2-{state['run_id']}"
            if os.path.lexists(runtime_root):
                fail("manifest_missing_with_runtime")
            fail("manifest_missing")

    def result(self, state_path, state, status=None, error_code=None):
        output = {
            "schema_version": SCHEMA_VERSION,
            "kind": RESULT_KIND,
            "status": status or state["status"],
            "error_code": error_code,
            "operation": state["operation"],
            "run_id": state["run_id"],
            "manifest": state["manifest_path"],
            "state": str(state_path),
            "records": [
                {key: record[key] for key in ("operation", "path", "sha256")}
                for record in state["records"]
            ],
            "onboarding_evidence": None
            if state["onboarding_evidence_path"] is None
            else {
                "path": state["onboarding_evidence_path"],
                "sha256": state["onboarding_evidence_sha256"],
            },
            "source_revision": {
                "commit_sha": state["source_commit_sha"],
                "tree_sha": state["source_tree_sha"],
            },
            "candidate_dependencies": {
                "record_sha256": state["candidate_dependency_record_sha256"],
                "tree_sha256": state["candidate_dependency_tree_sha256"],
            },
            "issuer_toolchain": {
                "cargo_sha256": state["tool_digests"]["cargo_sha256"],
                "rustc_sha256": state["tool_digests"]["rustc_sha256"],
                "rust_sysroot_sha256": state["tool_digests"]["rust_sysroot_sha256"],
                "rust_linkers_sha256": state["tool_digests"]["rust_linkers_sha256"],
                "darwin_toolchain_sha256": state["tool_digests"]["darwin_toolchain_sha256"],
                "macos_sdk_sha256": state["tool_digests"]["macos_sdk_sha256"],
                "build_environment_sha256": state["tool_digests"]["issuer_build_environment_sha256"],
                "build_environment": dict(state["issuer_build_environment"]),
            },
            "release_eligible": False,
            "persistent_sandbox_retained": True,
            "discord_teardown_complete": state["discord_teardown_complete"],
            "cleanup_complete": state["cleanup_complete"],
            "total_local_absence": state["postconditions_complete"],
            "protected_staging_unchanged": state["postconditions_complete"],
        }
        if set(output) != RESULT_FIELDS:
            fail("bootstrap_result_invalid")
        return output

    def run_locked(self, config_path, candidate_spec_path, operation):
        if operation not in D2A.ALLOWED_OPERATIONS:
            fail("operation_invalid")
        config_path, config, config_digest = read_private_json(config_path, "sandbox_config")
        validate_config(config)
        (
            candidate_spec_path,
            candidate_spec,
            spec_digest,
            provenance_path,
            provenance,
            provenance_digest,
        ) = load_candidate_publication(candidate_spec_path)
        if pathlib.Path(config["release_run_root"]) != absolute_normal_path(
            self.expected_release_root,
            "expected_release_run_root",
            must_exist=False,
        ):
            fail("release_run_root_invalid")
        # Building the credential-consuming leaf is the only pre-run mutation.
        # It happens before any bootstrap state, manifest, or service exists.
        source_revision = self.source_revision(candidate_spec)
        state_root = ensure_private_directory(
            pathlib.Path(config["bootstrap_state_root"]),
            "bootstrap_state_root",
        )
        toolchain_digests, issuer_build_environment = self.build_issuer(
            state_root, source_revision, candidate_spec
        )
        tool_digests = collect_tool_digests(self.tool_root, self.certification_root)
        tool_digests.update(toolchain_digests)
        run_id = f"d2-{datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dt%H%M%Sz')}-{secrets.token_hex(6)}"
        if not RUN_ID.fullmatch(run_id):
            fail("run_id_generation_failed")
        resource_prefix = f"starring-d2-{run_id[3:11]}-{run_id.rsplit('-', 1)[1]}"
        manifest_path = pathlib.Path(config["release_run_root"]) / run_id / "manifest.json"
        state_path = state_root / f"bootstrap-{run_id}.json"
        state = {
            "schema_version": SCHEMA_VERSION,
            "kind": STATE_KIND,
            "bootstrap_id": f"d2ab-{secrets.token_hex(16)}",
            "status": "running",
            "phase": "initialized",
            "operation": operation,
            "config_path": str(config_path),
            "config_sha256": config_digest,
            "candidate_spec_path": str(candidate_spec_path),
            "candidate_spec_sha256": spec_digest,
            "candidate_provenance_path": str(provenance_path),
            "candidate_provenance_sha256": provenance_digest,
            "candidate_dependency_record_sha256": provenance["dependencies"]["record_sha256"],
            "candidate_dependency_tree_sha256": provenance["dependencies"]["tree_sha256"],
            "source_commit_sha": source_revision["commit"],
            "source_tree_sha": source_revision["tree"],
            "run_id": run_id,
            "manifest_path": str(manifest_path),
            "manifest_sha256": None,
            "onboarding_evidence_path": None,
            "onboarding_evidence_sha256": None,
            "resource_prefix": resource_prefix,
            "tool_digests": tool_digests,
            "issuer_build_environment": issuer_build_environment,
            "records": [],
            "last_session_operation": None,
            "candidate_started": False,
            "discord_teardown_complete": False,
            "cleanup_complete": False,
            "postconditions_complete": False,
            "persistent_sandbox_retained": True,
            "release_eligible": False,
            "last_error": None,
            "updated_at": utc_now(),
        }
        write_state(state_path, state)
        try:
            self.prepare_manifest(state_path, state, config, candidate_spec)
            manifest = pathlib.Path(state["manifest_path"])
            self.save(state_path, state, phase="dry_run")
            output = self.orchestrator("dry-run", manifest)
            self.validate_stage("dry_run", output, state, config)
            self.save(state_path, state, phase="preflight")
            output = self.invoke([
                self.python,
                str(self.certification_root / "d2_preflight_evidence.py"),
                "--manifest", state["manifest_path"],
            ])[1]
            self.validate_stage("preflight", output, state, config)
            self.save(state_path, state, phase="prepare")
            output = self.orchestrator("prepare", manifest)
            self.validate_stage("prepare", output, state, config)
            self.save(state_path, state, phase="start")
            output = self.orchestrator("start", manifest)
            if output.get("phase") == "candidate_started":
                self.save(state_path, state, candidate_started=True)
            self.validate_stage("start", output, state, config)
            self.require_sealed_source_revision(candidate_spec, source_revision)
            self.save(
                state_path,
                state,
                phase="direct_onboard",
                last_session_operation="direct-onboard",
            )
            self.direct_onboard(state_path, state, config, candidate_spec)
            self.require_sealed_source_revision(candidate_spec, source_revision)
            self.save(
                state_path,
                state,
                phase="auth_smoke",
                last_session_operation="auth-smoke",
            )
            state["records"].append(self.run_d2a("auth-smoke", state, config))
            self.save(state_path, state)
            if operation == "one-shot":
                self.require_sealed_source_revision(candidate_spec, source_revision)
                self.save(
                    state_path,
                    state,
                    phase="requested_operation",
                    last_session_operation="one-shot",
                )
                state["records"].append(self.run_d2a("one-shot", state, config))
                self.save(state_path, state)
            reject_commercial_onboarding_artifacts(manifest)
            self.require_sealed_source_revision(candidate_spec, source_revision)
            self.save(state_path, state, phase="offline_verify")
            self.verify_records(state_path, state)
            self.require_sealed_source_revision(candidate_spec, source_revision)
            self.teardown(state_path, state)
            self.cleanup(state_path, state)
            self.save(state_path, state, phase="complete", status="passed", last_error=None)
            return self.result(state_path, state, status="passed")
        except BaseException as error:
            interrupted = isinstance(error, KeyboardInterrupt)
            code = "bootstrap_interrupted" if interrupted else (
                error.code if isinstance(error, BootstrapError) else "bootstrap_internal_error"
            )
            self.save(state_path, state, status="recovery_required", last_error=code)
            recovery_error = None
            if state["phase"] != "discord_teardown":
                try:
                    self.recover(state_path, state)
                except BaseException as cleanup_error:
                    recovery_error = cleanup_error
            if recovery_error is not None:
                recovery_code = recovery_error.code if isinstance(recovery_error, BootstrapError) else "recovery_failed"
                self.save(state_path, state, status="recovery_required", last_error=code)
                return self.result(state_path, state, status="failed", error_code=recovery_code)
            final_status = "failed"
            self.save(state_path, state, phase="complete" if state["postconditions_complete"] else state["phase"], status=final_status, last_error=code)
            return self.result(state_path, state, status=final_status, error_code=code)

    def run(self, config_path, candidate_spec_path, operation):
        with bootstrap_lock(self.lock_path):
            return self.run_locked(config_path, candidate_spec_path, operation)

    def resume_locked(self, state_path):
        state_path, state = load_state(state_path)
        # Keep the original terminal outcome across the durable flag clear and
        # a process stop before replay records its outcome.  A `passed` status
        # alone never skips the live checks below.
        passed_result = state["status"] == "passed"
        terminal_result = (
            state["status"] in {"passed", "failed"}
            and state["phase"] == "complete"
        )
        original = state["last_error"] or "bootstrap_recovery_requested"
        if terminal_result or state["postconditions_complete"]:
            # A persisted terminal observation is historical evidence, not a
            # live absence proof.  Clear both success flags durably so a failed
            # replay cannot return stale total_local_absence, then force the
            # orchestrator's idempotent cleanup and status observers below.
            self.save(
                state_path,
                state,
                cleanup_complete=False,
                postconditions_complete=False,
            )
        try:
            self.recover(state_path, state)
        except BaseException as error:
            code = error.code if isinstance(error, BootstrapError) else "recovery_failed"
            if passed_result:
                # The live observation failed, but that does not rewrite the
                # historical product outcome.  Preserve `passed` durably with
                # absence still unproved so a later resume can retry only the
                # cleanup/status replay.
                self.save(
                    state_path,
                    state,
                    phase="complete",
                    status="passed",
                    last_error=None,
                )
                return self.result(
                    state_path,
                    state,
                    status="failed",
                    error_code=code,
                )
            self.save(state_path, state, status="recovery_required", last_error=original)
            return self.result(state_path, state, status="failed", error_code=code)
        if passed_result:
            self.save(
                state_path,
                state,
                phase="complete",
                status="passed",
                last_error=None,
            )
            return self.result(state_path, state, status="passed")
        self.save(state_path, state, phase="complete", status="failed", last_error=original)
        return self.result(state_path, state, status="failed", error_code=original)

    def resume(self, state_path):
        with bootstrap_lock(self.lock_path):
            return self.resume_locked(state_path)

    def recover_audited_quarantined_no_issue_locked(
        self,
        state_path,
        confirmed_current_commit,
        confirmed_current_tree,
        confirmed_run_id,
        confirmed_manifest_sha256,
    ):
        state_path, state, loaded_state_sha256 = load_state_with_digest(
            state_path
        )
        original_error = state["last_error"]
        completion_flags = tuple(
            state[field]
            for field in (
                "discord_teardown_complete",
                "cleanup_complete",
                "postconditions_complete",
            )
        )
        prior_complete = (
            state["status"] == "failed"
            and state["phase"] == "complete"
            and completion_flags == (True, True, True)
        )
        replay_pending = (
            state["status"] == "failed"
            and state["phase"] == "complete"
            and completion_flags == (False, False, False)
        )
        if (
            state["run_id"] != AUDITED_QUARANTINED_INCIDENT["run_id"]
            or state["manifest_sha256"]
            != AUDITED_QUARANTINED_INCIDENT["manifest_sha256"]
            or original_error != "direct_onboard_failed"
            or state["candidate_started"] is not True
            or state["last_session_operation"] != "direct-onboard"
            or state["onboarding_evidence_path"] is not None
            or state["onboarding_evidence_sha256"] is not None
            or state["records"]
            or (state["status"], state["phase"])
            not in {
                ("recovery_required", "direct_onboard"),
                ("failed", "complete"),
            }
            or not (
                completion_flags == (False, False, False)
                or prior_complete
            )
            or audited_recovery_bootstrap_semantic_sha256(state)
            != AUDITED_QUARANTINED_INCIDENT[
                "bootstrap_state_semantic_sha256"
            ]
        ):
            fail("audited_recovery_bootstrap_state_invalid")
        if (
            not prior_complete
            and not replay_pending
            and loaded_state_sha256
            != AUDITED_QUARANTINED_INCIDENT["bootstrap_state_sha256"]
        ):
            fail("audited_recovery_bootstrap_state_invalid")
        if (
            not isinstance(confirmed_current_commit, str)
            or COMMIT.fullmatch(confirmed_current_commit) is None
            or not isinstance(confirmed_current_tree, str)
            or COMMIT.fullmatch(confirmed_current_tree) is None
            or confirmed_run_id != state["run_id"]
            or confirmed_manifest_sha256 != state["manifest_sha256"]
        ):
            fail("audited_recovery_confirmation_mismatch")
        try:
            current_source = self.source_revision(
                {
                    "commit_sha": confirmed_current_commit,
                    "source_tree_sha": confirmed_current_tree,
                }
            )
        except BootstrapError:
            fail("audited_recovery_confirmation_mismatch")
        if current_source != {
            "commit": confirmed_current_commit,
            "tree": confirmed_current_tree,
            "clean": True,
        }:
            fail("audited_recovery_confirmation_mismatch")

        # The first invocation must present the byte-exact incident state to
        # the audited orchestrator.  A replay first clears historical success
        # flags so a failed live revalidation can never report stale absence.
        if prior_complete:
            self.save(
                state_path,
                state,
                discord_teardown_complete=False,
                cleanup_complete=False,
                postconditions_complete=False,
            )
        try:
            ensure_resume_taint(state)
            manifest = pathlib.Path(state["manifest_path"])
            output = self.orchestrator(
                AUDITED_QUARANTINED_RECOVERY_COMMAND,
                manifest,
                "--bootstrap-state",
                str(state_path),
                "--confirm-current-commit",
                confirmed_current_commit,
                "--confirm-current-tree",
                confirmed_current_tree,
                "--confirm-run-id",
                confirmed_run_id,
                "--confirm-manifest-sha256",
                confirmed_manifest_sha256,
            )
            validate_audited_quarantined_recovery_output(
                output,
                state,
                state_path,
                confirmed_current_commit,
                confirmed_current_tree,
                self.source_root,
            )
            self.validate_postconditions(output["postconditions"], state)
        except BaseException as error:
            code = (
                error.code
                if isinstance(error, BootstrapError)
                else "audited_recovery_failed"
            )
            return self.result(
                state_path,
                state,
                status="failed",
                error_code=code,
            )
        self.save(
            state_path,
            state,
            phase="complete",
            status="failed",
            last_error=original_error,
            discord_teardown_complete=True,
            cleanup_complete=True,
            postconditions_complete=True,
        )
        return self.result(
            state_path,
            state,
            status="failed",
            error_code=original_error,
        )

    def recover_audited_quarantined_no_issue(
        self,
        state_path,
        confirmed_current_commit,
        confirmed_current_tree,
        confirmed_run_id,
        confirmed_manifest_sha256,
    ):
        with bootstrap_lock(self.lock_path):
            return self.recover_audited_quarantined_no_issue_locked(
                state_path,
                confirmed_current_commit,
                confirmed_current_tree,
                confirmed_run_id,
                confirmed_manifest_sha256,
            )


class SafeArgumentParser(argparse.ArgumentParser):
    def error(self, _message):
        fail("cli_invalid")


def parser():
    root = SafeArgumentParser(prog="d2a-bootstrap")
    commands = root.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run")
    run.add_argument("--sandbox-config", required=True)
    run.add_argument("--candidate-spec", required=True)
    run.add_argument("--operation", required=True, choices=sorted(D2A.ALLOWED_OPERATIONS))
    resume = commands.add_parser("resume")
    resume.add_argument("--state", required=True)
    audited_recovery = commands.add_parser(
        AUDITED_QUARANTINED_RECOVERY_COMMAND
    )
    audited_recovery.add_argument("--state", required=True)
    audited_recovery.add_argument("--confirm-current-commit", required=True)
    audited_recovery.add_argument("--confirm-current-tree", required=True)
    audited_recovery.add_argument("--confirm-run-id", required=True)
    audited_recovery.add_argument("--confirm-manifest-sha256", required=True)
    return root


def error_result(code):
    return {
        "schema_version": SCHEMA_VERSION,
        "kind": RESULT_KIND,
        "status": "failed",
        "error_code": code,
        "operation": None,
        "run_id": None,
        "manifest": None,
        "state": None,
        "records": [],
        "onboarding_evidence": None,
        "source_revision": None,
        "candidate_dependencies": None,
        "issuer_toolchain": None,
        "release_eligible": False,
        "persistent_sandbox_retained": True,
        "discord_teardown_complete": False,
        "cleanup_complete": False,
        "total_local_absence": False,
        "protected_staging_unchanged": False,
    }


def main(argv=None):
    try:
        arguments = parser().parse_args(argv)
        controller = BootstrapController()
        if arguments.command == "run":
            result = controller.run(arguments.sandbox_config, arguments.candidate_spec, arguments.operation)
        elif arguments.command == "resume":
            result = controller.resume(arguments.state)
        else:
            result = controller.recover_audited_quarantined_no_issue(
                arguments.state,
                arguments.confirm_current_commit,
                arguments.confirm_current_tree,
                arguments.confirm_run_id,
                arguments.confirm_manifest_sha256,
            )
    except BootstrapError as error:
        result = error_result(error.code)
    except KeyboardInterrupt:
        result = error_result("bootstrap_interrupted")
    except SystemExit:
        raise
    except BaseException:
        result = error_result("bootstrap_internal_error")
    print(canonical_json(result))
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    signal.signal(
        signal.SIGTERM,
        lambda _signal, _frame: (_ for _ in ()).throw(KeyboardInterrupt()),
    )
    raise SystemExit(main())
