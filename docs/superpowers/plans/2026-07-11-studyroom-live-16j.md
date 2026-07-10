# Phase 16j — StudyRoom Live Smoke Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. Steps use checkbox (`- [ ]`). **live 실행은 Claude hands-on — Codex는 코드까지.**

**Goal:** automation-runtime의 mutation seam 4개를 twilight로 실구현 + tools/interaction-smoke StudyRoom 시나리오 → 실제 Discord live smoke.

**Architecture:** `TwilightMutationAdapter`가 create_role/create_channel/upsert_overwrite/post_panel 실구현(bot-runtime 패턴 재사용) + ruleset_key(post_panel 버튼 custom_id). convert.rs에 ruleset_key/guild 라우팅 가드. tool이 RespondEphemeral-first StudyRoom 룰 + study_help 버튼. **automation-core/automation-state 무수정.**

## Global Constraints
- **코드 주석 금지.** **Codex 구현(코드), live는 Claude.**
- **automation-core/automation-state 무수정** — 16j는 automation-runtime + tool만.
- **automation-runtime은 bot-runtime에 의존하지 않음** — 검증된 twilight 패턴/변환을 복사(공통화는 후속).
- RespondEphemeral-first(3초 ACK). 완료/실패 메시징·defer는 16k.
- 토큰: env(`DISCORD_TEST_TOKEN`)만, print/commit 금지.
- 게이트: build(경고0)/test/clippy(`--all-targets -- -D warnings`)/fmt.

---

## Task 1: automation-runtime — 4 seam 실구현 + 라우팅 가드

- [ ] **Step 1: `error.rs` — classify_body_error 추가**

파일 끝(classify_error 다음)에:
```rust
pub fn classify_body_error(err: &twilight_http::response::DeserializeBodyError) -> AdapterError {
    AdapterError::new(
        AdapterErrorKind::Unknown,
        format!("twilight model error: {err}"),
    )
}
```

- [ ] **Step 2: `mutation.rs` 전체 교체** (ruleset_key + 4 seam + 변환 헬퍼)

```rust
use automation_core::{
    AdapterError, CreateChannelSpec, CreateRoleSpec, DiscordMutationAdapter, PostPanelSpec,
};
use automation_state::ButtonSpec;
use discord_model::{ChannelId, GuildId, MessageId, OverwriteTarget, Permissions, RoleId, UserId};
use twilight_http::Client;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::guild::Permissions as TwilightPermissions;
use twilight_model::http::permission_overwrite::{PermissionOverwrite, PermissionOverwriteType};
use twilight_model::id::Id;

use crate::custom_id::encode_button;
use crate::error::{classify_body_error, classify_error};

pub struct TwilightMutationAdapter<'a> {
    http: &'a Client,
    ruleset_key: String,
}

impl<'a> TwilightMutationAdapter<'a> {
    pub fn new(http: &'a Client, ruleset_key: String) -> Self {
        Self { http, ruleset_key }
    }
}

fn to_twilight_permissions(permissions: Permissions) -> TwilightPermissions {
    TwilightPermissions::from_bits_truncate(permissions.bits())
}

fn to_permission_overwrite(
    target: OverwriteTarget,
    allow: Permissions,
    deny: Permissions,
) -> PermissionOverwrite {
    let (raw_id, kind) = match target {
        OverwriteTarget::Role(role) => (role.0, PermissionOverwriteType::Role),
        OverwriteTarget::Member(user) => (user.0, PermissionOverwriteType::Member),
    };
    PermissionOverwrite {
        allow: Some(to_twilight_permissions(allow)),
        deny: Some(to_twilight_permissions(deny)),
        id: Id::new(raw_id),
        kind,
    }
}

fn to_button_component(guild: GuildId, ruleset_key: &str, button: &ButtonSpec) -> Component {
    Component::Button(Button {
        id: None,
        custom_id: Some(encode_button(guild, ruleset_key, &button.key)),
        disabled: false,
        emoji: None,
        label: Some(button.label.clone()),
        style: ButtonStyle::Primary,
        url: None,
        sku_id: None,
    })
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

    async fn create_role(
        &self,
        guild: GuildId,
        spec: CreateRoleSpec,
    ) -> Result<RoleId, AdapterError> {
        let role = self
            .http
            .create_role(Id::new(guild.0))
            .name(&spec.name)
            .await
            .map_err(|error| classify_error(&error))?
            .model()
            .await
            .map_err(|error| classify_body_error(&error))?;
        Ok(RoleId(role.id.get()))
    }

    async fn create_channel(
        &self,
        guild: GuildId,
        spec: CreateChannelSpec,
    ) -> Result<ChannelId, AdapterError> {
        let channel = self
            .http
            .create_guild_channel(Id::new(guild.0), &spec.name)
            .await
            .map_err(|error| classify_error(&error))?
            .model()
            .await
            .map_err(|error| classify_body_error(&error))?;
        Ok(ChannelId(channel.id.get()))
    }

    async fn upsert_overwrite(
        &self,
        _guild: GuildId,
        channel: ChannelId,
        target: OverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    ) -> Result<(), AdapterError> {
        let overwrite = to_permission_overwrite(target, allow, deny);
        self.http
            .update_channel_permission(Id::new(channel.0), &overwrite)
            .await
            .map_err(|error| classify_error(&error))?;
        Ok(())
    }

    async fn post_panel(
        &self,
        guild: GuildId,
        channel: ChannelId,
        spec: PostPanelSpec,
    ) -> Result<MessageId, AdapterError> {
        let buttons: Vec<Component> = spec
            .buttons
            .iter()
            .map(|button| to_button_component(guild, &self.ruleset_key, button))
            .collect();
        let components = [Component::ActionRow(ActionRow {
            id: None,
            components: buttons,
        })];
        let message = self
            .http
            .create_message(Id::new(channel.0))
            .content(&spec.content)
            .components(&components)
            .await
            .map_err(|error| classify_error(&error))?
            .model()
            .await
            .map_err(|error| classify_body_error(&error))?;
        Ok(MessageId(message.id.get()))
    }
}
```
> bot-runtime `TwilightDiscordAdapter`/convert 패턴 그대로(Phase 12 live 검증). post_panel은 tool의 install_panel Component 구성 + encode_button.
> **twilight 빌더 인자:** `.name(&spec.name)`/`.content(&spec.content)`/`create_guild_channel(.., &spec.name)`은 `&String`→`&str` deref coercion 가정(bot-runtime/tool과 동일). 만약 빌더가 `impl Into<String>` 등으로 `&String`을 거부하면 `.as_str()`로(예: `.name(spec.name.as_str())`) — 컴파일러 지시 따라 소소하게 조정.

