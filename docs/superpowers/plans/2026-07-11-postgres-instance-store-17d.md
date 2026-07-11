# Phase 17d — PostgreSQL InstanceStore Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. Steps use checkbox (`- [ ]`). 실제 Postgres 통합검증은 Claude.

**Goal:** `PostgresInstanceStore`가 `InstanceStore`를 PgPool로 구현 — instance가 재시작 후에도 영속. 기본 build/test는 DB 독립, 실제 Postgres 통합 테스트는 명시 실행.

**Architecture:** 새 edge crate `automation-instance-postgres`(sqlx runtime query). automation-instance는 `Backend` variant만 추가(sqlx 비의존). root `/migrations`.

## Global Constraints
- **코드 주석 금지.** **Codex 코드, live 통합검증은 Claude.**
- `automation-instance → sqlx 금지`. `query!` 매크로 미사용(DB 없이 build). 게이트 build/test(DB 독립 green)/clippy(-D warnings)/fmt. push.

---

## Task 1: automation-instance Backend + automation-instance-postgres crate

- [ ] **Step 1: `automation-instance/src/store.rs` — Backend variant**

`InstanceStoreError`에:
```rust
pub enum InstanceStoreError {
    DuplicateInstance,
    NotFound,
    Backend(String),
}
```
Run: `cargo test -p automation-instance` → 커밋 `feat(automation-instance): backend error variant`

- [ ] **Step 2: 새 crate scaffold**

`crates/automation-instance-postgres/Cargo.toml`:
```toml
[package]
name = "automation-instance-postgres"
version = "0.1.0"
edition.workspace = true

[dependencies]
automation-instance = { path = "../automation-instance" }
discord-model = { path = "../discord-model" }
serde = { workspace = true }
serde_json = { workspace = true }
sqlx = { version = "0.8.6", default-features = false, features = ["runtime-tokio-rustls", "postgres", "json", "derive", "migrate"] }

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```
workspace `Cargo.toml` members에 `"crates/automation-instance-postgres"` 추가.

`crates/automation-instance-postgres/build.rs`:
```rust
fn main() {
    println!("cargo:rerun-if-changed=../../migrations");
}
```

- [ ] **Step 3: `/migrations/202607110001_create_automation_instances.sql`**

```sql
CREATE TABLE automation_instances (
    guild_id    TEXT NOT NULL,
    instance_id TEXT NOT NULL,
    ruleset_key TEXT NOT NULL,
    kind        TEXT NOT NULL,
    created_by  TEXT NOT NULL,
    status      TEXT NOT NULL,
    resources   JSONB NOT NULL,
    PRIMARY KEY (guild_id, instance_id),
    CONSTRAINT automation_instances_instance_id_format CHECK (instance_id ~ '^[A-Za-z0-9_-]{1,32}$'),
    CONSTRAINT automation_instances_status_valid CHECK (status IN ('active','disabled','deleted')),
    CONSTRAINT automation_instances_resources_object CHECK (jsonb_typeof(resources) = 'object')
);
```

- [ ] **Step 4: `src/store.rs` — PostgresInstanceStore + Row + TryFrom**

