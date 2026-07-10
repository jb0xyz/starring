# Phase 16b — Layer 2 Live Smoke (Gateway) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 실제 Discord 버튼 클릭 → Gateway로 수신 → automation-core(무수정) 해석 → 실제 역할 지급 + ephemeral 응답. Layer 2를 live로 증명.

**Architecture:** Layer 1의 `bot-runtime`과 대칭인 신규 `automation-runtime`(Layer 2 live edge) + 얇은 `tools/interaction-smoke`(수동 runner). automation-runtime = custom_id 변환 + InteractionCreate→RuntimeEvent 변환 + per-interaction responder + mutation adapter + gateway 루프. automation-state/automation-core는 **한 글자도 안 건드린다**.

**Tech Stack:** Rust(edition 2021), twilight-gateway/http/model 0.17, tokio 1, rustls 0.23(ring). 순수 테스트=custom_id+no_ai_gateway. 실제 클릭은 사용자 수동.

## Global Constraints

- **코드 주석 절대 금지** (`//`,`///`,`//!`). **Codex가 구현**(코드 블록 그대로).
- **automation-core / automation-state 수정 금지** — seam 구현만.
- **automation-runtime은 ai-gateway/LLM 의존 금지** — `tests/no_ai_gateway.rs`로 강제(런타임에 event-time AI 불가).
- **twilight = 0.17** (bot-runtime과 버전 일치). ID는 `Id::new(u64)` / `id.get() -> u64`. discord-model 뉴타입은 `X(pub u64)`.
- **비동기**: `#[allow(async_fn_in_trait)]`는 automation-core trait에 이미 있음. automation-runtime lib은 `.await`만(직접 tokio 의존 없음). tool만 `#[tokio::main]` + rustls provider 설치.
- **member 등록 순서**: member 경로에 manifest가 있어야 cargo가 로드됨 → 크레이트/툴은 자기 Cargo.toml과 함께 등록(16a 교훈).
- **완료 게이트**: `cargo build` / `cargo test`(토큰 없이 green) / `cargo clippy --all-targets -- -D warnings` / `cargo fmt --all -- --check`. 완료 후 `git push origin main`.
- **live/토큰 없음(자동)** — 실제 클릭은 사용자가 env로 수동. modal/dynamic/DB/webhook/retry 없음.

---

## Grounded twilight 0.17 API (실물 대조 완료 — 이대로 쓰면 컴파일됨)

- `twilight_http::Client::new(token: String) -> Client`
- `Client::add_guild_member_role(Id<GuildMarker>, Id<UserMarker>, Id<RoleMarker>)` → await → `Result<Response<EmptyBody>, twilight_http::Error>`
- `Client::interaction(Id<ApplicationMarker>) -> InteractionClient`
- `InteractionClient::create_response(Id<InteractionMarker>, &str, &InteractionResponse)` → await → Result<_, Error>
- `Client::create_message(Id<ChannelMarker>) -> CreateMessage`; `.content(&str)`, `.components(&[Component])`, await
- `twilight_http::error::ErrorType::{Response{status,..}, RequestTimedOut, RequestError, Unauthorized}`; `status.get() -> u16`
- `twilight_model::id::Id::new(u64)`, `Id::get() -> u64`, markers `id::marker::{GuildMarker, UserMarker, RoleMarker, ChannelMarker, InteractionMarker, ApplicationMarker}`
- `Interaction { id: Id<InteractionMarker>, application_id: Id<ApplicationMarker>, token: String, guild_id: Option<Id<GuildMarker>>, member: Option<PartialMember>, user: Option<User>, data: Option<InteractionData> }`
- `InteractionData::MessageComponent(Box<MessageComponentInteractionData>)`; `.custom_id: String`
- `PartialMember.user: Option<User>`; `User.id: Id<UserMarker>`
- `twilight_model::http::interaction::{InteractionResponse{kind, data:Option<InteractionResponseData>}, InteractionResponseType::ChannelMessageWithSource, InteractionResponseData(Default 파생, content:Option<String>, flags:Option<MessageFlags>, ..)}`
- `twilight_model::channel::message::MessageFlags::EPHEMERAL`
- `twilight_model::channel::message::component::{Component::{ActionRow(ActionRow), Button(Button)}, ActionRow{id:Option<i32>, components:Vec<Component>}, Button{id:Option<i32>, custom_id:Option<String>, disabled:bool, emoji:Option<EmojiReactionType>, label:Option<String>, style:ButtonStyle, url:Option<String>, sku_id:Option<Id<SkuMarker>>}, ButtonStyle::Primary}`
- `twilight_gateway::{Shard, ShardId, Intents, Event, EventTypeFlags, StreamExt}`; `Shard::new(ShardId::ONE, token: String, Intents::empty())`; `use StreamExt; shard.next_event(EventTypeFlags::INTERACTION_CREATE).await -> Option<Result<Event, _>>`
- `Event::InteractionCreate(Box<InteractionCreate>)`; `InteractionCreate(pub Interaction)` → `.0` is Interaction
- rustls: `let _ = rustls::crypto::ring::default_provider().install_default();` (tool main 최상단)