- [ ] **Step 3: `gateway.rs` — adapter에 ruleset_key**

`let mutation = TwilightMutationAdapter::new(&http);` 를:
```rust
    let mutation = TwilightMutationAdapter::new(&http, ruleset_key.clone());
```

- [ ] **Step 4: `convert.rs` — ruleset_key/guild 라우팅 가드 + 테스트**

import: `use crate::custom_id::{self, ComponentKind, ParsedCustomId};` (ParsedCustomId 추가).

`interaction_to_event` 시그니처에 ruleset_key 추가 + 두 분기에 가드 삽입:
```rust
pub fn interaction_to_event(
    interaction: &Interaction,
    ruleset_key: &str,
) -> Option<RuntimeEvent> {
    let guild_id = GuildId(interaction.guild_id?.get());
    let actor = actor_id(interaction)?;
    match &interaction.data {
        Some(InteractionData::MessageComponent(data)) => {
            let parsed = custom_id::decode(&data.custom_id).ok()?;
            if parsed.kind != ComponentKind::Button {
                return None;
            }
            if !matches_context(&parsed, ruleset_key, guild_id) {
                return None;
            }
            Some(RuntimeEvent {
                guild_id,
                actor,
                kind: EventKind::ButtonClick {
                    component: parsed.key,
                },
            })
        }
        Some(InteractionData::ModalSubmit(data)) => {
            let parsed = custom_id::decode(&data.custom_id).ok()?;
            if parsed.kind != ComponentKind::Modal {
                return None;
            }
            if !matches_context(&parsed, ruleset_key, guild_id) {
                return None;
            }
            Some(RuntimeEvent {
                guild_id,
                actor,
                kind: EventKind::ModalSubmit {
                    modal: parsed.key,
                    inputs: collect_inputs(data),
                },
            })
        }
        _ => None,
    }
}

fn matches_context(parsed: &ParsedCustomId, ruleset_key: &str, guild_id: GuildId) -> bool {
    parsed.ruleset_key == ruleset_key && parsed.guild_id == guild_id
}
```
(collect_inputs/collect_text_inputs/actor_id 함수는 그대로 유지.)

