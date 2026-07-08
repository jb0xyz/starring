# Desired State Schema 설계 스펙

- **작성일**: 2026-07-09
- **상태**: 확정 (구현 대기 — Codex 핸드오프 대상)
- **범위**: Phase 2 — `crates/desired-state` (선언형 목표 상태 스키마 + 검증)
- **선행**: Phase 1 Core Domain Model 완료(`discord-model`, `domain`). 아키텍처 문서 §6.2, §36.

---

## 0. 목적

AI/사용자가 표현하는 **"목표 상태(Desired State)"** 를 담는 타입 스키마와 그 **검증**을 만든다. 이 crate는 **순수 데이터 스키마 + 검증**이며, 외부 상태나 실행 로직은 없다(fixture로 테스트 가능). Compiler·binding·Diff·Simulator는 전부 후속 crate다.

---

## 1. 확정된 설계 결정

| # | 결정 | 내용 |
|---|---|---|
| D1 | **모드 기반 선언 의미론** | `mode`: `patch`(기본)/`scoped_authoritative`/`full_authoritative`. AI 출력은 기본 patch(안전한 부분 목표), scope 안에서만 권위로 승격 |
| D2 | **정체성 4개념 분리** | `key`(문서 내 논리 정체성) / `match`(현재 리소스 탐색 전략) / `ownership`(관리 강도) / *binding*(key↔ID 지속 연결, **별도 DB 컴포넌트 — 이 crate 밖**) |
| D3 | **이름 ≠ 정체성** | 문서 내 참조는 전부 `key`로. 기존 리소스도 `key`+`match`로 선언 후 참조 |
| D4 | **고수준 intent 우선 + raw escape hatch** | 기본은 고수준 access intent, 세밀 제어는 raw `permission_overwrites`. raw는 add/remove 기본·replace 고위험 |
| D5 | **별도 타입** | `discord-model` 재사용 안 함. 공유 primitive(`Permissions`, `ChannelType`)만 사용. 의존 `desired-state → discord-model` |
| D6 | **ownership 3레벨** | `managed`(앱 소유·자유 수정/삭제) / `adopted`(기존 인수·관리하되 충돌 주의) / `referenced`(참조만·수정 금지) |
| D7 | **삭제 표현** | patch 모드에서 리소스별 `state: absent` |

---

## 2. 스코프 경계 (계층 모델)

주어진 계층: `Feature Intent → Access Policy → Capability → Raw Permission Patch → Discord Operation`

| 이 crate (`desired-state`) | 후속 (별도 crate/Phase) |
|---|---|
| `mode` + `scope` 선언 | Diff Engine (모드별 재조정) |
| 리소스 intent (`key`/`match`/`ownership`/`state`) | **binding registry (DB)** |
| 고수준 access intent (visibility/capability) | **Operation Graph Compiler** (intent→raw 하강) |
| raw escape hatch | conflict resolution / Simulator / Preview |
| **스키마 검증**(`validate()`) | Executor / Bot Runtime |

**핵심: 이 crate는 "무엇을 원하는가"의 표현과 그 정합성 검증까지만.** "어떻게 실현하는가"는 전부 밖.

---

## 3. Crate 구조 & 의존

```
desired-state ──depends on──▶ discord-model   (Permissions, ChannelType 등 primitive만)
```
- `domain` crate에는 의존하지 않는다 (desired-state는 key 기반 자체 고수준 타입을 가짐. `domain::Visibility`는 RoleId 기반이라 재사용 불가).
- 파일 구성(예): `src/{lib.rs, mode.rs, identity.rs, role.rs, channel.rs, access.rs, feature.rs, validate.rs}`.

---

## 4. 스키마 타입 (개략 — 실제 코드는 Codex/플랜)

### 4.1 Root

