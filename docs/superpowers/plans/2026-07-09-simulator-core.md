# Permission Simulator Core Implementation Plan (Phase 8)

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development 또는 executing-plans. **Codex가 구현한다.** Task 끝에 보고.

**Goal:** `crates/simulator` — `effective_permissions`(Discord 권한 해소 6단계) + can_view/can_send + AccessMatrix. 의존 discord-model만.

**Architecture:** GuildState + subject(역할집합) + channel → 실효 권한. base → admin bypass → @everyone overwrite → role overwrites 누적. OperationGraph 적용은 후속.

**Tech Stack:** Rust edition 2021 stable, serde, serde_json(dev), discord-model.

## Global Constraints
> ⚠️ **주석 금지**. 비트 연산 `.bits()` 명시(truncation 회피).
- 의존: `simulator → discord-model`. 역방향 금지.
- 완료 게이트: build/test/clippy(-D warnings)/fmt. Task별 커밋, Task 끝에 보고.

---

### Task 1: 스캐폴드 + effective_permissions (6단계)

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/simulator/Cargo.toml`, `src/lib.rs`, `src/permissions.rs`

**Interfaces:**
- Produces: `effective_permissions`, `can_view`, `can_send`.

- [ ] **Step 1: 워크스페이스 + crate 파일**

Root `Cargo.toml` members에 `"crates/simulator"` 추가.

Create `crates/simulator/Cargo.toml`:
```toml
[package]
name = "simulator"
version = "0.1.0"
edition.workspace = true

[dependencies]
serde = { workspace = true }
discord-model = { path = "../discord-model" }

[dev-dependencies]
serde_json = { workspace = true }
```

Create `crates/simulator/src/lib.rs`:
```rust
pub mod permissions;

pub use permissions::{can_send, can_view, effective_permissions};
```

- [ ] **Step 2: 알고리즘 테스트 작성**

Create `crates/simulator/src/permissions.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use discord_model::{
        Channel, ChannelId, ChannelType, Guild, GuildId, GuildState, OverwriteTarget,
        PermissionOverwrite, Permissions, Role, RoleId, UserId,
    };

    fn guild(roles: Vec<Role>, channels: Vec<Channel>) -> GuildState {
        GuildState { guild: Guild { id: GuildId(1), name: "g".to_string(), owner_id: UserId(1) }, roles, channels, members: vec![] }
    }
    fn role(id: u64, perms: Permissions) -> Role {
        Role { id: RoleId(id), name: format!("r{id}"), permissions: perms, position: 0, managed: false }
    }
    fn channel(overwrites: Vec<PermissionOverwrite>) -> Channel {
        Channel { id: ChannelId(10), name: "c".to_string(), channel_type: ChannelType::Text, parent_id: None, position: 0, overwrites }
    }
    fn ow(role_id: u64, allow: Permissions, deny: Permissions) -> PermissionOverwrite {
        PermissionOverwrite { target: OverwriteTarget::Role(RoleId(role_id)), allow, deny }
    }

    #[test]
    fn everyone_base_view() {
        let g = guild(vec![role(1, Permissions::VIEW_CHANNEL)], vec![]);
        let c = channel(vec![]);
        assert!(can_view(&g, &[], &c));
    }

    #[test]
    fn everyone_overwrite_deny_hides() {
        let g = guild(vec![role(1, Permissions::VIEW_CHANNEL)], vec![]);
        let c = channel(vec![ow(1, Permissions::empty(), Permissions::VIEW_CHANNEL)]);
        assert!(!can_view(&g, &[], &c));
    }

    #[test]
    fn role_allow_beats_everyone_deny() {
        let g = guild(vec![role(1, Permissions::VIEW_CHANNEL), role(100, Permissions::empty())], vec![]);
        let c = channel(vec![
            ow(1, Permissions::empty(), Permissions::VIEW_CHANNEL),
            ow(100, Permissions::VIEW_CHANNEL, Permissions::empty()),
        ]);
        assert!(!can_view(&g, &[], &c));
        assert!(can_view(&g, &[RoleId(100)], &c));
    }

    #[test]
    fn send_requires_view_and_send() {
        let g = guild(vec![role(1, Permissions::empty()), role(100, Permissions::empty())], vec![]);
        let with_send = channel(vec![ow(100, Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES, Permissions::empty())]);
        assert!(can_send(&g, &[RoleId(100)], &with_send));
        let view_only = channel(vec![ow(100, Permissions::VIEW_CHANNEL, Permissions::empty())]);
        assert!(!can_send(&g, &[RoleId(100)], &view_only));
    }

    #[test]
    fn administrator_bypasses_overwrites() {
        let g = guild(vec![role(1, Permissions::VIEW_CHANNEL), role(200, Permissions::ADMINISTRATOR)], vec![]);
        let c = channel(vec![ow(1, Permissions::empty(), Permissions::VIEW_CHANNEL)]);
        assert!(can_view(&g, &[RoleId(200)], &c));
    }
}
```

- [ ] **Step 3: 실패 확인** — `cargo test -p simulator` → FAIL.

- [ ] **Step 4: permissions.rs 구현**

`permissions.rs` 테스트 위에:
```rust
use discord_model::{Channel, GuildState, OverwriteTarget, PermissionOverwrite, Permissions, RoleId};

