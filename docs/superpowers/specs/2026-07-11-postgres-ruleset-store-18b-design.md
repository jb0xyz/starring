# Phase 18b — PostgreSQL RuleSet Store 설계 스펙

- **작성일**: 2026-07-11
- **상태**: 설계 확정 (구현 대기 — Codex 코드, Claude 실제 Postgres 통합검증). 사용자 브레인스토밍 승인(Ⓑ′ head-row FOR UPDATE + 3 보강 + 테스트 세트).
- **범위**: Phase 18b — 18a `RuleSetStore`의 **실제 PostgreSQL 구현**을 별도 edge crate에. RuleSet version/activation이 프로세스 재시작 후에도 영속. 기본 build/test는 DB 독립, 실제 Postgres 통합 테스트는 명시(ignored).
- **선행**: 18a(RuleSetStore trait/타입/InMemory/RuleSetHasher) · 17d(PostgresInstanceStore 패턴: runtime query + FromRow + Backend + MIGRATOR + reconnect durability).

---

## ⚠️ 최상위 원칙 (불변)
AI 설치시점 설계자, Runtime 결정론, event-time LLM 금지. no_ai_gateway 유지(SQL crate도). **crate 수정 범위**: **새 edge crate `automation-ruleset-postgres → automation-ruleset`만.** `automation-ruleset`(코어)는 **무수정**(18a가 `RuleSetStoreError::Backend(String)`를 이미 예약). automation-core/state/instance/instance-postgres/runtime **무수정**. 불변식: `automation-ruleset → sqlx 금지`, `automation-ruleset-postgres → automation-ruleset`(역방향 금지). **핵심 계약(고정)**:

> PostgreSQL RuleSetStore는 `(GuildId, RuleSetKey)`별 head row를 트랜잭션에서 잠그고, 잠금 안에서 content dedup과 monotonic version 할당을 원자적으로 수행한다. Published version은 변경되지 않으며 activation은 존재하는 version만 참조한다.

---

## 0. 범위

**포함:** 새 crate `automation-ruleset-postgres`(sqlx 0.8, PgPool) · 3 테이블(heads/versions/activations) migration(root `/migrations`) · `PostgresRuleSetStore<H: RuleSetHasher>`(RuleSetStore 구현, **head-row FOR UPDATE** 트랜잭션 publish) · `RuleSetVersionRow`(FromRow)+`TryFrom` · `MIGRATOR` · DB-less unit + **ignored 실제 Postgres 통합**(동시성 Barrier + reconnect durability + activation 무결성, `STARRING_TEST_DATABASE_URL`).

**제외(→후속):** runtime DB hydration + active 조회(18c) · `RuleSetRouteId`/`AutomationInstance.ruleset_version` pin/instance migration(18c) · idempotent installation/attach(18d) · DB role 권한 분리·UPDATE/DELETE 차단 trigger(운영 배포 강화, immutability는 18b에선 application-enforced) · `#[sqlx::test]` 격리 DB · `query!` 컴파일타임 매크로.

---

## 1. crate 구조 + 의존
```
automation-ruleset-postgres → automation-ruleset (RuleSetStore, 타입, RuleSetHasher)
        ↓ sqlx 0.8, discord-model
```
```toml
# crates/automation-ruleset-postgres/Cargo.toml
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
17d 미러(default-features=false, runtime query — `query!` 미사용 → **DB 없이 build**). `tokio`에 `sync`(Barrier) 추가. + workspace members. `automation-state`는 JSONB `definition: InteractionRuleSet` 타입용. `tests/no_ai_gateway.rs` 가드.

---

## 2. schema / migration (root `/migrations`)
`/migrations/202607110002_create_automation_rulesets.sql` (17d의 202607110001 다음):
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
    UNIQUE (guild_id, ruleset_key, content_hash),
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
- **컬럼 타입**: Discord ID(guild_id/created_by) **TEXT**(u64↔BIGINT 경계 회피, 17d). version/schema/next_version **BIGINT**(u32 값이 signed BIGINT에 안전). content_hash **TEXT** lowercase hex(psql 가독 + RuleSetContentHash hex serde 일치 + CHECK). definition **JSONB**(`sqlx::types::Json<InteractionRuleSet>`).
- **상한 CHECK로 DB도 u32 불변식 강제**: version/schema ≤ 4294967295(u32::MAX). **next_version ≤ 4294967296**(= u32::MAX+1 overflow sentinel — v u32::MAX 발행 후 head가 4294967296이 되고 그 다음 publish부터 VersionOverflow, 즉 u32::MAX도 마지막 유효 버전으로 사용 가능). ruleset_key 정규식도 DB에서 강제.
- **activations composite FK** → versions PK. **ON DELETE RESTRICT**: active version은 삭제 불가(immutable + 정합성 최종 방어선). 명시적 SELECT 검사(§4)는 사용자 친화 typed error용, FK는 미래 코드/수동 SQL/버그 방어선.
- **MIGRATOR**: `pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");`(17d와 **동일 root history** — instance-postgres와 ruleset-postgres가 같은 migration 순서 공유, 서비스별 분기 방지). **build.rs `rerun-if-changed=../../migrations`**(17d 패턴, migration만 추가돼도 재컴파일 감지).

