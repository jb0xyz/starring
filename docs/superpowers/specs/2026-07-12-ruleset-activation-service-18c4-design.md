# Phase 18c-4: Safe RuleSet Activation Service — Design

## Goal

Gate every active-pointer change behind the same readiness gate the bot uses at
boot. `activate_if_ready` re-validates a target RuleSet version against the current
Discord state and the stored bindings, and sets the active pointer only if the gate
passes. If it fails, the active pointer is unchanged (fail-closed).

## Context

`store.activate(guild, key, version)` is a raw pointer write — it checks only that
the version exists, not that it is *runnable*. An operator can therefore activate a
version that fails readiness (missing capability, blocking policy, unresolvable
binding, hash mismatch). The next boot's 18c-1 hydration then fails closed and the
bot will not start, leaving the guild stuck. 18c-4 is the write-side companion to
18c-1's read-side hydration: both share `check_readiness`, so an operator can never
activate a version the bot would refuse to hydrate.

This is the mechanism 18e's durable rollback (activate v2, then roll back to v1)
will reuse.

## Global Constraints

- No code comments anywhere (`//`, `///`, `//!` all forbidden).
- `activate_if_ready` lives in `automation-ruleset-readiness`, a sibling of
  `check_readiness` and `hydrate_active_ruleset`. Forbidden deps unchanged: `sqlx`,
  `twilight-*`, and — critically — `automation-ruleset-dispatch` (that would form
  `readiness → dispatch → readiness`). The readiness crate stays snapshot-provider-free.
- Operational activation invariant: the tool/API surface never calls
  `store.activate` directly; all activation passes through `activate_if_ready`. The
  low-level `store.activate` remains only for `activate_if_ready`'s own pointer
  write, store implementations, and tests.
- Fail-closed: no active-pointer change on any gate failure.

## Architecture

### Library: `activate_if_ready` (readiness crate)

Symmetry:

```
automation-ruleset-readiness
├─ check_readiness          is this artifact runnable?
├─ hydrate_active_ruleset   may we READ the current active? (boot)
└─ activate_if_ready        may we WRITE this target as active? (control-plane)
```

`hydrate_active_ruleset` and `activate_if_ready` both take already-built
`(guild_capabilities, role_permissions)` — the caller fetches the Discord snapshot
and runs `build_readiness_context`. The readiness crate never touches twilight or a
snapshot provider, so no cycle is introduced.

```rust
use std::collections::BTreeMap;

use automation_ruleset::{
    RuleSetActivation, RuleSetKey, RuleSetStore, RuleSetStoreError, RuleSetVersionId,
};
use desired_state::ResourceKey;
use discord_model::{GuildId, Permissions};
use resource_resolution::ResourceBindingMap;

use crate::gate::check_readiness;
use crate::types::{GuildCapabilities, ReadinessError, RuleSetReadinessInput, RuntimeRuleSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivationError {
    VersionLookup(RuleSetStoreError),
    VersionNotFound,
    NotReady(ReadinessError),
    Activate(RuleSetStoreError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivationOutcome {
    pub activation: RuleSetActivation,
    pub runtime_ruleset: RuntimeRuleSet,
}

pub async fn activate_if_ready<S>(
    store: &S,
    guild_id: GuildId,
    key: &RuleSetKey,
    version: RuleSetVersionId,
    bindings: &ResourceBindingMap,
    guild_capabilities: &GuildCapabilities,
    role_permissions: &BTreeMap<ResourceKey, Permissions>,
) -> Result<ActivationOutcome, ActivationError>
where
    S: RuleSetStore,
{
    let artifact = store
        .get_version(guild_id, key, version)
        .await
        .map_err(ActivationError::VersionLookup)?
        .ok_or(ActivationError::VersionNotFound)?;
    let runtime_ruleset = check_readiness(RuleSetReadinessInput {
        artifact: &artifact,
        bindings,
        guild_capabilities,
        role_permissions,
    })
    .map_err(ActivationError::NotReady)?;
    let activation = store
        .activate(guild_id, key, version)
        .await
        .map_err(ActivationError::Activate)?;
    Ok(ActivationOutcome {
        activation,
        runtime_ruleset,
    })
}
```

`check_readiness`'s `RuntimeRuleSet` is **preserved** in `ActivationOutcome` — its
`notices` let an operator see "activated, with these Notice-level warnings," and a
future API/UI can surface them. No activation-specific validation is written; the
gate is reused verbatim. `activate_if_ready` has **no `force` parameter** — force is
purely a tool-level accidental-replacement confirmation, never a service input, so
the gate cannot be bypassed through the library. Re-activating the version that is
already active still runs the **full gate** (no skip) — this catches permission and
binding drift since the last activation. Snapshot-fetch and `build_readiness_context`
failures are not `ActivationError` variants — they are handled at the edge, which
owns those inputs.

### Edge / tool surface (Ⓑ′ — single gated path)

```
seed-studyroom              publish only (DB-only, no active-pointer change, no Discord needed)
seed-studyroom --activate   publish, then gated activation via the shared path
activate <key> <version>    gated activation
```

Both activating paths run:

```
1. store.get_version(target)          edge pre-guard (cheap)
     None  -> VersionNotFound, no Discord snapshot fetched
     Some  -> continue
2. fetch current Discord snapshot      current_user + roles(guild) + guild_member(bot)
3. build_readiness_context(snapshot)   -> (guild_capabilities, role_permissions)
4. activate_if_ready(...)              re-reads get_version (authoritative) -> check_readiness -> store.activate
```

