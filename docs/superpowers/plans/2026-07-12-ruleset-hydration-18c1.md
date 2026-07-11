# Phase 18c-1 — Active RuleSet Hydration + Readiness Gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A pure `automation-ruleset-readiness` crate that re-validates a DB `RuleSetVersion` (schema → structural → hash → bindings → policy → capability) and promotes only passing artifacts to a `RuntimeRuleSet`; plus a tool that loads the active RuleSet from PostgreSQL (never a fixture), fail-closed, and runs the gateway with it.

**Architecture:** New pure crate `automation-ruleset-readiness` (no sqlx/twilight) holding the readiness gate, capability preflight, policy classification, context builder, and the store-generic hydrator. The `interaction-smoke` tool gains `seed-studyroom` and `run` subcommands: `run` fetches a Discord role/member snapshot, builds the readiness context, hydrates from Postgres, and only after success installs the panel and starts the gateway.

**Tech Stack:** Rust 2021. Reuses `automation-core` (validate_structural/validate_bindings/analyze/PolicyFinding), `automation-ruleset` (RuleSetVersion/RuleSetStore/content_hash), `discord-model` (Permissions), `resource-resolution` (ResourceBindingMap). Tool uses twilight 0.17 + sqlx via `automation-ruleset-postgres`.

## Global Constraints

- **No comments** anywhere (`//`, `///`, `//!`). Match existing files.
- **Cargo path:** gate commands use `$HOME/.cargo/bin/cargo`.
- **Gates (every task):** `cargo build` · `cargo test` · `cargo clippy --all-targets -- -D warnings` · `cargo fmt --all -- --check`.
- **Crate scope:** new crate `automation-ruleset-readiness` + tool `interaction-smoke` only. `automation-core`/`automation-ruleset`/`automation-state`/`automation-instance`/`automation-instance-postgres`/`automation-runtime` **untouched** (the policy analyzer already separates `EveryoneOverwrite` from `PrivilegedOverwriteAllow`).
- **Dependency guard:** `automation-ruleset-readiness` must NOT depend on `sqlx` or `twilight` (guard test in Task 1). It depends on automation-ruleset/core/state, discord-model, resource-resolution, desired-state.
- **Fail-closed:** DB active pointer is not trusted. Hydration failure ⇒ the process does not start; there is **no fixture fallback** (split-brain, forbidden). No Discord mutation occurs before hydration succeeds.
- **Empirically pre-verified:** `required_capabilities` (StudyRoom → `MANAGE_ROLES | MANAGE_CHANNELS`, all findings Notice), `policy_severity` (privileged grant/overwrite → Blocking) against real `PolicyFinding`/`Permissions`. Twilight `http.roles()` + `Permissions::from_bits_retain(bits())` grounded on `bot-runtime/reader.rs`.

---

## File Structure

- `Cargo.toml` (workspace) — **Modify**: add `crates/automation-ruleset-readiness` member (Task 1).
- `crates/automation-ruleset-readiness/Cargo.toml` — **Create** (Task 1).
- `crates/automation-ruleset-readiness/src/lib.rs` — **Create**: module wiring + re-exports (Tasks 1-2).
- `crates/automation-ruleset-readiness/src/types.rs` — **Create**: GuildCapabilities, RuleSetReadinessInput, RuntimeRuleSet, PolicySeverity, errors (Task 1).
- `crates/automation-ruleset-readiness/src/gate.rs` — **Create**: required_capabilities, policy_severity, check_readiness (Task 1).
- `crates/automation-ruleset-readiness/src/context.rs` — **Create**: build_readiness_context (Task 2).
- `crates/automation-ruleset-readiness/src/hydrate.rs` — **Create**: hydrate_active_ruleset (Task 2).
- `crates/automation-ruleset-readiness/tests/no_ai_gateway.rs`, `tests/dependency_guard.rs` — **Create** (Task 1).
- `tools/interaction-smoke/src/main.rs` — **Modify**: subcommands + hydration wiring (Task 3).
- `tools/interaction-smoke/Cargo.toml` — **Modify**: add automation-ruleset/-postgres/-readiness (Task 3).

---

## Task 1 — Readiness crate: types + gate (chunks A + B)

**Files:**
- Modify: workspace `Cargo.toml`
- Create: `crates/automation-ruleset-readiness/{Cargo.toml, src/lib.rs, src/types.rs, src/gate.rs, tests/no_ai_gateway.rs, tests/dependency_guard.rs}`

**Interfaces:**
- Produces: `GuildCapabilities`, `RuleSetReadinessInput`, `RuntimeRuleSet`, `PolicySeverity`, `ReadinessError`, `ReadinessContextError`; `required_capabilities`, `policy_severity`, `check_readiness`.