---

## File Structure

**신규 크레이트 `automation-runtime`:**
- `crates/automation-runtime/Cargo.toml`
- `crates/automation-runtime/src/lib.rs`
- `crates/automation-runtime/src/custom_id.rs` — encode/decode (순수)
- `crates/automation-runtime/src/error.rs` — classify_error (twilight Error → automation_core::AdapterError)
- `crates/automation-runtime/src/convert.rs` — InteractionCreate → RuntimeEvent
- `crates/automation-runtime/src/mutation.rs` — TwilightMutationAdapter
- `crates/automation-runtime/src/responder.rs` — TwilightInteractionResponder (per-interaction)
- `crates/automation-runtime/src/gateway.rs` — shard 루프
- `crates/automation-runtime/src/runner.rs` — per-interaction 배선
- `crates/automation-runtime/tests/no_ai_gateway.rs`

**신규 툴 `tools/interaction-smoke`:**
- `tools/interaction-smoke/Cargo.toml`
- `tools/interaction-smoke/src/main.rs`

**수정:** `Cargo.toml`(workspace members) — 두 항목 추가.

---

## Task 1: automation-runtime 스캐폴드 + custom_id + error + 가드 (순수 TDD)

**Files:**
- Modify: `Cargo.toml` (workspace members — automation-runtime)
- Create: `crates/automation-runtime/Cargo.toml`
- Create: `crates/automation-runtime/src/lib.rs`
- Create: `crates/automation-runtime/src/custom_id.rs`
- Create: `crates/automation-runtime/src/error.rs`
- Create: `crates/automation-runtime/tests/no_ai_gateway.rs`

**Interfaces:**
- Produces: `custom_id::{encode(GuildId, &str, &str) -> String, decode(&str) -> Result<ParsedCustomId, CustomIdError>, ParsedCustomId{guild_id, ruleset_key, button_key}, CustomIdError}`, `error::classify_error(&twilight_http::Error) -> automation_core::AdapterError`.

- [ ] **Step 1: workspace members에 automation-runtime 등록 + Cargo.toml 작성**

root `Cargo.toml` `members`의 `"crates/automation-core",` 다음 줄에 `"crates/automation-runtime",` 추가. 이어서:

```toml
[package]
name = "automation-runtime"
version = "0.1.0"
edition.workspace = true

[dependencies]
automation-state = { path = "../automation-state" }
automation-core = { path = "../automation-core" }
discord-model = { path = "../discord-model" }
resource-resolution = { path = "../resource-resolution" }
twilight-gateway = "0.17"
twilight-http = "0.17"
twilight-model = "0.17"
```

- [ ] **Step 2: 실패 테스트 작성 — `custom_id.rs` 하단 `#[cfg(test)]`** (먼저 타입/함수 없이 테스트만 두면 컴파일 실패)

`crates/automation-runtime/src/custom_id.rs`에 아래 테스트 모듈을 포함해 작성한다(구현과 함께):

