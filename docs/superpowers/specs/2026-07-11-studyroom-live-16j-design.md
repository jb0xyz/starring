# Phase 16j — StudyRoom Live Smoke 설계 스펙

- **작성일**: 2026-07-11
- **상태**: 설계 확정 (구현 대기 — Codex 핸드오프 + Claude hands-on live)
- **범위**: Phase 16j — 그간 default-unsupported였던 mutation seam 4개를 automation-runtime에 twilight **실구현** + tools/interaction-smoke에 **StudyRoom 시나리오** + **실제 Discord live smoke**.
- **선행**: 16a~16i(코어 전부). 16b/16d의 대형 live 버전.

---

## ⚠️ 최상위 원칙 (불변)
AI 설치시점 설계자, Runtime 결정론, event-time LLM 금지. **automation-core/automation-state 무수정**(16j는 **live 엣지**만 — automation-runtime + tool). no_ai_gateway 가드 유지.

**목표(한 문장):** 실제 Discord에서 StudyRoom create-flow를 끝까지 실행하고, created 비공개 채널에 welcome/control 패널을 게시하며, 그 패널의 **정적 도움말 버튼까지 live interaction으로 응답**한다.

---

## 0. 범위

**포함:** `TwilightMutationAdapter`에 create_role/create_channel/upsert_overwrite/post_panel **실구현**(bot-runtime 패턴 재사용) · adapter에 `ruleset_key`(post_panel 버튼 custom_id 인코딩) · error.rs body classifier · twilight 변환 헬퍼 · tools StudyRoom 시나리오(RespondEphemeral-first + study_help 버튼) · **실제 Discord live 검증** · 토큰 위생.

**제외(→후속):** **defer_ephemeral/followup/edit-original**(응답 lifecycle) · **완료/실패 follow-up 메시징** · **dynamic join**(공개 허브/instance registry/동적 custom_id 컨텍스트) · DB 영속 · cleanup 자동화.

---

## 1. 3초 전략 — RespondEphemeral-first (핵심 결정)

Discord interaction은 **initial response를 3초 안에** 보내야 함(Gateway 수신이어도 HTTP callback). StudyRoom submit은 REST mutation 6개(create_role/channel/overwrite×2/grant/post_panel ≈ 2~4초) → **응답을 뒤에 두면 3초 초과 위험**.

**해결: RespondEphemeral을 룰의 첫 액션으로.** interaction은 즉시 ACK되고, 이후 mutation은 봇의 일반 REST(3초 제약 밖). **새 seam 0, 룰 순서만.** 메시지는 현재진행형("만들고 있어요")이라 정확.

> **명시:** Phase 16j uses RespondEphemeral-first as a smoke-safe ACK strategy. It does not provide completion/failure follow-up messaging. Failure는 로그/console로 확인. defer/followup/edit-original은 후속 response-lifecycle phase(16k).

---

## 2. TwilightMutationAdapter — 4 seam 실구현

`TwilightMutationAdapter<'a>`에 `ruleset_key: String` 필드 추가, `new(http, ruleset_key)`. **bot-runtime `TwilightDiscordAdapter` 패턴 재사용**(Phase 12에서 live 검증됨):

- **create_role**: `http.create_role(Id::new(guild.0)).name(&spec.name).await? .model().await?` → `RoleId(role.id.get())`. (권한 없음 — VIEW는 overwrite로.)
- **create_channel**: `http.create_guild_channel(Id::new(guild.0), &spec.name).await? .model().await?` → `ChannelId(channel.id.get())`. (기본 text 채널.)
- **upsert_overwrite**: `to_permission_overwrite(target, allow, deny)` + `http.update_channel_permission(Id::new(channel.0), &overwrite).await?`. (bot-runtime convert 그대로.)
- **post_panel**: buttons → `Component::ActionRow([Component::Button{custom_id: Some(encode_button(guild, self.ruleset_key, key)), label, style: Primary, ...}])` + `http.create_message(Id::new(channel.0)).content(&spec.content).components(&components).await? .model().await?` → `MessageId(message.id.get())`.

**post_panel 버튼 custom_id**: `custom_id::encode_button(guild, &self.ruleset_key, &button.key)` — 그래서 클릭 시 Gateway가 decode→button_click 룰로 라우팅(도움말 버튼 성립). tool의 install_panel과 동일한 Component 구성.

