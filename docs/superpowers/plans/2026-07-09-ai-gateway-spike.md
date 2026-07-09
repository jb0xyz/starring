# AI Gateway Spike Implementation Plan (Phase 6)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. **Codex가 구현한다.** 이 phase는 **Mock 기반 결정론 빌드**만 구현한다(실제 모델 run은 사용자 수동). Task 끝에 보고.

**Goal:** `crates/ai-gateway`(순수: trait/mock/prompt/parse + `openai-client` feature) + `tools/ai-eval`(바이너리: NL fixture → 단계별 채점). **모델 없이 `cargo test` 통과.**

**Architecture:** `LlmClient` trait 뒤로 LLM 추상화. `generate_desired_state`=prompt build→complete→JSON parse(DesiredState). ai-eval이 parse→validate→compile→diff→graph 단계별 채점. reqwest는 feature 뒤.

**Tech Stack:** Rust edition 2021 stable, serde/serde_json, thiserror, (feature) reqwest blocking, 파이프라인 crate 전부(ai-eval).

## Global Constraints
> ⚠️ **주석 금지**: `//`, `///`, `//!` 없음.
- **기본 빌드/테스트는 reqwest 없이 순수.** reqwest는 `--features openai-client`에서만.
- 의존: `ai-gateway → desired-state`. `ai-eval → {ai-gateway, 파이프라인 5 crate}`. 역방향 금지.
- 완료 게이트: `cargo build`·`cargo test`·`cargo clippy --all-targets -- -D warnings`·`cargo fmt --all -- --check` (기본 features) + `cargo build -p ai-gateway --features openai-client`.
- Task별 커밋, Task 끝에 보고.

---

### Task 1: ai-gateway 코어 (순수)

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/ai-gateway/Cargo.toml`, `src/lib.rs`, `src/error.rs`, `src/client.rs`, `src/prompt.rs`, `src/generate.rs`

**Interfaces:**
- Produces: `LlmClient` trait, `MockLlmClient`, `GenerateInput`, `GeneratedDesiredState`, `AiGatewayError`, `generate_desired_state`, prompt/parse.

- [ ] **Step 1: 워크스페이스 + crate 파일**

Root `Cargo.toml` members에 `"crates/ai-gateway"` 추가.

Create `crates/ai-gateway/Cargo.toml`:
```toml
[package]
name = "ai-gateway"
version = "0.1.0"
edition.workspace = true

[features]
openai-client = ["dep:reqwest"]

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
desired-state = { path = "../desired-state" }
discord-model = { path = "../discord-model" }
reqwest = { version = "0.12", features = ["blocking", "json"], optional = true }
```

Create `crates/ai-gateway/src/lib.rs`:
```rust
pub mod client;
pub mod error;
pub mod generate;
pub mod prompt;

pub use client::{LlmClient, MockLlmClient};
pub use error::AiGatewayError;
pub use generate::{generate_desired_state, parse_desired_state, GenerateInput, GeneratedDesiredState};

#[cfg(feature = "openai-client")]
pub use client::OpenAiCompatibleClient;
```

Create `crates/ai-gateway/src/error.rs`:
```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AiGatewayError {
    #[error("llm request failed: {0}")]
    Request(String),
    #[error("empty response")]
    EmptyResponse,
}
```

- [ ] **Step 2: client 테스트 + 구현**

Create `crates/ai-gateway/src/client.rs`:
```rust
use crate::error::AiGatewayError;

pub trait LlmClient {
    fn complete(&self, system: &str, user: &str) -> Result<String, AiGatewayError>;
}

pub struct MockLlmClient {
    pub response: String,
}

impl MockLlmClient {
    pub fn new(response: impl Into<String>) -> Self {
        Self { response: response.into() }
    }
}

