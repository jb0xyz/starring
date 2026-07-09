# Operation Graph 설계 스펙 (Phase 5)

- **작성일**: 2026-07-09
- **상태**: 확정 (구현 대기 — Codex 핸드오프 대상)
- **범위**: Phase 5 — `crates/operation-graph` (DiffResult → 실행 순서 그래프)
- **선행**: Phase 1~4 완료. 아키텍처 문서 §8.

---

## 0. 목적

`DiffResult`(무엇이 달라지나)를 **실행 가능한 순서 그래프**(어떤 순서로)로 컴파일한다. 의존성은 op-type 하드코딩이 아니라 각 노드의 **produces/consumes 심볼 매칭**으로 자동 도출한다. 순수 Rust, **그래프 구조까지만**(실행/rollback/retry/Discord/policy 제외).

```
compile_operations(diff: &DiffResult, desired: &NormalizedDesiredState) -> Result<OperationGraph, OperationGraphError>
```

---

## 1. 확정된 설계 결정

| # | 결정 | 내용 |
|---|---|---|
| D1 | **produces/consumes 심볼 그래프** | 노드가 produces/consumes 선언 → compiler가 `depends_on` 자동 도출 |
| D2 | **입력 = (DiffResult, NormalizedDesiredState)** | 심볼·op은 DiffResult, payload는 Normalized에서 |
| D3 | **그래프 구조까지만** | depends_on 자동, cycle detection, topological sort. 실행/rollback/retry/Discord/policy 제외 |
| D4 | **심볼 해소 규칙** | create→produced / update·delete→existing / Everyone→intrinsic. produced 매칭 시 depends_on |
| D5 | **conflict diff는 실행 불가** | `diff.conflicts` 있으면 compile error |

---

## 2. 스코프 경계

| Phase 5 담당 | 담당 아님 (후속) |
|---|---|
| Operation 노드(kind+payload) 생성 | 실제 병렬 executor → Executor |
| produces/consumes → depends_on 자동 도출 | rollback **실행** → Rollback (순서는 reverse-topo로 도출만) |
| cycle detection, topological_order | retry **실행** → Executor |
| conflict diff 거부 | Discord API payload 생성 → Bot Runtime |
| | policy/risk, preview → P6/P7 |

---

## 3. Crate 구조 & 의존
```
operation-graph → {diff-engine, desired-compiler, desired-state, discord-model}
```
파일(예): `src/{lib.rs, node.rs, symbol.rs, compile.rs, order.rs, error.rs}`.

---

## 4. 타입

### 4.1 심볼 (의존성 관련만)
```
enum ResourceSymbol {
    Role(ResourceKey),
    Channel(ResourceKey),
}
```
- overwrite/panel은 **아무도 consume 안 하므로** 심볼 불필요(Phase 5). Everyone은 intrinsic(심볼 아님, consume 시 무시).

### 4.2 Operation (kind + payload)
payload는 NormalizedDesiredState에서 조회.
```
enum Operation {
    CreateRole { key: ResourceKey, name: Option<String>, permissions: Option<Permissions> },
    UpdateRole { key: ResourceKey, name: Option<String>, permissions: Option<Permissions> },
    DeleteRole { key: ResourceKey },
    CreateChannel { key: ResourceKey, name: Option<String>, channel_type: Option<ChannelType>, parent: Option<ResourceKey> },
    UpdateChannel { key: ResourceKey, name: Option<String>, channel_type: Option<ChannelType> },
    DeleteChannel { key: ResourceKey },
    CreateOverwrite { channel: ResourceKey, target: NormalizedTarget, allow: Permissions, deny: Permissions },
    UpdateOverwrite { channel: ResourceKey, target: NormalizedTarget, allow: Permissions, deny: Permissions },
}
```

### 4.3 노드 & 그래프
```
struct OpId(u32);

struct OperationNode {
    id: OpId,
    operation: Operation,
    produces: Vec<ResourceSymbol>,
    consumes: Vec<ResourceSymbol>,
    depends_on: Vec<OpId>,          // compiler가 계산해 저장
}

struct OperationGraph {
    nodes: Vec<OperationNode>,
}
```
- **rollback_hint / retry_policy는 Phase 5 제외**(rollback 순서는 reverse-topo로 도출, retry는 실행 단계). 필요 시 후속 phase에서 노드에 추가.

