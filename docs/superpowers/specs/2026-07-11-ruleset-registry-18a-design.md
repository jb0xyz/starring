# Phase 18a — RuleSet Registry Core 설계 스펙

- **작성일**: 2026-07-11
- **상태**: 설계 확정 (구현 대기 — Codex 코드, Claude 독립검증). 사용자 브레인스토밍 승인(Ⓐ + 3 결정 + 5 보강 + 20 테스트).
- **범위**: Phase 18a — RuleSet 정의를 **버전된 immutable artifact로 영속·활성화**하는 순수 코어. 새 crate `automation-ruleset`. DB·런타임 배선 없음(InMemory + 결정론 테스트). Durable RuleSet Lifecycle 아크(18a~18e)의 기반.
- **선행**: 17a(InstanceStore/InMemory 패턴) · 17d(edge crate 분리 패턴) · automation-state(InteractionRuleSet) · automation-core(validate).

---

## ⚠️ 최상위 원칙 (불변)
AI 설치시점 설계자, Runtime 결정론, event-time LLM 금지. `automation-ruleset`도 ai-gateway 미의존(`tests/no_ai_gateway.rs`). **crate 수정 범위**: 새 crate `automation-ruleset`(순수 코어, **→ automation-core 의존 허용**) + automation-**core에 검증 3분할 추가**(`validate_structural`/`validate_bindings`/`validate`=합성, 최소 additive). automation-state/instance/postgres/runtime **무수정**. **의존 방향 guard(잠금)**: `automation-ruleset → automation-core` 허용, **`automation-core → automation-ruleset` 절대 금지**(순환 방지 — 18c hydration은 두 crate를 조립하는 상위 edge/service에 둠). (검증이 더 커지면 장기적으로 `automation-validation` 순수 crate 추출이 깔끔하나 18a에선 불필요.) **핵심 계약(고정)**:

> RuleSetVersion은 유효성이 검증된 InteractionRuleSet을 publish할 때 생성되는 immutable artifact다. Store는 `(GuildId, RuleSetKey)` 범위에서 monotonic version을 원자적으로 할당하며, schema version과 canonical definition의 SHA-256 hash가 동일하면 기존 artifact를 재사용한다. Publish는 activation을 변경하지 않는다.

부수 원칙: **Draft ≠ version**(version은 publish 순간의 immutable artifact). **Published artifact의 존재 ≠ 활성 상태**(활성은 별도 `RuleSetActivation`). **Invalid RuleSet은 hash 계산·version 소비 전에 거부**.

---

## 0. 범위

**포함:** 새 crate `automation-ruleset` · 타입(`RuleSetKey`/`RuleSetVersionId`/`RuleSetContentHash`/`RuleSetVersion`/`RuleSetActivation`) · **명시적 canonicalizer + `RuleSetHasher` seam**(SHA-256, testable collision) · `RuleSetStore` trait(publish/get_version/list_versions/activate/active) + `PublishRuleSetRequest`/`PublishOutcome` + `RuleSetStoreError` · `InMemoryRuleSetStore<H>`(Mutex 원자적 publish) · automation-core `validate` 3분할(structural/bindings/합성) · 20 테스트 + no_ai_gateway 가드.

**제외(→후속):** PostgreSQL 구현(18b) · runtime DB hydration + active 조회(18c) · `AutomationInstance.ruleset_version` pin + instance migration(18c) · **`RuleSetRouteId`(짧은 custom_id routing token, §7)**(18c/18d) · Draft persistence(별도 개념, 후속) · `Retired` status · created_at 코어 노출(18b DB metadata) · idempotent installation/attach(18d) · Run Ledger(19) · reconciliation(20).

---

## 1. 크레이트 구조 + 의존

```
automation-ruleset → automation-state (InteractionRuleSet)
                   → automation-core   (validate_structural, ValidationError)   [역방향 금지 — dependency guard]
                   → discord-model     (GuildId, UserId)
                   → serde/serde_json, sha2
```
**dependency guard**: `tests/`에서 `automation-core → automation-ruleset` 부재 확인(순환 금지). automation-core는 18a 이후에도 automation-ruleset을 몰라야 함(hydration 조립은 18c 상위 layer).
```toml
# crates/automation-ruleset/Cargo.toml
[dependencies]
automation-state = { path = "../automation-state" }
automation-core = { path = "../automation-core" }
discord-model = { path = "../discord-model" }
serde = { workspace = true }
serde_json = { workspace = true }
sha2 = "0.10"
[dev-dependencies]
futures = "0.3"   # block_on (17a 패턴)
```
`sha2`는 순수 해시(외부 IO/async 아님) — 순수 코어에 적합. + workspace members 추가. `tests/no_ai_gateway.rs`(자기 Cargo.toml에 ai-gateway 문자열 0 검사, 16a 패턴). 18b = 별도 `automation-ruleset-postgres → automation-ruleset`.