```rust
use discord_model::GuildId;

const PREFIX: &str = "starring";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedCustomId {
    pub guild_id: GuildId,
    pub ruleset_key: String,
    pub button_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CustomIdError {
    WrongPrefix,
    WrongShape,
    BadGuildId,
}

pub fn encode(guild_id: GuildId, ruleset_key: &str, button_key: &str) -> String {
    format!("{PREFIX}:{}:{}:{}", guild_id.0, ruleset_key, button_key)
}

pub fn decode(custom_id: &str) -> Result<ParsedCustomId, CustomIdError> {
    let parts: Vec<&str> = custom_id.split(':').collect();
    if parts.len() != 4 {
        return Err(CustomIdError::WrongShape);
    }
    if parts[0] != PREFIX {
        return Err(CustomIdError::WrongPrefix);
    }
    let guild_id = parts[1]
        .parse::<u64>()
        .map(GuildId)
        .map_err(|_| CustomIdError::BadGuildId)?;
    Ok(ParsedCustomId {
        guild_id,
        ruleset_key: parts[2].to_string(),
        button_key: parts[3].to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let encoded = encode(GuildId(123456789), "demo_verify", "verify_button");
        assert_eq!(encoded, "starring:123456789:demo_verify:verify_button");
        assert_eq!(
            decode(&encoded).unwrap(),
            ParsedCustomId {
                guild_id: GuildId(123456789),
                ruleset_key: "demo_verify".to_string(),
                button_key: "verify_button".to_string(),
            }
        );
    }

    #[test]
    fn decode_rejects_wrong_prefix() {
        assert_eq!(
            decode("nope:1:rs:btn").unwrap_err(),
            CustomIdError::WrongPrefix
        );
    }

    #[test]
    fn decode_rejects_wrong_shape() {
        assert_eq!(decode("starring:1:rs").unwrap_err(), CustomIdError::WrongShape);
    }

    #[test]
    fn decode_rejects_bad_guild_id() {
        assert_eq!(
            decode("starring:abc:rs:btn").unwrap_err(),
            CustomIdError::BadGuildId
        );
    }
}
```

- [ ] **Step 3: `crates/automation-runtime/src/error.rs` 작성**

```rust
use automation_core::{AdapterError, AdapterErrorKind};
use twilight_http::error::ErrorType;

pub fn classify_error(err: &twilight_http::Error) -> AdapterError {
    let kind = match err.kind() {
        ErrorType::Response { status, .. } => match status.get() {
            429 => AdapterErrorKind::RateLimited,
            401 | 403 => AdapterErrorKind::Forbidden,
            404 => AdapterErrorKind::NotFound,
            _ => AdapterErrorKind::Unknown,
        },
        ErrorType::RequestTimedOut | ErrorType::RequestError => AdapterErrorKind::Network,
        ErrorType::Unauthorized => AdapterErrorKind::Forbidden,
        _ => AdapterErrorKind::Unknown,
    };
    AdapterError::new(kind, format!("twilight error: {err}"))
}
```

- [ ] **Step 4: `crates/automation-runtime/src/lib.rs` 작성** (이 Task 범위 모듈만)

```rust
pub mod custom_id;
pub mod error;

pub use custom_id::{decode, encode, CustomIdError, ParsedCustomId};
pub use error::classify_error;
```

- [ ] **Step 5: `crates/automation-runtime/tests/no_ai_gateway.rs` 작성**

```rust
#[test]
fn manifest_does_not_depend_on_ai_gateway() {
    let manifest = include_str!("../Cargo.toml");
    assert!(!manifest.contains("ai-gateway"));
    assert!(!manifest.contains("ai_gateway"));
    assert!(!manifest.contains("llm"));
}
```

- [ ] **Step 6: 테스트 실행**

Run: `cargo test -p automation-runtime`
Expected: PASS (custom_id 4 + no_ai_gateway 1 = 5 tests).

- [ ] **Step 7: 커밋**

```bash
git add crates/automation-runtime Cargo.toml
git commit -m "feat(automation-runtime): custom_id codec, error classify, ai-gateway guard"
```

---

## Task 2: seam 구현 (convert · mutation · responder) — 컴파일 게이트

**Files:**
- Create: `crates/automation-runtime/src/convert.rs`
- Create: `crates/automation-runtime/src/mutation.rs`
- Create: `crates/automation-runtime/src/responder.rs`
- Modify: `crates/automation-runtime/src/lib.rs`

**Interfaces:**
- Consumes: Task 1 `custom_id`, `error::classify_error`; `automation_core::{RuntimeEvent, EventKind, AdapterError, DiscordMutationAdapter, InteractionResponder}`; twilight types.
- Produces: `convert::interaction_to_event(&Interaction) -> Option<RuntimeEvent>`, `mutation::TwilightMutationAdapter<'a>` (impl DiscordMutationAdapter), `responder::TwilightInteractionResponder<'a>` + `from_interaction(&Client, &Interaction)` (impl InteractionResponder).

