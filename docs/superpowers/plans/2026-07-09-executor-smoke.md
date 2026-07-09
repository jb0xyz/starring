# Executor Smoke Tool Implementation Plan (Phase 12d)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. **Codex가 구현한다.** Task 끝에 보고. **완료 후 git push origin main.**

**Goal:** `tools/executor-smoke` — 실제 Discord 테스트 길드에 fixture를 실행하는 **수동 live 도구**. 전체 파이프라인(DesiredState→...→graph) → `Executor<TwilightDiscordAdapter>` → JobResult 출력 → RollbackAction 역순 자가청소. **12d는 컴파일 + fixture 순수 테스트까지 검증**; 실제 live 실행은 사용자가 토큰으로 수동 실행.

**Architecture:** tokio 바이너리. env(`DISCORD_TEST_TOKEN`/`DISCORD_TEST_GUILD`) 없으면 안내 후 종료(안전). fixture는 create-only(role+channel+overwrite, prefix `starring-smoke-*`)라 cleanup이 role/channel 삭제로 완전 정리(overwrite는 채널 삭제로 함께 제거).

**Tech Stack:** Rust edition 2021 stable, tokio, rustls(crypto provider), 전 파이프라인 + executor-core + bot-runtime.

## Global Constraints
> ⚠️ **주석 금지**. **fixture는 pure(테스트)·run/cleanup은 live(컴파일만)**. 실제 Discord 실행은 사용자 수동.
- 의존: `tools/executor-smoke → {전 파이프라인, executor-core, bot-runtime, tokio, rustls}`.
- **⚠️ #1 live-run 리스크 = rustls crypto provider**: `Client::new`가 provider 미설정 시 런타임 panic. 플랜은 `install_default()` 방식이지만 **rustls 버전이 twilight-http 0.17의 transitive rustls와 일치해야** 효과 있음. **Codex가 확인**(또는 twilight-http crypto feature 사용). 컴파일엔 무관, live 실행 시에만 문제 → 사용자가 실행하며 검증.
- 스펙: `docs/superpowers/specs/2026-07-09-executor-bot-runtime-design.md` §6/§8(Q7).
- 완료 게이트: build/test/clippy(-D warnings)/fmt. **Phase 완료 후 `git push origin main`.**

---

### Task 1: executor-smoke 바이너리

**Files:**
- Modify: `Cargo.toml`
- Create: `tools/executor-smoke/Cargo.toml`, `src/main.rs`

**Interfaces:**
- Produces: `fixture(GuildId) -> ApprovedExecutionRequest`(pure, 테스트), `cleanup`, `main`(live).

- [ ] **Step 1: 워크스페이스 + Cargo**

Root `Cargo.toml` members에 `"tools/executor-smoke"` 추가.

Create `tools/executor-smoke/Cargo.toml`:
```toml
[package]
name = "executor-smoke"
version = "0.1.0"
edition.workspace = true

[dependencies]
discord-model = { path = "../../crates/discord-model" }
desired-state = { path = "../../crates/desired-state" }
desired-compiler = { path = "../../crates/desired-compiler" }
diff-engine = { path = "../../crates/diff-engine" }
operation-graph = { path = "../../crates/operation-graph" }
approval-manager = { path = "../../crates/approval-manager" }
policy-engine = { path = "../../crates/policy-engine" }
executor-core = { path = "../../crates/executor-core" }
bot-runtime = { path = "../../crates/bot-runtime" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
rustls = { version = "0.23", features = ["ring"] }
```
> ⚠️ `rustls` 버전은 twilight-http 0.17이 쓰는 transitive rustls와 **동일해야** `install_default`가 유효. `cargo tree -p twilight-http | grep rustls`로 확인해 버전 맞출 것. 불일치 시 live 실행에서 `Client::new` panic(컴파일은 무관). 대안: twilight-http의 crypto feature 사용.

- [ ] **Step 2: main.rs — fixture 테스트 먼저**