- [ ] **Step 1: Register the crate.** In workspace `Cargo.toml` `members`, after `"crates/automation-ruleset-postgres",`:

```toml
    "crates/automation-ruleset-readiness",
```

- [ ] **Step 2: Create `Cargo.toml`.**

```toml
[package]
name = "automation-ruleset-readiness"
version = "0.1.0"
edition.workspace = true

[dependencies]
automation-ruleset = { path = "../automation-ruleset" }
automation-core = { path = "../automation-core" }
automation-state = { path = "../automation-state" }
discord-model = { path = "../discord-model" }
resource-resolution = { path = "../resource-resolution" }
desired-state = { path = "../desired-state" }

[dev-dependencies]
futures = "0.3"
```

- [ ] **Step 3: Create `src/types.rs`.**

```rust
use std::collections::BTreeMap;

use automation_core::{PolicyFinding, ValidationError};
use automation_ruleset::{
    RuleSetHashError, RuleSetKey, RuleSetSchemaVersion, RuleSetStoreError, RuleSetVersion,
    RuleSetVersionId,
};
use automation_state::InteractionRuleSet;
use desired_state::ResourceKey;
use discord_model::{GuildId, Permissions, RoleId};
use resource_resolution::ResourceBindingMap;

pub struct GuildCapabilities {
    pub base_permissions: Permissions,
}

impl GuildCapabilities {
    pub fn satisfies(&self, required: Permissions) -> bool {
        self.base_permissions.contains(Permissions::ADMINISTRATOR)
            || self.base_permissions.contains(required)
    }
}

pub struct RuleSetReadinessInput<'a> {
    pub artifact: &'a RuleSetVersion,
    pub bindings: &'a ResourceBindingMap,
    pub guild_capabilities: &'a GuildCapabilities,
    pub role_permissions: &'a BTreeMap<ResourceKey, Permissions>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeRuleSet {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub version: RuleSetVersionId,
    pub definition: InteractionRuleSet,
    pub notices: Vec<PolicyFinding>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicySeverity {
    Notice,
    Blocking,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadinessError {
    UnsupportedSchema(RuleSetSchemaVersion),
    StructurallyInvalid(Vec<ValidationError>),
    HashComputation(RuleSetHashError),
    HashMismatch,
    BindingInvalid(Vec<ValidationError>),
    BlockingPolicy(Vec<PolicyFinding>),
    MissingCapabilities { missing: Permissions },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadinessContextError {
    EveryoneRoleMissing,
    BoundRoleMissing { key: ResourceKey, role_id: RoleId },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HydrationError {
    NoActiveRuleSet,
    Store(RuleSetStoreError),
    NotReady(ReadinessError),
}
```

- [ ] **Step 4: Create `src/gate.rs` (with failing tests).**

