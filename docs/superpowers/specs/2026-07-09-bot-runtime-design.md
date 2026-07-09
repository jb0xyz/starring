# Bot Runtime (TwilightDiscordAdapter) 설계 스펙 (Phase 12c)

- **작성일**: 2026-07-09
- **상태**: 확정 (구현 대기 — Codex 핸드오프)
- **범위**: Phase 12c — `crates/bot-runtime` (DiscordAdapter의 twilight 구현)
- **선행**: Phase 12b(executor-core). 상위: `2026-07-09-executor-bot-runtime-design.md`.

---

## 0. 목적

executor-core의 `DiscordAdapter` trait을 **twilight 0.17.1 HTTP client**로 실제 구현. 첫 real Discord 접촉 코드. **12c는 컴파일 + 순수 로직(classify_status/변환) 테스트까지** — 실제 Discord 호출 검증은 12d(executor-smoke + 토큰).

---

## 1. 확정 결정

| # | 결정 | 내용 |
|---|---|---|
| D1 | **twilight 0.17.1** | twilight-http/twilight-model 0.17(조사: 0.16 아님). 버전 민감 부분은 Codex가 컴파일로 조정 |
| D2 | **classify_status 순수 분리** | `classify_status(u16) -> AdapterErrorKind` 순수·**테스트**. `classify_error(&twilight Error)`는 얇은 래퍼(twilight Error는 private라 unit test 불가 — 컴파일만) |
| D3 | **모듈 분리** | `convert.rs`(타입 변환)·`error.rs`(분류)·`adapter.rs`(TwilightDiscordAdapter). 디버깅 용이 |
| D4 | **tokio 없음** | async fn 구현은 runtime 불필요. tokio는 12d runner에서. bot-runtime dep = executor-core + twilight-http + twilight-model |
| D5 | **검증 범위** | ✅ cargo build/test·classify_status·convert 단위테스트·trait 구현. ❌ 실제 Discord 호출/토큰/rate limit/rollback → 12d |

---

## 2. 스코프 경계

| Phase 12c 담당 | 담당 아님 (12d) |
|---|---|
| TwilightDiscordAdapter impl DiscordAdapter | 실제 role/channel/overwrite 생성 |
| 타입 변환(Id/Permissions/ChannelType/Overwrite) | 봇 토큰 인증, 테스트 guild 변경 |
| classify_status(순수) + classify_error(래퍼) | rate limit 실동작, rollback cleanup |
| twilight 0.17.1 컴파일 | rustls crypto provider 런타임 설정(12d) |

---

## 3. Crate 구조 & 의존
```
bot-runtime → {executor-core, twilight-http = "0.17", twilight-model = "0.17"}
```
- twilight-http는 **12c에선 default features로 컴파일**(rustls crypto provider 런타임 panic은 12d smoke에서 install_default로 처리). tokio 없음.
- 파일: `src/{lib.rs, convert.rs, error.rs, adapter.rs}`.

---

## 4. 에러 분류 (D2 — 테스트 코어)

```rust
pub fn classify_status(status: u16) -> AdapterErrorKind {
    match status {
        429 => RateLimited,
        408 => Timeout,
        400 => BadRequest,
        401 | 403 => Forbidden,
        404 => NotFound,
        500..=599 => ServerError,
        _ => Unknown,
    }
}

fn classify_error(err: &twilight_http::Error) -> AdapterError {
    let kind = match err.kind() {
        ErrorType::Response { status, .. } => classify_status(status.get()),
        ErrorType::RequestTimedOut => Timeout,
        ErrorType::RequestError => Network,
        ErrorType::Unauthorized => Forbidden,
        _ => Unknown,   // BuildingRequest/Json/Parsing/Validation/RequestCanceled/non_exhaustive
    };
    AdapterError::new(kind, "twilight error")   // message는 twilight Error Display 등으로 채워도 됨
}
```

---

## 5. 타입 변환 (convert.rs — 테스트 가능)

