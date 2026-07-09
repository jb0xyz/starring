# Bot Runtime (TwilightDiscordAdapter) Implementation Plan (Phase 12c)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. **Codex가 구현한다.** Task 끝에 보고. **완료 후 git push origin main.**

**Goal:** `crates/bot-runtime` — executor-core의 `DiscordAdapter`를 twilight 0.17.1로 구현. **컴파일 + classify_status/convert 단위테스트까지**(실제 Discord 호출은 12d).

**Architecture:** convert.rs(우리 타입→twilight)·error.rs(classify_status 순수 + classify_error 래퍼)·adapter.rs(TwilightDiscordAdapter impl DiscordAdapter). tokio 없음(async fn은 runtime 불필요).

**Tech Stack:** Rust edition 2021 stable, twilight-http 0.17, twilight-model 0.17, executor-core, discord-model.

## Global Constraints
> ⚠️ **주석 금지**. **실제 Discord 호출/토큰 금지(12d 책임)** — 12c는 컴파일 + 순수 로직 테스트.
- 의존: `bot-runtime → {executor-core, discord-model, twilight-http = "0.17", twilight-model = "0.17"}`. tokio/NATS/DB 금지.
- **twilight 버전 민감**: 플랜 코드는 조사한 0.17.1 API 기준. builder 시그니처가 실제와 다르면 **Codex가 문서 보고 컴파일 조정**(목표=`cargo build` 통과). twilight-http는 **default features**로 시작(rustls provider 런타임 이슈는 12c 범위 아님).
- 스펙: `docs/superpowers/specs/2026-07-09-bot-runtime-design.md`.
- 완료 게이트: build/test/clippy(-D warnings)/fmt. Task별 커밋. **Phase 완료 후 `git push origin main`.**

---

### Task 1: crate + convert.rs + error.rs (테스트 가능 코어)

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/bot-runtime/Cargo.toml`, `src/{lib.rs, convert.rs, error.rs}`

**Interfaces:**
- Produces: `to_guild_id/to_role_id/to_channel_id/to_twilight_permissions/to_twilight_channel_type/to_permission_overwrite`, `classify_status`, `classify_error`.

- [ ] **Step 1: 워크스페이스 + Cargo + lib.rs**

Root `Cargo.toml` members에 `"crates/bot-runtime"` 추가.

Create `crates/bot-runtime/Cargo.toml`:
```toml
[package]
name = "bot-runtime"
version = "0.1.0"
edition.workspace = true

[dependencies]
executor-core = { path = "../executor-core" }
discord-model = { path = "../discord-model" }
twilight-http = "0.17"
twilight-model = "0.17"
```

Create `crates/bot-runtime/src/lib.rs` (adapter 모듈은 Task 2에서 추가):
```rust
pub mod convert;
pub mod error;

pub use error::classify_status;
```

- [ ] **Step 2: convert.rs 구현 + 테스트**

Create `crates/bot-runtime/src/convert.rs`:
```rust
use discord_model::{ChannelId, ChannelType, GuildId, OverwriteTarget, Permissions, RoleId};
use twilight_model::channel::permission_overwrite::{PermissionOverwrite, PermissionOverwriteType};
use twilight_model::channel::ChannelType as TwilightChannelType;
use twilight_model::guild::Permissions as TwilightPermissions;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, RoleMarker};
use twilight_model::id::Id;

pub fn to_guild_id(id: GuildId) -> Id<GuildMarker> {
    Id::new(id.0)
}

pub fn to_role_id(id: RoleId) -> Id<RoleMarker> {
    Id::new(id.0)
}

pub fn to_channel_id(id: ChannelId) -> Id<ChannelMarker> {
    Id::new(id.0)
}

pub fn to_twilight_permissions(permissions: Permissions) -> TwilightPermissions {
    TwilightPermissions::from_bits_truncate(permissions.bits())
}

pub fn to_twilight_channel_type(channel_type: ChannelType) -> TwilightChannelType {
    match channel_type {
        ChannelType::Text => TwilightChannelType::GuildText,
        ChannelType::Voice => TwilightChannelType::GuildVoice,
        ChannelType::Category => TwilightChannelType::GuildCategory,
    }
}

