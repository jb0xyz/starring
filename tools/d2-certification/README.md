# D2 disposable-guild certification

`d2_certification.py` owns the immutable run manifest and the ordered,
secret-free evidence ledger for the 17 D2 product E2E steps. It does not create
or delete a Discord guild, read a credential, start a process, mutate the
standing staging database, or claim that an incomplete run passed.

Every current manifest is closed to the exact top-level field inventory, carries
`certification_class: commercial_human_v1`, and pins the ordered human
boundaries for disposable-guild creation, Discord OAuth, initial product-preview
confirmation, real Discord interactions, replacement-preview confirmation, and
disposable-guild deletion. Automated, missing, or reordered boundary claims are
rejected rather than treated as commercial certification evidence.
If the separate D2A maintenance issuer ever uses a commercial run, it first
creates the durable run-local `d2a-taint.json` marker. Coordinator status,
advance, verification, and D3 binding then reject that run permanently; only
the normal isolated-resource cleanup path remains valid.

The preparation command binds one run to an exact Git commit, immutable API
and runtime binaries, the Codex worker and its executables, a unique resource
prefix, unique loopback ports, unique launchd labels, unique Keychain service
names, an isolated PostgreSQL root, and one exact disposable-guild text channel
as the `community_hub` resource binding. It also binds the fixed D2 Cloudflare
route as one indivisible tuple: tunnel
`57c22e8a-0ec2-4f67-a882-2c355b0348df`, public origin
`https://d2-api.starring.co.kr`, and loopback API origin
`http://127.0.0.1:28080`. The protected standing staging service names are
recorded with mutation disabled.

Each step is appended in order under an exclusive file lock. The recorder
rejects missing acceptance fields, invalid cross-step identities, nonzero
duplicate effects, unresolved final state, known credential fields, credential
URLs, bearer values, and full prompts or transcripts. The verifier passes only
after all 17 receipts satisfy the exact contracts.

`isolated_orchestrator.py` implements the disposable substrate lifecycle. Its
dry run rejects standing ports, labels, Keychain services, public origin, and
Discord application identity reuse. Preparation creates only the exact
manifest-derived PostgreSQL root, five unloaded launchd plists, a tunnel runner,
and per-run Keychain ownership markers. Start launches PostgreSQL and invokes
the immutable `starring-d2-db-bootstrap` candidate, which reuses the production
SQLx bootstrap library and records the exact migration ledger. It then invokes
the immutable sealed provisioner, creates or exactly replays 20 application
database credentials, one cluster-administrator credential, three application
keyrings, one worker bearer credential,
activates the 20 application roles, changes PostgreSQL from socket-only
bootstrap access to role-specific loopback SCRAM access, verifies replay after
the sealed restart, and starts the certification transport, worker, API,
runtime, and tunnel in that order. The transport owns two loopback listeners
and a manifest-owned `0600` Unix control socket. Runtime uses its explicit
`loopback_proxy_v1` mode, so Discord gateway and interaction-effect traffic
cannot silently fall back to the direct production transport. Controller
preflight and strict panel reconciliation remain direct and are isolated by the
dedicated D2 application and guild. Step 3 evidence binds the exact
candidates, transport source tree, control snapshot readiness, and Cloudflare
route tuple. It also joins the API and runtime launchd PID, program, plist,
arguments, and run counter to two stable macOS process samples. Each sample
binds the libproc start timestamp and executable path to an `O_NOFOLLOW`
file-descriptor digest, and runtime must report the same PID through its health
identity endpoint. Step 3 also pins the transport process instance identity.
Any transport restart changes that identity and invalidates the certification
run instead of silently resetting the in-memory fault counters. Direct `stop`
and `cleanup` are idempotent abort and recovery commands. After candidate-start
commitment they durably retire the run, operate only on manifest-derived
identities, and require the standing launchd, plist, and port snapshot to
remain unchanged. Candidate launchd jobs are bootstrapped without writing a
persistent enabled/disabled override. Dry-run, start, cleanup, and replay all
require both the exact run labels to be unloaded and their override entries to
be absent; unrelated historical labels do not affect a fresh run.

The complete run-owned Keychain boundary is 25 secret items and four lifecycle
ownership markers. The secret items are the 20 application database
credentials, one cluster-administrator credential, three purpose-separated
keyrings, and one worker bearer credential. Together with the four markers,
that is exactly 29 run-owned items. The dedicated Discord OAuth secret, Discord
bot token, and Cloudflare tunnel token are three external read-only items; the
orchestrator must preserve them and never copy, rotate, or delete them.

