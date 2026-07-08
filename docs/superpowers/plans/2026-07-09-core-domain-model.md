# Core Domain Model Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. **이 프로젝트는 Codex가 구현한다.** 각 Task는 독립적으로 테스트 가능한 산출물로 끝난다.

**Goal:** Starring의 최하위 타입 레이어를 구성하는 2개 crate(`discord-model`, `domain`)와 Cargo 워크스페이스 스캐폴드를 TDD로 구축한다.

**Architecture:** 하이브리드 2계층. 저수준 `discord-model`(Discord 상태 미러 + primitive: ID/Permissions/enum/엔티티/GuildState)과 고수준 `domain`(플랫폼 개념: OnboardingMode/Visibility/Feature)을 **단방향 의존**(`domain → discord-model`)으로 분리한다. 모든 타입은 serde 직렬화를 지원하고, ID·권한은 JSON에서 문자열로 표현하며, DB/persistence에는 무관하다.

**Tech Stack:** Rust (edition 2021, stable toolchain), `serde`, `serde_json`(dev), `bitflags` v2, Cargo workspace.

## Global Constraints

모든 Task는 아래 제약을 암묵적으로 포함한다.

- Rust **edition 2021**, toolchain **stable**.
- 의존 방향은 **`domain → discord-model`** 단방향. 순환 금지. `discord-model`은 우리 crate에 의존하지 않는다.
- 모든 데이터 타입 파생: `Clone, Debug, PartialEq, Eq, Serialize, Deserialize`. ID류는 추가로 `Copy, Hash, PartialOrd, Ord`. enum/`Permissions`는 추가로 `Copy`.
- **ID**(`GuildId`/`RoleId`/`ChannelId`/`UserId`)와 **`Permissions`**는 JSON에서 **문자열**로 직렬화한다 (Discord 관례 + JS number 정밀도 방지).
- `Permissions`는 `bitflags` v2. 역직렬화 시 **`from_bits_retain`** 을 써서 우리가 모델링하지 않은 비트도 보존한다(데이터 손실 금지).
- `domain`/`discord-model` 타입은 **DB/sqlx 의존 없음** (serde만).
- 완료 게이트(모든 Task 후 최종 확인): `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all -- --check` 전부 통과.
- crate 이름은 `discord-model`(패키지) → `discord_model`(코드 내 `use` 경로). `domain`은 그대로.

---

### Task 1: 워크스페이스 스캐폴드

빈 워크스페이스가 컴파일되는 상태를 만든다. (`git init` + 루트 파일 + 2개 빈 crate + 문서 이동)

**Files:**
- Create: `.gitignore`, `Cargo.toml`, `rust-toolchain.toml`, `README.md`
- Create: `crates/discord-model/Cargo.toml`, `crates/discord-model/src/lib.rs`
- Create: `crates/domain/Cargo.toml`, `crates/domain/src/lib.rs`
- Create: `docs/repo-structure.md`
- Move: `discord_ai_control_plane_architecture_oci.md` → `docs/discord_ai_control_plane_architecture_oci.md`

**Interfaces:**
- Consumes: (없음)
- Produces: 컴파일되는 Cargo 워크스페이스. crate `discord_model`, `domain` 존재(내용 비어 있음).

- [ ] **Step 1: git 저장소 초기화**

Run:
```bash
cd /path/to/Starring   # 실제 프로젝트 루트
git init
```
Expected: `Initialized empty Git repository ...`

- [ ] **Step 2: 루트 파일 생성**

Create `.gitignore`:
```gitignore
/target
```

Create `rust-toolchain.toml`:
```toml
[toolchain]
channel = "stable"
```

Create `Cargo.toml`:
```toml
[workspace]
members = ["crates/discord-model", "crates/domain"]
resolver = "2"

[workspace.package]
edition = "2021"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
bitflags = "2"
```

Create `README.md`:
```markdown
# Starring

AI 기반 Discord Control Plane. 자세한 아키텍처는 [docs/discord_ai_control_plane_architecture_oci.md](docs/discord_ai_control_plane_architecture_oci.md), 전체 레포 구조는 [docs/repo-structure.md](docs/repo-structure.md) 참고.

## Workspace

- `crates/discord-model` — 저수준 Discord 상태 모델 (ID, 권한, 엔티티, GuildState)
- `crates/domain` — 고수준 플랫폼 도메인 개념
```

