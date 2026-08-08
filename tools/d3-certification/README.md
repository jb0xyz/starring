# D3 exact-tree certification

This tool binds the backend-v1 release gates, the complete 17-step D2 receipt chain, the reviewed pull-request merge candidate, the merged `main` tree, and successful post-merge GitHub Actions runs to one fail-closed record.

It never persists gate command text, process output, environment values, or credentials. State directories must be absolute, owned by the current user, mode `0700`, and free of symlink traversal. Evidence files are mode `0600`, locked, chained, flushed, and directory-synced.

The production state parent is
`$HOME/Library/Application Support/Starring/d3-certifications`. Create it as an
owned mode-`0700` directory and retain it through finalization. `prepare`
creates a deterministic `d3-pr-<number>-<digest>` child beneath that parent;
all later commands use the returned `state` path rather than reconstructing the
name.

The supported gate runner is the fixed Homebrew Docker client at
`/opt/homebrew/bin/docker` connected only to the current user's mode-`0600`
Colima socket at `~/.colima/default/docker.sock`. Colima must run Linux arm64
with 18 GiB assigned; the runtime probe rejects a daemon exposing less than 17
GiB. The gate image is built from the digest-pinned Rust and Node base images
in `Dockerfile.gates`, must report arm64, Rust 1.97.0 and Node 26.5.0, and is
bound into the certification record by its image ID, Dockerfile digest,
implementation digest, and runner-policy digest.

## Sequence

Prepare a state directory from the current GitHub-generated merge ref. `origin` must be a credential-free `github.com` HTTPS or SSH URL. Git URL rewrites, alternate push URLs, custom upload/receive commands, proxies, SSH commands, TLS overrides, and curl address overrides are rejected. The gate manifest is fixed in code and accepts no missing, additional, duplicate, reordered, or changed command.

```zsh
D3_OUTPUT_ROOT="$HOME/Library/Application Support/Starring/d3-certifications"
install -d -m 0700 "$D3_OUTPUT_ROOT"
gates=(
  'cargo fmt --all -- --check'
  'cargo build --locked --workspace --all-targets'
  'cargo test --locked --workspace'
  'cargo clippy --locked --workspace --all-targets -- -D warnings'
  'cargo build --locked -p interaction-smoke --features unsafe-dev-activation'
  'npm --prefix tools/codex-worker run check'
  'npm --prefix tools/codex-worker test'
  'npm --prefix eval/codex-worker-slo run check'
  'npm --prefix eval/design-harness ci'
  'npm --prefix eval/design-harness run audit'
  'npm --prefix eval/design-harness run check'
  "python3 -m unittest discover -s tools/d2-certification -p 'test_*.py'"
  'node --test tools/d2-certification/product_driver.test.mjs'
  'cargo fmt --manifest-path tools/d2-certification-transport/Cargo.toml -- --check'
  'cargo test --locked --manifest-path tools/d2-certification-transport/Cargo.toml'
  'cargo clippy --locked --manifest-path tools/d2-certification-transport/Cargo.toml --all-targets -- -D warnings'
  'cargo test --locked -p automation-ruleset-postgres -- --ignored --test-threads=1'
  'cargo test --locked -p automation-instance-postgres -- --ignored --test-threads=1'
  'cargo test --locked -p automation-panel-installation-postgres -- --ignored --test-threads=1'
  'cargo test --locked -p automation-ruleset-activation-postgres -- --ignored --test-threads=1'
  'cargo test --locked -p authoring-promotion-postgres -- --ignored --test-threads=1'
  'cargo test --locked -p authoring-application-postgres -- --ignored --test-threads=1'
  'cargo test --locked -p automation-ruleset-dispatch -- --ignored --test-threads=1'
  'cargo test --locked -p automation-ruleset-readiness -- --ignored --test-threads=1'
  'cargo test --locked -p automation-runtime-convergence-postgres -- --ignored --test-threads=1'
  'cargo test --locked -p automation-runtime-execution-postgres --test postgres_security -- --ignored --test-threads=1'
  'cargo test --locked -p automation-runtime-serving-postgres -- --ignored --test-threads=1'
  'cargo test --locked -p automation-runtime-interaction-postgres -- --ignored --test-threads=1'
  'cargo test --locked -p automation-runtime-panel-postgres -- --ignored --test-threads=1'
)
prepare=(
  python3 tools/d3-certification/d3_certification.py prepare
  --repo /absolute/path/to/starring
  --output-root "$D3_OUTPUT_ROOT"
  --pr-number 30
  --expected-head HEAD_SHA
  --expected-base BASE_SHA
)
for gate in "${gates[@]}"; do
  prepare+=(--gate "$gate")
done
"${prepare[@]}"
```

Set `D3_STATE` to the absolute `state` value returned by `prepare`. The run
root referred to below as `<D3_RUN>` is the directory containing that
`state.json` file.

`run-gates` executes the 29 commands above, in order, in the detached
worktree. Candidate publication does not begin until all 29 pass. Completed
gates replay without execution. A failed or interrupted gate resumes as a new
or incomplete durable attempt.

