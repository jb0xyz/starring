# Approval Manager 설계 스펙 (Phase 11)

- **작성일**: 2026-07-09
- **상태**: 확정 (구현 대기 — Codex 핸드오프 대상)
- **범위**: Phase 11 — `crates/approval-manager` (verdict → 승인 상태머신)
- **선행**: Phase 1~10 완료. 순수 코어의 **마지막 결정론 관문** (Executor 앞).

> 승인 정책 변경 (2026-07-31): `RequireSecondApproval`,
> `PendingSecondApproval`, 2인 quorum은
> `2026-07-31-solo-product-approval-design.md`에 의해 폐기되었습니다.
> 승인 필요 작업은 요청자를 포함한 한 명의 인증된 관리자가 승인합니다.

---

## 0. 목적

PreviewModel/Policy verdict를 받아 **"이 작업이 실행 가능한가"**를 결정하는 **순수 승인 상태머신**. 저장 state enum 없이 원시 데이터에서 `state()`/`can_execute()`를 파생. DB/API/실행/twilight 없음. 원칙: "AI 제안 · Core 검증 · **사용자 승인** · Executor는 승인된 것만 실행."

```
ApprovalRequest::from_preview(&PreviewModel, requester) / ::new(Verdict, requester)
  → approve(UserId) / reject(UserId, reason)
  → state() -> ApprovalState / can_execute() -> bool
```

---

## 1. 확정된 설계 결정

| # | 결정 | 내용 |
|---|---|---|
| D1 | **파생 상태** | state enum 저장 X. 원시 데이터(verdict/required/approvals/rejection)에서 `state()` 순수 계산 → 불일치 버그 원천 차단 |
| D2 | **required_approvals** | Allow/Warn→0, RequireApproval→1, RequireSecondApproval→2, Deny→0(단 state=Blocked) |
| D3 | **approve/reject = Pending일 때만** | 그 외 상태는 타입드 error. 같은 user 중복 승인 금지 |
| D4 | **self-approval 허용(첫 컷)** | requester도 승인 가능. 팀/권한 검증은 후속(ApprovalPolicy) |
| D5 | **생성자 2개** | `new(Verdict, requester)`(코어) + `from_preview(&PreviewModel, requester)`(preview.verdict 추출) |

---

## 2. 스코프 경계

| Phase 11 담당 | 담당 아님 (후속) |
|---|---|
| ApprovalRequest, state()/can_execute() | DB 저장, API endpoint, push 알림 |
| approve/reject + 에러 게이트 | 실제 Discord 실행 → Executor |
| required_approvals 계산 | 실제 approver 권한/역할/팀 검증 |
| | 만료 시간, audit log 저장 |

**의존 금지**: db, api, executor, bot-runtime, twilight, ai-gateway, web/app.

---

## 3. Crate 구조 & 의존
```
approval-manager → {policy-engine(Verdict), discord-model(UserId), preview(PreviewModel)}
```
preview 의존은 `from_preview` 때문(자연스러움 — 승인은 preview 다음 관문). 파일(예): `src/{lib.rs, request.rs}`.

---

## 4. 타입 & API

```
enum ApprovalState { Blocked, ReadyToExecute, PendingApproval, PendingSecondApproval, Approved, Rejected }
enum ApprovalError { Blocked, AlreadyRejected, AlreadyApproved, DuplicateApproval, NotRequired }
struct Rejection { by: UserId, reason: String }

struct ApprovalRequest {
    verdict: Verdict,
    requester: UserId,
    required_approvals: usize,
    approvals: Vec<UserId>,
    rejection: Option<Rejection>,
}
impl ApprovalRequest {
    fn new(verdict: Verdict, requester: UserId) -> Self
    fn from_preview(preview: &PreviewModel, requester: UserId) -> Self   // = new(preview.verdict, requester)
    fn state(&self) -> ApprovalState
    fn can_execute(&self) -> bool
    fn approve(&mut self, user: UserId) -> Result<(), ApprovalError>
    fn reject(&mut self, user: UserId, reason: &str) -> Result<(), ApprovalError>
}
```
전부 `Clone + Debug + PartialEq + Eq + Serialize + Deserialize`. ApprovalError는 thiserror `Error`.

---

## 5. 상태머신 (심장)

**required_for(verdict)**: Allow|Warn→0, RequireApproval→1, RequireSecondApproval→2, Deny→0.

**state() (우선순위 순 — Deny 최우선)**:
1. `verdict == Deny` → **Blocked**
2. `rejection.is_some()` → **Rejected**
3. `required_approvals == 0` → **ReadyToExecute**
4. `approvals.len() >= required_approvals` → **Approved**
5. `required_approvals == 2 && approvals.len() == 1` → **PendingSecondApproval**
6. 그 외 → **PendingApproval**

**can_execute()** = `matches!(state(), ReadyToExecute | Approved)`.

**approve(user) / reject(user, reason)** — `state()`로 게이트(동일):
- `Blocked` → `Err(Blocked)`
- `Rejected` → `Err(AlreadyRejected)`
- `ReadyToExecute` → `Err(NotRequired)`
- `Approved` → `Err(AlreadyApproved)`
- `Pending*` → 진행:
  - approve: `approvals.contains(user)` → `Err(DuplicateApproval)`; else `approvals.push(user)`.
  - reject: `rejection = Some(Rejection{by:user, reason})`.

---

## 6. Phase 11 범위 경계
- ✅ 완전 구현: ApprovalRequest, new/from_preview, state()/can_execute(), approve/reject + 5 error, required 계산
- ❌ 제외: DB, API, push, Discord 실행, approver 권한 검증, 만료, audit

---

## 7. 컨벤션
serde(ApprovalRequest 직렬화 — 후속 persistence 대비)·주석 없음·결정적(state 순수 파생). 비트 아님.

---

## 8. 테스트 전략 (⭐ 상태머신 시나리오)
1. **RequireApproval**: new → PendingApproval, required=1. approve(u1) → Approved, can_execute=true.
2. **Deny**: new → Blocked, can_execute=false. approve(u1) → Err(Blocked).
3. **RequireSecondApproval**: new → PendingApproval. approve(u1) → PendingSecondApproval, can_execute=false. approve(u1 재시도) → Err(DuplicateApproval). approve(u2) → Approved, can_execute=true.
4. **Rejection**: RequireApproval → reject(u1,"reason") → Rejected, can_execute=false. approve(u2) → Err(AlreadyRejected).
5. **Ready(Allow/Warn)**: new(Allow) → ReadyToExecute, can_execute=true, required=0. approve → Err(NotRequired).
6. **from_preview**: 최소 PreviewModel(verdict=RequireApproval) → from_preview → required=1.
7. serde 라운드트립(ApprovalRequest).

---

## 9. Codex 핸드오프
1. `state()`는 순수 파생 — 매 호출 계산(캐시 X). approve/reject는 `state()`로 게이트 후 원시 데이터만 변경.
2. `required_approvals`는 `new()`에서 `required_for(verdict)`로 1회 계산·저장(verdict 불변이라 divergence 없음).
3. Verdict는 policy-engine, UserId는 discord-model, PreviewModel은 preview에서. `preview.verdict`로 from_preview.
4. ApprovalError는 thiserror `#[derive(Error)]` + `#[error("...")]`.
5. 완료 게이트: build/test/clippy(-D warnings)/fmt. members에 `crates/approval-manager` 추가. 완료 후 `git push origin main`.
