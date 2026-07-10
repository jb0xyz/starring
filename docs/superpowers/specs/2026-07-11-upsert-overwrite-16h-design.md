# Phase 16h — UpsertOverwrite + ChannelRef Core 설계 스펙

- **작성일**: 2026-07-11
- **상태**: 설계 확정 (구현 대기 — Codex 핸드오프)
- **범위**: Phase 16h — created channel을 참조해 permission overwrite로 **비공개 채널 권한**을 조립. 순수 Mock.
- **선행**: 16g(created linking: RoleRef, created_roles binding). live/PostPanel은 16i/16j.

---

## ⚠️ 최상위 원칙 (불변)
AI 설치시점 설계자, Runtime 결정론, event-time LLM 금지. **16h 순수 코어(Mock)** — automation-core/state만. **automation-runtime 무수정**(upsert_overwrite seam default-unsupported).

**목표:** created channel + created/existing role을 조합해 overwrite로 비공개 채널 구성. **UpsertOverwrite는 독립 primitive action**(CreateChannel 번들 아님).

---

## 0. 범위

**포함:** `ActionSpec::UpsertOverwrite` · `ChannelRef{Existing, Created}` · `OverwriteTargetSpec{Everyone, Role(RoleRef)}` · allow/deny `Permissions` · created channel binding **소비**(16g에서 미룸) · created role binding을 target에서 소비 · Mock upsert_overwrite 기록 · overwrite policy finding.

**제외(→16i/16j/후속):** PostPanel · join button · live · DB · **member target overwrite**(abuse surface) · category sync · permission simulator 확장 · rollback live · lifecycle/cleanup · allow/deny **이름배열 serde**(비트로 우선).

---

## 1. 왜 독립 action인가
CreateChannel에 overwrites 번들 = 처음엔 간단하나 곧 한계(기존 채널 overwrite 불가, ChannelRef 지연, overwrite policy 독립분석 어려움, vocabulary가 번들로 굳음). Layer 2 = 번들 아니라 **primitive 조립 엔진** — CreateChannel/UpsertOverwrite/GrantRole/PostPanel 각각 독립이어야 스터디룸·티켓·파티방·등업방·비공개문의방을 같은 엔진으로 조립.

---

## 2. 타입

