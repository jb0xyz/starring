# Phase 17d — PostgreSQL InstanceStore 설계 스펙

- **작성일**: 2026-07-11
- **상태**: 설계 확정 (구현 대기 — Codex 코드, Claude 실제 Postgres 통합검증)
- **범위**: Phase 17d — `InstanceStore`의 **실제 PostgreSQL 구현**을 별도 edge crate에. instance가 재시작 후에도 영속. 기본 build/test는 DB 독립, 실제 Postgres 통합 테스트는 명시 실행.
- **선행**: 17a(InstanceStore trait/InMemory) + 17b/17c(registration/join). 첫 DB — 기존 DB 의존 0.

---

## ⚠️ 최상위 원칙 (불변)
AI 설치시점 설계자, Runtime 결정론, event-time LLM 금지. no_ai_gateway 유지(SQL crate도). **`automation-instance`는 `Backend(String)` variant만 추가하되 SQLx/PostgreSQL 비의존**(모델+trait+InMemory+backend-agnostic error variant까지 순수 core에 OK; **sqlx/postgres dep 절대 금지**). DB 구현은 **별도 edge crate**. 불변식: `automation-instance → sqlx 금지`, `automation-instance-postgres → automation-instance`.

**목표(한 문장):** `PostgresInstanceStore`가 `InstanceStore`(register/get/list_by_guild/update_status)를 PgPool로 구현하고, register/get/list/update/**pool 재연결 후 get**이 실제 Postgres에서 durable하게 동작.

---

## 0. 범위

**포함:** 새 crate `automation-instance-postgres`(sqlx 0.8, PgPool) · `PostgresInstanceStore` · `AutomationInstanceRow`(FromRow) + `TryFrom<Row>` · root `/migrations` + `sqlx::migrate!` · `InstanceStoreError::Backend(String)`(17a 유예) · DB-less unit + **ignored 실제 Postgres 통합 테스트**(STARRING_TEST_DATABASE_URL).

**제외(→후속):** resource별 정규화 테이블(role_id 역조회 등) · PK 외 index · `#[sqlx::test]` 격리 DB · 컴파일타임 `query!` 매크로/offline 캐시 · 17e live(drop-in 교체) · reconciliation/audit 테이블.

---

## 1. crate 구조 + 의존 방향
```
automation-instance-postgres → automation-instance   (역방향 금지)
        ↓ sqlx 0.8
```
`automation-state`가 `automation-instance`에 의존하므로 순수 crate에 SQLx가 들어가면 Layer 2 스키마까지 DB 의존 → **절대 금지**.
```toml
# crates/automation-instance-postgres/Cargo.toml
[dependencies]
automation-instance = { path = "../automation-instance" }
discord-model = { path = "../discord-model" }
serde = { workspace = true }
serde_json = { workspace = true }
sqlx = { version = "0.8.6", default-features = false, features = ["runtime-tokio-rustls", "postgres", "json", "derive", "migrate"] }
[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```
`default-features = false` — 기본에 딸려오는 `any`/query-macro 계열 제외, 필요한 것만. `query!` 미사용 → `.sqlx` offline cache/빌드시 DATABASE_URL 불필요. `derive`=FromRow. + workspace members에 추가. `InstanceStoreError::Backend(String)`은 automation-instance에 추가(**backend-agnostic variant만, sqlx dep 없음**).

---

## 2. runtime query + FromRow (매크로 아님)
`sqlx::query_as::<_, AutomationInstanceRow>(...)` / `sqlx::query(...)` — **런타임 쿼리**. `query!`/`query_as!` 매크로 미사용 → **DB 없이 `cargo build`**(offline 캐시 불필요). 정확한 표현: *runtime query + FromRow mapping* — Rust 타입의 Encode/Decode/FromRow는 컴파일 보장, 테이블 존재·컬럼명·SQL/JSONB 정합은 **실제 실행(통합 테스트)에서 검증**. **따라서 통합 테스트는 선택이 아니라 17d 완료 게이트의 일부.**

---

## 3. schema / migration (root `/migrations`)
`/migrations/202607110001_create_automation_instances.sql`:
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
- **identity/status/kind/ruleset_key = 관계형 컬럼**(검색·격리·상태·복합PK), **resources만 JSONB**(가변 map). 전체 객체를 data JSONB 하나로 넣지 **않음**.
- **Discord IDs = TEXT**(guild_id/created_by/resources 내부 RoleId 등) — serde 문자열 규칙과 일치, u64↔signed BIGINT 경계 회피.
- crate에서 `pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");`. monorepo root migration history 하나로(향후 API/audit/ruleset 테이블 대비). 추가 index는 후속.
- **필수: `build.rs`로 migration 변경 감지** — `sqlx::migrate!`는 컴파일타임 embed proc macro라, migration 파일만 추가되고 Rust 소스가 안 바뀌면 Cargo가 재실행 안 할 수 있음. `crates/automation-instance-postgres/build.rs`:
  ```rust
  fn main() {
      println!("cargo:rerun-if-changed=../../migrations");
  }
  ```
  없으면 새 migration 추가해도 오래된 bundle이 빌드에 남음.
- **optional(비차단)**: `created_at TIMESTAMPTZ NOT NULL DEFAULT now()`, `updated_at ... DEFAULT now()` 지금 추가 가능(향후 API/운영/cleanup용, 비용 최소). **InstanceStore가 반환 안 하므로 row 타입/SELECT엔 미포함** — 17d 완료 필수 아님.

---

## 4. row 타입 분리 + TryFrom
```rust
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
impl TryFrom<AutomationInstanceRow> for AutomationInstance { /* parse guild/id/created_by/status, resources unwrap */ }
```
DB에 잘못된 값(형식 안 맞는 instance_id/status 등)이 있어도 **panic 금지 → `InstanceStoreError::Backend(...)`**. AutomationInstance→bind는 각 필드 to_string/serde.

---

## 5. Store SQL
- **register**: `INSERT INTO automation_instances (...) VALUES ($1..$7) ON CONFLICT (guild_id, instance_id) DO NOTHING` → `result.rows_affected() == 0` 이면 `DuplicateInstance`(SQLSTATE 파싱 불필요). resources = `Json(&instance.resources)`.
- **get**: `SELECT ... WHERE guild_id=$1 AND instance_id=$2` → `fetch_optional` → Option<Row> → TryFrom → Option<AutomationInstance>.
- **list_by_guild**: `SELECT ... WHERE guild_id=$1 ORDER BY instance_id` → fetch_all → TryFrom 각각(InMemory BTreeMap 결정성과 일치).
- **update_status**: `UPDATE ... SET status=$3 WHERE guild_id=$1 AND instance_id=$2` → `rows_affected()==0` 이면 `NotFound`.
- 모든 sqlx 에러 → `InstanceStoreError::Backend(format!(...))`.

---

## 6. InstanceStoreError::Backend
```rust
enum InstanceStoreError { DuplicateInstance, NotFound, Backend(String) }
```
- `Backend(String)`은 **log/audit용**(Postgres 상세). **사용자 Discord 문구엔 노출 금지** — interaction 실패는 별도 안전 문구("작업 상태를 저장하지 못했습니다.")로 매핑(17e에서). 17d core는 Backend까지만.

---

## 7. 테스트 전략
- **기본 `cargo test`는 DB 독립** — 실제 Postgres 통합 테스트는 `#[ignore]`.
- **ignored가 명시 실행됐는데 URL 없으면 panic**(조용한 skip 금지): `env::var("STARRING_TEST_DATABASE_URL").expect(...)`. + **DB 이름에 `test` 없으면 거부**(운영 DB 안전장치).
- 실행:
```bash
STARRING_TEST_DATABASE_URL=postgres://localhost/starring_test \
  cargo test -p automation-instance-postgres --test postgres_store -- --ignored --test-threads=1
```
- **durability = pool 재연결**: pool A 연결→migration→register→**pool A close**→pool B 새 연결→get→동일 확인(메모리 객체 재생성이 아니라 실제 DB 잔존 증명). 통합 테스트 하나가 register→재연결→get→list→update→get 관통.
- Docker 없음(로컬 psql만) → 전용 `starring_test` DB + 테스트 시작/끝 synthetic guild cleanup + `--test-threads=1`. (`#[sqlx::test]` 격리 DB는 후속.)