- [ ] **Step 1: `crates/automation-runtime/src/convert.rs` 작성**

```rust
use automation_core::{EventKind, RuntimeEvent};
use discord_model::{GuildId, UserId};
use twilight_model::application::interaction::{Interaction, InteractionData};

use crate::custom_id;

pub fn interaction_to_event(interaction: &Interaction) -> Option<RuntimeEvent> {
    let data = match &interaction.data {
        Some(InteractionData::MessageComponent(data)) => data,
        _ => return None,
    };
    let parsed = custom_id::decode(&data.custom_id).ok()?;
    let guild = interaction.guild_id?;
    let actor = actor_id(interaction)?;
    Some(RuntimeEvent {
        guild_id: GuildId(guild.get()),
        actor,
        kind: EventKind::ButtonClick {
            component: parsed.button_key,
        },
    })
}

fn actor_id(interaction: &Interaction) -> Option<UserId> {
    interaction
        .member
        .as_ref()
        .and_then(|member| member.user.as_ref())
        .or_else(|| interaction.user.as_ref())
        .map(|user| UserId(user.id.get()))
}
```

- [ ] **Step 2: `crates/automation-runtime/src/mutation.rs` 작성**

```rust
use automation_core::{AdapterError, DiscordMutationAdapter};
use discord_model::{GuildId, RoleId, UserId};
use twilight_http::Client;
use twilight_model::id::Id;

use crate::error::classify_error;

pub struct TwilightMutationAdapter<'a> {
    http: &'a Client,
}

impl<'a> TwilightMutationAdapter<'a> {
    pub fn new(http: &'a Client) -> Self {
        Self { http }
    }
}

impl DiscordMutationAdapter for TwilightMutationAdapter<'_> {
    async fn grant_role(
        &self,
        guild: GuildId,
        member: UserId,
        role: RoleId,
    ) -> Result<(), AdapterError> {
        self.http
            .add_guild_member_role(Id::new(guild.0), Id::new(member.0), Id::new(role.0))
            .await
            .map_err(|error| classify_error(&error))?;
        Ok(())
    }
}
```

- [ ] **Step 3: `crates/automation-runtime/src/responder.rs` 작성**

```rust
use automation_core::{AdapterError, InteractionResponder};
use twilight_http::Client;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::MessageFlags;
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_model::id::marker::{ApplicationMarker, InteractionMarker};
use twilight_model::id::Id;

use crate::error::classify_error;

pub struct TwilightInteractionResponder<'a> {
    http: &'a Client,
    application_id: Id<ApplicationMarker>,
    interaction_id: Id<InteractionMarker>,
    interaction_token: String,
}

impl<'a> TwilightInteractionResponder<'a> {
    pub fn from_interaction(http: &'a Client, interaction: &Interaction) -> Self {
        Self {
            http,
            application_id: interaction.application_id,
            interaction_id: interaction.id,
            interaction_token: interaction.token.clone(),
        }
    }
}

impl InteractionResponder for TwilightInteractionResponder<'_> {
    async fn respond_ephemeral(&self, content: String) -> Result<(), AdapterError> {
        let response = InteractionResponse {
            kind: InteractionResponseType::ChannelMessageWithSource,
            data: Some(InteractionResponseData {
                content: Some(content),
                flags: Some(MessageFlags::EPHEMERAL),
                ..Default::default()
            }),
        };
        self.http
            .interaction(self.application_id)
            .create_response(self.interaction_id, &self.interaction_token, &response)
            .await
            .map_err(|error| classify_error(&error))?;
        Ok(())
    }
}
```

- [ ] **Step 4: `crates/automation-runtime/src/lib.rs`에 모듈 추가**

`pub mod custom_id;` 블록을 다음으로 교체:

```rust
pub mod convert;
pub mod custom_id;
pub mod error;
pub mod mutation;
pub mod responder;

pub use convert::interaction_to_event;
pub use custom_id::{decode, encode, CustomIdError, ParsedCustomId};
pub use error::classify_error;
pub use mutation::TwilightMutationAdapter;
pub use responder::TwilightInteractionResponder;
```

- [ ] **Step 5: 빌드 + 테스트**

Run: `cargo build -p automation-runtime` then `cargo test -p automation-runtime`
Expected: 빌드 성공(경고 0), 기존 5 tests 그대로 PASS(신규 순수 테스트 없음 — seam은 live 검증).

