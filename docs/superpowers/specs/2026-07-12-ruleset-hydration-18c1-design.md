# Phase 18c-1 — Active RuleSet Hydration + Readiness Gate 설계 스펙

- **작성일**: 2026-07-12
- **상태**: 설계 확정 (구현 대기 — Codex 코드, Claude 실제 Discord+Postgres live). 사용자 브레인스토밍 승인(Ⓐ thin-but-present + 크레이트/seed/live-context 3결정 + 2 capability/policy 보강).
- **범위**: Phase 18c-1 — 봇이 **fixture 대신 PostgreSQL의 active RuleSet을 로드**하되, 신뢰하지 않고 재검증(구조·hash·binding·policy·capability)한 뒤에만 실행 가능한 `RuntimeRuleSet`으로 승격. Durable RuleSet Lifecycle 아크의 "절반 DB-driven"을 닫는 첫 슬라이스.
- **선행**: 18a(RuleSetStore/타입/content_hash/CURRENT) · 18b(PostgresRuleSetStore) · automation-core(validate_structural/validate_bindings/analyze/PolicyFinding).

---

## ⚠️ 최상위 원칙 (불변)
AI 설치시점 설계자, Runtime 결정론, event-time LLM 금지. no_ai_gateway 유지. **crate 수정 범위**: **새 순수 crate `automation-ruleset-readiness` + tool(interaction-smoke)만.** automation-core/ruleset/state/instance/instance-postgres/runtime **무수정**(analyzer가 이미 `EveryoneOverwrite`/`PrivilegedOverwriteAllow`를 분리 방출해 보강 불필요). **핵심 계약(고정)**:

> Runtime은 PostgreSQL의 active pointer를 신뢰하지 않는다. Active artifact를 다시 구조 검증하고, canonical hash를 검증하고, 현재 guild binding·policy·capability를 확인한 뒤에만 실행 가능한 RuleSet으로 승격한다. 실패하면 fail-closed(프로세스 시작 안 함) — fixture fallback은 절대 없다(17e InMemory fallback 금지와 동종 split-brain).

부수 원칙: **Hydration과 Activation(18c-3)은 항상 같은 `RuleSetReadinessGate`를 공유**(activate 때 통과했는데 재시작 hydration에서 다른 기준으로 실패하는 divergence 차단). **Readiness ≠ 완전 실행 가능성** — 역할 계층·채널 effective permission은 후속 runtime failure surface.

---

## 0. 범위

**포함:** 새 crate `automation-ruleset-readiness`(순수 gate: `check_readiness`/`policy_severity`/`required_capabilities`/`hydrate_active_ruleset` + 타입) · tool `seed-studyroom` 서브커맨드(DB publish+저수준 activate) · tool runtime hydration 경로(DB-only, fail-closed) · tool `build_readiness_context`(twilight bot member/roles → GuildCapabilities+role_permissions) · check_readiness 순수 단위 테스트 + Claude live.

**제외(→후속):** 역할 계층 검사(bot이 대상 역할보다 위) · 채널 effective permission(PostPanel VIEW/SEND, overwrite 실제 적용성) · RuleSet 수준 policy verdict 재설계 · `AutomationInstance.ruleset_version` pin(18c-2) · `RuleSetActivationService`(18c-3, 같은 gate 재사용) · RuleSetRouteId/idempotent install/attach(18d) · binding/capability DB 영속.

---

## 1. crate 구조 + 의존
```
automation-ruleset-readiness (순수 gate — twilight·sqlx·network 무관)
├─ automation-ruleset   (RuleSetVersion, RuleSetStore, content_hash, RuleSetKey/VersionId/SchemaVersion, CURRENT)
├─ automation-core      (validate_structural, validate_bindings, analyze, PolicyFinding, ValidationError)
├─ automation-state     (InteractionRuleSet, ActionSpec)
├─ discord-model        (Permissions, GuildId)
├─ resource-resolution  (ResourceBindingMap)
└─ desired-state        (ResourceKey)
```
**금지**: `→ sqlx`, `→ twilight`, `→ ai-gateway`. `hydrate_active_ruleset`는 `RuleSetStore` trait을 호출하지만 구체 DB/네트워크 무관(InMemory store로 결정론 검증). `automation-runtime`(twilight live edge)과 **이름·역할 구분** — readiness는 순수 승격 gate. `tests/no_ai_gateway.rs` 가드.