---

## 3. publish 트랜잭션 (head-row FOR UPDATE)
```
validate_structural(&def)         → 실패 InvalidDefinition (txn·DB 접근 전)
hasher.hash(CURRENT, &def)        → 실패 Canonicalization
tx = pool.begin()
  INSERT INTO heads (guild,key,next_version) VALUES ($1,$2,1) ON CONFLICT (guild,key) DO NOTHING
  SELECT next_version FROM heads WHERE guild=$1 AND key=$2 FOR UPDATE        -- (guild,key) 직렬화
  SELECT version, schema_version, definition FROM versions
      WHERE guild=$1 AND key=$2 AND content_hash=$3
    있음:
      schema_version == CURRENT AND definition == request.def  → tx.commit(); Reused(existing)
      else                                                     → tx.rollback(); HashCollision
    없음:
      v = next_version;  v > u32::MAX(4294967295) → tx.rollback(); VersionOverflow
      INSERT INTO versions (guild,key,version=v,schema,definition,content_hash,created_by)
      UPDATE heads SET next_version = next_version + 1 WHERE guild=$1 AND key=$2
      tx.commit(); Created(new)
```
- **`SELECT … FOR UPDATE`가 head row를 txn 종료까지 잠가** `(guild,key)`별 dedup+할당을 하나의 critical section으로. 첫 publish 두 개 동시: A가 head INSERT→lock→v1→commit, B는 INSERT ON CONFLICT에서 A 대기→lock 획득→hash v1 발견→Reused(v1). **counter-gap 없음**(dedup을 increment보다 먼저).
- **HashCollision 비교는 schema_version + definition 둘 다**(hash 입력에 schema 포함 → 정상이면 둘 다 일치, collision/corruption은 이 가정이 깨진 경우). 둘 중 하나라도 다르면 HashCollision.
- **트랜잭션 명시 종료**: Created/Reused → `commit()`(Reused도 변경 없어도 lock 즉시 해제 위해 명시 commit), HashCollision/VersionOverflow → `rollback()`, sqlx 에러 → rollback/drop(sqlx Transaction은 drop 시 rollback이지만 각 분기 명시). 모든 sqlx 에러 → `Backend(String)`.

---

## 4. 나머지 ops
- **get_version**: `SELECT ... FROM versions WHERE guild=$1 AND key=$2 AND version=$3` → fetch_optional → TryFrom.
- **list_versions**: `... WHERE guild=$1 AND key=$2 ORDER BY version` → fetch_all → TryFrom 각각.
- **activate** (한 문장 + FK 백스톱):
  ```sql
  INSERT INTO automation_ruleset_activations (guild_id, ruleset_key, active_version)
  SELECT guild_id, ruleset_key, version FROM automation_ruleset_versions
      WHERE guild_id = $1 AND ruleset_key = $2 AND version = $3
  ON CONFLICT (guild_id, ruleset_key) DO UPDATE SET active_version = EXCLUDED.active_version
  RETURNING guild_id, ruleset_key, active_version
  ```
  `fetch_optional` → 반환 행 없음(version 미존재로 SELECT가 0행) → **VersionNotFound**. 있으면 `RuleSetActivation` 반환. **같은 version 재activate = idempotent**(DO UPDATE, 동일 값). FK가 백스톱.
- **active**: `SELECT v.* FROM activations a JOIN versions v ON (a.guild_id,a.ruleset_key,a.active_version)=(v.guild_id,v.ruleset_key,v.version) WHERE a.guild_id=$1 AND a.ruleset_key=$2` → fetch_optional → TryFrom, 없으면 None.

---

## 5. Row 타입 + TryFrom → Backend
```rust
#[derive(sqlx::FromRow)]
struct RuleSetVersionRow {
    guild_id: String, ruleset_key: String, version: i64, schema_version: i64,
    definition: sqlx::types::Json<InteractionRuleSet>, content_hash: String, created_by: String,
}
impl TryFrom<RuleSetVersionRow> for RuleSetVersion { /* parse 각 필드 */ }
```
- guild_id/created_by → GuildId/UserId parse, ruleset_key → `RuleSetKey::parse`, version/schema(i64→u32 범위 검사) → `RuleSetVersionId::new`/`RuleSetSchemaVersion::new`, content_hash → `RuleSetContentHash::parse_hex`, definition unwrap.
- **잘못된 저장값(범위 밖 version, 형식 안 맞는 hash/key 등)은 panic 금지 → `Backend(...)`**(17d TryFrom 패턴). CHECK가 있어도 코드가 최종 방어(read 경로).

---

## 6. RuleSetHasher seam 유지 (18a/18b 대칭)
`PostgresRuleSetStore<H: RuleSetHasher>` — hasher 주입(InMemory와 동일). publish가 `self.hasher.hash(...)` 사용. **Postgres가 Sha256을 하드코딩하면 안 됨** — 그러면 18a/18b의 HashCollision 계약 검증 수준이 갈라짐. 통합 테스트가 `FixedHasher`(다른 두 def에 같은 hash) 주입 → HashCollision + version/head 불변 검증. 프로덕션은 `PostgresRuleSetStore::new(pool, Sha256RuleSetHasher)` 또는 `Default` hasher 편의 생성.

