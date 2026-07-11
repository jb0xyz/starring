# Safe RuleSet Activation Service (18c-4) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Gate every active-pointer change behind the same readiness gate the bot uses at boot: `activate_if_ready` activates a target version only if `check_readiness` passes against the current Discord state; on failure the active pointer is unchanged.

**Architecture:** A new `activate_if_ready` function in `automation-ruleset-readiness` (sibling of `check_readiness`/`hydrate_active_ruleset`) does `get_version → check_readiness → store.activate` and returns `ActivationOutcome { activation, runtime_ruleset }`. The tool exposes a single gated activation path (`activate <key> <version>`, `seed-studyroom --activate`) reusing one snapshot-context helper; the low-level `store.activate` is never called directly from the tool.

**Tech Stack:** Rust 2021, native `async fn` in trait, `futures::executor::block_on` tests, reuse of `check_readiness`/`build_readiness_context` and `RuleSetStore`.

## Global Constraints

- No code comments anywhere (`//`, `///`, `//!`).
- `automation-ruleset-readiness` forbidden deps unchanged: `sqlx`, `twilight-*`, `automation-ruleset-dispatch` (cycle). It stays snapshot-provider-free; callers pass pre-built `(guild_capabilities, role_permissions)`.
- Operational invariant: the tool never calls `store.activate` directly; all activation goes through `activate_if_ready`.
- `activate_if_ready` has no `force` parameter — force is a tool-level accidental-replacement confirmation only, never a gate bypass.
- Gates: `$HOME/.cargo/bin/cargo test` (workspace), `clippy --all-targets -- -D warnings`, `fmt --check`. Postgres tests are `#[ignore]`.

## File Structure

- Create `crates/automation-ruleset-readiness/src/activate.rs` — `activate_if_ready`, `ActivationError`, `ActivationOutcome`, unit tests
- Modify `crates/automation-ruleset-readiness/src/lib.rs` — module + re-exports
- Create `crates/automation-ruleset-readiness/tests/postgres_activate.rs` — ignored Postgres integration
- Modify `crates/automation-ruleset-readiness/Cargo.toml` — Postgres dev-deps (dev only)
- Modify `tools/interaction-smoke/src/main.rs` — extract `readiness_context` helper; `activate` subcommand; `seed-studyroom` publish-only + `--activate`

---

## Task 1: `activate_if_ready` in the readiness crate

**Files:**
- Create: `crates/automation-ruleset-readiness/src/activate.rs`
- Modify: `crates/automation-ruleset-readiness/src/lib.rs`
- Create: `crates/automation-ruleset-readiness/tests/postgres_activate.rs`
- Modify: `crates/automation-ruleset-readiness/Cargo.toml`

**Interfaces:**
- Produces: `activate_if_ready(store, guild, key, version, bindings, &GuildCapabilities, &role_permissions) -> Result<ActivationOutcome, ActivationError>`; `ActivationOutcome { activation, runtime_ruleset }`; `ActivationError`.
- Consumes: `automation_ruleset::{RuleSetStore, RuleSetKey, RuleSetVersionId, RuleSetActivation, RuleSetStoreError}`; `crate::{check_readiness, GuildCapabilities, ReadinessError, RuleSetReadinessInput, RuntimeRuleSet}`.

- [ ] **Step 1: Write `activate.rs`**

Create `crates/automation-ruleset-readiness/src/activate.rs`:

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

- [ ] **Step 2: Export from `lib.rs`**

In `crates/automation-ruleset-readiness/src/lib.rs`, add `pub mod activate;` and extend the re-exports:

```rust
pub use activate::{activate_if_ready, ActivationError, ActivationOutcome};
```

- [ ] **Step 3: Build**

Run: `$HOME/.cargo/bin/cargo build -p automation-ruleset-readiness`
Expected: clean.

- [ ] **Step 4: Write unit tests (spy store) at the bottom of `activate.rs`**

