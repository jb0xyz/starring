# Phase 18a — RuleSet Registry Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A pure `automation-ruleset` crate that publishes validated `InteractionRuleSet` definitions as `(guild, key)`-scoped immutable, content-addressed, monotonically-versioned artifacts, with a separate active-version pointer — plus a minimal `validate_structural`/`validate_bindings` split in automation-core.

**Architecture:** New pure crate `automation-ruleset` (InMemory + deterministic tests, no DB/runtime). Publish = `validate_structural → content_hash → dedup-or-allocate-version`. Activation is a low-level pointer store op (the full activation gate is Phase 18c). automation-core gains `validate_structural`/`validate_bindings`, with `validate` composing both (behavior-preserving).

**Tech Stack:** Rust 2021, serde/serde_json, sha2 0.10, `std::num::NonZeroU32`. Native async fn in trait (`#[allow(async_fn_in_trait)]`, generic static dispatch); tests use `futures::executor::block_on`.

## Global Constraints

- **No comments** anywhere (`//`, `///`, `//!`). Match existing files.
- **Cargo path:** gate commands use `$HOME/.cargo/bin/cargo`.
- **Gates (every task):** `cargo build` · `cargo test` · `cargo clippy --all-targets -- -D warnings` · `cargo fmt --all -- --check`.
- **Crate-modification scope:** new crate `automation-ruleset`; `automation-core` gets the validation split only. `automation-state` / `automation-instance` / `automation-instance-postgres` / `automation-runtime` **untouched**.
- **Dependency guard:** `automation-ruleset → automation-core` allowed; **`automation-core → automation-ruleset` forbidden** (cyclic-dep guard test in Task 6).
- **Safety invariant:** no event-time LLM. `automation-ruleset/tests/no_ai_gateway.rs` guards its own Cargo.toml (16a pattern).
- **Publish contract:** structural validation → hash → version, in that order; structural failure consumes **no** hash/version/store/activation change. `publish ≠ activate`.
- **Determinism:** no clocks, no `Math.random`; concurrency tests never assert which content maps to which version number.

---

## File Structure

- `crates/automation-core/src/validate.rs` — **Modify**: split into `validate_structural` + `validate_bindings`; `validate` composes both (Task 1).
- `crates/automation-core/src/lib.rs` — **Modify**: export `validate_structural`, `validate_bindings` (Task 1).
- `Cargo.toml` (workspace) — **Modify**: add `crates/automation-ruleset` member (Task 2).
- `crates/automation-ruleset/Cargo.toml` — **Create** (Task 2).
- `crates/automation-ruleset/src/lib.rs` — **Create**: module wiring + re-exports (Tasks 2-5).
- `crates/automation-ruleset/src/key.rs` — **Create**: `RuleSetKey` (Task 2).
- `crates/automation-ruleset/src/version.rs` — **Create**: `RuleSetVersionId`, `RuleSetSchemaVersion`, `CURRENT_RULESET_SCHEMA_VERSION` (Task 2).
- `crates/automation-ruleset/src/hash.rs` — **Create**: `RuleSetContentHash`, `content_hash`, `canonicalize`, `RuleSetHasher`, `Sha256RuleSetHasher`, `RuleSetHashError` (Task 3).
- `crates/automation-ruleset/src/model.rs` — **Create**: `RuleSetVersion`, `RuleSetActivation` (Task 4).
- `crates/automation-ruleset/src/store.rs` — **Create**: trait + request/outcome/error + `InMemoryRuleSetStore` (Tasks 4-5).
- `crates/automation-ruleset/tests/no_ai_gateway.rs` — **Create** (Task 6).
- `crates/automation-ruleset/tests/dependency_guard.rs` — **Create** (Task 6).

---

## Task 1 — automation-core validation split

**Files:**
- Modify: `crates/automation-core/src/validate.rs`
- Modify: `crates/automation-core/src/lib.rs:26`

**Interfaces:**
- Produces: `validate_structural(&InteractionRuleSet) -> Result<(), Vec<ValidationError>>`, `validate_bindings(&InteractionRuleSet, &ResourceBindingMap) -> Result<(), Vec<ValidationError>>`. `validate(ruleset, bindings)` keeps its signature and now composes both.
- The only binding-dependent checks today are the two `Existing`-ref lookups (`UnknownRoleRef`, `UnknownChannelRef`). Everything else is structural.

- [ ] **Step 1: Drop `bindings` from `check_role_ref` / `check_channel_ref`; move the `Existing` lookup out.** In `validate.rs`, replace `check_role_ref` (currently lines 445-486) and `check_channel_ref` (488-516) with:

```rust
fn check_role_ref(
    errors: &mut Vec<ValidationError>,
    rule: &InteractionRule,
    created: &BTreeMap<String, CreatedKind>,
    role: &RoleRef,
) {
    match role {
        RoleRef::Existing(_) => {}
        RoleRef::Created(inner) => match created.get(&inner.created) {
            None => errors.push(ValidationError::UnknownCreatedRoleRef {
                rule: rule.key.clone(),
                key: inner.created.clone(),
            }),
            Some(CreatedKind::Role) => {}
            Some(_) => errors.push(ValidationError::CreatedRoleRefTypeMismatch {
                rule: rule.key.clone(),
                key: inner.created.clone(),
            }),
        },
        RoleRef::Instance { instance, alias } => {
            if !matches!(rule.trigger, TriggerSpec::InstanceAction { .. }) {
                errors.push(ValidationError::InstanceRoleOutsideInstanceRule {
                    rule: rule.key.clone(),
                });
            }
            if !matches!(instance, InstanceRef::Event) {
                errors.push(ValidationError::InstanceRoleMustUseEvent {
                    rule: rule.key.clone(),
                });
            }
            check_resource_alias(errors, rule, alias);
        }
    }
}

fn check_channel_ref(
    errors: &mut Vec<ValidationError>,
    rule: &InteractionRule,
    created: &BTreeMap<String, CreatedKind>,
    channel: &ChannelRef,
) {
    match channel {
        ChannelRef::Existing(_) => {}
        ChannelRef::Created(inner) => match created.get(&inner.created) {
            None => errors.push(ValidationError::UnknownCreatedChannelRef {
                rule: rule.key.clone(),
                key: inner.created.clone(),
            }),
            Some(CreatedKind::Channel) => {}
            Some(_) => errors.push(ValidationError::CreatedChannelRefTypeMismatch {
                rule: rule.key.clone(),
                key: inner.created.clone(),
            }),
        },
    }
}
```