---

## 3. error.rs + 변환 헬퍼 + 라우팅 가드

- **error.rs**: `.model()`(body 역직렬화)용 **`classify_body_error(&DeserializeBodyError) -> AdapterError`** 추가(현재 classify_error만). **panic 금지 — AdapterErrorKind::Unknown 또는 BadRequest로만.** bot-runtime/error.rs 시그니처 대조.
- **twilight 변환**: `to_twilight_permissions(Permissions)`(from_bits_truncate) + `to_permission_overwrite(OverwriteTarget, allow, deny)` — bot-runtime/convert.rs 그대로 automation-runtime에 복사. **automation-runtime은 bot-runtime에 의존하지 않음**(검증된 패턴/변환 로직만 재사용; 중복 공통화 `discord-twilight-common`은 후속).
- **라우팅 가드(#3, 신규 — 실측: 현재 convert는 ruleset_key 미검사)**: `interaction_to_event(interaction, ruleset_key)`로 시그니처 확장 → decode 후 **`parsed.ruleset_key != ruleset_key`면 None**(다른 ruleset 버튼 무시) + **`parsed.guild_id != guild_id`면 None**. runner가 ruleset_key 전달. 그래야 studyroom_demo 봇이 다른 ruleset 버튼에 오작동 안 함.

---

## 4. gateway 배선
```rust
let mutation = TwilightMutationAdapter::new(&http, ruleset_key.clone());
```
(현재 `new(&http)` → ruleset_key 추가. adapter는 루프 전 1회 생성, guild는 per-call 파라미터라 단일 인스턴스 OK. runner/responder 무변경.)

---

## 5. StudyRoom 시나리오 (tools/interaction-smoke)

submit 룰을 지금의 RespondEphemeral 하나에서 **전체 StudyRoom**으로 교체(RespondEphemeral-first):
```
submit_study_modal (trigger: modal_submit create_study_modal):
  - { respond_ephemeral, content: "스터디룸 '${input.room_name}'을 만들고 있어요. 곧 새 채널이 나타납니다." }
  - { create_role,      key: study_member_role, name: "${input.room_name} 멤버" }
  - { create_channel,   key: study_channel,      name: "study-${input.room_name}" }
  - { upsert_overwrite, channel: {created: study_channel}, target: everyone,                        deny:  VIEW_CHANNEL }
  - { upsert_overwrite, channel: {created: study_channel}, target: {role: {created: study_member_role}}, allow: VIEW_CHANNEL }
  - { grant_role,       role: {created: study_member_role}, target: actor }
  - { post_panel,       channel: {created: study_channel}, content: "스터디룸 '${input.room_name}'이 생성되었습니다.", buttons: [{key: study_help, label: "도움말"}] }
```
+ study_help 정적 룰:
```
study_help_rule (trigger: button_click study_help):
  - { respond_ephemeral, content: "이 채널은 스터디 멤버만 볼 수 있는 비공개 스터디룸입니다. 공개 참가 기능은 다음 단계에서 연결됩니다." }
```
기존 open_study_modal(버튼→OpenModal) 룰 + 패널 설치("Create study room") 유지. bindings = `ResourceBindingMap::default()`(StudyRoom은 created ref만). env: DISCORD_TEST_TOKEN/GUILD/CHANNEL(선택 RULESET_KEY).

**키(짧게 — custom_id ≤100자 여유):** `ruleset_key = "studyroom_demo"`, 버튼 `create_study_room`/`study_help`, modal `create_study_modal`, created key `study_member_role`/`study_channel`. custom_id = `starring:<guild>:studyroom_demo:button:study_help`(≈50자, 안전). 시작 시 tool이 `validate(&ruleset, &bindings)` 호출로 사전 검증(2-pass 전역 button_keys — study_help 해소).

---

## 6. Live 검증 흐름 (Claude hands-on)
1. Claude가 재발급 토큰(env)으로 smoke runner 백그라운드 실행 → 허브 채널에 "Create study room" 패널 설치.
2. 사용자가 버튼 클릭 → 모달 → room_name 제출.
3. Bot: 즉시 "만들고 있어요" ephemeral → role/channel 생성 + @everyone deny + role allow + grant + welcome 패널 게시.
4. Claude가 로그 확인(`interaction ... -> Executed`) + REST로 생성된 role/channel/overwrite 검증(list roles/channels).
5. 사용자가 created 채널 진입 → welcome 패널 확인 → **도움말 클릭 → ephemeral 응답**.
6. Claude가 정리(생성된 role/channel 삭제).

성공 기준: 방 생성 완료 + 사용자가 비공개 채널 접근 + 도움말 버튼 live 응답 + 로그 Executed(모달 submit + 도움말 click).

---

## 7. 토큰 위생
- 노출된 토큰 **폐기(Developer Portal Reset)**, 새 토큰만 사용.
- token은 **절대 print 금지** · 로그 env redaction · **토큰 파일 커밋 금지**(.gitignore 확인) · smoke tool은 `DISCORD_TEST_TOKEN`을 **env로만** 읽음.

---

## 8. 테스트 (unit + live)

**unit (automation-runtime):** 라우팅 가드를 pure helper로 추출해 검증(twilight Interaction 구성 없이):
- `matches_context(parsed, ruleset_key, guild) -> bool` 헬퍼 → interaction_to_event가 사용.
  1. ruleset_key 일치 + guild 일치 → true.
  2. **ruleset_key mismatch → false**(다른 ruleset 버튼 무시 — 핵심).
  3. guild mismatch → false.
- custom_id encode/decode(ruleset_key 포함, study_help 복원)는 **기존 custom_id.rs 테스트로 커버**. no_ai_gateway 가드 유지.

**live (Claude hands-on — twilight 실호출이라 unit 부적합, 16b/16d처럼):**
- post_panel이 ruleset_key 포함 custom_id로 버튼 게시 + create_message 결과 MessageId 기록.
- create_role/create_channel/upsert_overwrite 실제 실행.
- 도움말 버튼 클릭 → 라우팅 → ephemeral.
- (body/http error → AdapterError 변환은 classify_body_error 존재로 보장; DeserializeBodyError 구성이 어려우면 unit 생략, live 관찰.)

## 9. 하지 않는 것 (Forbidden — 16k+)
defer_ephemeral/followup/edit-original · 완료/실패 메시징 · dynamic join(공개 허브/instance registry/동적 custom_id) · DB · cleanup 자동화 · action failure reporting. **cleanup: live 후 Claude가 API로 guild roles/channels 조회 → 생성된 `study-*` 채널·`* 멤버` 역할 삭제(tool/handle_event 무수정 — 생성 id 로깅 안 함).**

---

## 10. 로드맵
```
16i✅ PostPanel   16j▶ StudyRoom live smoke (이 스펙 — create-flow 실제 Discord)
16k  Deferred response lifecycle (defer_ephemeral + followup + edit-original + 실패 리포팅)
후속 트랙: Dynamic instance buttons → Public join registry → join live
```

---

## 11. Codex 핸드오프 (개요)
1. automation-runtime: error.rs(classify_body_error) + 변환 헬퍼(to_twilight_permissions/to_permission_overwrite) + mutation.rs(ruleset_key 필드 + 4 seam 실구현, post_panel은 encode_button + Component 구성) + gateway.rs(new에 ruleset_key.clone()) + **convert.rs(interaction_to_event에 ruleset_key 인자 + matches_context 가드 + 테스트) + runner.rs(interaction_to_event에 ruleset_key 전달)**.
2. tools/interaction-smoke: StudyRoom 시나리오(RespondEphemeral-first 7액션 + study_help 룰).
3. **automation-core/automation-state 무수정** 확인. 주석 없음. 게이트 build/test/clippy(-D warnings)/fmt.
4. **Claude가 live 실행**(재발급 토큰 env, hands-on 검증 + cleanup). Codex는 코드까지, live는 Claude.

## 최종 정리
16j = StudyRoom live smoke. TwilightMutationAdapter가 create_role/channel/upsert_overwrite/post_panel을 bot-runtime 패턴으로 실구현(+ruleset_key로 post_panel 버튼 custom_id 인코딩), tool이 RespondEphemeral-first StudyRoom 시나리오(+study_help 정적 버튼)를 돌림. 3초는 RespondEphemeral-first로 회피(새 seam 0), 완료/실패 메시징·defer는 16k. 실제 Discord에서 방 생성→비공개 채널→환영 패널→도움말 버튼 응답까지 검증. 토큰 재발급 후 env only.