The transport control snapshot is version 4. Its effect listener reports the
one-way admission phase (`open`, `draining`, or `teardown_delete_only`), the
durable finalization operation ID, and accepted, active, completed, and
uncertain request counters. Accepted requests always equal active plus
completed requests. A mutation whose upstream outcome or owned-resource
inventory commit cannot be proved increments the lifetime uncertainty counter;
finalization requires that counter to be exactly zero. Delete ownership and
request admission are reserved atomically, so concurrent deletion of the same
resource cannot be forwarded twice.

The migration-ledger SHA-256 is reproducible. For each successful ledger row
ordered by version, hash the signed 64-bit version in big-endian form, one
success byte, the checksum length as an unsigned 64-bit big-endian value, and
the raw SQLx checksum bytes.

Every executable candidate and the Codex worker entrypoint must be a canonical,
same-owner regular file with all write bits removed. Its immediate artifact
directory must have the same owner and all write bits removed, preventing path
replacement after manifest verification. The Python and Rust source trees are a
different boundary: they may remain owner-writable for development, but their
exact inventory and SHA-256 are revalidated at the start of every certification
command; group/world-writable source files or roots are rejected. They are
verified per invocation and are not described as immutable artifacts.

## Order

D2 runs between two D3 phases and cannot be run before them.

1. Freeze the branch. Any later commit, and any movement of the base, changes the
   merge tree and invalidates completed D2 receipts.
2. Open the release pull request.
3. `d3_certification.py prepare` pins the repository, pull request, head, base,
   and GitHub-generated merge candidate, and creates `<D3_RUN>/worktree` as a
   detached checkout of that merge candidate.
4. `d3_certification.py run-gates` runs the fixed gate manifest and then builds
   and seals `<D3_RUN>/candidate-bundle`.
5. Run D2 against that bundle through all seventeen steps.
6. `d3_certification.py bind-d2`, then `recheck`, then merge, then `finalize`.

`D3_STATE` is the absolute `state.json` path returned by D3 `prepare`, matching
the D3 runbook. `<D3_RUN>` is the directory containing that file.

## Candidate provisioning

D3 owns candidate provisioning. `run-gates` builds the five release binaries and
the seven-file Codex worker tree from the exact merge candidate and seals them
into `<D3_RUN>/candidate-bundle`. That directory is mode `0555`, its binaries
`0555`, its worker files `0444`, and `bundle.json` and `publication.json` `0400`.
Do not build or install candidates by hand.

`bind-d2` rejects a manifest whose candidate or source-tree identities do not
match the sealed bundle exactly:

| Manifest field | Required value |
| --- | --- |
| `candidates.api` | `<D3_RUN>/candidate-bundle/starring-api` |
| `candidates.runtime` | `<D3_RUN>/candidate-bundle/starring-runtime` |
| `candidates.db_bootstrap` | `<D3_RUN>/candidate-bundle/starring-d2-db-bootstrap` |
| `candidates.sealed_provisioner` | `<D3_RUN>/candidate-bundle/starring-d2-sealed-provisioner` |
| `candidates.certification_transport` | `<D3_RUN>/candidate-bundle/d2-certification-transport` |
| `candidates.codex_worker` | `<D3_RUN>/candidate-bundle/codex-worker/worker.mjs` |
| `source_trees.codex_worker.root` | `<D3_RUN>/candidate-bundle/codex-worker` |
| `source_trees.d2_toolchain.root` | `<D3_RUN>/worktree/tools/d2-certification` |
| `source_trees.certification_transport.root` | `<D3_RUN>/worktree/tools/d2-certification-transport` |

`node`, `codex`, and `cloudflared` are not release artifacts. They remain
operator-supplied installed executables and are not bound to the sealed bundle.

D2 has no knowledge of the bundle. It accepts whatever `--candidate` paths it is
given, so a manifest assembled from a hand-installed directory completes all
seventeen steps and fails only at `bind-d2`, discarding the run and its
disposable guild. Seal the bundle first.

The current execution boundary is deliberate. `start` reports
`candidate_services_loaded=true` only after all five services are loaded and
their local and public health probes pass. A dedicated D2 Discord application,
guild, and credential pair remain mandatory because sharing the standing
gateway session would violate the no-staging-mutation contract. The fixed D2
Cloudflare tunnel token is an external read-only Keychain input. The
orchestrator never copies or deletes any of the three external credentials.

The remaining certification work is:

1. Create a disposable Discord guild for this run and select one text channel as
   `community_hub`. The dedicated D2 Discord application and the three external
   Keychain identities holding its OAuth client secret, its bot token, and the
   fixed Cloudflare tunnel token are already provisioned and are reused across
   runs. The guild is not.
2. Complete steps 3 and 4 of the order above so the sealed bundle exists, then
   prepare and start the immutable candidate run against that bundle.
3. Complete OAuth and invoke `onboard` with the authenticated Discord principal.
4. Load `product_driver.js` into the authenticated same-origin browser and use
   its bounded product driver for authoring, promotion, approval, Apply, and
   status observations. The driver reads only the CSRF cookie, leaves the
   HTTP-only product session untouched, requires an explicit operator preview
   confirmation before approval, and excludes the prompt, assistant message,
   and full RuleSet from its returned evidence.
5. Use the candidate-only certification transport to exercise exact duplicate
   delivery and one deterministic indeterminate Discord outcome.
6. Perform the visible Discord interactions, replacement, disconnect, and
   teardown observations, then delete the disposable guild.
7. Run the redacted PostgreSQL and Discord absence probes before recording
   step 17.

Cross-time route joins use a domain-separated route-lineage digest rather than
the full point-in-time route snapshot. The lineage contains the deployment,
runtime generation, controller fence, route incarnation, process instance,
serving lease epoch, gateway shard, and gateway-owner lease epoch. It excludes
only `origin_serving_revision` and `origin_gateway_owner_revision`, because
normal serving heartbeats and gateway-owner renewals advance those counters.
The complete raw route remains exact-shape validated and is still sealed by
the coordinator-source digest. Steps 8 and 9 carry both counters separately;
Step 9 requires each to be no lower than Step 8. Step 12 applies the same rule
from the Step 9 route to its old-lineage source, while Step 13 applies it from
the reconstructed route to the later reconciliation receipt. A process,
generation, fence, incarnation, epoch, shard, or deployment change remains a
lineage mismatch. Counters are not ordered across a controlled reconstruction
or replacement, and Step 14 compares only lineage because its attestation
source intentionally projects the route's initial revisions.

The receipt-level serving-lease digest follows the same split: it binds every
serving field except the heartbeat-renewed `revision`. The full serving object
is still validated and source-hash sealed, and reconstruction requires each
route/serving pair to agree on process, runtime generation, serving epoch, and
serving revision. These receipt semantics apply only to fresh runs that pin
this exact D2 toolchain; a run created by an earlier toolchain is never resumed
under the new identity domains.

Example preparation shape:

```text
D3_STATE=/absolute/d3/output-root/run-id/state.json
D3_RUN="$(dirname "$D3_STATE")"
BUNDLE="$D3_RUN/candidate-bundle"
D2_TOOLCHAIN="$D3_RUN/worktree/tools/d2-certification"
CANDIDATE_COMMIT="$(jq -er '.merge_commit' "$D3_STATE")"

python3 "$D2_TOOLCHAIN/d2_certification.py" prepare \
  --output-root "$HOME/Library/Application Support/Starring/release-certifications" \
  --commit "$CANDIDATE_COMMIT" \
  --discord-guild-id "$DISPOSABLE_GUILD_ID" \
  --discord-hub-channel-id "$DISPOSABLE_HUB_CHANNEL_ID" \
  --discord-application-id "$DISCORD_APPLICATION_ID" \
  --discord-bot-user-id "$DISCORD_BOT_USER_ID" \
  --discord-actor-id "$DISCORD_ACTOR_ID" \
  --discord-oauth-keychain starring.d2.credentials:discord.oauth-client-secret \
  --discord-bot-keychain starring.d2.credentials:discord.bot-token \
  --tunnel-token-keychain starring.d2.credentials:cloudflare.tunnel-token \
  --cloudflare-tunnel-id 57c22e8a-0ec2-4f67-a882-2c355b0348df \
  --public-origin https://d2-api.starring.co.kr \
  --candidate api="$BUNDLE"/starring-api \
  --candidate runtime="$BUNDLE"/starring-runtime \
  --candidate codex_worker="$BUNDLE"/codex-worker/worker.mjs \
  --candidate db_bootstrap="$BUNDLE"/starring-d2-db-bootstrap \
  --candidate sealed_provisioner="$BUNDLE"/starring-d2-sealed-provisioner \
  --candidate certification_transport="$BUNDLE"/d2-certification-transport \
  --candidate codex=/absolute/installed/codex \
  --candidate node=/absolute/installed/node \
  --candidate cloudflared=/absolute/installed/cloudflared \
  --port postgres=55433 \
  --port api=28080 \
  --port runtime=29091 \
  --port worker=28181 \
  --port transport_gateway=29101 \
  --port transport_http=29102
```