Append to `crates/automation-ruleset-readiness/src/activate.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use automation_ruleset::{
        InMemoryRuleSetStore, PublishOutcome, PublishRuleSetRequest, RuleSetVersion,
    };
    use automation_state::{ActionSpec, InteractionRule, InteractionRuleSet, TriggerSpec};
    use discord_model::UserId;
    use futures::executor::block_on;

    struct SpyStore {
        inner: InMemoryRuleSetStore,
        activate_calls: AtomicUsize,
        fail_activate: bool,
    }

    impl SpyStore {
        fn new(fail_activate: bool) -> Self {
            Self {
                inner: InMemoryRuleSetStore::default(),
                activate_calls: AtomicUsize::new(0),
                fail_activate,
            }
        }
        fn activate_calls(&self) -> usize {
            self.activate_calls.load(Ordering::SeqCst)
        }
    }

    impl RuleSetStore for SpyStore {
        async fn publish(
            &self,
            request: PublishRuleSetRequest,
        ) -> Result<PublishOutcome, RuleSetStoreError> {
            self.inner.publish(request).await
        }
        async fn get_version(
            &self,
            guild_id: GuildId,
            key: &RuleSetKey,
            version: RuleSetVersionId,
        ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
            self.inner.get_version(guild_id, key, version).await
        }
        async fn list_versions(
            &self,
            guild_id: GuildId,
            key: &RuleSetKey,
        ) -> Result<Vec<RuleSetVersion>, RuleSetStoreError> {
            self.inner.list_versions(guild_id, key).await
        }
        async fn activate(
            &self,
            guild_id: GuildId,
            key: &RuleSetKey,
            version: RuleSetVersionId,
        ) -> Result<RuleSetActivation, RuleSetStoreError> {
            self.activate_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_activate {
                return Err(RuleSetStoreError::Backend("activate failed".to_string()));
            }
            self.inner.activate(guild_id, key, version).await
        }
        async fn active(
            &self,
            guild_id: GuildId,
            key: &RuleSetKey,
        ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
            self.inner.active(guild_id, key).await
        }
    }

    const GUILD: GuildId = GuildId(7);

    fn key() -> RuleSetKey {
        RuleSetKey::parse("studyroom").unwrap()
    }

    fn create_role_rule() -> InteractionRuleSet {
        InteractionRuleSet {
            version: 1,
            panels: vec![],
            modals: vec![],
            rules: vec![InteractionRule {
                key: "r".to_string(),
                trigger: TriggerSpec::InstanceAction {
                    action: "test".to_string(),
                },
                actions: vec![
                    ActionSpec::DeferEphemeral,
                    ActionSpec::CreateRole {
                        key: "role".to_string(),
                        name: "n".to_string(),
                    },
                    ActionSpec::EditResponse {
                        content: "done".to_string(),
                    },
                ],
            }],
        }
    }

    fn no_capability_rule() -> InteractionRuleSet {
        InteractionRuleSet {
            version: 1,
            panels: vec![],
            modals: vec![],
            rules: vec![InteractionRule {
                key: "r".to_string(),
                trigger: TriggerSpec::InstanceAction {
                    action: "test".to_string(),
                },
                actions: vec![
                    ActionSpec::DeferEphemeral,
                    ActionSpec::EditResponse {
                        content: "done".to_string(),
                    },
                ],
            }],
        }
    }

    fn publish(store: &SpyStore, def: InteractionRuleSet) -> RuleSetVersionId {
        let outcome = block_on(store.publish(PublishRuleSetRequest {
            guild_id: GUILD,
            ruleset_key: key(),
            definition: def,
            created_by: UserId(1),
        }))
        .unwrap();
        match outcome {
            PublishOutcome::Created(v) => v.version,
            PublishOutcome::Reused(v) => v.version,
        }
    }

    fn admin() -> GuildCapabilities {
        GuildCapabilities {
            base_permissions: Permissions::ADMINISTRATOR,
        }
    }

    fn no_manage_roles() -> GuildCapabilities {
        GuildCapabilities {
            base_permissions: Permissions::SEND_MESSAGES,
        }
    }

    fn call(
        store: &SpyStore,
        version: RuleSetVersionId,
        caps: &GuildCapabilities,
    ) -> Result<ActivationOutcome, ActivationError> {
        let bindings = ResourceBindingMap::default();
        let roles = BTreeMap::new();
        block_on(activate_if_ready(
            store, GUILD, &key(), version, &bindings, caps, &roles,
        ))
    }

    #[test]
    fn version_not_found_skips_activate() {
        let store = SpyStore::new(false);
        let missing = RuleSetVersionId::new(9).unwrap();
        assert_eq!(call(&store, missing, &admin()).unwrap_err(), ActivationError::VersionNotFound);
        assert_eq!(store.activate_calls(), 0);
    }

    #[test]
    fn not_ready_leaves_active_unchanged() {
        let store = SpyStore::new(false);
        let v1 = publish(&store, no_capability_rule());
        call(&store, v1, &admin()).unwrap();
        let v2 = publish(&store, create_role_rule());
        assert!(matches!(
            call(&store, v2, &no_manage_roles()).unwrap_err(),
            ActivationError::NotReady(_)
        ));
        assert_eq!(store.activate_calls(), 1);
        assert_eq!(
            block_on(store.active(GUILD, &key())).unwrap().unwrap().version,
            v1
        );
    }

    #[test]
    fn ready_activates_once() {
        let store = SpyStore::new(false);
        let v1 = publish(&store, create_role_rule());
        let outcome = call(&store, v1, &admin()).unwrap();
        assert_eq!(outcome.activation.active_version, v1);
        assert_eq!(store.activate_calls(), 1);
    }

    #[test]
    fn already_active_reruns_gate() {
        let store = SpyStore::new(false);
        let v1 = publish(&store, create_role_rule());
        call(&store, v1, &admin()).unwrap();
        call(&store, v1, &admin()).unwrap();
        assert_eq!(store.activate_calls(), 2);
    }

    #[test]
    fn notices_preserved() {
        let store = SpyStore::new(false);
        let v1 = publish(&store, create_role_rule());
        let outcome = call(&store, v1, &admin()).unwrap();
        assert!(!outcome.runtime_ruleset.notices.is_empty());
    }

    #[test]
    fn activate_error_keeps_pointer() {
        let store = SpyStore::new(true);
        let v1 = publish(&store, create_role_rule());
        assert!(matches!(
            call(&store, v1, &admin()).unwrap_err(),
            ActivationError::Activate(_)
        ));
        assert!(block_on(store.active(GUILD, &key())).unwrap().is_none());
    }
}
```

