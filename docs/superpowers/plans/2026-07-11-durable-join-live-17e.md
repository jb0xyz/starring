# Phase 17e — Durable Dynamic Join Live Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the interaction-smoke tool store automation instances in PostgreSQL with OS-random InstanceIds, defer the join interaction (ACK) before any DB lookup, and prove a restarted bot process still serves the existing join button while minting new rooms with distinct random ids.

**Architecture:** Five focused changes. (A) A pure base32 encoder + `RandomInstanceIdGenerator` in the tool, plus one additive `Entropy` variant on `InstanceIdGenerationError`. (B) A `TooLong` length guard on `encode_instance_action` so the codec can never emit a >100-char custom_id. (C) Reorder `handle_event` so `DeferEphemeral` ACKs before the PostgreSQL instance lookup, routing post-defer resolution failures through the single existing failure-edit path. (D) Wire the tool to Postgres-or-die startup. (E) Switch the tool's join rule to the deferred contract and run the live restart-survival demo.

**Tech Stack:** Rust 2021, sqlx 0.8.6 (runtime query, PgPool, migrate), getrandom 0.2, twilight 0.17, serde. Tests use `futures::executor::block_on`; DB-independent unit tests only (no live DB in CI).

## Global Constraints

- **No comments** anywhere (`//`, `///`, `//!` all forbidden) — match existing files, which carry none.
- **Cargo path:** all gate commands use `$HOME/.cargo/bin/cargo`.
- **Gates (every task):** `cargo build` · `cargo test` (DB-independent) · `cargo clippy --all-targets -- -D warnings` · `cargo fmt --all -- --check`.
- **Crate-modification scope (grounded — no crate is claimed "untouched" that this plan modifies):**
  - `automation-state`: **untouched**.
  - `automation-instance-postgres`: **untouched**.
  - `automation-instance`: **one additive, backend-agnostic enum variant** on `InstanceIdGenerationError` (`Entropy`). **No new dependency** — `getrandom` and `sqlx` are NOT added here; `getrandom` is added to the tool only. The enum stays `Copy` (unit variant).
  - `automation-core`: **only** `handle_event` ordering (+ one private helper).
  - `automation-runtime`: **only** the `encode_instance_action` signature and its single caller.
  - `tools/interaction-smoke`: new `random_instance_id` module + Postgres/generator wiring + join-rule change.