```
DesiredState {
    mode: DesiredStateMode,        // 기본 Patch
    scope: Option<Scope>,          // ScopedAuthoritative일 때만 의미
    roles:    Vec<RoleIntent>,
    channels: Vec<ChannelIntent>,
    features: Vec<FeatureIntent>,
}

enum DesiredStateMode { Patch, ScopedAuthoritative, FullAuthoritative }   // 기본 Patch

// scoped_authoritative의 "관리 범위" 선언. 해석은 Diff의 몫, 여기선 담기만.
Scope {
    roles:    Option<ResourceScope>,
    channels: Option<ResourceScope>,
}
enum ResourceScope { All, Keys(Vec<ResourceKey>), NamePrefix(String) }
```

### 4.2 정체성 (모든 intent 공통)

```
ResourceKey(String)   // 문서 내 유일 논리 정체성 + 참조 핸들

// 각 *Intent에 #[serde(flatten)]로 공유 (플랜이 라운드트립 검증; flatten이 문제되면 직접 필드로 폴백)
Identity {
    key: ResourceKey,
    match_by: MatchStrategy,   // serde "match" (Rust 예약어 회피 위해 필드명 match_by)
    ownership: Ownership,      // 기본 Managed
    state: ResourceState,      // 기본 Present
}

enum MatchStrategy {
    ByName,               // intent의 name으로 현재 리소스 탐색 (기본)
    ByExplicitId(String), // 사용자가 명시한 기존 snowflake(문자열)
}                         // #[non_exhaustive] — 향후 ByAttributes 등

enum Ownership { Managed, Adopted, Referenced }   // 기본 Managed
enum ResourceState { Present, Absent }            // 기본 Present
```

### 4.3 RoleIntent

```
RoleIntent {
    // + Identity (key/match/ownership/state)
    name: Option<String>,               // 부분 명세: None = 주장 안 함
    permissions: Option<Permissions>,   // discord-model::Permissions. 역할 기본 권한(저수준, 역할엔 고수준 없음)
}
```
- position(계층)은 **Phase 2 제외** — 시스템/Compiler가 배치 결정.

### 4.4 ChannelIntent + 고수준 access + raw escape

```
ChannelIntent {
    // + Identity
    name: Option<String>,
    channel_type: Option<ChannelType>,    // discord-model
    parent: Option<ResourceKey>,          // 카테고리 (key 참조)
    access: Option<AccessIntent>,         // 고수준 (기본 경로)
    raw_overwrites: Option<Vec<PermissionOverwriteIntent>>,   // escape hatch
}

// 고수준: "누가(key) 무엇을(capability) 할 수 있나"
AccessIntent {
    everyone: Option<AccessGrant>,
    roles: BTreeMap<ResourceKey, AccessGrant>,   // 역할(key)별
}
AccessGrant { allow: Vec<Capability>, deny: Vec<Capability> }

enum Capability { View, Send, React, ManageMessages, Connect, Speak }   // 고수준 curated, #[non_exhaustive]

// raw escape hatch (저수준, discord-model::Permissions 사용)
PermissionOverwriteIntent {
    target: OverwriteTargetIntent,
    op: OverwriteOp,          // 기본 Add
    allow: Permissions,
    deny: Permissions,
}
enum OverwriteTargetIntent { Role(ResourceKey), Member(String) }   // Member = user snowflake(문자열)
enum OverwriteOp { Add, Remove, Replace }   // Replace = 고위험(Diff/Policy가 별도 취급)
```

### 4.5 FeatureIntent

```
enum FeatureIntent {
    Verification(VerificationIntent),
    Moderation(ModerationIntent),   // 스켈레톤
    Logging(LoggingIntent),         // 스켈레톤
}

VerificationIntent {
    // + Identity (기본 ownership Managed)
    channel: ResourceKey,       // 인증 채널 (key)
    grants_role: ResourceKey,   // 인증 시 부여 역할 (key)
    // 버튼/메시지 구성은 후속
}

ModerationIntent {}   // 스켈레톤
LoggingIntent {}      // 스켈레톤
```

---

## 5. 검증 레이어 `DesiredState::validate()`

핵심 로직. `Result<(), Vec<ValidationError>>` (모든 위반 수집). `thiserror` 기반 `ValidationError`.

