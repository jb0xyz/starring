# Continuous Integration Workflow — Design

## Goal

Add GitHub Actions CI that fixes the workspace's cross-crate safety invariants on
every push and pull request, without touching real Discord or the home LLM. This
is stage-1 close-out: it guards the engine before stage-2 harness work grows the
tool/API/UI surface, so an accidental bypass of an engine boundary is caught
immediately rather than late.

## What CI must protect

The invariants that already exist as tests or dependency guards, now enforced
automatically:

- No approval-less activation bypass (`interaction-smoke`/runtime never call
  `activate_if_ready` or `RuleSetStore::activate` directly — the activation-guard
  test).
- Pinned-version dispatch, readiness/activation gate identity, complete instance
  footprint, and the teardown/activation state machines (their unit + Postgres
  tests).
- Pure crates keep no `sqlx`/`twilight`/`ai-gateway` regular dependency (the
  per-crate `dependency_guard` tests).
- `unsafe-dev-activation` is absent from a default build (its guard test).
- PostgreSQL CAS, lease, and the partial unique index behave under contention
  (the ignored Postgres integration tests).

These are ordinary `cargo test`/guard tests already; CI runs them, it does not
add new ones.

## Grounded facts

- No existing `.github/workflows/`; this is new.
- Toolchain is pinned by `rust-toolchain.toml` (`channel = "stable"`); the CI
  toolchain step must respect it.
- The Postgres integration tests are `#[ignore]`, require
  `STARRING_TEST_DATABASE_URL`, **assert the URL contains `test`**, and **run
  `MIGRATOR.run` themselves** — so CI needs only a reachable empty Postgres and a
  database whose name contains `test`; no pre-migration step.
- Six packages carry ignored Postgres tests (not four): `automation-ruleset-postgres`,
  `automation-instance-postgres`, `automation-panel-installation-postgres`,
  `automation-ruleset-activation-postgres`, `automation-ruleset-dispatch`,
  `automation-ruleset-readiness`.

## Structure

One workflow file, `.github/workflows/ci.yml`, triggered on `push` and
`pull_request`. Two jobs.

### Job 1 — DB-less gate (default parallelism)

Runs on every push/PR, no services:

```
cargo fmt --all -- --check
cargo build --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p interaction-smoke --features unsafe-dev-activation
```

- `cargo test --workspace` keeps default (parallel) test threads. The whole
  workspace is **not** forced to `--test-threads=1` — that would only paper over
  a Postgres-test hygiene issue at the cost of slowing every unrelated test.
- The final step compiles the `unsafe-dev-activation` feature so the gated code
  keeps building, while the feature stays **off** in the normal test/build steps
  (its guard test asserts absence in the default build).
- The dependency guards, the activation-bypass guard, and the unsafe-dev-absent
  guard are ordinary tests, so they run inside `cargo test --workspace` here — no
  extra wiring.

### Job 2 — PostgreSQL integration gate (serial ignored)

A separate job with a Postgres service container. It runs only the ignored
integration tests, serially, per package (explicit list, so no unrelated ignored
test is ever swept in and made to reach an external service):

```
STARRING_TEST_DATABASE_URL=postgres://postgres:postgres@localhost:5432/starring_test

cargo test -p automation-ruleset-postgres              -- --ignored --test-threads=1
cargo test -p automation-instance-postgres             -- --ignored --test-threads=1
cargo test -p automation-panel-installation-postgres   -- --ignored --test-threads=1
cargo test -p automation-ruleset-activation-postgres   -- --ignored --test-threads=1
cargo test -p automation-ruleset-dispatch              -- --ignored --test-threads=1
cargo test -p automation-ruleset-readiness             -- --ignored --test-threads=1
```

- The database name (`starring_test`) contains `test`, satisfying the harness
  guard. The tests self-migrate, so the job only needs the service up and the
  database created.
- `--test-threads=1` per package avoids the known cross-test contention in the
  panel-installation Postgres tests (shared guild constant) — serializing the
  ignored suite is the correct fix, and it is confined to this job.
- Explicit package list rather than `--workspace -- --ignored`: it guarantees CI
  never runs a future ignored test that expects Discord or the home LLM.

### Shared setup

- Checkout, install the toolchain honoring `rust-toolchain.toml` with `rustfmt`
  and `clippy` components, and cache the cargo registry + build (`Swatinem/rust-cache`
  or the equivalent actions cache) so runs stay fast.
- Postgres service (Job 2) via the standard `services:` container with a health
  check; create `starring_test` before the test steps.

## Explicitly excluded from CI

Live verification stays a manual runbook, never CI:

```
Discord bot login, real guild mutation, real panel installation,
real pinned interaction, real teardown, home LLM access.
```

Wiring CI secrets to a live guild would add flakiness and mutate real resources.
CI proves the code contract; the runbooks prove the live lifecycle.

## Completion criteria

- Runs automatically on `main` push and on pull requests.
- Job 1 (fmt, build, test, clippy, feature build) passes with default parallelism.
- Job 2 (six Postgres packages, ignored, serial) passes against the service DB.
- No real Discord / external LLM access; no secrets required.
- `unsafe-dev-activation` stays off by default and still compiles.
- The dependency and direct-call guards run and pass.

## Roadmap

Next: rewrite `CURRENT_STATE.md` against the real 30-member workspace and the
current lifecycle (the docs drift noted in the harness-direction memory), then
begin stage-2 harness brainstorming scoped to a Discord automation designer.
