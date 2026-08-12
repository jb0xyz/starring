# D2 headless session issuer

This is an operator-only CLI for running a Node product test against an active,
isolated D2 candidate without completing Discord OAuth in a browser. The issuer
does not add an API route, feature flag, or alternate production authentication
path.

The CLI fails closed unless the input is the canonical mode-`0600` manifest for
an active `commercial_human_v1` D2 run. It verifies the manifest digest, exact
top-level and typed nested inventory, run-owned paths and Keychain service,
ordered human boundaries, manifest-selected isolated ports and PostgreSQL
socket, candidate launchd jobs, candidate Node digest, actor, and disposable
Discord identity. Ambient `PG*` and `DATABASE_URL` variables are rejected.
The ordered boundaries are exactly `create_disposable_discord_guild`,
`complete_discord_oauth`, `confirm_product_preview`,
`execute_real_discord_interactions`, `confirm_replacement_preview`, and
`delete_disposable_discord_guild`.
All nine candidate files are re-opened without symlink following and checked
for their manifest digest, owner, link count, and sealed mode. The three fixed
source-tree inventories are rehashed from their current bytes. Candidate-start
evidence must bind those exact digests and the manifest tunnel identity.

For API, runtime, worker, transport, and tunnel, the issuer independently
reconstructs the complete launchd plist dictionary from the manifest and
compares it to the current plist. It also binds the live launchd job's complete
configured environment, working directory, log paths, umask, timeouts, file
limits, program, arguments, state, and PID. Current executable paths, file
digests, and kernel-reported process arguments are checked twice; the tunnel
must have exec'd the sealed `cloudflared`, and the worker must be the sealed
Node plus sealed worker entrypoint. External Keychain identities may not
collide with protected staging/production services, standing Discord
identities, or the run-owned Keychain namespace.
The global Discord ownership registry is read under the global lock with
owner/mode/link/symlink and stable-byte checks. It must contain the exact
run/manifest/guild/application/bot claim, and every extant D2 runtime root must
have a registered owner; an absent or mismatched claim fails closed.

Immediately after argument parsing it takes the nonblocking exclusive global
lock `/private/tmp/starring-d2-certification.lock`. After binding the manifest,
it also takes the run's exclusive `coordinator/coordinator.lock`, always in that
order. Both locks remain held through final session revocation. Contention fails
with `d2_operation_busy` before lifecycle takeover or credential access. Before
reading any credential, the issuer sets and verifies both core-dump limits at
zero; the Node child inherits that limit.

Before invoking the issuer, the bootstrap briefly holds that same global D2
lock and durably creates the canonical mode-`0600` run-local `d2a-taint.json`
alongside `d2a-session-lifecycle.json`. The issuer requires the taint for
`direct-onboard`; later operations accept only its byte-exact replay. The taint
binds the run and manifest to the issuer, runner, product driver, checked-in
scenario, and domain-separated issuer-source digests and permanently records
this lane as `automated_maintenance_v1`, direct-auth, and not release-eligible.

The lifecycle JSON has exactly these ordered fields: `schema_version`, `kind`,
`run_id`, `manifest_sha256`, `operation`, `origin`, `issuer_sha256`,
`issuer_source_sha256`, `uid`, `boot_identity`, `process_group_id`,
`started_at`, `status`, `session_revoked`, `revoked_at`, and `quarantined_at`.
Its `origin` is exactly `bootstrap` or `issuer`. The only valid bootstrap form
is the pre-issuer sentinel: `operation:direct-onboard`, `origin:bootstrap`,
`status:not_issued`, a null process-group ID, false `session_revoked`, and null
terminal timestamps, with every identity and digest bound to the taint and
manifest. The bootstrap creates it with `O_EXCL` and never overwrites any
existing lifecycle path.

While still holding the global D2 lock, the issuer alone may compare the exact
bootstrap sentinel and atomically replace it with `origin:issuer`,
`status:active`, and the positive ID of its dedicated process group. Every
issuer-origin state keeps that positive process-group ID. A missing, malformed,
non-canonical, or merely similar marker fails closed; it is never repaired or
downgraded by overwriting it. The exact bootstrap sentinel authorizes direct
pre-candidate local cleanup only after the orchestrator independently proves
that the run is prepared, no candidate-start or mutation artifact exists, all
run-owned launchd services are absent, and PostgreSQL is not running. If the
candidate was already started before issuer handoff failed, the same sentinel
instead requires the normal full run-owned Discord teardown and closed fence.