### 4.4 에러
```
enum OperationGraphError {
    DiffHasConflicts(usize),        // conflict 개수
    MissingPayload { key: String }, // diff change에 대응하는 desired 없음(방어)
    DependencyCycle,
}
```

---

## 5. produces/consumes 규칙 (D4)

| Operation | produces | consumes |
|---|---|---|
| CreateRole | `Role(key)` | — |
| UpdateRole / DeleteRole | — | — (existing 대상, create 의존 없음) |
| CreateChannel | `Channel(key)` | parent 있으면 `Channel(parent)` |
| UpdateChannel / DeleteChannel | — | — |
| CreateOverwrite / UpdateOverwrite | — | `Channel(channel)` + target이 `Role(key)`면 `Role(key)` (Everyone/Member는 없음) |

**depends_on 도출**: 각 노드의 consume 심볼이 다른 노드의 produce와 일치하면 그 노드에 depends_on 추가. produce에 없는 consume(existing/intrinsic)은 의존 없음.

> **unresolved 단순화**: Phase 5는 current state를 안 봐서 "존재 여부"를 확인 못 한다. produce 안 된 consume은 **existing으로 간주**(무의존). 진짜 dangling은 이미 Phase 2 validate / Phase 4 conflict가 잡음. 실제 unresolved 검출은 후속(current state 필요).

---

## 6. Compile 로직

1. `diff.conflicts` 비어 있지 않으면 → `Err(DiffHasConflicts(n))`.
2. `diff.changes`에서 **NoOp 제외**, 각 change를 순서대로:
   - `op`(Create/Update/Delete) + `target`(Role/Channel/Overwrite)로 `Operation` 구성. payload는 `desired`에서 조회(role: `desired.roles`에서 key로, channel: `desired.channels`에서 key로, overwrite: 해당 channel의 overwrites에서 target으로). 못 찾으면 `Err(MissingPayload)`.
   - produces/consumes(§5)와 `OpId`(순차) 부여.
3. produce 심볼 → OpId 맵 구성. 각 노드 consume을 매칭해 `depends_on` 채움.
4. **cycle detection**(topological sort 시도) → cycle이면 `Err(DependencyCycle)`.
5. `Ok(OperationGraph { nodes })`.

**메서드**:
- `OperationGraph::topological_order(&self) -> Vec<OpId>` (실행 순서. compile에서 cycle 없음 보장).
- `OperationGraph::rollback_order(&self) -> Vec<OpId>` (reverse topological).

---

## 7. Phase 5 범위 경계
- ✅ 완전 구현: 심볼/Operation/노드/그래프 타입, `compile_operations`, produces/consumes → depends_on 자동, cycle detection, topological/rollback order, conflict 거부
- ❌ 제외: rollback_hint/retry_policy 필드, 실제 executor/rollback/retry, Discord payload, policy/preview, overwrite delete(Phase 4에 없음), verification panel op(Phase 4 deferred)

---

## 8. 컨벤션 (승계)
serde·주석 없음·DB 무관·파생 표준. 결정적 출력(노드 순서 = diff.changes 순서). `OpId` u32.

---

## 9. 테스트 전략 (⭐ 의존성 자동 도출)
- produces/consumes 정확성(op별).
- **의존성**: CreateRole(verified) + CreateOverwrite(general, target=verified) → overwrite 노드가 role 노드에 depends_on. role이 existing이면 무의존.
- CreateChannel(parent 있음) → parent create에 depends_on.
- cycle → `DependencyCycle`.
- conflict diff → `DiffHasConflicts`.
- topological_order 유효성(depends_on이 order상 앞).
- **⭐ 인증 시나리오 (핵심)**: DesiredState → compile → diff(vs empty GuildState) → `compile_operations`. CreateRole·CreateChannel·CreateOverwrite 노드 생성, **verified overwrite 노드가 role create에 자동 depends_on**, topo order 유효. (produces/consumes 모델의 payoff 검증)

---

## 10. Codex 핸드오프 유의사항
1. `NormalizedTarget`은 desired-compiler에서 re-export(Operation의 overwrite target). Permissions/ChannelType은 discord-model.
2. depends_on은 사람이 안 쓰고 compiler가 계산해 저장.
3. cycle detection = Kahn's algorithm 또는 DFS. topological_order도 같은 로직 재사용.
4. 완료 게이트: build/test/clippy(-D warnings)/fmt. 워크스페이스 members에 `crates/operation-graph` 추가.
