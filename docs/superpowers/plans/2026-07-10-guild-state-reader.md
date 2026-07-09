# GuildState Reader Implementation Plan (Phase 13a)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. **Codex가 구현한다.** Task 끝에 보고. **완료 후 git push origin main.**

**Goal:** `GuildStateReader` trait(executor-core) + `TwilightDiscordAdapter`가 impl(bot-runtime, twilight read) + twilight→discord-model 역변환. 실제 Discord read는 안 함(13b).

**Architecture:** executor-core에 읽기 포트 trait(twilight 무관). bot-runtime의 TwilightDiscordAdapter가 `roles()`/`guild_channels()`로 읽어 GuildState 변환. 읽기 변환은 `reader.rs`에(convert.rs의 http PermissionOverwrite와 이름 충돌 회피).

**Tech Stack:** Rust edition 2021 stable, twilight 0.17.1, executor-core, discord-model.

## Global Constraints
> ⚠️ **주석 금지**. **실제 Discord read 금지(13b)** — 컴파일 + 역변환 순수 테스트까지.
- 의존 변경 없음(executor-core=discord-model, bot-runtime=twilight 기존).
- twilight 버전 민감: 조사한 0.17.1 기준. 어긋나면 Codex 컴파일 조정.
- 스펙: `docs/superpowers/specs/2026-07-10-guild-state-reader-design.md`.
- 완료 게이트: build/test/clippy(-D warnings)/fmt. Task별 커밋. 완료 후 `git push origin main`.

---

### Task 1: executor-core GuildStateReader trait

**Files:**
- Create: `crates/executor-core/src/reader.rs`
- Modify: `crates/executor-core/src/lib.rs`

**Interfaces:**
- Produces: `GuildStateReader` trait.

- [ ] **Step 1: reader.rs**

Create `crates/executor-core/src/reader.rs`:
```rust
use discord_model::{GuildId, GuildState};

use crate::adapter::AdapterError;

#[allow(async_fn_in_trait)]
pub trait GuildStateReader {
    async fn read_guild_state(&self, guild_id: GuildId) -> Result<GuildState, AdapterError>;
}
```

- [ ] **Step 2: lib.rs 갱신**

`crates/executor-core/src/lib.rs`에 추가:
```rust
pub mod reader;
```
그리고 pub use 블록에 추가:
```rust
pub use reader::GuildStateReader;
```

- [ ] **Step 3: 게이트 + 커밋**
```bash
cargo build -p executor-core && cargo clippy -p executor-core --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(executor-core): add GuildStateReader read port trait"
```

- [ ] **Step 4: Task 보고**

---

### Task 2: bot-runtime TwilightDiscordAdapter read 구현

**Files:**
- Modify: `crates/bot-runtime/src/adapter.rs` (http 필드 가시성)
- Modify: `crates/bot-runtime/src/lib.rs`
- Create: `crates/bot-runtime/src/reader.rs`

**Interfaces:**
- Consumes: `executor_core::GuildStateReader`, twilight read API.
- Produces: `impl GuildStateReader for TwilightDiscordAdapter`.

- [ ] **Step 1: adapter.rs — http 필드 pub(crate)**

`crates/bot-runtime/src/adapter.rs`의 struct 필드를 변경:
```rust
pub struct TwilightDiscordAdapter {
    pub(crate) http: Client,
}
```
(기존 `http: Client` → `pub(crate) http: Client`. reader.rs가 같은 crate에서 접근.)

- [ ] **Step 2: lib.rs — reader 모듈 추가**

`crates/bot-runtime/src/lib.rs`에 `pub mod reader;` 추가.

- [ ] **Step 3: reader.rs 테스트 먼저**

Create `crates/bot-runtime/src/reader.rs` (테스트 포함, 아래 Step 4에서 위에 구현 추가):
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use twilight_model::id::Id;

    #[test]
    fn channel_type_maps_back() {
        assert_eq!(from_twilight_channel_type(TwChannelType::GuildText), ChannelType::Text);
        assert_eq!(from_twilight_channel_type(TwChannelType::GuildVoice), ChannelType::Voice);
        assert_eq!(
            from_twilight_channel_type(TwChannelType::GuildCategory),
            ChannelType::Category
        );
    }

    #[test]
    fn overwrite_role_and_member() {
        let role_ow = TwOverwrite {
            allow: twilight_model::guild::Permissions::from_bits_truncate(
                Permissions::VIEW_CHANNEL.bits(),
            ),
            deny: twilight_model::guild::Permissions::empty(),
            id: Id::new(7),
            kind: TwOverwriteType::Role,
        };
        let converted = from_twilight_overwrite(role_ow);
        assert_eq!(converted.target, OverwriteTarget::Role(RoleId(7)));
        assert_eq!(converted.allow.bits(), Permissions::VIEW_CHANNEL.bits());

        let member_ow = TwOverwrite {
            allow: twilight_model::guild::Permissions::empty(),
            deny: twilight_model::guild::Permissions::empty(),
            id: Id::new(9),
            kind: TwOverwriteType::Member,
        };
        assert_eq!(
            from_twilight_overwrite(member_ow).target,
            OverwriteTarget::Member(UserId(9))
        );
    }
}
```

- [ ] **Step 4: 실패 확인** — `cargo test -p bot-runtime` → FAIL(from_twilight_* 미구현).

- [ ] **Step 5: reader.rs 구현 (테스트 위에)**

`reader.rs` 테스트 모듈 위에:
```rust
use discord_model::{
    Channel, ChannelId, ChannelType, Guild, GuildId, GuildState, OverwriteTarget,
    PermissionOverwrite, Permissions, Role, RoleId, UserId,
};
use executor_core::{AdapterError, GuildStateReader};
use twilight_model::channel::permission_overwrite::{
    PermissionOverwrite as TwOverwrite, PermissionOverwriteType as TwOverwriteType,
};
use twilight_model::channel::{Channel as TwChannel, ChannelType as TwChannelType};
use twilight_model::guild::Role as TwRole;