---

## 8. 테스트 (핵심)
**DB-less(기본 cargo test):** 1. Row→AutomationInstance 변환. 2. AutomationInstance→bind 값. 3. status encode/decode. 4. 잘못된 persisted InstanceId→Backend. 5. 잘못된 persisted status→Backend. 6. no_ai_gateway 가드.
**ignored 실제 Postgres:** 1. migration. 2. register→get roundtrip. 3. 같은 guild 중복→DuplicateInstance. 4. 다른 guild 같은 id→성공. 5. list_by_guild 격리. 6. list 결정 순서. 7. update_status. 8. missing update→NotFound. 9. resources JSONB role/channel/message roundtrip. 10. **pool close+reconnect 후 get 성공(17d 핵심 완료 증거)**.

---

## 9. 로드맵
```
17c✅ Dynamic Join Core   17d▶ PostgreSQL InstanceStore (이 스펙)
17e Durable Dynamic Join Live (tool이 InMemory→Postgres drop-in, 재시작 후에도 참가 버튼 작동)
```

---

## 10. Codex 핸드오프 (개요)
1. automation-instance: `InstanceStoreError::Backend(String)` 추가(변형만).
2. 새 crate automation-instance-postgres: Cargo.toml(sqlx) + workspace member + PostgresInstanceStore(PgPool, InstanceStore impl, runtime query) + AutomationInstanceRow/TryFrom + MIGRATOR + lib.
3. `/migrations/...create_automation_instances.sql`.
4. tests: DB-less(row 변환/Backend/no_ai_gateway) + `tests/postgres_store.rs`(#[ignore] 실제 Postgres, STARRING_TEST_DATABASE_URL expect, DB name 검사, reconnect durability).
5. 주석 없음. 게이트 build/test(DB 독립 green)/clippy(-D warnings)/fmt. push.
6. **Claude가 로컬 Postgres(psql, starring_test)로 ignored 통합 테스트 실행 검증**(register→reconnect→get durability).

## 최종 정리
17d = PostgreSQL InstanceStore. 별도 edge crate `automation-instance-postgres`(automation-instance 순수 유지), sqlx runtime query + FromMapping, resources만 JSONB + identity/status 관계형 컬럼, PK (guild_id,instance_id), Discord IDs TEXT, register ON CONFLICT/update rows_affected 매핑, Backend 에러. 기본 build/test DB 독립, 실제 Postgres 통합 테스트는 ignored+명시(STARRING_TEST_DATABASE_URL, DB name 검사), **reconnect durability가 완료 증거**. 17e에서 InMemory→Postgres drop-in.
