# Starring

Starring은 자연어 설계를 검증된 RuleSet으로 컴파일하고, 사람의 미리보기·승인·Apply 경계를 거쳐 Discord 자동화를 제공하는 Rust 기반 control plane과 runtime입니다. 모델은 후보 설계만 만들며 배포, Discord 실행, PostgreSQL 변경 권한을 갖지 않습니다.

## 현재 Backend V1 범위

- 한 대의 Mac mini와 하나의 canonical Discord shard
- `starring.private_study_room@1` recipe
- Luna-medium 기반의 동기식 bounded authoring
- 암호화된 PostgreSQL generation, Preview, 승인, Apply
- Requested-to-Live runtime, durable interaction receipt, deterministic preflight, effect journal, reconciliation과 bounded compensation
- recipe와 분리된 `AutomationSpec V1` 타입 계약, 정적 preview, 순수 simulation, source-map/컴파일 결속
- 별도 `StatefulSpec R0` 타입 계약, 결정론적 simulation, 상태형 워크플로를 legacy rule과 분리하는 불변 컴파일 번들, 검증형 효과 저널 계약, 배포 차단
- same-origin `/app` 자연어 작성·미리보기·승인·Apply·배포 상태 웹 콘솔

웹 콘솔은 현재 설치 ID를 직접 입력하는 얇은 제품 경계입니다. 설치 검색·관리 API, StatefulSpec의 live 저장·실행·배포, 타이머 기반 자동화, 임의 connector/HTTP, Discord game, multi-shard·multi-host HA, 비동기 authoring queue, 고부하 production SLO 인증은 포함하지 않습니다. 소스와 운영 절차는 Backend V1 후보 상태이며, 최종 인증은 disposable-guild D2, exact-tree D3, 동일 tree의 `main` 병합과 post-merge CI가 모두 완료된 D3 terminal record `<D3_RUN>/final.json`이 있을 때만 성립합니다.

## 시작점

- [현재 구현과 제한](CURRENT_STATE.md)
- [AutomationSpec V1 계약](docs/automation-spec-v1.md)
- [StatefulSpec R0 계약](docs/stateful-spec-r0.md)
- [Phase D 인증 handoff](docs/superpowers/handoffs/2026-08-01-commercial-certification-phase-d-handoff.md)
- [상용 완료 계획](docs/superpowers/plans/2026-07-29-authoring-runtime-commercial-completion.md)
- [API와 PostgreSQL cutover](docs/superpowers/runbooks/2026-07-19-production-control-plane-cutover.md)
- [통합 macOS staging cutover](docs/superpowers/runbooks/2026-07-29-macos-starring-integrated-staging-cutover.md)
- [runtime 운영](docs/superpowers/runbooks/2026-07-29-macos-starring-runtime-staging-operations.md)
- [Codex worker 운영](docs/superpowers/runbooks/2026-07-17-macos-codex-worker-operations.md)
- [D2 disposable-guild 인증 도구](tools/d2-certification/README.md)
- [D3 exact-tree 인증 도구](tools/d3-certification/README.md)

`docs/discord_ai_control_plane_architecture_oci.md`와 `docs/repo-structure.md`는 초기 역사 자료이며 현재 운영 기준이 아닙니다.