pub fn to_permission_overwrite(
    target: OverwriteTarget,
    allow: Permissions,
    deny: Permissions,
) -> PermissionOverwrite {
    let (raw_id, kind) = match target {
        OverwriteTarget::Role(role) => (role.0, PermissionOverwriteType::Role),
        OverwriteTarget::Member(user) => (user.0, PermissionOverwriteType::Member),
    };
    PermissionOverwrite {
        allow: to_twilight_permissions(allow),
        deny: to_twilight_permissions(deny),
        id: Id::new(raw_id),
        kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_roundtrip() {
        let p = Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES;
        assert_eq!(to_twilight_permissions(p).bits(), p.bits());
    }

    #[test]
    fn channel_type_maps() {
        assert_eq!(to_twilight_channel_type(ChannelType::Text), TwilightChannelType::GuildText);
        assert_eq!(to_twilight_channel_type(ChannelType::Voice), TwilightChannelType::GuildVoice);
        assert_eq!(
            to_twilight_channel_type(ChannelType::Category),
            TwilightChannelType::GuildCategory
        );
    }

    #[test]
    fn ids_convert() {
        assert_eq!(to_role_id(RoleId(42)).get(), 42);
        assert_eq!(to_channel_id(ChannelId(500)).get(), 500);
        assert_eq!(to_guild_id(GuildId(1)).get(), 1);
    }

    #[test]
    fn overwrite_role_target() {
        let ow = to_permission_overwrite(
            OverwriteTarget::Role(RoleId(7)),
            Permissions::VIEW_CHANNEL,
            Permissions::empty(),
        );
        assert_eq!(ow.id.get(), 7);
        assert_eq!(ow.kind, PermissionOverwriteType::Role);
        assert_eq!(ow.allow.bits(), Permissions::VIEW_CHANNEL.bits());
    }
}
```

- [ ] **Step 3: error.rs 구현 + classify_status 테스트**

Create `crates/bot-runtime/src/error.rs`:
```rust
use executor_core::{AdapterError, AdapterErrorKind};
use twilight_http::error::ErrorType;

pub fn classify_status(status: u16) -> AdapterErrorKind {
    match status {
        429 => AdapterErrorKind::RateLimited,
        408 => AdapterErrorKind::Timeout,
        400 => AdapterErrorKind::BadRequest,
        401 | 403 => AdapterErrorKind::Forbidden,
        404 => AdapterErrorKind::NotFound,
        500..=599 => AdapterErrorKind::ServerError,
        _ => AdapterErrorKind::Unknown,
    }
}

pub(crate) fn classify_error(err: &twilight_http::Error) -> AdapterError {
    let kind = match err.kind() {
        ErrorType::Response { status, .. } => classify_status(status.get()),
        ErrorType::RequestTimedOut => AdapterErrorKind::Timeout,
        ErrorType::RequestError => AdapterErrorKind::Network,
        ErrorType::Unauthorized => AdapterErrorKind::Forbidden,
        _ => AdapterErrorKind::Unknown,
    };
    AdapterError::new(kind, format!("twilight error: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_classification() {
        assert_eq!(classify_status(429), AdapterErrorKind::RateLimited);
        assert_eq!(classify_status(408), AdapterErrorKind::Timeout);
        assert_eq!(classify_status(400), AdapterErrorKind::BadRequest);
        assert_eq!(classify_status(401), AdapterErrorKind::Forbidden);
        assert_eq!(classify_status(403), AdapterErrorKind::Forbidden);
        assert_eq!(classify_status(404), AdapterErrorKind::NotFound);
        assert_eq!(classify_status(503), AdapterErrorKind::ServerError);
        assert_eq!(classify_status(200), AdapterErrorKind::Unknown);
    }

    #[test]
    fn retryable_reflects_status() {
        assert!(AdapterError::new(classify_status(429), "").is_retryable());
        assert!(!AdapterError::new(classify_status(403), "").is_retryable());
    }
}
```

- [ ] **Step 4: 게이트 + 커밋**
```bash
cargo test -p bot-runtime && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(bot-runtime): add twilight type conversions and status classification"
```
> ⚠️ twilight-http/twilight-model 0.17이 처음 다운로드·컴파일됨. `Id::new`/`from_bits_truncate`/`ChannelType::Guild*`/`PermissionOverwrite`/`ErrorType`/`StatusCode::get`이 실제 0.17.1과 다르면 **문서 보고 조정**(예: `status.get()`이 self by value면 `StatusCode` Copy로 동작). TLS feature 컴파일 실패 시 twilight-http features 조정.

- [ ] **Step 5: Task 보고** (편차 = twilight API 조정 내용 명시)

---

### Task 2: adapter.rs (TwilightDiscordAdapter impl DiscordAdapter)

**Files:**
- Modify: `crates/bot-runtime/src/lib.rs`
- Create: `crates/bot-runtime/src/adapter.rs`

**Interfaces:**
- Produces: `TwilightDiscordAdapter` (impl `DiscordAdapter`).

- [ ] **Step 1: lib.rs에 adapter 모듈 추가**

`crates/bot-runtime/src/lib.rs`를 교체:
```rust
pub mod adapter;
pub mod convert;
pub mod error;

pub use adapter::TwilightDiscordAdapter;
pub use error::classify_status;
```

- [ ] **Step 2: adapter.rs 구현**

Create `crates/bot-runtime/src/adapter.rs` (twilight 0.17.1 기준; builder 시그니처는 컴파일로 조정):
```rust
use discord_model::{ChannelId, GuildId, OverwriteTarget, Permissions, RoleId};
use executor_core::{AdapterError, ChannelSpec, DiscordAdapter, RoleSpec};
use twilight_http::Client;

use crate::convert::{
    to_channel_id, to_guild_id, to_permission_overwrite, to_role_id, to_twilight_channel_type,
    to_twilight_permissions,
};
use crate::error::classify_error;

pub struct TwilightDiscordAdapter {
    http: Client,
}

impl TwilightDiscordAdapter {
    pub fn new(token: String) -> Self {
        Self {
            http: Client::new(token),
        }
    }

    pub fn from_client(http: Client) -> Self {
        Self { http }
    }
}

impl DiscordAdapter for TwilightDiscordAdapter {
    async fn create_role(&self, guild: GuildId, spec: RoleSpec) -> Result<RoleId, AdapterError> {
        let mut req = self.http.create_role(to_guild_id(guild));
        if let Some(name) = &spec.name {
            req = req.name(name.as_str());
        }
        if let Some(perms) = spec.permissions {
            req = req.permissions(to_twilight_permissions(perms));
        }
        let role = req
            .await
            .map_err(|e| classify_error(&e))?
            .model()
            .await
            .map_err(|e| classify_error(&e))?;
        Ok(RoleId(role.id.get()))
    }

    async fn update_role(
        &self,
        guild: GuildId,
        id: RoleId,
        spec: RoleSpec,
    ) -> Result<(), AdapterError> {
        let mut req = self.http.update_role(to_guild_id(guild), to_role_id(id));
        if let Some(name) = &spec.name {
            req = req.name(Some(name.as_str()));
        }
        if let Some(perms) = spec.permissions {
            req = req.permissions(to_twilight_permissions(perms));
        }
        req.await.map_err(|e| classify_error(&e))?;
        Ok(())
    }

    async fn delete_role(&self, guild: GuildId, id: RoleId) -> Result<(), AdapterError> {
        self.http
            .delete_role(to_guild_id(guild), to_role_id(id))
            .await
            .map_err(|e| classify_error(&e))?;
        Ok(())
    }

    async fn create_channel(
        &self,
        guild: GuildId,
        spec: ChannelSpec,
    ) -> Result<ChannelId, AdapterError> {
        let name = spec.name.as_deref().unwrap_or_default();
        let mut req = self.http.create_guild_channel(to_guild_id(guild), name);
        if let Some(channel_type) = spec.channel_type {
            req = req.kind(to_twilight_channel_type(channel_type));
        }
        if let Some(parent) = spec.parent_id {
            req = req.parent_id(to_channel_id(parent));
        }
        let channel = req
            .await
            .map_err(|e| classify_error(&e))?
            .model()
            .await
            .map_err(|e| classify_error(&e))?;
        Ok(ChannelId(channel.id.get()))
    }

    async fn update_channel(
        &self,
        _guild: GuildId,
        id: ChannelId,
        spec: ChannelSpec,
    ) -> Result<(), AdapterError> {
        let mut req = self.http.update_channel(to_channel_id(id));
        if let Some(name) = &spec.name {
            req = req.name(name.as_str());
        }
        if let Some(channel_type) = spec.channel_type {
            req = req.kind(to_twilight_channel_type(channel_type));
        }
        req.await.map_err(|e| classify_error(&e))?;
        Ok(())
    }

    async fn delete_channel(&self, _guild: GuildId, id: ChannelId) -> Result<(), AdapterError> {
        self.http
            .delete_channel(to_channel_id(id))
            .await
            .map_err(|e| classify_error(&e))?;
        Ok(())
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
            .update_channel_permission(to_channel_id(channel), &overwrite)
            .await
            .map_err(|e| classify_error(&e))?;
        Ok(())
    }
}
```
> **twilight 조정 힌트 (Codex)**: create_role의 `.name()`은 `&str`, update_role의 `.name()`은 `Option<&str>`(조사 확인). builder가 `self`를 consume하고 `Self`를 반환하면 `req = req.x(..)` 패턴 유효. `.model().await`는 model body가 있는 응답(create)만; delete/update/overwrite는 응답 무시. 시그니처가 다르면 문서 기준으로 최소 수정.

- [ ] **Step 3: 최종 게이트**
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build
```
Expected: 전부 성공(bot-runtime은 컴파일 + convert/classify_status 테스트). 총 테스트 실제 출력대로 보고.

- [ ] **Step 4: 커밋 + push + 보고**
```bash
git add -A
git commit -m "feat(bot-runtime): implement TwilightDiscordAdapter for DiscordAdapter"
git push origin main
```
보고에 **twilight API 조정 내용**(어떤 builder 시그니처를 바꿨는지) 명시.

---

## 완료 정의 (Definition of Done)
- [ ] `cargo build`/`test`/`clippy -D warnings`/`fmt --check` 통과 (twilight 0.17 컴파일 포함)
- [ ] bot-runtime: convert(6 함수) + classify_status(순수) + classify_error(래퍼) + TwilightDiscordAdapter(7 메서드, impl DiscordAdapter)
- [ ] **테스트**: classify_status(상태코드 매핑) + convert(permissions/channel_type/id/overwrite) 단위테스트 통과
- [ ] **실제 Discord 호출/토큰 없음**(12d 책임). twilight-http default features(rustls provider는 12d)
- [ ] 의존 방향(tokio 없음)·주석 없음·Task별 커밋·**main push**. 편차(twilight API 조정) 보고
