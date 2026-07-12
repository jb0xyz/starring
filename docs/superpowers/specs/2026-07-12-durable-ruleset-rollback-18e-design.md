# Phase 18e: Durable RuleSet Rollback Live — Design

## Goal

Phase 18e adds no new lifecycle feature. Using a unique RuleSet key and two
observably different fixture variants, it certifies **live** that publish, gated
activation, hydration, version pin, pinned dispatch, panel reconciliation,
rollback, restart recovery, and teardown operate as one PostgreSQL-backed
lifecycle. Concretely:

> After activating RuleSet v2 and rolling back to v1, each `AutomationInstance`
> keeps dispatching with the immutable RuleSet version pinned at its creation,
> while a newly created instance pins the current active version. The active
> pointer, RuleSet artifacts, instance pins, and panel installation state are all
> restored from PostgreSQL after a process restart.

## Nature and Non-Goals

18e is an **integration certification stage**, not an engine phase. The
deliverable is evidence (a runbook), backed by a small tool affordance that lets
one seed two observably different RuleSet versions in an isolated registry key.

Explicitly OUT of scope (each is a separate later step):

- activation ↔ approval binding
- production API
- CI (fmt/clippy/test) automation
- `CURRENT_STATE.md` refresh
- registry / version model redesign
- the `InteractionRuleSet.version` field debt cleanup

## Code Scope Guard

18e's character is preserved only if the engine stays untouched.

```
Allowed to modify:
  tools/interaction-smoke/
  docs/superpowers/specs/2026-07-12-durable-ruleset-rollback-18e-design.md
  docs/superpowers/runbooks/2026-07-12-durable-ruleset-rollback-18e.md

Forbidden to modify:
  crates/automation-core/
  crates/automation-runtime/
  crates/automation-ruleset*/          (ruleset, ruleset-postgres, ruleset-dispatch)
  crates/automation-ruleset-readiness/
  crates/automation-instance*/          (instance, instance-postgres, instance-teardown)
  crates/automation-panel-installation*/
  migrations/
```

If the tool cannot be built without an engine change, that is a **separate
defect** to raise and scope on its own — it must not be silently fixed inside
18e. The whole point of 18e is that the engine built across 18a–18d is already
sufficient.

## Context

The Durable RuleSet Lifecycle arc:

```
18a Registry core (immutable, content-addressed, monotonic versions)
18b PostgreSQL RuleSet store (durable versions / activations / heads)
18c-1 Active hydration + readiness gate (boot loads active, fail-closed)
18c-2 Instance version pin (instance records the version it was born with)
18c-3 Pinned dispatch (InstanceAction runs the instance's pinned version, per click)
18c-4 Gated activation (activate_if_ready shares the hydration gate)
18d-1 Durable panel installation (reconcile declared panels to PanelSpec)
18d-2 Preallocated identity (complete instance footprint)
18d-3 Durable instance teardown (Active → Deleting → Deleted)
18e Live rollback certification (this document)
```

Every capability the rollback story needs already exists. Verified while
scoping 18e:

- The tool already parses the RuleSet key **once** (`main.rs:63`) and threads a
  single `RuleSetKey` into `seed_studyroom`, `activate`, and the `run` path. The
  `run` path derives everything from the hydrated `runtime`: the panel
  installation key (`runtime.ruleset_key`), the `RunningRuleSetIdentity`
  (`main.rs:187`), the static custom_id (via `identity.key`), and the pinned
  dispatcher (which reads each instance's stored key). No operational path
  references the `RULESET_KEY` constant except that single parse site.
- A single running gateway serves both pinned versions at once: `make` (static)
  uses the hydrated active version; `join`/`close` (InstanceAction) are
  dispatched by loading the instance's pinned version fresh from Postgres per
  click (18c-3). The gateway never needs to "run as v1" or "run as v2".
- The gateway hydrates the active version once at boot; it does not hot-reload.
  A new active version therefore takes effect for `make` and the declared panel
  only after a restart. This is the correct durable model and the reason the
  scenario restarts after each activation.

## Global Constraints

- No code comments anywhere (`//`, `///`, `//!`).
- **Engine untouched** (see Code Scope Guard). Only `interaction-smoke` and the
  18e docs change.
- Fail-closed CLI: ambiguous or malformed invocation is rejected, never guessed.
- The tool is a development smoke runner; favor **explicitness** over backward
  compatibility for the new flags.

## Tool Changes (interaction-smoke only)

Four changes, all in `tools/interaction-smoke/src/main.rs`, plus tests.

### 1. CLI parsing — fail-closed, position-independent

Today the tool assumes positions: `mode = args().nth(1)`, activate version =
`args().nth(2)`, and scans `--activate`/`--force-activate` with `any`. A
top-level `--ruleset-key <value>` breaks those position assumptions, so parsing
is restructured into a **pure function over the argument slice** that extracts
flags in any position and leaves ordered positionals.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FixtureVariant {
    V1,
    V2,
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Seed {
        variant: FixtureVariant,
        activate: bool,
        force_activate: bool,
    },
    Activate {
        version: u32,
    },
    Run,
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedCli {
    ruleset_key: Option<String>,
    command: Command,
}

#[derive(Debug, PartialEq, Eq)]
enum CliError {
    MissingRulesetKeyValue,
    DuplicateRulesetKey,
    MissingVariantValue,
    DuplicateVariant,
    InvalidVariant(String),
    UnknownFlag(String),
    MissingVariantForSeed,
    VariantNotAllowed,
    ActivateFlagNotAllowed,
    MissingActivateVersion,
    InvalidActivateVersion(String),
    UnknownMode(String),
    UnexpectedPositional(String),
    InvalidRulesetKey(String),
}

fn parse_cli(args: &[String]) -> Result<ParsedCli, CliError>;
```

`parse_cli` contract (fail-closed):

- Value flags `--ruleset-key <v>` and `--variant <v>` each require a following
  value token (else `MissingRulesetKeyValue` / `MissingVariantValue`) and reject
  a second occurrence (`DuplicateRulesetKey` / `DuplicateVariant`).
- `--variant` value must be `v1` or `v2` (case-sensitive), else
  `InvalidVariant(value)`.
- Boolean flags `--activate` / `--force-activate` are seed-only. Structurally
  they live inside `Command::Seed`, so their presence on any non-seed mode is an
  error (`ActivateFlagNotAllowed`).
- Any other `--token` is `UnknownFlag(token)`.
- Remaining positionals, in order, are `[mode, extra...]`. `mode` defaults to
  `"run"` when there is no positional.
  - `"seed-studyroom"` requires `--variant` (`MissingVariantForSeed` if absent),
    carries the boolean flags, and rejects a second positional
    (`UnexpectedPositional`).
  - `"activate"` requires exactly one further positional parsed as `u32`
    (`MissingActivateVersion` / `InvalidActivateVersion`), and rejects
    `--variant` (`VariantNotAllowed`) and the activate flags
    (`ActivateFlagNotAllowed`).
  - `"run"` accepts no variant, no version, and no activate flags; any of these
    is the corresponding error.
  - Any other mode is `UnknownMode(mode)`.

`main` builds the argument slice from `std::env::args().skip(1)`, calls
`parse_cli`, prints a usage line on `CliError`, and exits non-zero.

### 2. RuleSet key resolution — pure `resolve_ruleset_key`

Resolution precedence is a pure function so its behavior is tested without
touching process env in parallel tests:

```rust
const DEFAULT_RULESET_KEY: &str = "studyroom_demo";

fn resolve_ruleset_key(
    cli_value: Option<&str>,
    env_value: Option<&str>,
) -> Result<RuleSetKey, CliError> {
    let raw = cli_value.or(env_value).unwrap_or(DEFAULT_RULESET_KEY);
    RuleSetKey::parse(raw).map_err(|error| CliError::InvalidRulesetKey(format!("{error:?}")))
}
```

`main` performs the only `std::env::var("STARRING_RULESET_KEY")` lookup, once, at
the boundary, and passes the resolved `RuleSetKey` into every downstream path
exactly as the tool already does. The existing single parse site
(`main.rs:63`) is replaced by this resolution; the `RULESET_KEY` constant is
renamed `DEFAULT_RULESET_KEY` and survives **only** as the fallback default.

Precedence:

```
--ruleset-key <v>            (CLI, highest)
  else STARRING_RULESET_KEY  (env)
  else studyroom_demo        (default)
```

Because the key is resolved once and threaded, all key-bearing paths use the
same value automatically:

```
RuleSet publish key
active pointer lookup key
activate target key
hydrate_active_ruleset key
RunningRuleSetIdentity.key
declared panel installation key (guild, ruleset_key, panel_key)
static button custom_id ruleset key
new instance ruleset_key (stored at RegisterInstance)
```

Because resolution happens once and the resolved `RuleSetKey` is threaded, no
operational path can hardcode `studyroom_demo` — it survives only as
`DEFAULT_RULESET_KEY`. This is a structural guarantee of the single-resolution
design; an optional mechanical source-scan test may assert `studyroom_demo`
appears only on the `DEFAULT_RULESET_KEY` line (`#[cfg(test)]` uses exempt).

### 3. Fixture variant — `studyroom_ruleset(variant)`

`studyroom_ruleset()` becomes `studyroom_ruleset(variant: FixtureVariant)`. The
two variants differ **only** in two presentation strings; every structural
element — the internal `version: 1` field, the modal, the create/submit rule and
its full instance manifest (role, channel, welcome panel, hub panel,
`RegisterInstance`), and the close rule — is byte-for-byte identical.

```rust
fn studyroom_ruleset(variant: FixtureVariant) -> InteractionRuleSet {
    let (create_panel_content, join_response) = match variant {
        FixtureVariant::V1 => ("스터디룸 만들기 · v1", "스터디룸에 참가했습니다. [v1]"),
        FixtureVariant::V2 => (
            "스터디룸 만들기 · v2",
            "스터디룸 참가가 완료되었습니다. [v2]",
        ),
    };
    ...
}
```

The existing `studyroom_ruleset()` body is kept verbatim with exactly two
substitutions: `panels[0].content` (the `study_panel` panel) becomes
`create_panel_content.to_string()`, and the join rule's terminal
`ActionSpec::EditResponse { content }` becomes `join_response.to_string()`. The
internal `version: 1` field, the modal, the create/submit rule, the welcome and
hub panels, the `RegisterInstance` manifest, and the close rule are unchanged.

- **Difference point 1** — the declared create panel: `panels[0].content` (the
  panel with `key: "study_panel"`, currently `"Create a study room"`). Changing
  it changes `spec_hash(render_revision, panel)` (18d-1), so `run` reconciles the
  installed panel across versions (`Edited`).
- **Difference point 2** — the join response: the `InteractionRule` whose trigger
  is `TriggerSpec::InstanceAction { action: "join" }`, its terminal
  `ActionSpec::EditResponse { content }` (currently
  `"스터디룸에 참가했습니다."`). It is executed via pinned dispatch, so an
  existing v1 room and a new v2 room return different text from the same button.

The internal `InteractionRuleSet.version` is **1 in both variants** and must not
be used to distinguish them — it is a different concept from the registry
`RuleSetVersionId` and touching it would entrench the confusion the isolated key
avoids. Both differences are pure text; neither alters required capabilities,
policy findings, bindings, or the instance manifest, so both variants pass the
identical readiness gate and activation is never blocked by version variance.

### 4. `seed-studyroom` requires `--variant`

`--variant` is mandatory for `seed-studyroom` (no silent default), preventing an
accidental new publish on the default key. `seed_studyroom` gains a
`variant: FixtureVariant` parameter and publishes `studyroom_ruleset(variant)`.
The existing `#[cfg(test)]` callers of `studyroom_ruleset()` and
`seed_studyroom(...)` are updated to pass a variant (default `V1` for the
validation/manifest tests that are variant-agnostic).

CLI surface:

```
interaction-smoke --ruleset-key studyroom_18e_20260712_a seed-studyroom --variant v1
interaction-smoke --ruleset-key studyroom_18e_20260712_a seed-studyroom --variant v2
interaction-smoke --ruleset-key studyroom_18e_20260712_a activate <version>
interaction-smoke --ruleset-key studyroom_18e_20260712_a run
```

## Observable v1 / v2 Difference

| Location | v1 | v2 |
| --- | --- | --- |
| declared create panel content (`study_panel`) | `스터디룸 만들기 · v1` | `스터디룸 만들기 · v2` |
| join rule `EditResponse` content | `스터디룸에 참가했습니다. [v1]` | `스터디룸 참가가 완료되었습니다. [v2]` |

Identical across variants (must not diverge): internal `version` field, modal,
create/submit rule, welcome panel, hub panel, `RegisterInstance` manifest, close
button and `study_close_rule` (kept identical so teardown is a regression check,
not a version-difference vector).

Forbidden difference vectors (would make activation variance the confound):
permissions, bindings, role/channel manifest, teardown footprint, privileged
policy severity.

## Isolation Semantics

- **Dedicated key.** The runbook uses a key distinct from `studyroom_demo` (whose
  registry already holds versions 1 and 2 with `next_version = 3` from earlier
  phases). Dispatch is key-agnostic (it reads each instance's stored key), so
  isolation is complete.
- **Repeatable via a NEW key, not via same-key reset.** A dedicated key is not
  permanently empty: re-running the runbook against the *same* key finds its v1
  and v2 already published. "Repeatable" therefore means **pick a fresh unique
  key each run** (e.g. `studyroom_18e_20260712_a`, `..._b`), not that one key
  restarts at version 1. Runbook start condition: confirm the target key has no
  head / version / activation, or choose a new unique key. Never delete existing
  registry data to reset.
- **Fixture variant number ≠ registry version.** `FixtureVariant::V1/V2` are demo
  behaviors; `RuleSetVersionId` is what the registry assigns by publish order. On
  a clean key they coincide (V1 → version 1, V2 → version 2), but no code or test
  asserts that coincidence. The runbook records the **actual** publish output:

  ```
  variant V1 published as registry version A
  variant V2 published as registry version B
  ```

  and all later `activate`/pin verification uses `A`/`B`. Re-publishing the same
  artifact returns `Reused`, so versions are read from output, never hardcoded.
- **Panel coexistence caveat.** Panel installation is keyed by
  `(guild, ruleset_key, panel_key)`, so `studyroom_18e...` gets its own
  installation record. If a `studyroom_demo` bot is also running against the same
  channel, both keys can each leave a panel there and blur the evidence. The
  runbook requires that no `studyroom_demo` gateway/panel is active during 18e
  (or that a dedicated test channel binding is used), then runs only the 18e key.

## What Each Live Step Proves

```
publish (18a/18b)            seed --variant vN → Created(version N) in Postgres
gated activation (18c-4)     activate N → readiness gate → active pointer = N
hydration (18c-1)            run → active artifact re-verified → RuntimeRuleSet
declared panel install (18d-1)  first run Posted; cross-version run Edited; rollback Edited back
version pin (18c-2)          RegisterInstance stores instance.ruleset_version = active at creation
pinned dispatch (18c-3)      join/close load the instance's pinned version per click, ignoring active
preallocated footprint (18d-2)  each instance owns role+channel+welcome+hub at registration
teardown (18d-3)             close button → Active→Deleting→Deleted, all resources deleted
restart recovery             active pointer + instance pins + panel record restored from Postgres
```

## Live Certification Scenario

Dedicated key `K` (fresh, e.g. `studyroom_18e_<date>_a`); reused bot/guild; local
`starring` DB. Registry versions written as `A` (variant V1) and `B` (variant V2)
per actual publish output.

```
[0] Precondition: K has no head/version/activation; no studyroom_demo gateway active on the channel.

[1] v1 bootstrap
    seed --variant v1        -> published registry version A (record A)
    activate A               -> active = A (N notices)
    run                      -> hydrate active A; declared panel Posted (content "· v1")
    create room R1 (make → modal → submit)
      R1.ruleset_version = A ; R1 join click → "스터디룸에 참가했습니다. [v1]"

[2] restart #1 + v2 activation
    (stop gateway)
    seed --variant v2        -> published registry version B (record B; B ≠ A)
    activate B               -> active = B
    run                      -> hydrate active B; declared panel Edited ("· v1" → "· v2")
    create room R2
      R2.ruleset_version = B ; R2 join click → "스터디룸 참가가 완료되었습니다. [v2]"
      R1 join click          → still "…[v1]"   (pinned A loaded fresh, active B ignored)

[3] restart #2 + v1 rollback
    (stop gateway)
    activate A               -> active = A      (re-activate existing immutable artifact; no re-seed)
    run                      -> hydrate active A; declared panel Edited ("· v2" → "· v1")
    create room R3
      R3.ruleset_version = A ; R3 join click → "…[v1]"
      R2 join click          → still "…[v2]"   (pinned B)

[4] teardown regression
    click "방 닫기" on one room → TeardownOutcome Completed; role/channel/welcome/hub deleted;
      instance status = deleted

Final state:  active = A ;  R1 = A(v1) ;  R2 = B(v2) ;  R3 = A(v1)
```

Restart #1 and #2 prove that active pointer, instance pins, and the panel record
are restored from PostgreSQL rather than surviving only in process memory: after
each restart the gateway re-hydrates from the DB, existing rooms keep their pins,
and the declared panel reconciles to the newly hydrated version.

## Gated Activation Failure — optional supplementary evidence

Rollback alone does not exercise the active-pointer protection on a failed
activation. This is **optional**, not part of the required completion criteria,
because inducing it live can destabilize the other scenarios.

- If safely reproducible: temporarily revoke a bot capability (e.g. remove
  `MANAGE_ROLES` from the bot's role), attempt `activate <target>`, observe
  `ActivationError::NotReady` with the **active pointer unchanged**, then restore
  the capability and re-verify. This needs no fixture or engine change — only a
  live Discord permission toggle.
- Otherwise: reference the existing 18c-4 live evidence
  (`activate 1 → activated (8 notices)`, not-ready target keeps the active
  pointer and published artifact), which already certified this property.

## Runbook Deliverable

18e is evidence-centric. The primary artifact is a runbook filled during live
execution:

```
docs/superpowers/runbooks/2026-07-12-durable-ruleset-rollback-18e.md
```

Template sections (each filled with observed values, not assumptions):

```
- chosen ruleset key K and precondition check (empty head/version/activation)
- initial DB state
- seed/publish results: variant → PublishOutcome (Created/Reused) → registry version + content hash
- each activation result and active-pointer transition
- run output: hydrated version, notices, panel reconcile outcome
- instance IDs with pinned ruleset_version (R1, R2, R3)
- per-click actual responses (v1 vs v2 text) including cross-version clicks
- restart #1 and #2 recovery: restored active, preserved pins
- panel installation record change across versions (installed_version, spec_hash)
- teardown result and post-teardown resource/DB state
- optional gated-activation-failure evidence (or 18c-4 reference)
- final DB state (active pointer, instances and pins)
- known limitations
```

## Testing

Pure, DB-less unit tests in `interaction-smoke` (no process env mutation, no
Discord):

1. `parse_cli` happy paths: `run` default; `seed-studyroom --variant v1/v2`
   with/without `--activate`/`--force-activate`; `activate 7`; `--ruleset-key`
   before or after the subcommand (position independence).
2. `parse_cli` fail-closed matrix: duplicate `--ruleset-key`; valueless
   `--ruleset-key`; valueless `--variant`; invalid `--variant v3`; unknown flag;
   `activate` without version; `activate` with a non-numeric version;
   `--variant` on `run`/`activate`; `--activate` on `run`/`activate`.
3. `resolve_ruleset_key`: CLI wins over env; env wins over default; default when
   both absent; an invalid key string yields `CliError::InvalidRulesetKey`.
4. `variant_definitions_differ_only_in_presentation`: publish both variants to an
   `InMemoryRuleSetStore` (which uses the same production `Sha256RuleSetHasher` as
   the Postgres store) and assert V1 → `Created`, V2 → `Created` with a
   **different** registry version, and V1 again → `Reused` — proving
   `content_hash(V1) != content_hash(V2)` through the production hash path. Then
   assert both definitions are **equal in every activation-gate input**:
   `validate` (structural + bindings) succeeds for both;
   `required_capabilities` equal; policy severity/findings equal; and the
   `RegisterInstance` instance manifest is identical. This certifies the
   difference is text-only.
5. Both variants preserve the 18d-2 complete-manifest invariant: reuse the
   existing manifest/registration test for `V1` and `V2`.
6. Optional no-hardcoded-key guard: a source scan asserting `studyroom_demo`
   appears only on the `DEFAULT_RULESET_KEY` line (the property is structurally
   guaranteed by single resolution; this test is a mechanical backstop).

## Completion Criteria

Required (must be demonstrated live):

```
v1 publish + gated activate
v2 publish + gated activate
existing instance keeps its pinned version under a newer active version
v1 rollback (gated re-activation)
new instance after rollback pins v1; existing v2 instance stays v2
restart recovery of active pointer + instance pins (restart #1 and #2)
declared panel reconcile across versions
teardown regression
runbook evidence recorded
```

Optional: NotReady activation rejection (live if safe, else 18c-4 reference).

Completion statement:

> After activating RuleSet v2 and rolling back to v1, each `AutomationInstance`
> keeps dispatching with the immutable RuleSet version pinned at its creation,
> while a newly created instance pins the current active version. The active
> pointer, RuleSet artifacts, instance pins, and panel installation state are all
> restored from PostgreSQL after a process restart.

## Known Limitations

- Certification uses one guild and one reused bot; multi-guild concurrency is not
  exercised.
- The gateway hydrates active once at boot; changing the active version requires
  a restart (the durable model, and what the restart steps prove).
- The declared-panel reconcile edits the single per-key panel in place; a stray
  `studyroom_demo` panel on the same channel is out of 18e's scope and is
  excluded by the precondition.
- The optional gated-failure step, when performed live, mutates bot permissions
  transiently and must restore them.

## Roadmap

18e closes the Durable RuleSet Lifecycle arc (18a–18e). Next in stage 1 (engine
close-out) is the safety-boundary work deliberately excluded here: bind
activation to an approval decision (the current `activate_if_ready` + CLI surface
is not approval-gated), add CI (fmt/clippy/test, running ignored Postgres tests
with `--test-threads=1` to avoid the known panel-installation-postgres parallel
flake), and write `CURRENT_STATE.md`.
