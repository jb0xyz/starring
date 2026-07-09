# GuildState Reader 설계 스펙 (Phase 13a)

- **작성일**: 2026-07-10
- **상태**: 확정 (구현 대기 — Codex 핸드오프)
- **범위**: Phase 13a — `GuildStateReader` trait(executor-core) + TwilightDiscordAdapter impl(bot-runtime) + 역변환
- **선행**: Phase 12 완료(쓰기 live 검증). twilight 0.17.1.

---

## 0. 목적

실제 Discord guild를 twilight HTTP로 읽어 `discord-model::GuildState`로 변환 → Starring이 **양방향 Control Plane**. 13a는 **컴파일 + 역변환 단위테스트까지**; 실제 read 검증은 13b(live).

```
GuildStateReader::read_guild_state(guild_id) -> Result<GuildState, AdapterError>
```

---

## 1. 확정 결정

| # | 결정 | 내용 |
|---|---|---|
| D1 | **trait은 executor-core** | DiscordAdapter(쓰기 포트) 옆에 GuildStateReader(읽기 포트). executor-core는 twilight 무관 |
| D2 | **TwilightDiscordAdapter가 둘 다 impl** | 같은 http Client 공유. read impl은 `reader.rs` |
| D3 | **AdapterError/classify_error 재사용** | 새 에러 타입 없음. read HTTP 오류 분류 동일 |
| D4 | **REST read만** | `roles()`/`guild_channels()`. Gateway/member전체/audit/messages/DB 제외 |
| D5 | **역변환 테스트 = 단순한 것만** | channel_type/overwrite는 테스트(twilight 타입 생성 쉬움). role/channel 역변환은 컴파일 검증(twilight Role/Channel 생성이 복잡) |

---

## 2. twilight 0.17.1 read API (조사 확정)

- `Client::roles(Id<GuildMarker>).await?.model().await?` → **`Vec<twilight_model::guild::Role>`** (메서드명 `roles`, guild_roles 아님).
- `Client::guild_channels(Id<GuildMarker>).await?.model().await?` → `Vec<twilight_model::channel::Channel>`.
- `Role`: `id: Id<RoleMarker>`, `name: String`, `permissions: Permissions`, `position: i64`, `managed: bool`(+color/hoist/mentionable). **position i64.**
- `Channel`: `id`(비Option), `kind: ChannelType`(비Option), `name: Option<String>`, `parent_id: Option<Id<ChannelMarker>>`, `position: Option<i32>`, `permission_overwrites: Option<Vec<PermissionOverwrite>>`. **guild_id는 신뢰 불가 → 무시.**
- read `PermissionOverwrite` = write와 동일 model 타입(`allow/deny: Permissions`, `id: Id<GenericMarker>`, `kind: PermissionOverwriteType{Role,Member,Unknown(u8)}`). `.id.get()`→u64.
- 스레드 제외, 페이지네이션 없음(1회), roles 순서 무보장.

---

## 3. Crate 구조 & 파일
```
executor-core/src/reader.rs   // GuildStateReader trait (신규)
bot-runtime/src/reader.rs      // impl GuildStateReader for TwilightDiscordAdapter (신규)
bot-runtime/src/convert.rs     // from_twilight_role/channel/overwrite/channel_type (추가)
bot-runtime/src/adapter.rs     // http 필드 pub(crate)로 (reader.rs 접근용)
```
의존 변경 없음(executor-core는 discord-model만; bot-runtime는 twilight 기존).

---

## 4. 역변환 (convert.rs)

| twilight → discord-model | 매핑 |
|---|---|
| Role | `id: RoleId(r.id.get())`, `name: r.name`, `permissions: Permissions::from_bits_retain(r.permissions.bits())`, `position: i32::try_from(r.position).unwrap_or(0)`, `managed: r.managed` |
| ChannelType | GuildVoice→Voice, GuildCategory→Category, **그 외(GuildText 포함)→Text** |
| PermissionOverwrite | kind Member→`Member(UserId(id.get()))`, 그 외→`Role(RoleId(id.get()))`. allow/deny=`from_bits_retain(bits())` |
| Channel | `id: ChannelId(c.id.get())`, `name: c.name.unwrap_or_default()`, `channel_type: from_twilight_channel_type(c.kind)`, `parent_id: c.parent_id.map(|p| ChannelId(p.get()))`, `position: c.position.unwrap_or(0)`, `overwrites: c.permission_overwrites.unwrap_or_default().into_iter().map(from_twilight_overwrite).collect()` |

> **@everyone 보존**: Discord `roles()`가 @everyone(id==guild_id) 역할을 포함해 반환 → 자동 보존. overwrite의 @everyone도 Role(id==guild_id)로 그대로 변환.

---

## 5. read_guild_state (reader.rs)
```
1. roles = http.roles(to_guild_id(guild_id)).await? .model().await?   // Vec<TwRole>
2. channels = http.guild_channels(...).await? .model().await?          // Vec<TwChannel>
3. GuildState {
     guild: Guild { id: guild_id, name: "", owner_id: UserId(0) },     // 메타 미조회(첫 컷) — 해소/시뮬은 guild.id만 씀
     roles: roles.map(from_twilight_role),
     channels: channels.map(from_twilight_channel),
     members: [],
   }
```
`.await` 오류→classify_error, `.model()` 오류(DeserializeBodyError)→classify_body_error(12c) 재사용.

---

## 6. 스코프 경계
- ✅ 컴파일: GuildStateReader trait·impl·역변환 4개·read_guild_state. 테스트: channel_type/overwrite 역변환.
- ❌ 실제 Discord read(13b), guild 메타(name/owner) 조회, 스레드, member, DB, role/channel 역변환 단위테스트(twilight 생성 복잡 → 13b live)

---

## 7. 컨벤션
주석 없음. `#[allow(async_fn_in_trait)]`은 GuildStateReader trait 정의에. Permissions는 `from_bits_retain`(Discord 미지 비트 보존).

---

## 8. 테스트 (컴파일 + 역변환)
- from_twilight_channel_type: GuildText→Text, GuildVoice→Voice, GuildCategory→Category.
- from_twilight_overwrite: Role kind→Role target+allow bits, Member kind→Member target.
- 컴파일만: from_twilight_role/channel, read_guild_state.

---

## 9. Codex 핸드오프
1. 메서드명 `roles`(guild_roles 아님), `guild_channels`. `.await?.model().await?`로 `Vec<Role>`/`Vec<Channel>`.
2. **Role.position i64 → `i32::try_from(...).unwrap_or(0)`**. Channel.position은 Option<i32>. permission_overwrites Option → `unwrap_or_default()`.
3. adapter.rs `http` 필드를 `pub(crate)`로(reader.rs 접근). classify_body_error는 12c 함수 재사용(시그니처 맞춤).
4. twilight builder/필드가 조사와 다르면 문서 보고 컴파일 조정.
5. 게이트: build/test/clippy(-D warnings)/fmt. 완료 후 `git push origin main`. **실제 Discord read 금지(13b).**