use crate::adapter::TwilightDiscordAdapter;
use crate::convert::to_guild_id;
use crate::error::{classify_body_error, classify_error};

fn from_twilight_role(role: TwRole) -> Role {
    Role {
        id: RoleId(role.id.get()),
        name: role.name,
        permissions: Permissions::from_bits_retain(role.permissions.bits()),
        position: i32::try_from(role.position).unwrap_or(0),
        managed: role.managed,
    }
}

fn from_twilight_channel_type(kind: TwChannelType) -> ChannelType {
    match kind {
        TwChannelType::GuildVoice => ChannelType::Voice,
        TwChannelType::GuildCategory => ChannelType::Category,
        _ => ChannelType::Text,
    }
}

fn from_twilight_overwrite(overwrite: TwOverwrite) -> PermissionOverwrite {
    let target = match overwrite.kind {
        TwOverwriteType::Member => OverwriteTarget::Member(UserId(overwrite.id.get())),
        _ => OverwriteTarget::Role(RoleId(overwrite.id.get())),
    };
    PermissionOverwrite {
        target,
        allow: Permissions::from_bits_retain(overwrite.allow.bits()),
        deny: Permissions::from_bits_retain(overwrite.deny.bits()),
    }
}

fn from_twilight_channel(channel: TwChannel) -> Channel {
    Channel {
        id: ChannelId(channel.id.get()),
        name: channel.name.unwrap_or_default(),
        channel_type: from_twilight_channel_type(channel.kind),
        parent_id: channel.parent_id.map(|p| ChannelId(p.get())),
        position: channel.position.unwrap_or(0),
        overwrites: channel
            .permission_overwrites
            .unwrap_or_default()
            .into_iter()
            .map(from_twilight_overwrite)
            .collect(),
    }
}

impl GuildStateReader for TwilightDiscordAdapter {
    async fn read_guild_state(&self, guild_id: GuildId) -> Result<GuildState, AdapterError> {
        let tw_guild = to_guild_id(guild_id);
        let roles = self
            .http
            .roles(tw_guild)
            .await
            .map_err(|e| classify_error(&e))?
            .model()
            .await
            .map_err(|e| classify_body_error(&e))?;
        let channels = self
            .http
            .guild_channels(tw_guild)
            .await
            .map_err(|e| classify_error(&e))?
            .model()
            .await
            .map_err(|e| classify_body_error(&e))?;
        Ok(GuildState {
            guild: Guild { id: guild_id, name: String::new(), owner_id: UserId(0) },
            roles: roles.into_iter().map(from_twilight_role).collect(),
            channels: channels.into_iter().map(from_twilight_channel).collect(),
            members: Vec::new(),
        })
    }
}
```
> **twilight 조정 힌트 (Codex)**: 메서드는 `http.roles(guild_id)` / `http.guild_channels(guild_id)`(둘 다 `.await?.model().await?`로 Vec 반환). `Role.position`은 **i64**(i32::try_from). `Channel`의 name/parent_id/position/permission_overwrites는 **Option**. read overwrite는 `twilight_model::channel::permission_overwrite::PermissionOverwrite`(convert.rs의 http 변주와 다름 — 여기선 alias `TwOverwrite`로 임포트). 시그니처가 다르면 문서 보고 최소 수정.

- [ ] **Step 6: 통과 확인** — `cargo test -p bot-runtime` → 역변환 테스트 통과 + 컴파일.

- [ ] **Step 7: 최종 게이트**
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build
```
Expected: 전부 성공. 총 테스트 실제 출력대로 보고.

- [ ] **Step 8: 커밋 + push + 보고**
```bash
git add -A
git commit -m "feat(bot-runtime): implement GuildStateReader with twilight read and reverse conversion"
git push origin main
```
보고에 **twilight read API 조정 내용**(메서드/필드 시그니처) 명시.

---

## 완료 정의 (Definition of Done)
- [ ] `cargo build`/`test`/`clippy -D warnings`/`fmt --check` 통과
- [ ] executor-core: `GuildStateReader` trait(read_guild_state -> Result<GuildState, AdapterError>)
- [ ] bot-runtime: TwilightDiscordAdapter impl GuildStateReader(roles/guild_channels read) + from_twilight_role/channel/overwrite/channel_type + adapter http pub(crate)
- [ ] **테스트**: channel_type/overwrite 역변환 통과. role/channel 역변환·read_guild_state는 컴파일 검증(실제 read는 13b)
- [ ] @everyone(id==guild_id) 보존·position i64→i32·Option 정규화. 실제 Discord read 없음
- [ ] 의존 방향·주석 없음·Task별 커밋·**main push**. 편차(twilight API 조정) 보고
