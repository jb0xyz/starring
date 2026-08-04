import argparse
import hashlib
import json
import os
import pathlib
import re
import sys

from d2_certification import (
    CertificationError,
    canonical_json,
    load_json_file,
    require_absolute_path,
    require_owned_mode,
)
from d2_orchestrator_contract import (
    OrchestratorError,
    external_keychain_inventory,
    global_operation_lock,
    keychain_inventory,
    load_context,
    utc_now,
    write_atomic,
)
from d2_orchestrator_platform import Platform
from isolated_orchestrator import command_dry_run


KIND = "starring.d2.preflight-absence-evidence.v1"
PROCESS_LINE_PATTERN = re.compile(r"^\s*([1-9][0-9]*)\s+([0-9]+)\s+(.+?)\s*$")
DIGEST_PATTERN = re.compile(r"^[0-9a-f]{64}$")


def process_rows(platform):
    result = platform.run(
        ["/bin/ps", "-A", "-ww", "-o", "pid=", "-o", "ppid=", "-o", "command="],
        timeout=10,
    )
    if result.returncode != 0 or len(result.stdout) > 2 * 1024 * 1024:
        raise OrchestratorError("preflight_process_scan_failed")
    try:
        output = result.stdout.decode("utf-8")
    except UnicodeDecodeError as error:
        raise OrchestratorError("preflight_process_scan_invalid") from error
    rows = []
    for line in output.splitlines():
        match = PROCESS_LINE_PATTERN.fullmatch(line)
        if match is None:
            if line.strip():
                raise OrchestratorError("preflight_process_scan_invalid")
            continue
        rows.append((int(match.group(1)), int(match.group(2)), match.group(3)))
    if len({pid for pid, _ppid, _command in rows}) != len(rows):
        raise OrchestratorError("preflight_process_scan_invalid")
    return rows


def ancestor_process_ids(rows, pid=None):
    parents = {process_id: parent_id for process_id, parent_id, _command in rows}
    current = os.getpid() if pid is None else pid
    ancestors = set()
    while current > 0 and current not in ancestors:
        ancestors.add(current)
        current = parents.get(current, 0)
    return ancestors


def smoke_process_count(context, rows):
    manifest = context.manifest
    markers = {
        manifest["run_id"],
        manifest["discord"]["resource_prefix"],
        str(context.root),
        *(service["label"] for service in manifest["services"].values()),
        *manifest["keychain_services"].values(),
    }
    ancestors = ancestor_process_ids(rows)
    return sum(
        1
        for pid, _ppid, command in rows
        if pid not in ancestors and any(marker in command for marker in markers)
    )


def owner_count(context, platform):
    loaded = sum(
        1
        for service in context.manifest["services"].values()
        if platform.launchd_loaded(service["label"])
    )
    keychain = sum(
        1
        for service, account in keychain_inventory(context)
        if platform.keychain_present(service, account)
    )
    root = int(context.root.exists())
    ports = sum(
        1
        for service in context.manifest["services"].values()
        for name in ("port", "gateway_port", "http_port")
        if name in service and not platform.port_available(service[name])
    )
    postgres = int(
        not platform.port_available(context.manifest["database"]["port"])
    )
    return loaded + keychain + root + ports + postgres


def standing_snapshot_sha256(snapshot):
    return hashlib.sha256(canonical_json(snapshot).encode("utf-8")).hexdigest()


def validate_preflight_evidence(context, value):
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
        not isinstance(value, dict)
        or set(value) != fields
        or type(value["schema_version"]) is not int
        or value["schema_version"] != 1
        or value["kind"] != KIND
        or value["manifest_sha256"] != context.digest
        or type(value["prior_runtime_owner_count"]) is not int
        or value["prior_runtime_owner_count"] != 0
        or type(value["prior_smoke_process_count"]) is not int
        or value["prior_smoke_process_count"] != 0
        or type(value["external_credential_count"]) is not int
        or value["external_credential_count"]
        != len(external_keychain_inventory(context))
        or not isinstance(value["standing_snapshot_sha256"], str)
        or not DIGEST_PATTERN.fullmatch(value["standing_snapshot_sha256"])
        or not isinstance(value["observed_at"], str)
        or not value["observed_at"].endswith("Z")
    ):
        raise OrchestratorError("preflight_evidence_invalid")
    return value


def preflight_evidence_path(context):
    return context.artifact_directory / "preflight-absence-evidence.json"


def load_private_evidence(path):
    try:
        require_owned_mode(path, 0o600, "preflight_evidence")
        return load_json_file(path, "preflight_evidence")
    except CertificationError as error:
        raise OrchestratorError(str(error)) from error


def command_preflight_evidence(context, platform):
    before = command_dry_run(context, platform)
    rows = process_rows(platform)
    evidence = {
        "schema_version": 1,
        "kind": KIND,
        "observed_at": utc_now(),
        "manifest_sha256": context.digest,
        "prior_runtime_owner_count": owner_count(context, platform),
        "prior_smoke_process_count": smoke_process_count(context, rows),
        "standing_snapshot_sha256": standing_snapshot_sha256(
            before["standing_snapshot"]
        ),
        "external_credential_count": len(external_keychain_inventory(context)),
    }
    validate_preflight_evidence(context, evidence)
    after = command_dry_run(context, platform)
    if before != after:
        raise OrchestratorError("preflight_observation_drift")
    path = preflight_evidence_path(context)
    if path.exists():
        recorded = load_private_evidence(path)
        validate_preflight_evidence(context, recorded)
        if {
            key: value for key, value in recorded.items() if key != "observed_at"
        } != {key: value for key, value in evidence.items() if key != "observed_at"}:
            raise OrchestratorError("preflight_evidence_replay_drift")
        status = "exact_replay"
    else:
        write_atomic(path, canonical_json(evidence) + "\n")
        recorded = load_private_evidence(path)
        validate_preflight_evidence(context, recorded)
        status = "recorded"
    return {
        "status": status,
        "manifest_sha256": context.digest,
        "evidence": str(path),
    }


def parser():
    root = argparse.ArgumentParser(prog="d2-preflight-evidence")
    root.add_argument("--manifest", required=True)
    return root


def main(argv=None):
    try:
        arguments = parser().parse_args(argv)
        context = load_context(require_absolute_path(arguments.manifest, "manifest"))
        with global_operation_lock():
            result = command_preflight_evidence(context, Platform())
        print(json.dumps(result, sort_keys=True, separators=(",", ":")))
        return 0
    except (OrchestratorError, ValueError) as error:
        print(str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
