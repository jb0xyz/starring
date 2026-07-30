# Preview Model 설계 스펙 (Phase 10)

- **작성일**: 2026-07-09
- **상태**: 확정 (구현 대기 — Codex 핸드오프 대상)
- **범위**: Phase 10 — `crates/preview` (코어 결과 → 승인용 PreviewModel)
- **선행**: Phase 1~9 완료. 순수 결정론 코어의 **캡스톤**.

> 승인 정책 변경 (2026-07-31): Preview의 승인 필요 여부는 단일
> `RequireApproval` verdict만 사용합니다. 2인 승인 verdict는
> `2026-07-31-solo-product-approval-design.md`에 의해 폐기되었습니다.

---

## 0. 목적

이미 계산된 코어 결과들(DiffResult, OperationGraph, PolicyDecision, VirtualApplyResult, before/after AccessMatrix)을 **사람이 읽고 승인할 UI-중립 구조화 데이터** `PreviewModel`로 결정론적 합성한다. AI 문구 생성/실행/승인상태/DB 없음. "AI는 제안, Core는 검증, **Preview는 결정론적 생성**, 사용자는 승인."

```
build_preview(title, &DiffResult, &OperationGraph, &PolicyDecision, &VirtualApplyResult,
              before: &AccessMatrix, after: &AccessMatrix) -> PreviewModel
```

---

## 1. 확정된 설계 결정

| # | 결정 | 내용 |
|---|---|---|
| D1 | **타입드 필드** | PreviewSection/Item 제네릭 X. changes/access_changes/policy_findings를 **타입으로** 유지 → 테스트·UI가 재파싱 불필요 |
| D2 | **입력 6+title** | DiffResult·OperationGraph·PolicyDecision·VirtualApplyResult·AccessMatrix(before/after)+title. **DesiredState 제외**(YAGNI — 변경은 graph, 안전은 policy, 후상태는 apply, 가시성은 matrix) |
| D3 | **access_changes = matrix diff** | (subject, channel) **이름** 합집합, 없는 쪽 can_view/send=false, 다르면 emit. 새 subject는 after에만 → false→true 자연 표현 |
| D4 | **severity 규칙** | delete=Warning, @everyone overwrite=Notice, 그 외=Info |
| D5 | **Finding 재사용** | policy-engine `Finding`(rule_id/verdict/target/message) 그대로. 래퍼 X. audit에도 재사용 |
| D6 | **첫 컷은 문자열** | deferred=`Vec<String>`("kind:key"), warnings=`Vec<String>`(VirtualApplyResult 그대로). 후속에 타입화 여지 |

---

## 2. 스코프 경계

| Phase 10 담당 | 담당 아님 (후속) |
|---|---|
| 코어 결과 → PreviewModel 합성 | AI 설명문 생성, Markdown/HTML |
| changes/access_changes/findings/warnings/deferred | 실제 Discord 실행 → Executor |
| verdict/approval_required/blocked 파생 | 승인 상태 저장 → Approval Manager |
| | DB, App/Web UI, operation 재계산, policy 재평가, AccessMatrix 재계산(호출자가 넘김) |

**의존 금지**: ai-gateway, executor, bot-runtime, db, approval-manager, web/app.

---

## 3. Crate 구조 & 의존
```
preview → {diff-engine, operation-graph, policy-engine, virtual-apply, simulator, discord-model}
```
합성 캡스톤이라 코어 다수 의존은 자연스러움(상위 실행/DB/AI로는 절대 X). 파일(예): `src/{lib.rs, model.rs, build.rs}`.

---

## 4. 타입 & API

```
struct PreviewModel {
    title: String,
    verdict: Verdict,                       // policy-engine 재사용
    approval_required: bool,
    blocked: bool,
    changes: Vec<PreviewChange>,
    access_changes: Vec<AccessChange>,
    policy_findings: Vec<Finding>,          // policy-engine 재사용
    warnings: Vec<String>,
    deferred: Vec<String>,
}

enum PreviewSeverity { Info, Notice, Warning }

enum PreviewChangeKind {
    RoleCreate, RoleUpdate, RoleDelete,
    ChannelCreate, ChannelUpdate, ChannelDelete,
    OverwriteCreate, OverwriteUpdate,
}

struct PreviewChange { kind: PreviewChangeKind, target: String, severity: PreviewSeverity }

struct AccessChange {
    subject: String, channel: String,
    before_can_view: bool, after_can_view: bool,
    before_can_send: bool, after_can_send: bool,
}
```
전부 `Clone + Debug + PartialEq + Eq + Serialize + Deserialize`.