- [ ] **Step 3: 2개 crate 스켈레톤 생성**

Create `crates/discord-model/Cargo.toml`:
```toml
[package]
name = "discord-model"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
bitflags = { workspace = true }

[dev-dependencies]
serde_json = { workspace = true }
```

Create `crates/discord-model/src/lib.rs`:
```rust
//! 저수준 Discord 상태 모델 (ID, 권한, 엔티티, GuildState).
```

Create `crates/domain/Cargo.toml`:
```toml
[package]
name = "domain"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
discord-model = { path = "../discord-model" }

[dev-dependencies]
serde_json = { workspace = true }
```

Create `crates/domain/src/lib.rs`:
```rust
//! 고수준 플랫폼 도메인 개념 (OnboardingMode, Visibility, Feature).
```

- [ ] **Step 4: 문서 이동 + repo-structure.md 생성**

Run:
```bash
mkdir -p docs
git mv discord_ai_control_plane_architecture_oci.md docs/discord_ai_control_plane_architecture_oci.md 2>/dev/null || mv discord_ai_control_plane_architecture_oci.md docs/
```

Create `docs/repo-structure.md`:
```markdown
# 전체 타깃 레포 구조 (계획)

> 아키텍처 문서 17장의 monorepo 구조. **현재는 계획**이며, 각 crate/service는 해당 Phase 구현 시점에 워크스페이스에 추가한다. 지금 물리 생성된 것은 `crates/discord-model`, `crates/domain`, `docs/`뿐이다.

\`\`\`text
discord-ai-control-plane/
├─ apps/
│  ├─ web/                         # Next.js dashboard
│  └─ ios/                         # SwiftUI app
├─ services/
│  ├─ api/                         # Rust axum backend API
│  ├─ bot-runtime/                 # Rust twilight Discord bot
│  ├─ worker/                      # Rust background workers
│  ├─ verifier/                    # Rust verifier worker
│  └─ notification-worker/
├─ crates/
│  ├─ domain/                      # [생성됨] 고수준 도메인 개념
│  ├─ discord-model/               # [생성됨] 저수준 Discord 상태
│  ├─ desired-state/               # Desired State schema
│  ├─ diff-engine/                 # Current vs Desired 비교
│  ├─ operation-graph/             # 실행 그래프 모델/컴파일러
│  ├─ policy-engine/               # 정책 검사
│  ├─ simulator/                   # 권한/상태 시뮬레이터
│  ├─ ai-gateway/                  # vLLM/OpenAI-compatible client
│  ├─ event-bus/                   # NATS abstraction
│  ├─ db/                          # sqlx repositories
│  ├─ telemetry/                   # tracing/OpenTelemetry
│  └─ config/                      # 환경설정 로딩
├─ proto/                          # gRPC/protobuf
├─ infra/                          # docker / compose / terraform / k8s
├─ migrations/                     # sqlx migrations
├─ docs/                           # [생성됨]
├─ scripts/
└─ Cargo.toml
\`\`\`
```

- [ ] **Step 5: 빈 워크스페이스가 컴파일되는지 확인**

Run:
```bash
cargo build
cargo test
```
Expected: `build` 성공, `test`는 `0 tests` 통과. 에러 없음.

- [ ] **Step 6: 커밋**

```bash
git add -A
git commit -m "chore: scaffold cargo workspace with discord-model and domain crates"
```

---

### Task 2: Snowflake ID 뉴타입

`GuildId`, `RoleId`, `ChannelId`, `UserId` — u64 wrapper, JSON 문자열 직렬화, `Display`/`FromStr`.

**Files:**
- Create: `crates/discord-model/src/ids.rs`
- Modify: `crates/discord-model/src/lib.rs`

**Interfaces:**
- Consumes: (없음)
- Produces:
  - `pub struct GuildId(pub u64)`, `RoleId`, `ChannelId`, `UserId` — 각각 `Clone+Copy+Debug+PartialEq+Eq+Hash+PartialOrd+Ord`, JSON에서 문자열로 (de)serialize, `Display`, `FromStr<Err=std::num::ParseIntError>`.
  - `lib.rs`에서 re-export: `pub use ids::{ChannelId, GuildId, RoleId, UserId};`