`$BUNDLE` is `<D3_RUN>/candidate-bundle`, and `--commit` must be the exact
GitHub-generated merge commit pinned by D3. `bind-d2` rejects a D2 manifest
that names the branch tip or any other commit, even when its tree is equal.
Every D2 Python command must use `$D2_TOOLCHAIN` from the same detached D3
worktree. Preparing from the ordinary branch checkout pins the wrong
`source_trees.d2_toolchain.root` and cannot bind; running later commands there
executes a toolchain other than the one D3 binds. Such a run cannot pass
`bind-d2` even if all seventeen steps complete.

`codex`, `node`, and `cloudflared` are the only hand-supplied executables. Each
must still be a canonical, same-owner regular file with all write bits removed,
inside a directory with the same owner and no write bits.

Then run the substrate lifecycle with the immutable manifest:

```text
D2_RUN=/absolute/d2/run
MANIFEST="$D2_RUN/manifest.json"
ORCH="$D2_RUN/orchestrator"
FINAL="$ORCH/finalization"

python3 "$D2_TOOLCHAIN/isolated_orchestrator.py" dry-run \
  --manifest "$MANIFEST"
python3 "$D2_TOOLCHAIN/d2_preflight_evidence.py" \
  --manifest "$MANIFEST"
python3 "$D2_TOOLCHAIN/isolated_orchestrator.py" prepare \
  --manifest "$MANIFEST"
python3 "$D2_TOOLCHAIN/isolated_orchestrator.py" start \
  --manifest "$MANIFEST"
python3 "$D2_TOOLCHAIN/isolated_orchestrator.py" onboard \
  --manifest "$MANIFEST" \
  --principal-id discord:<authenticated-user-id> \
  --display-name <authenticated-display-name>
python3 "$D2_TOOLCHAIN/isolated_orchestrator.py" transport-control \
  --manifest "$MANIFEST" \
  --operation snapshot
python3 "$D2_TOOLCHAIN/isolated_orchestrator.py" certify-live-runtime-restart \
  --manifest "$MANIFEST"
python3 "$D2_TOOLCHAIN/isolated_orchestrator.py" certify-live-runtime-restart \
  --manifest "$MANIFEST" \
  --confirmation-file /absolute/live-runtime-restart-confirmation.json
```

`d2_preflight_evidence.py` must run after the successful dry run and before
orchestrator `prepare`. Its returned `coordinator_source` is the reviewed step
2 source. `isolated_orchestrator.py prepare` creates run-owned state, so
capturing prior-absence evidence afterward is invalid by construction.

`certify-live-runtime-restart` performs the one admitted drained-runtime
restart internally. Do not invoke standalone `restart-drained-runtime` as an
additional step in the successful certification sequence.

`start` durably binds the Step 3 evidence and standing snapshot in a pending
candidate-start transition before publishing the consumable coordinator
source. An interrupted start adopts the same live API, runtime, plist, health,
and transport identities and completes without launching replacements. Live
identity drift during pending publication or recovery makes the run
retirement-only. Before a runtime-restart protocol begins, a completed-start
replay verifies the immutable transition and source together with their exact
process identities, current service health, the pinned transport instance, and
the standing snapshot. Once a drained or live runtime-restart protocol begins,
that protocol owns replay and `start` fails non-mutating with
`orchestrator_phase_invalid`; this keeps the certified Step 11 replacement
valid. Direct `stop` or `cleanup` after commitment also retires the run:
certification commands fail with
`candidate_start_transition_retirement_required`, and the operator must run
abort cleanup and prepare a new disposable run. The old source is never deleted
or rebound to replacement processes.

The successful certification path never calls direct `stop`, direct `cleanup`,
or standalone `teardown-discord-resources`. Those commands are abort-only after
commitment. Successful teardown goes through `finalize-run` after coordinator
step 15; it invokes a private certified cleanup boundary and remains eligible
for steps 16 and 17. Standalone teardown writes a durable abort tombstone before
the first deletion. The tombstone blocks coordinator status, advance, and
verify as well as D3 bind, recheck, and finalize, while the same teardown and
cleanup commands remain available to resume an interrupted abort.

