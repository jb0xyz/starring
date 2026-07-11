# Phase 18b — PostgreSQL RuleSet Store Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `PostgresRuleSetStore<H: RuleSetHasher>` implementing 18a's `RuleSetStore` against PostgreSQL, where concurrent publishes for the same `(guild, ruleset_key)` are serialized by a head-row `SELECT … FOR UPDATE`, identical content reuses the existing version, and only new content consumes a monotonic version.

**Architecture:** New edge crate `automation-ruleset-postgres → automation-ruleset` (mirrors 17d's `automation-instance-postgres`). Three tables (heads/versions/activations) in the shared root `/migrations`. Runtime queries (no `query!` macro → DB-independent build). Publish runs in a transaction: ensure+lock head row, dedup by `content_hash`, allocate `next_version` only for new content.

**Tech Stack:** Rust 2021, sqlx 0.8.6 (PgPool, transactions, `query`/`query_as`/`query_scalar` + `FromRow`, `sqlx::types::Json`), tokio (multi-thread + `sync::Barrier` for concurrency tests). The transaction executor pattern (`&mut *tx`, `query_scalar`, `FOR UPDATE`, `commit`/`rollback`) is pre-verified to compile.

## Global Constraints

- **No comments** anywhere (`//`, `///`, `//!`). Match existing files (17d `automation-instance-postgres` is the template).
- **Cargo path:** gate commands use `$HOME/.cargo/bin/cargo`.
- **Gates (every task):** `cargo build` · `cargo test` (DB-independent) · `cargo clippy --all-targets -- -D warnings` · `cargo fmt --all -- --check`.
- **Crate-modification scope:** new crate `automation-ruleset-postgres` only. `automation-ruleset` (core) **untouched** — `RuleSetStoreError::Backend(String)` already exists from 18a. `automation-core`/`automation-state`/`automation-instance`/`automation-instance-postgres`/`automation-runtime` **untouched**.
- **Dependency guard:** `automation-ruleset-postgres → automation-ruleset` allowed; **`automation-ruleset → sqlx` forbidden** and **`automation-ruleset → automation-ruleset-postgres` forbidden** (guard test in Task 1).
- **Safety:** no event-time LLM. `tests/no_ai_gateway.rs` guards its own Cargo.toml (17d pattern).
- **DB-independent build/test:** runtime queries only (no `query!`). Default `cargo test` must pass with no database. Real-Postgres tests are `#[ignore]` and require `STARRING_TEST_DATABASE_URL` (DB name must contain `test`).
- **Publish contract:** `validate_structural → content_hash → BEGIN → ensure+lock head → dedup → (Reused | HashCollision | allocate+INSERT+increment) → COMMIT/ROLLBACK`. Structural failure touches no DB. Published versions are never UPDATEd/DELETEd (application-enforced immutability).

---

## File Structure

- `Cargo.toml` (workspace) — **Modify**: add `crates/automation-ruleset-postgres` member (Task 1).
- `crates/automation-ruleset-postgres/Cargo.toml` — **Create** (Task 1).
- `crates/automation-ruleset-postgres/build.rs` — **Create**: migration rerun-if-changed (Task 1).
- `crates/automation-ruleset-postgres/src/lib.rs` — **Create**: `MIGRATOR`, exports, `PostgresRuleSetStore`, row mapping (Tasks 1-3).
- `migrations/202607110002_create_automation_rulesets.sql` — **Create** (Task 1).
- `crates/automation-ruleset-postgres/tests/no_ai_gateway.rs` — **Create** (Task 1).
- `crates/automation-ruleset-postgres/tests/dependency_guard.rs` — **Create** (Task 1).
- `crates/automation-ruleset-postgres/tests/postgres_ruleset.rs` — **Create**: ignored real-Postgres tests (Task 4).

---

## Task 1 — Crate skeleton + migration + row mapping (chunk A)

**Files:**
- Modify: `Cargo.toml` (workspace members)
- Create: `crates/automation-ruleset-postgres/{Cargo.toml, build.rs, src/lib.rs}`, `migrations/202607110002_create_automation_rulesets.sql`, `tests/no_ai_gateway.rs`, `tests/dependency_guard.rs`

**Interfaces:**
- Produces: `MIGRATOR`, `RuleSetVersionRow` (FromRow) + `TryFrom<RuleSetVersionRow> for RuleSetVersion`, `backend(...)` helper.

- [ ] **Step 1: Register the crate.** In workspace `Cargo.toml` `members`, after `"crates/automation-ruleset",`:

```toml
    "crates/automation-ruleset-postgres",
```

- [ ] **Step 2: Create `crates/automation-ruleset-postgres/Cargo.toml`.**

```toml
[package]
name = "automation-ruleset-postgres"
version = "0.1.0"
edition.workspace = true

[dependencies]
automation-ruleset = { path = "../automation-ruleset" }
automation-state = { path = "../automation-state" }
discord-model = { path = "../discord-model" }
serde = { workspace = true }
serde_json = { workspace = true }
sqlx = { version = "0.8.6", default-features = false, features = ["runtime-tokio-rustls", "postgres", "json", "derive", "macros", "migrate"] }

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread", "sync"] }
```

- [ ] **Step 3: Create `build.rs`.**

```rust
fn main() {
    println!("cargo:rerun-if-changed=../../migrations");
}
```

- [ ] **Step 4: Create the migration** `migrations/202607110002_create_automation_rulesets.sql`.

```sql
CREATE TABLE automation_ruleset_heads (
    guild_id     TEXT NOT NULL,
    ruleset_key  TEXT NOT NULL,
    next_version BIGINT NOT NULL,
    PRIMARY KEY (guild_id, ruleset_key),
    CONSTRAINT arh_key_format CHECK (ruleset_key ~ '^[A-Za-z0-9_-]{1,64}$'),
    CONSTRAINT arh_next_range CHECK (next_version BETWEEN 1 AND 4294967296)
);

CREATE TABLE automation_ruleset_versions (
    guild_id       TEXT NOT NULL,
    ruleset_key    TEXT NOT NULL,
    version        BIGINT NOT NULL,
    schema_version BIGINT NOT NULL,
    definition     JSONB NOT NULL,
    content_hash   TEXT NOT NULL,
    created_by     TEXT NOT NULL,
    PRIMARY KEY (guild_id, ruleset_key, version),
    CONSTRAINT arv_hash_unique UNIQUE (guild_id, ruleset_key, content_hash),
    CONSTRAINT arv_key_format CHECK (ruleset_key ~ '^[A-Za-z0-9_-]{1,64}$'),
    CONSTRAINT arv_hash_format CHECK (content_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT arv_version_range CHECK (version BETWEEN 1 AND 4294967295),
    CONSTRAINT arv_schema_range CHECK (schema_version BETWEEN 1 AND 4294967295),
    CONSTRAINT arv_definition_object CHECK (jsonb_typeof(definition) = 'object')
);

CREATE TABLE automation_ruleset_activations (
    guild_id       TEXT NOT NULL,
    ruleset_key    TEXT NOT NULL,
    active_version BIGINT NOT NULL,
    PRIMARY KEY (guild_id, ruleset_key),
    CONSTRAINT ara_fk FOREIGN KEY (guild_id, ruleset_key, active_version)
        REFERENCES automation_ruleset_versions (guild_id, ruleset_key, version)
        ON DELETE RESTRICT
);
```

- [ ] **Step 5: Create `src/lib.rs` with MIGRATOR + row mapping (failing tests first).**

```rust
use automation_ruleset::{
    RuleSetContentHash, RuleSetKey, RuleSetSchemaVersion, RuleSetStoreError, RuleSetVersion,
    RuleSetVersionId,
};
use automation_state::InteractionRuleSet;
use discord_model::{GuildId, UserId};

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

fn backend(error: impl std::fmt::Display) -> RuleSetStoreError {
    RuleSetStoreError::Backend(error.to_string())
}

#[derive(sqlx::FromRow)]
struct RuleSetVersionRow {
    guild_id: String,
    ruleset_key: String,
    version: i64,
    schema_version: i64,
    definition: sqlx::types::Json<InteractionRuleSet>,
    content_hash: String,
    created_by: String,
}

impl TryFrom<RuleSetVersionRow> for RuleSetVersion {
    type Error = RuleSetStoreError;

    fn try_from(row: RuleSetVersionRow) -> Result<Self, Self::Error> {
        let guild_id = row
            .guild_id
            .parse::<GuildId>()
            .map_err(|_| backend(format!("invalid persisted guild_id: {}", row.guild_id)))?;
        let ruleset_key = RuleSetKey::parse(&row.ruleset_key)
            .map_err(|error| backend(format!("invalid persisted ruleset_key: {error:?}")))?;
        let version = u32::try_from(row.version)
            .ok()
            .and_then(|value| RuleSetVersionId::new(value).ok())
            .ok_or_else(|| backend(format!("invalid persisted version: {}", row.version)))?;
        let schema_version = u32::try_from(row.schema_version)
            .ok()
            .and_then(|value| RuleSetSchemaVersion::new(value).ok())
            .ok_or_else(|| {
                backend(format!("invalid persisted schema_version: {}", row.schema_version))
            })?;
        let content_hash = RuleSetContentHash::parse_hex(&row.content_hash)
            .ok_or_else(|| backend(format!("invalid persisted content_hash: {}", row.content_hash)))?;
        let created_by = row
            .created_by
            .parse::<UserId>()
            .map_err(|_| backend(format!("invalid persisted created_by: {}", row.created_by)))?;
        Ok(RuleSetVersion {
            guild_id,
            ruleset_key,
            version,
            schema_version,
            definition: row.definition.0,
            content_hash,
            created_by,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automation_ruleset::{content_hash, CURRENT_RULESET_SCHEMA_VERSION};
    use automation_state::{
        ActionSpec, ActionTarget, InstanceRef, InteractionRule, RoleRef, TriggerSpec,
    };

    fn definition() -> InteractionRuleSet {
        InteractionRuleSet {
            version: 1,
            panels: vec![],
            modals: vec![],
            rules: vec![InteractionRule {
                key: "r".to_string(),
                trigger: TriggerSpec::InstanceAction {
                    action: "join".to_string(),
                },
                actions: vec![ActionSpec::GrantRole {
                    role: RoleRef::Instance {
                        instance: InstanceRef::Event,
                        alias: "member_role".to_string(),
                    },
                    target: ActionTarget::Actor,
                }],
            }],
        }
    }

    fn row() -> RuleSetVersionRow {
        RuleSetVersionRow {
            guild_id: "7".to_string(),
            ruleset_key: "studyroom".to_string(),
            version: 1,
            schema_version: 1,
            definition: sqlx::types::Json(definition()),
            content_hash: content_hash(CURRENT_RULESET_SCHEMA_VERSION, &definition())
                .unwrap()
                .to_hex(),
            created_by: "3".to_string(),
        }
    }

    #[test]
    fn valid_row_converts() {
        let version = RuleSetVersion::try_from(row()).unwrap();
        assert_eq!(version.guild_id, GuildId(7));
        assert_eq!(version.version, RuleSetVersionId::FIRST);
        assert_eq!(version.created_by, UserId(3));
    }

    #[test]
    fn invalid_persisted_values_are_backend() {
        let mut bad = row();
        bad.version = 0;
        assert!(matches!(
            RuleSetVersion::try_from(bad),
            Err(RuleSetStoreError::Backend(_))
        ));
        let mut bad = row();
        bad.version = 5_000_000_000;
        assert!(matches!(
            RuleSetVersion::try_from(bad),
            Err(RuleSetStoreError::Backend(_))
        ));
        let mut bad = row();
        bad.content_hash = "nothex".to_string();
        assert!(matches!(
            RuleSetVersion::try_from(bad),
            Err(RuleSetStoreError::Backend(_))
        ));
        let mut bad = row();
        bad.ruleset_key = "bad key".to_string();
        assert!(matches!(
            RuleSetVersion::try_from(bad),
            Err(RuleSetStoreError::Backend(_))
        ));
    }
}
```

- [ ] **Step 6: Create `tests/no_ai_gateway.rs`.**

```rust
#[test]
fn crate_does_not_depend_on_ai_gateway() {
    let manifest = include_str!("../Cargo.toml");
    assert!(!manifest.contains("ai-gateway"));
}
```

- [ ] **Step 7: Create `tests/dependency_guard.rs`.**

```rust
#[test]
fn core_ruleset_crate_stays_pure() {
    let manifest = include_str!("../../automation-ruleset/Cargo.toml");
    assert!(!manifest.contains("sqlx"));
    assert!(!manifest.contains("automation-ruleset-postgres"));
}
```

- [ ] **Step 8: Gates (DB-independent).**

Run: `$HOME/.cargo/bin/cargo test -p automation-ruleset-postgres`
Expected: PASS — row conversion + Backend tests + guards. (`sqlx::migrate!` embeds the migration at compile time; no DB needed.)

Run: `$HOME/.cargo/bin/cargo clippy -p automation-ruleset-postgres --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 9: Commit.**

```bash
git add Cargo.toml Cargo.lock crates/automation-ruleset-postgres/ migrations/202607110002_create_automation_rulesets.sql
git commit -m "feat(automation-ruleset-postgres): crate skeleton + migration + row mapping"
```

---

## Task 2 — Store struct + read ops + activate (chunk B)

**Files:**
- Modify: `crates/automation-ruleset-postgres/src/lib.rs`

**Interfaces:**
- Produces: `PostgresRuleSetStore<H: RuleSetHasher>` with `new(pool)` (default Sha256 hasher) and `with_hasher(pool, hasher)`; `get_version`/`list_versions`/`active`/`activate` implementations. `publish` is added in Task 3.

- [ ] **Step 1: Add imports + struct + constructors.** At the top of `src/lib.rs`, extend the imports and add the struct:

```rust
use automation_ruleset::{
    PublishOutcome, PublishRuleSetRequest, RuleSetActivation, RuleSetContentHash, RuleSetHasher,
    RuleSetKey, RuleSetSchemaVersion, RuleSetStore, RuleSetStoreError, RuleSetVersion,
    RuleSetVersionId, Sha256RuleSetHasher,
};
use automation_state::InteractionRuleSet;
use discord_model::{GuildId, UserId};
use sqlx::PgPool;
use std::sync::Arc;
```

Remove the now-duplicated **top-of-file** `use automation_ruleset::{...}` / `use discord_model::{...}` / `use automation_state::InteractionRuleSet;` lines from Task 1 Step 5 (consolidate into this single import block; leave the separate `#[cfg(test)] mod tests` imports untouched). `CURRENT_RULESET_SCHEMA_VERSION` is intentionally **not** imported yet — it is only used by `publish` (added in Task 3). Then add:

```rust
const VERSION_COLUMNS: &str =
    "guild_id, ruleset_key, version, schema_version, definition, content_hash, created_by";

pub struct PostgresRuleSetStore<H: RuleSetHasher = Sha256RuleSetHasher> {
    pool: PgPool,
    hasher: Arc<H>,
}

impl PostgresRuleSetStore<Sha256RuleSetHasher> {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            hasher: Arc::new(Sha256RuleSetHasher),
        }
    }
}

impl<H: RuleSetHasher> PostgresRuleSetStore<H> {
    pub fn with_hasher(pool: PgPool, hasher: H) -> Self {
        Self {
            pool,
            hasher: Arc::new(hasher),
        }
    }
}
```

`hasher: Arc<H>` keeps the store `Send + Sync` whenever `H: Send + Sync` (both `Sha256RuleSetHasher` and the test `FixedHasher` are unit structs, so they qualify), and lets the whole store be shared cheaply. `self.hasher.hash(...)` still works via `Arc`'s `Deref`.

- [ ] **Step 2: Implement read ops + activate** (publish is a stub until Task 3). Add:

```rust
impl<H: RuleSetHasher> RuleSetStore for PostgresRuleSetStore<H> {
    async fn publish(
        &self,
        _request: PublishRuleSetRequest,
    ) -> Result<PublishOutcome, RuleSetStoreError> {
        unimplemented!()
    }

    async fn get_version(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
        let row = sqlx::query_as::<_, RuleSetVersionRow>(&format!(
            "SELECT {VERSION_COLUMNS} FROM automation_ruleset_versions \
             WHERE guild_id = $1 AND ruleset_key = $2 AND version = $3"
        ))
        .bind(guild_id.to_string())
        .bind(key.as_str())
        .bind(i64::from(version.get()))
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(RuleSetVersion::try_from).transpose()
    }

    async fn list_versions(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Vec<RuleSetVersion>, RuleSetStoreError> {
        let rows = sqlx::query_as::<_, RuleSetVersionRow>(&format!(
            "SELECT {VERSION_COLUMNS} FROM automation_ruleset_versions \
             WHERE guild_id = $1 AND ruleset_key = $2 ORDER BY version"
        ))
        .bind(guild_id.to_string())
        .bind(key.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter().map(RuleSetVersion::try_from).collect()
    }

    async fn activate(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
        version: RuleSetVersionId,
    ) -> Result<RuleSetActivation, RuleSetStoreError> {
        let row = sqlx::query(
            "INSERT INTO automation_ruleset_activations (guild_id, ruleset_key, active_version) \
             SELECT guild_id, ruleset_key, version FROM automation_ruleset_versions \
             WHERE guild_id = $1 AND ruleset_key = $2 AND version = $3 \
             ON CONFLICT (guild_id, ruleset_key) DO UPDATE SET active_version = EXCLUDED.active_version \
             RETURNING active_version",
        )
        .bind(guild_id.to_string())
        .bind(key.as_str())
        .bind(i64::from(version.get()))
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        match row {
            Some(_) => Ok(RuleSetActivation {
                guild_id,
                ruleset_key: key.clone(),
                active_version: version,
            }),
            None => Err(RuleSetStoreError::VersionNotFound),
        }
    }

    async fn active(
        &self,
        guild_id: GuildId,
        key: &RuleSetKey,
    ) -> Result<Option<RuleSetVersion>, RuleSetStoreError> {
        let row = sqlx::query_as::<_, RuleSetVersionRow>(&format!(
            "SELECT {} FROM automation_ruleset_versions v \
             JOIN automation_ruleset_activations a \
               ON a.guild_id = v.guild_id AND a.ruleset_key = v.ruleset_key \
              AND a.active_version = v.version \
             WHERE v.guild_id = $1 AND v.ruleset_key = $2",
            VERSION_COLUMNS
                .split(", ")
                .map(|c| format!("v.{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .bind(guild_id.to_string())
        .bind(key.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(RuleSetVersion::try_from).transpose()
    }
}
```

Note: `activate` returns the typed `RuleSetActivation` directly (the RETURNING row confirms a version existed); the composite FK is the DB-level backstop. `active` joins activations→versions so a returned row is always a full immutable artifact.

- [ ] **Step 3: Gates.**

Run: `$HOME/.cargo/bin/cargo build -p automation-ruleset-postgres && $HOME/.cargo/bin/cargo test -p automation-ruleset-postgres`
Expected: PASS (compiles; DB-less tests still green; `publish` stub is not exercised without a DB).

Run: `$HOME/.cargo/bin/cargo clippy -p automation-ruleset-postgres --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 4: Commit.**

```bash
git add crates/automation-ruleset-postgres/src/lib.rs
git commit -m "feat(automation-ruleset-postgres): store struct + get/list/active/activate"
```

---

## Task 3 — Head-row publish transaction (chunk C)

**Files:**
- Modify: `crates/automation-ruleset-postgres/src/lib.rs`

**Interfaces:**
- Consumes: `automation_core::validate_structural` (via the `automation-core` dep — add it), the hasher, `sqlx` transactions. The transaction executor pattern (`&mut *tx`, `query_scalar`, `FOR UPDATE`) is pre-verified to compile.

- [ ] **Step 1: Add `automation-core` dependency** (publish calls `validate_structural`). In `Cargo.toml` `[dependencies]`:

```toml
automation-core = { path = "../automation-core" }
```

- [ ] **Step 2: Replace the `publish` stub** with the head-row transaction. First add `CURRENT_RULESET_SCHEMA_VERSION` to the top-of-file `use automation_ruleset::{...}` block (publish now uses it). Then:

```rust
    async fn publish(
        &self,
        request: PublishRuleSetRequest,
    ) -> Result<PublishOutcome, RuleSetStoreError> {
        automation_core::validate_structural(&request.definition)
            .map_err(RuleSetStoreError::InvalidDefinition)?;
        let schema_version = CURRENT_RULESET_SCHEMA_VERSION;
        let content_hash = self
            .hasher
            .hash(schema_version, &request.definition)
            .map_err(|error| match error {
                automation_ruleset::RuleSetHashError::Serialization(message) => {
                    RuleSetStoreError::Canonicalization(message)
                }
            })?;
        let guild = request.guild_id.to_string();
        let key = request.ruleset_key.as_str();
        let hash_hex = content_hash.to_hex();

        let mut tx = self.pool.begin().await.map_err(backend)?;

        sqlx::query(
            "INSERT INTO automation_ruleset_heads (guild_id, ruleset_key, next_version) \
             VALUES ($1, $2, 1) ON CONFLICT (guild_id, ruleset_key) DO NOTHING",
        )
        .bind(&guild)
        .bind(key)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;

        let next_version: i64 = sqlx::query_scalar(
            "SELECT next_version FROM automation_ruleset_heads \
             WHERE guild_id = $1 AND ruleset_key = $2 FOR UPDATE",
        )
        .bind(&guild)
        .bind(key)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;

        let existing = sqlx::query_as::<_, RuleSetVersionRow>(&format!(
            "SELECT {VERSION_COLUMNS} FROM automation_ruleset_versions \
             WHERE guild_id = $1 AND ruleset_key = $2 AND content_hash = $3"
        ))
        .bind(&guild)
        .bind(key)
        .bind(&hash_hex)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;

        if let Some(row) = existing {
            let existing_version = RuleSetVersion::try_from(row)?;
            if existing_version.schema_version == schema_version
                && existing_version.definition == request.definition
            {
                tx.commit().await.map_err(backend)?;
                return Ok(PublishOutcome::Reused(existing_version));
            }
            tx.rollback().await.map_err(backend)?;
            return Err(RuleSetStoreError::HashCollision);
        }

        let version = match u32::try_from(next_version)
            .ok()
            .and_then(|value| RuleSetVersionId::new(value).ok())
        {
            Some(version) => version,
            None => {
                tx.rollback().await.map_err(backend)?;
                return Err(RuleSetStoreError::VersionOverflow);
            }
        };

        sqlx::query(&format!(
            "INSERT INTO automation_ruleset_versions ({VERSION_COLUMNS}) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)"
        ))
        .bind(&guild)
        .bind(key)
        .bind(i64::from(version.get()))
        .bind(i64::from(schema_version.get()))
        .bind(sqlx::types::Json(&request.definition))
        .bind(&hash_hex)
        .bind(request.created_by.to_string())
        .execute(&mut *tx)
        .await
        .map_err(backend)?;

        sqlx::query(
            "UPDATE automation_ruleset_heads SET next_version = next_version + 1 \
             WHERE guild_id = $1 AND ruleset_key = $2",
        )
        .bind(&guild)
        .bind(key)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;

        tx.commit().await.map_err(backend)?;

        Ok(PublishOutcome::Created(RuleSetVersion {
            guild_id: request.guild_id,
            ruleset_key: request.ruleset_key,
            version,
            schema_version,
            definition: request.definition,
            content_hash,
            created_by: request.created_by,
        }))
    }
```

Rationale locked by the invariants: `INSERT … ON CONFLICT DO NOTHING` ensures the head exists, `SELECT … FOR UPDATE` serializes `(guild,key)` publishes, dedup runs **before** any version consumption, `HashCollision` compares **both** `schema_version` and `definition`, `VersionOverflow` fires when `next_version > u32::MAX` (the DB `CHECK (next_version BETWEEN 1 AND 4294967296)` lets `u32::MAX` itself be the last usable version). On a mid-transaction sqlx error the `?` early-returns and `tx` drops → sqlx rolls back (version INSERT undone, head unchanged).

- [ ] **Step 3: Gates (DB-independent — publish still not exercised without DB).**

Run: `$HOME/.cargo/bin/cargo build -p automation-ruleset-postgres && $HOME/.cargo/bin/cargo test -p automation-ruleset-postgres`
Expected: PASS (compiles; the pre-verified transaction pattern; DB-less tests green).

Run: `$HOME/.cargo/bin/cargo clippy -p automation-ruleset-postgres --all-targets -- -D warnings && $HOME/.cargo/bin/cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 4: Commit.**

```bash
git add crates/automation-ruleset-postgres/Cargo.toml crates/automation-ruleset-postgres/src/lib.rs Cargo.lock
git commit -m "feat(automation-ruleset-postgres): head-row FOR UPDATE publish transaction"
```

---

## Task 4 — Real-Postgres integration tests (chunks D + E)

**Files:**
- Create: `crates/automation-ruleset-postgres/tests/postgres_ruleset.rs`

**Interfaces:**
- `#[ignore]`d tests requiring `STARRING_TEST_DATABASE_URL` (DB name must contain `test`). Each test uses a distinct synthetic guild and cleans up. The four completion-evidence tests are: 20-way concurrent same-content, distinct-content concurrent, mid-transaction rollback, reconnect durability.

- [ ] **Step 1: Create the test harness + tests.**

```rust
use std::sync::Arc;

use automation_ruleset::{
    PublishOutcome, PublishRuleSetRequest, RuleSetContentHash, RuleSetHashError, RuleSetHasher,
    RuleSetKey, RuleSetSchemaVersion, RuleSetStore, RuleSetStoreError, RuleSetVersionId,
};
use automation_ruleset_postgres::{PostgresRuleSetStore, MIGRATOR};
use automation_state::{
    ActionSpec, ActionTarget, InstanceRef, InteractionRule, InteractionRuleSet, RoleRef,
    TriggerSpec,
};
use discord_model::{GuildId, UserId};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

fn database_url() -> String {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL must be set for ignored postgres tests");
    assert!(
        url.contains("test"),
        "refusing to run against a database whose name does not contain 'test'"
    );
    url
}

async fn pool() -> PgPool {
    let pool = PgPoolOptions::new()
        .max_connections(24)
        .connect(&database_url())
        .await
        .expect("connect");
    MIGRATOR.run(&pool).await.expect("migrate");
    pool
}

async fn cleanup(pool: &PgPool, guild: GuildId) {
    let g = guild.to_string();
    for table in [
        "automation_ruleset_activations",
        "automation_ruleset_versions",
        "automation_ruleset_heads",
    ] {
        sqlx::query(&format!("DELETE FROM {table} WHERE guild_id = $1"))
            .bind(&g)
            .execute(pool)
            .await
            .unwrap();
    }
}

fn definition(alias: &str) -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![],
        modals: vec![],
        rules: vec![InteractionRule {
            key: "r".to_string(),
            trigger: TriggerSpec::InstanceAction {
                action: "join".to_string(),
            },
            actions: vec![ActionSpec::GrantRole {
                role: RoleRef::Instance {
                    instance: InstanceRef::Event,
                    alias: alias.to_string(),
                },
                target: ActionTarget::Actor,
            }],
        }],
    }
}

fn request(guild: GuildId, key: &RuleSetKey, def: InteractionRuleSet) -> PublishRuleSetRequest {
    PublishRuleSetRequest {
        guild_id: guild,
        ruleset_key: key.clone(),
        definition: def,
        created_by: UserId(1),
    }
}

fn key() -> RuleSetKey {
    RuleSetKey::parse("studyroom").unwrap()
}

async fn head_next(pool: &PgPool, guild: GuildId, key: &RuleSetKey) -> i64 {
    sqlx::query_scalar(
        "SELECT next_version FROM automation_ruleset_heads WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(guild.to_string())
    .bind(key.as_str())
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn concurrent_same_content_creates_one_reuses_rest() {
    let pool = pool().await;
    let guild = GuildId(9_000_001);
    cleanup(&pool, guild).await;
    let store = Arc::new(PostgresRuleSetStore::new(pool.clone()));
    let barrier = Arc::new(tokio::sync::Barrier::new(20));

    let mut handles = Vec::new();
    for _ in 0..20 {
        let store = store.clone();
        let barrier = barrier.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            store
                .publish(request(guild, &key(), definition("member_role")))
                .await
        }));
    }
    let mut created = 0;
    let mut reused = 0;
    for handle in handles {
        match handle.await.unwrap().unwrap() {
            PublishOutcome::Created(_) => created += 1,
            PublishOutcome::Reused(_) => reused += 1,
        }
    }
    assert_eq!(created, 1);
    assert_eq!(reused, 19);
    assert_eq!(store.list_versions(guild, &key()).await.unwrap().len(), 1);
    assert_eq!(head_next(&pool, guild, &key()).await, 2);
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn concurrent_distinct_content_no_version_collision() {
    let pool = pool().await;
    let guild = GuildId(9_000_002);
    cleanup(&pool, guild).await;
    let store = Arc::new(PostgresRuleSetStore::new(pool.clone()));
    let barrier = Arc::new(tokio::sync::Barrier::new(10));

    let mut handles = Vec::new();
    for i in 0..10 {
        let store = store.clone();
        let barrier = barrier.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            store
                .publish(request(guild, &key(), definition(&format!("alias_{i}"))))
                .await
        }));
    }
    let mut versions = Vec::new();
    for handle in handles {
        if let PublishOutcome::Created(v) = handle.await.unwrap().unwrap() {
            versions.push(v.version.get());
        }
    }
    versions.sort_unstable();
    versions.dedup();
    assert_eq!(versions.len(), 10);
    assert_eq!(store.list_versions(guild, &key()).await.unwrap().len(), 10);
    cleanup(&pool, guild).await;
}

async fn drop_head_trigger(pool: &PgPool) {
    sqlx::query(
        "DROP TRIGGER IF EXISTS starring_test_fail_ruleset_head_update \
         ON automation_ruleset_heads",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DROP FUNCTION IF EXISTS starring_test_fail_ruleset_head_update()")
        .execute(pool)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn mid_transaction_failure_rolls_back_version_and_head() {
    let pool = pool().await;
    let guild = GuildId(9_000_003);
    cleanup(&pool, guild).await;
    drop_head_trigger(&pool).await;
    let store = PostgresRuleSetStore::new(pool.clone());

    store
        .publish(request(guild, &key(), definition("v1")))
        .await
        .unwrap();

    sqlx::query(
        "CREATE FUNCTION starring_test_fail_ruleset_head_update() RETURNS trigger AS $$ \
         BEGIN RAISE EXCEPTION 'forced head update failure'; END; $$ LANGUAGE plpgsql",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(&format!(
        "CREATE TRIGGER starring_test_fail_ruleset_head_update \
         BEFORE UPDATE ON automation_ruleset_heads \
         FOR EACH ROW WHEN (NEW.guild_id = '{}') \
         EXECUTE FUNCTION starring_test_fail_ruleset_head_update()",
        guild
    ))
    .execute(&pool)
    .await
    .unwrap();

    let result = store.publish(request(guild, &key(), definition("v2"))).await;

    drop_head_trigger(&pool).await;

    assert!(matches!(result, Err(RuleSetStoreError::Backend(_))));
    assert_eq!(store.list_versions(guild, &key()).await.unwrap().len(), 1);
    assert_eq!(head_next(&pool, guild, &key()).await, 2);
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn version_overflow_boundary() {
    let pool = pool().await;
    let guild = GuildId(9_000_004);
    cleanup(&pool, guild).await;
    let store = PostgresRuleSetStore::new(pool.clone());

    store
        .publish(request(guild, &key(), definition("first")))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE automation_ruleset_heads SET next_version = 4294967295 \
         WHERE guild_id = $1 AND ruleset_key = $2",
    )
    .bind(guild.to_string())
    .bind(key().as_str())
    .execute(&pool)
    .await
    .unwrap();

    let created = store
        .publish(request(guild, &key(), definition("max")))
        .await
        .unwrap();
    match created {
        PublishOutcome::Created(v) => assert_eq!(v.version.get(), u32::MAX),
        PublishOutcome::Reused(_) => panic!("expected Created"),
    }
    assert_eq!(head_next(&pool, guild, &key()).await, 4_294_967_296);

    let overflow = store
        .publish(request(guild, &key(), definition("over")))
        .await;
    assert!(matches!(overflow, Err(RuleSetStoreError::VersionOverflow)));
    assert_eq!(head_next(&pool, guild, &key()).await, 4_294_967_296);
    cleanup(&pool, guild).await;
}

struct FixedHasher;

impl RuleSetHasher for FixedHasher {
    fn hash(
        &self,
        _schema_version: RuleSetSchemaVersion,
        _definition: &InteractionRuleSet,
    ) -> Result<RuleSetContentHash, RuleSetHashError> {
        Ok(RuleSetContentHash::parse_hex(&"cd".repeat(32)).unwrap())
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn hash_collision_leaves_store_unchanged() {
    let pool = pool().await;
    let guild = GuildId(9_000_005);
    cleanup(&pool, guild).await;
    let store = PostgresRuleSetStore::with_hasher(pool.clone(), FixedHasher);

    store
        .publish(request(guild, &key(), definition("a")))
        .await
        .unwrap();
    let err = store
        .publish(request(guild, &key(), definition("b")))
        .await
        .unwrap_err();
    assert_eq!(err, RuleSetStoreError::HashCollision);
    assert_eq!(store.list_versions(guild, &key()).await.unwrap().len(), 1);
    assert_eq!(head_next(&pool, guild, &key()).await, 2);
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn activation_integrity_and_rollback() {
    let pool = pool().await;
    let guild = GuildId(9_000_006);
    cleanup(&pool, guild).await;
    let store = PostgresRuleSetStore::new(pool.clone());
    let v1 = RuleSetVersionId::FIRST;
    let v2 = RuleSetVersionId::new(2).unwrap();

    assert_eq!(
        store.activate(guild, &key(), v1).await.unwrap_err(),
        RuleSetStoreError::VersionNotFound
    );
    assert!(store.active(guild, &key()).await.unwrap().is_none());

    store
        .publish(request(guild, &key(), definition("a")))
        .await
        .unwrap();
    store
        .publish(request(guild, &key(), definition("b")))
        .await
        .unwrap();

    store.activate(guild, &key(), v2).await.unwrap();
    assert_eq!(
        store.active(guild, &key()).await.unwrap().unwrap().version,
        v2
    );
    store.activate(guild, &key(), v2).await.unwrap();
    store.activate(guild, &key(), v1).await.unwrap();
    assert_eq!(
        store.active(guild, &key()).await.unwrap().unwrap().version,
        v1
    );
    cleanup(&pool, guild).await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn reconnect_durability() {
    let guild = GuildId(9_000_007);
    {
        let pool_a = pool().await;
        cleanup(&pool_a, guild).await;
        let store = PostgresRuleSetStore::new(pool_a.clone());
        store
            .publish(request(guild, &key(), definition("a")))
            .await
            .unwrap();
        store.activate(guild, &key(), RuleSetVersionId::FIRST).await.unwrap();
        pool_a.close().await;
    }
    let pool_b = pool().await;
    let store = PostgresRuleSetStore::new(pool_b.clone());
    let versions = store.list_versions(guild, &key()).await.unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(
        store.active(guild, &key()).await.unwrap().unwrap().version,
        RuleSetVersionId::FIRST
    );
    cleanup(&pool_b, guild).await;
}
```

Notes:
- **`tokio::spawn` Send-safety:** the tasks call `store.publish(...)` on a **concrete** `Arc<PostgresRuleSetStore<Sha256RuleSetHasher>>` (not a `dyn RuleSetStore`), so the returned future's `Send`-ness is determined by the concrete impl — and the publish body only holds a `sqlx::Transaction<Postgres>` (which is `Send`) across awaits, so the future is `Send + 'static` and `spawn` compiles. **If a future Rust/sqlx change makes this not hold**, replace `tokio::spawn(...)` + join with `futures::future::join_all(futures)` (poll the concrete futures on one task — the head-row lock still serializes them at the DB, exercising the same contention); this avoids the `Send` requirement entirely. Step 2 compile-checks this before any DB run.
- `Barrier::wait()` runs **before** `publish` (which acquires a pooled connection) — with a 24-connection pool, all tasks acquire connections and then serialize on the head-row lock; no connection starvation.
- The rollback test uses a uniquely-named trigger/function (`starring_test_fail_ruleset_head_update`), calls `drop_head_trigger` (which uses `DROP … IF EXISTS`) **before** installing (clears any leaked prior run) and again **before the assertions** (so an assertion panic can never leave the trigger behind for the next run). The trigger fires only on the `UPDATE` of the head (the `INSERT … ON CONFLICT DO NOTHING` does not update, so it does not fire).
- The overflow test exercises both stages: `u32::MAX` becomes the last valid version (head → `u32::MAX + 1`), and the next publish is `VersionOverflow` with head unchanged.
- Each test uses a distinct synthetic `guild_id` and cleans up in FK-safe order (activations → versions → heads); no `TRUNCATE`.

- [ ] **Step 2: Compile-check the tests without a database** (verifies the concurrency `spawn` future is `Send` before any DB run — this is where a native-async-trait `Send` problem would surface):

Run: `$HOME/.cargo/bin/cargo test -p automation-ruleset-postgres --no-run`
Expected: compiles cleanly (all ignored tests build). If it fails on a `Send` bound around `spawn`, apply the `join_all` fallback from the Notes above and re-run.

- [ ] **Step 3: Claude runs the ignored tests against local Postgres** (Codex does NOT run these — no DB access):

```bash
STARRING_TEST_DATABASE_URL=postgres://localhost/starring_test \
  $HOME/.cargo/bin/cargo test -p automation-ruleset-postgres --test postgres_ruleset -- --ignored --test-threads=1
```
Expected: all 7 pass (concurrency, distinct, rollback, overflow, collision, activation, reconnect).

- [ ] **Step 4: Full workspace gate (DB-independent).**

```bash
$HOME/.cargo/bin/cargo build && \
$HOME/.cargo/bin/cargo test && \
$HOME/.cargo/bin/cargo clippy --all-targets -- -D warnings && \
$HOME/.cargo/bin/cargo fmt --all -- --check
```
Expected: whole workspace green (ignored postgres tests skipped without the env var).

- [ ] **Step 5: Commit.**

```bash
git add crates/automation-ruleset-postgres/tests/postgres_ruleset.rs
git commit -m "test(automation-ruleset-postgres): concurrency/rollback/overflow/reconnect"
```

---

## Self-Review

- **Spec coverage:** §1 crate/deps → Task 1-3. §2 schema (heads/versions/activations, CHECKs, FK) → Task 1 Step 4. §3 head-row publish transaction → Task 3. §4 get/list/activate(single-statement)/active → Task 2. §5 Row + TryFrom → Backend → Task 1. §6 `RuleSetHasher` seam (`PostgresRuleSetStore<H>`) → Task 2 + FixedHasher test. §7 application-enforced immutability (no UPDATE/DELETE of versions) → by construction. §8 tests: DB-less (row/Backend/no_ai_gateway/dependency guard) → Task 1; ignored concurrency/rollback/overflow/reconnect/collision/activation → Task 4.
- **Placeholder scan:** none — all code complete. The sqlx transaction pattern (`&mut *tx`, `query_scalar`, `FOR UPDATE`, `commit`/`rollback`) is pre-verified to compile; `sqlx::migrate!` embeds the migration (DB-independent build).
- **Type consistency:** `RuleSetVersionRow.version: i64` → `u32::try_from` → `RuleSetVersionId::new` (Backend on out-of-range). `version.get()` (u32) → `i64::from(...)` for BIGINT binds. `sqlx::types::Json(&definition)` for JSONB. `PostgresRuleSetStore<H = Sha256RuleSetHasher>` default type param enables `new(pool)`; `with_hasher` for the FixedHasher tests. `activate` maps empty RETURNING → `VersionNotFound`.
- **Concurrency-test safety:** `Barrier::wait()` precedes connection acquisition (no starvation with a 24-connection pool); rollback is forced by a temporary trigger (deterministic, cleaned up); overflow is a two-stage boundary test.
- **18c note (recorded, not implemented here):** runtime hydration should re-run `validate_structural` + recompute the content hash and compare against the stored `content_hash`, failing closed on mismatch — because 18b immutability is application-enforced and cannot fully prevent manual-SQL corruption.