- [ ] **Step 1: 실패하는 테스트 작성**

Create `crates/discord-model/src/ids.rs`:
```rust
// (구현은 Step 3에서 채운다. 지금은 테스트만.)

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_json_string() {
        let id = GuildId(123456789012345678);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"123456789012345678\"");
        let back: GuildId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn display_and_from_str_roundtrip() {
        let id: RoleId = "42".parse().unwrap();
        assert_eq!(id, RoleId(42));
        assert_eq!(id.to_string(), "42");
    }

    #[test]
    fn distinct_id_types_do_not_mix() {
        // 컴파일만 되면 됨: 타입이 분리되어 있음을 문서화.
        let a = ChannelId(1);
        let b = UserId(1);
        assert_eq!(a.0, b.0);
    }
}
```

Modify `crates/discord-model/src/lib.rs` to:
```rust
//! 저수준 Discord 상태 모델 (ID, 권한, 엔티티, GuildState).

pub mod ids;

pub use ids::{ChannelId, GuildId, RoleId, UserId};
```

- [ ] **Step 2: 테스트가 실패(컴파일 에러)하는지 확인**

Run:
```bash
cargo test -p discord-model
```
Expected: FAIL — `cannot find type GuildId`(미정의) 컴파일 에러.

- [ ] **Step 3: 최소 구현 작성**

`crates/discord-model/src/ids.rs`의 테스트 모듈 **위에** 추가:
```rust
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

macro_rules! define_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl FromStr for $name {
            type Err = std::num::ParseIntError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok($name(s.parse()?))
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.0.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let s = String::deserialize(deserializer)?;
                s.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

define_id!(GuildId);
define_id!(RoleId);
define_id!(ChannelId);
define_id!(UserId);
```

- [ ] **Step 4: 테스트 통과 확인**

Run:
```bash
cargo test -p discord-model
```
Expected: PASS — 3 tests.

- [ ] **Step 5: 커밋**

```bash
cargo fmt --all
git add -A
git commit -m "feat(discord-model): add snowflake id newtypes with string serde"
```

---

### Task 3: Permissions 비트플래그

Discord 권한 비트를 `bitflags`로 표현. JSON 문자열 직렬화, 미모델링 비트 보존.

**Files:**
- Create: `crates/discord-model/src/permissions.rs`
- Modify: `crates/discord-model/src/lib.rs`

**Interfaces:**
- Consumes: (없음)
- Produces:
  - `pub struct Permissions: u64` (bitflags) — `Clone+Copy+Debug+PartialEq+Eq+Hash`, JSON 문자열 (de)serialize, 역직렬화 시 미모델링 비트 보존.
  - 상수: `CREATE_INSTANT_INVITE, KICK_MEMBERS, BAN_MEMBERS, ADMINISTRATOR, MANAGE_CHANNELS, MANAGE_GUILD, VIEW_CHANNEL, SEND_MESSAGES, MANAGE_MESSAGES, MANAGE_ROLES`.
  - `lib.rs` re-export: `pub use permissions::Permissions;`

- [ ] **Step 1: 실패하는 테스트 작성**

Create `crates/discord-model/src/permissions.rs`:
```rust
// (구현은 Step 3에서. 지금은 테스트만.)

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_values_match_discord() {
        assert_eq!(Permissions::ADMINISTRATOR.bits(), 1 << 3);
        assert_eq!(Permissions::MANAGE_CHANNELS.bits(), 1 << 4);
        assert_eq!(Permissions::VIEW_CHANNEL.bits(), 1 << 10);
        assert_eq!(Permissions::SEND_MESSAGES.bits(), 1 << 11);
        assert_eq!(Permissions::MANAGE_ROLES.bits(), 1 << 28);
    }

    #[test]
    fn serializes_as_string() {
        let p = Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES;
        // (1<<10) | (1<<11) = 1024 + 2048 = 3072
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, "\"3072\"");
        let back: Permissions = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn retains_unmodeled_bits() {
        // 우리가 정의하지 않은 비트(1<<40)도 라운드트립에서 보존되어야 한다.
        let json = "\"1099511627776\""; // 1 << 40
        let p: Permissions = serde_json::from_str(json).unwrap();
        assert_eq!(p.bits(), 1u64 << 40);
        assert_eq!(serde_json::to_string(&p).unwrap(), json);
    }
}
```

