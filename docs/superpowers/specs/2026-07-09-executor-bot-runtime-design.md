# Executor + Bot Runtime 설계 스펙 (Phase 12)

- **작성일**: 2026-07-09
- **상태**: 설계 확정 (구현은 **다음 세션** — 큰 phase라 서브페이즈 분해)
- **범위**: 승인된 OperationGraph → 실제 Discord 실행 (첫 real 엣지)
- **선행**: Phase 1~11 완료(순수 결정 파이프라인). 아키텍처 문서 §11.

---

## 0. 목적

**승인된(can_execute) OperationGraph를 어떻게 안전하게 Discord API 호출로 바꿀 것인가.** Executor는 **판단하지 않는다** — Policy/Approval을 통과한, Preview와 동일한 graph만 실행한다. 핵심 안전장치: DiscordAdapter trait으로 실세계 격리(Core는 결정론 유지), synthetic→real id 해소를 Virtual Apply와 공유(preview-exec drift 방지), fail-fast, rollback 데이터 첫 컷부터 저장.

---

## 1. 7개 확정 설계 결정 (브레인스토밍 결과)

| Q | 결정 | 핵심 |
|---|---|---|
| **Q1** | **DiscordAdapter trait** | Executor Core는 `A: DiscordAdapter`만 의존. `MockDiscordAdapter`(테스트)/`TwilightDiscordAdapter`(실제). twilight/tokio/HTTP는 어댑터 뒤. **첫 async 진입** — 어댑터 메서드 `async fn`, Core도 async지만 Mock으로 결정론 테스트 |
| **Q2** | **공용 resource-resolution 추출** | Phase 9 `ApplyContext`의 해소 로직을 `crates/resource-resolution`로 추출. `ResourceResolutionContext{bindings, normalized, resolver, guild_id}`. **레이어는 id를 mint 안 함** — 호출자가 mint(VA=synthetic, Executor=adapter 응답)하고 `bind`. 3단계 해소(binding→resolver→error) 공유 → **preview-exec drift 방지** |
| **Q3** | **4변주 StepOutcome** | `Success/FailedRetryable/FailedFatal/Skipped`. retry는 후속, **분류·Skipped는 지금**. `AdapterError{kind, message}` + `is_retryable()`(kind만 봄, 메시지 파싱 X) |
| **Q4** | **fail-fast 전체중단** | 첫 실패(retryable 무관) 즉시 중단 → 남은 **모든** 노드 Skipped → JobStatus=Failed. `steps`는 모든 executable 노드 담음(부분적용 가시). 입력 `ApprovedExecutionRequest` |
| **Q5** | **전체 RollbackAction 캡처** | Success step마다 역산 저장(실행은 후속). before-state는 실행시점에만 존재 → snapshot 기반 순수 역산(추가 read 0). reverse-order replay는 미래 |
| **Q6** | **in-process MVP** | `executor-core`(순수)/`bot-runtime`(twilight) crate 분리, 첫 경로 in-process. NATS worker는 후속 transport(Core 불변). **REST mutation만** — gateway/shard/event 제외 |
| **Q7** | **수동 smoke 도구** | 결정론=MockAdapter `cargo test`. 실세계=`tools/executor-smoke`(`DISCORD_TEST_TOKEN`/`GUILD`, `live-discord` feature, 수동 실행, RollbackAction 자가청소). Phase 6 ai-eval 패턴 |

---

## 2. Crate 구조 & 의존
```
resource-resolution → {desired-compiler, desired-state, diff-engine, discord-model}   (Phase 9에서 추출)
executor-core        → {operation-graph, approval-manager, resource-resolution, discord-model, desired-compiler}
bot-runtime          → {executor-core, twilight, tokio}          [live-discord feature]
tools/executor-smoke → {전 파이프라인 + bot-runtime}             [live-discord feature, 수동]
virtual-apply        → resource-resolution 재사용하도록 리팩토링
```
**executor-core 의존 금지**: twilight, tokio 런타임 셋업, NATS, DB, HTTP server, gateway.

---

## 3. 핵심 타입