```rust
use automation_instance::{
    AutomationInstance, InstanceId, InstanceKind, InstanceResources, InstanceStatus, InstanceStore,
    InstanceStoreError,
};
use discord_model::{GuildId, UserId};
use sqlx::PgPool;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

pub struct PostgresInstanceStore {
    pool: PgPool,
}

impl PostgresInstanceStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct AutomationInstanceRow {
    guild_id: String,
    instance_id: String,
    ruleset_key: String,
    kind: String,
    created_by: String,
    status: String,
    resources: sqlx::types::Json<InstanceResources>,
}

fn status_str(status: InstanceStatus) -> &'static str {
    match status {
        InstanceStatus::Active => "active",
        InstanceStatus::Disabled => "disabled",
        InstanceStatus::Deleted => "deleted",
    }
}

fn backend(error: impl std::fmt::Display) -> InstanceStoreError {
    InstanceStoreError::Backend(error.to_string())
}

impl TryFrom<AutomationInstanceRow> for AutomationInstance {
    type Error = InstanceStoreError;
    fn try_from(row: AutomationInstanceRow) -> Result<Self, Self::Error> {
        let guild_id = row
            .guild_id
            .parse::<GuildId>()
            .map_err(|_| backend(format!("invalid persisted guild_id: {}", row.guild_id)))?;
        let id = InstanceId::parse(&row.instance_id)
            .map_err(|error| backend(format!("invalid persisted instance_id: {error:?}")))?;
        let created_by = row
            .created_by
            .parse::<UserId>()
            .map_err(|_| backend(format!("invalid persisted created_by: {}", row.created_by)))?;
        let status = match row.status.as_str() {
            "active" => InstanceStatus::Active,
            "disabled" => InstanceStatus::Disabled,
            "deleted" => InstanceStatus::Deleted,
            other => return Err(backend(format!("invalid persisted status: {other}"))),
        };
        Ok(AutomationInstance {
            id,
            guild_id,
            ruleset_key: row.ruleset_key,
            kind: InstanceKind(row.kind),
            created_by,
            resources: row.resources.0,
            status,
        })
    }
}

impl InstanceStore for PostgresInstanceStore {
    async fn register(&self, instance: AutomationInstance) -> Result<(), InstanceStoreError> {
        let result = sqlx::query(
            "INSERT INTO automation_instances (guild_id, instance_id, ruleset_key, kind, created_by, status, resources) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (guild_id, instance_id) DO NOTHING",
        )
        .bind(instance.guild_id.to_string())
        .bind(instance.id.as_str())
        .bind(&instance.ruleset_key)
        .bind(&instance.kind.0)
        .bind(instance.created_by.to_string())
        .bind(status_str(instance.status))
        .bind(sqlx::types::Json(&instance.resources))
        .execute(&self.pool)
        .await
        .map_err(|error| backend(error))?;
        if result.rows_affected() == 0 {
            return Err(InstanceStoreError::DuplicateInstance);
        }
        Ok(())
    }

    async fn get(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
    ) -> Result<Option<AutomationInstance>, InstanceStoreError> {
        let row = sqlx::query_as::<_, AutomationInstanceRow>(
            "SELECT guild_id, instance_id, ruleset_key, kind, created_by, status, resources \
             FROM automation_instances WHERE guild_id = $1 AND instance_id = $2",
        )
        .bind(guild_id.to_string())
        .bind(instance_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| backend(error))?;
        row.map(AutomationInstance::try_from).transpose()
    }

    async fn list_by_guild(
        &self,
        guild_id: GuildId,
    ) -> Result<Vec<AutomationInstance>, InstanceStoreError> {
        let rows = sqlx::query_as::<_, AutomationInstanceRow>(
            "SELECT guild_id, instance_id, ruleset_key, kind, created_by, status, resources \
             FROM automation_instances WHERE guild_id = $1 ORDER BY instance_id",
        )
        .bind(guild_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|error| backend(error))?;
        rows.into_iter().map(AutomationInstance::try_from).collect()
    }

    async fn update_status(
        &self,
        guild_id: GuildId,
        instance_id: &InstanceId,
        status: InstanceStatus,
    ) -> Result<(), InstanceStoreError> {
        let result = sqlx::query(
            "UPDATE automation_instances SET status = $3 WHERE guild_id = $1 AND instance_id = $2",
        )
        .bind(guild_id.to_string())
        .bind(instance_id.as_str())
        .bind(status_str(status))
        .execute(&self.pool)
        .await
        .map_err(|error| backend(error))?;
        if result.rows_affected() == 0 {
            return Err(InstanceStoreError::NotFound);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(instance_id: &str, status: &str) -> AutomationInstanceRow {
        AutomationInstanceRow {
            guild_id: "7".to_string(),
            instance_id: instance_id.to_string(),
            ruleset_key: "studyroom_demo".to_string(),
            kind: "study_room".to_string(),
            created_by: "3".to_string(),
            status: status.to_string(),
            resources: sqlx::types::Json(InstanceResources::default()),
        }
    }

    #[test]
    fn valid_row_converts() {
        let instance = AutomationInstance::try_from(row("room1", "active")).unwrap();
        assert_eq!(instance.guild_id, GuildId(7));
        assert_eq!(instance.id.as_str(), "room1");
        assert_eq!(instance.status, InstanceStatus::Active);
        assert_eq!(instance.created_by, UserId(3));
        assert_eq!(instance.kind, InstanceKind("study_room".to_string()));
    }

    #[test]
    fn invalid_persisted_instance_id_is_backend() {
        assert!(matches!(
            AutomationInstance::try_from(row("bad id", "active")),
            Err(InstanceStoreError::Backend(_))
        ));
    }

    #[test]
    fn invalid_persisted_status_is_backend() {
        assert!(matches!(
            AutomationInstance::try_from(row("room1", "weird")),
            Err(InstanceStoreError::Backend(_))
        ));
    }
}
```

