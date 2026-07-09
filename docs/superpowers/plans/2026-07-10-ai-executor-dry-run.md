# AI → Executor Dry-Run Bridge Plan (Phase 14)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. **Codex가 구현한다.** 끝에 보고. **완료 후 git push origin main.** 실제 Discord/토큰 없음.

**Goal:** `tools/starring-demo` — 자연어 프롬프트를 ai-gateway로 DesiredState화 → 전체 결정론 파이프라인 → executor-core MockDiscordAdapter 실행 → 각 단계 출력. **토큰 없는 CLI 데모.**

**Architecture:** Mock LLM 기본(직렬화 DesiredState 반환 → 결정론), `openai-client` feature + env면 Ollama. `run_dry(desired, guild)`가 compile→diff→graph→policy→virtual-apply→simulator→preview→approval→executor(Mock, block_on) 수행 후 DryRunReport 반환(테스트 가능). main이 AI 생성 + 출력.

**Tech Stack:** 전 파이프라인 crate + futures(block_on). tokio/실제 Discord 없음.

## Global Constraints
> ⚠️ **주석 금지**. **실제 Discord/토큰 금지.** MockAdapter로 실행. Mock LLM 기본.
- 게이트: build/test/clippy(-D warnings)/fmt. 완료 후 `git push origin main`.
- 금지: DB, NATS, web, app, live bot runtime, multi-turn, repair loop.

---

### Task 1: starring-demo 도구

**Files:**
- Modify: `Cargo.toml`
- Create: `tools/starring-demo/Cargo.toml`, `src/main.rs`

- [ ] **Step 1: 워크스페이스 + Cargo**

Root `Cargo.toml` members에 `"tools/starring-demo"` 추가.

Create `tools/starring-demo/Cargo.toml`:
```toml
[package]
name = "starring-demo"
version = "0.1.0"
edition.workspace = true

[dependencies]
ai-gateway = { path = "../../crates/ai-gateway" }
desired-state = { path = "../../crates/desired-state" }
desired-compiler = { path = "../../crates/desired-compiler" }
diff-engine = { path = "../../crates/diff-engine" }
operation-graph = { path = "../../crates/operation-graph" }
policy-engine = { path = "../../crates/policy-engine" }
virtual-apply = { path = "../../crates/virtual-apply" }
simulator = { path = "../../crates/simulator" }
preview = { path = "../../crates/preview" }
approval-manager = { path = "../../crates/approval-manager" }
executor-core = { path = "../../crates/executor-core" }
discord-model = { path = "../../crates/discord-model" }
futures = "0.3"
serde_json = { workspace = true }

[features]
openai-client = ["ai-gateway/openai-client"]
```

- [ ] **Step 2: main.rs 작성**