- [ ] **Step 2: Rename `validate` → `validate_structural`, drop its `bindings` param, and update the two call sites.** In `validate.rs`, change the function signature (currently line 134):

```rust
pub fn validate_structural(ruleset: &InteractionRuleSet) -> Result<(), Vec<ValidationError>> {
```

Delete the `bindings: &ResourceBindingMap,` parameter. Inside the body, update the two calls (currently lines 243 and 285-287):
- `check_role_ref(&mut errors, rule, bindings, &created, role);` → `check_role_ref(&mut errors, rule, &created, role);`
- In the `UpsertOverwrite` arm: `check_channel_ref(&mut errors, rule, bindings, &created, channel);` → `check_channel_ref(&mut errors, rule, &created, channel);` and `check_role_ref(&mut errors, rule, bindings, &created, role);` → `check_role_ref(&mut errors, rule, &created, role);`
- In the `PostPanel` arm: `check_channel_ref(&mut errors, rule, bindings, &created, channel);` → `check_channel_ref(&mut errors, rule, &created, channel);`

Everything else in the function body is unchanged.

- [ ] **Step 3: Add `validate_bindings` and the composing `validate`.** In `validate.rs`, add (after `validate_structural`):

```rust
pub fn validate_bindings(
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();
    for rule in &ruleset.rules {
        for action in &rule.actions {
            match action {
                ActionSpec::GrantRole { role, .. } => {
                    check_role_binding(&mut errors, rule, bindings, role);
                }
                ActionSpec::UpsertOverwrite { channel, target, .. } => {
                    check_channel_binding(&mut errors, rule, bindings, channel);
                    if let OverwriteTargetSpec::Role(role) = target {
                        check_role_binding(&mut errors, rule, bindings, role);
                    }
                }
                ActionSpec::PostPanel { channel, .. } => {
                    check_channel_binding(&mut errors, rule, bindings, channel);
                }
                _ => {}
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub fn validate(
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();
    if let Err(structural) = validate_structural(ruleset) {
        errors.extend(structural);
    }
    if let Err(binding) = validate_bindings(ruleset, bindings) {
        errors.extend(binding);
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn check_role_binding(
    errors: &mut Vec<ValidationError>,
    rule: &InteractionRule,
    bindings: &ResourceBindingMap,
    role: &RoleRef,
) {
    if let RoleRef::Existing(key) = role {
        if !bindings.role_bindings.contains_key(key) {
            errors.push(ValidationError::UnknownRoleRef {
                rule: rule.key.clone(),
                role: key.clone(),
            });
        }
    }
}

fn check_channel_binding(
    errors: &mut Vec<ValidationError>,
    rule: &InteractionRule,
    bindings: &ResourceBindingMap,
    channel: &ChannelRef,
) {
    if let ChannelRef::Existing(key) = channel {
        if !bindings.channel_bindings.contains_key(key) {
            errors.push(ValidationError::UnknownChannelRef {
                rule: rule.key.clone(),
                channel: key.clone(),
            });
        }
    }
}
```

`validate` collects BOTH layers' errors (behavior-preserving: existing multi-error reporting is kept; tests use `.contains(&err)`, order-independent).

- [ ] **Step 4: Export the new functions.** In `crates/automation-core/src/lib.rs`, change the validate re-export (line 26) to:

```rust
pub use validate::{validate, validate_bindings, validate_structural, ValidationError};
```

- [ ] **Step 5: Add a structural-vs-binding regression test.** Append to `validate.rs`'s `#[cfg(test)] mod tests` (a test module already exists in the crate's test suite; if `validate.rs` has none, add one). Put this in `crates/automation-core/tests/validate_split.rs` instead (integration test) to avoid touching validate.rs internals:

```rust
use automation_core::{validate, validate_bindings, validate_structural, ValidationError};
use automation_state::{
    ActionSpec, ActionTarget, InteractionRule, InteractionRuleSet, PanelSpec, RoleRef, TriggerSpec,
};
use automation_state::{ButtonRoute, ButtonSpec};
use desired_state::ResourceKey;
use resource_resolution::ResourceBindingMap;

fn verify_rule(role_key: &str) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "p".to_string(),
            channel: ResourceKey("c".to_string()),
            content: "x".to_string(),
            buttons: vec![ButtonSpec {
                label: "V".to_string(),
                route: ButtonRoute::Static {
                    key: "b".to_string(),
                },
            }],
        }],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "r".to_string(),
            trigger: TriggerSpec::ButtonClick {
                component: "b".to_string(),
            },
            actions: vec![ActionSpec::GrantRole {
                role: RoleRef::Existing(ResourceKey(role_key.to_string())),
                target: ActionTarget::Actor,
            }],
        }],
    }
}

#[test]
fn structural_passes_without_bindings_binding_layer_flags_missing() {
    let ruleset = verify_rule("member");
    assert!(validate_structural(&ruleset).is_ok());
    let empty = ResourceBindingMap::default();
    let errors = validate_bindings(&ruleset, &empty).unwrap_err();
    assert!(errors.contains(&ValidationError::UnknownRoleRef {
        rule: "r".to_string(),
        role: ResourceKey("member".to_string()),
    }));
    assert!(validate(&ruleset, &empty).is_err());
    let mut bound = ResourceBindingMap::default();
    bound
        .role_bindings
        .insert(ResourceKey("member".to_string()), discord_model::RoleId(9));
    assert!(validate(&ruleset, &bound).is_ok());
}
```

