# AI Gateway Spike 설계 스펙 (Phase 6)

- **작성일**: 2026-07-09
- **상태**: 확정 (구현 대기 — Codex 핸드오프 대상)
- **범위**: Phase 6 — `crates/ai-gateway` + `tools/ai-eval` (소형 LLM DesiredState 생성 가능성 검증)
- **선행**: Phase 1~5 완료.

---

## 0. 목적 (가정 검증)

**검증 가정:** "소형 로컬 LLM이 자연어 요청 + 최소 GuildState context를 받아, `validate → compile → diff → operation-graph`까지 통과하는 유효한 DesiredState를 생성할 수 있는가?"

**production AI 서비스가 아니다.** 목적은 feasibility 검증. **빌드(Codex, Mock 결정론) / 실제 run(로컬 모델, 수동) 분리.**

---

## 1. 확정된 설계 결정

| # | 결정 | 내용 |
|---|---|---|
| D1 | **빌드/실행 분리** | Codex: Mock 기반 결정론 빌드. 실제 spike: 로컬 OpenAI-compatible endpoint(수동) |
| D2 | **타깃 = 로컬 소형 모델** | vLLM/llama.cpp/Ollama + Gemma/Qwen 계열. 1차 후보 사용자 지정(`AI_MODEL`) |
| D3 | **endpoint-agnostic client** | `AI_BASE_URL`/`AI_API_KEY`/`AI_MODEL` env. OpenAI-compatible `/v1/chat/completions`만 바라봄 |
| D4 | **프롬프트 = 스키마가이드 + strict JSON + few-shot** | few-shot은 **실제 DesiredState fixture를 serialize**(포맷 보장). 스키마가이드는 실제 필드 기준. repair loop 제외(raw 성공률 측정) |
| D5 | **ai-gateway 순수 + HTTP feature 격리** | `ai-gateway`=trait/mock/prompt/parse(순수). OpenAI HTTP client=`openai-client` feature + `tools/ai-eval`에서만 |
| D6 | **단계별 채점** | parse→deserialize→validate→compile→diff→graph→expected. 어느 단계에서 실패하는지 기록 |

---

## 2. 스코프 경계

| Phase 6 담당 | 담당 아님 (후속) |
|---|---|
| LlmClient trait + MockLlmClient | 앱/웹 UI, streaming chat |
| OpenAI-compatible client(feature) | Discord Bot Runtime / 실제 실행 |
| prompt builder + response parser | DB 저장, 장기 메모리 |
| `generate_desired_state` (NL→DesiredState draft) | repair loop, multi-turn agent |
| ai-eval harness(단계별 채점, 리포트) | fine-tuning, production orchestration |
| fixture 평가 세트 5~10개 | Policy(P7)·Simulator(P8) |

---

## 3. Crate 구조 & 의존

```
crates/ai-gateway    (순수: trait/mock/prompt/parse. feature "openai-client"=reqwest client)
  → desired-state (DesiredState 파싱 대상), serde/serde_json, thiserror
  → (openai-client) reqwest(blocking)

tools/ai-eval        (바이너리, openai-client feature 켬)
  → ai-gateway, desired-state, desired-compiler, diff-engine, operation-graph, discord-model
```
> **기본 `cargo test`는 reqwest 없이 Mock으로 통과.** reqwest는 `--features openai-client`에서만.

---

## 4. `ai-gateway` 타입

