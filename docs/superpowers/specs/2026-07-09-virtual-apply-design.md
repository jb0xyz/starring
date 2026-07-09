# Virtual Apply Engine 설계 스펙 (Phase 9)

- **작성일**: 2026-07-09
- **상태**: 확정 (구현 대기 — Codex 핸드오프 대상)
- **범위**: Phase 9 — `crates/virtual-apply` (OperationGraph → after GuildState)
- **선행**: Phase 1~8 완료. 아키텍처 문서 §10. Simulator의 A 파트.

---

## 0. 목적

`OperationGraph`를 현재 `GuildState` 복사본에 **가상 적용**해 예측 **after GuildState**를 만든다(dry-run, Discord API 없음). Simulator가 이 after-state로 before/after 가시성 preview를 낸다. **정책 승인된 그래프를 그대로 적용**(재도출 아님).

```
apply(current: &GuildState, graph: &OperationGraph, normalized: &NormalizedDesiredState, resolver: &impl ResourceResolver)
    -> Result<VirtualApplyResult, VirtualApplyError>
```

---

## 1. 확정된 설계 결정

| # | 결정 | 내용 |
|---|---|---|
| D1 | **입력 = 4개** | current GuildState + OperationGraph + NormalizedDesiredState + Resolver. 그래프 적용 + key 해소 |
| D2 | **key 해소** | Create op의 key → synthetic id. 그 외 key → normalized의 identity + Resolver로 current id. Missing/Conflict → error |
| D3 | **synthetic id** | 기존 최대 id + 1부터 카운터(role/channel 공유, 기존과 충돌 회피). simulation 전용 |
| D4 | **정책 그래프 적용** | OperationGraph를 topo 순서로 적용(NormalizedDesiredState는 payload/identity 조회용). 재도출(B) 아님 |
| D5 | **출력 = VirtualApplyResult** | after GuildState + applied op ids + synthetic id 맵 + warnings |

---

## 2. 스코프 경계

| Phase 9 담당 | 담당 아님 (후속) |
|---|---|
| CreateRole/UpdateRole/DeleteRole 가상 적용 | Discord API/Bot Runtime → Executor |
| CreateChannel/UpdateChannel/DeleteChannel | DB binding, 실제 id |
| CreateOverwrite/UpdateOverwrite (synthetic/current target) | Policy 재평가, approval |
| synthetic id, topo 순서 적용 | VerificationPanel, retry, rollback 실행 |
| VirtualApplyResult | before/after 델타·preview 문구(호출자가 Simulator 2회 호출) |

---

## 3. Crate 구조 & 의존
```
virtual-apply → {operation-graph, desired-compiler, desired-state, diff-engine, discord-model}
```
- `diff-engine`: `ResourceResolver`/`InMemoryMatchResolver`/`ResolveResult` 재사용. `NormalizedTarget`은 desired-compiler.
- 파일(예): `src/{lib.rs, apply.rs, result.rs, error.rs}`.

---

## 4. 타입

```
struct VirtualApplyResult {
    after: GuildState,
    applied: Vec<OpId>,
    synthetic_roles: BTreeMap<ResourceKey, RoleId>,
    synthetic_channels: BTreeMap<ResourceKey, ChannelId>,
    warnings: Vec<String>,
}

enum VirtualApplyError {
    UnresolvedKey { key: String },              // 기존 참조 Resolver Missing/Conflict
    MissingIdentity { key: String },            // normalized에 key 없음(방어)
    GraphCycle,                                 // topological_order 실패(방어)
}
```

---

## 5. 적용 로직 (D2/D3/D4 — 심장)