- [ ] **Step 5: Run unit tests**

Run: `$HOME/.cargo/bin/cargo test -p automation-ruleset-readiness activate`
Expected: all pass.

- [ ] **Step 6: Add Postgres dev-deps and ignored integration test**

In `crates/automation-ruleset-readiness/Cargo.toml`, add under `[dev-dependencies]` (dev only — keeps runtime deps free of sqlx):

```toml
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
automation-ruleset-postgres = { path = "../automation-ruleset-postgres" }
```

Create `crates/automation-ruleset-readiness/tests/postgres_activate.rs`: using `STARRING_TEST_DATABASE_URL` (DB name must contain `test`), run `automation_ruleset_postgres::MIGRATOR`, publish a ready v1 (`no_capability_rule` shape: `[Defer, Edit]`) and a not-ready v2 (`create_role_rule` shape: `[Defer, CreateRole, Edit]`, needs MANAGE_ROLES). Call `activate_if_ready(v1, admin caps)` → asserts `Ok`, `store.active == v1`. Call `activate_if_ready(v2, caps without MANAGE_ROLES)` → asserts `NotReady`, `store.active` still `v1`, and `store.get_version(v2)` still returns the artifact (publish not rolled back). Mark `#[ignore]`.

Run: `STARRING_TEST_DATABASE_URL=postgres://localhost/starring_test $HOME/.cargo/bin/cargo test -p automation-ruleset-readiness --test postgres_activate -- --ignored --test-threads=1`
Expected: PASS.

- [ ] **Step 7: Gate and commit**

Run: `$HOME/.cargo/bin/cargo test -p automation-ruleset-readiness && $HOME/.cargo/bin/cargo clippy --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --check`
Expected: clean.

