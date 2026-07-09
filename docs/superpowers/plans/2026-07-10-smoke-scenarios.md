# Executor Smoke — 시나리오 러너 확장 Plan (Phase 12e)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. **Codex가 구현한다.** 끝에 보고. **완료 후 git push origin main.** 실제 Discord 호출은 안 함(사용자/Claude가 수동 실행).

**Goal:** `tools/executor-smoke`를 **named 시나리오 러너**로 확장 — 정책 평가 + 자동 승인 흐름 포함. `cargo run -p executor-smoke -- <scenario>`.

**Architecture:** 시나리오별 DesiredState → compile → diff → graph → **PolicyEngine 평가** → verdict/findings 출력 → ApprovalRequest 자동 승인(Deny/부족이면 실행 거부) → 승인된 것만 Executor 실행 → JobResult → rollback 청소. 기존 12d 구조 확장.

**Tech Stack:** 기존 executor-smoke deps 그대로(policy-engine 이미 있음). Cargo 변경 없음.

## Global Constraints
> ⚠️ **주석 금지**. 실제 Discord 호출은 코드가 안 함(env로 러너 동작, 실행은 사용자). 검증=컴파일+fixture/scenario 순수 테스트.
- 게이트: build/test/clippy(-D warnings)/fmt. 완료 후 `git push origin main`.

---

### Task 1: 시나리오 + 정책/승인 러너

**Files:**
- Modify: `tools/executor-smoke/src/main.rs`

**Interfaces:**
- Produces: `scenario(&str) -> DesiredState`, `game_community()`, `no_admin()`, `simple()`, 정책/승인 러너.

- [ ] **Step 1: import 추가**

`main.rs` 상단 use에 추가:
```rust
use policy_engine::{PolicyDecision, PolicyEngine};
```
(기존 `use policy_engine::Verdict;`는 유지하거나 위 라인으로 합침. Verdict는 더 이상 직접 안 써도 됨 — approval은 decision.verdict 사용.)

- [ ] **Step 2: 시나리오 함수 3개 (기존 fixture → simple로 이름 변경 + 신규 2개)**

`main.rs`의 기존 `fixture(guild_id)` 함수를 아래로 교체(시나리오 3개 + DesiredState만 반환하는 헬퍼로 분리):
```rust
fn simple() -> DesiredState {
    let verified = ResourceKey("smoke-verified".to_string());
    let mut roles = BTreeMap::new();
    roles.insert(
        verified.clone(),
        AccessGrant { allow: vec![Capability::View], deny: vec![] },
    );
    DesiredState {
        roles: vec![RoleIntent {
            identity: Identity { key: verified.clone(), ..Default::default() },
            name: Some("starring-smoke-verified".to_string()),
            permissions: Some(Permissions::empty()),
        }],
        channels: vec![ChannelIntent {
            identity: Identity { key: ResourceKey("smoke-channel".to_string()), ..Default::default() },
            name: Some("starring-smoke-channel".to_string()),
            channel_type: Some(ChannelType::Text),
            parent: None,
            access: Some(AccessIntent { everyone: None, roles }),
            raw_overwrites: None,
        }],
        ..Default::default()
    }
}

fn game_community() -> DesiredState {
    let verified = ResourceKey("smoke-verified".to_string());
    let view_send = || {
        let mut roles = BTreeMap::new();
        roles.insert(
            verified.clone(),
            AccessGrant { allow: vec![Capability::View, Capability::Send], deny: vec![] },
        );
        AccessIntent {
            everyone: Some(AccessGrant { allow: vec![], deny: vec![Capability::View] }),
            roles,
        }
    };
    let view_only = || {
        let mut roles = BTreeMap::new();
        roles.insert(
            verified.clone(),
            AccessGrant { allow: vec![Capability::View], deny: vec![] },
        );
        AccessIntent {
            everyone: Some(AccessGrant { allow: vec![], deny: vec![Capability::View] }),
            roles,
        }
    };
    let channel = |key: &str, name: &str, access: Option<AccessIntent>| ChannelIntent {
        identity: Identity { key: ResourceKey(key.to_string()), ..Default::default() },
        name: Some(name.to_string()),
        channel_type: Some(ChannelType::Text),
        parent: None,
        access,
        raw_overwrites: None,
    };
    DesiredState {
        roles: vec![RoleIntent {
            identity: Identity { key: verified.clone(), ..Default::default() },
            name: Some("starring-smoke-인증됨".to_string()),
            permissions: Some(Permissions::empty()),
        }],
        channels: vec![
            channel("smoke-rules", "starring-smoke-규칙", None),
            channel("smoke-verify", "starring-smoke-인증", None),
            channel("smoke-general", "starring-smoke-일반", Some(view_send())),
            channel("smoke-questions", "starring-smoke-질문", Some(view_send())),
            channel("smoke-party", "starring-smoke-파티모집", Some(view_send())),
            channel("smoke-notice", "starring-smoke-공지", Some(view_only())),
        ],
        ..Default::default()
    }
}

fn no_admin() -> DesiredState {
    DesiredState {
        roles: vec![RoleIntent {
            identity: Identity { key: ResourceKey("smoke-admin".to_string()), ..Default::default() },
            name: Some("starring-smoke-admin".to_string()),
            permissions: Some(Permissions::ADMINISTRATOR),
        }],
        ..Default::default()
    }
}

fn scenario(name: &str) -> DesiredState {
    match name {
        "game-community" => game_community(),
        "no-admin" => no_admin(),
        _ => simple(),
    }
}

fn minimal_snapshot(guild_id: GuildId) -> GuildState {
    GuildState {
        guild: Guild { id: guild_id, name: "smoke".to_string(), owner_id: UserId(1) },
        roles: vec![],
        channels: vec![],
        members: vec![],
    }
}
```

