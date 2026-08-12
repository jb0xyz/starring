# D2A automated maintenance lane

D2A is the headless, non-release lane for repeated backend maintenance checks. It reuses an active, isolated D2 candidate but does not replace the commercial D2 browser, Discord, or human-approval proof. Every D2A evidence object therefore has `certification_class: "automated_maintenance_v1"`, `direct_auth_used: true`, and `release_eligible: false`.

Before direct authentication, the issuer permanently marks the reused run with
`d2a-taint.json`. Commercial D2 progress and D3 binding reject that marker, so
the same database or Discord run can never be reused later as release evidence.
The issuer also holds both the global D2 operation lock and the run's coordinator
lock through final session revocation, preventing overlap with another D2 or
D2A mutation. The controller accepts only the repository's fixed issuer,
runner, product driver, and checked-in scenario, verifies their identities
before and after execution, and copies the exact taint marker into the offline
result. It also rejects both commercial onboarding artifacts
(`orchestrator/onboarding-evidence.json` and
`orchestrator/coordinator-sources/step-04-onboarding.json`). D2A authentication
requires only the run-root `d2a-onboarding-evidence.json` created by the direct
onboarding lifecycle.

The direct onboarding evidence is canonical JSON in an owner-held, single-link
`0600` regular file with exactly 21 fields:

```json
{
  "schema_version": 1,
  "kind": "starring.d2a.direct-onboarding-evidence.v1",
  "certification_class": "automated_maintenance_v1",
  "operation": "direct-onboard",
  "observed_at": "<UTC timestamp>",
  "run_id": "<manifest run ID>",
  "manifest_sha256": "<manifest SHA-256>",
  "principal_id": "discord:<actor snowflake>",
  "guild_id": "<sandbox guild snowflake>",
  "discord_application_id": "<manifest application snowflake>",
  "hub_channel_id": "<sandbox hub-channel snowflake>",
  "binding_key": "community_hub",
  "installation_id": "installation:<manifest resource prefix>",
  "outcome": "fresh",
  "provisioner_sha256": "<manifest sealed-provisioner SHA-256>",
  "issuer_sha256": "<session issuer SHA-256>",
  "issuer_source_sha256": "<session issuer source SHA-256>",
  "discord_hub_preflight": true,
  "direct_auth_used": true,
  "session_revoked": true,
  "release_eligible": false
}
```

`outcome` may instead be `exact_replay`; no other field is optional. The
controller validates the file before and after issuer execution, copies its
exact bytes into every result, and binds the raw copied-file SHA-256 plus the
manifest, provisioner, issuer/source, Discord IDs, and installation into
`final.json`. Offline verification repeats the full schema and cross-binding
checks and rejects a missing or modified copy. These records remain automated,
direct-auth, and non-release evidence only.

## Headless product runner

`headless_product_runner.mjs` is a secret-consuming leaf process. It must be launched by the isolated session issuer; session and CSRF values are sent in one JSON object over stdin and must never be placed in arguments, environment variables, files, logs, or evidence.

The runner accepts at most 64 KiB and rejects empty input, concatenated JSON values, arrays, unknown fields, and malformed identities. The stdin object is:

```json
{
  "schema_version": 1,
  "session": "<43-character base64url credential>",
  "csrf": "<distinct 43-character base64url credential>",
  "public_origin": "https://d2-api.starring.co.kr",
  "principal_id": "discord:<actor snowflake>",
  "guild_id": "<disposable guild snowflake>",
  "installation_id": "<manifest-bound installation id>",
  "run_id": "<manifest-bound D2 run id>",
  "manifest_sha256": "<lowercase SHA-256>",
  "operation": "auth-smoke"
}
```

`one-shot` additionally requires `scenario`, `scenario_sha256`, and an issuer-generated `authoring_session_id`. The digest is computed by the issuer from the exact checked-in scenario bytes. A checked-in scenario uses this contract:

```json
{
  "schema_version": 1,
  "kind": "starring.d2a.product-scenario.v1",
  "session_id_prefix": "d2a-study-room-v1",
  "message": "<bounded authoring request>",
  "expected_generation": 0,
  "expected_summary": {
    "panels": 1,
    "modals": 1,
    "rules": 4,
    "actions": 15,
    "target_version": 1,
    "required_approvals": 1
  }
}
```