```bash
git add crates/automation-ruleset-readiness
git commit -m "feat(automation-ruleset-readiness): gated activate_if_ready with readiness preflight"
```

---

## Task 2: Tool — single gated activation surface

**Files:**
- Modify: `tools/interaction-smoke/src/main.rs`

**Interfaces:**
- Consumes: `automation_ruleset_readiness::{activate_if_ready, ActivationError, ActivationOutcome}`, the existing `bindings(channel_id)` helper.
- Produces: `readiness_context` helper; `activate` subcommand; `seed-studyroom --activate`.

- [ ] **Step 1: Extract the shared `readiness_context` helper**

Extract the snapshot → `build_readiness_context` block (current `main` lines ~65-92) into one helper used by `run` and both activation paths:

```rust
async fn readiness_context(
    http: &Client,
    guild_id: u64,
    bindings: &ResourceBindingMap,
) -> Result<
    (
        automation_ruleset_readiness::GuildCapabilities,
        std::collections::BTreeMap<ResourceKey, Permissions>,
    ),
    Box<dyn std::error::Error>,
> {
    let bot = http.current_user().await?.model().await?;
    let guild_roles = http.roles(Id::new(guild_id)).await?.model().await?;
    let bot_member = http
        .guild_member(Id::new(guild_id), bot.id)
        .await?
        .model()
        .await?;
    let roles_snapshot: std::collections::BTreeMap<RoleId, Permissions> = guild_roles
        .iter()
        .map(|role| {
            (
                RoleId(role.id.get()),
                Permissions::from_bits_retain(role.permissions.bits()),
            )
        })
        .collect();
    let bot_role_ids: Vec<RoleId> = bot_member.roles.iter().map(|id| RoleId(id.get())).collect();
    automation_ruleset_readiness::build_readiness_context(
        GuildId(guild_id),
        bindings,
        &roles_snapshot,
        &bot_role_ids,
    )
    .map_err(|e| format!("readiness context failed: {e:?}").into())
}
```

Replace the inlined block in `run` with `let (guild_capabilities, role_permissions) = readiness_context(&http, guild_id, &bindings).await?;`.

Run: `$HOME/.cargo/bin/cargo build -p interaction-smoke`
Expected: clean (behavior of `run` unchanged).

- [ ] **Step 2: Add the `activate <key> <version>` subcommand**

In `main`, before the `run` fallthrough, handle `mode == "activate"`:

```rust
if mode == "activate" {
    let version_arg: u32 = std::env::args()
        .nth(2)
        .ok_or("usage: activate <version>")?
        .parse()?;
    let version = automation_ruleset::RuleSetVersionId::new(version_arg)
        .map_err(|e| format!("invalid version: {e:?}"))?;
    if ruleset_store
        .get_version(GuildId(guild_id), &ruleset_key, version)
        .await
        .map_err(|e| format!("version lookup failed: {e:?}"))?
        .is_none()
    {
        return Err(format!("version {version} not found; nothing activated").into());
    }
    let token = env::var("DISCORD_TEST_TOKEN")?;
    let channel_id: u64 = env::var("DISCORD_TEST_CHANNEL")?.parse()?;
    let bindings = bindings(channel_id);
    let http = Client::new(token);
    let (caps, role_permissions) = readiness_context(&http, guild_id, &bindings).await?;
    let outcome = automation_ruleset_readiness::activate_if_ready(
        &ruleset_store,
        GuildId(guild_id),
        &ruleset_key,
        version,
        &bindings,
        &caps,
        &role_permissions,
    )
    .await
    .map_err(|e| format!("activation failed (active pointer unchanged): {e:?}"))?;
    eprintln!(
        "activated {} ({} notices)",
        outcome.activation.active_version,
        outcome.runtime_ruleset.notices.len()
    );
    return Ok(());
}
```

The single-version key (`RULESET_KEY`) is used for this demo tool; the `<key>` argument is fixed to the tool's studyroom key. The pre-guard `get_version` skips the Discord snapshot when the version is missing.

- [ ] **Step 3: Make `seed-studyroom` publish-only + `--activate`**

