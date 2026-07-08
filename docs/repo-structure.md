# 전체 타깃 레포 구조 (계획)

> 아키텍처 문서 17장의 monorepo 구조. **현재는 계획**이며, 각 crate/service는 해당 Phase 구현 시점에 워크스페이스에 추가한다. 지금 물리 생성된 것은 `crates/discord-model`, `crates/domain`, `docs/`뿐이다.

```text
discord-ai-control-plane/
├─ apps/
│  ├─ web/                         # Next.js dashboard
│  └─ ios/                         # SwiftUI app
├─ services/
│  ├─ api/                         # Rust axum backend API
│  ├─ bot-runtime/                 # Rust twilight Discord bot
│  ├─ worker/                      # Rust background workers
│  ├─ verifier/                    # Rust verifier worker
│  └─ notification-worker/
├─ crates/
│  ├─ domain/                      # [생성됨] 고수준 도메인 개념
│  ├─ discord-model/               # [생성됨] 저수준 Discord 상태
│  ├─ desired-state/               # Desired State schema
│  ├─ diff-engine/                 # Current vs Desired 비교
│  ├─ operation-graph/             # 실행 그래프 모델/컴파일러
│  ├─ policy-engine/               # 정책 검사
│  ├─ simulator/                   # 권한/상태 시뮬레이터
│  ├─ ai-gateway/                  # vLLM/OpenAI-compatible client
│  ├─ event-bus/                   # NATS abstraction
│  ├─ db/                          # sqlx repositories
│  ├─ telemetry/                   # tracing/OpenTelemetry
│  └─ config/                      # 환경설정 로딩
├─ proto/                          # gRPC/protobuf
├─ infra/                          # docker / compose / terraform / k8s
├─ migrations/                     # sqlx migrations
├─ docs/                           # [생성됨]
├─ scripts/
└─ Cargo.toml
```