The issuer resolves a new authoring session for every invocation as `<session_id_prefix>-<16 lowercase hex characters>`. Prefixes are at most 111 characters so the resolved product resource ID never exceeds 128 characters. Generation is fixed at zero for the new session. This prevents a second maintenance run against the same candidate from colliding with a prior session while preserving a checked-in, reviewable prefix. The six summary fields are matched exactly before approval. Unknown or missing scenario fields are rejected, so the checked-in summary is always the complete approval policy and cannot degrade into an empty or partial match.

The scenario is compile-time bound into the issuer. Supplying a different path
or changing its bytes without rebuilding the issuer fails before a session is
issued. Schema v1 intentionally has no optional approval fields: a new semantic
or digest policy requires a reviewed schema and evidence update.

The runner evaluates the repository's real `../d2-certification/product_driver.js` in an isolated VM context. Its fetch boundary overwrites every request with exactly the issued session and CSRF cookies, adds the exact public `Origin` and CSRF header to every non-GET request, restricts requests to the D2 origin and `/v1/` or `/v2/`, and rejects redirects.

Both operations first require `/v1/me` to match the manifest-bound principal and require the installation authority check to return 204. `auth-smoke` stops there. `one-shot` runs the existing one-shot authoring, preview, approval, and apply flow, but its approval callback is bound to the checked-in scenario policy. An accepted apply response is only intermediate: the runner then uses the existing `waitForLive()` product driver path and does not pass until both deployment views are `live`, the runtime phase is `live`, and the serving lease is `fresh`. The D2A evidence preserves the redacted deployment and operational HTTP statuses, polling attempt, promotion identity, attestation identity and revisions, heartbeat/lease timestamps, and terminal states. A perpetual `runtime_pending` result fails with `deployment_live_timeout`.

Whether the requested operation succeeds or fails, the runner attempts `POST /v1/logout` with the same credentials and then requires a subsequent `GET /v1/me` to return 401. Missing logout or a still-active session fails the run. A successful stdout value is one bounded canonical JSON D2A evidence object; it contains only identities, statuses, hashes, summary/apply results, and the explicit release-coverage gaps. It never contains credentials or existing `starring.d2.browser-*` / `starring.d2.chrome-*` evidence. Failures produce only a stable `starring.d2a.runner-error.v1` JSON object and a nonzero exit status; stderr stays empty.

The private, non-secret `d2a-session-lifecycle.json` has exactly these ordered
fields: `schema_version`, `kind`, `run_id`, `manifest_sha256`, `operation`,
`origin`, `issuer_sha256`, `issuer_source_sha256`, `uid`, `boot_identity`,
`process_group_id`, `started_at`, `status`, `session_revoked`, `revoked_at`,
and `quarantined_at`. `origin` is exactly `bootstrap` or `issuer`. For the
isolated bootstrap path, the bootstrap creates the only valid `origin:bootstrap`
form alongside the early taint under the global D2 lock: `operation` is
`direct-onboard`, `status` is `not_issued`, `process_group_id` is null,
`session_revoked` is false, and both terminal timestamps are null. It uses
exclusive creation and never overwrites an existing lifecycle marker.

The issuer is the only component allowed to compare that exact bootstrap
sentinel and atomically replace it, under the same global D2 lock, with an
`origin:issuer`, `active` marker whose positive process-group ID identifies the
issuer's dedicated process group. The issuer refuses to read credentials unless
`pid == pgrp == sid`. Normal pre-issuance errors end as `not_issued`; confirmed
database revocation ends as `revoked`, even when the product operation itself
failed; uncertain revocation ends as `quarantined`; and an unhandleable
`SIGKILL` leaves `active`. Every issuer-origin marker retains a positive
process-group ID. Only an exact bootstrap sentinel, or an issuer-origin
`not_issued` or `revoked` marker whose process group is proven absent, permits
automated cleanup. `active`, `quarantined`, malformed, or incoherent markers
permanently require manual recovery for that run—neither an expiry estimate nor
a reboot silently promotes them. No session value, fingerprint, revocation
digest, or CSRF value is written to the lifecycle file. Core dumps are disabled
before credentials enter memory and that limit is inherited by the Node
process.

