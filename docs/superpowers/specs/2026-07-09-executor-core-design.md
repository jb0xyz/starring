# Executor Core 설계 스펙 (Phase 12b)

- **작성일**: 2026-07-09
- **상태**: 확정 (구현 대기 — Codex 핸드오프)
- **범위**: Phase 12b — `crates/executor-core` (Executor 상태머신 + DiscordAdapter trait + Mock)
- **선행**: Phase 12a 완료(resource-resolution). 상위 스펙: `2026-07-09-executor-bot-runtime-design.md`.

---

## 0. 목적

승인된 OperationGraph를 **DiscordAdapter trait 뒤의 실행기**로 topo 순서 실행 → JobResult. 실세계 Discord는 `A: DiscordAdapter`로 격리 → **MockDiscordAdapter로 결정론 테스트**(토큰/네트워크 0). 첫 async지만 순수 코어 정신 유지(tokio/async-trait 없음).

```
Executor::new(adapter).execute(&ApprovedExecutionRequest).await -> Result<JobResult, ExecutorError>
```

---

## 1. 확정 결정 (12b 브레인스토밍)

| # | 결정 | 내용 |
|---|---|---|
| D1 | **native async fn in trait** | `#[allow(async_fn_in_trait)]` + generic `Executor<A: DiscordAdapter>` 정적 디스패치. async-trait/dyn 없음(YAGNI). Send 바운드 없음(in-process MVP) |
| D2 | **테스트 = futures block_on** | executor-core `[dev-dependencies] futures`. `block_on(async { ... })`. **tokio는 executor-core에 없음**(12c bot-runtime만) |
| D3 | **MockDiscordAdapter 3요소** | `next_id: AtomicU64`(fake real id, base 900_000) + `calls: Mutex<Vec<AdapterCall>>`(콜 기록) + `fail_on: Option<(usize, AdapterError)>`(1-based N번째 콜 실패) |
| D4 | **해소는 resource-resolution 재사용** | Virtual Apply와 동일 `ResourceResolutionContext`. Create→adapter 응답 id를 bind, 기존→resolver. **preview-exec 동일 경로** |
| D5 | **fail-fast + rollback 캡처** | 상위 스펙 §4/§5 그대로. Success step만 RollbackAction, snapshot 역산 |

---

## 2. 스코프 경계

| Phase 12b 담당 | 담당 아님 |
|---|---|
| Executor·execute()·fail-fast·topo·해소·rollback 캡처 | 실제 twilight 호출 → 12c |
| DiscordAdapter trait + MockDiscordAdapter | 자동 rollback 실행, retry loop |
| ApprovedExecutionRequest·StepResult·JobResult·RollbackAction·AdapterError | NATS/DB/gateway |

**의존 금지**: twilight, tokio, async-trait, NATS, DB.

---

## 3. Crate 구조 & 의존
```
executor-core → {operation-graph, approval-manager, resource-resolution, desired-compiler, desired-state, discord-model}
             [dev] futures
```
파일(예): `src/{lib.rs, adapter.rs, mock.rs, request.rs, result.rs, error.rs, execute.rs}`.

---

## 4. 타입

**입력** (판단 X, 게이트만 — Virtual Apply처럼 normalized 필요):
```rust
struct ApprovedExecutionRequest {
    operation_graph: OperationGraph,
    normalized: NormalizedDesiredState,   // 해소용(identity 조회) — VA와 동일
    approval: ApprovalRequest,            // execute 첫 줄: can_execute() 아니면 NotApproved
    snapshot: GuildState,                 // before-state (해소 base + rollback 역산원)
    guild_id: GuildId,
    requested_by: UserId,
    approved_by: Vec<UserId>,
}
enum ExecutorError { NotApproved, GraphCycle }   // pre-flight 거부(실행 시작 안 함)
```