- [ ] **Step 6: 커밋**

```bash
git add crates/automation-runtime
git commit -m "feat(automation-runtime): twilight seams (convert, mutation, per-interaction responder)"
```

---

## Task 3: gateway 루프 + runner — 컴파일 게이트

**Files:**
- Create: `crates/automation-runtime/src/gateway.rs`
- Create: `crates/automation-runtime/src/runner.rs`
- Modify: `crates/automation-runtime/src/lib.rs`

**Interfaces:**
- Produces: `gateway::run(token: String, ruleset: InteractionRuleSet, bindings: ResourceBindingMap)` (async, shard 루프), `runner::handle_interaction(&Client, &TwilightMutationAdapter, &InteractionRuleSet, &ResourceBindingMap, &Interaction)` (async).

- [ ] **Step 1: `crates/automation-runtime/src/runner.rs` 작성**

```rust
use automation_core::handle_event;
use automation_state::InteractionRuleSet;
use resource_resolution::ResourceBindingMap;
use twilight_http::Client;
use twilight_model::application::interaction::Interaction;

use crate::convert::interaction_to_event;
use crate::mutation::TwilightMutationAdapter;
use crate::responder::TwilightInteractionResponder;

pub async fn handle_interaction(
    http: &Client,
    mutation: &TwilightMutationAdapter<'_>,
    ruleset: &InteractionRuleSet,
    bindings: &ResourceBindingMap,
    interaction: &Interaction,
) {
    let Some(event) = interaction_to_event(interaction) else {
        return;
    };
    let responder = TwilightInteractionResponder::from_interaction(http, interaction);
    match handle_event(&event, ruleset, bindings, mutation, &responder).await {
        Ok(outcome) => eprintln!("interaction {} -> {outcome:?}", interaction.id.get()),
        Err(error) => eprintln!("interaction {} failed: {error:?}", interaction.id.get()),
    }
}
```

- [ ] **Step 2: `crates/automation-runtime/src/gateway.rs` 작성**

```rust
use automation_state::InteractionRuleSet;
use resource_resolution::ResourceBindingMap;
use twilight_gateway::{Event, EventTypeFlags, Intents, Shard, ShardId, StreamExt};
use twilight_http::Client;

use crate::mutation::TwilightMutationAdapter;
use crate::runner::handle_interaction;

pub async fn run(token: String, ruleset: InteractionRuleSet, bindings: ResourceBindingMap) {
    let http = Client::new(token.clone());
    let mutation = TwilightMutationAdapter::new(&http);
    let mut shard = Shard::new(ShardId::ONE, token, Intents::empty());

    while let Some(item) = shard.next_event(EventTypeFlags::INTERACTION_CREATE).await {
        let event = match item {
            Ok(event) => event,
            Err(source) => {
                eprintln!("gateway receive error: {source}");
                continue;
            }
        };
        if let Event::InteractionCreate(interaction_create) = event {
            handle_interaction(&http, &mutation, &ruleset, &bindings, &interaction_create.0).await;
        }
    }
}
```

- [ ] **Step 3: `crates/automation-runtime/src/lib.rs`에 gateway/runner 추가**

모듈 선언에 `pub mod gateway;` `pub mod runner;`를, 재노출에 `pub use gateway::run;`를 알파벳 순 위치로 추가(최종):

```rust
pub mod convert;
pub mod custom_id;
pub mod error;
pub mod gateway;
pub mod mutation;
pub mod responder;
pub mod runner;

pub use convert::interaction_to_event;
pub use custom_id::{decode, encode, CustomIdError, ParsedCustomId};
pub use error::classify_error;
pub use gateway::run;
pub use mutation::TwilightMutationAdapter;
pub use responder::TwilightInteractionResponder;
```

- [ ] **Step 4: 빌드 + 테스트 + clippy**

Run: `cargo build -p automation-runtime` / `cargo test -p automation-runtime` / `cargo clippy -p automation-runtime --all-targets -- -D warnings`
Expected: 빌드 성공, 5 tests PASS, clippy 0.

- [ ] **Step 5: 커밋**

```bash
git add crates/automation-runtime
git commit -m "feat(automation-runtime): gateway shard loop and interaction runner"
```

---

## Task 4: tools/interaction-smoke (얇은 수동 runner)