One historical automated-maintenance run is explicitly allowlisted for
`recover-audited-preissuer-rollback`. Its candidate start failed and completely
rolled back before the issuer took over the bootstrap `not_issued` sentinel,
but the then-current cleanup gate did not admit the resulting `stopped` phase.
Normal manifest loading and bootstrap `resume` remain source-exact and are not
relaxed. The repair command instead validates the canonical historical
manifest, immutable candidates and worker tree, exact bootstrap/state/journal/
taint/lifecycle identity, empty receipts, absence of candidate commitment and
post-start artifacts, all run launchd jobs and PostgreSQL processes absent, and
the protected staging snapshot unchanged. It observes—without rebinding—the
historical versus current controller and transport source hashes.

The repository must be a clean committed HEAD. The operator supplies the exact
current commit and tree as well as the historical run and manifest digest:

```text
python3 tools/d2-certification/isolated_orchestrator.py \
  recover-audited-preissuer-rollback \
  --manifest /absolute/historical-run/manifest.json \
  --bootstrap-state /absolute/bootstrap-state.json \
  --confirm-current-commit <clean-current-HEAD> \
  --confirm-current-tree <clean-current-HEAD-tree> \
  --confirm-run-id <historical-run-id> \
  --confirm-manifest-sha256 <historical-manifest-sha256>
```

Under the global D2 lock it fsyncs an immutable, secret-free recovery intent,
closes the pre-start teardown fence, invokes the existing scoped cleanup
primitive, rechecks total absence and current source identity, and writes final
evidence. Interrupted executions replay only from that exact intent and source
revision. Any other run, same-as-historical source, dirty or differently
confirmed source, ambiguous journal, active service/process, artifact,
commitment, or protected-staging drift fails before the intent is created.

`onboard` creates or exactly replays revision 1 with the single external
channel binding `community_hub -> discord.hub_channel_id`. Its redacted output
and `onboarding-evidence.json` both retain `binding_key: community_hub` and the
manifest-pinned `hub_channel_id`; a missing, different, or extra manifest field
fails closed before candidate execution. Before database mutation, the bot
must also resolve that ID as a type-0 text channel in the manifest guild. The
persisted authority payload digest uses the same canonical revision, binding,
policy, and TTL identity contract enforced by runtime hydration.

`certify-live-runtime-restart` owns the step 11 process boundary and internally
performs exactly one replacement drain recovery. It requires exactly the first
ten verified D2 receipts, binds their prior live witness and the deployment,
route, and instance from steps 8 and 9, and accepts only the fixed
`live_fresh_lease` checkpoint.
Before signaling, it joins the exact old launchd PID to the loopback runtime
health identity and fsyncs that per-boot process instance ID, launchd run
count, runtime candidate and plist identity, dependency processes, transport
instance, standing snapshot, and receipt-chain head. It sends only `SIGTERM` to the
manifest runtime label, requires a clean exit within 30 seconds, and then
observes a complete 30-second launchd throttle window with no PID, the same run
count, exit code `0`, and closed readiness. It uses `restart-drained-runtime`
for the exact inactive start and verifies a stable new PID with a different
per-boot process instance ID.

The first invocation stops at `awaiting_canonical_confirmation` and does not
write completion or step 11 evidence. In the authenticated browser, call
`liveRuntimeRestartConfirmation` on the loaded `StarringD2ProductDriver` with
the returned `operation_id`, `installation_id`, `promotion_id`,
`process_instance_id`, and `shutdown_boundary`. The helper reads both canonical deployment endpoints,
requires product and operational `live`, runtime `live`, serving `fresh`, exact
heartbeat and lease agreement, the same positive attestation revision, the
exact expected process instance ID, and a heartbeat later than the durable
shutdown boundary. It also seals the driver's
normalized public origin. Its returned object is the complete redacted JSON
contract; save only that JSON to an absolute, owner-controlled mode-`0600`
file. Rerun the command with `--confirmation-file` before the fixed 45-second
serving lease expires. If it expires, run only the browser helper again and
replace the confirmation file. A confirmation supplied before the durable
awaiting record is rejected before any signal or restart.

```js
const product = StarringD2ProductDriver.create();
const confirmation = await product.liveRuntimeRestartConfirmation({
  operationId: "<operation_id from phase 1>",
  installationId: "<installation_id from phase 1>",
  promotionId: "<promotion_id from phase 1>",
  processInstanceId: "<process_instance_id from phase 1>",
  shutdownBoundary: "<shutdown_boundary from phase 1>",
});
window.prompt(
  "Copy the exact live-runtime-restart confirmation JSON",
  JSON.stringify(confirmation),
);
```