**어댑터** (`#[allow(async_fn_in_trait)]`):
```rust
trait DiscordAdapter {
    async fn create_role(&self, guild: GuildId, spec: RoleSpec) -> Result<RoleId, AdapterError>;
    async fn update_role(&self, guild: GuildId, id: RoleId, spec: RoleSpec) -> Result<(), AdapterError>;
    async fn delete_role(&self, guild: GuildId, id: RoleId) -> Result<(), AdapterError>;
    async fn create_channel(&self, guild: GuildId, spec: ChannelSpec) -> Result<ChannelId, AdapterError>;
    async fn update_channel(&self, guild: GuildId, id: ChannelId, spec: ChannelSpec) -> Result<(), AdapterError>;
    async fn delete_channel(&self, guild: GuildId, id: ChannelId) -> Result<(), AdapterError>;
    async fn upsert_overwrite(&self, guild: GuildId, channel: ChannelId, target: OverwriteTarget, allow: Permissions, deny: Permissions) -> Result<(), AdapterError>;
}
struct RoleSpec { name: Option<String>, permissions: Option<Permissions> }
struct ChannelSpec { name: Option<String>, channel_type: Option<ChannelType>, parent_id: Option<ChannelId> }
struct AdapterError { kind: AdapterErrorKind, message: String }
enum AdapterErrorKind { RateLimited, Timeout, Network, ServerError, Forbidden, MissingPermissions, RoleHierarchy, NotFound, BadRequest, Unknown }
// is_retryable() = RateLimited|Timeout|Network|ServerError (Unknown 포함 나머지는 fatal)
```

**결과** (상위 스펙 §3):
```rust
enum StepOutcome { Success, FailedRetryable(AdapterError), FailedFatal(AdapterError), Skipped }
enum CreatedResource { Role { key: ResourceKey, id: RoleId }, Channel { key: ResourceKey, id: ChannelId } }
struct StepResult { op_id: OpId, outcome: StepOutcome, created: Option<CreatedResource>, rollback: Option<RollbackAction> }
enum JobStatus { Succeeded, Failed }
struct JobResult { status: JobStatus, steps: Vec<StepResult> }
enum RollbackAction { DeleteRole{id}, RestoreRole{id, before: Role}, RecreateRole{before: Role},
                      DeleteChannel{id}, RestoreChannel{id, before: Channel}, RecreateChannel{before: Channel},
                      RestoreOverwrite{channel: ChannelId, target: OverwriteTarget, before: Option<PermissionOverwrite>} }
```

**Executor**:
```rust
struct Executor<A: DiscordAdapter> { adapter: A }
impl<A: DiscordAdapter> Executor<A> {
    fn new(adapter: A) -> Self
    async fn execute(&self, request: &ApprovedExecutionRequest) -> Result<JobResult, ExecutorError>
}
```

---

## 5. 실행 알고리즘 (심장)

`execute(&self, request)`:
1. **pre-flight**: `!request.approval.can_execute()` → `Err(NotApproved)`. `graph.topological_order()` 실패 → `Err(GraphCycle)`.
2. `resolver = InMemoryMatchResolver::new(&request.snapshot)`. `ctx = ResourceResolutionContext::new(&request.normalized, &resolver, request.guild_id)`.
3. topo 순서 노드 순회, `stopped=false`:
   - `stopped`이면 → StepResult{op_id, Skipped, None, None}.
   - 아니면 `run_op(op, ctx, snapshot).await`:
     - **해소 실패**(ResolutionError) → StepResult{FailedFatal(AdapterError{Unknown, "unresolved: ..."}), ...}, `stopped=true`.
     - **rollback before** = snapshot에서 조회(update/delete/overwrite).
     - **adapter 호출**(await):
       - `Ok` → Success + created(Create) + rollback. Create면 `ctx.bind_*(key, 실제id)`.
       - `Err(e)` → `if e.is_retryable(){FailedRetryable}else{FailedFatal}(e)`, `stopped=true`.
   - StepResult 기록.
4. `status = if steps.any(실패) { Failed } else { Succeeded }`. `Ok(JobResult{status, steps})`.

