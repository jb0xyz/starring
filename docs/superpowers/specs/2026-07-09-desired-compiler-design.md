# Desired Compiler 설계 스펙 (Phase 3)

- **작성일**: 2026-07-09
- **상태**: 확정 (구현 대기 — Codex 핸드오프 대상)
- **범위**: Phase 3 — `crates/desired-compiler` + 선행: discord-model Permissions 확장, desired-state Capability 확장·규칙6 완화
- **선행**: Phase 1(discord-model, domain), Phase 2(desired-state) 완료.

---

## 0. 목적

고수준 `DesiredState`를 Diff Engine이 **단순 비교**할 수 있는 정규화된 형태 `NormalizedDesiredState`로 낮춘다(normalize/lower). resolve(DB binding·Discord ID 해소)는 하지 않는다. 순수 Rust 변환 crate.

```
compile(&DesiredState) -> Result<NormalizedDesiredState, Vec<CompileError>>
```

---

## 1. 확정된 설계 결정

| # | 결정 | 내용 |
|---|---|---|
| D1 | **crate = `desired-compiler`**, 출력 = `NormalizedDesiredState` | resolve 아님 → "Normalized"(not Resolved) |
| D2 | **책임 = 하강/정규화만** | Capability→Permission, AccessIntent→overwrite, VerificationFeature→panel, raw escape 병합, dedupe, 문서 내부 충돌 |
| D3 | **Capability 텍스트 풍부셋 9종** | view/send/read_history/add_reactions/attach_files/embed_links/manage_messages/connect/speak. 관리자·moderation은 후속 privileged 계층 |
| D4 | **discord-model Permissions 확장** | READ_MESSAGE_HISTORY, ADD_REACTIONS, ATTACH_FILES, EMBED_LINKS, CONNECT, SPEAK 비트 추가 |
| D5 | **@everyone = `NormalizedTarget::Everyone`** | AccessIntent.everyone → 별도 타깃 변형 |
| D6 | **Phase 2 규칙 6 제거** | access↔raw 구조 검사(coarse)를 desired-state에서 제거. 정밀 충돌은 Compiler가 담당 |

---

## 2. 스코프 경계

| Phase 3 (`desired-compiler`) 담당 | 담당 아님 (후속) |
|---|---|
| Capability → Discord Permission 매핑 | 현재 상태(GuildState) 비교 → **Diff(P4)** |
| AccessIntent → NormalizedOverwrite 하강 | create/update/delete/no-op 판단 → **Diff(P4)** |
| VerificationFeature → NormalizedVerificationPanel | Discord ID binding 해소 → **binding registry** |
| raw escape 보존·병합·dedupe | DB/sqlx |
| 문서 내부 충돌(`allow & deny` 겹침) 감지 | Operation Graph → **P5** |
| mode/scope passthrough | risk/승인 판단 → **Policy(P6)** |
| | Simulator/Preview → **P7**, Discord API/AI |

---

## 3. Crate 구조 & 의존

```
desired-compiler ──▶ desired-state ──▶ discord-model
```
- `desired-compiler`는 diff/db/binding/operation-graph/policy/simulator/bot/ai에 **의존 금지**.
- 파일(예): `src/{lib.rs, normalized.rs, capability.rs, compile.rs, error.rs}`.

---

## 4. 선행 수정 (기존 crate)

### 4.1 `discord-model` Permissions 비트 추가 (D4)
기존 큐레이티드 셋에 아래 추가(공식 Discord 비트 — Codex 검증):
```
ADD_REACTIONS        = 1 << 6
EMBED_LINKS          = 1 << 14
ATTACH_FILES         = 1 << 15
READ_MESSAGE_HISTORY = 1 << 16
CONNECT              = 1 << 20
SPEAK                = 1 << 21
```

### 4.2 `desired-state` Capability 확장 (D3)
현재 `View/Send/React/ManageMessages/Connect/Speak` →
`View/Send/ReadHistory/AddReactions/AttachFiles/EmbedLinks/ManageMessages/Connect/Speak`.
- `React` → `AddReactions`로 개명(serde `add_reactions`). 기존 테스트는 `React` 미사용이라 안전.
- serde snake_case: view/send/read_history/add_reactions/attach_files/embed_links/manage_messages/connect/speak.

### 4.3 `desired-state` 규칙 6 제거 (D6)
`validate()`에서 `check_access_raw_conflict` 호출·메서드, `ValidationError::AccessRawConflict` variant, `access_raw_conflict_detected` 테스트 제거. validate()는 5규칙으로.

---

## 5. NormalizedDesiredState 타입 (`desired-compiler`)

desired-state의 `Identity`, `ResourceKey`, `DesiredStateMode`, `Scope`와 discord-model `Permissions`, `ChannelType` 재사용.