The second invocation accepts only the exact schema and binds its operation,
installation, promotion, public origin, shutdown boundary, and canonical
process instance ID to the durable local restart. Only then does it publish
completion and mode-`0600` step 11 evidence. The receipt step is named
`runtime_restarted_with_canonical_process_identity` and binds the full
confirmation digest, operation, scope, boundary, origin, attestation revision,
and process identity. It records `process_identity_joined: true` only after the
launchd PID, loopback health identity, and authenticated canonical attestation
form one exact three-way join.

The browser boundary supplies the authenticated canonical product observation
without giving the certification tool a reusable product session. The flow performs no direct database
observation and stores no cookie, token, or session fingerprint. Interrupted
operations resume from their durable phase, and completed operations replay
without another signal or a current-lease requirement. The command returns a
`coordinator_source`; advance the coordinator with that exact source:

```bash
python3 "$D2_TOOLCHAIN/d2_run.py" advance \
  --manifest "$MANIFEST" \
  --step 11 \
  --source <coordinator_source-returned-by-certify-live-runtime-restart>
```

`restart-drained-runtime` is available only while the manifest is in
`candidate_started`. It requires the prior runtime PID and readiness to be
absent after drain, unloads only that inactive manifest job, and launches the
same label from the exact generated runtime plist. It never terminates a live
runtime, and an inactive loaded job is accepted only with launchd state
`exited` and last exit code `0`. A fresh command rejects an absent launchd job;
absence is accepted only while replaying a durable pending intent after its
recorded inactive job was booted out. Before and after the operation it binds the
immutable runtime candidate and plist digests, the exact API, worker,
transport, and tunnel programs, complete argument vectors, loaded plist paths,
run counters, plist digests, and process identities, the PostgreSQL
process identity, the pinned transport instance, and the standing staging
snapshot. Completion seals the stable new launchd PID and run counter together
with its libproc executable identity, process start timestamp, and exact
runtime health process-instance identity. Every later active certification
boundary revalidates that sealed generation. Retrying an interrupted intent
resumes the observed new runtime, while retrying the sole completed operation
returns an exact replay without starting another process. A second generation
is not admitted into the release chain and permanently retires the run. Intent
and completion publication uses a separate
same-filesystem temporary directory; only strict owned `0600` interrupted
files are recoverable. Any unjournaled PID, failed drain, unexpected temporary
entry, or identity drift fails closed.

`finalize-run` first fsyncs an admission-freeze intent while the exact runtime
and transport generation are still live. It then closes effect admission,
suspends the bound runtime, waits for every pre-close request to settle, and
requires zero uncertainty before sealing the transport snapshot and resource
inventory. Only then does it stop tunnel and runtime, fsync a teardown-admission
intent, and move the same transport operation into delete-only mode. In that
mode only atomically reserved, run-owned resource DELETE requests can reach
Discord. Every replay revalidates the pinned transport instance, operation ID,
counter monotonicity, durable intent digests, and a drained boundary before
continuing. Once the validated effect-freeze intent exists, only finalization
recovery, total-absence finalization, read-only status, or an explicit abort is
admitted. Candidate and fault commands return `orchestrator_phase_invalid`
without inspecting intentionally stopped services or creating a retirement
marker.

Fault injection uses only the manifest-bound orchestrator commands below. Each
command verifies the pinned transport instance, writes and fsyncs a durable
operation intent, performs one exact control request over the private Unix
socket, verifies the postcondition, and writes a secret-free completion
snapshot. If the response is lost, retry the same command. The pending intent
reuses the operation ID, so an already armed fault is recognized as an exact
replay. A different pending operation, a busy arm, a widened response schema,
or a changed transport instance fails closed.

```text
python3 "$D2_TOOLCHAIN/isolated_orchestrator.py" transport-control \
  --manifest /absolute/run/manifest.json \
  --operation arm-next-duplicate
python3 "$D2_TOOLCHAIN/isolated_orchestrator.py" transport-control \
  --manifest /absolute/run/manifest.json \
  --operation disarm-duplicate
python3 "$D2_TOOLCHAIN/isolated_orchestrator.py" transport-control \
  --manifest /absolute/run/manifest.json \
  --operation arm-next-indeterminate
python3 "$D2_TOOLCHAIN/isolated_orchestrator.py" transport-control \
  --manifest /absolute/run/manifest.json \
  --operation disarm-indeterminate
python3 "$D2_TOOLCHAIN/isolated_orchestrator.py" transport-control \
  --manifest /absolute/run/manifest.json \
  --operation partition-gateway
python3 "$D2_TOOLCHAIN/isolated_orchestrator.py" transport-control \
  --manifest /absolute/run/manifest.json \
  --operation heal-gateway
```