### automation-state (스키마)
```rust
#[serde(deny_unknown_fields)]                // 실증: {created:x, extra:y} REJECT
pub struct CreatedRef { pub created: String } // RoleRef/ChannelRef가 공유

pub enum RoleRef {                           // 16g — Created를 CreatedRef로 migrate(JSON 무변)
    Existing(ResourceKey),                   // JSON: "verified_member"
    Created(CreatedRef),                     // JSON: { "created": "study_member_role" }
}

pub enum ChannelRef {                        // untagged (RoleRef와 완전 대칭)
    Existing(ResourceKey),                   // JSON: "general" (bare string)
    Created(CreatedRef),                     // JSON: { "created": "study_channel" }
}

pub enum OverwriteTargetSpec {               // externally-tagged (실증 완료)
    Everyone,                                // JSON: "everyone" (unit variant)
    Role(RoleRef),                           // JSON: { "role": { "created": "..." } } 또는 { "role": "existing_key" }
}

enum ActionSpec {
    ..., CreateChannel {..}, CreateRole {..},
    UpsertOverwrite {
        channel: ChannelRef,
        target: OverwriteTargetSpec,
        #[serde(default)] allow: Permissions,
        #[serde(default)] deny: Permissions,
    },
}
```
> **serde 형태 (실증 완료):** `CreatedRef{created}`(deny_unknown_fields)를 RoleRef/ChannelRef가 공유 — `{created:x, extra:y}` **REJECT**(원칙 #2). existing=bare string, created=`{created:x}`(JSON은 16g와 동일 — RoleRef는 struct변형→tuple(CreatedRef) 리팩터, wire 무변). OverwriteTargetSpec=externally-tagged(`"everyone"` / `{role:<RoleRef>}`). 내부태그 ActionSpec 중첩 + outer unknown REJECT + roundtrip 클린 검증됨. allow/deny=`discord_model::Permissions`(비트 serde) — `["view_channel"]` 이름배열 DSL은 **후속 ergonomic layer**로 분리(16h는 비트, 테스트는 Rust `Permissions::VIEW_CHANNEL`). **serde roundtrip 테스트로 DSL 모양 고정**(원칙 #1).

### automation-core
```rust
pub enum PlannedChannel { Resolved(ChannelId), Created(String) }
pub enum PlannedOverwriteTarget { Everyone, Role(PlannedRole) }   // PlannedRole은 16g

enum PlannedAction {
    ..., 
    UpsertOverwrite {
        channel: PlannedChannel,
        target: PlannedOverwriteTarget,
        allow: Permissions,
        deny: Permissions,
    },
}

struct RuntimeBindings {
    created_roles: BTreeMap<String, RoleId>,      // 16g
    created_channels: BTreeMap<String, ChannelId>, // 16h 추가(CreateChannel이 채움, UpsertOverwrite가 소비)
}

// seam (default-unsupported)
async fn upsert_overwrite(&self, guild: GuildId, channel: ChannelId, target: <overwrite target>, allow: Permissions, deny: Permissions) -> Result<(), AdapterError>;
```
seam의 target 타입은 role overwrite(16h는 Everyone/Role 둘 다 role) — `discord_model::OverwriteTarget` 재사용 여부는 플랜에서 대조. **@everyone = RoleId(guild_id)**(Layer 1 diff와 동일 해석).

---

## 3. interpret / run 해소 (16g 패턴 확장)

- **interpret**: ChannelRef::Existing(key)→`bindings.channel_bindings` 해소→`PlannedChannel::Resolved`(미해소 None) / Created→보존. target Everyone→Everyone / Role(RoleRef)→RoleRef 해소(Existing→Resolved / Created→Created).
- **run**(RuntimeBindings): CreateChannel이 `created_channels[key]=id` 채움(16g에선 미소비였음). UpsertOverwrite: channel 해소(Resolved / Created→created_channels), target 해소(Everyone→`RoleId(guild_id)` / Role→Resolved 또는 created_roles), `upsert_overwrite(guild, channel_id, target, allow, deny)`.

fail-fast: 앞 action 실패 시 뒤 미실행.

---

## 4. validate (order 기반, 16g 확장)

1. UpsertOverwrite.channel Created → 앞선 CreateChannel key여야(아니면 UnknownCreatedChannelRef; forward 포함).
2. channel Existing → `channel_bindings` 해소 가능(UnknownChannelRef).
3. target Role Created → 앞선 CreateRole key여야.
4. target Role Existing → `role_bindings` 해소 가능.
5. ChannelRef::Created가 CreateRole key 참조 → TypeMismatch.
6. target RoleRef::Created가 CreateChannel key 참조 → TypeMismatch(16g CreatedRoleRefTypeMismatch 재사용/확장).
7. **allow ∩ deny ≠ ∅ → validate error**(구조적 모순).
8. **allow, deny 둘 다 empty → validate error**(no-op 금지).
9. Created ref가 자기보다 뒤 action 참조 → 실패(forward).

(order 추적 map은 16g의 `created: key→Role|Channel` 그대로 사용.)

---

## 5. policy (최소)
- `EveryoneOverwrite { rule }` — @everyone overwrite 변경.
- `PrivilegedOverwriteAllow { rule }` — allow가 privileged_mask(ADMIN/MANAGE_*/BAN/KICK 등) 포함 → high-risk.
스터디룸은 VIEW_CHANNEL만 필요 → 테스트 fixture는 VIEW_CHANNEL 중심(privileged 테스트만 예외). PolicyFinding(enum)에 2변형 추가.

---

## 6. StudyRoom 비공개 채널 (16h 완성분)
```
- { create_role,   key: study_member_role, name: "${input.room_name} 멤버" }
- { create_channel, key: study_channel,     name: "study-${input.room_name}" }
- { upsert_overwrite, channel: {created: study_channel}, target: everyone,                       deny: [view_channel] }
- { upsert_overwrite, channel: {created: study_channel}, target: {role: {created: study_member_role}}, allow: [view_channel] }
- { grant_role, role: {created: study_member_role}, target: actor }
```
증명: created channel/role 참조로 비공개 채널 권한 조립.

---

## 7. 테스트 (15 + serde 고정)

**serde 고정(automation-state, 원칙 #1·#2 — DSL 모양 확정):**
- S1. `ChannelRef::Existing("general")` ↔ `"general"`; `ChannelRef::Created(CreatedRef{created:"study_channel"})` ↔ `{"created":"study_channel"}`.
- S2. `OverwriteTargetSpec::Everyone` ↔ `"everyone"`; `Role(RoleRef::Existing("verified_member"))` ↔ `{"role":"verified_member"}`; `Role(RoleRef::Created(..))` ↔ `{"role":{"created":"study_member_role"}}`.
- S3. `{"created":"x","extra":"y"}` (channel/role created) → **역직렬화 실패**(CreatedRef deny_unknown_fields).
- S4. UpsertOverwrite action에 unknown 필드 → 실패(ActionSpec deny_unknown_fields).

**core(automation-core):**
1. UpsertOverwrite created channel → created ChannelId 해소.
2. Everyone target → RoleId(guild_id).
3. Role created target → created RoleId.
4. Mock upsert_overwrite 기록.
5. **비공개 스터디룸 call 순서**: create_role→create_channel→upsert everyone deny→upsert role allow→grant_role.
6. channel ref 없는 created key → validate 실패.
7. channel ref가 CreateRole key → validate 실패(type).
8. target role ref 없는 created key → validate 실패.
9. target role ref가 CreateChannel key → validate 실패(type).
10. forward ref → validate 실패.
11. allow∩deny 겹침 → validate 실패.
12. allow+deny 둘 다 empty → validate 실패.
13. @everyone overwrite → policy finding.
14. privileged allow → policy finding.
15. created channel id는 뒤 action이 참조 가능하나 문자열 템플릿 접근 불가(`${created.x.id}` 자동거부, 기존).

핵심 = **5번**(스터디룸 비공개 시퀀스가 Mock에서 정확히).

---

## 8. seam
`DiscordMutationAdapter::upsert_overwrite` default-unsupported / Mock 기록 / live 16j. grant_role/create_* seam 불변. **automation-runtime 무수정.**

---

## 9. 로드맵
```
16g✅ Created linking   16h▶ UpsertOverwrite+ChannelRef (이 스펙)   16i PostPanel(ChannelRef 재사용)   16j StudyRoom live
```

---

## 10. Codex 핸드오프 (개요)
1. automation-state: `CreatedRef`(deny_unknown_fields, 공유) + RoleRef.Created를 CreatedRef로 migrate(JSON 무변, 기존 RoleRef::Created 구성 mechanical 갱신) + ChannelRef + OverwriteTargetSpec + ActionSpec::UpsertOverwrite + serde 고정 테스트(S1~S4).
2. automation-core: plan(PlannedChannel/PlannedOverwriteTarget/PlannedAction), adapter(upsert_overwrite default-unsupported + target 타입), mock(upsert_overwrite 기록 + MutationCall), interpret(Existing 해소/Created 보존), run(created_channels 채움 + overwrite 해소·실행), validate(channel/target ref + overlap/empty), policy(EveryoneOverwrite/PrivilegedOverwriteAllow).
3. **automation-runtime 무수정** 확인.
4. 주석 없음. 게이트 build/test/clippy(-D warnings)/fmt. push. live/토큰 없음.

## 최종 정리
16h = UpsertOverwrite 독립 primitive + ChannelRef. created channel/role을 참조해 @everyone deny + role allow로 비공개 채널 조립. interpret Existing 해소/run Created 해소(16g 패턴), validate order+overlap/empty, policy overwrite 위험 표시, @everyone=guild role. 16i(PostPanel)이 ChannelRef 재사용. StudyRoom 서버상태 변경 거의 완성.
