# Phase 18c-2 — AutomationInstance RuleSet Version Pin 설계 스펙

- **작성일**: 2026-07-12
- **상태**: 설계 확정 (구현 대기 — Codex 코드, Claude 실제 Discord+Postgres live). 사용자 브레인스토밍 승인(non-Option + InstanceRuleSetVersion + guarded backfill + RunningRuleSetIdentity + 12 테스트).
- **범위**: Phase 18c-2 — 새 `AutomationInstance`가 **자신을 생성한 RuntimeRuleSet의 version을 변경 불가능한 시스템 metadata로 기록·영속**. 관리자가 v2를 activate해도 기존 instance는 생성 당시 version을 기억(dispatch 연결은 후속).
- **선행**: 17a(AutomationInstance/InstanceStore) · 17d(PostgresInstanceStore/migration) · 18c-1(hydration→RuntimeRuleSet.version) · 18a(RuleSetVersionId).

---

## ⚠️ 최상위 원칙 (불변)
AI 설치시점 설계자, Runtime 결정론. **crate 수정 범위(cross-cutting)**: automation-**instance**(모델+타입) · automation-**instance-postgres**(migration/row/register) · automation-**core**(RunningRuleSetIdentity/RuntimeContext/RegisterInstance/handle_event) · automation-**runtime**(gateway::run/handle_event 배선) · **tool** + handle_event 호출 테스트들. automation-**state/ruleset/ruleset-postgres/ruleset-readiness/runtime(convert 등) 무수정**. **순환 금지**: `automation-instance → automation-ruleset` 절대 금지(automation-ruleset → automation-state → automation-instance라서 순환) — 그래서 version 타입은 automation-instance 로컬. **핵심 계약(고정)**:

> 새 AutomationInstance는 자신을 생성한 RuntimeRuleSet의 version을 변경 불가능한 시스템 metadata로 기록하고 재시작 후에도 PostgreSQL에 영속한다. version은 ruleset_key처럼 시스템 주입값이며 rule author/AI가 지정하지 않는다.

---

## 0. 범위

**포함:** `InstanceRuleSetVersion(NonZeroU32)` 타입(automation-instance) · `AutomationInstance.ruleset_version` 필드(non-Option) · `RunningRuleSetIdentity{key,version}`(automation-core) · guarded backfill migration · postgres row/register/TryFrom 반영 · RuntimeContext.ruleset_version + from_event + RegisterInstance 저장 · gateway::run/handle_event 배선 · tool이 RuntimeRuleSet→RunningRuleSetIdentity 변환 · 12 테스트 + Claude live.

**제외(→후속):** **InstanceAction 이벤트가 pinned version을 조회해 그 버전 RuleSet으로 dispatch**(소비 경로, 다음 슬라이스) · `automation_instances → automation_ruleset_versions` **FK**(legacy backfill이 실제 artifact 존재를 보장 못 하므로 legacy 정리 후 별도 migration) · created_at/updated_at.

---

## 1. 타입

```rust
// automation-instance (automation-ruleset 의존 없음 — 순환 방지)
pub struct InstanceRuleSetVersion(NonZeroU32);
// new(u32)->Result(0 거부, VersionError), get()->u32, Display; custom serde(u32, ≥1 검증)

// AutomationInstance에 필드 추가 (non-Option, serde(default) 금지)
pub struct AutomationInstance {
    pub id: InstanceId,
    pub guild_id: GuildId,
    pub ruleset_key: String,
    pub ruleset_version: InstanceRuleSetVersion,   // 신규
    pub kind: InstanceKind,
    pub created_by: UserId,
    pub resources: InstanceResources,
    pub status: InstanceStatus,
}

// automation-core (automation-core → automation-instance 이미 의존)
pub struct RunningRuleSetIdentity {
    pub key: String,
    pub version: InstanceRuleSetVersion,
}
```
`RunningRuleSetIdentity`로 key/version을 함께 운반 → 서로 다른 RuleSet의 key/version 오조합을 구조적으로 방지. `AutomationInstance`는 `deny_unknown_fields` 유지 → 기존 fixture(serde JSON)는 전부 명시적으로 `ruleset_version` 추가(default 금지).

---