impl LlmClient for MockLlmClient {
    fn complete(&self, _system: &str, _user: &str) -> Result<String, AiGatewayError> {
        if self.response.is_empty() {
            Err(AiGatewayError::EmptyResponse)
        } else {
            Ok(self.response.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_returns_canned() {
        let c = MockLlmClient::new("hello");
        assert_eq!(c.complete("s", "u").unwrap(), "hello");
    }
}
```

- [ ] **Step 3: prompt 구현 (스키마가이드 + few-shot=실제 DesiredState serialize)**

Create `crates/ai-gateway/src/prompt.rs`:
```rust
use std::collections::BTreeMap;

use desired_state::{
    AccessGrant, AccessIntent, Capability, ChannelIntent, DesiredState, Identity, ResourceKey,
    ResourceState, RoleIntent,
};
use discord_model::{ChannelType, Permissions};

use crate::generate::GenerateInput;

pub fn build_system_prompt() -> String {
    let mut s = String::from(SCHEMA_GUIDE);
    s.push_str("\n\nExamples of valid DesiredState JSON:\n");
    for ds in example_desired_states() {
        s.push_str(&serde_json::to_string(&ds).unwrap());
        s.push('\n');
    }
    s.push_str("\nOutput ONLY the JSON document. No markdown fences, no explanation, no comments.");
    s
}

pub fn build_user_prompt(input: &GenerateInput) -> String {
    format!("Current server:\n{}\n\nRequest:\n{}", input.guild_context_summary, input.user_prompt)
}

fn example_desired_states() -> Vec<DesiredState> {
    let role = |key: &str, name: &str| RoleIntent {
        identity: Identity { key: ResourceKey(key.to_string()), ..Default::default() },
        name: Some(name.to_string()),
        permissions: Some(Permissions::empty()),
    };

    let vip = DesiredState { roles: vec![role("vip", "VIP")], ..Default::default() };

    let mut roles = BTreeMap::new();
    roles.insert(ResourceKey("verified".to_string()), AccessGrant { allow: vec![Capability::View, Capability::Send], deny: vec![] });
    let auth = DesiredState {
        roles: vec![role("verified", "Verified")],
        channels: vec![ChannelIntent {
            identity: Identity { key: ResourceKey("general".to_string()), ..Default::default() },
            name: Some("general".to_string()),
            channel_type: Some(ChannelType::Text),
            parent: None,
            access: Some(AccessIntent { everyone: Some(AccessGrant { allow: vec![], deny: vec![Capability::View] }), roles }),
            raw_overwrites: None,
        }],
        ..Default::default()
    };

    let delete = DesiredState {
        roles: vec![RoleIntent {
            identity: Identity { key: ResourceKey("vip".to_string()), state: ResourceState::Absent, ..Default::default() },
            name: Some("VIP".to_string()),
            permissions: None,
        }],
        ..Default::default()
    };

    vec![vip, auth, delete]
}

const SCHEMA_GUIDE: &str = "You output a DesiredState JSON for a Discord server.\nTop-level: \"mode\" (use \"patch\"), \"roles\" [], \"channels\" [], \"features\" [].\nRole: {\"key\":\"<logical id>\",\"name\":\"<name>\",\"permissions\":\"0\",\"match\":{\"by\":\"by_name\"},\"ownership\":\"managed\",\"state\":\"present\"}. To delete set \"state\":\"absent\".\nChannel: {\"key\":\"...\",\"name\":\"...\",\"channel_type\":\"text\",\"access\":{\"everyone\":{\"allow\":[],\"deny\":[\"view\"]},\"roles\":{\"<role key>\":{\"allow\":[\"view\",\"send\"],\"deny\":[]}}}}.\nCapabilities: view, send, read_history, add_reactions, attach_files, embed_links, manage_messages, connect, speak.\nReference roles by their key. Never grant administrator.";
```

(prompt.rs는 `discord_model::{ChannelType, Permissions}`를 쓴다 — discord-model 의존은 Step 1 Cargo.toml에 이미 포함됨.)

- [ ] **Step 4: generate 테스트 + 구현**

Create `crates/ai-gateway/src/generate.rs`:
```rust
use desired_state::DesiredState;

use crate::client::LlmClient;
use crate::error::AiGatewayError;
use crate::prompt::{build_system_prompt, build_user_prompt};

pub struct GenerateInput {
    pub user_prompt: String,
    pub guild_context_summary: String,
}

#[derive(Clone, Debug)]
pub struct GeneratedDesiredState {
    pub raw_text: String,
    pub parsed: Option<DesiredState>,
    pub parse_error: Option<String>,
    pub model: String,
}

pub fn generate_desired_state(client: &impl LlmClient, input: &GenerateInput, model: &str) -> Result<GeneratedDesiredState, AiGatewayError> {
    let system = build_system_prompt();
    let user = build_user_prompt(input);
    let raw_text = client.complete(&system, &user)?;
    let (parsed, parse_error) = match parse_desired_state(&raw_text) {
        Ok(ds) => (Some(ds), None),
        Err(e) => (None, Some(e)),
    };
    Ok(GeneratedDesiredState { raw_text, parsed, parse_error, model: model.to_string() })
}

pub fn parse_desired_state(raw: &str) -> Result<DesiredState, String> {
    let json = extract_json(raw);
    serde_json::from_str::<DesiredState>(json).map_err(|e| e.to_string())
}

fn extract_json(raw: &str) -> &str {
    let trimmed = raw.trim();
    match (trimmed.find('{'), trimmed.rfind('}')) {
        (Some(start), Some(end)) if end >= start => &trimmed[start..=end],
        _ => trimmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::MockLlmClient;

    #[test]
    fn valid_json_parses() {
        let ds = DesiredState::default();
        let json = serde_json::to_string(&ds).unwrap();
        let client = MockLlmClient::new(json);
        let g = generate_desired_state(&client, &GenerateInput { user_prompt: "x".to_string(), guild_context_summary: "y".to_string() }, "mock").unwrap();
        assert_eq!(g.parsed, Some(DesiredState::default()));
        assert!(g.parse_error.is_none());
    }

    #[test]
    fn non_json_reports_error() {
        let client = MockLlmClient::new("sorry I cannot");
        let g = generate_desired_state(&client, &GenerateInput { user_prompt: "x".to_string(), guild_context_summary: "y".to_string() }, "mock").unwrap();
        assert!(g.parsed.is_none());
        assert!(g.parse_error.is_some());
    }

    #[test]
    fn extracts_json_from_fences() {
        let ds = DesiredState::default();
        let inner = serde_json::to_string(&ds).unwrap();
        let fenced = format!("```json\n{inner}\n```");
        let client = MockLlmClient::new(fenced);
        let g = generate_desired_state(&client, &GenerateInput { user_prompt: "x".to_string(), guild_context_summary: "y".to_string() }, "mock").unwrap();
        assert_eq!(g.parsed, Some(DesiredState::default()));
    }

    #[test]
    fn system_prompt_has_guide_and_examples() {
        let p = build_system_prompt();
        assert!(p.contains("Capabilities"));
        assert!(p.contains("ONLY"));
        assert!(p.contains("verified"));
    }
}
```
> `DesiredState`가 `PartialEq`라 `assert_eq!` 가능. few-shot 예시가 실제 serialize라 `system_prompt_has_guide_and_examples`의 "verified"가 존재.

- [ ] **Step 5: 통과 + 커밋**
```bash
cargo test -p ai-gateway && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(ai-gateway): add LlmClient trait, prompt builder, and DesiredState generation"
```

- [ ] **Step 6: Task 보고**

---

### Task 2: openai-client feature

**Files:**
- Modify: `crates/ai-gateway/src/client.rs`

**Interfaces:**
- Produces: `OpenAiCompatibleClient`(feature `openai-client`), `from_env`/`new`, `LlmClient` impl. build_request_body(순수, 테스트).

- [ ] **Step 1: 테스트 + 구현 (feature 뒤)**

`crates/ai-gateway/src/client.rs` 끝(테스트 모듈 위)에 추가:
```rust
#[cfg(feature = "openai-client")]
pub struct OpenAiCompatibleClient {
    base_url: String,
    api_key: String,
    model: String,
    http: reqwest::blocking::Client,
}

#[cfg(feature = "openai-client")]
impl OpenAiCompatibleClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self { base_url: base_url.into(), api_key: api_key.into(), model: model.into(), http: reqwest::blocking::Client::new() }
    }

    pub fn from_env() -> Result<Self, AiGatewayError> {
        let base_url = std::env::var("AI_BASE_URL").map_err(|_| AiGatewayError::Request("AI_BASE_URL not set".to_string()))?;
        let model = std::env::var("AI_MODEL").map_err(|_| AiGatewayError::Request("AI_MODEL not set".to_string()))?;
        let api_key = std::env::var("AI_API_KEY").unwrap_or_default();
        Ok(Self::new(base_url, api_key, model))
    }

    pub fn model(&self) -> &str {
        &self.model
    }
}

#[cfg(feature = "openai-client")]
fn build_request_body(model: &str, system: &str, user: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ],
        "temperature": 0.2,
        "stream": false
    })
}

