# D2A automated maintenance harness

## Purpose

D2A is the unattended integration lane for routine backend development. It
starts from an already isolated D2 candidate and exercises the real product
HTTP, session, CSRF, installation-authority, authoring, promotion, approval,
apply, and deployment contracts without asking an operator to complete Discord
OAuth in a browser.

D2A is not a release certificate. The commercial D2 lane continues to prove
the six ordered human boundaries: `create_disposable_discord_guild`,
`complete_discord_oauth`, `confirm_product_preview`,
`execute_real_discord_interactions`, `confirm_replacement_preview`, and
`delete_disposable_discord_guild`. Every D2A result is permanently marked:

```json
{
  "certification_class": "automated_maintenance_v1",
  "direct_auth_used": true,
  "release_eligible": false
}
```

The D3 binder accepts only `commercial_human_v1` D2 manifests with the exact
human-boundary declaration. It must never accept a D2A final record.

The current D2A lane can reuse an already running isolated D2 candidate, but it
may not consume or coexist with commercial onboarding evidence and may not
leave that run release-eligible. Before invoking the issuer, the bootstrap
holds the global D2 lock and creates the durable run-root `d2a-taint.json`
together with an exclusive pre-issuer lifecycle sentinel. The taint permanently
blocks commercial coordinator progress, verification, and D3 binding. A later
release certificate therefore starts from a fresh D2 run; cleanup of the
tainted isolated run remains allowed only through the lifecycle and teardown
fences below.

## Security boundary

There is no test route, feature switch, stable bearer token, or alternate
readiness path in `starring-api`.

The session issuer is a separate, non-listening, unpublished tool. It exposes
no TCP or Unix control endpoint; its only outbound network use is the bounded,
manifest-bound Discord hub preflight and the product HTTP work performed by its
fixed child. It operates only when all of the following identities agree:

- canonical D2 manifest and digest;
- run ID and `/private/tmp/starring-d2-<run-id>` root;
- run-owned Unix PostgreSQL socket, database name, and port;
- run-owned Keychain namespace;
- manifest actor, guild, application, resource prefix, and installation;
- active candidate process and the run-root D2A direct-onboarding evidence.

The two commercial artifacts
`orchestrator/onboarding-evidence.json` and
`orchestrator/coordinator-sources/step-04-onboarding.json` are forbidden. The
issuer instead persists `d2a-onboarding-evidence.json` after its short direct
onboarding session has been revoked. That canonical owner-held `0600`,
single-link file has the exact 21-field
`starring.d2a.direct-onboarding-evidence.v1` schema. It binds the automated
class, direct-onboard operation, run and manifest, actor/guild/application/hub,
community-hub installation, sealed provisioner, issuer binary and issuer source
digests, successful hub preflight, revoked session, direct-auth use, and
`release_eligible:false`.

It rejects TCP database URLs and ambient `PG*` configuration. It creates and
consumes a normal OAuth-flow record and calls the existing session-issue
database capability with independently generated secrets. It does not write
identity tables directly.

The raw session and CSRF values exist only in the issuer process and an
anonymous stdin pipe to the Node child. They are never placed in arguments,
environment variables, files, logs, evidence, or standard output. The Node
child uses the same `product_driver.js` request and response validators as the
commercial lane, adds the real cookie/Origin/CSRF boundary, then logs out and
requires `/v1/me` to return 401 before it can report success.

The secret-free session lifecycle marker is canonical ordered JSON with exactly
`schema_version`, `kind`, `run_id`, `manifest_sha256`, `operation`, `origin`,
`issuer_sha256`, `issuer_source_sha256`, `uid`, `boot_identity`,
`process_group_id`, `started_at`, `status`, `session_revoked`, `revoked_at`, and
`quarantined_at`, in that order. `origin` is exactly `bootstrap` or `issuer`.
Under the global D2 lock and alongside the taint, the bootstrap creates with
`O_EXCL` the sole valid bootstrap form: `operation:direct-onboard`,
`origin:bootstrap`, `status:not_issued`, null `process_group_id`, false
`session_revoked`, and null terminal timestamps, with all identities and
digests bound to the manifest and taint. Only an absent lifecycle path may be
created; an existing path is never overwritten.