`arm-next-duplicate` applies only to the next eligible manifest-owned Discord
interaction. `arm-next-indeterminate` applies only to the next manifest-owned
`create_role` effect. Run each arm immediately before its reviewed product
action, then use a `snapshot` completion evidence object to populate the exact
step 10 or step 13 receipt fields. For step 15, partition the gateway, record
the failed-closed observation and partition counters, then heal it. Raw socket
clients and unbound network fault tools are outside the certification contract.

Every lifecycle operation takes a machine-wide nonblocking D2 lock. Mutation
intent and completion receipts are append-only and fsynced, including the
parent-directory entries created by atomic rename or first journal creation.
Cleanup reconstructs
the owned root, labels, and Keychain accounts from the immutable manifest rather
than trusting the last state write, so it also recovers a prior interrupted run.

Use the coordinator for every certification step. `status` returns the next
required source kinds and execution modes. Repeat `advance` in order for steps
1 through 15 with the reviewed mode-`0600` sources required by that status:

```text
python3 "$D2_TOOLCHAIN/d2_run.py" status \
  --manifest "$MANIFEST"

python3 "$D2_TOOLCHAIN/d2_run.py" advance \
  --manifest "$MANIFEST" \
  --step <1-through-15> \
  --source /absolute/reviewed-source-1.json \
  --source /absolute/reviewed-source-2.json
```

After coordinator step 15, use the certified finalization path and then advance
steps 16 and 17 with its exact machine evidence and the two chronological
browser observations:

```text
python3 "$D2_TOOLCHAIN/isolated_orchestrator.py" finalize-run \
  --manifest "$MANIFEST"

python3 "$D2_TOOLCHAIN/d2_run.py" advance \
  --manifest "$MANIFEST" \
  --step 16 \
  --source "$FINAL/database-precleanup.json" \
  --source "$ORCH/discord-resource-teardown-evidence.json" \
  --source "$FINAL/orchestrator-finalization.json"

python3 "$D2_TOOLCHAIN/isolated_orchestrator.py" finalize-total-absence \
  --manifest "$MANIFEST" \
  --prefix-scan-evidence /absolute/prefix-scan.json \
  --guild-deletion-evidence /absolute/guild-deletion.json

python3 "$D2_TOOLCHAIN/d2_run.py" advance \
  --manifest "$MANIFEST" \
  --step 17 \
  --source "$FINAL/database-absence.json" \
  --source "$FINAL/orchestrator-total-absence.json" \
  --source /absolute/prefix-scan.json \
  --source /absolute/guild-deletion.json

umask 077
python3 "$D2_TOOLCHAIN/d2_run.py" verify \
  --manifest "$MANIFEST" > "$D2_RUN/final.json"
```

Observe zero resource-prefix matches after step 16, then delete the disposable
guild and save those two browser evidence files in that order. The low-level
`d2_certification.py record` command does not create coordinator intents or
completions and must not be used for a D3-bound release run.

## Certified browser steps 5-7

The browser driver is installed as `globalThis.StarringD2ProductDriver`. After
the operator completes OAuth on the exact D2 origin, create one driver. The
certified initial product flow is deliberately split across the coordinator's
step-6 completion boundary. Save only the returned evidence objects below as
absolute, owner-controlled mode-`0600` JSON files. The natural-language request
is used for the request only and is omitted from those objects.

Capture the idle worker boundary before authoring:

```text
python3 "$D2_TOOLCHAIN/isolated_orchestrator.py" worker-authoring-evidence \
  --manifest "$MANIFEST" \
  --checkpoint before
```

In the authenticated same-origin browser, begin authoring without promoting,
approving, or applying:

```js
const product = StarringD2ProductDriver.create()
const installationId = "installation:<run-owned-id>"
const sessionId = "d2-<run-owned-id>"
const authoring = await product.beginCertificationAuthoring({
  installationId,
  sessionId,
  message: "<reviewed one-shot study-room request>",
})
window.prompt(
  "Copy the exact step-5 browser authoring evidence JSON",
  JSON.stringify(authoring.authoring_evidence),
)
window.prompt(
  "Copy the exact step-6 browser preview evidence JSON",
  JSON.stringify(authoring.preview_ready_evidence),
)
```