#[cfg(feature = "openai-client")]
impl LlmClient for OpenAiCompatibleClient {
    fn complete(&self, system: &str, user: &str) -> Result<String, AiGatewayError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = build_request_body(&self.model, system, user);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .map_err(|e| AiGatewayError::Request(e.to_string()))?;
        let value: serde_json::Value = resp.json().map_err(|e| AiGatewayError::Request(e.to_string()))?;
        value["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or(AiGatewayError::EmptyResponse)
    }
}

#[cfg(all(test, feature = "openai-client"))]
mod openai_tests {
    use super::*;

    #[test]
    fn request_body_shape() {
        let b = build_request_body("m", "sys", "usr");
        assert_eq!(b["model"], "m");
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["content"], "usr");
        assert_eq!(b["stream"], false);
    }
}
```

- [ ] **Step 2: 검증 (feature 켜고/끄고 둘 다)**
```bash
cargo test -p ai-gateway
cargo test -p ai-gateway --features openai-client
cargo clippy -p ai-gateway --all-targets --features openai-client -- -D warnings
cargo fmt --all -- --check
```
Expected: 기본 테스트(reqwest 없이) 통과 + feature 테스트(request_body_shape 포함) 통과.

- [ ] **Step 3: 커밋 + 보고**
```bash
git add -A
git commit -m "feat(ai-gateway): add openai-compatible client behind feature flag"
```

---

### Task 3: tools/ai-eval + README + 최종 게이트

**Files:**
- Modify: `Cargo.toml`
- Create: `tools/ai-eval/Cargo.toml`, `src/lib.rs`, `src/main.rs`, `README.md`

**Interfaces:**
- Produces: `EvalStage`, `EvalFixture`, `FixtureResult`, `EvaluationReport`, `evaluate`, `fixtures`, `main`(실제 run, feature).

- [ ] **Step 1: 워크스페이스 + crate 파일**

Root `Cargo.toml` members에 `"tools/ai-eval"` 추가.

Create `tools/ai-eval/Cargo.toml`:
```toml
[package]
name = "ai-eval"
version = "0.1.0"
edition.workspace = true