```rust
use automation_core::{analyze, validate_bindings, validate_structural, PolicyFinding};
use automation_ruleset::{content_hash, RuleSetVersion, CURRENT_RULESET_SCHEMA_VERSION};
use automation_state::{ActionSpec, InteractionRuleSet};
use discord_model::Permissions;

use crate::types::{PolicySeverity, ReadinessError, RuleSetReadinessInput, RuntimeRuleSet};

pub fn required_capabilities(ruleset: &InteractionRuleSet) -> Permissions {
    let mut required = Permissions::empty();
    for rule in &ruleset.rules {
        for action in &rule.actions {
            match action {
                ActionSpec::CreateRole { .. }
                | ActionSpec::GrantRole { .. }
                | ActionSpec::UpsertOverwrite { .. } => required |= Permissions::MANAGE_ROLES,
                ActionSpec::CreateChannel { .. } => required |= Permissions::MANAGE_CHANNELS,
                _ => {}
            }
        }
    }
    required
}

pub fn policy_severity(finding: &PolicyFinding) -> PolicySeverity {
    match finding {
        PolicyFinding::DynamicResourceCreation { .. }
        | PolicyFinding::CreatedResourceReference { .. }
        | PolicyFinding::EveryoneOverwrite { .. }
        | PolicyFinding::RuntimeMessagePost { .. }
        | PolicyFinding::RuntimeInteractivePanel { .. } => PolicySeverity::Notice,
        _ => PolicySeverity::Blocking,
    }
}

pub fn check_readiness(input: RuleSetReadinessInput) -> Result<RuntimeRuleSet, ReadinessError> {
    let artifact: &RuleSetVersion = input.artifact;
    if artifact.schema_version != CURRENT_RULESET_SCHEMA_VERSION {
        return Err(ReadinessError::UnsupportedSchema(artifact.schema_version));
    }
    validate_structural(&artifact.definition).map_err(ReadinessError::StructurallyInvalid)?;
    let recomputed = content_hash(artifact.schema_version, &artifact.definition)
        .map_err(ReadinessError::HashComputation)?;
    if recomputed != artifact.content_hash {
        return Err(ReadinessError::HashMismatch);
    }
    validate_bindings(&artifact.definition, input.bindings).map_err(ReadinessError::BindingInvalid)?;
    let findings = analyze(&artifact.definition, input.role_permissions);
    let blocking: Vec<PolicyFinding> = findings
        .iter()
        .filter(|f| policy_severity(f) == PolicySeverity::Blocking)
        .cloned()
        .collect();
    if !blocking.is_empty() {
        return Err(ReadinessError::BlockingPolicy(blocking));
    }
    let notices: Vec<PolicyFinding> = findings
        .into_iter()
        .filter(|f| policy_severity(f) == PolicySeverity::Notice)
        .collect();
    let required = required_capabilities(&artifact.definition);
    if !input.guild_capabilities.satisfies(required) {
        let missing = required & !input.guild_capabilities.base_permissions;
        return Err(ReadinessError::MissingCapabilities { missing });
    }
    Ok(RuntimeRuleSet {
        guild_id: artifact.guild_id,
        ruleset_key: artifact.ruleset_key.clone(),
        version: artifact.version,
        definition: artifact.definition.clone(),
        notices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GuildCapabilities;
    use automation_ruleset::{content_hash as ch, RuleSetContentHash};
    use automation_state::{
        ActionTarget, ChannelRef, CreatedRef, InteractionRule, OverwriteTargetSpec, RoleRef,
        TriggerSpec,
    };
    use automation_ruleset::RuleSetKey;
    use automation_ruleset::RuleSetVersionId;
    use desired_state::ResourceKey;
    use discord_model::{GuildId, UserId};
    use resource_resolution::ResourceBindingMap;
    use std::collections::BTreeMap;

    fn ruleset(actions: Vec<ActionSpec>) -> InteractionRuleSet {
        InteractionRuleSet {
            version: 1,
            panels: vec![],
            modals: vec![],
            rules: vec![InteractionRule {
                key: "r".to_string(),
                trigger: TriggerSpec::ModalSubmit {
                    modal: "m".to_string(),
                },
                actions,
            }],
        }
    }

    fn artifact(def: InteractionRuleSet) -> RuleSetVersion {
        let hash = ch(CURRENT_RULESET_SCHEMA_VERSION, &def).unwrap();
        RuleSetVersion {
            guild_id: GuildId(7),
            ruleset_key: RuleSetKey::parse("studyroom").unwrap(),
            version: RuleSetVersionId::FIRST,
            schema_version: CURRENT_RULESET_SCHEMA_VERSION,
            definition: def,
            content_hash: hash,
            created_by: UserId(1),
        }
    }

    fn admin() -> GuildCapabilities {
        GuildCapabilities {
            base_permissions: Permissions::ADMINISTRATOR,
        }
    }

    fn input<'a>(
        art: &'a RuleSetVersion,
        binds: &'a ResourceBindingMap,
        caps: &'a GuildCapabilities,
        roles: &'a BTreeMap<ResourceKey, Permissions>,
    ) -> RuleSetReadinessInput<'a> {
        RuleSetReadinessInput {
            artifact: art,
            bindings: binds,
            guild_capabilities: caps,
            role_permissions: roles,
        }
    }

    fn create_role() -> ActionSpec {
        ActionSpec::CreateRole {
            key: "role".to_string(),
            name: "n".to_string(),
        }
    }

    #[test]
    fn required_capabilities_maps_actions() {
        let rs = ruleset(vec![
            create_role(),
            ActionSpec::CreateChannel {
                key: "ch".to_string(),
                name: "n".to_string(),
            },
        ]);
        assert_eq!(
            required_capabilities(&rs),
            Permissions::MANAGE_ROLES | Permissions::MANAGE_CHANNELS
        );
        assert!(!required_capabilities(&ruleset(vec![ActionSpec::PostPanel {
            key: "p".to_string(),
            channel: ChannelRef::Created(CreatedRef { created: "ch".to_string() }),
            content: "c".to_string(),
            buttons: vec![],
        }]))
        .contains(Permissions::SEND_MESSAGES));
    }

    #[test]
    fn studyroom_shape_passes_all_notice() {
        let def = ruleset(vec![
            create_role(),
            ActionSpec::UpsertOverwrite {
                channel: ChannelRef::Created(CreatedRef { created: "ch".to_string() }),
                target: OverwriteTargetSpec::Everyone,
                allow: Permissions::empty(),
                deny: Permissions::VIEW_CHANNEL,
            },
            ActionSpec::GrantRole {
                role: RoleRef::Created(CreatedRef { created: "role".to_string() }),
                target: ActionTarget::Actor,
            },
        ]);
        let art = artifact(def);
        let binds = ResourceBindingMap::default();
        let roles = BTreeMap::new();
        let out = check_readiness(input(&art, &binds, &admin(), &roles)).unwrap();
        assert_eq!(out.version, RuleSetVersionId::FIRST);
        assert!(!out.notices.is_empty());
    }

    #[test]
    fn hash_mismatch_blocked() {
        let mut art = artifact(ruleset(vec![create_role()]));
        art.content_hash = RuleSetContentHash::parse_hex(&"00".repeat(32)).unwrap();
        let binds = ResourceBindingMap::default();
        let roles = BTreeMap::new();
        assert_eq!(
            check_readiness(input(&art, &binds, &admin(), &roles)).unwrap_err(),
            ReadinessError::HashMismatch
        );
    }

    #[test]
    fn privileged_grant_blocked() {
        let art = artifact(ruleset(vec![ActionSpec::GrantRole {
            role: RoleRef::Existing(ResourceKey("admin".to_string())),
            target: ActionTarget::Actor,
        }]));
        let mut roles = BTreeMap::new();
        roles.insert(ResourceKey("admin".to_string()), Permissions::ADMINISTRATOR);
        let binds = ResourceBindingMap::default();
        assert!(matches!(
            check_readiness(input(&art, &binds, &admin(), &roles)).unwrap_err(),
            ReadinessError::BlockingPolicy(_)
        ));
    }

    #[test]
    fn missing_capability_blocked() {
        let art = artifact(ruleset(vec![create_role()]));
        let caps = GuildCapabilities {
            base_permissions: Permissions::SEND_MESSAGES,
        };
        let binds = ResourceBindingMap::default();
        let roles = BTreeMap::new();
        assert!(matches!(
            check_readiness(input(&art, &binds, &caps, &roles)).unwrap_err(),
            ReadinessError::MissingCapabilities { .. }
        ));
    }
}
```