Before the first attempt, `run-gates` creates a bounded bootstrap while network
access is available. Both Cargo lockfiles are first checked in a networkless
container against the exact crates.io source and the single pinned Twilight Git
revision. The workspace vendor is then materialized from the exact
checksum-bearing crates.io packages in `Cargo.lock`; the transport vendor owns
the pinned Git source. The generated workspace Cargo configuration maps
crates.io and Twilight to those two distinct directories, while the transport
configuration maps its complete graph to the transport vendor. Both mappings
are verified with offline Cargo metadata. npm packages are accepted only from
the registry named in the checked lockfile. Temporary homes, targets, package
files, and staging configuration are removed before the exact bootstrap
inventory is sealed read-only and tree-hashed. Bootstrap staging is capped at 4
GiB and 500,000 entries and requires at least 8 GiB host free space.

Each gate attempt gets a new non-root container with a read-only root
filesystem, all capabilities dropped, `no-new-privileges`, no Docker socket,
2,048 PIDs, four CPUs, a 14 GiB memory limit, and the same 14 GiB memory-plus-
swap limit so no additional container swap is available. Scratch is bounded to
3 GiB and each Cargo attempt owns a disposable 8 GiB tmpfs target volume that
is removed with the attempt. Cargo uses one build job, no incremental state,
and `debug=0` for dev and test artifacts; debug assertions remain enabled.
Docker's OOM state and the child cgroup `memory.events` OOM counter are checked
separately, so an OOM in a descendant cannot be mistaken for an ordinary tool
exit. Gates are offline except the explicitly networked npm audit in gate 10.

The PostgreSQL URL file remains a dedicated absolute mode-`0600` policy input.
It must name an explicit loopback port and a `starring_test` or `starring_d3*`
database with credentials, but those credentials and that server are not used
by a gate. For each of gates 17 through 29, the runner derives only the approved
database name, generates a fresh random password, and starts a digest-pinned,
read-only PostgreSQL container with tmpfs storage, no published ports, and no
host mounts. The gate container joins only that PostgreSQL container's network
namespace. The supplied value, generated credentials, and effective URL are
never added to the command manifest or evidence.

```zsh
run_gates=(
  python3 tools/d3-certification/d3_certification.py run-gates
  --state "$D3_STATE"
  --postgres-database-url-file /absolute/private/postgres-database-url
)
for gate in "${gates[@]}"; do
  run_gates+=(--gate "$gate")
done
"${run_gates[@]}"
```

## Sealed candidate publication

After gate completion, `run-gates` builds five Rust release binaries under
`<D3_RUN>/candidate-build`. The build uses separate workspace and transport
target directories and dedicated mode-`0700` `CARGO_HOME`, `HOME`,
`XDG_CACHE_HOME`, `XDG_CONFIG_HOME`, and `TMPDIR` directories. It seals the
binaries together with an exact seven-file snapshot of
`tools/codex-worker`:

```text
candidate-bundle/                         0555
  bundle.json                             0400
  publication.json                        0400
  starring-api                            0555
  starring-runtime                        0555
  starring-d2-db-bootstrap                0555
  starring-d2-sealed-provisioner          0555
  d2-certification-transport              0555
  codex-worker/                           0555
    admission-registry.mjs                0444
    codex-runner.mjs                      0444
    metrics-log.mjs                       0444
    protocol.mjs                          0444
    request-timeline.mjs                  0444
    scheduler.mjs                         0444
    worker.mjs                            0444
```

The five binaries are native `aarch64-apple-darwin` artifacts. Before building,
the tool durably writes mode-`0600`
`candidate-bundle-intent.json`. The sealed intent binds the merge commit and
tree, gate-chain head, build recipe and lockfiles, exact source trees,
toolchain identities, isolated directories, and one staging nonce and path. A
retry must match that intent exactly.

Candidate Cargo commands are `--frozen`, offline, and use only the sealed
workspace or transport vendor configuration. The native Rust and Apple linker
tools, developer directory, environment, lockfiles, and source trees are
identity-bound before and after the build. `CARGO_BUILD_JOBS=1`; the mutable
build tree is capped at 4 GiB and 500,000 entries, must retain at least 2 GiB
free, and must originate on a filesystem with at least 8 GiB available.

Every native build command runs without network access under a Seatbelt profile
that permits writes only below its isolated build root. It is launched by a
unique, deterministic per-command launchd bootstrap job. That job applies an
8 GiB data/resident-set ceiling, a 4 GiB output-file ceiling, a two-hour CPU
ceiling, and bounded process and descriptor counts. The tool does not accept
success until it has durably read the job result and booted out the whole
launchd coalition, including descendants that changed session or closed file
descriptors. The persistent `candidate-build.lock`, launchd plist, deterministic
job label, and exclusive result file make crash behavior explicit: a retry
rejects an active prior job, consumes a completed durable result, and fails
closed when service and result state are ambiguous. Sealing occurs only after
the build fence has been closed and exclusively reacquired.