The issuer is a dedicated process-group/session leader and is the only
component allowed, while holding the same global D2 lock, to compare that exact
bootstrap sentinel and atomically replace it with an `origin:issuer`, `active`
marker carrying its positive dedicated process-group ID. Normal pre-issuance
errors end as issuer-origin `not_issued`; confirmed mandatory revocation ends
as `revoked` (including a failed product operation); and unconfirmed revocation
ends as `quarantined`. An unhandleable process death leaves `active`. Every
issuer-origin state retains the positive process-group ID. No marker stores a
raw credential, session digest, fingerprint, or revocation handle. Automated
teardown is permitted only for issuer-origin `not_issued` and `revoked` after
process-group absence is proved; `active`, `quarantined`, malformed,
non-canonical, or incoherent state is a permanent `manual_recovery_required`
boundary for that run.

The bootstrap and candidate compiler supervisors use unique targets, dedicated
process groups, simultaneous capped stdout/stderr streaming, monotonic
deadlines, and bounded TERM/KILL/reap. Durable state records the process group
before accepting build completion. Unknown group absence blocks reuse and
cleanup. Both build paths reject repository, ancestor, and Cargo-home config;
use an isolated offline Cargo home; and bind the recursive Rust sysroot, linker
shims, resolved Xcode clang/ld/ar/ranlib/otool, OS build, and root-owned SDK
tree before and after compilation. SDK symlinks must resolve to existing paths
inside the bound SDK. Mach-O linkage is validated in the private target before
the issuer is atomically published. Clean source commit/tree checks bracket
that final replacement.

Run-owned Discord teardown is itself fenced under the global D2 lock. The
durable fence transitions from open to `closing`, and only a successful exact
teardown transitions it to `closed`. Local cleanup requires coherent taint and
terminal lifecycle markers plus the closed fence. A missing marker, teardown
failure, or attempted marker deletion therefore cannot bypass the boundary.
The sole pre-candidate exception is the exact bootstrap sentinel: the
orchestrator may close the fence without Discord teardown only after proving
the run remains in `prepared`, no candidate-start commitment or mutation,
transport, or teardown artifact exists, every run-owned launchd service is
absent, and PostgreSQL is not running. When candidate start was already
committed before issuer handoff failed, the sentinel instead requires the
normal full run-owned Discord teardown and closed fence. Any other
bootstrap-origin or malformed marker fails closed.

## Commands

The controller consumes a running isolated D2 manifest only after direct
onboarding has produced the run-root D2A evidence:

```sh
python3 tools/d2-maintenance/d2a.py run \
  --manifest /absolute/path/to/manifest.json \
  --operation auth-smoke
```

A product scenario uses a checked-in confirmation policy:

```sh
python3 tools/d2-maintenance/d2a.py run \
  --manifest /absolute/path/to/manifest.json \
  --operation one-shot \
  --scenario /absolute/path/to/tools/d2-maintenance/scenarios/study-room.v1.json
```

The result is written outside the D2 run directory, below
`~/Library/Application Support/Starring/d2a-runs/`. The controller copies the
exact direct-onboarding evidence into every result and binds its raw SHA-256 in
`final.json` together with the manifest, provisioner, issuer/source, Discord
IDs, and installation. Offline verification repeats the exact schema,
cross-binding, mode/link, digest, and non-release checks:

```sh
python3 tools/d2-maintenance/d2a.py verify \
  --record /absolute/path/to/d2a-result/final.json
```

## Scenario confirmation

Automation does not mean blind approval. A one-shot scenario contains one
exact, complete preview-summary policy. The runner compares all six summary
fields to the real approval preview before returning true to the product
driver's confirmation callback. Unknown, missing, or mismatched fields refuse
approval and still trigger session revocation. Content-digest policies are not
part of schema v1; adding one requires a versioned scenario and evidence
contract rather than an optional field.

## Coverage roadmap

The first lane removes the OAuth/browser dependency for product HTTP flows.
Real user-created Discord interactions cannot be generated through the bot API,
and user-token automation is prohibited. Fully unattended interaction testing
therefore belongs in a second isolated mode:

1. add `loopback_simulation_v1` only to the certification transport;
2. emulate the Discord Gateway handshake and signed interaction callback;
3. model effect HTTP responses and resource inventory entirely in the run root;
4. preserve the production runtime and API binaries unchanged;
5. keep all simulation evidence in D2A kinds with `release_eligible:false`.

Commercial D2 remains the final proof for real Discord behavior even after the
simulation lane is complete.
