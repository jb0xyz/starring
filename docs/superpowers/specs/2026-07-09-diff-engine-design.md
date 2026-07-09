# Diff Engine 설계 스펙 (Phase 4)

- **작성일**: 2026-07-09
- **상태**: 확정 (구현 대기 — Codex 핸드오프 대상)
- **범위**: Phase 4 — `crates/diff-engine` (현재 vs 목표 비교 → 변경 계산)
- **선행**: Phase 1(discord-model), Phase 2(desired-state), Phase 3(desired-compiler) 완료.

---

## 0. 목적

현재 `GuildState`와 목표 `NormalizedDesiredState`를 비교해 **변경(create/update/delete/no-op/conflict)**을 계산한다. 아키텍처 핵심(선언형 재조정)의 첫 실체화. 순수 Rust, 외부 상태(DB/Discord) 없음.

```
DiffEngine::diff(&NormalizedDesiredState, &impl ResourceResolver) -> DiffResult
```

---

## 1. 확정된 설계 결정

| # | 결정 | 내용 |
|---|---|---|
| D1 | **Resolver 트레이트 seam** | Diff는 "비교"만. key→현재리소스 해소는 `ResourceResolver`가 담당. 첫 구현체 `InMemoryMatchResolver`(GuildState 기반). 미래 `BindingAwareResolver` 주입 |
| D2 | **ResolveResult** | `Existing(T)` / `Missing` / `Conflict{reason}` (Phase 4). `Forbidden`·`StaleBinding`은 후속 |
| D3 | **삭제 = patch + 명시적 absent** | `state:absent` → delete candidate. scoped/full **암묵 pruning은 후속**(Authoritative Pruning phase). mode/scope는 보존만 |
| D4 | **패널 diff 연기** | roles/channels/permission overwrites만 diff. VerificationPanel은 현재상태 추적(registry) 없어 `deferred`로 기록 |
| D5 | **delete는 candidate** | 실제 삭제 아님. Policy→Approval→OpGraph→Runtime은 후속 |
| D6 | **출력 = changes/conflicts/deferred** | 공통 `DiffChange`(op+target+changed) |

---

## 2. 스코프 경계

| Phase 4 (`diff-engine`) 담당 | 담당 아님 (후속) |
|---|---|
| Role/Channel/Overwrite create·update·delete·no-op·conflict | VerificationPanel diff → **Feature Registry phase** |
| `state:absent` 명시 삭제 candidate | scoped/full 암묵 pruning → **Authoritative Pruning** |
| in-memory match 해소(Resolver) | binding registry(DB) → **BindingAwareResolver** |
| mode/scope 보존 | 실제 삭제 실행·risk 판단 → Policy/OpGraph/Runtime |

---

## 3. Crate 구조 & 의존

```
diff-engine → desired-compiler (NormalizedDesiredState 등)
diff-engine → desired-state    (Identity, MatchStrategy, Ownership, ResourceState, ResourceKey)
diff-engine → discord-model    (GuildState, Role, Channel, PermissionOverwrite, RoleId, ...)
```
**선행 수정**: `desired-state`의 `MatchStrategy`에서 `#[non_exhaustive]` 제거 (Resolver가 exhaustive match로 전략 누락을 컴파일타임에 잡게 — Capability 선례와 동일).

---

## 4. Resolver 트레이트 (D1/D2)

```
pub enum ResolveResult<T> {
    Existing(T),                 // 현재 리소스(clone)
    Missing,
    Conflict { reason: String },
}

pub trait ResourceResolver {
    fn resolve_role(&self, identity: &Identity, name: Option<&str>) -> ResolveResult<Role>;
    fn resolve_channel(&self, identity: &Identity, name: Option<&str>) -> ResolveResult<Channel>;
    // permission overwrite는 channel 매칭 후 그 channel의 overwrites 안에서 target 대조(아래 6.3)
    // resolve_verification_panel은 Phase 4 미구현(패널 연기)
}

pub struct InMemoryMatchResolver<'a> { guild: &'a GuildState }
impl<'a> InMemoryMatchResolver<'a> { pub fn new(guild: &'a GuildState) -> Self }
```

**InMemoryMatchResolver 매칭 규칙** (role/channel 공통):
- `MatchStrategy::ByName`: name으로 현재 검색 → 0=Missing / 1=Existing / 2+=Conflict. name 없으면 Conflict("ByName requires name").
- `MatchStrategy::ByExplicitId(id)`: id 파싱 후 현재 검색 → 있으면 Existing / 없으면 Missing (Diff가 아래 규칙으로 해석).

---

## 5. DiffResult / DiffChange 타입 (D6)

```
pub struct DiffResult {
    pub changes: Vec<DiffChange>,        // Create/Update/Delete/NoOp (desired 리소스별)
    pub conflicts: Vec<DiffConflict>,
    pub deferred: Vec<DeferredItem>,     // panel 등 미지원
}

pub struct DiffChange {
    pub op: ChangeOp,
    pub target: DiffTarget,
    pub changed: Vec<ChangedField>,      // Update일 때만 채움
}
pub enum ChangeOp { Create, Update, Delete, NoOp }
pub enum DiffTarget {
    Role { key: ResourceKey },
    Channel { key: ResourceKey },
    Overwrite { channel: ResourceKey, target: NormalizedTarget },
}
pub enum ChangedField { Name, Permissions, ChannelType, Parent, Allow, Deny }

pub struct DiffConflict { pub target: DiffTarget, pub reason: String }
pub struct DeferredItem { pub kind: String, pub key: ResourceKey, pub reason: String }
```

