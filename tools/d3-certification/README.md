# D3 exact-tree certification

This tool binds the backend-v1 release gates, the complete 17-step D2 receipt chain, the reviewed pull-request merge candidate, the merged `main` tree, and successful post-merge GitHub Actions runs to one fail-closed record.

It never persists gate command text, process output, environment values, or credentials. State directories must be absolute, owned by the current user, mode `0700`, and free of symlink traversal. Evidence files are mode `0600`, locked, chained, flushed, and directory-synced.

## Sequence

Prepare a state directory from the current GitHub-generated merge ref. Every gate command is supplied here so its digest is immutable before execution.

```sh
python3 tools/d3-certification/d3_certification.py prepare \
  --repo /absolute/path/to/starring \
  --output-root /private/tmp/starring-d3 \
  --pr-number 30 \
  --expected-head HEAD_SHA \
  --expected-base BASE_SHA \
  --gate 'cargo test --workspace' \
  --gate 'cargo clippy --workspace --all-targets -- -D warnings' \
  --gate 'cargo fmt --all -- --check'
```

Run the exact pinned gates in the detached worktree. Completed gates replay without execution. A failed or interrupted gate resumes as a new or incomplete durable attempt.

```sh
python3 tools/d3-certification/d3_certification.py run-gates \
  --state /private/tmp/starring-d3/RUN/state.json \
  --gate 'cargo test --workspace' \
  --gate 'cargo clippy --workspace --all-targets -- -D warnings' \
  --gate 'cargo fmt --all -- --check'
```

Capture the canonical output of `d2_certification.py verify` in an owned mode-`0600` JSON file, then bind it to the D2 manifest, all 17 chained receipts, and the pinned commit tree. Set `umask 077` before redirecting the verifier output so the record is never created with a permissive mode.

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

After an independent merge, bind the fetched `origin/main` tree and successful post-merge Actions run IDs. The GitHub CLI must already be authenticated without inline credentials.

```sh
python3 tools/d3-certification/d3_certification.py finalize \
  --state /private/tmp/starring-d3/RUN/state.json \
  --github-repository jb0xyz/starring \
  --actions-run-id RUN_ID
```

`final.json` is the terminal D3 record. A passing record requires exact merge-tree equality on `main`, a complete gate evidence chain, the same D2 receipt chain observed during pre-merge recheck, and every named Actions run to be completed successfully against the fetched `main` commit.