- **Safety invariant:** no event-time LLM. `crates/automation-*/tests/no_ai_gateway.rs` must stay green. AI designs at install time; runtime executes stored rules deterministically.
- **Postgres-or-die:** DB connect/migration failure ⇒ the process must not start. **No InMemory fallback** anywhere (split-brain prevention).
- **Secret safety:** the full `STARRING_DATABASE_URL` (may contain a password) must never be printed, logged, or embedded in an error message.
- **Codex does all code/git; Claude runs the live Discord+Postgres demo** (Task 5's live runbook). Codex performs no live/token/DB-connected runs.

---

## File Structure

- `crates/automation-instance/src/generator.rs` — **Modify**: add `InstanceIdGenerationError::Entropy` (Task 1).
- `tools/interaction-smoke/src/random_instance_id.rs` — **Create**: pure `encode_instance_id` + `RandomInstanceIdGenerator` + unit tests (Task 1).
- `tools/interaction-smoke/src/main.rs` — **Modify**: declare module + swap generator (Task 1), Postgres startup (Task 4), deferred join rule + validate test (Task 5).
- `tools/interaction-smoke/Cargo.toml` — **Modify**: add `getrandom` (Task 1), `automation-instance-postgres` + `sqlx` (Task 4).
- `crates/automation-runtime/src/custom_id.rs` — **Modify**: `TooLong` variant + length guard + tests (Task 2).
- `crates/automation-runtime/src/mutation.rs` — **Modify**: consume the fallible `encode_instance_action` (Task 2).
- `crates/automation-core/src/run.rs` — **Modify**: reorder `handle_event`, add `resolve_instance_and_run` helper (Task 3).
- `crates/automation-core/tests/dynamic_join.rs` — **Modify**: add deferred-join ordering tests (Task 3).

---

## Task 1 — Random InstanceId edge (user chunk A)

**Files:**
- Modify: `crates/automation-instance/src/generator.rs:5-8`
- Create: `tools/interaction-smoke/src/random_instance_id.rs`
- Modify: `tools/interaction-smoke/src/main.rs:5` and `:37-38`
- Modify: `tools/interaction-smoke/Cargo.toml`

**Interfaces:**
- Produces: `InstanceIdGenerationError::Entropy` (unit variant, keeps the enum `Copy`); `interaction_smoke`'s `random_instance_id::encode_instance_id(bytes: [u8; 8]) -> String`; `random_instance_id::RandomInstanceIdGenerator` implementing `automation_instance::InstanceIdGenerator`.
- Consumes: `automation_instance::{InstanceId, InstanceIdGenerationError, InstanceIdGenerator}`; `getrandom::getrandom`.
- **Dependency boundary:** `getrandom` is added to the **tool** manifest only (Step 3). `automation-instance` gains the `Entropy` variant but **no** new dependency — the OS-entropy call lives entirely in the tool's `RandomInstanceIdGenerator`.

- [ ] **Step 1: Add the `Entropy` variant.** In `crates/automation-instance/src/generator.rs`, replace the enum (lines 5-8):

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceIdGenerationError {
    Invalid(InstanceIdError),
    Entropy,
}
```

- [ ] **Step 2: Verify the instance crate still builds and matches nothing exhaustively.**

Run: `$HOME/.cargo/bin/cargo test -p automation-instance`
Expected: PASS (existing generator tests unaffected; no exhaustive match on the enum exists).

- [ ] **Step 3: Add `getrandom` to the tool.** In `tools/interaction-smoke/Cargo.toml`, under `[dependencies]`, add:

```toml
getrandom = "0.2"
```

- [ ] **Step 4: Create the encoder + generator with failing tests first.** Create `tools/interaction-smoke/src/random_instance_id.rs`:

```rust
use automation_instance::{InstanceId, InstanceIdGenerationError, InstanceIdGenerator};

const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

pub fn encode_instance_id(bytes: [u8; 8]) -> String {
    let value = u64::from_be_bytes(bytes);
    let mut out = String::with_capacity(12);
    for position in 0..12 {
        let shift = 5 * (11 - position);
        let index = ((value >> shift) & 0x1f) as usize;
        out.push(ALPHABET[index] as char);
    }
    out
}

pub struct RandomInstanceIdGenerator;

impl RandomInstanceIdGenerator {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RandomInstanceIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl InstanceIdGenerator for RandomInstanceIdGenerator {
    fn generate(&self) -> Result<InstanceId, InstanceIdGenerationError> {
        let mut bytes = [0u8; 8];
        getrandom::getrandom(&mut bytes).map_err(|_| InstanceIdGenerationError::Entropy)?;
        InstanceId::parse(&format!("i_{}", encode_instance_id(bytes)))
            .map_err(InstanceIdGenerationError::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_fixed_vectors() {
        assert_eq!(encode_instance_id([0, 0, 0, 0, 0, 0, 0, 0]), "000000000000");
        assert_eq!(
            encode_instance_id([0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]),
            "zzzzzzzzzzzz"
        );
        assert_eq!(
            encode_instance_id([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]),
            "4d2pf2dbsqqg"
        );
        assert_eq!(encode_instance_id([0, 0, 0, 0, 0, 0, 0, 1]), "000000000001");
    }

    #[test]
    fn encoder_uses_crockford_lowercase_without_padding() {
        let encoded = encode_instance_id([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]);
        assert_eq!(encoded.len(), 12);
        assert!(encoded.bytes().all(|byte| ALPHABET.contains(&byte)));
        assert!(!encoded.chars().any(|character| matches!(character, 'i' | 'l' | 'o' | 'u')));
        assert!(!encoded.contains('='));
    }

    #[test]
    fn generated_id_has_prefix_and_parses() {
        let generator = RandomInstanceIdGenerator::new();
        let id = generator.generate().unwrap();
        assert!(id.as_str().starts_with("i_"));
        assert_eq!(id.as_str().len(), 14);
        assert_eq!(InstanceId::parse(id.as_str()).unwrap(), id);
    }

    #[test]
    fn generated_ids_vary() {
        let generator = RandomInstanceIdGenerator::new();
        assert_ne!(generator.generate().unwrap(), generator.generate().unwrap());
    }
}
```

- [ ] **Step 5: Wire the module + swap the generator in `main.rs`.** In `tools/interaction-smoke/src/main.rs`:
  - Change the import line 5 from `use automation_instance::{InMemoryInstanceStore, SequenceInstanceIdGenerator};` to:

```rust
use automation_instance::InMemoryInstanceStore;
```

  - Add the module declaration near the top of the file, immediately after the `use` block (before `const RULESET_KEY`):

```rust
mod random_instance_id;
```

  - Replace line 38 `let instance_ids = SequenceInstanceIdGenerator::new("room", 1);` with:

```rust
    let instance_ids = random_instance_id::RandomInstanceIdGenerator::new();
```

- [ ] **Step 6: Run the tool crate tests and gates.**

Run: `$HOME/.cargo/bin/cargo test -p interaction-smoke`
Expected: PASS — the four `random_instance_id` tests pass; the fixed vectors match the empirically-verified values.

Run: `$HOME/.cargo/bin/cargo clippy -p interaction-smoke -p automation-instance --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --all -- --check`
Expected: clean (the `Default` impl prevents `new_without_default`).

- [ ] **Step 7: Commit.**

```bash
git add crates/automation-instance/src/generator.rs tools/interaction-smoke/
git commit -m "feat(interaction-smoke): random InstanceId generator + entropy error variant"
```

---

## Task 2 — custom_id length guard (invariant #2)

**Files:**
- Modify: `crates/automation-runtime/src/custom_id.rs`
- Modify: `crates/automation-runtime/src/mutation.rs:1-4`, `:47-69`, `:140-144`

**Interfaces:**
- Produces: `CustomIdError::TooLong`; `encode_instance_action(instance_id: &str, action: &str) -> Result<String, CustomIdError>` (was `-> String`).
- Consumes (in `mutation.rs`): `automation_core::{AdapterError, AdapterErrorKind}`.

- [ ] **Step 1: Add the `TooLong` variant, the limit constant, and the guard.** In `crates/automation-runtime/src/custom_id.rs`:
  - Add the constant near the other `const`s (after line 5):

```rust
const MAX_CUSTOM_ID_LEN: usize = 100;
```

  - Add `TooLong` to `CustomIdError`:

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CustomIdError {
    WrongPrefix,
    WrongShape,
    BadGuildId,
    UnknownKind,
    TooLong,
}
```

  - Replace `encode_instance_action` (lines 50-52) with:

```rust
pub fn encode_instance_action(instance_id: &str, action: &str) -> Result<String, CustomIdError> {
    let encoded = format!("{PREFIX}:{INSTANCE}:{instance_id}:{action}");
    if encoded.len() > MAX_CUSTOM_ID_LEN {
        return Err(CustomIdError::TooLong);
    }
    Ok(encoded)
}
```

- [ ] **Step 2: Update the single caller in `mutation.rs`** (do this in the same edit pass so the crate stays compilable).
  - Change the import (lines 1-4) to add `AdapterErrorKind`:

```rust
use automation_core::{
    AdapterError, AdapterErrorKind, CreateChannelSpec, CreateRoleSpec, DiscordMutationAdapter,
    PostPanelButtonSpec, PostPanelSpec, ResolvedButtonRoute,
};
```

  - Replace `to_button_component` (lines 47-69) with a fallible version:

```rust
fn to_button_component(
    guild: GuildId,
    ruleset_key: &str,
    button: &PostPanelButtonSpec,
) -> Result<Component, AdapterError> {
    let custom_id = match &button.route {
        ResolvedButtonRoute::Static { key } => encode_button(guild, ruleset_key, key),
        ResolvedButtonRoute::InstanceAction {
            instance_id,
            action,
        } => encode_instance_action(instance_id.as_str(), action).map_err(|error| {
            AdapterError::new(
                AdapterErrorKind::BadRequest,
                format!("custom_id error: {error:?}"),
            )
        })?,
    };
    Ok(Component::Button(Button {
        id: None,
        custom_id: Some(custom_id),
        disabled: false,
        emoji: None,
        label: Some(button.label.clone()),
        style: ButtonStyle::Primary,
        url: None,
        sku_id: None,
    }))
}
```

  - In `post_panel`, replace the button-building block (lines 140-144) with a fallible collect:

```rust
        let buttons: Vec<Component> = spec
            .buttons
            .iter()
            .map(|button| to_button_component(guild, &self.ruleset_key, button))
            .collect::<Result<Vec<Component>, AdapterError>>()?;
```

- [ ] **Step 3: Update the existing roundtrip test + add the worst-case test.** In the `#[cfg(test)]` module of `custom_id.rs`, replace `encode_instance_action_roundtrip` (lines 157-168) with:

```rust
    #[test]
    fn encode_instance_action_roundtrip() {
        let encoded = encode_instance_action("room_001", "join").unwrap();
        assert_eq!(encoded, "starring:i:room_001:join");
        assert_eq!(
            decode(&encoded).unwrap(),
            ParsedCustomId::InstanceAction {
                instance_id: "room_001".to_string(),
                action: "join".to_string(),
            }
        );
    }

    #[test]
    fn encode_instance_action_enforces_hundred_char_limit() {
        let max_instance_id = "z".repeat(32);
        let action_at_limit = "a".repeat(56);
        let encoded = encode_instance_action(&max_instance_id, &action_at_limit).unwrap();
        assert_eq!(encoded.len(), 100);
        let action_over_limit = "a".repeat(57);
        assert_eq!(
            encode_instance_action(&max_instance_id, &action_over_limit).unwrap_err(),
            CustomIdError::TooLong
        );
    }
```

The `instance_id` max of 32 is `InstanceId`'s enforced `MAX_LEN`; `starring:i:` is 11 chars, `+ 32 + ":" + 56 = 100` (Ok), `+ 57 = 101` (`TooLong`). Without the length check in Step 1, this test fails (it would return `Ok` on a 101-char string) — that is what the guard defends.

- [ ] **Step 4: Run tests + gates for the runtime crate.**

Run: `$HOME/.cargo/bin/cargo test -p automation-runtime`
Expected: PASS (both codec tests green; `post_panel` still returns `Result<MessageId, AdapterError>`, so `?` type-checks).

Run: `$HOME/.cargo/bin/cargo clippy -p automation-runtime --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 5: Commit.**

```bash
git add crates/automation-runtime/src/custom_id.rs crates/automation-runtime/src/mutation.rs
git commit -m "feat(automation-runtime): custom_id length guard (TooLong)"
```

---

## Task 3 — Join ACK ordering reorder (user chunk C, invariant #1)

**Files:**
- Modify: `crates/automation-core/src/run.rs:427-492` (rewrite `handle_event`, add helper)
- Modify: `crates/automation-core/tests/dynamic_join.rs` (add deferred-join ordering tests)

**Interfaces:**
- Consumes: existing `interpret`, `run`, `render`, `SanitizeContext`, the `instance_*` error constructors, `EventKind`, `ResolvedInstanceContext`, `InstanceStatus`, `ActionPlan`, `PlannedAction` — all already imported in `run.rs`.
- Produces: unchanged public signature `handle_event(event, ruleset, bindings, services, failure_message, ruleset_key) -> Result<HandleOutcome, AdapterError>`. Behavior change only: `DeferEphemeral` ACK now precedes the PostgreSQL `instances.get(...)` lookup; a resolution failure that happens after a successful defer is reported through exactly one failure `edit_response`.

**Design note (why this is safe for existing tests):** the current code already strips the leading `DeferEphemeral` before calling `run`, so `run` never re-defers — invariant #1 (never defer twice) is preserved. Existing `dynamic_join.rs` tests use a *non-deferred* ruleset (`[GrantRole, RespondEphemeral]`), so `defer_acked` is `false` and failure paths still produce empty `responder.calls()`; the `deferred.rs` (16k) tests use non-`InstanceAction` events, whose resolution block is skipped in both old and new code. Both suites stay green.

- [ ] **Step 1: Add the shared-trace spies, imports, and the failing ordering tests.** Append to `crates/automation-core/tests/dynamic_join.rs`.

First add these imports at the top of the file (alongside the existing `use` block):

```rust
use std::sync::{Arc, Mutex};

use automation_core::adapter::InteractionResponder;
use automation_core::plan::ModalPresentation;
use automation_instance::InstanceStoreError;
```

Then add a shared ordered trace plus two spies that write into it — this is what locks the exact `defer → get → edit` interleaving (a "catch, then belatedly defer" implementation would produce `get → defer → edit` and fail):

```rust
#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Vec<String>>>);

impl Trace {
    fn record(&self, entry: String) {
        self.0.lock().unwrap().push(entry);
    }
    fn entries(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

struct TracingStore {
    trace: Trace,
    inner: InMemoryInstanceStore,
}

impl InstanceStore for TracingStore {
    async fn register(&self, instance: AutomationInstance) -> Result<(), InstanceStoreError> {
        self.inner.register(instance).await
    }
    async fn get(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        self.trace.record("instance_store.get".to_string());
        self.inner.get(guild_id, instance_id).await
    }
    async fn list_by_guild(
        &self,
        guild_id: GuildId,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
        self.inner.list_by_guild(guild_id).await
    }
    async fn update_status(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
        status: InstanceStatus,
    ) -> Result<(), InstanceStoreError> {
        self.inner
            .update_status(guild_id, instance_id, status)
            .await
    }
}

struct TracingResponder {
    trace: Trace,
}

impl InteractionResponder for TracingResponder {
    async fn respond_ephemeral(&self, content: String) -> Result<(), AdapterError> {
        self.trace.record(format!("respond_ephemeral:{content}"));
        Ok(())
    }
    async fn open_modal(&self, _modal: &ModalPresentation) -> Result<(), AdapterError> {
        self.trace.record("open_modal".to_string());
        Ok(())
    }
    async fn defer_ephemeral(&self) -> Result<(), AdapterError> {
        self.trace.record("defer_ephemeral".to_string());
        Ok(())
    }
    async fn edit_response(&self, content: String) -> Result<(), AdapterError> {
        self.trace.record(format!("edit_response:{content}"));
        Ok(())
    }
}

struct FailingDeferResponder {
    trace: Trace,
}

impl InteractionResponder for FailingDeferResponder {
    async fn respond_ephemeral(&self, _content: String) -> Result<(), AdapterError> {
        Ok(())
    }
    async fn open_modal(&self, _modal: &ModalPresentation) -> Result<(), AdapterError> {
        Ok(())
    }
    async fn defer_ephemeral(&self) -> Result<(), AdapterError> {
        self.trace.record("defer_ephemeral".to_string());
        Err(AdapterError::new(AdapterErrorKind::Unknown, "defer failed"))
    }
    async fn edit_response(&self, content: String) -> Result<(), AdapterError> {
        self.trace.record(format!("edit_response:{content}"));
        Ok(())
    }
}
```

Then add a deferred-ruleset helper next to `join_ruleset` (after line 87):

```rust
fn deferred_join_ruleset(role: RoleRef) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "join_rule".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "join".to_string(),
            },
            actions: vec![
                ActionSpec::DeferEphemeral,
                ActionSpec::GrantRole {
                    role,
                    target: ActionTarget::Actor,
                },
                ActionSpec::EditResponse {
                    content: "joined".to_string(),
                },
            ],
        }],
    }
}
```

Then add these three tests at the end of the file (the success path uses the existing mocks; the ordering and defer-failure tests use the shared trace):

```rust
#[test]
fn deferred_join_defers_before_resolution_and_grants() {
    let instances = InMemoryInstanceStore::new();
    let stored = instance(
        GuildId(7),
        InstanceStatus::Active,
        "studyroom",
        InstanceResources {
            roles: BTreeMap::from([("member_role".to_string(), RoleId(55))]),
            ..InstanceResources::default()
        },
    );
    block_on(instances.register(stored)).unwrap();
    let mutation = MockMutationAdapter::new();
    let responder = MockInteractionResponder::new();
    let generator = SequenceInstanceIdGenerator::new("unused", 1);
    let services = AutomationServices {
        mutation: &mutation,
        responder: &responder,
        instances: &instances,
        instance_ids: &generator,
    };
    let outcome = block_on(handle_event(
        &event(GuildId(7), "join"),
        &deferred_join_ruleset(instance_role(InstanceRef::Event, "member_role")),
        &ResourceBindingMap::default(),
        &services,
        "could not join",
        "studyroom",
    ))
    .unwrap();
    assert_eq!(outcome, HandleOutcome::Executed);
    assert_eq!(
        mutation.calls(),
        vec![MutationCall::GrantRole {
            guild: GuildId(7),
            member: UserId(42),
            role: RoleId(55),
        }]
    );
    assert_eq!(
        responder.calls(),
        vec![
            ResponderCall::DeferEphemeral,
            ResponderCall::EditResponse {
                content: "joined".to_string(),
            },
        ]
    );
    assert_eq!(
        responder
            .calls()
            .iter()
            .filter(|call| matches!(call, ResponderCall::DeferEphemeral))
            .count(),
        1
    );
}

#[test]
fn deferred_join_missing_instance_traces_defer_then_lookup_then_edit() {
    let trace = Trace::default();
    let instances = TracingStore {
        trace: trace.clone(),
        inner: InMemoryInstanceStore::new(),
    };
    let mutation = MockMutationAdapter::new();
    let responder = TracingResponder {
        trace: trace.clone(),
    };
    let generator = SequenceInstanceIdGenerator::new("unused", 1);
    let services = AutomationServices {
        mutation: &mutation,
        responder: &responder,
        instances: &instances,
        instance_ids: &generator,
    };
    let error = block_on(handle_event(
        &event(GuildId(7), "join"),
        &deferred_join_ruleset(instance_role(InstanceRef::Event, "member_role")),
        &ResourceBindingMap::default(),
        &services,
        "could not join",
        "studyroom",
    ))
    .unwrap_err();
    assert_eq!(error.kind, AdapterErrorKind::NotFound);
    assert!(error.message.contains("InstanceNotFound"));
    assert_eq!(
        trace.entries(),
        vec![
            "defer_ephemeral".to_string(),
            "instance_store.get".to_string(),
            "edit_response:could not join".to_string(),
        ]
    );
    assert!(mutation.calls().is_empty());
}

#[test]
fn deferred_join_defer_failure_skips_lookup_and_edit() {
    let trace = Trace::default();
    let instances = TracingStore {
        trace: trace.clone(),
        inner: InMemoryInstanceStore::new(),
    };
    let mutation = MockMutationAdapter::new();
    let responder = FailingDeferResponder {
        trace: trace.clone(),
    };
    let generator = SequenceInstanceIdGenerator::new("unused", 1);
    let services = AutomationServices {
        mutation: &mutation,
        responder: &responder,
        instances: &instances,
        instance_ids: &generator,
    };
    let error = block_on(handle_event(
        &event(GuildId(7), "join"),
        &deferred_join_ruleset(instance_role(InstanceRef::Event, "member_role")),
        &ResourceBindingMap::default(),
        &services,
        "could not join",
        "studyroom",
    ))
    .unwrap_err();
    assert_eq!(error.kind, AdapterErrorKind::Unknown);
    assert_eq!(trace.entries(), vec!["defer_ephemeral".to_string()]);
    assert!(mutation.calls().is_empty());
}
```

Invariants these lock: (1) `DeferEphemeral` fires exactly once and **before** `instance_store.get` — even though the leading defer is stripped from the plan handed to `run`, so the pre-executed defer is never lost; (2) a missing instance after a successful defer produces exactly one failure `edit_response` carrying `failure_message`; (3) if the defer itself fails, `instance_store.get` and `edit_response` are **not** called (you must not edit an initial response that was never created).

- [ ] **Step 2: Run the new tests to verify they fail against current ordering.**

Run: `$HOME/.cargo/bin/cargo test -p automation-core --test dynamic_join deferred_join`
Expected: FAIL. `deferred_join_missing_instance_traces_defer_then_lookup_then_edit` fails against the current code because `handle_event` resolves the instance *before* deferring, so the trace is `["instance_store.get", ...]` (or the error returns before any defer) rather than `["defer_ephemeral", "instance_store.get", ...]`.

- [ ] **Step 3: Rewrite `handle_event` and add the helper.** In `crates/automation-core/src/run.rs`, replace the entire current `handle_event` function (lines 427-492) with the following (the helper first, then the reordered `handle_event`):

```rust
async fn resolve_instance_and_run<M, R, S, G>(
    event: &RuntimeEvent,
    context: &mut RuntimeContext,
    steps: Vec<PlannedAction>,
    services: &AutomationServices<'_, M, R, S, G>,
    ruleset_key: &str,
) -> Result<(), AdapterError>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceStore,
    G: InstanceIdGenerator,
{
    if let EventKind::InstanceAction {
        instance_id,
        action,
    } = &event.kind
    {
        let instance = services
            .instances
            .get(event.guild_id, instance_id)
            .await
            .map_err(instance_store_error)?
            .ok_or_else(|| instance_not_found(instance_id))?;
        if instance.status != InstanceStatus::Active {
            return Err(instance_inactive(&instance));
        }
        if instance.ruleset_key != ruleset_key {
            return Err(instance_ruleset_mismatch(&instance, ruleset_key));
        }
        context.instance = Some(ResolvedInstanceContext {
            instance,
            action: action.clone(),
        });
    }
    run(context, &ActionPlan { steps }, services).await?;
    Ok(())
}

pub async fn handle_event<M, R, S, G>(
    event: &RuntimeEvent,
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
    services: &AutomationServices<'_, M, R, S, G>,
    failure_message: &str,
    ruleset_key: &str,
) -> Result<HandleOutcome, AdapterError>
where
    M: DiscordMutationAdapter,
    R: InteractionResponder,
    S: InstanceStore,
    G: InstanceIdGenerator,
{
    let mut context = RuntimeContext::from_event(event, ruleset_key);
    match interpret(event, ruleset, bindings) {
        Some(plan) => {
            let mut steps = plan.steps;
            let defer_acked = if matches!(steps.first(), Some(PlannedAction::DeferEphemeral)) {
                services.responder.defer_ephemeral().await?;
                steps.remove(0);
                true
            } else {
                false
            };
            match resolve_instance_and_run(event, &mut context, steps, services, ruleset_key).await {
                Ok(()) => Ok(HandleOutcome::Executed),
                Err(error) => {
                    if defer_acked {
                        if let Ok(rendered) = render(
                            failure_message,
                            &context,
                            SanitizeContext::EphemeralMessageContent,
                        ) {
                            let _ = services.responder.edit_response(rendered).await;
                        }
                    }
                    Err(error)
                }
            }
        }
        None => Ok(HandleOutcome::NoOp),
    }
}
```

- [ ] **Step 4: Run the new tests to verify they pass.**

Run: `$HOME/.cargo/bin/cargo test -p automation-core --test dynamic_join`
Expected: PASS — both new tests green, and all pre-existing `dynamic_join` tests still green.

- [ ] **Step 5: Run the whole automation-core suite for regressions.**

Run: `$HOME/.cargo/bin/cargo test -p automation-core`
Expected: PASS — especially `deferred.rs` (16k defer/edit contract) and `no_ai_gateway.rs` unaffected.

Run: `$HOME/.cargo/bin/cargo clippy -p automation-core --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --all -- --check`
Expected: clean (helper has 5 params; `handle_event` 6 — both under the `too_many_arguments` threshold).

- [ ] **Step 6: Commit.**

```bash
git add crates/automation-core/src/run.rs crates/automation-core/tests/dynamic_join.rs
git commit -m "feat(automation-core): defer ACK before instance resolution in handle_event"
```

---

## Task 4 — Postgres-or-die startup (user chunk B)

**Files:**
- Modify: `tools/interaction-smoke/Cargo.toml`
- Modify: `tools/interaction-smoke/src/main.rs:5`, `:23-50`

**Interfaces:**
- Consumes: `sqlx::PgPool`, `automation_instance_postgres::{PostgresInstanceStore, MIGRATOR}`, `random_instance_id::RandomInstanceIdGenerator` (from Task 1).
- Produces: a tool that refuses to start unless `STARRING_DATABASE_URL` connects and migrations apply; never prints the URL.

- [ ] **Step 1: Add the Postgres + sqlx dependencies.** In `tools/interaction-smoke/Cargo.toml`, under `[dependencies]`, add:

```toml
automation-instance-postgres = { path = "../../crates/automation-instance-postgres" }
sqlx = { version = "0.8.6", default-features = false, features = ["runtime-tokio-rustls", "postgres", "migrate"] }
```

- [ ] **Step 2: Remove the now-unused InMemory import.** In `main.rs`, delete the line `use automation_instance::InMemoryInstanceStore;` (added in Task 1 Step 5). After this task nothing from `automation_instance` is imported directly in `main.rs`.

- [ ] **Step 3: Add the connect-error reporter.** In `main.rs`, add this function (e.g. directly after `fn created`). The user-facing string is a fixed phrase; internal detail goes to stderr **only** for non-configuration errors (the `Configuration` variant is the one that can echo the DSN, so it logs no detail):

```rust
fn report_connect_error(error: sqlx::Error) -> String {
    match &error {
        sqlx::Error::Configuration(_) => {
            eprintln!("postgres startup: connection failed (invalid configuration)");
        }
        other => {
            eprintln!("postgres startup: connection failed: {other}");
        }
    }
    "PostgreSQL startup failed during connection".to_string()
}
```

- [ ] **Step 4: Rewrite `main` to Postgres-or-die.** Replace the body of `main` from the env reads through `gateway::run` (lines 27-49) with:

```rust
    let token = env::var("DISCORD_TEST_TOKEN")?;
    let guild_id: u64 = env::var("DISCORD_TEST_GUILD")?.parse()?;
    let channel_id: u64 = env::var("DISCORD_TEST_CHANNEL")?.parse()?;
    let database_url = env::var("STARRING_DATABASE_URL")?;

    let ruleset = studyroom_ruleset();
    let bindings = bindings(channel_id);
    validate(&ruleset, &bindings).expect("studyroom ruleset should validate");

    let pool = sqlx::PgPool::connect(&database_url)
        .await
        .map_err(report_connect_error)?;
    automation_instance_postgres::MIGRATOR
        .run(&pool)
        .await
        .map_err(|error| {
            eprintln!("postgres startup: migration failed: {error}");
            "PostgreSQL startup failed during migration".to_string()
        })?;
    let instances = automation_instance_postgres::PostgresInstanceStore::new(pool);
    let instance_ids = random_instance_id::RandomInstanceIdGenerator::new();

    install_panel(&token, guild_id, channel_id).await?;
    eprintln!("postgres connected; panel installed; listening for interactions (Ctrl-C to stop)");
    gateway::run(
        token,
        RULESET_KEY.to_string(),
        ruleset,
        bindings,
        "스터디룸 생성에 실패했습니다. 봇 권한 또는 역할 순서를 확인해주세요.".to_string(),
        instances,
        instance_ids,
    )
    .await;
    Ok(())
```

Notes:
- **Eager connect, not lazy:** `PgPool::connect` establishes a real connection and fails fast; do **not** use `connect_lazy` (it would defer failure past startup and defeat Postgres-or-die).
- **Ordering:** connect + migrate happen **before** `install_panel`, so a DB failure aborts startup before any Discord side-effect. On any failure the process exits (no InMemory fallback exists).
- **Secret safety:** `database_url` is read into a local and never printed. The user-facing messages are the fixed phrases `PostgreSQL startup failed during connection` / `... during migration`. Internal stderr detail is emitted only for non-`Configuration` connect errors and for `MigrateError` (neither embeds the DSN); the `Configuration` variant — the only sqlx error that can echo the URL — logs no detail.

- [ ] **Step 5: Build the tool (offline; no DB needed).**

Run: `$HOME/.cargo/bin/cargo build -p interaction-smoke`
Expected: PASS — `sqlx` uses runtime queries (no `query!` macro), so no `DATABASE_URL` is needed at build time.

Run: `$HOME/.cargo/bin/cargo clippy -p interaction-smoke --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 6: Commit.**

```bash
git add tools/interaction-smoke/Cargo.toml tools/interaction-smoke/src/main.rs Cargo.lock
git commit -m "feat(interaction-smoke): postgres-or-die startup with PostgresInstanceStore"
```

---

## Task 5 — Deferred join rule + live scenario (user chunk D, invariant #3)

**Files:**
- Modify: `tools/interaction-smoke/src/main.rs:193-210` (join rule) + add a `#[cfg(test)]` validate test.

**Interfaces:**
- Consumes: `validate`, `studyroom_ruleset`, `bindings` (all in `main.rs`).
- Produces: a join rule using the 16k deferred contract `[DeferEphemeral, GrantRole{Instance{Event, "member_role"}}, EditResponse]`, so that live, the ACK precedes the PostgreSQL lookup (Task 3 reorder + this rule together).

- [ ] **Step 1: Add a failing validate test.** At the bottom of `tools/interaction-smoke/src/main.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn studyroom_ruleset_validates() {
        validate(&studyroom_ruleset(), &bindings(1)).expect("studyroom ruleset should validate");
    }

    #[test]
    fn join_rule_uses_deferred_contract() {
        let ruleset = studyroom_ruleset();
        let join = ruleset
            .rules
            .iter()
            .find(|rule| rule.key == "study_join_rule")
            .expect("join rule present");
        assert!(matches!(
            join.actions.first(),
            Some(ActionSpec::DeferEphemeral)
        ));
        assert!(matches!(
            join.actions.last(),
            Some(ActionSpec::EditResponse { .. })
        ));
    }
}
```

- [ ] **Step 2: Run to confirm `join_rule_uses_deferred_contract` fails.**

Run: `$HOME/.cargo/bin/cargo test -p interaction-smoke join_rule_uses_deferred_contract`
Expected: FAIL — the current join rule starts with `GrantRole`, not `DeferEphemeral`.

- [ ] **Step 3: Switch the join rule to the deferred contract.** In `main.rs`, replace the `study_join_rule` `InteractionRule` (lines 193-210) with:

```rust
            InteractionRule {
                key: "study_join_rule".to_string(),
                trigger: TriggerSpec::InstanceAction {
                    action: "join".to_string(),
                },
                actions: vec![
                    ActionSpec::DeferEphemeral,
                    ActionSpec::GrantRole {
                        role: RoleRef::Instance {
                            instance: InstanceRef::Event,
                            alias: "member_role".to_string(),
                        },
                        target: ActionTarget::Actor,
                    },
                    ActionSpec::EditResponse {
                        content: "스터디룸에 참가했습니다.".to_string(),
                    },
                ],
            },
```

- [ ] **Step 4: Run the tool tests + full gates.**

Run: `$HOME/.cargo/bin/cargo test -p interaction-smoke`
Expected: PASS — `studyroom_ruleset_validates` confirms the deferred join rule passes validation (defer-first, edit-last, `InstanceRef::Event`, alias valid), so the live process will not panic on boot.

Run from the workspace root:
```bash
$HOME/.cargo/bin/cargo build && \
$HOME/.cargo/bin/cargo test && \
$HOME/.cargo/bin/cargo clippy --all-targets -- -D warnings && \
$HOME/.cargo/bin/cargo fmt --all -- --check
```
Expected: whole workspace green (DB-independent). This is the full **code gate** for 17e.

- [ ] **Step 5: Commit.**

```bash
git add tools/interaction-smoke/src/main.rs
git commit -m "feat(interaction-smoke): deferred join contract for ACK-before-DB"
```

- [ ] **Step 6: (Optional, Claude-run) PostgreSQL integration test for the store** — re-confirm 17d durability still holds against the local DB before the live demo:

```bash
STARRING_TEST_DATABASE_URL=postgres://localhost/starring_test \
  $HOME/.cargo/bin/cargo test -p automation-instance-postgres --test postgres_store -- --ignored --test-threads=1
```
Expected: PASS (reconnect-durability).

---

## Live Scenario Runbook (Claude executes — NOT Codex)

Prerequisites: a reachable local PostgreSQL with a `starring` database; a Discord bot token/guild/channel (reuse the prior smoke server). Env: `DISCORD_TEST_TOKEN`, `DISCORD_TEST_GUILD`, `DISCORD_TEST_CHANNEL`, `STARRING_DATABASE_URL` (e.g. `postgres://localhost/starring`).

Run the bot with `$HOME/.cargo/bin/cargo run -p interaction-smoke` and drive Discord manually.

- [ ] **L1 — First process, create Room A.** Start the bot (expect `postgres connected; panel installed; ...`). Click "Create study room", submit a room name. Confirm the "처리 중 → 생성 완료" ephemeral, the new private channel, and the hub "참가하기" button.
- [ ] **L2 — Verify DB persistence.** In `psql`: `SELECT guild_id, instance_id, status FROM automation_instances;` — one `active` row whose `instance_id` starts with `i_` (14 chars). Record instance A's id.
- [ ] **L3 — Full restart.** Stop the bot (Ctrl-C; PgPool closes). Start a fresh process (new PgPool, same fixture).
- [ ] **L4 — Join A after restart (persistence).** Click the pre-existing "참가하기" button on Room A's hub message. Expect the deferred ephemeral ("처리 중" → success), and confirm the clicking user receives the member role and can see Room A's channel. This proves the instance was read from PostgreSQL, not memory.
- [ ] **L5 — Create Room B after restart (restart-safe generator).** Create a second study room. Confirm a second `active` row with a **different** `i_`-prefixed id (no `DuplicateInstance`), and that Room A's row is untouched.
- [ ] **L6 — Cleanup (Discord + DB together, invariant #3).** Remove all four artifacts for each test room:
  1. the created **room channel**,
  2. the created **member role**,
  3. the **public hub join panel** message (the "참가하기" button message) — this does **not** disappear when the room channel is deleted, so delete it explicitly (grab its `MessageId` from the live log / Discord UI),
  4. the DB **instance status → `Deleted`**, via psql `UPDATE automation_instances SET status='deleted' WHERE instance_id IN (<A>, <B>);` or a one-off `PostgresInstanceStore::update_status(guild, id, InstanceStatus::Deleted)`.

  Leaving instances `active` after their Discord resources are gone creates drift (there is no reconciler yet), so the smoke run must not create it itself.

**Known gap (documented, not a 17e blocker):** the `RegisterInstance` manifest captures the room channel, member role, and in-room welcome panel, but **not** the hub join panel — that `PostPanel` runs *after* `RegisterInstance`, and there is no API yet to attach post-registration resources to an existing instance's `resources`. So hub-panel cleanup is manual for this smoke. Attaching/updating instance resources after registration is a future instance-lifecycle / reconciliation concern.

**17e is complete only when L4 (persistence) and L5 (restart-safe generator) both succeed and L6 leaves no active-but-orphaned rows.**

---

## Known Limitations (documented, not 17e blockers)

- **No per-action `RunResult` audit.** `handle_event` returns `HandleOutcome::{Executed, NoOp}` and `run` returns `Vec<CreatedResource>`; there is no structured per-step ledger (`DeferEphemeral: Success`, …). The defer/edit lifecycle is observable through the responder call sequence (locked by the Task 3 trace tests), not a `RunResult.steps`. A structured audit is a future lifecycle/observability concern.
- **`CreatedResource.action_index` is relative to the post-defer sub-plan.** Because `handle_event` strips the leading `DeferEphemeral` with `steps.remove(0)` before calling `run`, the `action_index` on any `CreatedResource` is 0-based over the *executed* (post-defer) plan, not the original `ActionPlan`. No current consumer reads these indices (`handle_event` discards `run`'s return), so this is safe today; if a future consumer needs original-plan alignment, `run` will need an index offset. Left as-is for 17e.
- **Compact custom_id carries no ruleset key.** The instance custom_id is `starring:i:<instance_id>:<action>` (no ruleset segment). This is safe here because, after the DB lookup, `handle_event` checks `instance.ruleset_key == ruleset_key` and the tool loads exactly one fixture RuleSet. A production dispatcher that loads multiple RuleSets concurrently is future work, tied to RuleSet persistence/activation.
- **Hub join panel not attached to instance resources.** See the L6 "Known gap" above — cleanup of the hub panel is manual for this smoke.

---

## Self-Review

- **Spec coverage:** §1 RandomInstanceIdGenerator → Task 1. §1 exact alphabet + pure encoder + fixed vectors → Task 1 Steps 4. §1.5 defer-before-Store.get reorder → Task 3. §1/§6 custom_id worst-case + `TooLong` runtime guard → Task 2. §2 Postgres-or-die + secret redaction → Task 4. §5 live demo A/B + cleanup → Live Runbook. §6 fixed-vector/worst-case/deterministic-order/no-fallback tests → Tasks 1-3 + Runbook L2/L5. §1 `Entropy` typed error → Task 1 Step 1. Deterministic duplicate (§6-4) is the existing 17d store test (`same guild duplicate → DuplicateInstance`) exercised live at L5; the DB PK is the judge, matching the spec's "충돌 정확성은 확률 아니라 PK로 검증".
- **Placeholder scan:** none — every code step carries complete code; fixed-vector expected values are empirically verified (`000000000000`, `zzzzzzzzzzzz`, `4d2pf2dbsqqg`, `000000000001`).
- **Type consistency:** `encode_instance_action` returns `Result<String, CustomIdError>` in Task 2 and is consumed with `?` in `mutation.rs` (same task). `resolve_instance_and_run` takes `&mut RuntimeContext` and calls `run(context, ...)` (`&mut` reborrows to `&`). `InstanceIdGenerationError::Entropy` is a unit variant (enum stays `Copy`). `report_connect_error` returns `String`, propagated via `?` into `Box<dyn Error>` (which has `From<String>`).
- **Ordering-proof rigor:** `deferred_join_missing_instance_traces_defer_then_lookup_then_edit` is the discriminator via the shared `Trace` — it passes only when the exact sequence is `[defer_ephemeral, instance_store.get, edit_response:…]`; a "resolve first" or "catch-then-belatedly-defer" implementation produces a different order and fails. `deferred_join_defer_failure_skips_lookup_and_edit` proves a failed defer performs neither the lookup nor the edit.
- **Defer-not-lost (enhancement #3):** there is no `RunResult.steps` audit structure in the current architecture (`run` returns `Vec<CreatedResource>`; `handle_event` returns `HandleOutcome`). Rather than introduce one (out of 17e scope), the observable "the stripped defer is not lost" invariant is enforced by the trace: `defer_ephemeral` appears exactly once and first even though it is `remove(0)`-ed from the plan handed to `run`. See Known Limitations for the `CreatedResource.action_index` consequence.