Modify `crates/discord-model/src/lib.rs` to:
```rust
//! 저수준 Discord 상태 모델 (ID, 권한, 엔티티, GuildState).

pub mod ids;
pub mod permissions;

pub use ids::{ChannelId, GuildId, RoleId, UserId};
pub use permissions::Permissions;
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

Run:
```bash
cargo test -p discord-model
```
Expected: FAIL — `cannot find ... Permissions` 컴파일 에러.

- [ ] **Step 3: 최소 구현 작성**

`crates/discord-model/src/permissions.rs`의 테스트 모듈 **위에** 추가:
```rust
use bitflags::bitflags;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct Permissions: u64 {
        const CREATE_INSTANT_INVITE = 1 << 0;
        const KICK_MEMBERS          = 1 << 1;
        const BAN_MEMBERS           = 1 << 2;
        const ADMINISTRATOR         = 1 << 3;
        const MANAGE_CHANNELS       = 1 << 4;
        const MANAGE_GUILD          = 1 << 5;
        const VIEW_CHANNEL          = 1 << 10;
        const SEND_MESSAGES         = 1 << 11;
        const MANAGE_MESSAGES       = 1 << 13;
        const MANAGE_ROLES          = 1 << 28;
    }
}

impl Serialize for Permissions {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.bits().to_string())
    }
}