- [ ] **Step 5: Create `src/lib.rs`.**

```rust
pub mod gate;
pub mod types;

pub use gate::{check_readiness, policy_severity, required_capabilities};
pub use types::{
    GuildCapabilities, HydrationError, PolicySeverity, ReadinessContextError, ReadinessError,
    RuleSetReadinessInput, RuntimeRuleSet,
};
```

- [ ] **Step 6: Create `tests/no_ai_gateway.rs` and `tests/dependency_guard.rs`.**

```rust
// tests/no_ai_gateway.rs
#[test]
fn no_ai_gateway() {
    assert!(!include_str!("../Cargo.toml").contains("ai-gateway"));
}
```
```rust
// tests/dependency_guard.rs
#[test]
fn readiness_is_pure() {
    let manifest = include_str!("../Cargo.toml");
    assert!(!manifest.contains("sqlx"));
    assert!(!manifest.contains("twilight"));
}
```

- [ ] **Step 7: Gates.**

Run: `$HOME/.cargo/bin/cargo test -p automation-ruleset-readiness`
Expected: PASS (gate unit tests + guards). Pre-verified: required_capabilities/policy_severity logic.

Run: `$HOME/.cargo/bin/cargo clippy -p automation-ruleset-readiness --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 8: Commit.**

```bash
git add Cargo.toml Cargo.lock crates/automation-ruleset-readiness/
git commit -m "feat(automation-ruleset-readiness): readiness gate + capability/policy preflight"
```

---

## Task 2 — Context builder + hydrator (chunks C + D)

**Files:**
- Create: `crates/automation-ruleset-readiness/src/context.rs`, `src/hydrate.rs`
- Modify: `crates/automation-ruleset-readiness/src/lib.rs`

**Interfaces:**
- Produces: `build_readiness_context(guild_id, bindings, roles_snapshot, bot_role_ids) -> Result<(GuildCapabilities, BTreeMap<ResourceKey, Permissions>), ReadinessContextError>` (pure, fail-closed on missing bound role); `hydrate_active_ruleset(store, guild_id, key, bindings, caps, role_permissions) -> Result<RuntimeRuleSet, HydrationError>`.

- [ ] **Step 1: Create `src/context.rs`.**

```rust
use std::collections::BTreeMap;

use desired_state::ResourceKey;
use discord_model::{GuildId, Permissions, RoleId};
use resource_resolution::ResourceBindingMap;