The step-1 pre-guard exists only to skip the expensive Discord snapshot for a
nonexistent version. The security decision is step 4's second `get_version` +
`check_readiness`; RuleSet versions are immutable, so a divergence between the two
reads is abnormal and is resolved fail-closed on the inner read's result.

`seed-studyroom --activate` additionally applies accidental-replacement protection
BEFORE step 2 (tool-level, not in `activate_if_ready`):

```
current = store.active(guild, key)
  Some(c) if c.version != target and no --force-activate -> stop (accidental switch prevented)
  otherwise                                               -> proceed to gated activation
```

The standalone `activate <key> <version>` command treats itself as explicit
replace-intent and applies no accidental-replacement check.

`--force-activate` means only "I intend to replace a different current active." It
never bypasses readiness: missing capability, blocking policy, binding failure, and
hash mismatch all still block, with or without the flag. There is no readiness-bypass
on the tool surface — bypassing the gate would recreate the exact "next boot won't
start" state 18c-4 exists to prevent.

### Shared snapshot-context helper (tool)

The snapshot → `build_readiness_context` plumbing currently inlined in the `run`
subcommand (fetch `current_user`, `roles(guild)`, `guild_member(bot)`, fold into a
`BTreeMap<RoleId, Permissions>` + `bot_role_ids`, call `build_readiness_context`) is
extracted into one tool helper reused by `run`, `activate`, and `seed --activate`:

```
async fn readiness_context(http, guild_id, bindings)
    -> Result<(GuildCapabilities, BTreeMap<ResourceKey, Permissions>), EdgeError>
```

The tool's activation wrapper maps failures explicitly: `SnapshotFailed`,
`ContextInvalid(ReadinessContextError)`, `Activation(ActivationError)`.

The `ResourceBindingMap` is likewise built by the single existing `bindings(channel_id)`
helper (already used by `run`), shared identically by `run`, `activate`, and
`seed --activate` — never a per-command fixture or hardcoded map. This closes the
divergence where activation passes on one binding map but the next `run` hydration
fails on a different one.

### Publish and activation are separate lifecycles

`seed-studyroom --activate` can publish successfully and then fail the activation
gate. The correct result: the new immutable version stays in the registry, the active
pointer keeps its prior value, and the command exits with the activation error. The
publish is **not** rolled back — registry publish and active-pointer change are
deliberately decoupled lifecycles.

## Guarantees

```
VersionNotFound      -> store.activate called 0 times
NotReady             -> store.activate called 0 times; existing active pointer unchanged
readiness passes     -> store.activate called exactly once
target already active-> gate still runs; store.activate called exactly once (idempotent)
store.activate errs  -> ActivationError::Activate; prior pointer unchanged
```

`RuleSetStore::activate`'s contract: on success the target pointer is fully reflected;
on failure the prior pointer is unchanged. The 18b Postgres implementation guarantees
this with a single atomic `INSERT ... ON CONFLICT DO UPDATE` statement.

Verified with an in-memory spy `RuleSetStore` that counts `activate` calls
(`activate_calls == 0` on the failing paths, `== 1` on success including the
already-active case).

## TOCTOU limitation (documented)

Checking against a fresh snapshot cannot prevent the environment from changing
between the check and the write (snapshot passes → a role's permissions change →
activate). So 18c-4's guarantee is precisely:

> Activation checks the target RuleSet's readiness against the latest Discord state
> and stored bindings at request time, and changes the active pointer only if that
> check passes.

The final defense remains 18c-1 hydration, which re-runs the gate at every boot and
fails closed — so even if permissions change right after activation, the next boot
is protected.

## Testing

- Readiness crate unit tests (in-memory `RuleSetStore` spy that counts `activate`):
  - `VersionNotFound` → `check_readiness` not reached, `activate_calls == 0`.
  - `NotReady` (e.g. `MissingCapabilities`, `BlockingPolicy`) → `activate_calls == 0`,
    active pointer unchanged.
  - ready target → `activate_calls == 1`, `ActivationOutcome.activation` points to target.
  - target already active → gate runs, `activate_calls == 1` (idempotent).
  - notice-only version → `ActivationOutcome` success with `runtime_ruleset.notices`
    non-empty (notices preserved through activation).
  - `store.activate` error → `ActivationError::Activate`, prior pointer intact.
- Tool-level accidental-replacement test: `seed --activate` with a different current
  active and no `--force-activate` refuses without calling `activate_if_ready`.
- Ignored Postgres integration: publish a ready v1 + a not-ready v2 (a rule requiring a
  capability the test caps omit); `activate_if_ready` activates v1, rejects v2 with
  `NotReady`, `store.active` still points to v1 after the rejected v2, and the v2
  artifact remains queryable via `get_version` (publish not rolled back).
- Live (reused bot/guild/local `starring`): publish v1 → `activate studyroom_demo 1`
  succeeds (gate passes, active = v1). Publish a v2 that fails readiness (e.g. binds a
  role that is missing) → `activate studyroom_demo 2` is rejected, active stays v1,
  the bot still hydrates v1 on restart.

## Roadmap

- 18d RouteId + idempotent installation + attach-after-register.
- 18e Durable RuleSet rollback live: `activate` v2 (gated) → new rooms pin v2, existing
  rooms keep pinned v1 (18c-3 dispatch) → `activate` v1 to roll back — all through the
  gated path built here.
