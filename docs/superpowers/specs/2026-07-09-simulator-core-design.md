# Permission Simulator Core 설계 스펙 (Phase 8)

- **작성일**: 2026-07-09
- **상태**: 확정 (구현 대기 — Codex 핸드오프 대상)
- **범위**: Phase 8 — `crates/simulator` (Discord 권한 해소 + AccessMatrix)
- **선행**: Phase 1~7 완료. 아키텍처 문서 §10.

---

## 0. 목적

GuildState에서 **특정 역할 조합(subject)이 특정 채널을 볼/쓸 수 있는지**를 Discord 권한 알고리즘으로 계산한다. Simulator의 심장 = `effective_permissions`. **OperationGraph 적용(after-state 생성)은 다음 컷(Virtual Apply Engine)** — Phase 8은 권한 해소 코어만.

```
effective_permissions(&GuildState, subject_roles: &[RoleId], &Channel) -> Permissions
access_matrix(&GuildState, subjects: &[SubjectSpec]) -> AccessMatrix
```

---

## 1. 확정된 설계 결정

| # | 결정 | 내용 |
|---|---|---|
| D1 | **C 먼저, A 나중** | Phase 8 = 순수 권한 해소 코어. OperationGraph 가상 적용은 후속 |
| D2 | **입력 = GuildState 직접** | before/after GuildState fixture를 직접 넣어 검증. 적용 로직 없음 |
| D3 | **subject = 역할 집합** | `&[RoleId]`. new=[] / verified=[verified] / admin=[admin]. @everyone은 항상 암묵 포함 |
| D4 | **Discord 알고리즘 6단계** | base → admin bypass → @everyone overwrite → role overwrites(누적). member overwrite 제외 |
| D5 | **출력 = AccessMatrix** | subject×channel → can_view/can_send. preview 문구 없음 |

---

## 2. 스코프 경계

| Phase 8 담당 | 담당 아님 (후속) |
|---|---|
| `effective_permissions`(Discord 해소) | OperationGraph 가상 적용 → **Virtual Apply Engine** |
| can_view/can_send, AccessMatrix | synthetic id, dry-run executor |
| ADMINISTRATOR bypass, @everyone/role overwrite | member 개별 overwrite, preview 문구 |
| | before/after 델타 자동 계산(호출자가 두 matrix 비교) |

---

## 3. Crate 구조 & 의존
```
simulator → discord-model   (오직 이것만)
```
파일(예): `src/{lib.rs, permissions.rs, matrix.rs}`.

---

## 4. 타입 & API

```
fn effective_permissions(guild: &GuildState, subject_roles: &[RoleId], channel: &Channel) -> Permissions
fn can_view(guild: &GuildState, subject_roles: &[RoleId], channel: &Channel) -> bool     // VIEW_CHANNEL
fn can_send(guild: &GuildState, subject_roles: &[RoleId], channel: &Channel) -> bool     // can_view && SEND_MESSAGES

struct SubjectSpec { name: String, roles: Vec<RoleId> }
struct AccessCell { subject: String, channel: String, can_view: bool, can_send: bool }
struct AccessMatrix { cells: Vec<AccessCell> }
fn access_matrix(guild: &GuildState, subjects: &[SubjectSpec]) -> AccessMatrix   // 각 subject × 각 channel
```

---

## 5. Discord 권한 해소 알고리즘 (D4 — 심장)

`effective_permissions(guild, subject_roles, channel)`:
1. **base** = @everyone 역할 권한 | (subject_roles 각 역할 권한의 OR).
   - @everyone 역할 = `guild.roles`에서 `id == guild.guild.id`(Discord: @everyone id == guild id). 없으면 empty.
2. **ADMINISTRATOR bypass**: `base.contains(ADMINISTRATOR)` → `return Permissions::all()`.
3. `perms = base`.
4. **@everyone channel overwrite** (`channel.overwrites`에서 target `Role(RoleId(guild.id))`): 있으면 `perms = (perms & !deny) | allow`.
5. **role overwrites 누적**: subject_roles 각각의 overwrite(target `Role(rid)`)에서 `deny_accum |= deny`, `allow_accum |= allow`. 그 후 `perms = (perms & !deny_accum) | allow_accum`.
6. `perms` 반환.

> 비트 연산은 `.bits()` 명시: `Permissions::from_bits_retain((perms.bits() & !deny.bits()) | allow.bits())`.
> `can_view = perms.contains(VIEW_CHANNEL)`. `can_send = can_view && perms.contains(SEND_MESSAGES)`.

---

## 6. Phase 8 범위 경계
- ✅ 완전 구현: effective_permissions(6단계), can_view/can_send, SubjectSpec/AccessCell/AccessMatrix, access_matrix, ADMINISTRATOR bypass
- ❌ 제외: OperationGraph 적용/after-state 생성, synthetic id, member overwrite, preview 문구, Discord API, 델타 자동 계산

---

## 7. 컨벤션
serde(AccessMatrix 직렬화 — preview/audit 대비)·주석 없음·결정적(channel/subject 순서 = 입력 순서). 의존 discord-model만.

---

## 8. 테스트 전략 (⭐ Discord 알고리즘 핵심 케이스)
GuildState fixture 직접 구성.
1. **everyone view**: @everyone에 VIEW_CHANNEL, overwrite 없음 → new(=[]) can_view=true.
2. **channel overwrite deny**: @everyone base VIEW_CHANNEL 있어도 채널 @everyone overwrite deny VIEW → new can_view=false.
3. **role overwrite allow**: @everyone deny VIEW, verified role overwrite allow VIEW → verified([verified]) can_view=true, new=false.
4. **send**: verified overwrite allow VIEW+SEND → verified can_send=true. VIEW만이면 can_send=false.
5. **deny/allow 순서**: @everyone deny + verified allow가 같은 채널 → verified는 allow가 이겨 view 가능(role overwrite가 @everyone 뒤에 적용).
6. **ADMINISTRATOR bypass**: admin role base에 ADMINISTRATOR + 채널이 @everyone deny VIEW여도 → admin([admin]) can_view=true(overwrite 무시).
7. **access_matrix**: new/verified/admin × 채널들 → 기대 매트릭스. (인증 시나리오: general 채널 new=숨김, verified=보임+쓰기, admin=보임.)

---

## 9. Codex 핸드오프
1. @everyone 판별: `guild.guild.id.0`로 `RoleId` 구성해 역할/overwrite 매칭.
2. overwrite target 매칭: `OverwriteTarget::Role(RoleId(..))`. (member overwrite는 이번엔 무시.)
3. 비트 연산 `.bits()` 명시(truncation 회피). `Permissions::all()`은 admin bypass.
4. 완료 게이트: build/test/clippy(-D warnings)/fmt. members에 `crates/simulator` 추가.
