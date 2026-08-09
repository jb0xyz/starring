# Starring

Starring은 자연어 설계를 검증된 RuleSet으로 컴파일하고, 사람의 미리보기·승인·Apply 경계를 거쳐 Discord 자동화를 제공하는 Rust 기반 control plane과 runtime입니다. 모델은 후보 설계만 만들며 배포, Discord 실행, PostgreSQL 변경 권한을 갖지 않습니다.

## 현재 Backend V1 범위

- 한 대의 Mac mini와 하나의 canonical Discord shard
- `starring.private_study_room@1` recipe
- Luna-medium 기반의 동기식 bounded authoring
- 암호화된 PostgreSQL generation, Preview, 승인, Apply
- Requested-to-Live runtime, durable interaction receipt, deterministic preflight, effect journal, reconciliation과 bounded compensation

Frontend, 임의 recipe와 Discord game, installation 관리 API, multi-shard·multi-host HA, 비동기 authoring queue, 고부하 production SLO 인증은 포함하지 않습니다. 소스와 운영 절차는 Backend V1 후보 상태이며, 최종 인증은 disposable-guild D2, exact-tree D3, 동일 tree의 `main` 병합과 post-merge CI가 모두 완료된 D3 terminal record `<D3_RUN>/final.json`이 있을 때만 성립합니다.

## 시작점

- [현재 구현과 제한](CURRENT_STATE.md)
- [Phase D 인증 handoff](docs/superpowers/handoffs/2026-08-01-commercial-certification-phase-d-handoff.md)
- [상용 완료 계획](docs/superpowers/plans/2026-07-29-authoring-runtime-commercial-completion.md)
- [API와 PostgreSQL cutover](docs/superpowers/runbooks/2026-07-19-production-control-plane-cutover.md)
- [통합 macOS staging cutover](docs/superpowers/runbooks/2026-07-29-macos-starring-integrated-staging-cutover.md)
- [runtime 운영](docs/superpowers/runbooks/2026-07-29-macos-starring-runtime-staging-operations.md)
- [Codex worker 운영](docs/superpowers/runbooks/2026-07-17-macos-codex-worker-operations.md)
- [D2 disposable-guild 인증 도구](tools/d2-certification/README.md)
- [D3 exact-tree 인증 도구](tools/d3-certification/README.md)

`docs/discord_ai_control_plane_architecture_oci.md`와 `docs/repo-structure.md`는 초기 역사 자료이며 현재 운영 기준이 아닙니다.