Run the runner unit tests with:

```sh
node --test tools/d2-maintenance/headless_product_runner.test.mjs
```

## Isolated bootstrap command

`d2a_bootstrap.py` creates a new isolated candidate, runs D2A, and retires the
candidate in one command. It keeps the configured sandbox guild and hub channel
but removes only resources recorded by the run-owned Discord transport. It
never invokes `d2_run.py`, D3, commercial finalization, or guild deletion, and
every result remains `release_eligible: false`.

The command consumes an owner-only `0600` sandbox config plus the builder's
owner-only `0400` candidate spec/provenance pair. All are canonical regular
files; unknown fields, symlinks, hard links, non-owner files, and looser modes
are rejected. The sandbox config schema is exact:

```json
{
  "schema_version": 1,
  "kind": "starring.d2a.persistent-sandbox-config.v1",
  "sandbox_id": "macmini-d2a",
  "guild_lifecycle": "persistent_reuse_no_delete_v1",
  "discord": {
    "guild_id": "1536845588954353676",
    "hub_channel_id": "1536845619266846792",
    "application_id": "1533144492293754900",
    "bot_user_id": "1533144492293754900",
    "actor_id": "1056857223529250906",
    "actor_display_name": "보건"
  },
  "credential_refs": {
    "discord_oauth": "starring.d2.credentials:discord.oauth-client-secret",
    "discord_bot": "starring.d2.credentials:discord.bot-token",
    "cloudflare_tunnel": "starring.d2.credentials:cloudflare.tunnel-token"
  },
  "cloudflare": {
    "tunnel_id": "57c22e8a-0ec2-4f67-a882-2c355b0348df",
    "public_origin": "https://d2-api.starring.co.kr"
  },
  "ports": {
    "postgres": 55433,
    "api": 28080,
    "runtime": 29091,
    "worker": 28181,
    "transport_gateway": 29101,
    "transport_http": 29102
  },
  "release_run_root": "/Users/<owner>/Library/Application Support/Starring/release-certifications",
  "d2a_result_root": "/absolute/private/d2a-runs",
  "bootstrap_state_root": "/absolute/private/d2a-bootstrap-runs"
}
```

The application and bot IDs are intentionally the same D2 Discord application
identity. That shared identity must remain distinct from the guild, hub, and
actor IDs. The three credential strings are Keychain service/account references,
not credential values; credential values are forbidden in config, state, output,
arguments, and logs.

`release_run_root` is not configurable in practice: it must equal the current
owner's canonical `~/Library/Application Support/Starring/release-certifications`
directory because the issuer accepts manifests only below that exact root.

The bootstrap accepts only the builder-published v2 candidate spec. Both the
spec and its sibling `provenance.json` are exact owner-only `0400` files inside
the immutable bundle; ordinary sandbox and operator input configs remain
`0600`:

```json
{
  "schema_version": 2,
  "kind": "starring.d2a.candidate-spec.v2",
  "commit_sha": "<40 lowercase hex characters>",
  "source_tree_sha": "<40 lowercase hex characters>",
  "bundle": "/absolute/immutable/candidate-bundle",
  "provenance_sha256": "<raw provenance.json SHA-256>",
  "candidates": {
    "api": {"path": "/absolute/immutable/candidate-bundle/starring-api", "sha256": "<SHA-256>"},
    "certification_transport": {"path": "/absolute/immutable/candidate-bundle/d2-certification-transport", "sha256": "<SHA-256>"},
    "cloudflared": {"path": "/absolute/immutable/candidate-bundle/cloudflared", "sha256": "<SHA-256>"},
    "codex": {"path": "/absolute/immutable/candidate-bundle/codex", "sha256": "<SHA-256>"},
    "codex_worker": {"path": "/absolute/immutable/candidate-bundle/codex-worker/worker.mjs", "sha256": "<SHA-256>"},
    "db_bootstrap": {"path": "/absolute/immutable/candidate-bundle/starring-d2-db-bootstrap", "sha256": "<SHA-256>"},
    "node": {"path": "/absolute/immutable/candidate-bundle/node", "sha256": "<SHA-256>"},
    "runtime": {"path": "/absolute/immutable/candidate-bundle/starring-runtime", "sha256": "<SHA-256>"},
    "sealed_provisioner": {"path": "/absolute/immutable/candidate-bundle/starring-d2-sealed-provisioner", "sha256": "<SHA-256>"}
  }
}
```