규칙:
1. **key 유일성** — 모든 리소스(role/channel/feature)의 `key`가 문서 전체에서 유일.
2. **참조 무결성** — 모든 key 참조(`channel.parent`, `access.roles`의 key, `feature.channel`/`grants_role`, `overwrite.target`의 Role key)가 문서 내 선언된 key로 해소돼야 함.
3. **mode ↔ scope 정합** — `scope`는 `mode == ScopedAuthoritative`일 때만 허용. 그 외 mode에서 scope 존재 시 에러. ScopedAuthoritative인데 scope 없으면 에러.
4. **ownership ↔ state/변경 정합** — `Referenced`는 수정 불가: `state`는 `Present`여야 하고 변경 필드(name/permissions/access/raw)는 모두 None/빈값. `Absent`는 `Managed`/`Adopted`에서만.
5. **match ↔ name 정합** — `match_by == ByName`이면 `name`이 있어야 함(탐색 기준).
6. **access ↔ raw 충돌** — 같은 채널에서 `access`와 `raw_overwrites`가 **동일 대상(target)** 을 동시에 건드리면 conflict → 에러(D4의 충돌 규칙을 스키마 수준에서 조기 차단).

---

## 6. 공통 컨벤션 (Phase 1 승계)

- serde 기반, **JSON 캐노니컬**(테스트는 serde_json). YAML은 후속(serde_yaml) 선택.
- ID·`Permissions`는 JSON **문자열**(Phase 1 규칙 그대로, discord-model 타입 사용).
- enum 태깅: snake_case. sum type은 적절히 내부/인접 태깅(플랜에서 확정).
- **주석 없음**([[starring-no-comments-convention]]).
- **DB/sqlx 무관**. serde만.
- 파생: `Clone, Debug, PartialEq, Eq, Serialize, Deserialize` (+ 필요 시 `Default`).

---

## 7. Phase 2 범위 경계

- ✅ **완전 구현**: `DesiredState`/`Mode`/`Scope`, 정체성(key/match/ownership/state), `RoleIntent`, `ChannelIntent`(+`AccessIntent`+`Capability`+raw escape), `VerificationIntent`, `validate()` 6규칙
- ⚠️ **스켈레톤**: `ModerationIntent`, `LoggingIntent`
- ❌ **제외**: Compiler(intent→raw 하강), binding registry, Diff, conflict resolution, Simulator, position/계층 배치, YAML 로더

---

## 8. 테스트 전략

- **serde 라운드트립**: 각 타입 + 전체 `DesiredState`. 문자열 ID·권한 형태 검증.
- **default 동작**: mode 없으면 Patch, ownership 없으면 Managed, state 없으면 Present.
- **검증 규칙별 테스트**: 6개 규칙 각각 위반 케이스 + 통과 케이스.
- **⭐ 인증 시나리오 픽스처**: 문서 §6.2 예시("신규 유저는 인증 채널만...")를 `DesiredState`(mode=patch, `verified_member` role + 인증/일반 channel + verification feature)로 구성 → `validate()` 통과 → serde 라운드트립. Phase 1의 GuildState 픽스처와 짝이 되는, 이후 Diff 테스트의 입력.

---

## 9. Codex 핸드오프 유의사항

1. `match`는 Rust 예약어 → 필드명 `match_by`, `#[serde(rename = "match")]`.
2. `AccessIntent.roles`, `Scope` 등 맵/컬렉션의 key가 `ResourceKey`(String wrapper)면 JSON 객체 키로 직렬화됨(Phase 1의 RoleId 맵키 방식과 동일 — 문자열).
3. `Capability`/`Permissions`는 **다른 계층**이다. Capability(고수준)를 Permissions(저수준)로 변환하지 말 것 — 그 하강은 후속 Compiler의 몫. 이 crate는 둘을 각각 담기만.
4. 완료 기준: `cargo build/test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all -- --check` 전부 통과. 워크스페이스 members에 `crates/desired-state` 추가.