---

## 6. Diff 로직

### 6.1 공통 present/absent 규칙 (role·channel)
`identity.state`(present 기본 / absent) + ResolveResult + ownership:

| state | resolve | ownership | 결과 |
|---|---|---|---|
| present | Missing (ByName) | managed/adopted | **Create** |
| present | Missing (ByExplicitId) | any | **Conflict**("explicit id not found") |
| present | Missing | referenced | **Conflict**("referenced not found") |
| present | Existing | referenced | **NoOp** |
| present | Existing | managed/adopted | 필드 비교 → **Update**(변경) / **NoOp**(동일) |
| present | Conflict | any | **Conflict** |
| absent | Existing | managed/adopted | **Delete** (referenced면 Conflict) |
| absent | Missing | any | **NoOp** ("already absent") |
| absent | Conflict | any | **Conflict** |

### 6.2 Role 필드 비교 (Update 판정)
`NormalizedRole`(name?/permissions?)의 **Some 필드만** 현재 Role과 비교:
- `name` Some & 다름 → `ChangedField::Name`
- `permissions` Some & 다름 → `ChangedField::Permissions`
- changed 비어 있으면 NoOp, 있으면 Update.

### 6.3 Channel + Overwrite 비교
채널 매칭(Existing) 후:
- 채널 메타: name/channel_type/parent Some 필드 비교 → changed에 반영.
- **overwrites (가산 diff)**: desired 채널의 각 `NormalizedOverwrite`에 대해:
  - **target 해소**: `Everyone` → 현재 @everyone 역할 overwrite(target `Role(RoleId(guild.id))`) / `Role(key)` → key를 role로 resolve → 그 RoleId의 overwrite / `Member(id)` → 그 UserId의 overwrite.
  - 현재 채널에 해당 target overwrite 없음 → **Create** `Overwrite{channel, target}`.
  - 있고 allow/deny 다름 → **Update**(changed: Allow/Deny).
  - 같음 → **NoOp**.
  - target의 role key가 resolve 안 되면 → **Conflict**.
  - **주의**: 현재에 있는데 desired에 없는 overwrite는 patch라서 **건드리지 않음**(overwrite 삭제는 후속/authoritative). 즉 overwrite는 create/update/no-op만.
- 채널 자체 create/delete/no-op은 6.1 규칙.

### 6.4 VerificationPanel / mode
- `verification_panels` 각 항목 → `deferred`에 `{kind:"verification_panel", key, reason:"panel state not tracked in Phase 4"}`.
- `mode`/`scope`: DiffResult 계산에 영향 없음(patch처럼 처리). 문서에 명시: **Phase 4는 scoped/full 암묵 pruning 미수행**.

---

## 7. Phase 4 범위 경계
- ✅ 완전 구현: `ResourceResolver`+`InMemoryMatchResolver`, `DiffResult`/`DiffChange`, Role/Channel/Overwrite diff(present/absent/ownership/target 해소), MatchStrategy non_exhaustive 제거
- ⚠️ deferred 기록만: VerificationPanel
- ❌ 제외: scoped/full 암묵 pruning, overwrite 삭제, binding registry, 실제 삭제 실행, Policy/OpGraph/Simulator, Moderation/Logging

---

## 8. 컨벤션 (승계)
serde·주석 없음·DB 무관·파생 표준. `ResolveResult<T>`는 `T: Clone`. 결정적 출력(정렬).

---

## 9. 테스트 전략 (⭐ 멱등성)
- Resolver: ByName 0/1/2건, ByExplicitId 있음/없음.
- present/absent × missing/existing/conflict 케이스별 DiffChange.
- Role 필드 변경 감지, Channel overwrite create/update/no-op, @everyone target 해소.
- **⭐ 인증 시나리오 2종 (핵심)**:
  - `before` GuildState(인증됨 역할 없음, 일반=everyone 공개) vs Phase 3 산출 NormalizedDesiredState → **Create role + Update overwrites** 나옴.
  - `after` GuildState(목표 상태 그대로) vs 같은 NormalizedDesiredState → **모두 NoOp** (멱등성 증명 — 이 프로젝트 철학의 첫 end-to-end 검증).

---

## 10. Codex 핸드오프 유의사항
1. `MatchStrategy` non_exhaustive 제거는 desired-state 수정(+ 기존 테스트 영향 확인).
2. `ResolveResult`는 owned clone 반환(lifetime 회피). `InMemoryMatchResolver`는 `&GuildState` 보유.
3. @everyone 해소: 현재 @everyone 역할 id == guild.id. desired `NormalizedTarget::Everyone` ↔ 현재 overwrite `OverwriteTarget::Role(RoleId(guild.id))`.
4. overwrite는 가산(create/update/no-op)만 — 삭제 없음.
5. 완료 게이트: build/test/clippy(-D warnings)/fmt. 워크스페이스 members에 `crates/diff-engine` 추가.