use crate::types::{GuildCapabilities, ReadinessContextError};

pub fn build_readiness_context(
    guild_id: GuildId,
    bindings: &ResourceBindingMap,
    roles_snapshot: &BTreeMap<RoleId, Permissions>,
    bot_role_ids: &[RoleId],
) -> Result<(GuildCapabilities, BTreeMap<ResourceKey, Permissions>), ReadinessContextError> {
    let everyone = RoleId(guild_id.0);
    let mut base = *roles_snapshot
        .get(&everyone)
        .ok_or(ReadinessContextError::EveryoneRoleMissing)?;
    for role_id in bot_role_ids {
        if let Some(perms) = roles_snapshot.get(role_id) {
            base |= *perms;
        }
    }
    let mut role_permissions = BTreeMap::new();
    for (key, role_id) in &bindings.role_bindings {
        let perms = roles_snapshot.get(role_id).ok_or_else(|| {
            ReadinessContextError::BoundRoleMissing {
                key: key.clone(),
                role_id: *role_id,
            }
        })?;
        role_permissions.insert(key.clone(), *perms);
    }
    Ok((
        GuildCapabilities {
            base_permissions: base,
        },
        role_permissions,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(pairs: &[(u64, Permissions)]) -> BTreeMap<RoleId, Permissions> {
        pairs.iter().map(|(id, p)| (RoleId(*id), *p)).collect()
    }

    #[test]
    fn everyone_missing_fails() {
        let err = build_readiness_context(
            GuildId(7),
            &ResourceBindingMap::default(),
            &snapshot(&[]),
            &[],
        )
        .unwrap_err();
        assert_eq!(err, ReadinessContextError::EveryoneRoleMissing);
    }

    #[test]
    fn bound_role_missing_fails_closed() {
        let mut bindings = ResourceBindingMap::default();
        bindings
            .role_bindings
            .insert(ResourceKey("mod".to_string()), RoleId(123));
        let err = build_readiness_context(
            GuildId(7),
            &bindings,
            &snapshot(&[(7, Permissions::empty())]),
            &[],
        )
        .unwrap_err();
        assert_eq!(
            err,
            ReadinessContextError::BoundRoleMissing {
                key: ResourceKey("mod".to_string()),
                role_id: RoleId(123),
            }
        );
    }

    #[test]
    fn base_is_everyone_or_bot_roles() {
        let (caps, roles) = build_readiness_context(
            GuildId(7),
            &ResourceBindingMap::default(),
            &snapshot(&[
                (7, Permissions::VIEW_CHANNEL),
                (900, Permissions::MANAGE_ROLES),
            ]),
            &[RoleId(900)],
        )
        .unwrap();
        assert!(caps.base_permissions.contains(Permissions::MANAGE_ROLES));
        assert!(caps.base_permissions.contains(Permissions::VIEW_CHANNEL));
        assert!(roles.is_empty());
    }
}
```

- [ ] **Step 2: Create `src/hydrate.rs`.**

```rust
use std::collections::BTreeMap;

use automation_ruleset::{RuleSetKey, RuleSetStore};
use desired_state::ResourceKey;
use discord_model::{GuildId, Permissions};
use resource_resolution::ResourceBindingMap;

use crate::gate::check_readiness;
use crate::types::{GuildCapabilities, HydrationError, RuleSetReadinessInput, RuntimeRuleSet};

pub async fn hydrate_active_ruleset(
    store: &impl RuleSetStore,
    guild_id: GuildId,
    key: &RuleSetKey,
    bindings: &ResourceBindingMap,
    guild_capabilities: &GuildCapabilities,
    role_permissions: &BTreeMap<ResourceKey, Permissions>,
) -> Result<RuntimeRuleSet, HydrationError> {
    let artifact = store
        .active(guild_id, key)
        .await
        .map_err(HydrationError::Store)?
        .ok_or(HydrationError::NoActiveRuleSet)?;
    check_readiness(RuleSetReadinessInput {
        artifact: &artifact,
        bindings,
        guild_capabilities,
        role_permissions,
    })
    .map_err(HydrationError::NotReady)
}

#[cfg(test)]
mod tests {
    use super::*;
    use automation_ruleset::{
        InMemoryRuleSetStore, PublishRuleSetRequest, RuleSetStore as _,
    };
    use automation_state::{ActionSpec, InteractionRule, InteractionRuleSet, TriggerSpec};
    use discord_model::UserId;
    use futures::executor::block_on;

    fn def() -> InteractionRuleSet {
        InteractionRuleSet {
            version: 1,
            panels: vec![],
            modals: vec![],
            rules: vec![InteractionRule {
                key: "r".to_string(),
                trigger: TriggerSpec::ModalSubmit {
                    modal: "m".to_string(),
                },
                actions: vec![ActionSpec::CreateRole {
                    key: "role".to_string(),
                    name: "n".to_string(),
                }],
            }],
        }
    }

    fn key() -> RuleSetKey {
        RuleSetKey::parse("studyroom").unwrap()
    }

    #[test]
    fn no_active_is_error() {
        let store = InMemoryRuleSetStore::default();
        let roles = BTreeMap::new();
        let caps = GuildCapabilities {
            base_permissions: Permissions::ADMINISTRATOR,
        };
        let err = block_on(hydrate_active_ruleset(
            &store,
            GuildId(7),
            &key(),
            &ResourceBindingMap::default(),
            &caps,
            &roles,
        ))
        .unwrap_err();
        assert_eq!(err, HydrationError::NoActiveRuleSet);
    }

    #[test]
    fn publish_activate_then_hydrate() {
        let store = InMemoryRuleSetStore::default();
        let published = block_on(store.publish(PublishRuleSetRequest {
            guild_id: GuildId(7),
            ruleset_key: key(),
            definition: def(),
            created_by: UserId(1),
        }))
        .unwrap();
        let version = match published {
            automation_ruleset::PublishOutcome::Created(v) => v.version,
            automation_ruleset::PublishOutcome::Reused(v) => v.version,
        };
        block_on(store.activate(GuildId(7), &key(), version)).unwrap();

        let caps = GuildCapabilities {
            base_permissions: Permissions::ADMINISTRATOR,
        };
        let roles = BTreeMap::new();
        let out = block_on(hydrate_active_ruleset(
            &store,
            GuildId(7),
            &key(),
            &ResourceBindingMap::default(),
            &caps,
            &roles,
        ))
        .unwrap();
        assert_eq!(out.version, version);
    }
}
```

- [ ] **Step 3: Extend `src/lib.rs`.**

```rust
pub mod context;
pub mod hydrate;

pub use context::build_readiness_context;
pub use hydrate::hydrate_active_ruleset;
```

- [ ] **Step 4: Gates.**

Run: `$HOME/.cargo/bin/cargo test -p automation-ruleset-readiness`
Expected: PASS — context (everyone-missing, bound-role-missing fail-closed, base OR) + hydrator (no-active, publish→activate→hydrate).

Run: `$HOME/.cargo/bin/cargo clippy -p automation-ruleset-readiness --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 5: Commit.**

```bash
git add crates/automation-ruleset-readiness/
git commit -m "feat(automation-ruleset-readiness): context builder + active-ruleset hydrator"
```

---

## Task 3 — Tool: seed + DB-only hydration (chunk E)

**Files:**
- Modify: `tools/interaction-smoke/Cargo.toml`, `tools/interaction-smoke/src/main.rs`

**Interfaces:**
- Consumes: `automation_ruleset_readiness::{build_readiness_context, hydrate_active_ruleset, GuildCapabilities, HydrationError}`, `automation_ruleset_postgres::PostgresRuleSetStore`, `automation_ruleset::{RuleSetKey, RuleSetStore, PublishRuleSetRequest, PublishOutcome}`. Twilight: `http.roles(guild)`, `http.guild_member(guild, bot)`, `http.current_user()` (grounded on `bot-runtime/reader.rs`; Codex verifies the exact twilight 0.17 method names and reports any adjustment).

- [ ] **Step 1: Add dependencies.** In `tools/interaction-smoke/Cargo.toml` `[dependencies]`:

```toml
automation-ruleset = { path = "../../crates/automation-ruleset" }
automation-ruleset-postgres = { path = "../../crates/automation-ruleset-postgres" }
automation-ruleset-readiness = { path = "../../crates/automation-ruleset-readiness" }
```

- [ ] **Step 2: Add the argv dispatch + seed subcommand.** In `main`, before the existing logic, branch on the first CLI arg (`seed-studyroom` vs default `run`). Seed publishes the fixture and activates only if safe:

```rust
    let mode = std::env::args().nth(1).unwrap_or_else(|| "run".to_string());
    let force_activate = std::env::args().any(|a| a == "--force-activate");

    let database_url = env::var("STARRING_DATABASE_URL")?;
    let pool = sqlx::PgPool::connect(&database_url)
        .await
        .map_err(report_connect_error)?;
    automation_instance_postgres::MIGRATOR
        .run(&pool)
        .await
        .map_err(|error| {
            eprintln!("postgres startup: instance migration failed: {error}");
            "PostgreSQL startup failed during migration".to_string()
        })?;
    automation_ruleset_postgres::MIGRATOR
        .run(&pool)
        .await
        .map_err(|error| {
            eprintln!("postgres startup: ruleset migration failed: {error}");
            "PostgreSQL startup failed during migration".to_string()
        })?;

    let ruleset_store = automation_ruleset_postgres::PostgresRuleSetStore::new(pool.clone());
    let ruleset_key = automation_ruleset::RuleSetKey::parse(RULESET_KEY)
        .map_err(|e| format!("invalid ruleset key: {e:?}"))?;

    if mode == "seed-studyroom" {
        return seed_studyroom(&ruleset_store, guild_id, &ruleset_key, force_activate).await;
    }
```

```rust
async fn seed_studyroom(
    store: &automation_ruleset_postgres::PostgresRuleSetStore,
    guild_id: u64,
    key: &automation_ruleset::RuleSetKey,
    force_activate: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use automation_ruleset::{PublishOutcome, PublishRuleSetRequest, RuleSetStore};
    let guild = GuildId(guild_id);
    let published = store
        .publish(PublishRuleSetRequest {
            guild_id: guild,
            ruleset_key: key.clone(),
            definition: studyroom_ruleset(),
            created_by: UserId(0),
        })
        .await
        .map_err(|e| format!("publish failed: {e:?}"))?;
    let version = match &published {
        PublishOutcome::Created(v) | PublishOutcome::Reused(v) => v.version,
    };
    eprintln!("seed: {published:?}");
    let active = store
        .active(guild, key)
        .await
        .map_err(|e| format!("active lookup failed: {e:?}"))?;
    match active {
        None => {
            store.activate(guild, key, version).await.map_err(|e| format!("activate failed: {e:?}"))?;
            eprintln!("seed: activated {version}");
        }
        Some(current) if current.version == version => {
            store.activate(guild, key, version).await.map_err(|e| format!("activate failed: {e:?}"))?;
            eprintln!("seed: already active {version} (idempotent)");
        }
        Some(current) => {
            if force_activate {
                store.activate(guild, key, version).await.map_err(|e| format!("activate failed: {e:?}"))?;
                eprintln!("seed: force-activated {version} (was {})", current.version);
            } else {
                return Err(format!(
                    "seed: active version {} != published {}; pass --force-activate to override",
                    current.version, version
                )
                .into());
            }
        }
    }
    Ok(())
}
```

`RegisterInstance` in the seed's `studyroom_ruleset()` still needs `created_by` on the artifact; the RuleSet artifact's `created_by` is the seeding user (`UserId(0)` placeholder is fine for a bootstrap).

- [ ] **Step 3: Rewrite the `run` path to hydrate from DB (no fixture, no mutation before success).** Replace the token/guild/channel reads + fixture build + `install_panel` + `gateway::run` with:

```rust
    let token = env::var("DISCORD_TEST_TOKEN")?;
    let channel_id: u64 = env::var("DISCORD_TEST_CHANNEL")?.parse()?;
    let bindings = bindings(channel_id);

    let http = Client::new(token.clone());
    let bot = http.current_user().await?.model().await?;
    let guild_roles = http.roles(Id::new(guild_id)).await?.model().await?;
    let bot_member = http.guild_member(Id::new(guild_id), bot.id).await?.model().await?;

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

    let (guild_capabilities, role_permissions) =
        automation_ruleset_readiness::build_readiness_context(
            GuildId(guild_id),
            &bindings,
            &roles_snapshot,
            &bot_role_ids,
        )
        .map_err(|e| format!("readiness context failed: {e:?}"))?;

    let runtime = automation_ruleset_readiness::hydrate_active_ruleset(
        &ruleset_store,
        GuildId(guild_id),
        &ruleset_key,
        &bindings,
        &guild_capabilities,
        &role_permissions,
    )
    .await
    .map_err(|e| format!("hydration failed (fail-closed, not starting): {e:?}"))?;

    eprintln!(
        "hydrated ruleset {} v{} ({} notices); installing panel + starting gateway",
        runtime.ruleset_key, runtime.version, runtime.notices.len()
    );
    install_panel(&token, guild_id, channel_id).await?;
    let instances = InMemoryInstanceStore::new();
    let instance_ids = random_instance_id::RandomInstanceIdGenerator::new();
    gateway::run(
        token,
        runtime.ruleset_key.as_str().to_string(),
        runtime.definition,
        bindings,
        "스터디룸 처리에 실패했습니다.".to_string(),
        instances,
        instance_ids,
    )
    .await;
    Ok(())
```

Imports to add to `main.rs`: `use discord_model::{GuildId, Permissions, RoleId, UserId};` (extend the existing discord_model import). `install_panel` must be called **only after** hydration succeeds (any earlier Discord mutation would violate fail-closed). The instance store stays InMemory in 18c-1 (durable instances were 17d/17e; wiring PostgresInstanceStore back is orthogonal and can follow — keep 18c-1 focused on ruleset hydration).

> The 17e-era `run` used `PostgresInstanceStore`. If the current tool still wires `PostgresInstanceStore`, keep that instead of `InMemoryInstanceStore` here — do not regress instance durability. Match whatever the current `run` uses for the instance store; only the **ruleset source** changes in 18c-1.

- [ ] **Step 4: Gates (DB-independent build).**

Run: `$HOME/.cargo/bin/cargo build -p interaction-smoke`
Expected: PASS. If any twilight method name differs (e.g. `guild_member`/`current_user`/`roles` return shapes in twilight 0.17), fix minimally and report the exact call used.

Run: `$HOME/.cargo/bin/cargo clippy -p interaction-smoke --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 5: Full workspace gate.**

```bash
$HOME/.cargo/bin/cargo build && $HOME/.cargo/bin/cargo test && \
$HOME/.cargo/bin/cargo clippy --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --all -- --check
```
Expected: whole workspace green.

- [ ] **Step 6: Commit.**

```bash
git add tools/interaction-smoke/Cargo.toml tools/interaction-smoke/src/main.rs Cargo.lock
git commit -m "feat(interaction-smoke): seed-studyroom + DB-only ruleset hydration"
```

---

## Live Runbook (Claude executes — chunk F)

Env: `DISCORD_TEST_TOKEN`, `DISCORD_TEST_GUILD`, `DISCORD_TEST_CHANNEL`, `STARRING_DATABASE_URL` (postgres://localhost/starring).

- [ ] **L1 — fail-closed first:** with no active ruleset, `cargo run -p interaction-smoke` (run mode) → **process exits** (`hydration failed … not starting`); confirm **no panel** posted in the channel.
- [ ] **L2 — seed:** `cargo run -p interaction-smoke -- seed-studyroom` → publishes v1 + activates; `psql` shows one `active` row in `automation_ruleset_versions` + `automation_ruleset_activations`.
- [ ] **L3 — hydrate + run:** `cargo run -p interaction-smoke` → logs `hydrated ruleset studyroom_demo v1 (N notices)` → panel installed → gateway listening. Click Create study room → StudyRoom created; click 참가하기 → role granted.
- [ ] **L4 — restart re-hydrate:** stop + restart the bot → it reloads the ruleset from DB (not fixture) and works again.
- [ ] **L5 — seed safety:** re-run `seed-studyroom` (same fixture) → `Reused` + idempotent activate (no version change).
- [ ] **L6 — cleanup:** delete Discord test artifacts; leave the `starring` DB rows or mark the ruleset activation as desired.

**18c-1 complete when L1 (fail-closed, zero mutation) + L3 (DB-hydrated StudyRoom works) + L4 (restart reloads from DB) all succeed.**

---

## Self-Review

- **Spec coverage:** §2 types → Task 1 types.rs. §3 required_capabilities/policy_severity/check_readiness → Task 1 gate.rs (pre-verified). §3 hydrate + §5 build_readiness_context → Task 2. §5 seed-studyroom (safe activate) + run (fail-closed, mutation-after-hydration) → Task 3. §6 tests: gate unit + context fail-closed + hydrator (Tasks 1-2); seed-safety + mutation-order (Task 3 + live L1/L5). §6.5 limitation (no version pin) → documented; instance store stays as-is.
- **Placeholder scan:** none. All test helpers pass a local `ResourceBindingMap` (no `'static` scaffolding). Twilight `http.roles()` + `Permissions::from_bits_retain(bits())` are grounded on `bot-runtime/reader.rs`; `http.guild_member(...)` / `http.current_user()` are standard twilight 0.17 methods flagged for Codex to confirm (Task 3 Step 4) and report any adjustment.
- **Type consistency:** `content_hash(artifact.schema_version, …)` (not CURRENT). `required & !base_permissions` for `missing`. `RoleId(guild_id.0)` = @everyone. `Permissions::from_bits_retain(twilight.bits())` (reader.rs pattern). `hydrate_active_ruleset` generic over `impl RuleSetStore` (InMemory in tests, Postgres in tool).
- **Fail-closed:** `run` performs no Discord mutation before `hydrate_active_ruleset` returns Ok; `install_panel` is after. Hydration error → `?` propagates → process exits. No fixture fallback.