[features]
openai-client = ["ai-gateway/openai-client"]

[dependencies]
ai-gateway = { path = "../../crates/ai-gateway", default-features = false }
desired-state = { path = "../../crates/desired-state" }
desired-compiler = { path = "../../crates/desired-compiler" }
diff-engine = { path = "../../crates/diff-engine" }
operation-graph = { path = "../../crates/operation-graph" }
discord-model = { path = "../../crates/discord-model" }
```

- [ ] **Step 2: lib 테스트 + 구현**

Create `tools/ai-eval/src/lib.rs`:
```rust
use ai_gateway::{generate_desired_state, GenerateInput, LlmClient};
use desired_compiler::compile;
use diff_engine::{diff, InMemoryMatchResolver};
use discord_model::GuildState;
use operation_graph::compile_operations;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvalStage {
    ParseFailed,
    Parsed,
    Validated,
    Compiled,
    Diffed,
    Graphed,
}

pub struct EvalFixture {
    pub name: String,
    pub user_prompt: String,
    pub guild: GuildState,
}

pub struct FixtureResult {
    pub name: String,
    pub reached: EvalStage,
    pub failure: Option<String>,
}

pub struct EvaluationReport {
    pub results: Vec<FixtureResult>,
}

pub fn evaluate(client: &impl LlmClient, model: &str, fixtures: &[EvalFixture]) -> EvaluationReport {
    EvaluationReport { results: fixtures.iter().map(|f| evaluate_one(client, model, f)).collect() }
}