Before manifest preparation, the bootstrap loads the sibling provenance by
fixed name and requires the exact top-level and nested schemas: normalized
five-command recipe, complete secret-free build environment, source/git and
builder identities, recursive Rust/toolchain records, Darwin selection, all
artifact/worker/operator records, timestamps, and non-release flags. It verifies
the raw provenance digest and matches all nine path/hash records (five built
artifacts, the worker entry point, and three operators) to the actual immutable
bundle. Unknown, missing, duplicate, boolean-as-version, mutable, or mismatched
fields fail closed.

Both the candidate and issuer builds reject every Cargo config discoverable
from the real build directory through the filesystem root and force offline
Cargo. The candidate uses an initially empty private `CARGO_HOME` plus the
explicit sealed D3 vendor configuration; the issuer uses a private
`CARGO_HOME` containing only validated cache links. Both bind the complete
Cargo environment. They recursively bind the pinned Rust
sysroot and system linker shims before and after compilation. On macOS they
also bind `/usr/bin/xcrun`, `xcode-select`, the resolved clang/ld/ar/ranlib/
otool files, OS build, and the root-owned non-writable SDK tree (including
internal, existing symlink targets). Every built Mach-O is checked with the
resolved `otool`; user/build dylibs and unresolved `@rpath` dependencies are
rejected. The bootstrap first runs
the exact locked release build for the session issuer with the Mac mini's pinned
`~/.rustup/toolchains/stable-aarch64-apple-darwin/bin/cargo` and `rustc`.
Both must be canonical, owner-held executables in the owner-held `0755` toolchain
directory. Rustup proxies, toolchain downloads, and PATH-selected alternatives
are rejected. Before building, the bootstrap executes bounded
`cargo --version` and `rustc --version --verbose` checks and requires Cargo
1.97.0, Rust 1.97.0, and host `aarch64-apple-darwin`; both executable SHA-256
values are rechecked after the build and retained in durable state and the
final result. The issuer is linked and fully checked in its unique private
target, the clean source commit/tree is rechecked, and only then is the fixed
credential-capable path atomically replaced. A second source check immediately
follows publication.

The fixed `/usr/bin/git` view must also show a clean worktree whose exact `HEAD`
commit and tree equal the v2 candidate spec. The same check is repeated after
the issuer build, before direct onboarding, before each authenticated product
operation, before offline verification, and again before teardown. A drift at
any boundary fails into the normal Discord teardown/local cleanup path, so the
maintenance issuer, controller, runner, and candidate binaries cannot silently
mix revisions. Durable state and the final result record that
commit/tree while the taint and direct-onboarding evidence bind the exact issuer
source digest and built issuer hash. Only after these identities are fixed does
the bootstrap create durable state or a manifest.

Run either the authentication maintenance check or the additional checked-in
one-shot product scenario:

```sh
python3 tools/d2-maintenance/d2a_bootstrap.py run \
  --sandbox-config /absolute/private/d2a-sandbox.v1.json \
  --candidate-spec /absolute/immutable/candidate-bundle/candidate-spec.json \
  --operation one-shot
```

The sequence is fixed: manifest preparation, byte-exact early D2A taint plus
the exclusive pre-issuer lifecycle sentinel, orchestrator dry-run, absence
preflight, prepare, start, issuer `direct-onboard`, auth smoke, optional
one-shot, offline evidence verification, run-owned Discord teardown, local
cleanup, and final absence/protected-standing checks. Commercial orchestrator
`onboard` is never called. Before and after direct onboarding, the bootstrap
rejects both commercial onboarding evidence paths.

Direct onboarding invokes only the fixed issuer binary with the manifest,
`--operation direct-onboard`, and the configured display name. Its stdout must
equal the persisted `d2a-onboarding-evidence.json` object. The bootstrap
requires the exact 21-field schema and binds the run, manifest, principal,
guild, Discord application, hub, installation, sealed provisioner, issuer, and
issuer source hashes. The hub-preflight, direct-auth, and revoked-session flags
must be true, `release_eligible` must be false, and the evidence file must be an
owner-held single-link `0600` regular file. Its path and raw SHA-256 are retained
in both durable bootstrap state and the final result.