## 2. migration (guarded deleted-only v1 backfill)
`/migrations/202607120001_add_instance_ruleset_version.sql`:
```sql
ALTER TABLE automation_instances ADD COLUMN ruleset_version BIGINT;

UPDATE automation_instances SET ruleset_version = 1 WHERE status = 'deleted';

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM automation_instances WHERE ruleset_version IS NULL) THEN
        RAISE EXCEPTION
            'non-deleted legacy automation instances require an explicit ruleset version';
    END IF;
END
$$;

ALTER TABLE automation_instances ALTER COLUMN ruleset_version SET NOT NULL;

ALTER TABLE automation_instances
    ADD CONSTRAINT automation_instances_ruleset_version_valid
    CHECK (ruleset_version BETWEEN 1 AND 4294967295);
```
- **deleted 테스트 row만 v1 backfill** — active·disabled legacy는 NULL 남아 **migration fail-closed**(근거 없는 v1 부여 금지). 문구는 "non-deleted"(active+disabled 둘 다).
- **영구 DEFAULT 없음** — register가 항상 명시. DEFAULT가 있으면 배선 버그가 조용히 v1로 저장돼 DB에서 감춰짐.
- CHECK 1..u32::MAX. **FK 없음**(§0 제외).
- MIGRATOR는 17d와 동일 root `/migrations`(automation-instance-postgres). build.rs rerun-if-changed 유지.

---

## 3. 값 흐름 (system-injected)
```
RuntimeRuleSet.version(RuleSetVersionId, 18c-1 hydration)
  └ tool: InstanceRuleSetVersion::new(runtime.version.get())  ← RuleSetVersionId→InstanceRuleSetVersion 변환(경계, 순환 회피)
→ RunningRuleSetIdentity{ key: runtime.ruleset_key.as_str().into(), version }
→ gateway::run(token, identity, ruleset, bindings, failure_message, store, generator)
→ handle_event(event, ruleset, bindings, services, failure_message, identity)
→ RuntimeContext::from_event(event, identity) → context{ ruleset_key: identity.key.clone(), ruleset_version: identity.version, … }
→ RegisterInstance: AutomationInstance{ ruleset_key: context.ruleset_key.clone(), ruleset_version: context.ruleset_version, … }
→ store.register → Postgres
```
- `RuntimeContext`에 `ruleset_version: InstanceRuleSetVersion` **non-Option** 추가 — from_event가 identity에서 항상 채움 → RegisterInstance에 `None` 분기 없음. "version 없이 RegisterInstance 금지"가 **타입으로 강제**(param 필수).
- `handle_event`는 `ruleset_key: &str`를 **`identity: &RunningRuleSetIdentity`로 교체**(인자 수 불변 6). from_event도 `ruleset_key`→`identity`. RuntimeContext는 `ruleset_key: String` 유지 + `ruleset_version` 추가(기존 `context.ruleset_key` 사용처 무변).
- `gateway::run`은 `ruleset_key: String`→`identity: RunningRuleSetIdentity`. runner도 반영.
- **`ActionSpec::RegisterInstance` 무수정**(key/kind/resources만). version은 시스템 metadata.
- 변환 `RuleSetVersionId→InstanceRuleSetVersion`은 **tool에서**(automation-ruleset[RuleSetVersionId]와 automation-instance[InstanceRuleSetVersion] 둘 다 있는 유일 지점; RuleSetVersionId가 NonZeroU32라 항상 성공).

---

## 4. DB 변환 불변식
`AutomationInstanceRow`에 `ruleset_version: i64` 추가. TryFrom:
```
i64 < 1        → Backend
i64 > u32::MAX → Backend
범위 내         → InstanceRuleSetVersion::new(value as u32)  (new가 0 재검증)
```
**금지**: `as u32` 무검증 캐스팅 / `unwrap` / 0을 v1로 보정. register INSERT는 `ruleset_version` 컬럼을 **항상 명시**(DB default 의존 금지), bind는 `instance.ruleset_version.get() as i64`. get/list SELECT에 컬럼 추가.

---

