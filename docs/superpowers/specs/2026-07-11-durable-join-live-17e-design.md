# Phase 17e — Durable Dynamic Join Live 설계 스펙

- **작성일**: 2026-07-11
- **상태**: 설계 확정 (구현 대기 — Codex 코드, Claude 실제 Discord+Postgres live)
- **범위**: Phase 17e — tool이 `InMemoryInstanceStore`→`PostgresInstanceStore` **drop-in** 교체 + OS 난수 InstanceId 생성기 + automation-core **handle_event defer→instance 순서 조정**(§1.5) → 실제 Discord에서 **봇 재시작 후에도 참가 버튼 작동** 증명. Phase 17 아크의 대미.
- **선행**: 17a~17d(registry/registration/join/Postgres). 16j/16k live 패턴.

---

## ⚠️ 최상위 원칙 (불변)
AI 설치시점 설계자, Runtime 결정론, event-time LLM 금지. no_ai_gateway 유지. **crate 수정 범위(grounding으로 확정)**: automation-**state/postgres 무수정** · automation-**instance**는 `InstanceIdGenerationError`에 **backend-agnostic Entropy 변형만** 추가(17d Backend와 동일 패턴, getrandom 실패 표현용; **getrandom/sqlx 의존은 추가 안 함** — 생성기 구현체는 tool edge) · automation-**core**는 handle_event **순서만** 조정(defer→instance 해소, §1.5) · automation-**runtime**은 `encode_instance_action` **길이 가드만**(§1, invariant #2) · **tool** 배선/난수 생성기 edge. **DB가 안 되면 프로세스 시작 안 함**(InMemory fallback 절대 금지 — split-brain 방지).

**목표(한 문장):** 방 생성→instance가 **PostgreSQL에 영속**→봇 프로세스 완전 재시작→기존 join 버튼 클릭→DB에서 instance 조회→역할 지급. + 재시작 후 새 방도 별도 랜덤 ID로 충돌 없이 생성.

---

## 0. 범위

**포함:** **automation-core handle_event reorder**(defer ACK → 그 뒤 instance 해소, §1.5) · automation-**instance** `InstanceIdGenerationError::Entropy` 변형 · automation-**runtime** `encode_instance_action` 길이 가드(`CustomIdError::TooLong`, invariant #2) · tool이 `PostgresInstanceStore`로 교체(PgPool + startup migration) · `RandomInstanceIdGenerator`(tool edge, OS CSPRNG) · **Postgres-or-die startup**(fallback 없음) · `STARRING_DATABASE_URL` · encoder/생성기/custom_id/reorder-order unit 테스트 · **실제 Discord+Postgres live 검증**(재시작 durability).

**제외(→후속):** RuleSet DB 영속/복원(재시작 후 fixture 재로드 — 아래 한계) · bounded retry(충돌 재시도) · Backend API/Web UI · 정규화 resource 테이블 · created_at/updated_at · connection pool 튜닝.

---

## 1. RandomInstanceIdGenerator (tool edge)
```
OS CSPRNG(getrandom) 8바이트 → u64(big-endian) 하위 60비트 → 5비트씩 12조각 → base32 12자 → "i_" prefix → InstanceId::parse
예: i_7k2p9x4m1qaz  (14자, 60비트, [a-z0-9_])
```
- **alphabet 상수 고정(문서+테스트)**: Crockford lowercase `"0123456789abcdefghjkmnpqrstvwxyz"`(32자, no padding). 60비트=12×5비트(8바이트=64비트 중 하위 60비트, 상위 4비트 버림 — 고정 규칙). getrandom 실패 시 **panic 금지 → typed error**(InstanceIdGenerationError).
- **pure encoder 분리**: `encode_instance_id(bytes: [u8; 8]) -> String`(순수, getrandom 무관) + `RandomInstanceIdGenerator`(getrandom→encode). → **fixed-vector 테스트**(`encode_instance_id([고정 8바이트]) == "고정 12자"`, 결정론 — **플랜에서 실제 실행해 예상값 확정**) + generator는 "결과가 `i_`+InstanceId 규칙 만족"만. 충돌 안전은 확률 테스트 아님(§6).
- 위치: `tools/interaction-smoke/src/random_instance_id.rs` — `impl InstanceIdGenerator`. **automation-instance는 `Entropy` 변형만 추가**(getrandom 실패 표현용, Copy 유지); 생성기 구현체는 tool edge(ID 생성은 store 책임 아님 → postgres crate에도 안 넣음).
- **DB가 최종 충돌 판정자(TOCTOU 회피)**: 생성 전 DB 조회 안 함 → candidate → `register` → PostgreSQL 복합 PK가 중복 판정 → `DuplicateInstance` → **17e fail-fast**(16k failure edit). bounded retry는 후속.
- **InstanceId는 식별자, 인증 토큰 아님** — id를 알아도 다른 guild instance 접근 불가. **항상 interaction.guild_id + instance_id 조회 + Active + ruleset_key 일치 + alias 존재 검사**(17c 불변식 유지).
- custom_id는 **17c에서 4-seg**(`starring:i:<instance_id>:<action>`) — 여유 많지만 **worst-case 길이 테스트 필수**(§6).

---

## 1.5. Join: DeferEphemeral ACK가 Store.get보다 먼저 (필수 core 조정)
**17c handle_event는 instance를 defer 전에 해소**(Store.get→context.instance→그다음 run이 defer). InMemory는 즉시라 무관했지만 **Postgres는 DB/네트워크가 ACK 앞에 오면 3초 위험**. → **reorder**: handle_event가 **defer 먼저(ACK), instance 해소는 그 뒤**.
```
Interaction 수신 → custom_id에서 instance_id/action만 추출 → trigger match
→ DeferEphemeral(ACK)                    ← 3초 내
→ (InstanceAction면) PostgreSQL Store.get + status/ruleset 검증
→ GrantRole(RoleRef::Instance 해소) → EditResponse
```
- join 룰은 16k 계약 그대로: `[defer_ephemeral, grant_role{instance:event, alias}, edit_response]`.
- **handle_event 재배치**: 16k defer/strip **먼저** → InstanceAction 해소(Store.get)를 **그 다음**. 해소 실패(missing/Disabled/ruleset)는 **이미 defer_acked라 원본을 실패로 edit**(16k fallback — "interaction failed" 대신 "처리 중→실패"). context.instance는 run 이전에 채워짐(run/GrantRole 무변경).
- 이게 17e의 유일한 automation-core 변경(순서 조정). automation-instance/state/postgres/run 로직은 무변경.

## 2. Postgres-or-die startup (fallback 금지)
```
1. STARRING_DATABASE_URL 읽기 (없으면 시작 실패)
2. PgPool::connect (실패 → 시작 실패)
3. MIGRATOR.run(&pool) (실패 → 시작 실패)
4. 성공 후에만 패널 설치 + gateway::run
```
- **InMemory fallback 절대 금지** — "Postgres 안 되면 InMemory" 하면 일부는 DB, 일부는 메모리인 split-brain. DB unavailable → **bot runtime 시작 안 함**(durable runtime의 올바른 동작).
- `STARRING_DATABASE_URL`(live) — URL에 비밀번호 가능 → **로그/Debug/에러 메시지에 전체 URL 노출 금지**.

---

## 3. tool 배선 (drop-in)
- tool(`#[tokio::main]`)이 PgPool 연결 → `PostgresInstanceStore::new(pool)` → `gateway::run(token, ruleset_key, ruleset, bindings, failure_message, store, generator)`. run/handle_event가 generic(`AutomationServices<...S,G...>`)이라 **drop-in**.
- **Send 확인(플랜)**: gateway는 tokio multi-thread, run future가 store futures 보유 → Postgres/sqlx futures는 Send라 컴파일 통과해야(InMemory도 통과했음). 플랜서 실제 컴파일 확인.
- Cargo: tool에 automation-instance-postgres + sqlx(PgPool 타입용) + getrandom dep.

---

## 4. 명시적 한계 (신뢰 위해 반드시)
> **17e가 주장하는 것:** AutomationInstance와 resource bindings가 **프로세스 재시작을 넘어 유지**된다.
> **아직 주장하지 않는 것:** 모든 RuleSet 설치 상태가 DB에 영속되어 자동 복원된다.

재시작 후 join이 작동하는 건 tool이 **같은 compiled fixture를 재로드**하기 때문(instance는 DB, RuleSet은 fixture). RuleSet persistence/activation/versioning은 별도 후속.

---

## 5. live 완료 증거 (Claude hands-on)
**1차 프로세스:** 봇 실행(Postgres 연결+migration) → StudyRoom 생성 → 랜덤 InstanceId → **PostgreSQL에 등록**(psql로 row 직접 확인) → 공개 허브에 join 버튼 게시.
**재시작:** 봇 프로세스 **완전 종료**(PgPool 종료) → 새 프로세스 재실행(새 PgPool, 같은 fixture).
**재시작 후 join(A — persistence):** 기존 게시된 join 버튼 클릭 → guild+instance_id로 **PostgreSQL 조회** → Active 확인 → `resources.roles["member_role"]` → 클릭 사용자에 역할 지급 → ephemeral.
**재시작 후 새 방(B — restart-safe generator):** 새 StudyRoom 생성 → 기존 instance A 그대로 조회됨 + 새 instance B가 **별도 랜덤 ID로 충돌 없이** 등록(DuplicateInstance 없음).
- **A = persistence 증명, B = restart-safe generator 증명.** 둘 다 성공해야 17e 완료.

---

## 6. 테스트 + 안전장치
1. **fixed-vector encoder**: `encode_instance_id([고정 바이트]) == "고정 예상"`(결정론) + prefix/alphabet/길이/no-padding/InstanceId::parse.
2. generator가 유효 InstanceId 생성(규칙 만족만; 유일성은 확률 테스트로 과신 X — ~수백 개 형식·중복 없음 정도, 암호학 보장으로 표현 X).
3. **worst-case custom_id ≤ 100**: guild_id 20자 + ruleset_key 최대 + instance_id 14자 + action → `encode_instance_action(...).len() <= 100`.
4. **deterministic duplicate**: 고정-id generator로 같은 id 두 번 → 첫 register Ok, 두 번째 `DuplicateInstance`(충돌 정확성은 확률 아니라 PK로 검증).
5. PgPool/migration 실패 → Gateway 미시작(구조적 — startup Err 전파). **runtime 중 DB 끊김 → Backend error → action 실패 → 16k failure edit**(InMemory 전환 X).
6. **InMemory fallback 없음**(구조적 — fallback 코드 부재) + token/DB URL 로그 미출력.
- **migration locking 유지**: `MIGRATOR.run`의 locking 비활성화 금지(동시 apply 시 오류/손실 위험 — sqlx 경고).

---

## 7. 로드맵
```
17d✅ PostgreSQL InstanceStore   17e▶ Durable Dynamic Join Live (이 스펙 — Phase 17 대미)
후속: RuleSet DB 영속/activation/versioning · bounded retry · Backend API/Web UI · AI 룰 생성
```

---

## 8. Codex 핸드오프 (개요)
1. **automation-core handle_event reorder**(§1.5): 16k defer/strip **먼저**(ACK) → InstanceAction instance 해소(Store.get)를 **그 다음** → run. 해소 실패는 defer_acked라 16k failure edit. run/GrantRole/RoleRef 해소 로직 무변경. defer가 instance 조회보다 앞선다는 순서 테스트 추가(Mock store로).
2. tools/interaction-smoke: `src/random_instance_id.rs`(encode_instance_id pure + RandomInstanceIdGenerator, getrandom+base32) + main.rs(STARRING_DATABASE_URL→PgPool→MIGRATOR.run→PostgresInstanceStore→gateway::run, **fallback 없음**) + Cargo(automation-instance-postgres, sqlx, getrandom). encoder fixed-vector + generator + worst-case custom_id 테스트.
3. **automation-runtime** `encode_instance_action`→`Result<String, CustomIdError>`(TooLong 가드) + mutation.rs 소비측 반영 + worst-case 길이 테스트. **automation-instance**는 `Entropy` 변형만. **automation-state/postgres 무수정** 확인. 주석 없음. 게이트 build/test/clippy(-D warnings)/fmt. push. **live/토큰/DB 접속 없음**(Codex).
4. **Claude가 실제 Discord+Postgres live**(재사용/재발급 토큰, starring 전용 DB): 방 생성→재시작→기존 join 작동 + 새 방 생성 검증.

## 최종 정리
17e = Durable Dynamic Join Live. tool이 InMemory→PostgresInstanceStore drop-in + OS 난수 InstanceId 생성기(tool edge, `i_`+base32 12자, DB 복합 PK가 최종 충돌 판정). Postgres-or-die startup(fallback 금지). store는 generic이라 무수정 drop-in, **automation-core handle_event만 defer→instance 순서 조정**(Postgres 조회가 ACK보다 늦게, §1.5). **완료 증거 = 방 생성→Postgres 등록→봇 완전 재시작→기존 join 버튼 작동(persistence) + 재시작 후 새 방 별도 ID 생성(restart-safe generator).** 한계: instance는 영속하나 RuleSet은 fixture 재로드(RuleSet 영속은 후속). 이걸로 Phase 17이 "재시작 후에도 살아있는 durable Layer 2 automation"을 증명.
