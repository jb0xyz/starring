# Core Domain Model 설계 스펙

- **작성일**: 2026-07-09
- **상태**: 확정 (구현 대기 — Codex 핸드오프 대상)
- **범위**: 프로젝트(Starring) 최초 구현 단위 — 폴더 스캐폴드 + Core Domain Model (Phase 1)
- **선행 문서**: `discord_ai_control_plane_architecture_oci.md` (전체 아키텍처)

---

## 0. 목적

Starring의 **가장 밑바닥 타입 레이어**를 만든다. 이후 모든 crate(diff-engine, operation-graph, policy-engine, simulator, desired-state, db …)가 이 타입 위에 세워진다. 이 스펙은 **타입 정의**가 핵심이며, 비즈니스 로직(Diff/Policy/Simulator 등)은 범위 밖이다.

---

## 1. 확정된 설계 결정 (Decisions)

| # | 결정 | 내용 |
|---|---|---|
| D1 | **하이브리드 2계층** | 저수준 `discord-model` + 고수준 `domain` + (미래) 명시적 매핑 |
| D2 | **큐레이티드 서브셋** | 플랫폼이 실제 조작/판단하는 것만 모델링. 이모지·스티커·스레드·포럼·음성상태·초대 등 제외 |
| D3 | **충실한 비트플래그 권한** | Discord 권한 비트(u64)를 `bitflags`로 표현. PermissionOverwrite = (allow, deny) 쌍 |
| D4 | **`GuildState` 스냅샷** | Diff가 비교하는 단위 = 서버 전체 스냅샷. Phase 1의 실질 결과물 |
| D5 | **Role 필드 최소화** | 표시용 필드(color/hoist/mentionable) 제외. Diff/Policy에 필요한 것만 |
| D6 | **스캐폴드 A안** | 지금은 워크스페이스 + `domain`/`discord-model`만 실제 생성. 전체 구조는 문서로 확정, 점진적 확장 |

---

## 2. Crate 구조 & 의존 방향

```
domain  ──depends on──▶  discord-model
(고수준)                 (저수준·기반, 우리 crate에 의존 없음)
```

- **단방향, 순환 없음.**
- 공유 primitive(ID 뉴타입, `Permissions`, enum)는 **저수준 `discord-model`에 둔다** (고수준이 저수준 ID를 참조하기 때문).
- 미래에 `desired-state`/`diff-engine` 등이 primitive를 많이 쓰게 되면 `crates/primitives`로 추출 가능. **지금은 YAGNI로 `discord-model`에 유지.**

---

## 3. `discord-model` crate (저수준·기반)

### 3.1 ID 뉴타입 (snowflake)

`GuildId`, `RoleId`, `ChannelId`, `UserId` — 전부 `u64`(Discord snowflake) wrapper. **서로 안 섞이도록 타입 분리.**

```rust
// 매크로로 4개 정의 권장
pub struct GuildId(pub u64);
pub struct RoleId(pub u64);
pub struct ChannelId(pub u64);
pub struct UserId(pub u64);
```

파생/구현:
- `Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd`
- `Display` / `FromStr`
- **serde: 내부는 `u64`지만 JSON 직렬화 시 문자열로 한다** (Discord API 관례 + JS number 정밀도 손실 방지). `serde_with::DisplayFromStr` 또는 커스텀 (de)serialize.

### 3.2 Permissions (bitflags)

```rust
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
        // … Discord 공식 문서의 권한 비트에서 큐레이티드 서브셋 사용
    }
}
```

- 비트 값은 **Discord 공식 Permissions 문서를 근거로 정확히** 채운다 (위 값은 실제 Discord 비트 위치와 일치, 나머지는 Codex가 공식 문서에서 확인).
- **serde: 문자열로 직렬화** (Discord는 permissions를 문자열로 전송). bitflags v2 serde 사용 시 bits를 문자열화.

### 3.3 enums

```rust
pub enum ChannelType { Text, Voice, Category }
pub enum OverwriteTarget { Role(RoleId), Member(UserId) }
```
- 큐레이티드: 스레드/포럼/스테이지/뉴스 등 제외. 확장 대비해 `#[non_exhaustive]` 고려.

### 3.4 엔티티