**입력 (판단 안 함, 게이트만):**
```rust
struct ApprovedExecutionRequest {
    operation_graph: OperationGraph,
    approval: ApprovalRequest,      // 첫 줄: if !approval.can_execute() { refuse }
    snapshot: GuildState,           // before-state (해소 base + rollback 데이터원)
    guild_id: GuildId,
    requested_by: UserId,
    approved_by: Vec<UserId>,
}
```

**DiscordAdapter (실세계 seam):**
```rust
trait DiscordAdapter {
    async fn create_role(&self, guild: GuildId, spec: RoleSpec) -> Result<RoleId, AdapterError>;
    async fn update_role(&self, guild: GuildId, id: RoleId, spec: RoleSpec) -> Result<(), AdapterError>;
    async fn delete_role(&self, guild: GuildId, id: RoleId) -> Result<(), AdapterError>;
    async fn create_channel(&self, guild: GuildId, spec: ChannelSpec) -> Result<ChannelId, AdapterError>;
    async fn update_channel(&self, guild: GuildId, id: ChannelId, spec: ChannelSpec) -> Result<(), AdapterError>;
    async fn delete_channel(&self, guild: GuildId, id: ChannelId) -> Result<(), AdapterError>;
    async fn upsert_overwrite(&self, guild: GuildId, ch: ChannelId, target: OverwriteTarget, allow: Permissions, deny: Permissions) -> Result<(), AdapterError>;
}
struct RoleSpec { name: Option<String>, permissions: Option<Permissions> }
struct ChannelSpec { name: Option<String>, channel_type: Option<ChannelType>, parent_id: Option<ChannelId> }  // parent는 resolved id
```
generic `Executor<A: DiscordAdapter>` 정적 디스패치(native async fn in traits, stable). `create_*`가 **실제 id 반환** → binding.

**결과:**
```rust
enum StepOutcome { Success, FailedRetryable(AdapterError), FailedFatal(AdapterError), Skipped }
enum CreatedResource { Role { key: ResourceKey, id: RoleId }, Channel { key: ResourceKey, id: ChannelId } }
struct StepResult { op_id: OpId, outcome: StepOutcome, created: Option<CreatedResource>, rollback: Option<RollbackAction> }
enum JobStatus { Succeeded, Failed }
struct JobResult { status: JobStatus, steps: Vec<StepResult> }   // steps=모든 executable 노드

struct AdapterError { kind: AdapterErrorKind, message: String }
enum AdapterErrorKind { RateLimited, Timeout, Network, ServerError,        // retryable
                        Forbidden, MissingPermissions, RoleHierarchy, NotFound, BadRequest, Unknown }  // fatal
enum RollbackAction {
    DeleteRole { id: RoleId }, RestoreRole { id: RoleId, before: Role }, RecreateRole { before: Role },
    DeleteChannel { id: ChannelId }, RestoreChannel { id: ChannelId, before: Channel }, RecreateChannel { before: Channel },
    RestoreOverwrite { channel: ChannelId, target: OverwriteTarget, before: Option<PermissionOverwrite> },
}
```

**공용 해소 (resource-resolution):**
```rust
struct ResourceBindingMap { role_bindings: HashMap<ResourceKey, RoleId>, channel_bindings: HashMap<ResourceKey, ChannelId> }
struct ResourceResolutionContext<'a, R: ResourceResolver> { bindings: ResourceBindingMap, normalized: &'a NormalizedDesiredState, resolver: &'a R, guild_id: GuildId }
// resolve_role_key/resolve_channel_key (binding→resolver→UnresolvedKey/MissingIdentity), bind_role/bind_channel, resolve_target(Everyone→RoleId(guild), Role(key)→resolve, Member→parse)
struct ExecutionContext<'a, R> { guild_id: GuildId, resources: ResourceResolutionContext<'a, R>, step_results: Vec<StepResult> }
```

---

## 4. 실행 로직 (심장)