It reads only the run-owned API Keychain lifecycle marker and the OAuth writer,
session issuer, and security revoker database URLs. The URLs must have the
provisioned loopback shape, but all connections are rebuilt to use the
manifest-bound Unix socket. Session creation uses the existing
`starring_product_oauth_flow_create_v1`,
`starring_product_oauth_flow_consume_v1`, and
`starring_product_session_issue_v1` security-definer functions. Direct table
writes are not used.

Build and invoke it with absolute paths:

The first credential-consuming operation is the automated, non-release
onboarding step. It accepts no child command:

```text
starring-d2-session-issuer \
  --manifest /absolute/path/to/manifest.json \
  --operation direct-onboard \
  --display-name '<manifest actor display name>'
```

This operation first requires the exact taint and absence of both commercial
onboarding artifacts. It verifies the manifest-bound Discord hub with a bounded
Bot API read, issues a normal product session with a 120-second absolute bound,
and runs only the manifest-sealed provisioner `onboard` command with a 90-second
timeout. The provisioner binary is rehashed before and after execution. The
session is revoked before canonical mode-`0600`
`d2a-onboarding-evidence.json` is written; `auth-smoke` and `one-shot` refuse to
run without that exact evidence.

Then run the product operation:

```text
cargo build --locked --release
starring-d2-session-issuer \
  --manifest /absolute/path/to/manifest.json \
  --operation auth-smoke \
  -- \
  /absolute/path/to/the/manifest-candidate/node \
  /absolute/repository/tools/d2-maintenance/headless_product_runner.mjs
```

`one-shot` requires an absolute, owner-controlled JSON scenario:

```text
starring-d2-session-issuer \
  --manifest /absolute/path/to/manifest.json \
  --operation one-shot \
  --scenario /absolute/repository/tools/d2-maintenance/scenarios/study-room.v1.json \
  -- \
  /absolute/path/to/the/manifest-candidate/node \
  /absolute/repository/tools/d2-maintenance/headless_product_runner.mjs
```

The child command is exactly the manifest-bound Node executable plus the
checked-in runner. The issuer embeds and verifies the exact runner bytes and the
runner's sibling-relative `tools/d2-certification/product_driver.js`. For v1,
`study-room.v1.json` is the sole embedded filename-and-byte allowlist; adding a
scenario requires a reviewed issuer source change.

The Node child receives exactly one JSON object through anonymous stdin. It
contains `schema_version`, `session`, `csrf`, `public_origin`, `principal_id`,
`guild_id`, `installation_id`, `run_id`, `manifest_sha256`, `operation`, and an
optional `scenario`. A one-shot input also contains `scenario_sha256`, computed
from the scenario's exact raw bytes, and an issuer-generated
`authoring_session_id`. That ID is `${session_id_prefix}-${suffix}`, where the
suffix is 16 lowercase hexadecimal characters bound to the fresh session
digest; the full ID is at most 128 characters. Credentials are never placed in
arguments, environment variables, files, or issuer diagnostics.

The child must emit one JSON object as public evidence. Stdout and stderr are
captured and drained; stdout is capped at 1 MiB, known credentials and sensitive
fields are redacted, and stderr is never relayed. Only validated, redacted JSON
is written to issuer stdout.

The issuer monitors SIGINT and SIGTERM across issuance, child execution, and
normal cleanup. After every issued-session disposition (success, failure,
timeout, signal, or ambiguous commit acknowledgement), it invokes
`starring_product_session_security_revoke_v1` through the run-owned security
revoker. `revoked`, `exact_replay`, and `already_revoked` (normal child logout)
are terminal success outcomes. Revocation is retried with the same digest;
failure to confirm a terminal outcome fails the command and suppresses
evidence. The session also has a ten-minute idle and absolute limit as the final
SIGKILL or host-crash bound.