pub fn effective_permissions(guild: &GuildState, subject_roles: &[RoleId], channel: &Channel) -> Permissions {
    let everyone_id = RoleId(guild.guild.id.0);

    let mut base = role_permissions(guild, everyone_id);
    for rid in subject_roles {
        base |= role_permissions(guild, *rid);
    }
    if base.contains(Permissions::ADMINISTRATOR) {
        return Permissions::all();
    }

    let mut perms = base;
    if let Some(overwrite) = find_overwrite(channel, everyone_id) {
        perms = apply_overwrite(perms, overwrite.allow, overwrite.deny);
    }

    let mut allow_accum = Permissions::empty();
    let mut deny_accum = Permissions::empty();
    for rid in subject_roles {
        if let Some(overwrite) = find_overwrite(channel, *rid) {
            allow_accum |= overwrite.allow;
            deny_accum |= overwrite.deny;
        }
    }
    apply_overwrite(perms, allow_accum, deny_accum)
}

fn role_permissions(guild: &GuildState, id: RoleId) -> Permissions {
    guild
        .roles
        .iter()
        .find(|r| r.id == id)
        .map(|r| r.permissions)
        .unwrap_or_else(Permissions::empty)
}

fn find_overwrite(channel: &Channel, role_id: RoleId) -> Option<&PermissionOverwrite> {
    channel.overwrites.iter().find(|o| o.target == OverwriteTarget::Role(role_id))
}

fn apply_overwrite(perms: Permissions, allow: Permissions, deny: Permissions) -> Permissions {
    Permissions::from_bits_retain((perms.bits() & !deny.bits()) | allow.bits())
}

pub fn can_view(guild: &GuildState, subject_roles: &[RoleId], channel: &Channel) -> bool {
    effective_permissions(guild, subject_roles, channel).contains(Permissions::VIEW_CHANNEL)
}

pub fn can_send(guild: &GuildState, subject_roles: &[RoleId], channel: &Channel) -> bool {
    let perms = effective_permissions(guild, subject_roles, channel);
    perms.contains(Permissions::VIEW_CHANNEL) && perms.contains(Permissions::SEND_MESSAGES)
}
```

- [ ] **Step 5: 통과 + 커밋**
```bash
cargo test -p simulator && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add -A
git commit -m "feat(simulator): add Discord permission resolution"
```

- [ ] **Step 6: Task 보고**

---

### Task 2: AccessMatrix + 인증 시나리오 + 최종 게이트

**Files:**
- Create: `crates/simulator/src/matrix.rs`, `crates/simulator/tests/verification_scenario.rs`
- Modify: `crates/simulator/src/lib.rs`

**Interfaces:**
- Produces: `SubjectSpec`, `AccessCell`, `AccessMatrix`, `access_matrix`.

- [ ] **Step 1: matrix 테스트 + 구현**

Create `crates/simulator/src/matrix.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use discord_model::{Channel, ChannelId, ChannelType, Guild, GuildId, GuildState, Role, RoleId, UserId, Permissions};

    #[test]
    fn matrix_covers_subjects_and_channels() {
        let g = GuildState {
            guild: Guild { id: GuildId(1), name: "g".to_string(), owner_id: UserId(1) },
            roles: vec![Role { id: RoleId(1), name: "everyone".to_string(), permissions: Permissions::VIEW_CHANNEL, position: 0, managed: false }],
            channels: vec![Channel { id: ChannelId(10), name: "general".to_string(), channel_type: ChannelType::Text, parent_id: None, position: 0, overwrites: vec![] }],
            members: vec![],
        };
        let subjects = vec![SubjectSpec { name: "new".to_string(), roles: vec![] }];
        let m = access_matrix(&g, &subjects);
        assert_eq!(m.cells.len(), 1);
        assert_eq!(m.cells[0].subject, "new");
        assert_eq!(m.cells[0].channel, "general");
        assert!(m.cells[0].can_view);
    }
}
```

`matrix.rs` 테스트 위에:
```rust
use serde::{Deserialize, Serialize};