Add `discord-model` to automation-core's `[dev-dependencies]` if not already present (it is a normal dependency, so `discord_model::RoleId` is available in tests).

- [ ] **Step 6: Run gates.**

Run: `$HOME/.cargo/bin/cargo test -p automation-core`
Expected: PASS — all pre-existing validate tests still green (composition preserves behavior) + the new split test.

Run: `$HOME/.cargo/bin/cargo clippy -p automation-core --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 7: Commit.**

```bash
git add crates/automation-core/src/validate.rs crates/automation-core/src/lib.rs crates/automation-core/tests/validate_split.rs
git commit -m "feat(automation-core): split validate into structural + bindings"
```

---

## Task 2 — Crate skeleton + key/version types

**Files:**
- Modify: `Cargo.toml` (workspace members)
- Create: `crates/automation-ruleset/Cargo.toml`, `src/lib.rs`, `src/key.rs`, `src/version.rs`

**Interfaces:**
- Produces: `RuleSetKey` (1-64, `[A-Za-z0-9_-]`, validating serde), `RuleSetVersionId(NonZeroU32)`, `RuleSetSchemaVersion(NonZeroU32)`, `CURRENT_RULESET_SCHEMA_VERSION`.

- [ ] **Step 1: Register the crate.** In the workspace `Cargo.toml`, add to `members` (after `"crates/automation-instance-postgres",`):

```toml
    "crates/automation-ruleset",
```

- [ ] **Step 2: Create `crates/automation-ruleset/Cargo.toml`.**

```toml
[package]
name = "automation-ruleset"
version = "0.1.0"
edition.workspace = true

[dependencies]
automation-state = { path = "../automation-state" }
automation-core = { path = "../automation-core" }
discord-model = { path = "../discord-model" }
serde = { workspace = true }
serde_json = { workspace = true }
sha2 = "0.10"

[dev-dependencies]
futures = "0.3"
```

- [ ] **Step 3: Create `src/key.rs` (failing tests first).**

```rust
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