```rust
pub struct Guild {
    pub id: GuildId,
    pub name: String,
    pub owner_id: UserId,
}

pub struct Role {
    pub id: RoleId,
    pub name: String,
    pub permissions: Permissions,
    pub position: i32,   // 역할 계층 — "봇 역할보다 높은 역할 수정 금지" 정책의 근거
    pub managed: bool,   // 봇/통합이 관리하는 역할 (직접 수정 불가)
}

pub struct PermissionOverwrite {
    pub target: OverwriteTarget,
    pub allow: Permissions,
    pub deny: Permissions,
}

pub struct Channel {
    pub id: ChannelId,
    pub name: String,
    pub channel_type: ChannelType,
    pub parent_id: Option<ChannelId>,   // 카테고리
    pub position: i32,
    pub overwrites: Vec<PermissionOverwrite>,
}

pub struct Member {
    pub user_id: UserId,
    pub roles: Vec<RoleId>,
}
```

**제외한 필드 (의도적, D2/D5)**: Role의 color/hoist/mentionable, Member의 nickname/joined_at/avatar, Channel의 topic/nsfw/rate_limit 등 표시·부가 정보. → 필요해지면 그때 추가.

### 3.5 `GuildState` (D4 — 핵심)

```rust
pub struct GuildState {
    pub guild: Guild,
    pub roles: Vec<Role>,
    pub channels: Vec<Channel>,
    pub members: Vec<Member>,
}
```

- **이것이 "Current State"의 실체**이며 Diff Engine이 Desired State와 통째로 비교하는 단위다.
- Object Storage snapshot(`snapshots/guild_id/job_id/snapshot.json.zst`)의 직렬화 대상도 이 타입.
- **설계 노트**: 대형 서버에서 `members` 전량 로딩은 비싸다. Phase 1에서는 타입만 정의하고, `members`는 **비어 있을 수 있음**(로딩 안 함)을 허용한다. 무엇을 로딩할지는 이후 Context Builder의 책임.

---

## 4. `domain` crate (고수준·플랫폼 개념)

`discord-model`의 ID/타입을 참조한다. **Phase 1에서는 "타입 정의"가 목적이며 로직은 없다.**

```rust
pub enum OnboardingMode {
    Open,
    VerificationRequired,
}

pub struct Visibility {
    pub everyone: bool,
    pub roles: std::collections::BTreeMap<RoleId, bool>,
}

pub enum Feature {
    Verification(VerificationPanel),
    Moderation(ModerationRule),
    Logging(LoggingRule),
}

pub struct VerificationPanel {
    pub channel_id: ChannelId,
    pub grants_role: RoleId,
    // 버튼/메시지 구성은 Phase 2+에서 확장
}

// Phase 1: 스켈레톤만 (필드 최소 or 빈 구조체 + TODO 주석)
pub struct ModerationRule { /* skeleton */ }
pub struct LoggingRule    { /* skeleton */ }
```

- **주의**: 이 고수준 타입들은 플랫폼 공통 어휘다. Discord 리소스를 가리킬 때는 실제 `ChannelId`/`RoleId`를 참조한다(현재 상태 표현에 사용). 반면 Desired State에서 쓰는 **logical key**(예: `key: verified_member`) 기반 표현은 Phase 2 `desired-state` crate의 **별개 타입/관심사**이며 이 스펙에서 다루지 않는다.
- **주의**: `OnboardingMode`/`Visibility`는 아키텍처 문서의 Phase 1 목록에는 없지만, 승인된 "고수준 개념" 어휘(Q1-A)에 해당하므로 **타입 정의만** 선반영한다(로직 없음).

---

## 5. 공통 컨벤션

- **파생**: 모든 데이터 타입에 `Clone, Debug, PartialEq, Eq, Serialize, Deserialize`. ID류는 추가로 `Copy, Hash, Ord`.
- **순수성**: 도메인 타입은 **DB/sqlx에 무관**하게 유지한다. persistence 매핑은 이후 `db` crate의 책임. serde만 붙여 AI/NATS/snapshot 직렬화를 지원.
- **ID/permissions는 JSON에서 문자열**로 (Discord 관례 + JS 정밀도).
- **에러**: Phase 1은 대부분 순수 데이터. 검증이 필요한 생성자에만 `thiserror` 기반 에러. 무리한 invariant 강제는 지양(YAGNI).
- **`#[non_exhaustive]`**: 나중에 variant/field가 늘어날 enum(`ChannelType`, `Feature` 등)에 고려.

---