```
trait LlmClient {
    fn complete(&self, system: &str, user: &str) -> Result<String, AiGatewayError>;
}

struct MockLlmClient { response: String }   // 미리 정한 응답 반환(결정론)

struct GenerateInput {
    user_prompt: String,
    guild_context_summary: String,   // 최소 현재 상태 요약(역할/채널 이름 등)
}

struct GeneratedDesiredState {
    raw_text: String,
    parsed: Option<DesiredState>,     // parse/deserialize 성공 시 Some
    parse_error: Option<String>,
    model: String,
}

enum AiGatewayError { Http(String), EmptyResponse, ... }

fn generate_desired_state(client: &impl LlmClient, input: &GenerateInput, model: &str)
    -> Result<GeneratedDesiredState, AiGatewayError>
```
- `generate_desired_state`: prompt build → `client.complete` → response parse(JSON 추출 + serde) → `GeneratedDesiredState`. **DesiredState draft까지만.** validate/compile/diff는 ai-eval의 몫.
- **prompt builder**(내부): system=스키마가이드+few-shot+strict JSON 지시, user=`guild_context_summary` + `user_prompt`.
- **response parser**(내부): raw_text에서 JSON 블록 추출(markdown ```json 펜스/설명문 제거) → `serde_json::from_str::<DesiredState>`. 실패 시 `parsed=None`+`parse_error`.

## 5. OpenAI-compatible client (feature `openai-client`)
```
struct OpenAiCompatibleClient { base_url: String, api_key: String, model: String }
impl OpenAiCompatibleClient { fn from_env() -> Result<Self, AiGatewayError> }  // AI_BASE_URL/AI_API_KEY/AI_MODEL
impl LlmClient for OpenAiCompatibleClient { /* reqwest blocking POST /v1/chat/completions */ }
```

## 6. 프롬프트 설계 (D4)
- **스키마 가이드**(compact, 실제 필드): `mode`(patch 기본), `roles`(key/name/permissions, ownership/state), `channels`(key/name/access.everyone/access.roles(capability), raw_overwrites), `features`(verification), capability 목록(view/send/read_history/...), 문자열 permissions, key 참조 규칙.
- **strict 출력**: JSON만, markdown/설명/주석 금지.
- **few-shot**: 실제 DesiredState fixture를 `serde_json`으로 serialize한 예시 3~4개(역할 생성 / 인증 시스템 / 채널 접근 / 역할 삭제). → 포맷 100% 일치 보장.
- **repair loop 없음**(1차 spike는 raw 성공률).

## 7. `tools/ai-eval` harness
```
struct EvalFixture { name: String, user_prompt: String, guild: GuildState, expect_reaches: EvalStep }
enum EvalStep { Parse, Validate, Compile, Diff, Graph }
struct FixtureResult { name: String, reached: EvalStep, failure: Option<String> }
struct EvaluationReport { results: Vec<FixtureResult> }  // + per-step 성공률 집계

fn evaluate(client: &impl LlmClient, model: &str, fixtures: &[EvalFixture]) -> EvaluationReport
```
- 각 fixture: `generate_desired_state` → parsed 있으면 `validate()` → `compile()` → `diff(vs guild)` → `compile_operations` 순서로 태우고, **도달한 최종 단계** 기록.
- `main`: env에 `AI_BASE_URL` 있으면 `OpenAiCompatibleClient::from_env()`, 없으면 안내 출력(또는 `--mock`). fixtures 로드 → evaluate → 리포트 출력(단계별 성공률).

## 8. 평가 fixture (5~10개) & 성공 기준
- fixture: "VIP 역할 만들어줘" / "인증됨 역할 + 일반 채널 인증자만" / "#일반에 인증자만 메시지" / "VIP 삭제" / "관리자 권한 줘"(ADMINISTRATOR 생성 안 하는지) / "인증 패널"(panel deferred).
- 성공 기준(느슨): parse 70%+ / validate 50%+ / graph 도달 40%+ / 단순 역할 fixture graph 70%+ / 복합 fixture graph 30~50%+. **완벽 아님 — feasibility.**

## 9. Phase 6 범위 경계
- ✅ 완전 구현: ai-gateway(trait/mock/prompt/parse/generate), openai-client(feature), ai-eval(단계별 채점/리포트), fixture 세트, Mock 결정론 테스트
- ❌ 제외: repair loop, streaming, DB, UI, Bot Runtime, 실제 Discord, multi-turn, Policy/Simulator

## 10. 컨벤션
serde·주석 없음·결정적(Mock 테스트). env로 endpoint. reqwest는 feature 뒤. `Date::now`/난수 미사용.

## 11. 테스트 전략 (⭐ Mock 결정론)
- MockLlmClient(canned valid DesiredState JSON) → `generate_desired_state`가 `parsed=Some` + serde 일치.
- MockLlmClient(non-JSON) → `parsed=None`+`parse_error`.
- response parser: ```json 펜스/앞뒤 설명 있는 응답에서 JSON 추출.
- prompt builder: 프롬프트에 스키마 가이드·strict 지시·few-shot 포함.
- **ai-eval**: MockLlmClient(역할 생성 canned) → EvaluationReport에서 그 fixture `reached=Graph`. non-JSON canned → `reached=Parse`(실패).
- **실제 run(수동)**: `AI_BASE_URL=... AI_MODEL=... cargo run -p ai-eval --features openai-client`. 결정론 아님 — 리포트만.

## 12. Codex 핸드오프
1. 빌드는 모델 없이 Mock으로 결정론 완성(게이트 통과). 실제 모델 없어도 `cargo test` 통과해야 함.
2. few-shot 예시는 실제 `DesiredState` 값을 `serde_json::to_string_pretty`로 만들어 상수화(포맷 표류 방지). Codex가 유효 DesiredState 3~4개 구성 → serialize → 프롬프트에 삽입.
3. `openai-client` feature 없이 기본 빌드/테스트가 통과해야 함(reqwest 미컴파일).
4. 완료 게이트: `cargo build`·`cargo test`·`cargo clippy --all-targets -- -D warnings`·`cargo fmt --all -- --check`(기본 features). members에 `crates/ai-gateway`, `tools/ai-eval` 추가.
5. 실제 spike run 방법을 `tools/ai-eval/README.md`에 기록(env·모델·해석법).
