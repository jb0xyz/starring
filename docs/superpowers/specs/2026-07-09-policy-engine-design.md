# Policy Engine 설계 스펙 (Phase 7)

- **작성일**: 2026-07-09
- **상태**: 확정 (구현 대기 — Codex 핸드오프 대상)
- **범위**: Phase 7 — `crates/policy-engine` (OperationGraph 안전 관문)
- **선행**: Phase 1~6.5 완료. 아키텍처 문서 §9. **실증 동기**: gemma4:e4b가 확률적으로 admin 부여(`permissions:"87375"`) → Graphed 무방비 통과. Policy가 실행 전 차단해야 함.

> 승인 정책 변경 (2026-07-31): `RequireSecondApproval`과 위험도별 2인
> 승인 규칙은 `2026-07-31-solo-product-approval-design.md`에 의해
> 폐기되었습니다. 승인 필요 작업은 모두 `RequireApproval`이며 한 명의
> 인증된 관리자가 승인합니다.

---

## 0. 목적

`OperationGraph`를 실행 전에 정책 검사해 **verdict**(allow/warn/require_approval/require_second_approval/deny)를 낸다. pluggable `PolicyRule`들이 각각 `Finding`을 반환, 엔진이 집계. 순수 Rust(Rego/OPA·DB·approval state machine 제외).

```
PolicyEngine::evaluate(&OperationGraph) -> PolicyDecision
```

---

## 1. 확정된 설계 결정

| # | 결정 | 내용 |
|---|---|---|
| D1 | **PolicyRule 트레이트 pluggable** | `evaluate(&OperationGraph) -> Vec<Finding>`. 하드코딩 impl 지금, RegoRule 미래 |
| D2 | **입력 = OperationGraph** | 실제 권한 비트(CreateRole permissions, overwrite allow)가 여기 있음. 비트 디코드로 admin 탐지 |
| D3 | **verdict = 가장 강한 것이 이김** | `Allow<Warn<RequireApproval<RequireSecondApproval<Deny`(Ord). finding 없으면 Allow |
| D4 | **첫 컷 3규칙** | PrivilegedPermission(deny) / Destructive(delete→approval) / EveryoneChange(approval) |
| D5 | **선행: 권한 비트 2개 추가** | discord-model에 `MENTION_EVERYONE`(1<<17), `MODERATE_MEMBERS`(1<<40) |

---

## 2. 스코프 경계

| Phase 7 담당 | 담당 아님 (후속) |
|---|---|
| PolicyRule/PolicyEngine/Finding/Verdict/PolicyDecision | Rego/OPA → 미래 RegoRule |
| 3 규칙 + verdict 집계 | DB policy config, 서버/플랜별 override |
| 권한 비트 디코드(privileged) | approval state machine → Approval Manager |
| | ownership 기반 규칙(adopted delete 등 — Operation에 ownership 없음) |
| | preview 텍스트 → Simulator/Preview |

---

## 3. Crate 구조 & 의존
```
policy-engine → operation-graph, desired-compiler(NormalizedTarget), discord-model
```
- diff-engine/desired-state 직접 의존 최소화(OperationGraph의 Operation만 봄). `NormalizedTarget`(Everyone 판별)은 desired-compiler에서.
- 파일(예): `src/{lib.rs, verdict.rs, finding.rs, engine.rs, rules/{privileged.rs, destructive.rs, everyone.rs}}`.
- **선행 수정**: discord-model Permissions에 `MENTION_EVERYONE = 1 << 17`, `MODERATE_MEMBERS = 1 << 40` 추가(공식 Discord 비트 검증).

---

## 4. 타입

```
enum Verdict { Allow, Warn, RequireApproval, RequireSecondApproval, Deny }   // Ord(선언 순 = 강도 순)

struct Finding {
    rule_id: String,
    verdict: Verdict,        // 이 finding이 제안하는 강도(warn 이상)
    target: String,          // "role:vip", "channel:general", "overwrite:general:everyone"
    message: String,
}

struct PolicyDecision {
    verdict: Verdict,        // findings의 max, 없으면 Allow
    findings: Vec<Finding>,
}

trait PolicyRule {
    fn id(&self) -> &str;
    fn evaluate(&self, graph: &OperationGraph) -> Vec<Finding>;
}

struct PolicyEngine {
    rules: Vec<Box<dyn PolicyRule + Send + Sync>>,
}
impl PolicyEngine {
    fn with_default_rules() -> Self;                       // 3규칙
    fn evaluate(&self, graph: &OperationGraph) -> PolicyDecision;
}
```
- `evaluate`: 전 rule의 findings 수집 → `verdict = findings.iter().map(|f| f.verdict).max().unwrap_or(Allow)`.