---

## 2. 타입

```rust
pub struct GuildCapabilities {
    pub base_permissions: Permissions,   // 봇의 guild base 권한 (@everyone OR bot role perms)
}
impl GuildCapabilities {
    pub fn satisfies(&self, required: Permissions) -> bool {
        self.base_permissions.contains(Permissions::ADMINISTRATOR)
            || self.base_permissions.contains(required)
    }   // ADMINISTRATOR면 개별 비트 없어도 만족 (단순 required & !perms는 admin 봇을 오차단)
}

pub struct RuleSetReadinessInput<'a> {
    pub artifact: &'a RuleSetVersion,
    pub bindings: &'a ResourceBindingMap,
    pub guild_capabilities: &'a GuildCapabilities,
    pub role_permissions: &'a BTreeMap<ResourceKey, Permissions>,  // Existing 역할 권한(analyze의 privileged-grant 판정용)
}

pub struct RuntimeRuleSet {
    pub guild_id: GuildId,
    pub ruleset_key: RuleSetKey,
    pub version: RuleSetVersionId,
    pub definition: InteractionRuleSet,   // 검증 통과한 정의만 runtime에 도달
    pub notices: Vec<PolicyFinding>,      // 통과했지만 감사/향후 UI용 (deferred check는 문서화)
}

pub enum PolicySeverity { Notice, Blocking }

pub enum ReadinessError {
    UnsupportedSchema(RuleSetSchemaVersion),
    StructurallyInvalid(Vec<ValidationError>),
    HashComputation(RuleSetHashError),        // 재계산이 Result라 필요
    HashMismatch,
    BindingInvalid(Vec<ValidationError>),
    BlockingPolicy(Vec<PolicyFinding>),
    MissingCapabilities { missing: Permissions },
}

pub enum HydrationError {
    NoActiveRuleSet,
    Store(RuleSetStoreError),
    NotReady(ReadinessError),
}

pub enum ReadinessContextError {
    BoundRoleMissing { key: ResourceKey, role_id: RoleId },   // bindings에 있는 Existing role이 guild snapshot에 없음
    EveryoneRoleMissing,                                      // @everyone(=guild id) 역할 snapshot 부재
}
```

---

## 3. 순수 함수 (주입 trait 없음)

```rust
pub fn required_capabilities(ruleset: &InteractionRuleSet) -> Permissions;
// guild-level hard requirement만 (채널 effective 권한은 deferred):
//   CreateRole/GrantRole → MANAGE_ROLES
//   CreateChannel        → MANAGE_CHANNELS
//   UpsertOverwrite      → MANAGE_ROLES   (allow/deny 비트 실제 적용성은 deferred notice)
//   PostPanel            → (guild 없음 — VIEW_CHANNEL/SEND_MESSAGES는 채널 overwrite 의존 → deferred)
//   RespondEphemeral/OpenModal/Defer/Edit/RegisterInstance → 없음
// 반환은 위 action들의 OR. GrantRole은 대상 role 종류 무관 MANAGE_ROLES.

pub fn policy_severity(finding: &PolicyFinding) -> PolicySeverity;
// **notice whitelist / unknown = Blocking** (fail-safe: 새 variant는 자동 Blocking):
//   Notice: DynamicResourceCreation | CreatedResourceReference | EveryoneOverwrite
//         | RuntimeMessagePost | RuntimeInteractivePanel
//   Blocking(_ =>): PrivilegedRoleGrant, PrivilegedOverwriteAllow, 그 외 전부
// EveryoneOverwrite가 Notice여도 안전 — 위험한 @everyone allow는 analyzer가 PrivilegedOverwriteAllow(Blocking)를 별도 방출.

pub fn check_readiness(input: RuleSetReadinessInput) -> Result<RuntimeRuleSet, ReadinessError>;
```
**check_readiness 순서** (hash가 binding보다 먼저 — 변조 artifact는 환경 검사 전 차단):
```
1. artifact.schema_version == CURRENT_RULESET_SCHEMA_VERSION           아니면 UnsupportedSchema
2. validate_structural(&artifact.definition)                          Err → StructurallyInvalid
3. content_hash(**artifact.schema_version**, &artifact.definition)     Err → HashComputation  (CURRENT 하드코딩 금지 — 향후 복수 schema 대비)
4. 재계산 hash == artifact.content_hash                                아니면 HashMismatch
5. validate_bindings(&artifact.definition, bindings)                  Err → BindingInvalid
6. analyze(&artifact.definition, role_permissions) → findings
7. severity 분류 → blocking = findings.filter(Blocking), notices = filter(Notice)
8. !blocking.is_empty()                                               → BlockingPolicy(blocking)
9. required_capabilities(&definition); !guild_capabilities.satisfies(required) → MissingCapabilities{missing}
10. Ok(RuntimeRuleSet{ guild_id, ruleset_key, version, definition, notices })
```