Create `tools/starring-demo/src/main.rs`:
```rust
use std::collections::BTreeMap;

use ai_gateway::{generate_desired_state, GenerateInput, LlmClient, MockLlmClient};
use approval_manager::ApprovalRequest;
use desired_compiler::compile;
use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, Identity, ResourceKey,
    RoleIntent,
};
use diff_engine::{diff, InMemoryMatchResolver};
use discord_model::{
    Channel, ChannelId, ChannelType, Guild, GuildId, GuildState, OverwriteTarget,
    PermissionOverwrite, Permissions, Role, RoleId, UserId,
};
use executor_core::{
    AdapterCall, ApprovedExecutionRequest, Executor, JobResult, MockDiscordAdapter,
};
use operation_graph::compile_operations;
use policy_engine::{PolicyDecision, PolicyEngine, Verdict};
use preview::{build_preview, PreviewModel};
use simulator::{access_matrix, SubjectSpec};
use virtual_apply::{apply, VirtualApplyResult};

struct DryRunReport {
    verdict: Verdict,
    preview: PreviewModel,
    executed: bool,
    job: Option<JobResult>,
    calls: Vec<AdapterCall>,
}

fn demo_guild() -> GuildState {
    GuildState {
        guild: Guild { id: GuildId(1), name: "demo".to_string(), owner_id: UserId(1) },
        roles: vec![Role {
            id: RoleId(1),
            name: "@everyone".to_string(),
            permissions: Permissions::VIEW_CHANNEL,
            position: 0,
            managed: false,
        }],
        channels: vec![Channel {
            id: ChannelId(500),
            name: "일반".to_string(),
            channel_type: ChannelType::Text,
            parent_id: None,
            position: 0,
            overwrites: vec![PermissionOverwrite {
                target: OverwriteTarget::Role(RoleId(1)),
                allow: Permissions::VIEW_CHANNEL,
                deny: Permissions::empty(),
            }],
        }],
        members: vec![],
    }
}

fn demo_desired() -> DesiredState {
    let verified = ResourceKey("verified".to_string());
    let mut roles = BTreeMap::new();
    roles.insert(
        verified.clone(),
        AccessGrant { allow: vec![Capability::View, Capability::Send], deny: vec![] },
    );
    DesiredState {
        roles: vec![RoleIntent {
            identity: Identity { key: verified, ..Default::default() },
            name: Some("인증됨".to_string()),
            permissions: Some(Permissions::empty()),
        }],
        channels: vec![ChannelIntent {
            identity: Identity { key: ResourceKey("general".to_string()), ..Default::default() },
            name: Some("일반".to_string()),
            channel_type: Some(ChannelType::Text),
            parent: None,
            access: Some(AccessIntent {
                everyone: Some(AccessGrant { allow: vec![], deny: vec![Capability::View] }),
                roles,
            }),
            raw_overwrites: None,
        }],
        ..Default::default()
    }
}

fn summarize(guild: &GuildState) -> String {
    let roles: Vec<&str> = guild.roles.iter().map(|r| r.name.as_str()).collect();
    let channels: Vec<&str> = guild.channels.iter().map(|c| c.name.as_str()).collect();
    format!("Roles: {}. Channels: {}.", roles.join(", "), channels.join(", "))
}

fn build_subjects(applied: &VirtualApplyResult) -> Vec<SubjectSpec> {
    let mut subjects = vec![SubjectSpec { name: "new_member".to_string(), roles: vec![] }];
    for (key, id) in &applied.synthetic_roles {
        subjects.push(SubjectSpec { name: format!("has:{}", key.0), roles: vec![*id] });
    }
    subjects
}

fn run_dry(desired: &DesiredState, guild: &GuildState) -> Result<DryRunReport, String> {
    desired.validate().map_err(|e| format!("validate: {e:?}"))?;
    let normalized = compile(desired).map_err(|e| format!("compile: {e:?}"))?;
    let resolver = InMemoryMatchResolver::new(guild);
    let diff_result = diff(&normalized, &resolver);
    if !diff_result.conflicts.is_empty() {
        return Err(format!("diff conflicts: {:?}", diff_result.conflicts));
    }
    let graph = compile_operations(&diff_result, &normalized).map_err(|e| format!("graph: {e}"))?;
    let decision: PolicyDecision = PolicyEngine::with_default_rules().evaluate(&graph);
    let applied = apply(guild, &graph, &normalized, &resolver).map_err(|e| format!("virtual-apply: {e:?}"))?;
    let subjects = build_subjects(&applied);
    let before_matrix = access_matrix(guild, &subjects);
    let after_matrix = access_matrix(&applied.after, &subjects);
    let preview = build_preview(
        "AI request",
        &diff_result,
        &graph,
        &decision,
        &applied,
        &before_matrix,
        &after_matrix,
    );

    let mut approval = ApprovalRequest::new(decision.verdict, UserId(1));
    for approver in [UserId(10), UserId(11)] {
        if approval.can_execute() {
            break;
        }
        let _ = approval.approve(approver);
    }

    if !approval.can_execute() {
        return Ok(DryRunReport {
            verdict: decision.verdict,
            preview,
            executed: false,
            job: None,
            calls: Vec::new(),
        });
    }

    let request = ApprovedExecutionRequest {
        operation_graph: graph,
        normalized,
        approval,
        snapshot: guild.clone(),
        guild_id: guild.guild.id,
        requested_by: UserId(1),
        approved_by: vec![UserId(10), UserId(11)],
    };
    let executor = Executor::new(MockDiscordAdapter::new());
    let job = futures::executor::block_on(executor.execute(&request))
        .map_err(|e| format!("executor: {e:?}"))?;
    let calls = executor.adapter().calls();
    Ok(DryRunReport {
        verdict: decision.verdict,
        preview,
        executed: true,
        job: Some(job),
        calls,
    })
}

fn print_report(report: &DryRunReport) {
    println!("=== policy verdict: {:?} ===", report.verdict);
    println!(
        "=== preview: {} changes, {} access changes, approval_required={} ===",
        report.preview.changes.len(),
        report.preview.access_changes.len(),
        report.preview.approval_required
    );
    for change in &report.preview.changes {
        println!("  change: {:?} {} [{:?}]", change.kind, change.target, change.severity);
    }
    for access in &report.preview.access_changes {
        println!(
            "  access: {} @ {}  view {}->{}  send {}->{}",
            access.subject,
            access.channel,
            access.before_can_view,
            access.after_can_view,
            access.before_can_send,
            access.after_can_send
        );
    }
    if report.executed {
        println!("=== executed on MockDiscordAdapter ===");
        if let Some(job) = &report.job {
            println!("  job status: {:?}", job.status);
        }
        println!("  adapter call sequence ({}):", report.calls.len());
        for call in &report.calls {
            println!("    {call:?}");
        }
    } else {
        println!("=== NOT executed (blocked / insufficient approval) — no changes ===");
    }
}

fn run_with_client<C: LlmClient>(client: &C, model: &str, prompt: &str, guild: &GuildState) {
    println!("=== prompt ===\n{prompt}\n");
    let input = GenerateInput {
        user_prompt: prompt.to_string(),
        guild_context_summary: summarize(guild),
    };
    let generated = match generate_desired_state(client, &input, model) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("ai-gateway error: {e}");
            return;
        }
    };
    println!("=== AI raw ({}) ===\n{}\n", generated.model, generated.raw_text);
    let desired = match generated.parsed {
        Some(d) => d,
        None => {
            eprintln!("parse failed: {}", generated.parse_error.unwrap_or_default());
            return;
        }
    };
    println!(
        "=== parsed DesiredState ===\n{}\n",
        serde_json::to_string_pretty(&desired).unwrap_or_default()
    );
    match run_dry(&desired, guild) {
        Ok(report) => print_report(&report),
        Err(stage_error) => eprintln!("pipeline stopped: {stage_error}"),
    }
}

fn default_prompt() -> String {
    "게임 커뮤니티 인증 구조. 신규 유저는 인증 채널만, 인증하면 일반 채널을 보고 쓸 수 있게.".to_string()
}

fn main() {
    let prompt = std::env::args().nth(1).unwrap_or_else(default_prompt);
    let guild = demo_guild();

    #[cfg(feature = "openai-client")]
    {
        if std::env::var("AI_BASE_URL").is_ok() {
            match ai_gateway::OpenAiCompatibleClient::from_env() {
                Ok(client) => {
                    let model = std::env::var("AI_MODEL").unwrap_or_else(|_| "local".to_string());
                    run_with_client(&client, &model, &prompt, &guild);
                    return;
                }
                Err(e) => eprintln!("openai client init failed: {e}; using mock"),
            }
        }
    }

    let mock = MockLlmClient::new(serde_json::to_string(&demo_desired()).unwrap());
    run_with_client(&mock, "mock", &prompt, &guild);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_desired_runs_through_executor() {
        let report = run_dry(&demo_desired(), &demo_guild()).unwrap();
        assert_eq!(report.verdict, Verdict::RequireApproval);
        assert!(report.executed);
        assert!(matches!(
            report.job.as_ref().unwrap().status,
            executor_core::JobStatus::Succeeded
        ));
        assert!(!report.preview.access_changes.is_empty());
    }

    #[test]
    fn admin_request_is_blocked_before_execution() {
        let admin = DesiredState {
            roles: vec![RoleIntent {
                identity: Identity { key: ResourceKey("admin".to_string()), ..Default::default() },
                name: Some("admin".to_string()),
                permissions: Some(Permissions::ADMINISTRATOR),
            }],
            ..Default::default()
        };
        let report = run_dry(&admin, &demo_guild()).unwrap();
        assert_eq!(report.verdict, Verdict::Deny);
        assert!(!report.executed);
        assert!(report.calls.is_empty());
    }
}
```