---

## 5. 첫 컷 규칙 (D4)

### 5.1 `PrivilegedPermissionRule` → Deny (대표)
```
PRIVILEGED = ADMINISTRATOR | MANAGE_GUILD | MANAGE_ROLES | MANAGE_CHANNELS
           | KICK_MEMBERS | BAN_MEMBERS | MENTION_EVERYONE | MODERATE_MEMBERS
```
- 각 노드에서 **부여되는 권한** 추출:
  - `CreateRole`/`UpdateRole` → `permissions`(Option)
  - `CreateOverwrite`/`UpdateOverwrite` → `allow`
- `granted & PRIVILEGED != empty` → `Finding{verdict: Deny, target, message: "privileged permission granted: <flags>"}`.
- **no-admin 케이스**: `permissions:"87375"`(ADMINISTRATOR 포함) → Deny.

### 5.2 `DestructiveOperationRule`
- `DeleteRole` → `Finding{RequireApproval, "role deletion"}`.
- `DeleteChannel` → `Finding{RequireSecondApproval, "channel deletion"}`.

### 5.3 `EveryoneChangeRule`
- `CreateOverwrite`/`UpdateOverwrite`에서 `target == NormalizedTarget::Everyone` → `Finding{RequireApproval, "changes @everyone access"}`.
- (인증 시나리오의 general 채널 @everyone deny VIEW는 여기서 RequireApproval — 의도된 동작.)

---

## 6. Phase 7 범위 경계
- ✅ 완전 구현: Verdict/Finding/PolicyDecision/PolicyRule/PolicyEngine, 3규칙, 집계, 권한 비트 2개 추가, 규칙별 테스트
- ❌ 제외: Rego/OPA, DB/서버별 policy, ownership 규칙(adopted delete 등), bot hierarchy 규칙(현재 상태 필요), approval 실행, preview 텍스트, executor 연동

---

## 7. 컨벤션
serde(Verdict/Finding/PolicyDecision 직렬화 — audit/preview 대비)·주석 없음·결정적. `Box<dyn PolicyRule + Send + Sync>`.

---

## 8. 테스트 전략 (⭐ no-admin 차단)
- Verdict Ord/집계(deny > require_approval > ...).
- **PrivilegedPermissionRule**: CreateRole{permissions: ADMINISTRATOR} → Deny finding. permissions "0" → finding 없음. overwrite allow에 MANAGE_ROLES → Deny.
- **DestructiveOperationRule**: DeleteRole → RequireApproval, DeleteChannel → RequireSecondApproval.
- **EveryoneChangeRule**: CreateOverwrite(Everyone) → RequireApproval. Role target → 없음.
- **집계**: 여러 finding → max verdict.
- **⭐ no-admin 통합**: `{"name":"Administrator","permissions":"87375"}` → compile→diff(empty)→graph→PolicyEngine → **verdict=Deny**. (실증 케이스가 이제 차단됨을 검증)
- **인증 시나리오**: verify-gate graph → @everyone deny VIEW로 EveryoneChange RequireApproval, privileged 없음 → verdict=RequireApproval(deny 아님).

---

## 9. Codex 핸드오프
1. discord-model 비트 2개 추가는 공식 Discord 문서 검증(MENTION_EVERYONE=1<<17, MODERATE_MEMBERS=1<<40).
2. Operation에서 부여 권한 추출: match로 각 variant의 permissions/allow 접근.
3. `NormalizedTarget`은 operation-graph 경유(Operation의 overwrite target) 또는 desired-compiler에서. Everyone 판별.
4. 완료 게이트: build/test/clippy(-D warnings)/fmt. members에 `crates/policy-engine` 추가.