**Files:**
- Modify: `Cargo.toml` (workspace members — interaction-smoke)
- Create: `tools/interaction-smoke/Cargo.toml`
- Create: `tools/interaction-smoke/src/main.rs`

**Interfaces:**
- Consumes: `automation_runtime::{custom_id, gateway}`, `automation_state::*`, `resource_resolution::ResourceBindingMap`, twilight panel 타입.

- [ ] **Step 1: workspace members에 interaction-smoke 등록 + Cargo.toml 작성**

root `Cargo.toml` `members`의 `"tools/starring-demo",` 다음 줄에 `"tools/interaction-smoke",` 추가. 이어서:

```toml
[package]
name = "interaction-smoke"
version = "0.1.0"
edition.workspace = true

[dependencies]
automation-runtime = { path = "../../crates/automation-runtime" }
automation-state = { path = "../../crates/automation-state" }
desired-state = { path = "../../crates/desired-state" }
discord-model = { path = "../../crates/discord-model" }
resource-resolution = { path = "../../crates/resource-resolution" }
twilight-http = "0.17"
twilight-model = "0.17"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
rustls = { version = "0.23", features = ["ring"] }
```

- [ ] **Step 2: `tools/interaction-smoke/src/main.rs` 작성**

```rust
use std::env;

use automation_runtime::{custom_id, gateway};
use automation_state::{
    ActionSpec, ActionTarget, ButtonSpec, InteractionRule, InteractionRuleSet, PanelSpec,
    TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{GuildId, RoleId};
use resource_resolution::ResourceBindingMap;
use twilight_http::Client;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::id::Id;

const RULESET_KEY: &str = "demo_verify";
const BUTTON_KEY: &str = "verify_button";
const ROLE_KEY: &str = "verified_member";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let token = env::var("DISCORD_TEST_TOKEN")?;
    let guild_id: u64 = env::var("DISCORD_TEST_GUILD")?.parse()?;
    let channel_id: u64 = env::var("DISCORD_TEST_CHANNEL")?.parse()?;
    let role_id: u64 = env::var("DISCORD_TEST_ROLE")?.parse()?;

    let ruleset = demo_ruleset();
    let mut bindings = ResourceBindingMap::default();
    bindings
        .role_bindings
        .insert(ResourceKey(ROLE_KEY.to_string()), RoleId(role_id));

    install_panel(&token, guild_id, channel_id).await?;
    eprintln!("panel installed; listening for button clicks (Ctrl-C to stop)");
    gateway::run(token, ruleset, bindings).await;
    Ok(())
}

fn demo_ruleset() -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "demo_panel".to_string(),
            channel: ResourceKey("verify_channel".to_string()),
            content: "Click to verify".to_string(),
            buttons: vec![ButtonSpec {
                key: BUTTON_KEY.to_string(),
                label: "Verify".to_string(),
            }],
        }],
        rules: vec![InteractionRule {
            key: "demo_verify_rule".to_string(),
            trigger: TriggerSpec::ButtonClick {
                component: BUTTON_KEY.to_string(),
            },
            actions: vec![
                ActionSpec::GrantRole {
                    role: ResourceKey(ROLE_KEY.to_string()),
                    target: ActionTarget::Actor,
                },
                ActionSpec::RespondEphemeral {
                    content: "You are verified!".to_string(),
                },
            ],
        }],
    }
}

async fn install_panel(
    token: &str,
    guild_id: u64,
    channel_id: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let http = Client::new(token.to_string());
    let encoded = custom_id::encode(GuildId(guild_id), RULESET_KEY, BUTTON_KEY);
    let button = Component::Button(Button {
        id: None,
        custom_id: Some(encoded),
        disabled: false,
        emoji: None,
        label: Some("Verify".to_string()),
        style: ButtonStyle::Primary,
        url: None,
        sku_id: None,
    });
    let components = [Component::ActionRow(ActionRow {
        id: None,
        components: vec![button],
    })];
    http.create_message(Id::new(channel_id))
        .content("Verification panel")
        .components(&components)
        .await?;
    Ok(())
}
```

- [ ] **Step 3: 빌드**

Run: `cargo build -p interaction-smoke`
Expected: 성공, 경고 0. (실행은 env 필요 — 사용자 수동.)

- [ ] **Step 4: 커밋**