## 5. 테스트 (12 — 사용자 확정)
```
1  InstanceRuleSetVersion::new(1) 정상
2  serde "0" deserialize 거부
3  new(u32::MAX) 정상
4  DB row ruleset_version 0/음수/>u32::MAX → Backend (row TryFrom)
5  deleted legacy row → v1 backfill (ignored 실 Postgres)
6  active legacy row → migration 실패 (ignored)
7  disabled legacy row → migration 실패 (ignored)
8  신규 register가 ruleset_version 명시 저장 (InMemory + ignored Postgres)
9  get/list reconnect 후 version 유지 (ignored)
10 RuntimeRuleSet v7 → RegisterInstance → AutomationInstance.ruleset_version == 7 (핵심; automation-core Mock)
11 ActionSpec::RegisterInstance에 ruleset_version 필드 없음(serde 확인)
12 비-instance 이벤트도 RuntimeContext가 version 항상 보유(non-Option 구조)
```
기존 handle_event/from_event 호출(dynamic_join/deferred/instance_registration 등)은 `RunningRuleSetIdentity{ key: "…".into(), version: InstanceRuleSetVersion::new(1).unwrap() }`로 명시 갱신. 기존 AutomationInstance fixture(register 테스트)도 ruleset_version 추가.

---

## 6. 명시적 한계
> **18c-2가 주장하는 것:** 새 instance가 생성 당시 RuleSet version을 시스템 metadata로 저장하고 재시작 후에도 보존한다.
> **아직 주장하지 않는 것:** InstanceAction 이벤트가 pinned version을 조회해 그 버전의 RuleSet으로 dispatch한다.

18c-2는 **저장 단계**. pinned-version dispatch(같은 방의 join이 생성 당시 version 규칙으로 처리)는 후속 hydration/dispatcher 슬라이스에서 연결.

---

## 7. 로드맵
```
18c-1✅ Active Hydration   18c-2▶ Instance Version Pin (이 스펙)
후속: pinned-version dispatch(InstanceAction이 instance.ruleset_version의 RuleSet 로드) + 18c-3 RuleSetActivationService(같은 readiness gate) → 18d RouteId+idempotent install+attach → 18e durable RuleSet live(v2 activate→새 방 v2/기존 방 pinned v1→rollback)
```

---

## 8. Codex 핸드오프 (개요)
1. automation-instance: `InstanceRuleSetVersion`(NonZeroU32, serde/parse ≥1) + `AutomationInstance.ruleset_version`. 기존 fixture/serde 테스트 명시 갱신.
2. automation-core: `RunningRuleSetIdentity` + `RuntimeContext.ruleset_version`(non-Option) + `from_event(event, identity)` + `handle_event`가 identity 받음 + RegisterInstance가 context.ruleset_version 저장. handle_event/from_event 호출 테스트 전부 identity로 갱신.
3. automation-runtime: gateway::run/runner가 `RunningRuleSetIdentity` 전달(무로직 배선).
4. automation-instance-postgres: migration(§2) + AutomationInstanceRow.ruleset_version + TryFrom(Backend) + register/get/list SQL.
5. tool: RuntimeRuleSet.version→InstanceRuleSetVersion 변환 + RunningRuleSetIdentity 구성 → gateway::run. (seed는 무관.)
6. 주석 없음. 게이트 build/test(DB 독립 green)/clippy(-D warnings)/fmt. push. **live/DB 접속 없음**(Codex). ignored Postgres(backfill/migration-fail/reconnect)는 Claude.
7. **Claude live**: seed→run(hydrate v_N)→StudyRoom 생성→psql로 `AutomationInstance.ruleset_version == N` 확인 + 재시작 후 유지.

## 최종 정리
18c-2 = Instance RuleSet Version Pin. `InstanceRuleSetVersion(NonZeroU32)`(automation-instance 로컬, 순환 회피)를 `AutomationInstance`에 non-Option으로 추가, hydration의 `RuntimeRuleSet.version`이 `RunningRuleSetIdentity`(key+version 묶음)로 gateway→handle_event→RuntimeContext→RegisterInstance→Postgres로 시스템 주입. migration은 **deleted-only v1 backfill + non-deleted legacy fail-closed + 영구 DEFAULT 없음 + CHECK**, FK는 후속. DB 변환은 범위검사 후 Backend(as/unwrap/보정 금지). ActionSpec 무수정. **완료 = 12 테스트(v7→instance.ruleset_version==7 핵심) + Claude live(생성 instance가 hydrated version 저장·재시작 보존)**. pinned-version dispatch는 후속 슬라이스.
