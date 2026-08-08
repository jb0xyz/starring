# D3 exact-tree certification

This tool binds the backend-v1 release gates, the complete 17-step D2 receipt chain, the reviewed pull-request merge candidate, the merged `main` tree, and successful post-merge GitHub Actions runs to one fail-closed record.

It never persists gate command text, process output, environment values, or credentials. State directories must be absolute, owned by the current user, mode `0700`, and free of symlink traversal. Evidence files are mode `0600`, locked, chained, flushed, and directory-synced.

## Sequence

Prepare a state directory from the current GitHub-generated merge ref. `origin` must be a credential-free `github.com` HTTPS or SSH URL. Git URL rewrites, alternate push URLs, custom upload/receive commands, proxies, SSH commands, TLS overrides, and curl address overrides are rejected. The gate manifest is fixed in code and accepts no missing, additional, duplicate, reordered, or changed command.

```zsh
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
  --output-root /private/tmp/starring-d3
  --pr-number 30
  --expected-head HEAD_SHA
  --expected-base BASE_SHA
)
for gate in "${gates[@]}"; do
  prepare+=(--gate "$gate")
done
"${prepare[@]}"
```

`run-gates` executes the 29 commands above, in order, in the detached
worktree. Candidate publication does not begin until all 29 pass. Completed
gates replay without execution. A failed or interrupted gate resumes as a new
or incomplete durable attempt.

The PostgreSQL URL file is a dedicated absolute mode-`0600` secret input. It
must name an explicit loopback port and a `starring_test` or `starring_d3*`
database with credentials. Its value is injected only into commands 17 through
29, is never added to the command manifest or evidence, and all gate processes
run with `CARGO_INCREMENTAL=0`.

```zsh
run_gates=(
  python3 tools/d3-certification/d3_certification.py run-gates
  --state /private/tmp/starring-d3/RUN/state.json
  --postgres-database-url-file /absolute/private/postgres-database-url
)
for gate in "${gates[@]}"; do
  run_gates+=(--gate "$gate")
done
"${run_gates[@]}"
```

## Sealed candidate publication

After gate completion, `run-gates` builds five Rust release binaries under
`<D3_STATE>/candidate-build`. The build uses separate workspace and transport
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

Before building, the tool durably writes mode-`0600`
`candidate-bundle-intent.json`. The sealed intent binds the merge commit and
tree, gate-chain head, build recipe and lockfiles, exact source trees,
toolchain identities, isolated directories, and one staging nonce and path. A
retry must match that intent exactly.

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
| `--candidate api=...` | `<D3_STATE>/candidate-bundle/starring-api` |
| `--candidate runtime=...` | `<D3_STATE>/candidate-bundle/starring-runtime` |
| `--candidate db_bootstrap=...` | `<D3_STATE>/candidate-bundle/starring-d2-db-bootstrap` |
| `--candidate sealed_provisioner=...` | `<D3_STATE>/candidate-bundle/starring-d2-sealed-provisioner` |
| `--candidate certification_transport=...` | `<D3_STATE>/candidate-bundle/d2-certification-transport` |
| `--candidate codex_worker=...` | `<D3_STATE>/candidate-bundle/codex-worker/worker.mjs` |

`node`, `codex`, and `cloudflared` are not bundle artifacts. Continue to pass
their operator-supplied installed executable paths to D2 preparation; D3 does
not build them or bind them to `candidate-bundle`.

`bind-d2` rejects a D2 manifest unless `commit_sha` is the exact pinned merge
commit and resolves to the pinned merge tree. It also requires the exact path
and SHA-256 for all five sealed binaries and `codex-worker/worker.mjs`. The
manifest source-tree identities must match the sealed seven-file worker tree
and the fixed D2 toolchain and certification-transport inventories in the
detached worktree. A different commit is rejected even when its tree is equal.

Capture the canonical output of `d2_run.py verify` in an owned mode-`0600` JSON file, then bind its required 17-step coordinator ledger to the D2 manifest, all 17 chained receipts, and the pinned commit tree. Set `umask 077` before redirecting the verifier output so the record is never created with a permissive mode. The low-level receipt verifier is not release authority.

```sh
python3 tools/d3-certification/d3_certification.py bind-d2 \
  --state /private/tmp/starring-d3/RUN/state.json \
  --d2-manifest /absolute/d2/run/manifest.json \
  --d2-final-record /absolute/d2/run/final.json
```

Re-fetch the PR head, base, and generated merge ref immediately before merge. This command requires the gates and D2 binding to be complete.

```sh
python3 tools/d3-certification/d3_certification.py recheck \
  --state /private/tmp/starring-d3/RUN/state.json
```

After an independent merge, bind the fetched `origin/main` tree, the closed and merged frozen pull request, and one successful post-merge Actions run. The PR's repository, base ref and SHA, head SHA, and merge commit must match the pinned state and fetched `main` exactly. The repository argument must exactly equal the owner/repository identity pinned from `origin`. The GitHub CLI must already be authenticated without inline credentials.

```sh
python3 tools/d3-certification/d3_certification.py finalize \
  --state /private/tmp/starring-d3/RUN/state.json \
  --github-repository jb0xyz/starring \
  --actions-run-id RUN_ID
```

`final.json` is the terminal D3 record. A passing record requires the frozen PR to be closed and merged into the fetched `main` commit, exact merge-tree equality on `main`, the complete canonical gate evidence chain, the same D2 receipt chain observed during pre-merge recheck, and successful `checks` and `postgres` jobs in the exact `CI` push workflow run against that commit.
