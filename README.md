# Starring

AI 기반 Discord Control Plane. 자세한 아키텍처는 [docs/discord_ai_control_plane_architecture_oci.md](docs/discord_ai_control_plane_architecture_oci.md), 전체 레포 구조는 [docs/repo-structure.md](docs/repo-structure.md) 참고.

## Workspace

- `crates/discord-model` — 저수준 Discord 상태 모델 (ID, 권한, 엔티티, GuildState)
- `crates/domain` — 고수준 플랫폼 도메인 개념