`apply(current, graph, normalized, resolver)`:
1. `after = current.clone()`.
2. **synthetic 카운터**: `base = (모든 role.id + channel.id + guild.id 중 최대) + 1`. `next()` 호출마다 증가(role/channel 공유). 기존과 충돌 없음.
3. `role_ids: BTreeMap<ResourceKey, RoleId>`, `channel_ids: BTreeMap<ResourceKey, ChannelId>` (해소 캐시).
4. `graph.topological_order()`로 순서 얻기(에러 시 GraphCycle). 각 OpId의 노드를 순서대로:
   - **CreateRole{key,name,perms}**: id=synthetic. `role_ids[key]=id`. `after.roles.push(Role{id, name(""기본), permissions(empty 기본)})`.
   - **UpdateRole{key,name,perms}**: `resolve_role(key)` → id → `after.roles`에서 찾아 name/perms 갱신(Some만).
   - **DeleteRole{key}**: resolve → `after.roles`에서 제거.
   - **CreateChannel{key,name,type,parent}**: id=synthetic. `channel_ids[key]=id`. parent 있으면 `resolve_channel(parent)`. `after.channels.push(Channel{id, name, type, parent_id, overwrites:[]})`.
   - **UpdateChannel{key,...}**: resolve → 갱신.
   - **DeleteChannel{key}**: resolve → 제거.
   - **CreateOverwrite{channel,target,allow,deny}**: `resolve_channel(channel)` → 채널 → `overwrites.push(PermissionOverwrite{target: resolve_target(target), allow, deny})`.
   - **UpdateOverwrite{...}**: 채널 찾아 같은 target overwrite 교체(없으면 추가).
   - 적용한 OpId를 `applied`에 기록.
5. `Ok(VirtualApplyResult{after, applied, synthetic_roles, synthetic_channels, warnings})`.

**key 해소** `resolve_role(key)`:
- `role_ids`에 있으면(생성됨) 그 id.
- 없으면 `normalized.roles`에서 key의 identity+name → `resolver.resolve_role(identity, name)`:
  - `Existing(role)` → role.id (캐시).
  - `Missing`/`Conflict` → `Err(UnresolvedKey)`.
- normalized에 key 없으면 `Err(MissingIdentity)`.
(channel도 동일. `resolve_target`: Everyone→`Role(RoleId(guild.id))`, Role(key)→`Role(resolve_role(key))`, Member(id)→`Member(UserId)`.)

---

## 6. Phase 9 범위 경계
- ✅ 완전 구현: apply(4입력), synthetic id, key 해소(synthetic/Resolver), 8 op 가상 적용, VirtualApplyResult/Error, topo 순서
- ❌ 제외: Discord API, DB, policy 재평가, approval, VerificationPanel, retry/rollback 실행, preview 문구

---

## 7. 컨벤션
serde(VirtualApplyResult 직렬화)·주석 없음·결정적(synthetic 카운터·topo 순서). 비트 아님.

---

## 8. 테스트 전략 (⭐ end-to-end 크라운 주얼)
- CreateRole → synthetic id 부여 + `role_ids` 등록.
- CreateOverwrite(role:key) → 앞선 CreateRole의 synthetic id를 target으로 사용(스레딩 검증).
- 기존 채널 참조 → Resolver로 current id 해소.
- Missing 참조 → UnresolvedKey.
- **⭐ 인증 시나리오 (핵심 end-to-end)**: `before`(general 채널 current id 존재, everyone 볼 수 있음, verified 없음) + DesiredState → compile → diff(before) → operation-graph → **virtual-apply** → after GuildState. 검증:
  - after에 synthetic verified 역할 존재.
  - after의 general(현재 id 유지)에 @everyone deny VIEW + verified(synthetic) allow VIEW+SEND overwrite.
  - **그 after에 Simulator 적용**: new can_view general=false, verified(synthetic id) can_view=true, can_send=true.
  - (Phase 4~8 전 파이프라인이 이어져 도는 최종 통합.)

---

## 9. Codex 핸드오프
1. Resolver로 해소하려면 key의 identity가 필요 → `normalized.roles`/`channels`에서 key로 찾아 `resolver.resolve_role(&identity, name)` 호출.
2. synthetic 카운터는 기존 모든 id(+guild.id) 최대+1부터 — 충돌 회피. role/channel 공유 가능(타입 다름).
3. topological_order로 순서 적용(CreateRole가 overwrite보다 먼저 → synthetic id 준비됨).
4. Everyone target = `OverwriteTarget::Role(RoleId(guild.guild.id.0))`.
5. 완료 게이트: build/test/clippy(-D warnings)/fmt. members에 `crates/virtual-apply` 추가.
