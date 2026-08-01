# D2 disposable-guild certification

`d2_certification.py` owns the immutable run manifest and the ordered,
secret-free evidence ledger for the 17 D2 product E2E steps. It does not create
or delete a Discord guild, read a credential, start a process, mutate the
standing staging database, or claim that an incomplete run passed.

The preparation command binds one run to an exact Git commit, immutable API
and runtime binaries, the Codex worker and its executables, a unique resource
prefix, unique loopback ports, unique launchd labels, unique Keychain service
names, and an isolated PostgreSQL root. It also binds the fixed D2 Cloudflare
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
manifest-derived PostgreSQL root, four unloaded launchd plists, a tunnel runner,
and per-run Keychain ownership markers. Start launches PostgreSQL and invokes
the immutable `starring-d2-db-bootstrap` candidate, which reuses the production
SQLx bootstrap library and records the exact migration ledger. It then invokes
the immutable sealed provisioner, creates or exactly replays 20 application
database credentials, three application keyrings, one worker bearer credential,
activates the 20 application roles, changes PostgreSQL from socket-only
bootstrap access to role-specific loopback SCRAM access, verifies replay after
the sealed restart, and starts worker, API, runtime, and tunnel in that order.
Step 3 evidence binds the exact candidates and Cloudflare route tuple. Stop and
cleanup are idempotent, operate only on manifest-derived identities, and require
the standing launchd, plist, and port snapshot to remain unchanged.

The migration-ledger SHA-256 is reproducible. For each successful ledger row
ordered by version, hash the signed 64-bit version in big-endian form, one
success byte, the checksum length as an unsigned 64-bit big-endian value, and
the raw SQLx checksum bytes.

The current execution boundary is deliberate. `start` reports
`candidate_services_loaded=true` only after all four candidates are loaded and
their local and public health probes pass. A dedicated D2 Discord application,
guild, and credential pair remain mandatory because sharing the standing
gateway session would violate the no-staging-mutation contract. The fixed D2
Cloudflare tunnel token is an external read-only Keychain input. The
orchestrator never copies or deletes any of the three external credentials.

The remaining certification work is:

1. Place the dedicated D2 Discord OAuth client secret and bot token in the two
   manifest-pinned external Keychain identities. The fixed Cloudflare tunnel
   token must already exist in its third external identity.
2. Create the disposable Discord guild and dedicated D2 Discord application,
   then prepare and start the immutable candidate run.
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

Example preparation shape:

```text
python3 tools/d2-certification/d2_certification.py prepare \
  --output-root "$HOME/Library/Application Support/Starring/release-certifications" \
  --commit "$CANDIDATE_COMMIT" \
  --discord-guild-id "$DISPOSABLE_GUILD_ID" \
  --discord-application-id "$DISCORD_APPLICATION_ID" \
  --discord-bot-user-id "$DISCORD_BOT_USER_ID" \
  --discord-oauth-keychain starring.d2.credentials:discord.oauth-client-secret \
  --discord-bot-keychain starring.d2.credentials:discord.bot-token \
  --tunnel-token-keychain starring.d2.credentials:cloudflare.tunnel-token \
  --cloudflare-tunnel-id 57c22e8a-0ec2-4f67-a882-2c355b0348df \
  --public-origin https://d2-api.starring.co.kr \
  --candidate api=/absolute/immutable/starring-api \
  --candidate runtime=/absolute/immutable/starring-runtime \
  --candidate codex_worker=/absolute/repo/tools/codex-worker/worker.mjs \
  --candidate codex=/absolute/codex \
  --candidate db_bootstrap=/absolute/immutable/starring-d2-db-bootstrap \
  --candidate sealed_provisioner=/absolute/immutable/starring-d2-sealed-provisioner \
  --candidate node=/absolute/node \
  --candidate cloudflared=/absolute/cloudflared \
  --port postgres=55433 \
  --port api=28080 \
  --port runtime=29091 \
  --port worker=28181
```

Build both exact provisioning candidates from the candidate commit, then run
the substrate lifecycle with the immutable manifest:

```text
cargo build --locked --release -p starring-db-bootstrap \
  --bin starring-d2-db-bootstrap
cargo build --locked --release -p starring-staging-provisioner \
  --bin starring-d2-sealed-provisioner

python3 tools/d2-certification/isolated_orchestrator.py dry-run \
  --manifest /absolute/run/manifest.json
python3 tools/d2-certification/isolated_orchestrator.py prepare \
  --manifest /absolute/run/manifest.json
python3 tools/d2-certification/isolated_orchestrator.py start \
  --manifest /absolute/run/manifest.json
python3 tools/d2-certification/isolated_orchestrator.py onboard \
  --manifest /absolute/run/manifest.json \
  --principal-id discord:<authenticated-user-id> \
  --display-name <authenticated-display-name>
python3 tools/d2-certification/isolated_orchestrator.py stop \
  --manifest /absolute/run/manifest.json
python3 tools/d2-certification/isolated_orchestrator.py cleanup \
  --manifest /absolute/run/manifest.json
```

Every lifecycle operation takes a machine-wide nonblocking D2 lock. Mutation
intent and completion receipts are append-only and fsynced. Cleanup reconstructs
the owned root, labels, and Keychain accounts from the immutable manifest rather
than trusting the last state write, so it also recovers a prior interrupted run.

Record one reviewed JSON evidence object at a time, then verify the completed
ledger:

```text
python3 tools/d2-certification/d2_certification.py record \
  --manifest /absolute/run/manifest.json \
  --step 1 \
  --evidence /absolute/reviewed-step-1.json

python3 tools/d2-certification/d2_certification.py verify \
  --manifest /absolute/run/manifest.json
```

The browser driver is installed as `globalThis.StarringD2ProductDriver`. After
the operator completes OAuth on the exact D2 origin, create one driver and
explicitly invoke the product flow. The natural-language request is used for
the request only and is omitted from the returned result:

```text
const product = StarringD2ProductDriver.create()
const productEvidence = await product.runOneShotProductFlow({
  installationId: "installation:<run-owned-id>",
  sessionId: "d2-<run-owned-id>",
  message: "<reviewed one-shot study-room request>",
  confirmPreview: async preview => window.confirm(
    JSON.stringify(preview.summary, null, 2)
  )
})
```