---

## 2. 타입

```rust
pub struct RuleSetKey(String);
// parse: 1~64자, [A-Za-z0-9_-]. custom Deserialize가 검증(InstanceId 패턴). Display/FromStr/AsRef/Ord.

pub struct RuleSetVersionId(NonZeroU32);
// 1부터. v0 없음. custom Deserialize가 >=1 검증. Display/Ord. next()는 checked → overflow는 §4 VersionOverflow.

pub struct RuleSetSchemaVersion(NonZeroU32);
// InteractionRuleSet 타입 자체의 스키마/포맷 버전(마이그레이션 호환용) — 정의 안의 InteractionRuleSet.version(도메인 필드)과 다름.
// 0 거부. 18a는 현재 typed 정의만 publish 가능 → 시스템이 CURRENT_RULESET_SCHEMA_VERSION(=1) 스탬프(호출자 raw u32 미검증 회피).
// 구 스키마 호환 판단은 18c hydration.
pub const CURRENT_RULESET_SCHEMA_VERSION: RuleSetSchemaVersion; // = 1

pub struct RuleSetContentHash([u8; 32]);
// Serialize=lowercase hex 64자, Deserialize=정확히 64자 lowercase hex만 허용([u8;32]로 파싱, 아니면 거부).
// 18b에서 DB TEXT로 왕복하므로 잘못된 저장값 거부 가능해야 함. Display=hex.

pub struct RuleSetVersion {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub version: RuleSetVersionId,
    pub schema_version: RuleSetSchemaVersion,
    pub definition: InteractionRuleSet,
    pub content_hash: RuleSetContentHash,
    pub created_by: UserId,
}   // status 없음, created_at 없음(§ 최상위 원칙)

pub struct RuleSetActivation {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub active_version: RuleSetVersionId,
}
```
`RuleSetVersion`/`RuleSetActivation`은 `Clone, Debug, PartialEq, Eq, Serialize, Deserialize`(deny_unknown_fields). `RuleSetKey`·`RuleSetContentHash`·`RuleSetVersionId`는 검증 custom serde라 derive Deserialize 대신 직접.

---

## 3. Content hash — 명시적 canonicalizer + hasher seam

**hash는 raw JSON이 아니라 validated·canonical typed value에서.** 현재 `InteractionRuleSet`이 Vec+BTreeMap만 써서 typed 재직렬화가 결정적이지만, **미래 필드에 HashMap이 들어와도 안전하도록 명시적 canonicalizer를 둔다**(현재 타입 감사에만 의존하지 않음 — content hash는 장기 식별 정보).

```rust
#[derive(Serialize)]
struct RuleSetHashInput<'a> {
    schema_version: RuleSetSchemaVersion,
    definition: &'a InteractionRuleSet,
}

pub fn content_hash(
    schema_version: RuleSetSchemaVersion,
    definition: &InteractionRuleSet,
) -> Result<RuleSetContentHash, RuleSetHashError>
// 1. value = serde_json::to_value(RuleSetHashInput{..})?   (typed → Value)
// 2. canonical = canonicalize(value)                        (object key 재귀 정렬, array 순서 보존)
// 3. bytes = serde_json::to_vec(&canonical)?               (결정적 bytes)
// 4. Sha256(bytes) → RuleSetContentHash([u8;32])
```
- `canonicalize(Value)`: **Object → key 정렬 후 각 value 재귀 / Array → 순서 그대로, 각 원소 재귀 / scalar → 그대로.** **배열은 절대 정렬 안 함** — `GrantRole→Respond ≠ Respond→GrantRole`(다른 hash). object key/map insertion 순서만 정규화.
- **panic 경로 금지**: `to_value`/`to_vec` 오류는 `RuleSetHashError::Serialization(String)` → publish에서 `RuleSetStoreError::Canonicalization`으로 매핑(`.unwrap()` 안 씀).
- **hash 입력 = schema_version + canonical definition만.** 제외: guild_id/ruleset_key/version/created_by/created_at/status.

