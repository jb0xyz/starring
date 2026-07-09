# Automatic Rollback Execution 설계 스펙 (Phase 15)

- **작성일**: 2026-07-10
- **상태**: 확정 (구현 대기 — Codex 핸드오프)
- **범위**: Phase 15 — executor-core에 opt-in 자동 rollback (`execute_with_rollback`)
- **선행**: Phase 12(executor-core, RollbackAction 캡처). **bot-runtime/twilight 변경 없음.**

---

## 0. 목적

**자동 best-effort rollback + 명시적 부분 보고.** forward 실행 실패 시, 성공했던 step의 RollbackAction을 역순 실행해 원복. **현재 adapter가 지원 못 하는 action(RestoreOverwrite before None)은 숨기지 않고 Skipped/Partial로 기록.** 기존 `execute()` 의미 불변.

---

## 1. 확정 결정

| # | 결정 | 내용 |
|---|---|---|
| A | **execute() 불변** | forward 순수 실행기 그대로. 기존 테스트 무변경 |
| B | **execute_with_rollback() 신설** | forward 실패 시 rollback까지 시도하는 상위 실행기(opt-in) |
| C | **RestoreOverwrite{None}→Skipped** | delete_overwrite 미지원(adapter 확장은 후속). reason 명시 |
| D | **JobRun로 감싸 기록** | `JobRun{job, rollback}`. rollback 결과를 실행 결과처럼 남김 |

---

## 2. 타입 (result.rs 추가)
```rust
pub struct JobRun { pub job: JobResult, pub rollback: RollbackReport }
pub struct RollbackReport { pub status: RollbackStatus, pub steps: Vec<RollbackStepResult> }
pub enum RollbackStatus { NotRequired, Succeeded, Partial, Failed }
pub struct RollbackStepResult { pub source_op_id: OpId, pub action: RollbackAction, pub outcome: RollbackOutcome }
pub enum RollbackOutcome { Undone, Failed(AdapterError), Skipped { reason: String } }
```
전부 Clone+Debug+PartialEq+Eq+Serialize+Deserialize.

---

## 3. RollbackStatus 계산 (명확히)
```
steps empty        → NotRequired
all Undone         → Succeeded
all Failed         → Failed
그 외(Skipped 포함) → Partial
```
> Skipped는 Failed 아님(adapter 호출 실패가 아니라 정직한 건너뜀) → Partial 쪽.

---

## 4. 실행 흐름 (execute_with_rollback)
1. `self.execute(request).await` 실행.
2. `Err(NotApproved/GraphCycle)`면 그대로 Err 반환(rollback 없음).
3. `JobStatus::Succeeded`면 `RollbackReport{ NotRequired, [] }`.
4. `JobStatus::Failed`면: `job.steps`에서 **outcome==Success && rollback.is_some()**인 step만 수집 → **역순** → 각 RollbackAction 실행 → RollbackStepResult 기록.
5. `Ok(JobRun{ job, rollback })`.

Skipped/Failed forward step은 성공 변경이 없어 rollback 없음.

---

## 5. RollbackAction 실행 매핑
| action | adapter 호출 |
|---|---|
| DeleteRole{id} | delete_role(guild, id) |
| RestoreRole{id, before} | update_role(guild, id, RoleSpec{name:Some(before.name), permissions:Some(before.permissions)}) |
| RecreateRole{before} | create_role(guild, RoleSpec{...}) (id 반환은 무시) |
| DeleteChannel{id} | delete_channel(guild, id) |
| RestoreChannel{id, before} | update_channel(guild, id, ChannelSpec{name:Some(before.name), channel_type:Some(before.channel_type), parent_id:before.parent_id}) |
| RecreateChannel{before} | create_channel(guild, ChannelSpec{...}) |
| RestoreOverwrite{ch, target, before:Some(o)} | upsert_overwrite(guild, ch, target, o.allow, o.deny) |
| RestoreOverwrite{before:None} | **호출 안 함 → Skipped{reason:"delete overwrite is not supported in Phase 15"}** |

adapter 결과 Ok→Undone, Err(e)→Failed(e). (Recreate*는 Discord상 원 id 복구 불가라 엄밀히 best-effort지만 첫 컷은 그대로 실행.)

---

## 6. Mock 다중 실패 (mock.rs)
- `fail_on: Vec<(usize, AdapterError)>`로 변경.
- `with_failure(n, err)` 유지(내부 `vec![(n, err)]`). `with_failures(Vec<(usize, AdapterError)>)` 추가.
- `check_fail(n)`: fail_on에서 n 매칭 시 Err. **콜 카운트는 forward+rollback 공유**(rollback 콜도 같은 카운터).

---

## 7. 스코프 경계
- ✅ execute_with_rollback, rollback 타입/실행/보고, Mock 다중실패
- ❌ execute() 수정, delete_overwrite/adapter 확장, bot-runtime/twilight, retry loop, rollback approval, live Discord

---

## 8. 컨벤션
주석 없음. rollback 실패는 **panic 아니라 기록**. 결정적.

---

## 9. 테스트
1. forward 전부 성공 → Succeeded / rollback NotRequired / steps empty.
2. op1 성공·op2 실패 → job Failed / op1 DeleteRole rollback → Succeeded.
3. op1·op2 성공·op3 실패 → rollback 순서 op2→op1(콜 시퀀스 검증).
4. rollback 자체 실패(with_failures forward+rollback) → RollbackOutcome::Failed / status Failed.
5. 일부 성공+일부 실패/skip → Partial.
6. RestoreOverwrite{None} → adapter 미호출 / Skipped / Partial (rollback() 직접 호출로 테스트).
7. NotApproved → execute_with_rollback도 Err(NotApproved) / forward·rollback 콜 0.
8. **기존 execute() 테스트 무변경 통과**(자동 rollback으로 콜 수 안 깨짐).

---

## 10. Codex 핸드오프
1. execute() 절대 수정 금지. execute_with_rollback/rollback/run_rollback/rollback_status 추가.
2. 5절 매핑 그대로. RestoreOverwrite{None}→Skipped(reason). RollbackOutcome::Failed에 AdapterError.
3. Mock fail_on→Vec, with_failure 시그니처 유지, with_failures 추가. 카운트 forward+rollback 공유.
4. rollback()은 private + async(직접 테스트는 execute.rs #[cfg(test)]에서). execute_with_rollback 흐름은 tests/executor_scenario.rs.
5. 게이트: build/test/clippy(-D warnings)/fmt. lib.rs에 신규 타입 export. 완료 후 `git push origin main`. **live Discord/토큰 없음.**
