# Phase 16b — Layer 2 Live Smoke (Gateway) 설계 스펙

- **작성일**: 2026-07-10
- **상태**: 설계 확정 (구현 대기 — Codex 핸드오프)
- **범위**: Phase 16b — 실제 Discord 버튼 클릭 → automation-core → 실제 역할 지급 + ephemeral 응답
- **선행**: Phase 16a 완료(automation-state/automation-core, 184 테스트). 아키텍처: `2026-07-10-interaction-rule-plane-design.md`

---

## ⚠️ 최상위 원칙 (Layer 2 안전 불변식 — live에서도 유지)

> **AI는 interaction rule을 설치 시점에만 설계한다. Runtime은 저장된 rule만 결정론적으로 실행한다. 이벤트 발생 시점에 LLM을 호출하지 않는다.**
> `Runtime must not call LLM during interaction handling.`

live edge인 `automation-runtime`도 **ai-gateway에 의존하지 않는다**(no_ai_gateway 가드). 불변식은 실제 런타임에서 가장 중요하다.

---

## 0. 목적

**목표 문장:** 실제 Discord 버튼 클릭이 저장된 Interaction Rule을 발동시키고, automation-core가 결정론적으로 해석한 뒤, 실제 역할 지급과 ephemeral 응답까지 완료한다.

Layer 1이 Phase 12에서 live로 증명됐듯, Layer 2를 live로 증명한다. **"상시 서비스 완성"이 아니라 얇은 live smoke.**

---

## 1. 범위: Gateway 기반 얇은 smoke

Discord interaction 수신은 **Gateway event** 또는 **outgoing HTTP webhook** 둘 중 하나이며 상호 배타적이다. 16b는 **Gateway**로 간다:

| | Gateway (16b) | HTTP webhook (후속) |
|---|---|---|
| 실행 | 봇 토큰으로 로컬 즉시 | public HTTPS endpoint 필요 |
| 설정 | 없음 | Portal endpoint URL + signature 검증 |
| 운영 | 없음 | 배포/터널(ngrok) |
| smoke 적합성 | ✅ | 과함 |

**응답은 어느 방식이든 HTTP callback으로 보낸다** — Gateway로 받아도 응답은 `POST /interactions/{id}/{token}/callback`.

---

## 2. 확정 결정 (D1~D9)

| # | 결정 |
|---|---|
| D1 | Layer 2 live edge는 **새 `automation-runtime` crate**에 둔다 |
| D2 | `bot-runtime`은 Layer 1 Discord REST 실행 adapter로 유지 (역할 안 흐림) |
| D3 | `tools/interaction-smoke`는 얇은 수동 runner만 |
| D4 | `automation-runtime`은 automation-core를 수정하지 않고 **seam 구현만** 제공 |
| D5 | **Gateway 방식**으로 InteractionCreate 수신 |
| D6 | HTTP outgoing webhook endpoint는 production/backend phase로 미룸 |
| D7 | **InteractionResponder는 interaction별로 생성**(id/token bind) |
| D8 | **custom_id encode/decode는 automation-runtime 책임** |
| D9 | runtime crate는 ai-gateway에 의존하지 않는다 |

---

## 3. 왜 D7이 16a를 안 건드리고 성립하나 (핵심)

Discord interaction 응답은 그 인터랙션의 `id`+`token`으로 `/interactions/{id}/{token}/callback`에 POST해야 한다. 그런데 16a `InteractionResponder::respond_ephemeral(&self, content)` 시그니처엔 token이 없다. 해결:

```
InteractionCreate 수신
→ interaction.id / interaction.token 추출
→ 그 이벤트 전용 TwilightInteractionResponder { http, application_id, interaction_id, interaction_token } 생성
→ automation_core::handle_event(event, ruleset, bindings, &mutation, &responder)
→ RespondEphemeral 실행 시 responder가 저장된 token으로 callback POST
```

**16a trait을 한 글자도 안 바꾼다.** token은 responder 생성 시점에 bind되고, `respond_ephemeral(content)`는 그걸 쓴다. seam 분리(mutation=REST vs responder=interaction token)가 정확히 이걸 위한 설계였다.

**응답 타이밍(3초):** initial callback은 3초, token은 follow-up용으로 15분 유효. 16b smoke는 `grant_role`(단일 REST, ~수백ms) → `respond_ephemeral` 순서로 3초 안에 끝나는 fast path. 느려지면 `defer_response`(ACK 먼저)+followup으로 가지만 **그건 16b+ 후속**이고 smoke엔 fast path로 충분.

---

## 4. 크레이트 구조