**testable collision seam:**
```rust
pub trait RuleSetHasher {
    fn hash(&self, schema_version: RuleSetSchemaVersion, definition: &InteractionRuleSet)
        -> Result<RuleSetContentHash, RuleSetHashError>;
}
pub struct Sha256RuleSetHasher;   // content_hash(..) 위임 — 프로덕션 기본
```
`InMemoryRuleSetStore<H: RuleSetHasher>`가 hasher를 주입받음(generic static dispatch, 코드베이스 패턴). 테스트는 **다른 definition에 같은 hash를 내는 fixed hasher**를 주입 → collision fail-closed 경로(§4)를 실제로 테스트(SHA-256 실충돌 불필요).

---

## 4. RuleSetStore trait + publish 계약

```rust
pub struct PublishRuleSetRequest {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub definition: InteractionRuleSet,
    pub created_by: UserId,
}   // schema_version/version/content_hash/status/created_at 없음 — 시스템이 결정
    // (publish가 CURRENT_RULESET_SCHEMA_VERSION 스탬프)

pub enum PublishOutcome {
    Created(RuleSetVersion),   // 새 version 생성
    Reused(RuleSetVersion),    // 동일 content 기존 version 재사용
}

pub enum RuleSetStoreError {
    InvalidDefinition(Vec<ValidationError>),  // 구조 검증 실패 (automation_core::ValidationError)
    VersionNotFound,                          // 없는 version activate
    VersionOverflow,                          // u32::MAX 다음 (panic/wrap 금지)
    HashCollision,                            // 동일 hash + 다른 저장 definition (fail closed)
    Canonicalization(String),                 // hash 직렬화 실패
    Backend(String),                          // 18b 예약(코어 미사용)
}

#[allow(async_fn_in_trait)]
pub trait RuleSetStore {
    async fn publish(&self, request: PublishRuleSetRequest)
        -> Result<PublishOutcome, RuleSetStoreError>;
    async fn get_version(&self, guild_id: GuildId, key: &RuleSetKey, version: RuleSetVersionId)
        -> Result<Option<RuleSetVersion>, RuleSetStoreError>;
    async fn list_versions(&self, guild_id: GuildId, key: &RuleSetKey)
        -> Result<Vec<RuleSetVersion>, RuleSetStoreError>;   // version 오름차순
    async fn activate(&self, guild_id: GuildId, key: &RuleSetKey, version: RuleSetVersionId)
        -> Result<RuleSetActivation, RuleSetStoreError>;      // 존재 version만, 결과 반환
    async fn active(&self, guild_id: GuildId, key: &RuleSetKey)
        -> Result<Option<RuleSetVersion>, RuleSetStoreError>; // 활성 없으면 None
}
```

**publish 순서(불변식):**
```
1. validate_structural(&definition)   → 실패 시 InvalidDefinition (hash·version 소비 전)
2. content_hash(CURRENT_RULESET_SCHEMA_VERSION, &definition)  → 실패 시 Canonicalization
3. (guild,key,hash) 조회:
     있음 + 저장 definition == 요청 definition  → Reused(기존)   (version 미소비)
     있음 + 저장 definition != 요청 definition  → HashCollision  (fail closed)
     없음 → max(version)+1 할당(overflow 검사) → 저장 → Created(새)
4. publish는 activation을 절대 변경 안 함
```
structural validation 실패 시: **hash 계산 없음 · version 소비 없음 · store 변경 없음 · activation 변경 없음.**

- **검증 3분할(automation-core, 합성)** — 로직 중복 금지, `validate`가 두 레이어를 합성:
  ```rust
  pub fn validate_structural(ruleset: &InteractionRuleSet) -> Result<(), Vec<ValidationError>>;
  pub fn validate_bindings(ruleset: &InteractionRuleSet, bindings: &ResourceBindingMap) -> Result<(), Vec<ValidationError>>;
  pub fn validate(ruleset: &InteractionRuleSet, bindings: &ResourceBindingMap) -> Result<(), Vec<ValidationError>> {
      validate_structural(ruleset)?;
      validate_bindings(ruleset, bindings)
  }
  ```
  - **structural(publish 포함)**: rule/panel/modal/button/output key 중복 · trigger가 로컬 button/modal key 참조 · action output key와 created ref 타입·순서 · Defer/EditResponse 계약 · template 문법·event context 호환 · instance resource alias 형식 · allow/deny 구조 모순 · 동일 trigger 중복 · action sequence 유효성. **Existing ref도 문법·key 형식은 structural로 검사**(실제 binding 존재 여부만 연기).
  - **bindings(18c activation/hydration)**: Existing role key → RoleId 해소 · Existing channel key → ChannelId 해소 · 필요한 guild binding 존재 · 역할 permission 기반 privileged policy · Discord 실제 리소스 존재 · 봇 계층·실행 capability.