---

## 5. 합성 로직 (심장)

`build_preview(...)`:
1. **verdict/approval/blocked**: `verdict = policy.verdict`. `approval_required = matches!(verdict, RequireApproval | RequireSecondApproval)`. `blocked = verdict == Deny`.
2. **changes** (graph.nodes 순회, op → PreviewChange):
   - CreateRole→(RoleCreate, name||key, Info) / UpdateRole→(RoleUpdate, ·, Info) / DeleteRole→(RoleDelete, ·, **Warning**)
   - CreateChannel→(ChannelCreate, ·, Info) / UpdateChannel→(ChannelUpdate, ·, Info) / DeleteChannel→(ChannelDelete, ·, **Warning**)
   - Create/UpdateOverwrite→(OverwriteCreate/Update, `"{channel} / {target_label}"`, target==Everyone? **Notice** : Info)
   - target_label: Everyone→`@everyone`, Role(key)→`role:{key}`, Member(id)→`member:{id}`. role/channel target: `name.clone().unwrap_or(key.0)`.
3. **access_changes** (D3): before/after를 `(subject,channel)→(can_view,can_send)` 맵으로. 키 합집합을 **정렬 순회**(BTreeSet, 결정적). 각 키 b=(before or false,false), a=(after or false,false). `b != a`면 AccessChange emit.
4. **policy_findings** = `policy.findings.clone()`.
5. **warnings** = `apply.warnings.clone()`.
6. **deferred** = `diff.deferred.iter().map(|d| format!("{}:{}", d.kind, d.key.0)).collect()`.

---

## 6. Phase 10 범위 경계
- ✅ 완전 구현: build_preview, PreviewModel/PreviewChange/AccessChange/PreviewSeverity/PreviewChangeKind, 6 필드 파생, matrix diff 합집합 규칙
- ❌ 제외: AI 문구, 실행, 승인상태, DB, UI, 재계산/재평가, 타입드 deferred/warning

---

## 7. 컨벤션
serde(PreviewModel 직렬화 — 앱/웹/audit)·주석 없음·결정적(changes=graph 순서, access_changes=정렬 순서). 비트 아님.

---

## 8. 테스트 전략 (⭐ 크라운 주얼)
- verdict 파생: RequireApproval→approval_required=true/blocked=false. Deny→blocked=true/approval_required=false. Allow→둘 다 false.
- changes: DeleteRole→Warning, @everyone overwrite→Notice, CreateRole→Info.
- access_changes: subject가 after에만 → before false, after true. view/send 동일하면 emit 안 함.
- deferred/warnings 매핑.
- **⭐ 인증 시나리오 full pipeline**: DesiredState→compile→diff→graph→policy→virtual-apply→(before/after matrix)→**build_preview**. 검증:
  - verdict=RequireApproval, approval_required=true, blocked=false.
  - changes에 RoleCreate(인증됨) + @everyone overwrite(Notice) 포함.
  - access_changes에 new_member/general (view true→false), verified_member/general (view false→true) 포함.
  - policy_findings에 EveryoneChangeRule의 finding 포함.
  - (Phase 3·4·5·7·8·9 전부가 preview까지 관통 — 코어→승인 데이터 완성.)

---

## 9. Codex 핸드오프
1. 크라운 주얼: before matrix는 `access_matrix(before_guild, subjects)`, after는 `access_matrix(after_guild, subjects)`. verified_member는 before엔 role 없어 after 전용 subject로 넣으면 false→true로 드러남.
2. matrix 페어링은 (subject,channel) String 키 — 동일 이름이면 페어. RoleId 달라도 무관.
3. Finding/Verdict/PolicyDecision은 policy-engine, AccessMatrix/AccessCell은 simulator, VirtualApplyResult는 virtual-apply, DiffResult/DeferredItem은 diff-engine, Operation/OperationGraph는 operation-graph에서.
4. 완료 게이트: build/test/clippy(-D warnings)/fmt. members에 `crates/preview` 추가. 완료 후 `git push origin main`.