- [ ] **Step 5: `src/lib.rs`**

```rust
pub mod store;

pub use store::{PostgresInstanceStore, MIGRATOR};
```

- [ ] **Step 6: `tests/no_ai_gateway.rs`**

```rust
#[test]
fn manifest_does_not_depend_on_ai_gateway() {
    let manifest = include_str!("../Cargo.toml");
    assert!(!manifest.contains("ai-gateway"));
    assert!(!manifest.contains("ai_gateway"));
    assert!(!manifest.contains("llm"));
}
```

- [ ] **Step 7: DB 독립 build/test + 커밋**

Run: `cargo build -p automation-instance-postgres`(DB 없이 통과) + `cargo test -p automation-instance-postgres`(DB-less unit + no_ai_gateway; ignored 통합은 미실행).
```bash
git add crates/automation-instance-postgres migrations Cargo.toml crates/automation-instance
git commit -m "feat(automation-instance-postgres): PostgresInstanceStore + migration"
```

---

## Task 2: ignored 통합 테스트 + 게이트 + push

- [ ] **Step 1: `tests/postgres_store.rs`(#[ignore] 실제 Postgres)**

```rust
use std::collections::BTreeMap;

use automation_instance::{
    AutomationInstance, InstanceId, InstanceKind, InstanceResources, InstanceStatus, InstanceStore,
    InstanceStoreError,
};
use automation_instance_postgres::{PostgresInstanceStore, MIGRATOR};
use discord_model::{GuildId, RoleId, UserId};
use sqlx::PgPool;

fn require_test_db() -> String {
    let url = std::env::var("STARRING_TEST_DATABASE_URL")
        .expect("STARRING_TEST_DATABASE_URL required for ignored postgres integration tests");
    assert!(
        url.contains("test"),
        "refusing non-test DB: url must contain 'test'"
    );
    url
}

fn instance(guild: u64, id: &str) -> AutomationInstance {
    let mut roles = BTreeMap::new();
    roles.insert("member_role".to_string(), RoleId(900_100));
    AutomationInstance {
        id: InstanceId::parse(id).unwrap(),
        guild_id: GuildId(guild),
        ruleset_key: "studyroom_demo".to_string(),
        kind: InstanceKind("study_room".to_string()),
        created_by: UserId(3),
        resources: InstanceResources {
            roles,
            channels: BTreeMap::new(),
            messages: BTreeMap::new(),
        },
        status: InstanceStatus::Active,
    }
}

async fn cleanup(pool: &PgPool, guild: u64) {
    let _ = sqlx::query("DELETE FROM automation_instances WHERE guild_id = $1")
        .bind(guild.to_string())
        .execute(pool)
        .await;
}

#[tokio::test]
#[ignore]
async fn register_reconnect_get_durability() {
    let url = require_test_db();
    let pool_a = PgPool::connect(&url).await.unwrap();
    MIGRATOR.run(&pool_a).await.unwrap();
    cleanup(&pool_a, 990001).await;
    let store_a = PostgresInstanceStore::new(pool_a.clone());
    let value = instance(990001, "durable_room");
    store_a.register(value.clone()).await.unwrap();
    assert_eq!(
        store_a.register(value.clone()).await.unwrap_err(),
        InstanceStoreError::DuplicateInstance
    );
    pool_a.close().await;

    let pool_b = PgPool::connect(&url).await.unwrap();
    let store_b = PostgresInstanceStore::new(pool_b.clone());
    let id = InstanceId::parse("durable_room").unwrap();
    assert_eq!(store_b.get(GuildId(990001), &id).await.unwrap(), Some(value));
    assert_eq!(store_b.list_by_guild(GuildId(990001)).await.unwrap().len(), 1);
    store_b
        .update_status(GuildId(990001), &id, InstanceStatus::Disabled)
        .await
        .unwrap();
    assert_eq!(
        store_b.get(GuildId(990001), &id).await.unwrap().unwrap().status,
        InstanceStatus::Disabled
    );
    cleanup(&pool_b, 990001).await;
    pool_b.close().await;
}

#[tokio::test]
#[ignore]
async fn guild_isolation_and_missing() {
    let url = require_test_db();
    let pool = PgPool::connect(&url).await.unwrap();
    MIGRATOR.run(&pool).await.unwrap();
    cleanup(&pool, 990002).await;
    cleanup(&pool, 990003).await;
    let store = PostgresInstanceStore::new(pool.clone());
    store.register(instance(990002, "room1")).await.unwrap();
    store.register(instance(990003, "room1")).await.unwrap();
    let id = InstanceId::parse("room1").unwrap();
    assert_eq!(store.get(GuildId(990002), &id).await.unwrap().unwrap().guild_id, GuildId(990002));
    assert!(store.get(GuildId(990099), &id).await.unwrap().is_none());
    assert_eq!(
        store
            .update_status(GuildId(990099), &id, InstanceStatus::Deleted)
            .await
            .unwrap_err(),
        InstanceStoreError::NotFound
    );
    cleanup(&pool, 990002).await;
    cleanup(&pool, 990003).await;
    pool.close().await;
}
```

- [ ] **Step 2~5: 게이트 + push**
- `cargo build`(경고0) / `cargo test`(전체 ~356, ignored 제외 green) / `cargo clippy --all-targets -- -D warnings`(0) / `cargo fmt --all -- --check` / `git push origin main`.
- 커밋: `test(automation-instance-postgres): postgres integration tests`

---

## Self-Review (스펙 대비)
- 새 edge crate(automation-instance-postgres → automation-instance, sqlx 역방향 없음), automation-instance는 Backend variant만(sqlx 비의존) ✅.
- sqlx 0.8.6 runtime query/query_as + FromRow, default-features=false, query! 미사용(DB 없이 build) ✅.
- resources JSONB + 관계형 컬럼 + PK(guild,instance) + TEXT ids, build.rs rerun-if-changed ✅.
- Row + TryFrom(잘못된 DB 값→Backend, panic 없음), register ON CONFLICT/update rows_affected 매핑 ✅.
- 기본 test DB 독립, ignored 통합(STARRING_TEST_DATABASE_URL expect·DB name 검사), **reconnect durability** ✅.

## Codex 핸드오프 (권장 2청크)
- **A** = Task 1(Backend + crate + store + migration + DB-less unit). build(DB 독립) + test. 커밋 2.
- **B** = Task 2(ignored 통합 테스트 + 게이트 + push). 커밋 1 + push.
보고: 테스트 수(ignored 제외) + 전체 + clippy/fmt + push 해시 + 이탈. **ignored 통합은 Claude가 로컬 Postgres로 실행.**

## Live 통합검증 (Claude, push 후)
`createdb starring_test` → `STARRING_TEST_DATABASE_URL=postgres://localhost/starring_test cargo test -p automation-instance-postgres --test postgres_store -- --ignored --test-threads=1` → register→reconnect→get durability 확인 → 정리.