---

## 7. Immutability (application-enforced)
18b는 versions에 **INSERT/SELECT만 제공, UPDATE/DELETE 미제공** → **application-enforced immutability**. DB role 권한 분리나 UPDATE/DELETE 차단 trigger는 운영 배포 단계 강화(후속). 스펙/문서엔 "application-enforced"로 정확히 표기.

---

## 8. 테스트
**DB-less(기본 cargo test):** 1. RuleSetVersionRow→RuleSetVersion 변환. 2. 잘못된 persisted version(범위 밖)/hash/key/guild → Backend. 3. no_ai_gateway 가드. 4. dependency guard(`automation-ruleset → automation-ruleset-postgres` 부재).

**ignored 실제 Postgres**(`STARRING_TEST_DATABASE_URL` expect + DB name 'test' 검사, `--ignored`; 동시성 위해 multi-thread tokio):
```
완료 증거 4개:
1. 동일 content 20개 동시 publish(Barrier로 동시 시작) → Created 정확히 1 + 나머지 Reused(v1) + versions 1행 + next_version=2
2. 서로 다른 content 동시 publish → version 중복 없음, 전부 저장 (번호-content 매핑 미단언)
3. txn 중간 실패(강제) → version INSERT·head UPDATE 모두 rollback
4. pool close/reconnect → publish→새 pool→get_version/list/active 동일 (durability)
+ Created/Reused/변경 content(v2)/재publish next 불변
+ HashCollision(FixedHasher) → version·head 불변
+ VersionOverflow(head를 4294967296 근처 seed) → version·head 불변
+ activation 무결성: 없는 version activate → VersionNotFound + activation row 불변 / 같은 version 재activate idempotent / v2→v1 activate → active()=v1
+ resources... JSONB roundtrip(정의 그대로 왕복)
```
Docker 없음(로컬 psql, 17d starring_test 재사용) → 전용 test DB + 시작/끝 synthetic guild cleanup + 동시성 테스트는 별 connection. **동시성은 반드시 실제 DB**(InMemory Mutex와 다른 경로 — Postgres row lock 검증이 18b 고유 가치).

---

## 9. 로드맵
```
18a✅ RuleSet Registry Core   18b▶ PostgreSQL RuleSet Store (이 스펙)
18c RuleSetActivationService(structural+bindings+policy 게이트)+runtime DB hydration+RuleSetRouteId+AutomationInstance.ruleset_version pin+instance migration
18d idempotent installation(Create 패널 중복 방지)+attach-after-register  ·  18e durable RuleSet live
후속: 19 Run Ledger+idempotency · 20 reconciliation · 21 API · 22 Web UI · 23 AI rule authoring
```

---

## 10. Codex 핸드오프 (개요)
1. 새 crate `automation-ruleset-postgres`: Cargo.toml(sqlx) + workspace member + build.rs(rerun-if-changed) + `PostgresRuleSetStore<H>`(RuleSetStore impl, head-row FOR UPDATE publish 트랜잭션, 명시 commit/rollback) + `RuleSetVersionRow`/TryFrom + `MIGRATOR` + lib.
2. `/migrations/202607110002_create_automation_rulesets.sql`(heads/versions/activations, 상한 CHECK + FK).
3. tests: DB-less(row 변환/Backend/no_ai_gateway/dependency guard) + `tests/postgres_ruleset.rs`(#[ignore], STARRING_TEST_DATABASE_URL expect + DB name 검사, 동시성 Barrier + reconnect + activation 무결성). 주석 없음. 게이트 build/test(DB 독립 green)/clippy(-D warnings)/fmt. push. **live/DB 접속 없음**(Codex).
4. **Claude가 로컬 Postgres(psql, starring_test)로 ignored 통합 실행 검증**(동시성 20-publish + reconnect durability + activation 무결성).

## 최종 정리
18b = PostgreSQL RuleSet Store. 별도 edge crate(`automation-ruleset` 순수 유지), sqlx runtime query, **head-row `SELECT … FOR UPDATE`로 `(guild,key)`별 publish 직렬화 → 잠금 안에서 dedup(content_hash)+monotonic version 할당 원자화**(counter-gap 없음, retry 없음). heads/versions/activations 3테이블, version/schema/next_version 상한 CHECK(u32 강제, next는 +1 sentinel), content_hash lowercase hex TEXT + CHECK, definition JSONB, **activations composite FK(ON DELETE RESTRICT) + 명시적 존재 검사**로 VersionNotFound. `PostgresRuleSetStore<H: RuleSetHasher>`로 18a와 대칭(HashCollision 실제 테스트). application-enforced immutability. 단일 root migration history. **완료 = DB 독립 build/test green + 실제 Postgres 동시성(20-publish, Created 1/Reused 다수)·mid-txn rollback·reconnect durability·activation 무결성 검증(Claude)**. 18c에서 이 store를 runtime hydration에 연결.