const MAX_LEN: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuleSetKey(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleSetKeyError {
    Empty,
    TooLong,
    InvalidChar,
}

impl RuleSetKey {
    pub fn parse(value: &str) -> Result<Self, RuleSetKeyError> {
        if value.is_empty() {
            return Err(RuleSetKeyError::Empty);
        }
        if value.len() > MAX_LEN {
            return Err(RuleSetKeyError::TooLong);
        }
        if !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(RuleSetKeyError::InvalidChar);
        }
        Ok(RuleSetKey(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RuleSetKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for RuleSetKey {
    type Err = RuleSetKeyError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        RuleSetKey::parse(value)
    }
}

impl AsRef<str> for RuleSetKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for RuleSetKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RuleSetKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        RuleSetKey::parse(&value).map_err(|e| serde::de::Error::custom(format!("{e:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_keys_parse() {
        assert_eq!(
            RuleSetKey::parse("studyroom_demo").unwrap().as_str(),
            "studyroom_demo"
        );
        assert!(RuleSetKey::parse(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn invalid_keys_rejected() {
        assert_eq!(RuleSetKey::parse(""), Err(RuleSetKeyError::Empty));
        assert_eq!(RuleSetKey::parse(&"a".repeat(65)), Err(RuleSetKeyError::TooLong));
        assert_eq!(RuleSetKey::parse("bad key"), Err(RuleSetKeyError::InvalidChar));
    }

    #[test]
    fn deserialize_rejects_invalid() {
        assert!(serde_json::from_str::<RuleSetKey>(r#""ok_key""#).is_ok());
        assert!(serde_json::from_str::<RuleSetKey>(r#""bad key""#).is_err());
    }
}
```

- [ ] **Step 4: Create `src/version.rs`.**

```rust
use std::fmt;
use std::num::NonZeroU32;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuleSetVersionId(NonZeroU32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleSetVersionError {
    Zero,
    Overflow,
}

impl RuleSetVersionId {
    pub const FIRST: RuleSetVersionId = RuleSetVersionId(NonZeroU32::MIN);

    pub fn new(value: u32) -> Result<Self, RuleSetVersionError> {
        NonZeroU32::new(value)
            .map(RuleSetVersionId)
            .ok_or(RuleSetVersionError::Zero)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }

    pub fn next(self) -> Result<Self, RuleSetVersionError> {
        self.0
            .get()
            .checked_add(1)
            .and_then(NonZeroU32::new)
            .map(RuleSetVersionId)
            .ok_or(RuleSetVersionError::Overflow)
    }
}

impl fmt::Display for RuleSetVersionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.get())
    }
}

impl Serialize for RuleSetVersionId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.0.get())
    }
}

impl<'de> Deserialize<'de> for RuleSetVersionId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = u32::deserialize(deserializer)?;
        RuleSetVersionId::new(value).map_err(|e| serde::de::Error::custom(format!("{e:?}")))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuleSetSchemaVersion(NonZeroU32);

impl RuleSetSchemaVersion {
    pub fn new(value: u32) -> Result<Self, RuleSetVersionError> {
        NonZeroU32::new(value)
            .map(RuleSetSchemaVersion)
            .ok_or(RuleSetVersionError::Zero)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

impl Serialize for RuleSetSchemaVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.0.get())
    }
}

impl<'de> Deserialize<'de> for RuleSetSchemaVersion {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = u32::deserialize(deserializer)?;
        RuleSetSchemaVersion::new(value).map_err(|e| serde::de::Error::custom(format!("{e:?}")))
    }
}

pub const CURRENT_RULESET_SCHEMA_VERSION: RuleSetSchemaVersion =
    RuleSetSchemaVersion(NonZeroU32::MIN);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_zero_rejected() {
        assert_eq!(RuleSetVersionId::new(0), Err(RuleSetVersionError::Zero));
        assert_eq!(RuleSetSchemaVersion::new(0), Err(RuleSetVersionError::Zero));
        assert!(serde_json::from_str::<RuleSetVersionId>("0").is_err());
        assert!(serde_json::from_str::<RuleSetSchemaVersion>("0").is_err());
    }

    #[test]
    fn first_and_next() {
        assert_eq!(RuleSetVersionId::FIRST.get(), 1);
        assert_eq!(RuleSetVersionId::FIRST.next().unwrap().get(), 2);
    }

    #[test]
    fn next_overflow_is_error() {
        let max = RuleSetVersionId::new(u32::MAX).unwrap();
        assert_eq!(max.next(), Err(RuleSetVersionError::Overflow));
    }

    #[test]
    fn current_schema_is_one() {
        assert_eq!(CURRENT_RULESET_SCHEMA_VERSION.get(), 1);
    }
}
```

- [ ] **Step 5: Create `src/lib.rs` (partial — grows in later tasks).**

```rust
pub mod key;
pub mod version;

pub use key::{RuleSetKey, RuleSetKeyError};
pub use version::{
    RuleSetSchemaVersion, RuleSetVersionError, RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION,
};
```

- [ ] **Step 6: Gates.**

Run: `$HOME/.cargo/bin/cargo test -p automation-ruleset`
Expected: PASS (key + version tests). `NonZeroU32::MIN` is a stable const (= 1).

Run: `$HOME/.cargo/bin/cargo clippy -p automation-ruleset --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 7: Commit.**

```bash
git add Cargo.toml Cargo.lock crates/automation-ruleset/
git commit -m "feat(automation-ruleset): crate skeleton + RuleSetKey/version types"
```

---

## Task 3 — Content hash + canonicalizer + hasher seam

**Files:**
- Create: `crates/automation-ruleset/src/hash.rs`
- Modify: `crates/automation-ruleset/src/lib.rs`

**Interfaces:**
- Produces: `RuleSetContentHash([u8; 32])` (hex serde), `content_hash(RuleSetSchemaVersion, &InteractionRuleSet) -> Result<RuleSetContentHash, RuleSetHashError>`, `RuleSetHasher` trait, `Sha256RuleSetHasher`. Canonicalize sorts object keys recursively, **preserves array order** (verified empirically: reordered fields/map keys → same hash; swapped actions → different hash).

- [ ] **Step 1: Create `src/hash.rs`.**

```rust
use std::collections::BTreeMap;
use std::fmt;

use automation_state::InteractionRuleSet;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::version::RuleSetSchemaVersion;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RuleSetContentHash([u8; 32]);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleSetHashError {
    Serialization(String),
}

impl RuleSetContentHash {
    pub fn to_hex(self) -> String {
        let mut out = String::with_capacity(64);
        for byte in self.0 {
            out.push_str(&format!("{byte:02x}"));
        }
        out
    }

    pub fn parse_hex(value: &str) -> Option<Self> {
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
            return None;
        }
        let mut bytes = [0u8; 32];
        for (i, chunk) in value.as_bytes().chunks(2).enumerate() {
            let hi = (chunk[0] as char).to_digit(16)? as u8;
            let lo = (chunk[1] as char).to_digit(16)? as u8;
            bytes[i] = (hi << 4) | lo;
        }
        Some(RuleSetContentHash(bytes))
    }
}

impl fmt::Display for RuleSetContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl Serialize for RuleSetContentHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for RuleSetContentHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        RuleSetContentHash::parse_hex(&value)
            .ok_or_else(|| serde::de::Error::custom("expected 64-char lowercase hex"))
    }
}

#[derive(Serialize)]
struct RuleSetHashInput<'a> {
    schema_version: RuleSetSchemaVersion,
    definition: &'a InteractionRuleSet,
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted: BTreeMap<String, Value> = BTreeMap::new();
            for (k, v) in map {
                sorted.insert(k, canonicalize(v));
            }
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize).collect()),
        other => other,
    }
}

pub fn content_hash(
    schema_version: RuleSetSchemaVersion,
    definition: &InteractionRuleSet,
) -> Result<RuleSetContentHash, RuleSetHashError> {
    let input = RuleSetHashInput {
        schema_version,
        definition,
    };
    let value =
        serde_json::to_value(&input).map_err(|e| RuleSetHashError::Serialization(e.to_string()))?;
    let bytes = serde_json::to_vec(&canonicalize(value))
        .map_err(|e| RuleSetHashError::Serialization(e.to_string()))?;
    let digest = Sha256::digest(&bytes);
    Ok(RuleSetContentHash(digest.into()))
}

pub trait RuleSetHasher {
    fn hash(
        &self,
        schema_version: RuleSetSchemaVersion,
        definition: &InteractionRuleSet,
    ) -> Result<RuleSetContentHash, RuleSetHashError>;
}

#[derive(Default)]
pub struct Sha256RuleSetHasher;

impl RuleSetHasher for Sha256RuleSetHasher {
    fn hash(
        &self,
        schema_version: RuleSetSchemaVersion,
        definition: &InteractionRuleSet,
    ) -> Result<RuleSetContentHash, RuleSetHashError> {
        content_hash(schema_version, definition)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::CURRENT_RULESET_SCHEMA_VERSION;
    use automation_state::{
        ActionSpec, ActionTarget, InteractionRule, InteractionRuleSet, RoleRef, TriggerSpec,
    };
    use desired_state::ResourceKey;

    fn ruleset(actions: Vec<ActionSpec>) -> InteractionRuleSet {
        InteractionRuleSet {
            version: 1,
            panels: vec![],
            modals: vec![],
            rules: vec![InteractionRule {
                key: "r".to_string(),
                trigger: TriggerSpec::InstanceAction {
                    action: "join".to_string(),
                },
                actions,
            }],
        }
    }

    fn grant() -> ActionSpec {
        ActionSpec::GrantRole {
            role: RoleRef::Existing(ResourceKey("m".to_string())),
            target: ActionTarget::Actor,
        }
    }

    fn respond() -> ActionSpec {
        ActionSpec::RespondEphemeral {
            content: "hi".to_string(),
        }
    }

    #[test]
    fn same_definition_same_hash() {
        let a = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &ruleset(vec![grant(), respond()])).unwrap();
        let b = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &ruleset(vec![grant(), respond()])).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn action_order_changes_hash() {
        let a = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &ruleset(vec![grant(), respond()])).unwrap();
        let b = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &ruleset(vec![respond(), grant()])).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn schema_version_changes_hash() {
        let v1 = RuleSetSchemaVersion::new(1).unwrap();
        let v2 = RuleSetSchemaVersion::new(2).unwrap();
        let a = content_hash(v1, &ruleset(vec![grant()])).unwrap();
        let b = content_hash(v2, &ruleset(vec![grant()])).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn hex_roundtrip_and_validation() {
        let h = content_hash(CURRENT_RULESET_SCHEMA_VERSION, &ruleset(vec![grant()])).unwrap();
        let hex = h.to_hex();
        assert_eq!(hex.len(), 64);
        assert_eq!(RuleSetContentHash::parse_hex(&hex), Some(h));
        assert_eq!(RuleSetContentHash::parse_hex("XYZ"), None);
        assert_eq!(RuleSetContentHash::parse_hex(&hex.to_uppercase()), None);
    }
}
```

Add `desired-state` to `[dev-dependencies]` (used by hash tests for `ResourceKey`). Update `Cargo.toml` dev-deps:

```toml
[dev-dependencies]
futures = "0.3"
desired-state = { path = "../desired-state" }
```

- [ ] **Step 2: Wire module.** In `src/lib.rs`, add:

```rust
pub mod hash;

pub use hash::{content_hash, RuleSetContentHash, RuleSetHashError, RuleSetHasher, Sha256RuleSetHasher};
```

- [ ] **Step 3: Gates.**

Run: `$HOME/.cargo/bin/cargo test -p automation-ruleset hash`
Expected: PASS. (Empirically pre-verified: reorder-stable, array-order-sensitive.)

Run: `$HOME/.cargo/bin/cargo clippy -p automation-ruleset --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 4: Commit.**

```bash
git add crates/automation-ruleset/
git commit -m "feat(automation-ruleset): content hash + canonicalizer + hasher seam"
```

---

## Task 4 — Model + Store trait + errors

**Files:**
- Create: `crates/automation-ruleset/src/model.rs`
- Create: `crates/automation-ruleset/src/store.rs` (trait + types; InMemory impl in Task 5)
- Modify: `crates/automation-ruleset/src/lib.rs`

**Interfaces:**
- Produces: `RuleSetVersion`, `RuleSetActivation`, `RuleSetStore` trait, `PublishRuleSetRequest`, `PublishOutcome`, `RuleSetStoreError`.
- Consumes: `automation_core::ValidationError`.

- [ ] **Step 1: Create `src/model.rs`.**

```rust
use automation_state::InteractionRuleSet;
use discord_model::{GuildId, UserId};
use serde::{Deserialize, Serialize};

use crate::hash::RuleSetContentHash;
use crate::key::RuleSetKey;
use crate::version::{RuleSetSchemaVersion, RuleSetVersionId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSetVersion {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub version: RuleSetVersionId,
    pub schema_version: RuleSetSchemaVersion,
    pub definition: InteractionRuleSet,
    pub content_hash: RuleSetContentHash,
    pub created_by: UserId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleSetActivation {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub active_version: RuleSetVersionId,
}
```

- [ ] **Step 2: Create `src/store.rs` (trait + types only).**

```rust
use automation_core::ValidationError;
use automation_state::InteractionRuleSet;
use discord_model::{GuildId, UserId};

use crate::key::RuleSetKey;
use crate::model::{RuleSetActivation, RuleSetVersion};
use crate::version::RuleSetVersionId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishRuleSetRequest {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub definition: InteractionRuleSet,
    pub created_by: UserId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishOutcome {
    Created(RuleSetVersion),
    Reused(RuleSetVersion),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleSetStoreError {
    InvalidDefinition(Vec<ValidationError>),
    VersionNotFound,
    VersionOverflow,
    HashCollision,
    Canonicalization(String),
    Backend(String),
}

#[allow(async_fn_in_trait)]
pub trait RuleSetStore {
    async fn publish(
        &self,
        request: PublishRuleSetRequest,
    ) -> Result<PublishOutcome, RuleSetStoreError>;

    async fn get_version(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError>;

    async fn list_versions(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Vec<RuleSetVersion>, RuleSetStoreError>;

    async fn activate(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<RuleSetActivation, RuleSetStoreError>;

    async fn active(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError>;
}
```

- [ ] **Step 3: Wire modules.** In `src/lib.rs`, add:

```rust
pub mod model;
pub mod store;

pub use model::{RuleSetActivation, RuleSetVersion};
pub use store::{PublishOutcome, PublishRuleSetRequest, RuleSetStore, RuleSetStoreError};
```

- [ ] **Step 4: Gates (compile check).**

Run: `$HOME/.cargo/bin/cargo build -p automation-ruleset`
Expected: PASS.

Run: `$HOME/.cargo/bin/cargo clippy -p automation-ruleset --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 5: Commit.**

```bash
git add crates/automation-ruleset/
git commit -m "feat(automation-ruleset): RuleSetVersion/Activation model + Store trait"
```

---

## Task 5 — InMemoryRuleSetStore + store tests

**Files:**
- Modify: `crates/automation-ruleset/src/store.rs` (add impl)
- Modify: `crates/automation-ruleset/src/lib.rs` (export)

**Interfaces:**
- Produces: `InMemoryRuleSetStore<H: RuleSetHasher>` with `Default` (= `Sha256RuleSetHasher`) and `new(hasher)`. Atomic publish (single Mutex critical section).

- [ ] **Step 1: Add the impl to `src/store.rs`.** Add imports at the top (do NOT re-import `RuleSetVersionId` — Task 4 already imports it; `RuleSetSchemaVersion` is not used here):

```rust
use std::collections::BTreeMap;
use std::sync::Mutex;

use automation_core::validate_structural;

use crate::hash::{RuleSetHasher, Sha256RuleSetHasher};
use crate::version::CURRENT_RULESET_SCHEMA_VERSION;
```

Then append:

```rust
#[derive(Default)]
struct GuildRuleSet {
    versions: BTreeMap<RuleSetVersionId, RuleSetVersion>,
    active: Option<RuleSetVersionId>,
}

pub struct InMemoryRuleSetStore<H: RuleSetHasher = Sha256RuleSetHasher> {
    hasher: H,
    inner: Mutex<BTreeMap<(GuildId, RuleSetKey), GuildRuleSet>>,
}

impl Default for InMemoryRuleSetStore<Sha256RuleSetHasher> {
    fn default() -> Self {
        Self::new(Sha256RuleSetHasher)
    }
}

impl<H: RuleSetHasher> InMemoryRuleSetStore<H> {
    pub fn new(hasher: H) -> Self {
        Self {
            hasher,
            inner: Mutex::new(BTreeMap::new()),
        }
    }
}

impl<H: RuleSetHasher> RuleSetStore for InMemoryRuleSetStore<H> {
    async fn publish(
        &self,
        request: PublishRuleSetRequest,
    ) -> Result<PublishOutcome, RuleSetStoreError> {
        validate_structural(&request.definition)
            .map_err(RuleSetStoreError::InvalidDefinition)?;
        let schema_version = CURRENT_RULESET_SCHEMA_VERSION;
        let content_hash = self
            .hasher
            .hash(schema_version, &request.definition)
            .map_err(|e| match e {
                crate::hash::RuleSetHashError::Serialization(m) => {
                    RuleSetStoreError::Canonicalization(m)
                }
            })?;
        let mut guilds = self.inner.lock().unwrap();
        let entry = guilds
            .entry((request.guild_id, request.ruleset_key.clone()))
            .or_default();
        for existing in entry.versions.values() {
            if existing.content_hash == content_hash {
                if existing.definition == request.definition {
                    return Ok(PublishOutcome::Reused(existing.clone()));
                }
                return Err(RuleSetStoreError::HashCollision);
            }
        }
        let version = match entry.versions.keys().next_back() {
            Some(max) => max.next().map_err(|_| RuleSetStoreError::VersionOverflow)?,
            None => RuleSetVersionId::FIRST,
        };
        let record = RuleSetVersion {
            guild_id: request.guild_id,
            ruleset_key: request.ruleset_key,
            version,
            schema_version,
            definition: request.definition,
            content_hash,
            created_by: request.created_by,
        };
        entry.versions.insert(version, record.clone());
        Ok(PublishOutcome::Created(record))
    }

    async fn get_version(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
        let guilds = self.inner.lock().unwrap();
        Ok(guilds
            .get(&(guild_id, key.clone()))
            .and_then(|entry| entry.versions.get(&version))
            .cloned())
    }

    async fn list_versions(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Vec<RuleSetVersion>, RuleSetStoreError> {
        let guilds = self.inner.lock().unwrap();
        Ok(guilds
            .get(&(guild_id, key.clone()))
            .map(|entry| entry.versions.values().cloned().collect())
            .unwrap_or_default())
    }

    async fn activate(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<RuleSetActivation, RuleSetStoreError> {
        let mut guilds = self.inner.lock().unwrap();
        let entry = guilds
            .get_mut(&(guild_id, key.clone()))
            .filter(|entry| entry.versions.contains_key(&version))
            .ok_or(RuleSetStoreError::VersionNotFound)?;
        entry.active = Some(version);
        Ok(RuleSetActivation {
            guild_id,
            ruleset_key: key.clone(),
            active_version: version,
        })
    }

    async fn active(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
        let guilds = self.inner.lock().unwrap();
        Ok(guilds.get(&(guild_id, key.clone())).and_then(|entry| {
            entry
                .active
                .and_then(|version| entry.versions.get(&version))
                .cloned()
        }))
    }
}
```

The local `let schema_version = CURRENT_RULESET_SCHEMA_VERSION;` is passed to the hasher and stored in the record; its type (`RuleSetSchemaVersion`) is inferred, so no type import is needed in this file.

- [ ] **Step 2: Export.** In `src/lib.rs`, extend the store re-export line:

```rust
pub use store::{
    InMemoryRuleSetStore, PublishOutcome, PublishRuleSetRequest, RuleSetStore, RuleSetStoreError,
};
```

- [ ] **Step 3: Add the store test suite.** Create `crates/automation-ruleset/tests/store.rs`:

```rust
use automation_ruleset::{
    content_hash, InMemoryRuleSetStore, PublishOutcome, PublishRuleSetRequest, RuleSetContentHash,
    RuleSetHashError, RuleSetHasher, RuleSetKey, RuleSetSchemaVersion, RuleSetStore,
    RuleSetStoreError, RuleSetVersionId, CURRENT_RULESET_SCHEMA_VERSION,
};
use automation_state::{
    ActionSpec, ActionTarget, InstanceRef, InteractionRule, InteractionRuleSet, RoleRef,
    TriggerSpec,
};
use discord_model::{GuildId, UserId};
use futures::executor::block_on;

fn ruleset(content: &str) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "r".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "join".to_string(),
            },
            actions: vec![
                ActionSpec::GrantRole {
                    role: RoleRef::Instance {
                        instance: InstanceRef::Event,
                        alias: "member_role".to_string(),
                    },
                    target: ActionTarget::Actor,
                },
                ActionSpec::RespondEphemeral {
                    content: content.to_string(),
                },
            ],
        }],
    }
}

fn req(guild: u64, key: &str, def: InteractionRuleSet) -> PublishRuleSetRequest {
    PublishRuleSetRequest {
        guild_id: GuildId(guild),
        ruleset_key: RuleSetKey::parse(key).unwrap(),
        definition: def,
        created_by: UserId(1),
    }
}

fn key(k: &str) -> RuleSetKey {
    RuleSetKey::parse(k).unwrap()
}

#[test]
fn first_publish_creates_v1_reuse_and_change() {
    let store = InMemoryRuleSetStore::default();
    let a = block_on(store.publish(req(7, "studyroom", ruleset("a")))).unwrap();
    let v1 = match a {
        PublishOutcome::Created(v) => v,
        PublishOutcome::Reused(_) => panic!("expected Created"),
    };
    assert_eq!(v1.version, RuleSetVersionId::FIRST);

    let again = block_on(store.publish(req(7, "studyroom", ruleset("a")))).unwrap();
    assert!(matches!(again, PublishOutcome::Reused(ref v) if v.version == RuleSetVersionId::FIRST));
    assert_eq!(
        block_on(store.list_versions(GuildId(7), &key("studyroom")))
            .unwrap()
            .len(),
        1
    );

    let changed = block_on(store.publish(req(7, "studyroom", ruleset("b")))).unwrap();
    assert!(matches!(changed, PublishOutcome::Created(ref v) if v.version.get() == 2));
}

#[test]
fn guild_and_key_isolation() {
    let store = InMemoryRuleSetStore::default();
    for (g, k) in [(7, "studyroom"), (8, "studyroom"), (7, "ticket")] {
        let out = block_on(store.publish(req(g, k, ruleset("x")))).unwrap();
        assert!(matches!(out, PublishOutcome::Created(ref v) if v.version == RuleSetVersionId::FIRST));
    }
}

#[test]
fn publish_does_not_change_activation() {
    let store = InMemoryRuleSetStore::default();
    block_on(store.publish(req(7, "studyroom", ruleset("a")))).unwrap();
    assert!(block_on(store.active(GuildId(7), &key("studyroom")))
        .unwrap()
        .is_none());
}

#[test]
fn activate_missing_then_activate_and_rollback() {
    let store = InMemoryRuleSetStore::default();
    assert_eq!(
        block_on(store.activate(GuildId(7), &key("studyroom"), RuleSetVersionId::FIRST)).unwrap_err(),
        RuleSetStoreError::VersionNotFound
    );
    block_on(store.publish(req(7, "studyroom", ruleset("a")))).unwrap();
    block_on(store.publish(req(7, "studyroom", ruleset("b")))).unwrap();
    let v1 = RuleSetVersionId::FIRST;
    let v2 = RuleSetVersionId::new(2).unwrap();

    let act = block_on(store.activate(GuildId(7), &key("studyroom"), v2)).unwrap();
    assert_eq!(act.active_version, v2);
    assert_eq!(
        block_on(store.active(GuildId(7), &key("studyroom"))).unwrap().unwrap().version,
        v2
    );
    block_on(store.activate(GuildId(7), &key("studyroom"), v1)).unwrap();
    assert_eq!(
        block_on(store.active(GuildId(7), &key("studyroom"))).unwrap().unwrap().version,
        v1
    );
}

#[test]
fn invalid_definition_rejected_before_version() {
    let store = InMemoryRuleSetStore::default();
    let mut bad = ruleset("a");
    bad.rules[0].key = String::new();
    bad.rules.push(InteractionRule {
        key: String::new(),
        trigger: TriggerSpec::InstanceAction {
            action: "join".to_string(),
        },
        actions: vec![ActionSpec::RespondEphemeral {
            content: "y".to_string(),
        }],
    });
    let err = block_on(store.publish(req(7, "studyroom", bad))).unwrap_err();
    assert!(matches!(err, RuleSetStoreError::InvalidDefinition(_)));
    assert!(block_on(store.list_versions(GuildId(7), &key("studyroom")))
        .unwrap()
        .is_empty());
}

struct FixedHasher;

impl RuleSetHasher for FixedHasher {
    fn hash(
        &self,
        _schema_version: RuleSetSchemaVersion,
        _definition: &InteractionRuleSet,
    ) -> Result<RuleSetContentHash, RuleSetHashError> {
        Ok(RuleSetContentHash::parse_hex(&"ab".repeat(32)).unwrap())
    }
}

#[test]
fn same_hash_different_definition_is_collision() {
    let store = InMemoryRuleSetStore::new(FixedHasher);
    block_on(store.publish(req(7, "studyroom", ruleset("a")))).unwrap();
    let err = block_on(store.publish(req(7, "studyroom", ruleset("b")))).unwrap_err();
    assert_eq!(err, RuleSetStoreError::HashCollision);
}

#[test]
fn returned_artifact_clone_does_not_mutate_store() {
    let store = InMemoryRuleSetStore::default();
    let out = block_on(store.publish(req(7, "studyroom", ruleset("a")))).unwrap();
    let mut v = match out {
        PublishOutcome::Created(v) => v,
        PublishOutcome::Reused(v) => v,
    };
    v.created_by = UserId(999);
    let stored = block_on(store.get_version(GuildId(7), &key("studyroom"), RuleSetVersionId::FIRST))
        .unwrap()
        .unwrap();
    assert_eq!(stored.created_by, UserId(1));
}

#[test]
fn schema_version_hash_relative_check() {
    let a = content_hash(RuleSetSchemaVersion::new(1).unwrap(), &ruleset("a")).unwrap();
    let b = content_hash(RuleSetSchemaVersion::new(2).unwrap(), &ruleset("a")).unwrap();
    assert_ne!(a, b);
    let _ = CURRENT_RULESET_SCHEMA_VERSION;
}
```

> Concurrency tests (spec §8 #16/#17) require a threaded executor; `futures::executor::block_on` is single-threaded. Implement them with `std::thread` + `std::sync::Arc<InMemoryRuleSetStore>` (the store is `Sync` via `Mutex`): spawn N threads each publishing, join, then assert exactly one `Created` for same-content and all-unique version IDs for distinct-content — **without** asserting which thread got which number. Add these as a separate step if the threaded harness is desired; otherwise the Mutex critical section (single lock across hash-check + version-alloc + insert) already guarantees the invariant structurally.

- [ ] **Step 4: Gates.**

Run: `$HOME/.cargo/bin/cargo test -p automation-ruleset`
Expected: PASS (all store + hash + key + version tests).

Run: `$HOME/.cargo/bin/cargo clippy -p automation-ruleset --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 5: Commit.**

```bash
git add crates/automation-ruleset/
git commit -m "feat(automation-ruleset): InMemoryRuleSetStore + store tests"
```

---

## Task 6 — Safety + dependency guards + final gate

**Files:**
- Create: `crates/automation-ruleset/tests/no_ai_gateway.rs`
- Create: `crates/automation-ruleset/tests/dependency_guard.rs`

- [ ] **Step 1: no_ai_gateway guard** (16a pattern — reads its own Cargo.toml, asserts no `ai-gateway`). Create `tests/no_ai_gateway.rs`:

```rust
#[test]
fn crate_does_not_depend_on_ai_gateway() {
    let manifest = include_str!("../Cargo.toml");
    assert!(
        !manifest.contains("ai-gateway"),
        "automation-ruleset must not depend on ai-gateway (event-time LLM forbidden)"
    );
}
```

- [ ] **Step 2: dependency guard** (forbid `automation-core → automation-ruleset`). Create `tests/dependency_guard.rs`:

```rust
#[test]
fn automation_core_does_not_depend_on_automation_ruleset() {
    let manifest = include_str!("../../automation-core/Cargo.toml");
    assert!(
        !manifest.contains("automation-ruleset"),
        "automation-core must not depend on automation-ruleset (cyclic dependency)"
    );
}
```

- [ ] **Step 3: Full workspace gate.**

```bash
$HOME/.cargo/bin/cargo build && \
$HOME/.cargo/bin/cargo test && \
$HOME/.cargo/bin/cargo clippy --all-targets -- -D warnings && \
$HOME/.cargo/bin/cargo fmt --all -- --check
```
Expected: whole workspace green.

- [ ] **Step 4: Commit.**

```bash
git add crates/automation-ruleset/tests/
git commit -m "feat(automation-ruleset): no-ai-gateway + dependency guards"
```

---

## Self-Review

- **Spec coverage:** §2 types → Task 2 (key/version) + Task 3 (hash) + Task 4 (model). §3 canonicalizer + hasher seam → Task 3. §4 Store trait + publish contract (validate→hash→dedup/version, publish≠activate) → Task 4 + Task 5. §5 InMemory atomicity → Task 5 (single Mutex section). §최상위 validate 3-split → Task 1. §7 custom_id boundary → documented (RuleSetKey 1-64, no route token). §8 tests → Tasks 2-5 map to the 20-item set (key valid/invalid #1, version-0 #2, Created/Reused/change #3-6, action-order #7, schema-version #9, guild/key isolation #10-11, publish≠activate #12, activate-missing/rollback #13-15, collision #18, clone-isolation #19); reorder-same-hash #8 verified empirically + relative in Task 3; concurrency #16/#17 noted as threaded step; no_ai_gateway #20 → Task 6. §최상위 dependency guard → Task 6.
- **Placeholder scan:** none — all code complete. Canonicalizer empirically verified (reorder-stable, array-order-sensitive) before this plan.
- **Type consistency:** `content_hash(RuleSetSchemaVersion, &InteractionRuleSet) -> Result<_, RuleSetHashError>` used identically in `Sha256RuleSetHasher` and publish (mapped to `Canonicalization`). `RuleSetVersionId::FIRST`/`next()` used in publish's allocation. `validate_structural` (Task 1) consumed by publish (Task 5). `InMemoryRuleSetStore<H = Sha256RuleSetHasher>` default type param enables `::default()`.
- **Behavior preservation:** Task 1's `validate` collects both layers' errors (existing tests use `.contains`, order-independent) — all pre-existing automation-core validate tests must stay green (Step 6 gate).