fn evaluate_one(client: &impl LlmClient, model: &str, fixture: &EvalFixture) -> FixtureResult {
    let name = fixture.name.clone();
    let input = GenerateInput { user_prompt: fixture.user_prompt.clone(), guild_context_summary: summarize(&fixture.guild) };
    let generated = match generate_desired_state(client, &input, model) {
        Ok(g) => g,
        Err(e) => return FixtureResult { name, reached: EvalStage::ParseFailed, failure: Some(e.to_string()) },
    };
    let desired = match generated.parsed {
        Some(d) => d,
        None => return FixtureResult { name, reached: EvalStage::ParseFailed, failure: generated.parse_error },
    };
    if let Err(errs) = desired.validate() {
        return FixtureResult { name, reached: EvalStage::Parsed, failure: Some(format!("{errs:?}")) };
    }
    let normalized = match compile(&desired) {
        Ok(n) => n,
        Err(errs) => return FixtureResult { name, reached: EvalStage::Validated, failure: Some(format!("{errs:?}")) },
    };
    let diff_result = diff(&normalized, &InMemoryMatchResolver::new(&fixture.guild));
    if !diff_result.conflicts.is_empty() {
        return FixtureResult { name, reached: EvalStage::Compiled, failure: Some(format!("{:?}", diff_result.conflicts)) };
    }
    match compile_operations(&diff_result, &normalized) {
        Ok(_) => FixtureResult { name, reached: EvalStage::Graphed, failure: None },
        Err(e) => FixtureResult { name, reached: EvalStage::Diffed, failure: Some(e.to_string()) },
    }
}

fn summarize(guild: &GuildState) -> String {
    let roles: Vec<&str> = guild.roles.iter().map(|r| r.name.as_str()).collect();
    let channels: Vec<&str> = guild.channels.iter().map(|c| c.name.as_str()).collect();
    format!("Roles: {}. Channels: {}.", roles.join(", "), channels.join(", "))
}

impl EvaluationReport {
    pub fn render(&self) -> String {
        let mut out = String::new();
        for r in &self.results {
            out.push_str(&format!("{:<30} {:?}", r.name, r.reached));
            if let Some(f) = &r.failure {
                out.push_str(&format!("  ({f})"));
            }
            out.push('\n');
        }
        let total = self.results.len().max(1);
        let graphed = self.results.iter().filter(|r| r.reached == EvalStage::Graphed).count();
        out.push_str(&format!("\ngraphed: {}/{} ({}%)\n", graphed, self.results.len(), graphed * 100 / total));
        out
    }
}