파일 끝에 테스트 모듈 추가:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(ruleset: &str, guild: u64) -> ParsedCustomId {
        ParsedCustomId {
            guild_id: GuildId(guild),
            ruleset_key: ruleset.to_string(),
            kind: ComponentKind::Button,
            key: "study_help".to_string(),
        }
    }

    #[test]
    fn matches_same_context() {
        assert!(matches_context(
            &parsed("studyroom_demo", 7),
            "studyroom_demo",
            GuildId(7)
        ));
    }

    #[test]
    fn rejects_ruleset_mismatch() {
        assert!(!matches_context(
            &parsed("other_demo", 7),
            "studyroom_demo",
            GuildId(7)
        ));
    }

    #[test]
    fn rejects_guild_mismatch() {
        assert!(!matches_context(
            &parsed("studyroom_demo", 9),
            "studyroom_demo",
            GuildId(7)
        ));
    }
}
```

- [ ] **Step 5: `runner.rs` — interaction_to_event에 ruleset_key 전달**

`let Some(event) = interaction_to_event(interaction) else {` 를:
```rust
    let Some(event) = interaction_to_event(interaction, ruleset_key) else {
```

- [ ] **Step 6: 빌드 + 테스트 + 커밋**

Run: `cargo build -p automation-runtime` (경고 0) / `cargo test -p automation-runtime` (기존 + matches_context 3)
```bash
git add crates/automation-runtime
git commit -m "feat(automation-runtime): live mutation seams + custom_id routing guard"
```

---

## Task 2: tools/interaction-smoke — StudyRoom 시나리오 + 게이트 + push

- [ ] **Step 1: `Cargo.toml` — automation-core 의존 추가**

`[dependencies]`에(automation-state 줄 옆):
```toml
automation-core = { path = "../../crates/automation-core" }
```

- [ ] **Step 2: `src/main.rs` 전체 교체** (StudyRoom 룰 + study_help + validate)

```rust
use std::env;

use automation_core::validate;
use automation_runtime::{custom_id, gateway};
use automation_state::{
    ActionSpec, ActionTarget, ButtonSpec, ChannelRef, CreatedRef, InteractionRule,
    InteractionRuleSet, ModalFieldSpec, ModalFieldStyle, ModalSpec, OverwriteTargetSpec, PanelSpec,
    RoleRef, TriggerSpec,
};
use desired_state::ResourceKey;
use discord_model::{GuildId, Permissions};
use resource_resolution::ResourceBindingMap;
use twilight_http::Client;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::id::Id;

const RULESET_KEY: &str = "studyroom_demo";
const BUTTON_KEY: &str = "create_study_room";
const MODAL_KEY: &str = "create_study_modal";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let token = env::var("DISCORD_TEST_TOKEN")?;
    let guild_id: u64 = env::var("DISCORD_TEST_GUILD")?.parse()?;
    let channel_id: u64 = env::var("DISCORD_TEST_CHANNEL")?.parse()?;

    let ruleset = studyroom_ruleset();
    validate(&ruleset, &ResourceBindingMap::default())
        .expect("studyroom ruleset should validate");

    install_panel(&token, guild_id, channel_id).await?;
    eprintln!("panel installed; listening for interactions (Ctrl-C to stop)");
    gateway::run(
        token,
        RULESET_KEY.to_string(),
        ruleset,
        ResourceBindingMap::default(),
    )
    .await;
    Ok(())
}

fn created(key: &str) -> CreatedRef {
    CreatedRef {
        created: key.to_string(),
    }
}

fn studyroom_ruleset() -> InteractionRuleSet {
    InteractionRuleSet {
        version: 1,
        panels: vec![PanelSpec {
            key: "study_panel".to_string(),
            channel: ResourceKey("study_channel".to_string()),
            content: "Create a study room".to_string(),
            buttons: vec![ButtonSpec {
                key: BUTTON_KEY.to_string(),
                label: "Create study room".to_string(),
            }],
        }],
        modals: vec![ModalSpec {
            key: MODAL_KEY.to_string(),
            title: "Create study room".to_string(),
            fields: vec![ModalFieldSpec {
                key: "room_name".to_string(),
                label: "Room name".to_string(),
                style: ModalFieldStyle::Short,
                required: true,
            }],
        }],
        rules: vec![
            InteractionRule {
                key: "open_study_modal".to_string(),
                trigger: TriggerSpec::ButtonClick {
                    component: BUTTON_KEY.to_string(),
                },
                actions: vec![ActionSpec::OpenModal {
                    modal: MODAL_KEY.to_string(),
                }],
            },
            InteractionRule {
                key: "submit_study_modal".to_string(),
                trigger: TriggerSpec::ModalSubmit {
                    modal: MODAL_KEY.to_string(),
                },
                actions: vec![
                    ActionSpec::RespondEphemeral {
                        content: "스터디룸 '${input.room_name}'을 만들고 있어요. 곧 새 채널이 나타납니다."
                            .to_string(),
                    },
                    ActionSpec::CreateRole {
                        key: "study_member_role".to_string(),
                        name: "${input.room_name} 멤버".to_string(),
                    },
                    ActionSpec::CreateChannel {
                        key: "study_channel".to_string(),
                        name: "study-${input.room_name}".to_string(),
                    },
                    ActionSpec::UpsertOverwrite {
                        channel: ChannelRef::Created(created("study_channel")),
                        target: OverwriteTargetSpec::Everyone,
                        allow: Permissions::empty(),
                        deny: Permissions::VIEW_CHANNEL,
                    },
                    ActionSpec::UpsertOverwrite {
                        channel: ChannelRef::Created(created("study_channel")),
                        target: OverwriteTargetSpec::Role(RoleRef::Created(created(
                            "study_member_role",
                        ))),
                        allow: Permissions::VIEW_CHANNEL,
                        deny: Permissions::empty(),
                    },
                    ActionSpec::GrantRole {
                        role: RoleRef::Created(created("study_member_role")),
                        target: ActionTarget::Actor,
                    },
                    ActionSpec::PostPanel {
                        channel: ChannelRef::Created(created("study_channel")),
                        content: "스터디룸 '${input.room_name}'이 생성되었습니다. 이 채널은 스터디 멤버만 볼 수 있어요."
                            .to_string(),
                        buttons: vec![ButtonSpec {
                            key: "study_help".to_string(),
                            label: "도움말".to_string(),
                        }],
                    },
                ],
            },
            InteractionRule {
                key: "study_help_rule".to_string(),
                trigger: TriggerSpec::ButtonClick {
                    component: "study_help".to_string(),
                },
                actions: vec![ActionSpec::RespondEphemeral {
                    content: "이 채널은 스터디 멤버만 볼 수 있는 비공개 스터디룸입니다. 공개 참가 기능은 다음 단계에서 연결됩니다."
                        .to_string(),
                }],
            },
        ],
    }
}

async fn install_panel(
    token: &str,
    guild_id: u64,
    channel_id: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let http = Client::new(token.to_string());
    let encoded = custom_id::encode_button(GuildId(guild_id), RULESET_KEY, BUTTON_KEY);
    let button = Component::Button(Button {
        id: None,
        custom_id: Some(encoded),
        disabled: false,
        emoji: None,
        label: Some("Create study room".to_string()),
        style: ButtonStyle::Primary,
        url: None,
        sku_id: None,
    });
    let components = [Component::ActionRow(ActionRow {
        id: None,
        components: vec![button],
    })];
    http.create_message(Id::new(channel_id))
        .content("Study room panel")
        .components(&components)
        .await?;
    Ok(())
}
```

- [ ] **Step 3~6: 게이트 + push**
- `cargo build` (경고 0) / `cargo test` (전체 ~289; 기존 286 + matches_context 3) / `cargo clippy --all-targets -- -D warnings` (0) / `cargo fmt --all -- --check`.
- **automation-core/automation-state 무수정 확인**: `git diff --stat <16i tip>..HEAD -- crates/automation-core crates/automation-state` 비어야 함.
- `git push origin main`.
- 커밋: `feat(interaction-smoke): StudyRoom live scenario`

---

## Self-Review (스펙 대비)
- 4 seam(create_role/channel/upsert_overwrite/post_panel) bot-runtime 패턴 재사용, ruleset_key로 post_panel 버튼 custom_id ✅.
- classify_body_error(.model()용, no panic → Unknown) ✅. 라우팅 가드 matches_context(ruleset_key/guild mismatch → None) + 테스트 3 ✅.
- RespondEphemeral-first StudyRoom 룰(7액션) + study_help 정적 룰 + validate-at-startup ✅.
- **automation-core/automation-state 무수정**(automation-runtime + tool만) ✅. bot-runtime 무의존(패턴 복사) ✅.
- 짧은 키(studyroom_demo/create_study_room/study_help — custom_id ≈50자) ✅. 토큰 env-only ✅.
- clippy: map/collect, from_bits_truncate, no 주석 ✅.

## Codex 핸드오프 (권장 2청크)
- **청크 A** = Task 1(automation-runtime). build + test(matches_context 3) + automation-core/state 무수정. 커밋 1개.
- **청크 B** = Task 2(tool + 게이트 + push). build/test/clippy/fmt + 무수정 확인 + push. 커밋 1개.
**live는 Codex 아님 — Claude가 재발급 토큰으로 hands-on.** 보고: 테스트 수 + 전체 + clippy/fmt + push 해시 + core/state 무수정 + 이탈.

## Live (Claude hands-on, 코드 push 후)
재발급 토큰 + 허브 채널로: env 설정 → `cargo run -p interaction-smoke` 백그라운드 → 패널 설치 확인 → 사용자가 "Create study room" 클릭 → 모달 → 제출 → "만들고 있어요" + 방 생성 → 로그 Executed 확인 → API로 role/channel 생성 검증 → 사용자가 채널 진입 + 도움말 클릭 → ephemeral → **API로 study-* role/channel 삭제(cleanup)**.