| 우리 타입 | twilight 0.17.1 | 함수 |
|---|---|---|
| RoleId/ChannelId/GuildId (u64) | `Id::new(u64)` (marker별) | to_role_id/to_channel_id/to_guild_id |
| Permissions (u64 bits) | `Permissions::from_bits_truncate(bits)` | to_twilight_permissions |
| ChannelType Text/Voice/Category | `ChannelType::{GuildText,GuildVoice,GuildCategory}` | to_twilight_channel_type |
| OverwriteTarget + allow/deny | `PermissionOverwrite{allow, deny, id: Id::new(raw)(GenericMarker 추론), kind: Role/Member}` | to_permission_overwrite |

> 변환 함수는 twilight_model 타입(public·생성가능·PartialEq)을 만들어 **단위 테스트 가능**(`.bits()`/`.get()`/변주 비교). `Id::new(0)`은 panic — 실제 id는 nonzero라 안전.

---

## 6. 어댑터 메서드 매핑 (adapter.rs — 컴파일 검증)

`TwilightDiscordAdapter { http: twilight_http::Client }`. 각 async 메서드(조사한 0.17.1 API; 정확 시그니처는 Codex 컴파일 조정):
| DiscordAdapter | twilight |
|---|---|
| create_role(g, spec) | `http.create_role(id).name(&str).permissions(P).await?.model().await?` → RoleId(role.id.get()) |
| update_role(g, id, spec) | `http.update_role(id_g, id_r).name(Some(&str)).permissions(P).await?` (update의 name은 **Option<&str>**) |
| delete_role(g, id) | `http.delete_role(id_g, id_r).await?` (EmptyBody) |
| create_channel(g, spec) | `http.create_guild_channel(id, name).kind(CT).parent_id(id).await?.model().await?` → ChannelId |
| update_channel(g, id, spec) | `http.update_channel(id_c).name(&str).kind(CT).await?` |
| delete_channel(g, id) | `http.delete_channel(id_c).await?` |
| upsert_overwrite(g, ch, target, allow, deny) | `http.update_channel_permission(id_c, &PermissionOverwrite).await?` (guild 미사용) |

> 실패는 `.await.map_err(|e| classify_error(&e))?`로 AdapterError 변환. create는 `.model().await`도 map_err. **create_role.name(&str) vs update_role.name(Option<&str>) 비대칭 주의**(조사 확인).

---

## 7. 컨벤션
주석 없음. classify_status 순수·결정적. `#[allow(async_fn_in_trait)]`은 trait 정의(executor-core)에 이미 있음 — 구현 impl엔 불필요.

---

## 8. 테스트 전략 (컴파일 + 순수 로직)
- **classify_status**: 429→RateLimited, 408→Timeout, 400→BadRequest, 401/403→Forbidden, 404→NotFound, 503→ServerError, 200/기타→Unknown.
- **convert**: to_twilight_permissions(bits 왕복), to_twilight_channel_type(Text→GuildText 등), to_permission_overwrite(kind/id/allow 검증).
- **컴파일만**: classify_error, adapter 7 메서드(TwilightDiscordAdapter가 DiscordAdapter 구현 → `cargo build`로 확인).
- ❌ 실제 Discord 호출 테스트 없음(12d).

---

## 9. Codex 핸드오프
1. **twilight 0.17.1 실제 API로 컴파일 조정** — builder 시그니처(name의 &str vs Option<&str>, kind/parent_id 등)가 조사와 다르면 문서 보고 맞춤. 목표는 `cargo build` 통과.
2. twilight-http는 **default features로 시작**(컴파일). rustls crypto provider 런타임 이슈는 12c 범위 아님(12d).
3. `Id::new`/`.get()`/`from_bits_truncate`/`.bits()`/`ChannelType::Guild*`/`PermissionOverwrite`/`PermissionOverwriteType` 사용.
4. 완료 게이트: build/test/clippy(-D warnings)/fmt. members에 `crates/bot-runtime`. 완료 후 `git push origin main`.
5. **주의: 실제 Discord 호출/토큰은 절대 하지 말 것**(12d 책임). 12c는 컴파일 + classify_status/convert 테스트까지.