pub fn fixtures() -> Vec<EvalFixture> {
    let empty = || GuildState { guild: discord_model::Guild { id: discord_model::GuildId(1), name: "srv".to_string(), owner_id: discord_model::UserId(1) }, roles: vec![], channels: vec![], members: vec![] };
    vec![
        EvalFixture { name: "create-vip-role".to_string(), user_prompt: "Create a VIP role.".to_string(), guild: empty() },
        EvalFixture { name: "verify-gate-general".to_string(), user_prompt: "Add a Verified role and make the general channel visible only to verified members.".to_string(), guild: empty() },
        EvalFixture { name: "delete-vip".to_string(), user_prompt: "Delete the VIP role.".to_string(), guild: empty() },
        EvalFixture { name: "no-admin".to_string(), user_prompt: "Give everyone administrator permission.".to_string(), guild: empty() },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_gateway::MockLlmClient;
    use desired_state::{Identity, ResourceKey, RoleIntent, DesiredState};
    use discord_model::Permissions;

    #[test]
    fn valid_desired_reaches_graph() {
        let ds = DesiredState { roles: vec![RoleIntent { identity: Identity { key: ResourceKey("vip".to_string()), ..Default::default() }, name: Some("VIP".to_string()), permissions: Some(Permissions::empty()) }], ..Default::default() };
        let client = MockLlmClient::new(serde_json::to_string(&ds).unwrap());
        let report = evaluate(&client, "mock", &fixtures()[..1]);
        assert_eq!(report.results[0].reached, EvalStage::Graphed);
    }

    #[test]
    fn garbage_reaches_parse_failed() {
        let client = MockLlmClient::new("no json here");
        let report = evaluate(&client, "mock", &fixtures()[..1]);
        assert_eq!(report.results[0].reached, EvalStage::ParseFailed);
    }
}
```

Create `tools/ai-eval/src/main.rs`:
```rust
fn main() {
    let fixtures = ai_eval::fixtures();

    #[cfg(feature = "openai-client")]
    {
        match ai_gateway::OpenAiCompatibleClient::from_env() {
            Ok(client) => {
                let model = client.model().to_string();
                let report = ai_eval::evaluate(&client, &model, &fixtures);
                print!("{}", report.render());
                return;
            }
            Err(e) => {
                eprintln!("no endpoint: {e}");
                std::process::exit(1);
            }
        }
    }

    #[cfg(not(feature = "openai-client"))]
    {
        let _ = fixtures;
        eprintln!("ai-eval: rebuild with --features openai-client and set AI_BASE_URL/AI_MODEL to run against a model.");
        std::process::exit(1);
    }
}
```
> main.rs가 `ai_gateway`를 직접 참조하므로 `tools/ai-eval/Cargo.toml`의 dependencies에는 `ai-gateway`가 이미 있음(default-features=false, feature는 openai-client로 전달). `#[cfg(feature="openai-client")]` 분기에서만 `OpenAiCompatibleClient` 사용.

Create `tools/ai-eval/README.md`:
```markdown
# ai-eval

소형 LLM이 유효한 DesiredState를 만드는지 검증하는 하네스.

## 기본 테스트 (Mock, 결정론)
    cargo test -p ai-eval

## 실제 모델 run (Ollama gemma4:e4b)
    AI_BASE_URL=http://localhost:11434/v1 AI_API_KEY=ollama AI_MODEL=gemma4:e4b \
      cargo run -p ai-eval --features openai-client

각 fixture가 parse/validate/compile/diff/graph 중 어디까지 도달하는지 리포트한다.
graphed 비율이 높을수록 소형 모델이 파이프라인을 끝까지 통과시킨 것.
```

- [ ] **Step 3: 검증**
```bash
cargo test -p ai-eval
cargo clippy -p ai-eval --all-targets -- -D warnings
```

- [ ] **Step 4: 최종 게이트 (전체)**
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build
cargo build -p ai-gateway --features openai-client
```
Expected: 기본 전부 통과 + openai-client feature도 컴파일. 총 테스트 실제 출력대로 보고.

- [ ] **Step 5: 커밋 + 보고**
```bash
git add -A
git commit -m "feat(ai-eval): add evaluation harness and fixtures"
```

---

## 완료 정의 (Definition of Done)
- [ ] 기본 `cargo build`/`test`/`clippy -D warnings`/`fmt --check` 통과 (reqwest 없이)
- [ ] `cargo build -p ai-gateway --features openai-client` 컴파일
- [ ] ai-gateway: LlmClient/MockLlmClient/generate_desired_state/prompt(few-shot=실제 serialize)/parse
- [ ] openai-client feature: OpenAiCompatibleClient(env, reqwest)
- [ ] ai-eval: 단계별 채점(parse→validate→compile→diff→graph), fixture, EvaluationReport, main(실제 run)
- [ ] Mock 결정론 테스트: valid→Graphed, garbage→ParseFailed
- [ ] README에 실제 run 방법(Ollama gemma4:e4b)
- [ ] 의존 방향·주석 없음·Task별 커밋