## 6. 스캐폴드 (D6 — A안)

### 6.1 지금 실제 생성할 것

```
Starring/
├─ Cargo.toml                 # [workspace] members = ["crates/domain", "crates/discord-model"], resolver = "2"
├─ rust-toolchain.toml        # channel = "stable"
├─ .gitignore                 # /target, etc.
├─ README.md                  # 한 문단 소개 + 구조 링크
├─ crates/
│  ├─ discord-model/
│  │  ├─ Cargo.toml
│  │  └─ src/
│  │     ├─ lib.rs
│  │     ├─ ids.rs
│  │     ├─ permissions.rs
│  │     ├─ entities.rs        # Guild/Role/Channel/PermissionOverwrite/Member
│  │     └─ state.rs           # GuildState
│  └─ domain/
│     ├─ Cargo.toml
│     └─ src/
│        ├─ lib.rs
│        ├─ onboarding.rs      # OnboardingMode, Visibility
│        └─ feature.rs         # Feature, VerificationPanel, ModerationRule, LoggingRule
└─ docs/
   ├─ discord_ai_control_plane_architecture_oci.md   # 루트에서 이동
   ├─ repo-structure.md                              # 전체 타깃 구조 확정본 (아래 6.2)
   └─ superpowers/specs/2026-07-09-core-domain-model-design.md   # 본 스펙
```

### 6.2 문서로만 확정할 전체 타깃 구조

`docs/repo-structure.md`에 아키텍처 문서 17장의 전체 monorepo 구조(services/*, 나머지 crates/*, proto/, infra/, migrations/, scripts/, .github/)를 **"계획된 구조"**로 기록한다. **지금 물리 생성하지 않는다.** 각 crate/service는 해당 Phase 구현 시점에 워크스페이스 members에 추가한다.

---

## 7. Cargo 의존성 (초기)

- `serde = { version = "1", features = ["derive"] }`
- `serde_with = "3"` (ID 문자열 직렬화용) — 또는 커스텀 impl로 대체 가능
- `bitflags = { version = "2", features = ["serde"] }`
- `thiserror = "1"` (필요 시)
- 워크스페이스 `[workspace.dependencies]`로 버전 공유 권장

---

## 8. 테스트 전략 (Phase 1)

- **serde 라운드트립**: 각 타입 `serialize → deserialize` 후 동일성(`PartialEq`) 검증. 특히 ID/permissions **문자열 직렬화** 형태 검증.
- **ID 타입 안전성**: `GuildId`와 `RoleId`가 섞이지 않음(컴파일 타임 보장 — 별도 테스트 불필요, 문서화만).
- **Permissions 비트 연산**: `contains`/`union`/`intersection` 기본 동작 + 대표 비트 값이 Discord 문서와 일치하는지 스팟 체크.
- **`GuildState` 픽스처**: 문서 예시(인증 채널/역할/권한) 시나리오를 표현하는 샘플 `GuildState`를 테스트 픽스처로 구성 (이후 Diff 테스트의 기반이 됨).

---

## 9. Phase 1 범위 경계 (명확히)

- ✅ **포함**: `discord-model`(IDs/Permissions/enums/5개 엔티티/GuildState) + `domain`(고수준 타입 **정의**) + 워크스페이스 스캐폴드 + 테스트
- ⚠️ **스켈레톤만**: `ModerationRule`, `LoggingRule`, `Feature`(variant는 정의하되 로직 없음)
- ❌ **제외**: Diff/Policy/Simulator/Operation Graph 로직, Desired State 스키마(logical key), Discord API 연동(bot-runtime), DB persistence 매핑, 매핑 레이어(고수준→저수준 컴파일)

---

## 10. Codex 핸드오프 유의사항

1. **`git init`부터** 시작 (현재 git repo 아님).
2. 루트의 `discord_ai_control_plane_architecture_oci.md`를 `docs/`로 이동.
3. Discord 권한 비트 값은 **공식 문서에서 검증** 후 채운다.
4. 이 스펙은 **이 머신 상태에 비의존적**이다(현재 머신엔 cargo 미설치). Codex 실행 환경에서 `cargo build`/`cargo test`/`cargo fmt`/`cargo clippy`가 통과해야 완료.
5. 구현 완료 기준: 워크스페이스가 컴파일되고, 8절 테스트가 통과하며, `cargo clippy` 경고 없음.