The nonblocking outer bootstrap lock is the fixed
`/private/tmp/starring-d2a-bootstrap.lock`. It is deliberately distinct from
the issuer/coordinator global D2 lock at
`/private/tmp/starring-d2-certification.lock`. While holding the outer lock, the
bootstrap briefly takes the global D2 lock to create the byte-exact taint and
the `origin:bootstrap` lifecycle sentinel as one serialized boundary. Sentinel
creation uses `O_EXCL`: only an absent lifecycle path is accepted, and no
existing marker—even another bootstrap-shaped marker—is overwritten. The
bootstrap releases the global D2 lock before launching the issuer. Each child
operation still acquires its own normal D2/coordinator protection, while the
bootstrap lock rejects another overlapping D2A bootstrap.

Cargo and every child command are supervised as dedicated process groups with
simultaneous bounded stdout/stderr streaming, a monotonic deadline, and
TERM-then-KILL reaping. Each issuer build has a unique target and a durable
`issuer-build-lifecycle.json` recording its secret-free environment digest,
process group, and `active`, `passed`, `failed`, or `quarantined` state.
Unproven group absence is `quarantined` and forbids target reuse or cleanup.

Failures and interrupts leave a durable `0600` state path in the bounded JSON
result. Resume performs cleanup only; it never repeats a product operation:

```sh
python3 tools/d2-maintenance/d2a_bootstrap.py resume \
  --state /absolute/private/d2a-bootstrap-runs/bootstrap-d2-....json
```

Discord teardown transitions a private `d2a-teardown-fence.json` from open to
`closing` under the same global D2 lock used for lifecycle validation, then to
`closed` only after successful run-owned teardown. Cleanup independently
requires the exact coherent taint, terminal session lifecycle, and closed
fence; deleting one marker cannot downgrade a D2A run. The one pre-candidate
exception is the exact `origin:bootstrap` sentinel: cleanup may close the fence
without Discord teardown only after the orchestrator proves the run is still
merely prepared, no candidate-start commitment or mutation/transport/teardown
artifact exists, every run-owned launchd service is absent, and PostgreSQL is
not running. If candidate start was already committed before issuer handoff
failed, that sentinel follows the normal full run-owned Discord teardown and
closed-fence path instead. Any other bootstrap-shaped, malformed, incoherent, hard-linked, or
non-canonical marker fails closed as `manual_recovery_required`. A teardown
failure keeps `closing` and blocks cleanup; `origin:issuer` `active` or
`quarantined` does the same. A recovered ordinary failed test remains failed;
start a new run for a fresh passing result.

Run the mocked bootstrap tests (they start no real service) with:

```sh
python3 -m unittest tools/d2-maintenance/test_d2a_bootstrap.py
```

## Local maintenance candidate builder

`d2a_candidate.py` produces the immutable nine-path candidate spec consumed by
the bootstrap. It is a local packaging tool only: its provenance explicitly
sets `release_eligible: false` and `commercial_certification: false`, and it
does not run D2, D3, deployment, migration, or certification commands.

The builder refuses to create state or build output unless the repository is
the canonical owner-held worktree, `HEAD` resolves to a commit and tree, and
both porcelain status and staged/unstaged diff checks are empty. Untracked
files are dirty. Source HEAD, the worker sources, operator inputs, and Rust
toolchain identities are checked again after all five builds and before any
bundle is published.

Its exact `0600` v2 input has three operator paths and one sealed D3 dependency
boundary:

```json
{
  "schema_version": 2,
  "kind": "starring.d2a.candidate-operator-config.v2",
  "operators": {
    "codex": "/absolute/immutable/codex",
    "node": "/absolute/immutable/node",
    "cloudflared": "/absolute/immutable/cloudflared"
  },
  "dependencies": {
    "bootstrap_root": "/absolute/d3-state/gate-bootstrap",
    "record_path": "/absolute/d3-state/gate-bootstrap.json",
    "record_sha256": "<parsed D3 record self-seal>",
    "tree_sha256": "<D3 sealed bootstrap tree digest>"
  }
}
```

Each operator must be a distinct canonical, owner-held, single-link `0555`
regular file in an owner-held non-writable directory. The builder accepts no
credential fields.