**hydrator** (store 주입 — InMemory/Postgres 둘 다):
```rust
pub async fn hydrate_active_ruleset(
    store: &impl RuleSetStore,
    guild_id: GuildId,
    key: &RuleSetKey,
    bindings: &ResourceBindingMap,
    guild_capabilities: &GuildCapabilities,
    role_permissions: &BTreeMap<ResourceKey, Permissions>,
) -> Result<RuntimeRuleSet, HydrationError>;
// store.active(guild,key) → None → NoActiveRuleSet
//                        → Err → Store(..)
//                        → Some(artifact) → check_readiness → NotReady(..) 또는 RuntimeRuleSet
```

---

## 4. capability = static permission preflight (한계 명시)
스펙 문구:
> Phase 18c-1 capability check is a static permission preflight. Role hierarchy and channel-effective permission evaluation are deferred and remain runtime failure surfaces.

18c-1 hard-block(guild base 부족): MANAGE_ROLES(CreateRole/GrantRole/UpsertOverwrite), MANAGE_CHANNELS(CreateChannel). **PostPanel의 SEND_MESSAGES는 guild hard-block에서 제외** — 채널 overwrite에 좌우되므로 guild base로 판정하면 유효 RuleSet 오차단. deferred(문서화, 후속 channel-effective gate): PostPanel 대상 채널 VIEW/SEND, UpsertOverwrite allow/deny 실제 적용성, **역할 계층**, 리소스 실재. StudyRoom은 VIEW_CHANNEL overwrite만 써서 admin smoke 봇 통과.

---

## 5. context builder(pure) + tool 배선

**`build_readiness_context`는 readiness crate의 순수 함수**(twilight 타입 안 받음 — tool이 twilight→plain snapshot 추출 후 호출). **fail-closed 완결성 검사가 여기 있어 단위 테스트 가능**:
```rust
pub fn build_readiness_context(
    guild_id: GuildId,
    bindings: &ResourceBindingMap,
    roles_snapshot: &BTreeMap<RoleId, Permissions>,   // guild 전체 role → perms (tool이 twilight에서 추출)
    bot_role_ids: &[RoleId],                          // 봇 멤버의 role id들
) -> Result<(GuildCapabilities, BTreeMap<ResourceKey, Permissions>), ReadinessContextError>
// base = roles_snapshot[@everyone(=guild_id)] (없으면 EveryoneRoleMissing) OR (bot_role_ids의 각 perms)
// role_permissions = bindings.role_bindings의 각 (key → RoleId):
//     roles_snapshot에서 perms 조회, **없으면 BoundRoleMissing{key,role_id}** (권한 없음으로 가정 절대 금지 — privileged grant 놓침)
```
**tool은 twilight 추출만**: Get Guild Roles → `BTreeMap<RoleId, Permissions>`, Get Guild Member(bot) → `bot_role_ids`. 그다음 pure `build_readiness_context` 호출.