**op별 (adapter 호출 / created / rollback)**:
| op | 해소 | adapter | created | rollback |
|---|---|---|---|---|
| CreateRole | — | create_role→id, bind_role | Role{key,id} | DeleteRole{id} |
| UpdateRole | resolve_role_key | update_role(id) | — | RestoreRole{id, snapshot 역할} |
| DeleteRole | resolve_role_key | delete_role(id) | — | RecreateRole{snapshot 역할} |
| CreateChannel | parent resolve | create_channel→id, bind_channel | Channel{key,id} | DeleteChannel{id} |
| UpdateChannel | resolve_channel_key | update_channel(id) | — | RestoreChannel{id, snapshot 채널} |
| DeleteChannel | resolve_channel_key | delete_channel(id) | — | RecreateChannel{snapshot 채널} |
| Create/UpdateOverwrite | resolve_channel_key + resolve_target | upsert_overwrite | — | RestoreOverwrite{ch, target, snapshot의 해당 target overwrite(Option)} |

> RollbackAction의 before는 **snapshot에서 resolved id로 조회**(추가 Discord read 0). Update/Delete는 해소된 id로 snapshot.roles/channels에서 find. Overwrite before는 snapshot 채널의 overwrites에서 target 일치 항목(없으면 None).

---

## 6. MockDiscordAdapter (D3)
```rust
struct MockDiscordAdapter { next_id: AtomicU64, calls: Mutex<Vec<AdapterCall>>, fail_on: Option<(usize, AdapterError)> }
enum AdapterCall { CreateRole{guild, spec}, UpdateRole{guild, id, spec}, DeleteRole{guild, id},
                   CreateChannel{guild, spec}, UpdateChannel{guild, id, spec}, DeleteChannel{guild, id},
                   UpsertOverwrite{guild, channel, target, allow, deny} }
```
- 각 async 메서드: **1-based 콜 카운트** 증가 → 기록(`calls.push`). `fail_on == Some((n, err))`이고 현재 콜==n이면 `Err(err.clone())`. 아니면 create는 `Ok(RoleId/ChannelId(next_id.fetch_add(1)))`(base 900_000), 나머지 `Ok(())`.
- `new()`(fail 없음), `with_failure(n, err)`, `calls()` 접근자.

---

## 7. 컨벤션
serde(요청/결과 직렬화 — 후속 NATS/audit 대비)·주석 없음·결정적(Mock 카운터·topo 순서). `#[allow(async_fn_in_trait)]`만 예외적 allow.

---

## 8. 테스트 전략 (⭐ 3축, block_on)
1. **성공**: 인증 시나리오 fixture(CreateRole verified + Update/Create overwrite general) → Mock 실행 → JobStatus=Succeeded, 모든 step Success, created id 기록, **AdapterCall 시퀀스**가 topo와 일치, overwrite target이 create_role 반환 id 사용(스레딩).
2. **fail-fast**: `with_failure(2, fatal)` → op1 Success, op2 FailedFatal, op3+ Skipped, JobStatus=Failed. retryable 버전(`with_failure(1, rate_limited)`)도 → FailedRetryable + 중단.
3. **거부/rollback**: `can_execute()=false`(Deny approval) → Err(NotApproved), adapter 콜 0. + Success step의 rollback 필드 검증(CreateRole→DeleteRole{id}, overwrite→RestoreOverwrite{before}).

---

## 9. Codex 핸드오프
1. ApprovedExecutionRequest에 **normalized 포함**(VA처럼 해소에 필요). resolver는 snapshot으로 생성.
2. adapter 호출은 `match ... .await`로(? 아님 — 실패는 StepResult로 흡수, fail-fast). 해소 실패도 FailedFatal(Unknown) step.
3. `#[allow(async_fn_in_trait)]` trait에. Executor generic `<A: DiscordAdapter>`. 테스트 `futures::executor::block_on`.
4. Mock 콜카운트 1-based, fake id base 900_000, AtomicU64/Mutex(Send-safe).
5. 완료 게이트: build/test/clippy(-D warnings)/fmt. members에 `crates/executor-core`. 완료 후 `git push origin main`.