Publication is crash-recoverable and exclusive. An empty pre-publication
staging directory is removed. An intent-owned mode-`0700` staging directory
with a valid publication identity is journaled, discarded, and rebuilt. An
intent-owned mode-`0555` staging directory is fully verified and published as
`candidate-bundle`. Foreign or ambiguous staging, an unjournaled discard, and
identity drift fail closed. A valid final bundle, including one recovered from
sealed staging, returns `candidate_bundle_disposition=exact_replay` without
rebuilding; a fresh build and publication returns `created`.

## D2 preparation and binding

D2 does not discover the bundle. After `run-gates`, prepare D2 with the exact
GitHub-generated merge commit pinned in D3 `state.json` and these bundle
paths:

| D2 preparation field | Required value |
| --- | --- |
| `--commit` | D3 `merge_commit` |
| `--candidate api=...` | `<D3_RUN>/candidate-bundle/starring-api` |
| `--candidate runtime=...` | `<D3_RUN>/candidate-bundle/starring-runtime` |
| `--candidate db_bootstrap=...` | `<D3_RUN>/candidate-bundle/starring-d2-db-bootstrap` |
| `--candidate sealed_provisioner=...` | `<D3_RUN>/candidate-bundle/starring-d2-sealed-provisioner` |
| `--candidate certification_transport=...` | `<D3_RUN>/candidate-bundle/d2-certification-transport` |
| `--candidate codex_worker=...` | `<D3_RUN>/candidate-bundle/codex-worker/worker.mjs` |

`node`, `codex`, and `cloudflared` are not bundle artifacts. Continue to pass
their operator-supplied installed executable paths to D2 preparation; D3 does
not build them or bind them to `candidate-bundle`.

`bind-d2` rejects a D2 manifest unless `commit_sha` is the exact pinned merge
commit and resolves to the pinned merge tree. It also requires the exact path
and SHA-256 for all five sealed binaries and `codex-worker/worker.mjs`. The
manifest source-tree identities must match the sealed seven-file worker tree
and the fixed D2 toolchain and certification-transport inventories in the
detached worktree. A different commit is rejected even when its tree is equal.
`bind-d2` also pins the canonical D2 manifest path and the device and inode of
both its run directory and `orchestrator` directory. Keep that evidence tree
in place through `recheck` and `finalize`. All three commands, including exact
replays, reject a run containing `candidate-start-retirement.json` or the
standalone teardown tombstone `discord-resource-teardown-abort.json`.

Capture the canonical output of `d2_run.py verify` in an owned mode-`0600` JSON file, then bind its required 17-step coordinator ledger to the D2 manifest, all 17 chained receipts, and the pinned commit tree. Set `umask 077` before redirecting the verifier output so the record is never created with a permissive mode. The low-level receipt verifier is not release authority.

```sh
python3 tools/d3-certification/d3_certification.py bind-d2 \
  --state "$D3_STATE" \
  --d2-manifest /absolute/d2/run/manifest.json \
  --d2-final-record /absolute/d2/run/final.json
```

Re-fetch the PR head, base, and generated merge ref immediately before merge. This command requires the gates and D2 binding to be complete.

```sh
python3 tools/d3-certification/d3_certification.py recheck \
  --state "$D3_STATE"
```

After an independent merge, bind the fetched `origin/main` tree, the closed and merged frozen pull request, and one successful post-merge Actions run. The PR's repository, base ref and SHA, head SHA, and merge commit must match the pinned state and fetched `main` exactly. The repository argument must exactly equal the owner/repository identity pinned from `origin`. The GitHub CLI must already be authenticated without inline credentials.

```sh
python3 tools/d3-certification/d3_certification.py finalize \
  --state "$D3_STATE" \
  --github-repository jb0xyz/starring \
  --actions-run-id RUN_ID
```

`final.json` is the terminal D3 record. A passing record requires the frozen PR to be closed and merged into the fetched `main` commit, exact merge-tree equality on `main`, the complete canonical gate evidence chain, the same D2 receipt chain observed during pre-merge recheck, and successful `checks` and `postgres` jobs in the exact `CI` push workflow run against that commit.

After creating or exactly replaying a valid terminal record, `finalize` retires
`candidate-build` through the device, inode, owner, and mode sealed in the
intent. Removal is descriptor-relative and fails closed on replacement; the
sealed candidate bundle and all certification evidence remain. Replaying
`finalize` also completes an interrupted retirement.

## Boundaries and limitations

The bootstrap needs registry network access only when it is first created, and
gate 10 intentionally reaches the npm registry for its live audit. Registry
availability and the content addressed by the lockfiles therefore remain
external inputs until the bootstrap is sealed. No gate stdout or stderr is
retained, so the evidence proves command identity, result, duration, runner,
and chain continuity rather than serving as a diagnostic log.

The candidate build is intentionally native to the certified Apple Silicon Mac
and its identity-bound local Rust and Command Line Tools installation. Seatbelt
and a launchd coalition constrain that native build, but they are not a second
virtual machine and do not make artifacts portable across different host
toolchains. D3 also does not replace D2: the D2 evidence directories and live
authority boundary must remain unchanged through `recheck` and `finalize`, and
D3 will not discover, recreate, or repair those external resources.