Each standard browser prompt pauses before the next one and does not depend on
the optional DevTools `copy()` utility. Copy its complete default value without
editing it, then save the two objects separately as
`/absolute/step-05-browser-authoring.json` and
`/absolute/step-06-browser-preview-ready.json`. Capture the completed worker
boundary using the exact step-5 browser evidence, then inspect the encrypted
generation before making a product decision:

```text
python3 "$D2_TOOLCHAIN/isolated_orchestrator.py" worker-authoring-evidence \
  --manifest "$MANIFEST" \
  --checkpoint after \
  --browser-evidence /absolute/step-05-browser-authoring.json

umask 077
"$BUNDLE/starring-d2-sealed-provisioner" inspect \
  --manifest "$MANIFEST" \
  --checkpoint authoring \
  > /absolute/step-06-db-authoring.json
chmod 0600 /absolute/step-06-db-authoring.json

python3 "$D2_TOOLCHAIN/d2_run.py" advance \
  --manifest "$MANIFEST" \
  --step 5 \
  --source /absolute/step-05-browser-authoring.json \
  --source "$ORCH/worker-authoring/evidence.json"

python3 "$D2_TOOLCHAIN/d2_run.py" advance \
  --manifest "$MANIFEST" \
  --step 6 \
  --source /absolute/step-06-browser-preview-ready.json \
  --source /absolute/step-06-db-authoring.json

python3 "$D2_TOOLCHAIN/d2_run.py" status \
  --manifest "$MANIFEST"
```

The status result for step 7 contains
`preview_completion_challenge_sha256`. In the same browser driver, bind that
exact value into a fresh decision command, hash the command, and invoke the
certification-only decision helper. Do not pass `confirmPreview`; this helper
requires the native Chrome confirmation surface. Review and accept that prompt
before it approves and applies the target.

The confirmation deliberately shows three different SHA-256 identities. The
authoring `candidate_ruleset_hash`, promotion `payload_digest`, and registry
`target_content_hash` use different domains and MUST NOT be compared as if they
were interchangeable. Step 6 binds the candidate identity into the decision
command, step 7 binds all three identities into the visible Chrome confirmation,
and step 8 requires the durable live serving identity to retain the exact
reviewed registry target.

```js
const decisionCommand = product.createCertificationDecisionCommand({
  installationId,
  sessionId,
  authoringGeneration: authoring.authoring_evidence.authoring_generation,
  candidateRulesetHash:
    authoring.preview_ready_evidence.candidate_ruleset_hash,
  previewCompletionChallengeSha256:
    "<preview_completion_challenge_sha256 from status>",
})
const decisionCommandSha256 =
  await product.decisionCommandSha256(decisionCommand)
const decision = await product.completeCertificationDecision({
  command: decisionCommand,
  decisionCommandSha256,
})
window.prompt(
  "Copy the exact step-7 browser product-decision evidence JSON",
  JSON.stringify(decision.product_decision_evidence),
)
```

Save the copied object as
`/absolute/step-07-browser-product-decision.json`, mode `0600`, then advance
step 7:

```text
python3 "$D2_TOOLCHAIN/d2_run.py" advance \
  --manifest "$MANIFEST" \
  --step 7 \
  --source /absolute/step-07-browser-product-decision.json
```

`runOneShotProductFlow` is a non-certification convenience and must not be used
for certified steps 5-7. It crosses the required step-6 completion boundary and
does not return the challenge-bound coordinator source contracts. Its only D2
certification use is inside `runReplacementFlow` for the step 14 replacement.

When an update must drain the currently live runtime, the driver handles the
public `runtime_drain_required` and `runtime_drain_pending` conflicts as one
bounded Apply handshake. It retries only the exact Apply request with the same
payload digest, approved revision, and idempotency key. The default is 11 total
attempts with a two, four, eight, then capped fifteen-second delay, for at most
119 seconds of scheduled waiting. `runtimeDrainAttempts` accepts 1 through 180.
An explicit `runtimeDrainIntervalMilliseconds` uses a fixed delay and accepts
100 through 15000.

An already approved promotion can resume without repeating authoring,
promotion, preview, or approval:

```text
const applied = await product.applyWithDrainHandshake({
  installationId: "installation:<run-owned-id>",
  promotionId: "<promotion-id>",
  expectedPayloadDigest: "<reviewed-payload-digest>",
  expectedRevision: <approved-revision>,
  idempotencyKey: "<original-apply-idempotency-key>"
})
```