- **activate(18a 의미 — 저수준 pointer만)**: version 존재 확인 → active-version pointer 갱신 → `RuleSetActivation` 반환. 같은 version 재활성화 = idempotent 성공. 이전 version 재활성화 = rollback 경로. 없는 version → `VersionNotFound`.
  > **Phase 18a activation only records an active-version pointer. Installability and binding validity are enforced atomically by the Phase 18c activation service, and rechecked during hydration.**
  즉 18a `activate`는 "안전한 활성화 명령"이 아니라 pointer 저장 op. 18c의 `RuleSetActivationService`가 target 조회→structural 확인→guild bindings full validation→policy/capability 확인 후 **모두 성공 시에만** `store.activate` 호출. startup hydration도 full 재검증 후 실패한 active RuleSet은 **fail closed**(운영 중 Discord 리소스 소실 대비).
- **active**: activation 없으면 `None`, 있으면 그 immutable `RuleSetVersion` 반환.

---

## 5. InMemory 원자성

`InMemoryRuleSetStore<H: RuleSetHasher>` — `Mutex<Inner>`. **publish의 다음 전체가 하나의 critical section**(중간 unlock 시 동시 publish가 같은 version 생성):
```
① 같은 (guild,key) 동일 hash 검색  ② 저장 definition 비교  ③ 일치면 Reused
④ 아니면 max version 계산  ⑤ overflow 검사  ⑥ 새 version 삽입  ⑦ Created
```
version 저장은 `(guild,key)`별 `BTreeMap<RuleSetVersionId, RuleSetVersion>`(list 오름차순 결정성). activation은 `(guild,key) → RuleSetVersionId` map. **get/list/active는 clone 반환**(store 불변 — 반환 artifact 수정이 내부에 영향 없음). `Default`는 `Sha256RuleSetHasher`; 테스트는 `new(fixed_hasher)`.

---

## 6. 불변식 (요약)
```
- Draft는 version이 아니다. version은 publish 순간의 immutable artifact.
- validate 먼저 → 실패면 hash·version 소비 없이 거부.
- content hash = schema_version + canonical definition(배열 순서 보존, object key 정렬).
- 동일 hash + 다른 definition = HashCollision(silent dedup 금지).
- version은 (guild,key) 범위 monotonic. gapless 아님(v1,v2,v4 허용).
- publish ≠ activate. publish는 activation 불변.
- RuleSetKey는 도메인 키. custom_id에 직접 삽입 보장 안 함(§7).
- 반환 artifact는 store 내부 사본 — 외부 수정 무영향.
```

---

## 7. custom_id 경계 (반드시 명시)
> `RuleSetKey`(1~64)는 **영속·사람 친화 도메인 키**이며, Discord custom_id에 **항상 직접 삽입 가능하다고 보장하지 않는다.** 현재 정적 버튼 custom_id `starring:<guild>:<ruleset>:button:<key>`에 64자 key를 넣으면 100자 초과 가능. **Runtime routing용 짧은 식별자(`RuleSetRouteId`, 예 `r_a82k4`)는 activation/installation(18c/18d)에서 별도 도입**한다. 18a는 route token 미생성 — 다만 64자 key를 기존 custom_id에 그대로 넣는 구조를 **장기 계약으로 굳히지 않는다.**

---