- [ ] **Step 3: main() 교체 — 정책/승인 러너**

`main.rs`의 `#[tokio::main] async fn main()`을 아래로 교체:
```rust
#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let scenario_name = std::env::args().nth(1).unwrap_or_else(|| "simple".to_string());

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

    let desired = scenario(&scenario_name);
    let snapshot = minimal_snapshot(guild_id);
    let normalized = match compile(&desired) {
        Ok(n) => n,
        Err(errors) => {
            eprintln!("compile failed: {errors:?}");
            return;
        }
    };
    let diff_result = diff(&normalized, &InMemoryMatchResolver::new(&snapshot));
    let graph = match compile_operations(&diff_result, &normalized) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("operation graph failed: {e:?}");
            return;
        }
    };

    let decision: PolicyDecision = PolicyEngine::with_default_rules().evaluate(&graph);
    println!("scenario '{scenario_name}': {} operations", graph.nodes.len());
    println!("policy verdict: {:?}", decision.verdict);
    for finding in &decision.findings {
        println!("  finding [{}] {} — {}", finding.rule_id, finding.target, finding.message);
    }

    let mut approval = ApprovalRequest::new(decision.verdict, UserId(1));
    for approver in [UserId(10), UserId(11)] {
        if approval.can_execute() {
            break;
        }
        let _ = approval.approve(approver);
    }

    if !approval.can_execute() {
        println!(
            "NOT executing — approval state {:?}. no Discord change made.",
            approval.state()
        );
        return;
    }

    let request = ApprovedExecutionRequest {
        operation_graph: graph,
        normalized,
        approval,
        snapshot,
        guild_id,
        requested_by: UserId(1),
        approved_by: vec![UserId(10), UserId(11)],
    };

    let executor = Executor::new(TwilightDiscordAdapter::new(token));
    println!("executing against guild {guild_raw} ...");
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
```

- [ ] **Step 4: 테스트 갱신** — 기존 `fixture` 참조 테스트를 scenario 기반으로:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_builds_two_creates() {
        let desired = simple();
        assert_eq!(desired.roles.len(), 1);
        assert_eq!(desired.channels.len(), 1);
    }

    #[test]
    fn game_community_has_role_and_six_channels() {
        let desired = game_community();
        assert_eq!(desired.roles.len(), 1);
        assert_eq!(desired.channels.len(), 6);
    }

    #[test]
    fn no_admin_grants_administrator() {
        let desired = no_admin();
        assert_eq!(desired.roles[0].permissions, Some(Permissions::ADMINISTRATOR));
    }
}
```

- [ ] **Step 5: 게이트 + 커밋 + push + 보고**
```bash
cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check
git add -A
git commit -m "feat(executor-smoke): add named scenarios with policy and approval flow"
git push origin main
```
보고: 커밋 해시, 테스트 수, 게이트, 편차.

---

## 완료 정의 (Definition of Done)
- [ ] `cargo build`/`test`/`clippy -D warnings`/`fmt --check` 통과
- [ ] scenario(simple/game-community/no-admin) + 정책 평가 + 자동승인 + 실행/거부 러너
- [ ] no-admin은 정책 Deny → Blocked → 실행 안 함(no Discord change), game-community는 RequireApproval → 자동승인 → 실행
- [ ] scenario DesiredState 순수 테스트(역할/채널 수, admin perms) 통과. 실제 Discord 호출 없음(수동 실행)
- [ ] 주석 없음·main push