```bash
git add tools/interaction-smoke Cargo.toml
git commit -m "feat(interaction-smoke): manual gateway live-smoke runner"
```

---

## Task 5: 워크스페이스 검증 게이트 + push

**Files:** 없음.

- [ ] **Step 1: 전체 빌드**

Run: `cargo build`
Expected: 성공, 경고 0.

- [ ] **Step 2: 전체 테스트**

Run: `cargo test`
Expected: 전부 PASS. 신규 = automation-runtime 5 (custom_id 4 + no_ai_gateway 1). 기존 184 무변경 → 총 189.

- [ ] **Step 3: clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: 0.

- [ ] **Step 4: fmt**

Run: `cargo fmt --all -- --check`
Expected: diff 없음.

- [ ] **Step 5: push**

```bash
git push origin main
```

---

## Self-Review (스펙 대비)

- **스펙 D1~D9:** D1 automation-runtime 신설(Task1~3) / D2 bot-runtime 무변경(수정 안 함) / D3 interaction-smoke 얇은 runner(Task4) / D4 automation-core/state 무수정(seam만) / D5 Gateway(gateway.rs) / D6 webhook endpoint 없음 / D7 responder per-interaction(from_interaction) / D8 custom_id automation-runtime 책임(custom_id.rs) / D9 no_ai_gateway 가드(Task1) ✅.
- **스펙 §3 per-interaction responder:** `from_interaction`이 interaction.id/token/application_id bind, 16a trait 무수정 ✅. app_id는 interaction 자체에서 취득(Ready 불필요 — API 대조로 더 단순화).
- **스펙 §6 custom_id:** `starring:<guild>:<ruleset>:<button>` encode/decode + 거부 테스트 ✅.
- **스펙 §7 convert:** MessageComponent만, custom_id decode, actor=member.user/user, 미해당 None ✅.
- **스펙 §8 seam:** TwilightMutationAdapter(add_guild_member_role) / TwilightInteractionResponder(create_response ephemeral) ✅.
- **스펙 §12 완료조건:** 패널 설치/버튼 수신/rule 매칭/역할 지급/ephemeral/로그/토큰 없이 test green/env 수동 ✅.
- **스펙 §13 forbidden:** modal/dynamic/DB/webhook/retry/daemon/event-time-AI 없음 ✅.
- **API 정확성:** 모든 twilight 0.17 시그니처 실물 대조(§Grounded). `Id::new`/`.get()`, `Event::InteractionCreate(Box).0`, `next_event(EventTypeFlags)` + `StreamExt`, `InteractionData::MessageComponent(Box)` Deref, `InteractionResponseData` Default, Button 전체 필드, rustls ring, tokio 1 ✅.
- **clippy:** `.or_else`(or_fun_call 회피), `map_err(|error| ...)`, let-else, 주석 없음 ✅.
- **lib.rs 순서:** 모듈/재노출 알파벳 순(rustfmt reorder) ✅.

---

## Codex 핸드오프 (권장 2청크)

- **청크 A** = Task 1 + Task 2 (crate + custom_id/error/가드 순수 TDD + seam 3모듈). 커밋 2개. 끝에서 build/test/clippy/fmt.
- **청크 B** = Task 3 + Task 4 + Task 5 (gateway/runner + tool + 전체 게이트 + push). 커밋 3개 + push.

**automation-core/automation-state 절대 무수정.** twilight 0.17만. 완료 보고: automation-runtime 테스트 수(기대 5) + 전체(기대 189) + clippy/fmt + push 해시 + 이탈 사항.

## 사용자 live 실행법 (Codex 아님, 사용자 수동)
1. 테스트 서버에 역할 하나 생성(예 "Verified"), 그 id 확보. 봇에 **Manage Roles** 부여 + 봇 최상위 역할을 그 역할보다 위로.
2. 봇을 서버 초대(인터랙션 수신 위해 `bot` scope). **새 토큰**(이전 노출 토큰 폐기).
3. 자기 터미널에서:
```
DISCORD_TEST_TOKEN=... DISCORD_TEST_GUILD=... DISCORD_TEST_CHANNEL=... DISCORD_TEST_ROLE=... cargo run -p interaction-smoke
```
4. 채널의 "Verify" 버튼 클릭 → 역할 부여 + "You are verified!" ephemeral 확인. 로그에 `interaction <id> -> Executed`.