## 8. 테스트 (20 — 사용자 확정 세트)
```
1.  RuleSetKey valid/invalid + invalid JSON deserialize 거부
2.  RuleSetVersionId 0 거부 + RuleSetSchemaVersion 0 거부(deserialize + 생성 경로)
3.  첫 publish → Created(v1)
4.  동일 definition 재publish → Reused(v1)
5.  Reused가 version 미소비(list_versions 여전히 [v1])
6.  변경 definition → Created(v2)
7.  action 순서 변경 → 다른 hash/version
8.  object field/map insertion order 차이 → 같은 hash(같은 version)
9.  schema_version 차이 → 다른 hash (publish는 CURRENT 고정이므로 `content_hash()` pure 함수로 직접 테스트)
10. guild별 version 격리(A/key=v1, B/key=v1)
11. ruleset key별 version 격리(guild/keyX=v1, guild/keyY=v1)
12. publish는 activation 불변(publish 후 active()=이전 그대로/None)
13. 없는 version activate → VersionNotFound
14. activate 후 active() = 해당 artifact
15. 이전 version 재activate(rollback) 작동
16. concurrent same-content publish → 정확히 하나 Created, 나머지 Reused(번호-content 매핑은 미단언)
17. concurrent distinct publish → version ID 전부 유일
18. forced hash collision(fixed hasher) → HashCollision
19. 반환 artifact clone 변경이 store 내부 불변
20. no_ai_gateway 의존 가드
```
+ 보조: VersionOverflow(경계 근처 주입 가능하면), invalid definition publish → InvalidDefinition + version 미소비. 동시성 테스트(16/17)는 **어떤 content가 v1/v2인지 단언 금지**(스케줄 의존 → flaky 회피).

---

## 9. 로드맵 (Durable RuleSet Lifecycle 아크)
```
18a▶ RuleSet Registry Core (이 스펙)
18b  PostgreSQL RuleSet Store (automation-ruleset-postgres; versions/activations 테이블,
     PK (guild,key,version), UNIQUE (guild,key,content_hash), 동시 version은 lock/atomic counter,
     published 수정 금지, created_at DEFAULT now())
18c  RuleSetActivationService(structural+bindings+policy/capability 게이트 후에만 store.activate) + Runtime DB hydration(full 재검증, 실패 active는 fail closed) + RuleSetRouteId + AutomationInstance.ruleset_version pin + instance migration
18d  Idempotent installation(ruleset_installations, 재게시 방지) + attach-after-register(원자적 ResourcePatch)
18e  Durable RuleSet live (fixture 없이 DB에서 v1 로드→설치→방 생성→재시작 idempotent→v2 publish/activate→새 방 v2/기존 방 pinned v1→v1 rollback)
후속: 19 Run Ledger+Idempotency · 20 Reconciliation · 21 Backend API · 22 Web UI · 23 AI Rule Authoring
```

---

## 10. Codex 핸드오프 (개요)
1. automation-core: `validate_structural`/`validate_bindings` 분리 + `validate` = 둘 합성(로직 중복 없이, 기존 테스트 보존). 3개 export. (Existing ref: 문법·형식은 structural, binding 해소만 bindings.)
2. 새 crate `automation-ruleset`: Cargo.toml(sha2) + workspace member. 타입(§2) · content_hash+canonicalize+RuleSetHasher/Sha256RuleSetHasher(§3) · RuleSetStore trait+Publish types+errors(§4) · InMemoryRuleSetStore<H>(§5) · lib exports.
3. tests: 20개(§8) + `tests/no_ai_gateway.rs`. 주석 없음. 게이트 build/test/clippy(-D warnings)/fmt. push.
4. **Claude 독립검증**(게이트 재현 + diff 스코프: state/instance/postgres/runtime 무수정, core는 validate 3분할만, `automation-core → automation-ruleset` 의존 부재). live 없음(순수 코어).

## 최종 정리
18a = RuleSet Registry Core. 새 순수 crate `automation-ruleset`(→ automation-core 의존, 역방향 금지) + core `validate` 3분할(structural/bindings/합성) 최소 추가. RuleSetVersion = validated InteractionRuleSet의 immutable published artifact, `(guild,key)` 범위 Store-owned monotonic version, system-stamped schema version + canonical SHA-256 content hash로 idempotent publish(Created/Reused), **publish ≠ activate**(18a activate는 저수준 pointer, full activation gate는 18c 서비스). NonZeroU32 version·NonZeroU32 schema version·검증된 hash 타입·명시적 canonicalizer(배열 순서 보존)·testable collision seam·binding-독립 구조 검증까지 못박아, 이후 18b(Postgres)/18c(hydration+pin)/18d(idempotent install)/18e(durable live) 전체 아크의 정합 기반을 만든다. RuleSetKey는 도메인 키이지 custom_id route token이 아니며, 짧은 routing 식별자는 후속. **완료 = 20 테스트 green + 스코프 규율 + Claude 독립검증**(순수 코어라 live 없음).