Change `seed_studyroom` to publish only by default (remove its low-level `store.activate` calls). Add a `--activate` flag: after publishing, apply the accidental-replacement guard, then run the same gated path.

```rust
async fn seed_studyroom(
    store: &impl RuleSetStore,
    guild_id: u64,
    key: &RuleSetKey,
    activate: bool,
    force_activate: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let published = store
        .publish(PublishRuleSetRequest {
            guild_id: GuildId(guild_id),
            ruleset_key: key.clone(),
            definition: studyroom_ruleset(),
            created_by: UserId(0),
        })
        .await
        .map_err(|e| format!("publish failed: {e:?}"))?;
    let version = match &published {
        PublishOutcome::Created(v) => v.version,
        PublishOutcome::Reused(v) => v.version,
    };
    eprintln!("seed: published {version}");
    if !activate {
        return Ok(());
    }
    if let Some(current) = store
        .active(GuildId(guild_id), key)
        .await
        .map_err(|e| format!("active lookup failed: {e:?}"))?
    {
        if current.version != version && !force_activate {
            return Err(format!(
                "seed: active version {} != target {version}; pass --force-activate to replace",
                current.version
            )
            .into());
        }
    }
    let token = env::var("DISCORD_TEST_TOKEN")?;
    let channel_id: u64 = env::var("DISCORD_TEST_CHANNEL")?.parse()?;
    let bindings = bindings(channel_id);
    let http = Client::new(token);
    let (caps, role_permissions) = readiness_context(&http, guild_id, &bindings).await?;
    let outcome = automation_ruleset_readiness::activate_if_ready(
        store,
        GuildId(guild_id),
        key,
        version,
        &bindings,
        &caps,
        &role_permissions,
    )
    .await
    .map_err(|e| format!("seed activation failed (published, active unchanged): {e:?}"))?;
    eprintln!(
        "seed: activated {} ({} notices)",
        outcome.activation.active_version,
        outcome.runtime_ruleset.notices.len()
    );
    Ok(())
}
```

Update the call site: parse `--activate` (`let activate = std::env::args().any(|a| a == "--activate");`) and pass `seed_studyroom(&ruleset_store, guild_id, &ruleset_key, activate, force_activate)`. The accidental-replacement guard applies only in `seed --activate`; the standalone `activate` command is explicit replace-intent and needs no force.

- [ ] **Step 4: Build, gate, commit**

Run: `$HOME/.cargo/bin/cargo build && $HOME/.cargo/bin/cargo clippy --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --check`
Expected: clean.

```bash
git add tools/interaction-smoke
git commit -m "feat(interaction-smoke): single gated activation surface (activate + seed --activate)"
```

---

## Live demo (after both tasks)

Reused bot/guild/local `starring`:

1. `seed-studyroom` (publish only) → confirm a version row exists, active pointer unchanged (`store.active` unchanged / none).
2. `activate <version>` for the ready v1 → succeeds, active = v1, notices reported.
3. Publish a v2 that fails readiness (e.g. bind an Existing role that is absent, or a rule needing a capability the bot lacks) → `activate 2` → rejected with the readiness error, active still v1.
4. Restart `run` → hydrates v1 (fail-closed defense still holds); confirm the not-ready v2 never became active.

## Self-Review notes

- `ActivationError`/`ActivationOutcome` derive `Eq`; `RuleSetStoreError`, `ReadinessError`, `RuleSetActivation`, `RuntimeRuleSet` are all `Eq` — verified.
- `activate_if_ready` has no `force` parameter; `check_readiness` is reused verbatim (no duplicated validation); notices flow out via `ActivationOutcome`.
- The spy store proves `activate_calls == 0` on `VersionNotFound`/`NotReady` and the prior pointer is unchanged; the already-active test proves the gate re-runs.
- All three tool paths (`run`, `activate`, `seed --activate`) share `readiness_context` and the `bindings(channel_id)` helper — no per-command divergence.
- Publish and activation are decoupled: `seed --activate` with a not-ready version leaves the published artifact in the registry and the active pointer unchanged (asserted in the Postgres integration test).