impl<'de> Deserialize<'de> for Permissions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bits = s.parse::<u64>().map_err(serde::de::Error::custom)?;
        // 미모델링 비트도 보존 (curated subset이므로 실제 Discord 값엔 더 많은 비트 존재)
        Ok(Permissions::from_bits_retain(bits))
    }
}
```

> 참고: 위 10개는 Discord 공식 Permissions 비트와 일치한다. 향후 필요한 권한은 [공식 문서](https://discord.com/developers/docs/topics/permissions)에서 정확한 비트를 확인해 추가한다.

- [ ] **Step 4: 테스트 통과 확인**

Run:
```bash
cargo test -p discord-model
```
Expected: PASS — 이전 3 + 신규 3 = 6 tests.

- [ ] **Step 5: 커밋**

```bash
cargo fmt --all
git add -A
git commit -m "feat(discord-model): add Permissions bitflags with string serde and bit retention"
```

---

### Task 4: 저수준 enum + 엔티티

`ChannelType`, `OverwriteTarget`, `Guild`, `Role`, `PermissionOverwrite`, `Channel`, `Member`.

**Files:**
- Create: `crates/discord-model/src/entities.rs`
- Modify: `crates/discord-model/src/lib.rs`

**Interfaces:**
- Consumes: `ids::{ChannelId, GuildId, RoleId, UserId}`, `permissions::Permissions`.
- Produces:
  - `pub enum ChannelType { Text, Voice, Category }` (Copy)
  - `pub enum OverwriteTarget { Role(RoleId), Member(UserId) }` (Copy) — JSON: `{"type":"role","id":"..."}` (adjacently tagged)
  - `pub struct Guild { id: GuildId, name: String, owner_id: UserId }`
  - `pub struct Role { id: RoleId, name: String, permissions: Permissions, position: i32, managed: bool }`
  - `pub struct PermissionOverwrite { target: OverwriteTarget, allow: Permissions, deny: Permissions }`
  - `pub struct Channel { id: ChannelId, name: String, channel_type: ChannelType, parent_id: Option<ChannelId>, position: i32, overwrites: Vec<PermissionOverwrite> }`
  - `pub struct Member { user_id: UserId, roles: Vec<RoleId> }`
  - `lib.rs` re-export 전체.

- [ ] **Step 1: 실패하는 테스트 작성**

Create `crates/discord-model/src/entities.rs`:
```rust
// (구현은 Step 3에서. 지금은 테스트만.)

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overwrite_target_serde() {
        let t = OverwriteTarget::Role(RoleId(7));
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(json, r#"{"type":"role","id":"7"}"#);
        assert_eq!(serde_json::from_str::<OverwriteTarget>(&json).unwrap(), t);
    }

    #[test]
    fn channel_type_serde() {
        assert_eq!(serde_json::to_string(&ChannelType::Category).unwrap(), r#""category""#);
    }

    #[test]
    fn role_roundtrip() {
        let role = Role {
            id: RoleId(1),
            name: "인증됨".to_string(),
            permissions: Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
            position: 3,
            managed: false,
        };
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(serde_json::from_str::<Role>(&json).unwrap(), role);
    }

    #[test]
    fn channel_with_overwrites_roundtrip() {
        let channel = Channel {
            id: ChannelId(10),
            name: "일반".to_string(),
            channel_type: ChannelType::Text,
            parent_id: Some(ChannelId(9)),
            position: 0,
            overwrites: vec![PermissionOverwrite {
                target: OverwriteTarget::Role(RoleId(1)),
                allow: Permissions::VIEW_CHANNEL,
                deny: Permissions::empty(),
            }],
        };
        let json = serde_json::to_string(&channel).unwrap();
        assert_eq!(serde_json::from_str::<Channel>(&json).unwrap(), channel);
    }

    #[test]
    fn guild_and_member_roundtrip() {
        let guild = Guild { id: GuildId(1), name: "srv".into(), owner_id: UserId(99) };
        assert_eq!(
            serde_json::from_str::<Guild>(&serde_json::to_string(&guild).unwrap()).unwrap(),
            guild
        );
        let member = Member { user_id: UserId(5), roles: vec![RoleId(1), RoleId(2)] };
        assert_eq!(
            serde_json::from_str::<Member>(&serde_json::to_string(&member).unwrap()).unwrap(),
            member
        );
    }
}
```

Modify `crates/discord-model/src/lib.rs` to:
```rust
//! 저수준 Discord 상태 모델 (ID, 권한, 엔티티, GuildState).

pub mod entities;
pub mod ids;
pub mod permissions;

pub use entities::{
    Channel, ChannelType, Guild, Member, OverwriteTarget, PermissionOverwrite, Role,
};
pub use ids::{ChannelId, GuildId, RoleId, UserId};
pub use permissions::Permissions;
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

Run:
```bash
cargo test -p discord-model
```
Expected: FAIL — `cannot find type Role`(등) 컴파일 에러.

- [ ] **Step 3: 최소 구현 작성**

`crates/discord-model/src/entities.rs`의 테스트 모듈 **위에** 추가:
```rust
use serde::{Deserialize, Serialize};

use crate::ids::{ChannelId, GuildId, RoleId, UserId};
use crate::permissions::Permissions;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelType {
    Text,
    Voice,
    Category,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum OverwriteTarget {
    Role(RoleId),
    Member(UserId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guild {
    pub id: GuildId,
    pub name: String,
    pub owner_id: UserId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Role {
    pub id: RoleId,
    pub name: String,
    pub permissions: Permissions,
    /// 역할 계층 위치. "봇 역할보다 높은 역할 수정 금지" 정책의 근거.
    pub position: i32,
    /// 봇/통합이 관리하는 역할(직접 수정 불가).
    pub managed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionOverwrite {
    pub target: OverwriteTarget,
    pub allow: Permissions,
    pub deny: Permissions,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub name: String,
    pub channel_type: ChannelType,
    pub parent_id: Option<ChannelId>,
    pub position: i32,
    pub overwrites: Vec<PermissionOverwrite>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub user_id: UserId,
    pub roles: Vec<RoleId>,
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run:
```bash
cargo test -p discord-model
```
Expected: PASS — 이전 6 + 신규 5 = 11 tests.

- [ ] **Step 5: 커밋**

```bash
cargo fmt --all
git add -A
git commit -m "feat(discord-model): add low-level enums and entities"
```

---

### Task 5: GuildState 스냅샷

Diff가 비교하는 단위 = 서버 전체 스냅샷.

**Files:**
- Create: `crates/discord-model/src/state.rs`
- Modify: `crates/discord-model/src/lib.rs`

**Interfaces:**
- Consumes: `entities::{Channel, Guild, Member, Role}`.
- Produces:
  - `pub struct GuildState { guild: Guild, roles: Vec<Role>, channels: Vec<Channel>, members: Vec<Member> }` — `members`는 `#[serde(default)]`(비어 있거나 부재 가능).
  - `lib.rs` re-export: `pub use state::GuildState;`

- [ ] **Step 1: 실패하는 테스트 작성**

Create `crates/discord-model/src/state.rs`:
```rust
// (구현은 Step 3에서. 지금은 테스트만.)

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Guild, GuildId, UserId};

    #[test]
    fn guild_state_roundtrip() {
        let state = GuildState {
            guild: Guild { id: GuildId(1), name: "srv".into(), owner_id: UserId(99) },
            roles: vec![],
            channels: vec![],
            members: vec![],
        };
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(serde_json::from_str::<GuildState>(&json).unwrap(), state);
    }

    #[test]
    fn members_default_when_absent() {
        // members 필드가 없는 JSON도 역직렬화되어야 한다(로딩 안 한 스냅샷).
        let json = r#"{"guild":{"id":"1","name":"srv","owner_id":"99"},"roles":[],"channels":[]}"#;
        let state: GuildState = serde_json::from_str(json).unwrap();
        assert!(state.members.is_empty());
    }
}
```

Modify `crates/discord-model/src/lib.rs` to add `pub mod state;` (알파벳 순 정렬) and `pub use state::GuildState;`. 최종:
```rust
//! 저수준 Discord 상태 모델 (ID, 권한, 엔티티, GuildState).

pub mod entities;
pub mod ids;
pub mod permissions;
pub mod state;

pub use entities::{
    Channel, ChannelType, Guild, Member, OverwriteTarget, PermissionOverwrite, Role,
};
pub use ids::{ChannelId, GuildId, RoleId, UserId};
pub use permissions::Permissions;
pub use state::GuildState;
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

Run:
```bash
cargo test -p discord-model
```
Expected: FAIL — `cannot find type GuildState` 컴파일 에러.

- [ ] **Step 3: 최소 구현 작성**

`crates/discord-model/src/state.rs`의 테스트 모듈 **위에** 추가:
```rust
use serde::{Deserialize, Serialize};

use crate::entities::{Channel, Guild, Member, Role};

/// 서버 전체 상태 스냅샷. Diff Engine이 Desired State와 비교하는 단위이며,
/// Object Storage snapshot의 직렬화 대상이다.
///
/// `members`는 대형 서버에서 로딩 비용이 크므로 비어 있을 수 있다(무엇을 로딩할지는
/// 이후 Context Builder의 책임).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuildState {
    pub guild: Guild,
    pub roles: Vec<Role>,
    pub channels: Vec<Channel>,
    #[serde(default)]
    pub members: Vec<Member>,
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run:
```bash
cargo test -p discord-model
```
Expected: PASS — 이전 11 + 신규 2 = 13 tests.

- [ ] **Step 5: 커밋**

```bash
cargo fmt --all
git add -A
git commit -m "feat(discord-model): add GuildState snapshot"
```

---

### Task 6: domain crate 고수준 타입

`OnboardingMode`, `Visibility`, `Feature`, `VerificationPanel`, `ModerationRule`, `LoggingRule`.

**Files:**
- Create: `crates/domain/src/onboarding.rs`, `crates/domain/src/feature.rs`
- Modify: `crates/domain/src/lib.rs`

**Interfaces:**
- Consumes: `discord_model::{ChannelId, RoleId}`.
- Produces:
  - `pub enum OnboardingMode { Open, VerificationRequired }` — JSON snake_case
  - `pub struct Visibility { everyone: bool, roles: BTreeMap<RoleId, bool> }` (roles는 `#[serde(default)]`)
  - `pub struct VerificationPanel { channel_id: ChannelId, grants_role: RoleId }`
  - `pub struct ModerationRule {}`(skeleton), `pub struct LoggingRule {}`(skeleton) — 둘 다 `Default`
  - `pub enum Feature { Verification(VerificationPanel), Moderation(ModerationRule), Logging(LoggingRule) }` — 내부 태그 `kind`, JSON snake_case
  - `lib.rs` re-export 전체.

- [ ] **Step 1: 실패하는 테스트 작성**

Create `crates/domain/src/onboarding.rs`:
```rust
// (구현은 Step 3에서. 지금은 테스트만.)

#[cfg(test)]
mod tests {
    use super::*;
    use discord_model::RoleId;

    #[test]
    fn onboarding_mode_serde() {
        assert_eq!(
            serde_json::to_string(&OnboardingMode::VerificationRequired).unwrap(),
            r#""verification_required""#
        );
    }

    #[test]
    fn visibility_roundtrip() {
        let mut roles = std::collections::BTreeMap::new();
        roles.insert(RoleId(1), true);
        let v = Visibility { everyone: false, roles };
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(serde_json::from_str::<Visibility>(&json).unwrap(), v);
    }
}
```

Create `crates/domain/src/feature.rs`:
```rust
// (구현은 Step 3에서. 지금은 테스트만.)

#[cfg(test)]
mod tests {
    use super::*;
    use discord_model::{ChannelId, RoleId};

    #[test]
    fn feature_verification_serde() {
        let f = Feature::Verification(VerificationPanel {
            channel_id: ChannelId(100),
            grants_role: RoleId(200),
        });
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"verification","channel_id":"100","grants_role":"200"}"#
        );
        assert_eq!(serde_json::from_str::<Feature>(&json).unwrap(), f);
    }

    #[test]
    fn skeleton_features_serde() {
        let f = Feature::Moderation(ModerationRule::default());
        let json = serde_json::to_string(&f).unwrap();
        assert_eq!(json, r#"{"kind":"moderation"}"#);
        assert_eq!(serde_json::from_str::<Feature>(&json).unwrap(), f);
    }
}
```

Modify `crates/domain/src/lib.rs` to:
```rust
//! 고수준 플랫폼 도메인 개념 (OnboardingMode, Visibility, Feature).

pub mod feature;
pub mod onboarding;

pub use feature::{Feature, LoggingRule, ModerationRule, VerificationPanel};
pub use onboarding::{OnboardingMode, Visibility};
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

Run:
```bash
cargo test -p domain
```
Expected: FAIL — `cannot find type OnboardingMode`(등) 컴파일 에러.

- [ ] **Step 3: 최소 구현 작성**

`crates/domain/src/onboarding.rs`의 테스트 모듈 **위에** 추가:
```rust
use std::collections::BTreeMap;

use discord_model::RoleId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingMode {
    Open,
    VerificationRequired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visibility {
    pub everyone: bool,
    #[serde(default)]
    pub roles: BTreeMap<RoleId, bool>,
}
```

`crates/domain/src/feature.rs`의 테스트 모듈 **위에** 추가:
```rust
use discord_model::{ChannelId, RoleId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationPanel {
    pub channel_id: ChannelId,
    pub grants_role: RoleId,
    // 버튼/메시지 구성은 Phase 2+에서 확장.
}

/// Phase 2+에서 확장 (skeleton).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModerationRule {}

/// Phase 2+에서 확장 (skeleton).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggingRule {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Feature {
    Verification(VerificationPanel),
    Moderation(ModerationRule),
    Logging(LoggingRule),
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run:
```bash
cargo test -p domain
```
Expected: PASS — 4 tests.

- [ ] **Step 5: 커밋**

```bash
cargo fmt --all
git add -A
git commit -m "feat(domain): add onboarding, visibility, and feature types"
```

---

### Task 7: 인증 시나리오 통합 픽스처 + 최종 게이트

아키텍처 문서의 인증 시나리오를 `GuildState` + `Feature`로 구성하는 통합 테스트를 추가하고, 전체 품질 게이트를 통과시킨다.

**Files:**
- Create: `crates/domain/tests/verification_scenario.rs`

**Interfaces:**
- Consumes: `discord_model::{...}` 전체, `domain::{Feature, VerificationPanel}`.
- Produces: (테스트만 — 후속 Diff 테스트의 기반 픽스처)

- [ ] **Step 1: 실패하는 통합 테스트 작성**

Create `crates/domain/tests/verification_scenario.rs`:
```rust
use discord_model::{
    Channel, ChannelId, ChannelType, Guild, GuildId, GuildState, Member, OverwriteTarget,
    PermissionOverwrite, Permissions, Role, RoleId, UserId,
};
use domain::{Feature, VerificationPanel};

/// 문서 예시("신규 유저는 인증 채널만, 인증하면 일반 채널")의 목표 상태를
/// GuildState로 표현하고 serde 라운드트립을 검증한다.
#[test]
fn verification_scenario_snapshot_roundtrips() {
    let verified = RoleId(1001);

    let verified_role = Role {
        id: verified,
        name: "인증됨".to_string(),
        permissions: Permissions::empty(),
        position: 1,
        managed: false,
    };

    let verification_channel = Channel {
        id: ChannelId(2001),
        name: "인증".to_string(),
        channel_type: ChannelType::Text,
        parent_id: None,
        position: 0,
        overwrites: vec![],
    };

    // #일반: @everyone은 볼 수 없고, 인증됨 역할만 볼 수 있음
    let general_channel = Channel {
        id: ChannelId(2002),
        name: "일반".to_string(),
        channel_type: ChannelType::Text,
        parent_id: None,
        position: 1,
        overwrites: vec![PermissionOverwrite {
            target: OverwriteTarget::Role(verified),
            allow: Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
            deny: Permissions::empty(),
        }],
    };

    let state = GuildState {
        guild: Guild { id: GuildId(1), name: "커뮤니티".into(), owner_id: UserId(1) },
        roles: vec![verified_role],
        channels: vec![verification_channel, general_channel],
        members: vec![Member { user_id: UserId(5), roles: vec![] }],
    };

    let panel = Feature::Verification(VerificationPanel {
        channel_id: ChannelId(2001),
        grants_role: verified,
    });

    let state_json = serde_json::to_string(&state).unwrap();
    assert_eq!(serde_json::from_str::<GuildState>(&state_json).unwrap(), state);

    let panel_json = serde_json::to_string(&panel).unwrap();
    assert_eq!(serde_json::from_str::<Feature>(&panel_json).unwrap(), panel);
}
```

- [ ] **Step 2: 테스트가 실패하는지 확인**

Run:
```bash
cargo test -p domain --test verification_scenario
```
Expected: 컴파일은 되지만 실패하지 않으면 OK. (모든 타입이 이미 존재하므로 이 테스트는 바로 통과할 수 있다. 통과하면 Step 3 생략하고 Step 4로.)

- [ ] **Step 3: (필요 시) 구현 보정**

만약 컴파일 에러(re-export 누락 등)가 나면 해당 crate의 `lib.rs` re-export를 보완한다. 예: `discord_model`에서 특정 타입이 re-export되지 않았다면 Task 4/5의 `lib.rs` 블록과 일치하도록 추가.

- [ ] **Step 4: 전체 품질 게이트 실행**

Run:
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build
```
Expected: 4개 명령 모두 성공. `test`는 전체(discord-model 13 + domain 4 + 통합 1 = 18) 통과, `clippy` 경고 0, `fmt` diff 0.

- [ ] **Step 5: 커밋**

```bash
git add -A
git commit -m "test: add verification scenario integration fixture"
```

---

## 완료 정의 (Definition of Done)

- [ ] 워크스페이스 컴파일 (`cargo build`)
- [ ] 전체 테스트 통과 (`cargo test`, 총 18개)
- [ ] `cargo clippy --all-targets -- -D warnings` 경고 0
- [ ] `cargo fmt --all -- --check` diff 0
- [ ] `discord-model`: ID 4종, `Permissions`, `ChannelType`/`OverwriteTarget`, 엔티티 5종, `GuildState`
- [ ] `domain`: `OnboardingMode`, `Visibility`, `Feature`, `VerificationPanel`, `ModerationRule`/`LoggingRule` 스켈레톤
- [ ] ID·권한 JSON 문자열 직렬화 검증됨
- [ ] `docs/`로 아키텍처 문서 이동 + `repo-structure.md` 존재
- [ ] 각 Task별 커밋 존재