The dependency paths must name one D3 `gate-bootstrap` and its sibling
`gate-bootstrap.json`. The builder verifies the record's canonical bytes and
self-seal, then reproduces the complete D3 tree digest with owner, mode,
single-link, symlink, entry-count, byte-count, and stable-read checks. It
requires the exact native workspace and transport Cargo configurations and
their read-only vendor roots. The current workspace/transport manifests and
lockfiles are identity-bound, and bounded offline
`cargo metadata --locked --no-deps`
checks prove they resolve against that vendor before compilation. The whole
dependency snapshot is revalidated before and after every build and on both
sides of publication; it is embedded verbatim in provenance.

The Rust boundary is fixed to the canonical owner-held
`~/.rustup/toolchains/stable-aarch64-apple-darwin/bin/{cargo,rustc}`. It rejects
rustup proxies and verifies Cargo 1.97.0 plus Rust 1.97.0 with host
`aarch64-apple-darwin`. The five build commands match the D3 candidate recipe's
package/bin boundaries:

```text
cargo --config <sealed-workspace-config> build --frozen --release --target-dir <workspace-target> -p starring-api --bin starring-api
cargo --config <sealed-workspace-config> build --frozen --release --target-dir <workspace-target> -p starring-runtime --bin starring-runtime
cargo --config <sealed-workspace-config> build --frozen --release --target-dir <workspace-target> -p starring-db-bootstrap --bin starring-d2-db-bootstrap
cargo --config <sealed-workspace-config> build --frozen --release --target-dir <workspace-target> -p starring-staging-provisioner --bin starring-d2-sealed-provisioner
cargo --config <sealed-transport-config> build --frozen --release --manifest-path tools/d2-certification-transport/Cargo.toml --target-dir <transport-target>
```

Every command receives `STARRING_RUNTIME_BUILD_REVISION=<exact HEAD>`, frozen
and offline Cargo settings, one build job, disabled incremental compilation,
the fixed compiler/linker/SDK paths, and the isolated Cargo home. Build output
is streamed with a fixed cap and deadline; cap+1, timeout, interruption, or an
orphan pipe-holder terminates and reaps the whole process group. The durable
candidate state sets `build_processes_quiescent:false` before every spawn and
allows scratch cleanup only after absence is proven. The builder copies exactly the seven production
Codex worker modules; test modules and `package.json` never enter the bundle.

Run it only after committing the complete maintenance implementation:

```sh
python3 tools/d2-maintenance/d2a_candidate.py build \
  --config /absolute/private/d2a-candidate-operators.v2.json
```

Successful bundles are unique children of
`~/Library/Application Support/Starring/d2a-candidates`. The final bundle and
worker directory are `0555`; five built binaries plus copied codex/node/
cloudflared files are `0555`; the seven worker files are `0444`; and
`candidate-spec.json` plus `provenance.json` are separate owner-only `0400`
files. The spec has the exact `starring.d2a.candidate-spec.v2` shape and binds
the source tree, immutable bundle path, nine path/hash records, and raw
`provenance.json` SHA-256. Provenance binds the source commit/tree and
`clean: true`, exact commands, the sealed dependency snapshot, toolchain
versions and hashes, builder identity, source/destination artifact hashes, and
the non-certifying flags.

Publication happens by one rename only after the staging bundle and every hash
have been verified. A failed or interrupted build returns a durable `0600`
state but no published candidate spec. Cleanup is explicit and bounded to that
state's hidden build/staging directories:

```sh
python3 tools/d2-maintenance/d2a_candidate.py resume-cleanup \
  --state "/absolute/Application Support/Starring/d2a-candidates/state-d2ac-....json"
```

If interruption occurred after the atomic rename, resume validates and retains
the completed immutable bundle, removes only build scratch data, and reports it
as passed. It never deletes a published bundle. If a parent crash leaves build
quiescence unknown, resume fails with `candidate_manual_recovery_required` and
preserves the unique target rather than risking a race with orphan Cargo.

The current working tree is intentionally dirty while this feature is being
developed, so no real five-binary build was run. The mocked tests start no
compiler or service:

```sh
python3 -m unittest tools/d2-maintenance/test_d2a_candidate.py
```
