import argparse
import json
import sys

from d2_certification import (
    CertificationError,
    STEP_SPECS,
    ZERO_DIGEST,
    load_receipts_from_handle,
    load_verified_manifest,
    open_locked_receipts,
    require_owned_mode,
)


SCHEMA_VERSION = 1
HUMAN_BOUNDARIES = {
    4: "complete_discord_oauth",
    7: "confirm_product_preview",
    9: "execute_real_discord_interactions",
    14: "confirm_replacement_preview",
    17: "delete_disposable_discord_guild",
}


def canonical_json(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def next_certification_action(manifest_path):
    verified_path, manifest, digest = load_verified_manifest(manifest_path)
    receipts_path = verified_path.with_name("receipts.jsonl")
    require_owned_mode(receipts_path, 0o600, "receipts")
    with open_locked_receipts(receipts_path, False) as handle:
        receipts = load_receipts_from_handle(handle, manifest, digest)
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
    if completed_steps == len(STEP_SPECS):
        return {
            **base,
            "status": "complete",
            "steps": len(STEP_SPECS),
        }
    step = completed_steps + 1
    specification = STEP_SPECS[step]
    if step in HUMAN_BOUNDARIES:
        return {
            **base,
            "status": "awaiting_human_boundary",
            "step": step,
            "code": specification.code,
            "boundary": HUMAN_BOUNDARIES[step],
            "required_evidence_fields": list(specification.required),
        }
    return {
        **base,
        "status": "next_step",
        "step": step,
        "code": specification.code,
        "required_evidence_fields": list(specification.required),
    }


def parser():
    root = argparse.ArgumentParser(prog="d2-run")
    commands = root.add_subparsers(dest="command", required=True)
    status = commands.add_parser("status")
    status.add_argument("--manifest", required=True)
    return root


def main(argv=None):
    try:
        arguments = parser().parse_args(argv)
        if arguments.command != "status":
            raise CertificationError("coordinator_command_invalid")
        print(canonical_json(next_certification_action(arguments.manifest)))
        return 0
    except CertificationError as error:
        print(str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