**서브커맨드 분리** (runtime과 seed 경로 절대 안 섞임):
- **`seed-studyroom [--force-activate]`**: `studyroom_ruleset()` fixture → `store.publish()`(Created/Reused) → **activation을 조용히 덮어쓰지 않음**:
  ```
  store.active(guild,key):
    None            → store.activate(published_version)
    Some == 발행버전 → idempotent 성공 (활성 그대로)
    Some != 발행버전 → 기본 실패(ActivationConflict) — --force-activate 있을 때만 activate
  ```
  결과 출력 → 종료. **개발 bootstrap 전용, 18c-3 activation service 우회, runtime startup에서 자동 실행 금지.** 재-seed(같은 fixture, active=그 버전): publish Reused + idempotent 성공.
- **`run`(기본)**: fixture 생성/fallback **금지**. **startup 순서 고정 — hydration 통과 전 Discord mutation 0회**:
  ```
  1 PgPool connect  2 MIGRATOR.run  3 Get Guild Roles + Member(bot) snapshot  4 build_readiness_context
  5 hydrate_active_ruleset(PostgresRuleSetStore, …)   실패(NoActiveRuleSet/Store/NotReady) → 프로세스 시작 안 함
  6 [성공 이후에만] 진입 패널 설치(RuntimeRuleSet 기반)  7 gateway::run(RuntimeRuleSet.definition, .ruleset_key.as_str(), …)
  ```
  **hydration 실패 전에는 패널 게시·역할/채널 생성·fixture 실행·gateway 이벤트 처리 전부 금지.** 단일 guild smoke라 hydration 실패=프로세스 종료. `gateway::run` 무수정(InteractionRuleSet 그대로). `studyroom_ruleset()`은 seed helper로만 잔존.

---