```
NormalizedDesiredState {
    mode: DesiredStateMode,                       // passthrough
    scope: Option<Scope>,                         // passthrough
    roles: Vec<NormalizedRole>,
    channels: Vec<NormalizedChannel>,
    verification_panels: Vec<NormalizedVerificationPanel>,
}

NormalizedRole {                                  // 사실상 passthrough (role 권한은 이미 raw)
    identity: Identity,
    name: Option<String>,
    permissions: Option<Permissions>,
}

NormalizedChannel {
    identity: Identity,
    name: Option<String>,
    channel_type: Option<ChannelType>,
    parent: Option<ResourceKey>,
    overwrites: Vec<NormalizedOverwrite>,         // access 하강 + raw 병합 결과. access/raw 필드는 사라짐
}

NormalizedOverwrite {
    target: NormalizedTarget,
    allow: Permissions,
    deny: Permissions,
}

enum NormalizedTarget { Everyone, Role(ResourceKey), Member(String) }   // Ord 파생(결정적 출력)

NormalizedVerificationPanel {                     // passthrough
    identity: Identity,
    channel: ResourceKey,
    grants_role: ResourceKey,
}
```

---

## 6. Compile 로직

### 6.1 Capability → Permission (D3 매핑)
```
view→VIEW_CHANNEL  send→SEND_MESSAGES  read_history→READ_MESSAGE_HISTORY
add_reactions→ADD_REACTIONS  attach_files→ATTACH_FILES  embed_links→EMBED_LINKS
manage_messages→MANAGE_MESSAGES  connect→CONNECT  speak→SPEAK
```
헬퍼: `capability_to_permission(Capability) -> Permissions`, `capabilities_to_permissions(&[Capability]) -> Permissions`(union fold).

### 6.2 채널별 하강·병합 (핵심)
채널마다 `BTreeMap<NormalizedTarget, (allow, deny)>`를 만든다(결정적 순서):
1. **AccessIntent 하강**:
   - `everyone` AccessGrant → target `Everyone`: allow = caps(allow)→perms, deny = caps(deny)→perms
   - `roles[key]` AccessGrant → target `Role(key)`
2. **raw_overwrites 병합** (리스트 순서대로, target별 (allow,deny)에 op 적용):
   - `Add`: `allow |= raw.allow; deny |= raw.deny`
   - `Remove`: `allow &= !raw.allow; deny &= !raw.deny`
   - `Replace`: `allow = raw.allow; deny = raw.deny` (덮어씀. 고위험이나 구조적으로 유지, risk는 Policy)
3. **정밀 충돌 감지**: 각 target에서 `allow & deny != empty` → `CompileError::PermissionConflict`
4. 맵을 `Vec<NormalizedOverwrite>`로(target 순서 정렬).

### 6.3 나머지
- roles → NormalizedRole passthrough. features의 Verification → NormalizedVerificationPanel. Moderation/Logging 스켈레톤은 이번엔 **무시**(normalized 출력에 미포함, 후속). mode/scope passthrough.
- `compile()`은 모든 채널을 처리하며 에러를 수집, 하나라도 있으면 `Err(Vec<CompileError>)`.

---

## 7. CompileError
```
enum CompileError {
    PermissionConflict { channel: String, target: String },
}
```
`thiserror`. 확장 여지 위해 향후 variant 추가 가능(지금은 1종).

---

## 8. 컨벤션 (Phase 1/2 승계)
- serde(JSON 문자열 ID·권한), 주석 없음([[starring-no-comments-convention]]), DB 무관, 파생 표준.
- 결정적 출력(BTreeMap/정렬) — 테스트 안정성.

---

## 9. Phase 3 범위 경계
- ✅ **완전 구현**: Permissions 확장, Capability 확장·규칙6 완화, NormalizedDesiredState 타입, capability 매핑, access 하강, raw 병합(add/remove/replace), 정밀 충돌, verification passthrough, mode/scope passthrough, CompileError, `compile()`
- ⚠️ **무시(미출력)**: Moderation/Logging feature(스켈레톤)
- ❌ **제외**: 현재 상태 비교, ID binding, DB, operation graph, policy, simulator, privileged/admin capability 계층

---

## 10. 테스트 전략 (사용자 제안 반영)
- capability 매핑: send→SEND_MESSAGES, read_history→READ_MESSAGE_HISTORY 등 스팟.
- visibility: everyone hidden → Everyone deny VIEW_CHANNEL. role visible → Role allow VIEW_CHANNEL.
- AccessIntent → overwrites 생성.
- raw escape 보존·병합(add/remove/replace), access+raw dedupe.
- **충돌**: 같은 target allow&deny 겹침 → CompileError.
- VerificationFeature → NormalizedVerificationPanel.
- **⭐ 인증 시나리오**: Phase 2의 `DesiredState` 인증 픽스처 → `compile()` 성공 → NormalizedDesiredState 검증(general 채널: Everyone deny VIEW, verified allow VIEW+SEND). Phase 4 Diff의 입력이 됨.

---

## 11. Codex 핸드오프 유의사항
1. Discord 신규 권한 비트는 공식 문서로 검증(4.1).
2. `React`→`AddReactions` 개명 + 3종 추가는 desired-state 수정. 규칙6 제거는 validate()·variant·테스트 함께.
3. 결정적 출력 위해 `NormalizedTarget`에 `Ord`, 내부 누적은 `BTreeMap` 사용.
4. Moderation/Logging feature는 이번엔 normalized 출력에서 제외(무시).
5. 완료 게이트: `cargo build/test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all -- --check`. 워크스페이스 members에 `crates/desired-compiler` 추가.