Create `tools/executor-smoke/src/main.rs` (테스트 포함, 전체):
```rust
use std::collections::BTreeMap;
use std::env;

use approval_manager::ApprovalRequest;
use bot_runtime::TwilightDiscordAdapter;
use desired_compiler::compile;
use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, Identity, ResourceKey,
    RoleIntent,
};
use diff_engine::{diff, InMemoryMatchResolver};
use discord_model::{ChannelType, Guild, GuildId, GuildState, Permissions, UserId};
use executor_core::{
    ApprovedExecutionRequest, DiscordAdapter, Executor, JobResult, RollbackAction, StepOutcome,
};
use operation_graph::compile_operations;
use policy_engine::Verdict;

fn fixture(guild_id: GuildId) -> ApprovedExecutionRequest {
    let verified = ResourceKey("starring-smoke-verified".to_string());
    let mut roles = BTreeMap::new();
    roles.insert(
        verified.clone(),
        AccessGrant { allow: vec![Capability::View], deny: vec![] },
    );
    let desired = DesiredState {
        roles: vec![RoleIntent {
            identity: Identity { key: verified, ..Default::default() },
            name: Some("starring-smoke-verified".to_string()),
            permissions: Some(Permissions::empty()),
        }],
        channels: vec![ChannelIntent {
            identity: Identity {
                key: ResourceKey("starring-smoke-channel".to_string()),
                ..Default::default()
            },
            name: Some("starring-smoke-channel".to_string()),
            channel_type: Some(ChannelType::Text),
            parent: None,
            access: Some(AccessIntent { everyone: None, roles }),
            raw_overwrites: None,
        }],
        ..Default::default()
    };
    let snapshot = GuildState {
        guild: Guild { id: guild_id, name: "smoke".to_string(), owner_id: UserId(1) },
        roles: vec![],
        channels: vec![],
        members: vec![],
    };
    let normalized = compile(&desired).unwrap();
    let diff_result = diff(&normalized, &InMemoryMatchResolver::new(&snapshot));
    let graph = compile_operations(&diff_result, &normalized).unwrap();
    ApprovedExecutionRequest {
        operation_graph: graph,
        normalized,
        approval: ApprovalRequest::new(Verdict::Allow, UserId(1)),
        snapshot,
        guild_id,
        requested_by: UserId(1),
        approved_by: vec![UserId(1)],
    }
}

async fn cleanup(adapter: &TwilightDiscordAdapter, guild: GuildId, result: &JobResult) {
    for step in result.steps.iter().rev() {
        if !matches!(step.outcome, StepOutcome::Success) {
            continue;
        }
        let Some(rollback) = &step.rollback else {
            continue;
        };
        let outcome = match rollback {
            RollbackAction::DeleteRole { id } => adapter.delete_role(guild, *id).await,
            RollbackAction::DeleteChannel { id } => adapter.delete_channel(guild, *id).await,
            other => {
                println!("  skip rollback (channel deletion covers overwrites): {other:?}");
                continue;
            }
        };
        match outcome {
            Ok(()) => println!("  rolled back: {rollback:?}"),
            Err(e) => println!("  rollback FAILED ({e:?}) — manual cleanup may be needed"),
        }
    }
}

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let token = match env::var("DISCORD_TEST_TOKEN") {
        Ok(t) => t,
        Err(_) => {
            eprintln!("set DISCORD_TEST_TOKEN and DISCORD_TEST_GUILD to run the smoke");
            return;
        }
    };
    let guild_raw = match env::var("DISCORD_TEST_GUILD").ok().and_then(|s| s.parse::<u64>().ok()) {
        Some(g) => g,
        None => {
            eprintln!("set DISCORD_TEST_GUILD to a numeric guild id");
            return;
        }
    };
    let guild_id = GuildId(guild_raw);

    let request = fixture(guild_id);
    let executor = Executor::new(TwilightDiscordAdapter::new(token));
    println!("executing smoke fixture against guild {guild_raw} ...");

    match executor.execute(&request).await {
        Ok(result) => {
            println!("job status: {:?}", result.status);
            for step in &result.steps {
                println!("  {:?}: {:?}", step.op_id, step.outcome);
            }
            println!("cleaning up (rollback, reverse order) ...");
            cleanup(executor.adapter(), guild_id, &result).await;
            println!("done. verify 'starring-smoke-*' resources are gone in the test guild.");
        }
        Err(e) => eprintln!("execution refused: {e:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_builds_three_creates_and_is_executable() {
        let request = fixture(GuildId(123));
        assert_eq!(request.operation_graph.nodes.len(), 3);
        assert!(request.approval.can_execute());
    }
}
```

- [ ] **Step 3: 게이트**
```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```
Expected: 전부 성공. executor-smoke는 **컴파일 + fixture 테스트**(실제 Discord 실행 아님). 총 테스트 실제 출력대로 보고.
> twilight/tokio/rustls가 처음 함께 컴파일됨. rustls provider API(`rustls::crypto::ring::default_provider().install_default()`)가 실제와 다르면 문서 보고 조정(컴파일 목적). 실제 provider 매칭은 live 실행 시 검증.

- [ ] **Step 4: 커밋 + push + 보고**
```bash
git add -A
git commit -m "feat(executor-smoke): add manual live Discord smoke tool with rollback cleanup"
git push origin main
```
보고에 **rustls/tokio 컴파일 조정 내용** + **fixture 테스트 결과** 명시.

---

## 완료 정의 (Definition of Done)
- [ ] `cargo build`/`test`/`clippy -D warnings`/`fmt --check` 통과 (tokio/rustls/twilight 함께 컴파일)
- [ ] executor-smoke: fixture(create-only, prefix starring-smoke-*) + run(Executor<TwilightDiscordAdapter>) + cleanup(rollback 역순, delete role/channel) + env 가드
- [ ] **fixture 단위 테스트**(3-op graph + can_execute) 통과. 실제 Discord 호출 없음(사용자 수동 실행)
- [ ] rustls provider install_default(버전 매칭은 live-run 검증) · env 없으면 안전 종료
- [ ] 주석 없음·**main push**. 편차(rustls/tokio API 조정) 보고

## 사용법 (사용자 수동 live 실행 — 12d 이후)
```bash
# throwaway 테스트 길드 + admin 봇 토큰 준비 후:
DISCORD_TEST_TOKEN=... DISCORD_TEST_GUILD=<guild_id> cargo run -p executor-smoke
# → role/channel/overwrite 생성 → JobResult 출력 → rollback으로 자가청소
# → 실패 시 'starring-smoke-*' 리소스 수동 삭제
```