```
crates/
  automation-runtime/          (신규 — Layer 2 live edge)
    Cargo.toml
    src/
      lib.rs
      custom_id.rs             encode/decode: starring:<guild_id>:<ruleset_key>:<button_key>
      convert.rs               Twilight InteractionCreate → RuntimeEvent::ButtonClick
      mutation.rs              TwilightMutationAdapter: grant_role = add_guild_member_role
      responder.rs             TwilightInteractionResponder(interaction별): respond_ephemeral = callback POST
      gateway.rs               Twilight shard connect / Ready→application_id / InteractionCreate 대기
      runner.rs                RuleSet + ResourceBindingMap + event → handle_event 배선
      error.rs                 RuntimeEdgeError (twilight/parse 분류)
    tests/
      no_ai_gateway.rs         매니페스트 ai-gateway 문자열 차단
      custom_id.rs             encode/decode roundtrip + 거부 (순수, 토큰 없음)

tools/
  interaction-smoke/           (신규 — 얇은 수동 runner)
    Cargo.toml
    src/main.rs                env → fixture ruleset → binding → 패널 설치 → gateway 시작 → 로그
```

역할 경계:
- **automation-state** = 스키마 / **automation-core** = validate·interpret·run·Mock(순수) / **automation-runtime** = 실제 Discord interaction edge / **interaction-smoke** = env 기반 수동 실행기.
- **automation-core는 button_key만 안다.** Discord custom_id 포맷·guild/ruleset encoding은 automation-runtime 책임(코어가 Discord 디테일을 모르게 유지).

---

## 5. 의존성 방향

```
automation-runtime
  → automation-state
  → automation-core
  → discord-model
  → resource-resolution
  → twilight-gateway / twilight-http / twilight-model (0.17, bot-runtime과 버전 일치)
  → tokio, rustls (live)

tools/interaction-smoke
  → automation-runtime, automation-state, automation-core, discord-model, resource-resolution
  → tokio, twilight-*
```

**금지 (automation-runtime):** ai-gateway / db / backend-api / nats. → no_ai_gateway 가드 테스트로 강제.

**bot-runtime 중복 허용:** twilight HTTP 셋업 약간 중복돼도 지금은 경계 > DRY. 공통화(`discord-twilight-common`: id 변환·error 분류·client factory)는 후속이고 16b에서 만들지 않는다.

---

## 6. custom_id 설계 (D8)

실제 버튼 custom_id에 Starring prefix를 붙여 서버/버전 충돌을 방지:
```
starring:<guild_id>:<ruleset_key>:<button_key>
예) starring:1524810437118525551:demo_verify:verify_button
```
(Discord custom_id 최대 100자 — 형식 준수.) automation-runtime `custom_id.rs`:
- `encode(guild_id, ruleset_key, button_key) -> String`
- `decode(&str) -> Result<ParsedCustomId{ guild_id, ruleset_key, button_key }, EdgeError>` — `starring:` prefix 확인, 4-segment 분리, guild_id 파싱. prefix/형식 불일치는 거부(무시 아님).

런타임 변환: custom_id → decode → guild_id 확인 → button_key 추출 → `RuntimeEvent::ButtonClick { guild_id, actor, kind: ButtonClick { component: button_key } }`. (ruleset_key는 어느 RuleSet을 볼지 선택 — smoke는 단일 fixture라 확인용.)

---

## 7. InteractionCreate → RuntimeEvent 변환 (convert.rs)

Twilight `Event::InteractionCreate(Box<InteractionCreate>)`에서:
- `interaction.data` → `InteractionData::MessageComponent(data)` → `data.custom_id`
- `interaction.guild_id`, actor = `interaction.author_id()`(또는 `interaction.member.user.id`)
- custom_id decode → button_key
- → `RuntimeEvent { guild_id, actor: UserId, kind: EventKind::ButtonClick { component: button_key } }`

MessageComponent가 아니거나 custom_id decode 실패 → 무시(로그)하고 no-op(에러 아님). 16b는 ButtonClick만 처리.

---

## 8. seam 실제 구현

**mutation.rs — `TwilightMutationAdapter { http }`** impl `automation_core::DiscordMutationAdapter`:
- `grant_role(guild, member, role)` = twilight `http.add_guild_member_role(guild, member, role).await` → Ok(()) / 실패는 status→`automation_core::AdapterError` 분류.

**responder.rs — `TwilightInteractionResponder { http, application_id, interaction_id, interaction_token }`** (interaction별 생성) impl `automation_core::InteractionResponder`:
- `respond_ephemeral(content)` = `http.interaction(application_id).create_response(interaction_id, &interaction_token, &InteractionResponse{ kind: ChannelMessageWithSource, data: 메시지(content) + flags EPHEMERAL })`.
- `application_id`는 READY에서 확보해 보관 — twilight는 응답을 `http.interaction(application_id).create_response(...)`로 보내므로 응답 클라이언트 구성에 필요하고(원 endpoint는 id/token만 쓰지만 twilight 래퍼가 app_id를 요구), follow-up/edit 확장에도 쓰인다.

에러 분류는 Layer 1 `bot-runtime/error.rs` 패턴 참고(status u16 → kind). automation-runtime은 자체 `AdapterError`(automation-core 것)를 쓴다.

---

## 9. Gateway 루프 (gateway.rs)