use discord_model::{GuildState, RoleId};

use crate::permissions::{can_send, can_view};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectSpec {
    pub name: String,
    pub roles: Vec<RoleId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessCell {
    pub subject: String,
    pub channel: String,
    pub can_view: bool,
    pub can_send: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessMatrix {
    pub cells: Vec<AccessCell>,
}

pub fn access_matrix(guild: &GuildState, subjects: &[SubjectSpec]) -> AccessMatrix {
    let mut cells = Vec::new();
    for subject in subjects {
        for channel in &guild.channels {
            cells.push(AccessCell {
                subject: subject.name.clone(),
                channel: channel.name.clone(),
                can_view: can_view(guild, &subject.roles, channel),
                can_send: can_send(guild, &subject.roles, channel),
            });
        }
    }
    AccessMatrix { cells }
}
```

Modify `lib.rs`: `pub mod matrix;` + `pub use matrix::{access_matrix, AccessCell, AccessMatrix, SubjectSpec};`.

- [ ] **Step 2: 통과 확인** — `cargo test -p simulator` → PASS. (matrix는 단순 데이터 타입+함수라 Step 1에서 테스트+구현을 함께 작성; 별도 RED 단계 없음.)

- [ ] **Step 3: 인증 시나리오 통합 테스트**

Create `crates/simulator/tests/verification_scenario.rs`:
```rust
use discord_model::{
    Channel, ChannelId, ChannelType, Guild, GuildId, GuildState, OverwriteTarget,
    PermissionOverwrite, Permissions, Role, RoleId, UserId,
};
use simulator::{access_matrix, SubjectSpec};

fn after_guild() -> GuildState {
    let everyone = RoleId(1);
    let verified = RoleId(100);
    GuildState {
        guild: Guild { id: GuildId(1), name: "srv".to_string(), owner_id: UserId(1) },
        roles: vec![
            Role { id: everyone, name: "everyone".to_string(), permissions: Permissions::VIEW_CHANNEL, position: 0, managed: false },
            Role { id: verified, name: "Verified".to_string(), permissions: Permissions::empty(), position: 1, managed: false },
        ],
        channels: vec![
            Channel {
                id: ChannelId(20), name: "verification".to_string(), channel_type: ChannelType::Text, parent_id: None, position: 0,
                overwrites: vec![PermissionOverwrite { target: OverwriteTarget::Role(everyone), allow: Permissions::VIEW_CHANNEL, deny: Permissions::empty() }],
            },
            Channel {
                id: ChannelId(21), name: "general".to_string(), channel_type: ChannelType::Text, parent_id: None, position: 1,
                overwrites: vec![
                    PermissionOverwrite { target: OverwriteTarget::Role(everyone), allow: Permissions::empty(), deny: Permissions::VIEW_CHANNEL },
                    PermissionOverwrite { target: OverwriteTarget::Role(verified), allow: Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES, deny: Permissions::empty() },
                ],
            },
        ],
        members: vec![],
    }
}

fn cell<'a>(m: &'a simulator::AccessMatrix, subject: &str, channel: &str) -> &'a simulator::AccessCell {
    m.cells.iter().find(|c| c.subject == subject && c.channel == channel).unwrap()
}

#[test]
fn verification_visibility_preview() {
    let g = after_guild();
    let subjects = vec![
        SubjectSpec { name: "new".to_string(), roles: vec![] },
        SubjectSpec { name: "verified".to_string(), roles: vec![RoleId(100)] },
    ];
    let m = access_matrix(&g, &subjects);

    assert!(cell(&m, "new", "verification").can_view);
    assert!(!cell(&m, "new", "general").can_view);
    assert!(cell(&m, "verified", "general").can_view);
    assert!(cell(&m, "verified", "general").can_send);
}
```

- [ ] **Step 5: 최종 게이트**
```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build
```
Expected: 전부 성공. 총 테스트 실제 출력대로 보고.

- [ ] **Step 6: 커밋 + 보고**
```bash
git add -A
git commit -m "feat(simulator): add AccessMatrix and verification visibility scenario"
```

---

## 완료 정의 (Definition of Done)
- [ ] `cargo build`/`test`/`clippy -D warnings`/`fmt --check` 통과
- [ ] simulator: effective_permissions(6단계) + can_view/can_send + AccessMatrix/access_matrix
- [ ] 알고리즘 테스트: everyone view / overwrite deny / role allow beats everyone deny / send / **ADMINISTRATOR bypass**
- [ ] **인증 시나리오**: new=verification만 보임, verified=general 보임+쓰기
- [ ] 의존 `simulator → discord-model`, 주석 없음, Task별 커밋