## 6. 테스트
**순수 단위(automation-ruleset-readiness, synthetic 입력 — admin 봇으론 missing-capability 못 만듦)**:
```
통과: StudyRoom 정의(EveryoneOverwrite/DynamicResourceCreation/RuntimeInteractivePanel = notice) + admin capabilities → RuntimeRuleSet, notices 보존
차단: 1 UnsupportedSchema  2 StructurallyInvalid(중복 key 등)  3 HashMismatch(artifact.content_hash 변조)
      4 BindingInvalid(Existing ref 미바인딩)  5 BlockingPolicy(PrivilegedRoleGrant: privileged role_permissions로 Existing grant)
      6 BlockingPolicy(PrivilegedOverwriteAllow: allow에 MANAGE_ROLES)  7 MissingCapabilities(MANAGE_ROLES 없는 caps)
      8 MissingCapabilities(MANAGE_CHANNELS 없음)
hydrator: NoActiveRuleSet(active 없음), NotReady 전파, InMemoryRuleSetStore로 publish+activate 후 hydrate 성공
required_capabilities: CreateRole→MANAGE_ROLES 등 매핑, PostPanel은 SEND_MESSAGES 미포함 확인
policy_severity: 5 notice + PrivilegedRoleGrant/PrivilegedOverwriteAllow = Blocking
**build_readiness_context(must-lock #1)**: bound Existing role이 roles_snapshot에 없음 → **BoundRoleMissing**(권한 없음 가정 금지) / @everyone 부재 → EveryoneRoleMissing / ADMINISTRATOR base → 임의 required 만족(satisfies)
no_ai_gateway 가드 + dependency guard(→ sqlx/twilight 부재)
```
**seed 안전(must-lock #3, InMemory store로)**: active=v2에서 seed v1(다른 버전) → **ActivationConflict**(active 불변) / active=none → activate / 같은 버전 재-seed → idempotent / --force-activate만 덮어씀.
**mutation-order(must-lock #2, live-verified + 코드 구조)**: run 경로가 hydration 성공 **이후에만** install_panel/gateway 호출(순서 §5 고정). live에서 active 없이 run → **패널 게시 0·gateway 시작 0·프로세스 종료** 실측.
**Claude live**: seed-studyroom→DB publish+activate→`run`이 fixture 없이 DB에서 hydrate→bot member/roles 조회→context→검증 통과→gateway→StudyRoom create/join 작동→프로세스 재시작 후 다시 DB hydration. + fail-closed 실측(active 없이 run→시작 실패, mutation 0).

---

## 6.5 명시적 한계 (신뢰 위해)
> **18c-1이 주장하는 것:** Runtime RuleSet 자체가 fixture가 아닌 **DB artifact에서 복원**되고, 검증 통과한 것만 실행된다.
> **아직 주장하지 않는 것:** 기존 instance가 **생성 당시 RuleSet version을 계속 사용**한다(version pin은 18c-2). 18c-1에서 재시작하면 runtime은 항상 **현재 active version**을 로드하며, instance는 자기 버전을 기억하지 않는다.

또 Readiness는 모든 실행 성공을 보장하지 않는다 — 18c-1은 **구조·artifact 무결성·명백한 policy 위반·guild-level permission 부족**을 차단하는 정적 preflight이며, 역할 계층·채널 effective permission·리소스 실존은 runtime failure surface로 남는다.

## 7. 로드맵
```
18a✅ Registry Core   18b✅ PostgreSQL Store   18c-1▶ Active Hydration + Readiness Gate (이 스펙)
18c-2 AutomationInstance ruleset_version pin + migration (hydration이 준 running version 기록)
18c-3 RuleSetActivationService (store.get_version(target) → 같은 ReadinessGate → store.activate; gate 실패면 active pointer 불변)
18d idempotent installation + RouteId + attach-after-register   ·   18e durable RuleSet live(v1 activate→로드→방 생성→재시작 idempotent→v2 activate→새 방 v2/기존 방 pinned v1→v1 rollback)
후속: 19 Run Ledger · 20 reconciliation · 21 API · 22 Web UI · 23 AI rule authoring
```

---

## 8. Codex 핸드오프 (개요)
1. 새 crate `automation-ruleset-readiness`: Cargo.toml(위 deps, sqlx/twilight 없음) + workspace member. 타입(§2) · `required_capabilities`/`policy_severity`/`check_readiness`/`hydrate_active_ruleset`(§3) · 순수 단위 테스트(§6) + no_ai_gateway + dependency guard.
2. tool: `seed-studyroom`/`run` 서브커맨드 분리 + `build_readiness_context`(twilight) + run 경로를 DB hydration으로(fixture 생성/fallback 제거, fail-closed). `studyroom_ruleset()`은 seed helper로만. Cargo에 automation-ruleset/automation-ruleset-postgres/automation-ruleset-readiness 추가.
3. 주석 없음. 게이트 build/test/clippy(-D warnings)/fmt. push. **live/DB 접속 없음**(Codex).
4. **Claude가 실제 Discord+Postgres live**(재사용 토큰, starring DB): seed→run이 DB에서 hydrate→StudyRoom 동작→재시작 재로드 + fail-closed 실측.

## 최종 정리
18c-1 = Active RuleSet Hydration + Readiness Gate. 새 순수 crate `automation-ruleset-readiness`가 DB active artifact를 **schema→structural→hash 재계산→binding→policy(notice whitelist/unknown blocking)→guild capability preflight** 순으로 재검증해 통과한 것만 `RuntimeRuleSet`으로 승격, 실패는 fail-closed(fixture fallback 금지). Hydration과 18c-3 Activation이 같은 gate 공유. capability는 static guild preflight(역할 계층·채널 effective 권한·SEND_MESSAGES는 deferred runtime surface). tool은 `seed-studyroom`/`run` 분리 + Discord role snapshot으로 봇 권한 계산(ADMINISTRATOR override). automation-core 무수정(analyzer가 이미 위험 overwrite를 PrivilegedOverwriteAllow로 분리). **완료 = 순수 gate 단위 테스트(전 차단/notice 케이스) + Claude live(fixture 없이 DB에서 RuleSet 로드→StudyRoom 동작→재시작 재로드→active 없으면 시작 실패).** 이걸로 "instance는 DB인데 rule은 fixture"의 비대칭이 닫힘.