`execute(request, adapter) -> JobResult`:
1. `if !request.approval.can_execute()` → refuse(실행 안 함).
2. `ctx = ExecutionContext`(snapshot 기반 resolver + 빈 bindings).
3. `graph.topological_order()` 순회. 각 op:
   - 실행 전: snapshot에서 before-value 조회(rollback 준비용).
   - Create: `adapter.create_*().await` → real id → `ctx.bind_*(key, id)` → `created`+`rollback=Delete{id}`.
   - Update: resolve id → `adapter.update_*().await` → `rollback=Restore{id, before}`.
   - Delete: resolve id → `adapter.delete_*().await` → `rollback=Recreate{before}`.
   - Overwrite: resolve ch id + target → `adapter.upsert_overwrite().await` → `rollback=RestoreOverwrite{before: Option}`.
   - 성공 → `StepResult{Success, created, rollback}`.
   - 실패 → `outcome = if err.is_retryable(){FailedRetryable}else{FailedFatal}`, **즉시 중단**.
4. 중단 시 남은 노드 전부 `Skipped`. `JobStatus = Failed`(실패 있으면) / `Succeeded`(전부 Success).

---

## 5. 스코프 경계

| Phase 12 담당 | 담당 아님 (후속) |
|---|---|
| Executor Core(topo·해소·StepResult·fail-fast·rollback 캡처) | 자동 rollback 실행, rollback approval |
| DiscordAdapter trait + Mock + Twilight(REST) | retry loop/backoff/rate-limit queue |
| in-process 실행, executor-smoke 수동 도구 | NATS worker, job persistence, DB binding registry |
| REST mutation(role/channel/overwrite) | gateway/shard/event/interaction(verification 버튼) |
| snapshot 기반 rollback 데이터 | continue-on-error, 부분 재개(resume) |

---

## 6. 테스트 전략 (3계층)
1. **Executor Core + MockDiscordAdapter** (`cargo test`): topo·id바인딩·StepResult·fail-fast·Skipped·RollbackAction 생성 — 로직 100%, 토큰/네트워크 0. 크라운 주얼: 인증 시나리오 fixture → Mock 실행 → 예상 adapter 콜 시퀀스 + JobResult + rollback 데이터.
2. **AdapterError 분류** (`cargo test`): `twilight/HTTP status → AdapterErrorKind` 순수 함수(429→RateLimited, 403→Forbidden/MissingPermissions, 404→NotFound, 400→BadRequest, 5xx→ServerError).
3. **Live smoke** (`tools/executor-smoke`, 게이트): 실제 테스트 길드에 fixture 실행 → JobResult 확인 → **Success step RollbackAction 역순 실행해 자가청소**. 리소스 prefix `starring-smoke-*`. cleanup 실패도 리포트. `cargo test`는 토큰 없이 항상 green.

---

## 7. 서브페이즈 분해 (다음 세션 플랜 단위)
큰 phase라 순서대로 나눠 구현:
- **12a — resource-resolution 추출**: Phase 9 `ApplyContext` 해소를 공용 crate로. virtual-apply 리팩토링. **126 테스트 그대로 통과**(안전한 순수 리팩토링). *(먼저)*
- **12b — executor-core**: Executor + DiscordAdapter trait + MockDiscordAdapter + 전 타입 + fail-fast + rollback 캡처. Mock 결정론 테스트. **이 phase의 코어·bulk.** *(순수)*
- **12c — bot-runtime**: TwilightDiscordAdapter(REST) + AdapterError 분류. twilight/tokio/async. *(첫 real 엣지)*
- **12d — tools/executor-smoke**: 수동 live 검증 + rollback 자가청소. *(수동 도구)*

12a→12b는 순수(Claude 재현 검증 가능), 12c→12d는 실세계(수동 검증 + 토큰 필요).

---

## 8. 다음 세션 핸드오프
- 이 스펙 확정 → 12a 플랜부터(안전 리팩토링) → 12b(순수 코어) → 12c/12d(엣지).
- 준비물(12c/12d): Discord **테스트 서버**(throwaway), 봇 토큰(admin 권한), `DISCORD_TEST_TOKEN`/`DISCORD_TEST_GUILD`.
- 원칙 불변: 주석 없음, TDD, 게이트(build/test/clippy/fmt), Codex 구현·Claude 재현 검증, Task 후 `git push origin main`.
- **아키텍처 문서 §11(Executor)** 업데이트 필요할 수 있음(현 설계 반영).