- twilight-gateway 0.17 `Shard`. **INTERACTION_CREATE는 privileged intent 불필요** → 최소 intent로 연결.
- 이벤트 루프: `Event::Ready(ready)` → `application_id = ready.application.id` 저장. `Event::InteractionCreate(i)` → convert → runner에 위임.
- rustls ring provider 설치(Layer 1 executor-smoke와 동일 패턴, 버전 일치 확인).

---

## 10. 패널 설치 + runner

**패널(tools/interaction-smoke):** 테스트 채널에 버튼 달린 메시지 발행 — `http.create_message(channel).components(&[ActionRow[ Button{ custom_id: encode(guild, "demo_verify", "verify_button"), label, style } ]])`. (단발성: 실행 시 1회 설치 후 리슨.)

**runner.rs:** `handle(event, ruleset, bindings, mutation, responder)` — `automation_core::handle_event` 호출 + 결과(HandleOutcome / adapter calls / interaction id) 로깅. mutation은 단일 재사용, responder는 인터랙션별 생성.

---

## 11. env + live 실행법

```
DISCORD_TEST_TOKEN     봇 토큰
DISCORD_TEST_GUILD     테스트 길드 id
DISCORD_TEST_CHANNEL   패널 설치할 채널 id
DISCORD_TEST_ROLE      부여할 기존 역할 id  (자동 생성 안 함 — 더 안전)
```
바인딩: `ResourceBindingMap { role_bindings: { ResourceKey("verified_member") → RoleId(DISCORD_TEST_ROLE) } }`. 역할을 미리 서버에 만들어 그 id를 주입 → 정리 부담·권한 위험 최소.

봇 권한: **Manage Roles** + 대상 역할이 봇 최상위 역할보다 아래. 실행: `cargo run -p interaction-smoke`(env 세팅 시). env 없으면 안전 종료.

---

## 12. 완료 조건

1. 테스트 서버에 버튼 메시지 생성됨
2. 버튼 클릭 시 Gateway로 interaction 수신됨
3. automation-core가 matching rule을 찾음
4. 실제 actor에게 Verified(=DISCORD_TEST_ROLE) 역할이 부여됨
5. ephemeral 응답이 Discord에 표시됨
6. 로그에 HandleOutcome / adapter calls / interaction id가 남음
7. `cargo test`는 토큰 없이 계속 green (automation-runtime 순수 테스트 = custom_id + no_ai_gateway)
8. live smoke는 env 있을 때만 수동 실행

**Claude 검증(토큰 없이):** 컴파일 + automation-runtime 순수 테스트(custom_id roundtrip/거부, no_ai_gateway) + clippy + fmt. 실제 버튼 클릭은 **사용자가 자기 터미널에서** env로 수동 실행(토큰 비공개).

---

## 13. 16b에서 하지 말 것 (Forbidden)

ModalSubmit / OpenModal / CreateChannel / CreateRole / 동적 템플릿 / DB 영속 / HTTP outgoing webhook endpoint / retry·backoff / 다중 서버 상시 데몬 / **AI 연결(event-time LLM)** / Twilight 공통 abstraction crate.

목적은 딱 하나: **Layer 2의 저장된 rule이 실제 Discord interaction으로 발동되는가.**

---

## 14. 로드맵 (16b 이후)

```
16a ✅ Rule Plane core
16b    Button live edge (이 스펙)
16c    Modal/OpenModal core (automation-core, Mock)
16d    Modal live edge (automation-runtime)
16e    Dynamic template + bounded create (sanitize)
16f    StudyRoom north-star workflow
```
16b를 먼저 하는 이유: 실세계 제약(interaction callback·custom_id·3초 타이밍)을 본 뒤에 Modal/template을 더 정확히 설계할 수 있다. Ⓑ(코어 확장)를 먼저 하면 나중에 live 붙일 때 모델을 다시 고칠 위험.

---

## 15. Codex 핸드오프 (개요)

1. crate: `automation-runtime`(src 7모듈 + tests 2) + `tools/interaction-smoke`(main). workspace members 등록.
2. **automation-core/automation-state 수정 절대 금지** — seam 구현만.
3. 순수/live 분리: custom_id·convert 로직은 순수 테스트, gateway/http는 env 가드 뒤. tokio 바이너리는 tool.
4. no_ai_gateway 가드(automation-runtime). 주석 없음. twilight 0.17 버전 일치.
5. 게이트: build/test(토큰 없이 green)/clippy(-D warnings)/fmt. 완료 후 push. **실제 클릭은 사용자 수동.**
6. twilight-gateway 0.17 실제 API(Shard/Event/create_response/add_guild_member_role/components)는 플랜 단계에서 소스 대조 후 확정.

---

## 최종 정리
Phase 16b = Gateway 기반 Layer 2 live smoke. 실제 버튼 클릭 → automation-runtime edge(custom_id decode·convert·per-interaction responder·mutation) → automation-core(순수, 무수정) → 실제 GrantRole + ephemeral. 이게 되면 Layer 2도 **"코어로 증명 + live로 증명"**이 닫히고, 대칭(bot-runtime↔automation-runtime)이 스터디룸까지 확장의 토대가 된다.