- [ ] **Step 3: 게이트 + 커밋 + push + 보고**
```bash
cargo build && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all -- --check
git add -A
git commit -m "feat(starring-demo): add AI-to-executor dry-run bridge (tokenless)"
git push origin main
```
> `openai-client` feature 컴파일도 확인: `cargo build -p starring-demo --features openai-client`. `OpenAiCompatibleClient::from_env()` 시그니처가 다르면 문서 보고 조정.

---

## 완료 정의 (Definition of Done)
- [ ] `cargo build`/`test`/`clippy -D warnings`/`fmt --check` 통과 (+ `--features openai-client` 빌드)
- [ ] starring-demo: NL→(Mock 기본/Ollama feature)→generate_desired_state→validate→compile→diff→graph→policy→virtual-apply→simulator→preview→approval→executor(Mock)→단계 출력
- [ ] **테스트**: demo_desired가 executor까지(RequireApproval→승인→Succeeded) / admin 요청은 Deny→미실행·콜0. 실제 Discord/토큰 없음
- [ ] 금지 항목(DB/NATS/web/live) 없음·주석 없음·main push. 편차(ai-gateway from_env 등) 보고

## 사용법
```bash
cargo run -p starring-demo                                  # Mock(결정론) — 프롬프트 무관 고정 구조
cargo run -p starring-demo -- "임의 자연어 요청"            # Mock은 프롬프트 무시(고정), 흐름 확인용
AI_BASE_URL=http://localhost:11434/v1 AI_API_KEY=ollama AI_MODEL=gemma4:e4b \
  cargo run -p starring-demo --features openai-client -- "게임 커뮤니티 만들어줘"   # 실제 LLM
```
