# AI 기반 Discord Control Plane 아키텍처 정리

> [!WARNING]
> 이 문서는 현재 구현보다 오래된 설계 기록입니다(OCI/vLLM 전제 등 현재 인프라와 불일치).
> 현재 시스템 상태·크레이트 구조·검증 수준은 루트 `CURRENT_STATE.md`를 기준으로 확인하세요. 충돌 시 `CURRENT_STATE.md`가 우선합니다.

## 0. 문서 목적

이 문서는 AI 기반 Discord 서버 통합 운영 플랫폼의 전체 구조를 정리한 기술 기획 문서다.

본 프로젝트는 단순 Discord 봇이 아니라, Discord 서버를 AI와 대화하며 운영할 수 있는 **AI Discord Control Plane**을 목표로 한다.

핵심 철학은 다음과 같다.

> AI는 Discord API를 직접 실행하지 않는다.  
> AI는 서버의 목표 상태를 설계하고, 백엔드 Control Plane이 이를 검증 가능한 변경 계획으로 변환한 뒤, 사용자의 승인을 받아 안전하게 실행한다.

---

## 1. 제품의 최종 방향

본 서비스는 Discord 서버 운영자가 앱, 웹 대시보드, 또는 Discord 내부 인터페이스에서 AI와 대화하며 서버를 관리할 수 있는 올인원 운영 플랫폼이다.

예시 요청:

```text
신규 유저는 인증 채널만 보이고, 인증하면 일반 채널과 질문 채널을 볼 수 있게 해줘.
```

이 요청을 받은 AI는 직접 Discord 서버를 수정하지 않는다. 대신 AI는 서버가 어떤 상태가 되어야 하는지를 나타내는 **Desired State**를 생성한다.

백엔드는 현재 서버 상태와 AI가 제안한 목표 상태를 비교하고, 필요한 변경 사항을 계산한 뒤, 위험도와 예상 결과를 시뮬레이션하고, 사용자에게 미리보기로 보여준다.

사용자가 승인하면 Bot Runtime이 Discord API를 호출해 실제 서버를 변경한다.

---

## 2. 핵심 아키텍처 한 줄 요약

```text
User Prompt
→ AI Desired State
→ Backend Diff
→ Operation Graph
→ Policy
→ Simulation
→ Approval
→ Execution
→ Verification
→ Audit
→ Rollback
```

---

## 3. 전체 시스템 구조

```text
[SwiftUI iOS App]
        ↓
[Next.js Web Dashboard]
        ↓
[Rust Backend API - axum]
        ↓
[Rust Core Control Plane]
        ├─ Auth / User / Guild Management
        ├─ Context Builder
        ├─ AI Gateway
        ├─ Desired State Validator
        ├─ Diff Engine
        ├─ Operation Graph Compiler
        ├─ Policy Engine
        ├─ Simulator
        ├─ Approval Manager
        ├─ Job Orchestrator
        ├─ Verifier
        ├─ Audit Logger
        └─ Rollback Manager
        ↓
[NATS JetStream]
        ↓
[Rust Discord Bot Runtime - twilight]
        ↓
[Discord API]

[Rust Backend AI Gateway]
        ↓
[vLLM OpenAI-Compatible Server]
        ↓
[Gemma / Qwen / Future Model]

[PostgreSQL]
[Redis]
[OCI Object Storage]
[OCI Vault]
[OpenTelemetry / Logs / Metrics]
```

---

## 4. 역할 분리

### 4.1 AI

AI는 실행자가 아니라 설계자다.

AI가 담당하는 것:

```text
- 자연어 요청 이해
- 서버 상태 기반 목표 상태 설계
- Desired State Draft 생성
- 사용자에게 설명 가능한 요약 생성
```

AI가 하면 안 되는 것:

```text
- Discord API 직접 호출
- DB 직접 읽기/쓰기
- 사용자 승인 없이 실행
- 권한 검증 없이 실행
- 봇 토큰 접근
- 결제/계정 권한 변경
```

---

### 4.2 Backend Control Plane

백엔드는 이 서비스의 핵심이다.

백엔드가 담당하는 것:

```text
- 사용자 인증
- Discord OAuth 연동
- 서버별 권한 확인
- 서버 상태 로딩
- AI 요청 생성
- AI 응답 검증
- Desired State 검증
- Current State와 Desired State 비교
- Diff 생성
- Operation Graph 생성
- 정책 검사
- 시뮬레이션
- 승인 관리
- Job 생성
- 실행 결과 검증
- Audit Log 저장
- Rollback 데이터 저장
```

---

### 4.3 Discord Bot Runtime

Bot Runtime은 실행기다.

Bot Runtime이 담당하는 것:

```text
- Discord Gateway 연결
- Discord 이벤트 수신
- Discord API 호출
- 역할 생성/수정/삭제
- 채널 생성/수정/삭제
- 권한 수정
- 메시지/버튼 패널 생성
- 서버 이벤트를 백엔드로 보고
- Operation Node 실행 결과 보고
```

Bot Runtime은 AI와 직접 소통하지 않는다.

---

### 4.4 App / Web

앱과 웹은 사용자가 AI와 대화하고 변경 작업을 승인하는 인터페이스다.

App / Web이 담당하는 것:

```text
- AI 채팅 UI
- 서버 선택
- 서버 상태 확인
- 변경 미리보기
- 승인/거절
- 관리자 로그 확인
- 실시간 알림
- 롤백 요청
- 운영 대시보드
```

특히 모바일 앱은 단순 로그 확인 앱이 아니라, AI와 대화하며 Discord 서버를 실제로 운영하는 핵심 인터페이스다.

---

## 5. 핵심 처리 흐름

예시 요청:

```text
신규 유저는 인증 채널만 보이고, 인증하면 일반 채널 볼 수 있게 해줘.
```

처리 단계:

```text
1. App에서 자연어 요청 전송
2. Backend가 유저/서버 권한 확인
3. Context Builder가 필요한 서버 상태 수집
4. AI Gateway가 vLLM에 요청
5. AI가 Desired State 생성
6. Backend가 Desired State Schema 검증
7. Diff Engine이 현재 상태와 목표 상태 비교
8. Operation Graph Compiler가 실행 그래프 생성
9. Policy Engine이 위험 작업 검사
10. Simulator가 적용 후 결과 예측
11. App/Web에 Preview 표시
12. 사용자가 승인
13. Job Orchestrator가 NATS JetStream에 작업 발행
14. Bot Runtime이 작업 consume
15. Discord API 실행
16. Verifier가 실제 Discord 상태 재확인
17. Audit Log 저장
18. Rollback 데이터 저장
19. App/Web에 결과 표시
```

---

## 6. Desired State 중심 설계

### 6.1 나쁜 방식: 단순 Action JSON

```json
{
  "actions": [
    {
      "type": "CREATE_ROLE",
      "name": "인증됨"
    },
    {
      "type": "CREATE_CHANNEL",
      "name": "인증"
    }
  ]
}
```

이 방식은 MVP에는 간단하지만 장기적으로 한계가 있다.

문제점:

```text
- 이미 존재하는 리소스를 중복 생성할 수 있음
- 현재 서버 상태와의 차이를 계산하기 어려움
- 롤백 설계가 약함
- 의존성 표현이 부족함
- 복잡한 서버 운영 기능으로 확장하기 어려움
```

---

### 6.2 좋은 방식: Desired State

AI는 실행할 명령이 아니라, 서버가 도달해야 할 목표 상태를 만든다.

예시:

```yaml
guild_desired_state:
  access_control:
    onboarding_mode: verification_required

  roles:
    - key: verified_member
      name: 인증됨
      permissions: []

  channels:
    - key: verification_channel
      name: 인증
      type: text
      visibility:
        everyone: true
      features:
        verification_panel:
          enabled: true
          grants_role: verified_member

    - key: general_channel
      name: 일반
      visibility:
        everyone: false
        roles:
          verified_member: true
      permissions:
        verified_member:
          - VIEW_CHANNEL
          - SEND_MESSAGES
```

백엔드는 이 Desired State를 현재 상태와 비교해서 필요한 변경만 계산한다.

---

## 7. Diff Engine

Diff Engine은 Current State와 Desired State를 비교한다.

입력:

```text
- Current Guild State
- Desired Guild State
```

출력:

```text
- create
- update
- delete
- no-op
- conflict
```

예시:

```text
현재 상태:
- 인증됨 역할 없음
- 인증 채널 없음
- 일반 채널은 everyone이 볼 수 있음

목표 상태:
- 인증됨 역할 필요
- 인증 채널 필요
- 일반 채널은 인증됨만 접근 가능

Diff:
+ 인증됨 역할 생성
+ 인증 채널 생성
~ 일반 채널 권한 수정
```

중요한 점은 `no-op`이다.

같은 요청을 여러 번 해도, 이미 원하는 상태라면 아무 작업도 하지 않아야 한다.

---

## 8. Operation Graph

Diff는 “무엇이 달라져야 하는지”를 나타내고, Operation Graph는 “어떤 순서로 실행해야 하는지”를 나타낸다.

예시:

```yaml
operation_graph:
  nodes:
    - id: create_verified_role
      op: discord.role.create
      args:
        name: 인증됨
      produces:
        role_id: verified_role_id

    - id: create_verification_channel
      op: discord.channel.create
      args:
        name: 인증
        type: text
      produces:
        channel_id: verification_channel_id

    - id: update_general_permission
      op: discord.channel.permission.update
      depends_on:
        - create_verified_role
      args:
        channel_name: 일반
        target: verified_role_id
        allow:
          - VIEW_CHANNEL
          - SEND_MESSAGES

    - id: create_verification_panel
      op: app.verification.panel.create
      depends_on:
        - create_verified_role
        - create_verification_channel
      args:
        channel_id: verification_channel_id
        grants_role: verified_role_id
```

Operation Graph가 필요한 이유:

```text
- 작업 간 의존성 표현
- 병렬 실행 가능성 확보
- 중간 실패 처리
- 재시도 정책
- 롤백 순서 계산
- 실행 상태 추적
```

---

## 9. Policy Engine

AI가 만든 Desired State와 백엔드가 만든 Operation Graph는 반드시 정책 검사를 통과해야 한다.

정책 예시:

```text
- AI는 ADMINISTRATOR 권한을 직접 부여할 수 없다.
- @everyone 권한 변경은 관리자 승인 필수다.
- 채널 삭제는 고위험 작업이다.
- 역할 삭제는 해당 역할을 가진 멤버 수를 보여줘야 한다.
- 대량 변경은 2차 승인 필요다.
- 봇 권한보다 높은 역할은 수정할 수 없다.
- 서버 소유자 권한에 영향 주는 작업은 금지한다.
```

정책 결과:

```text
allow
warn
require_approval
require_second_approval
deny
```

장기적으로는 Policy as Code 구조를 고려할 수 있다.

예시:

```rego
deny[msg] {
  input.operation == "role.update"
  input.permissions[_] == "ADMINISTRATOR"
  msg := "AI는 관리자 권한을 직접 부여할 수 없습니다."
}

require_approval[msg] {
  input.target == "@everyone"
  msg := "@everyone 권한 변경은 관리자 승인이 필요합니다."
}
```

---

## 10. Simulator

Simulator는 변경 적용 전 예상 결과를 계산한다.

예시 Preview:

```text
적용 후 예상 상태

신규 유저:
- 볼 수 있음: #인증, #공지
- 볼 수 없음: #일반, #질문, #자유

인증된 유저:
- 볼 수 있음: #인증, #공지, #일반, #질문, #자유
- 메시지 가능: #일반, #질문, #자유

관리자:
- 영향 없음

생성될 항목:
+ 역할: 인증됨
+ 채널: #인증
+ 메시지: 인증 버튼 패널

수정될 항목:
~ #일반 권한
~ #질문 권한
~ #자유 권한

위험도:
중간

사유:
@everyone의 채널 보기 권한이 변경됩니다.
```

Simulator는 특히 권한 관련 작업에서 매우 중요하다.

---

## 11. Verifier

실행 후에는 Discord API 호출 성공만 믿으면 안 된다.

Verifier는 실제 Discord 서버 상태를 다시 조회해서 원하는 상태가 적용됐는지 확인한다.

예시:

```text
검증 결과

역할 생성: OK
#인증 채널 생성: OK
#일반 권한 수정: OK
인증 패널 메시지 생성: OK
인증됨 역할 지급 테스트: OK
```

실패 예시:

```text
#질문 채널 권한 수정 실패
사유: 봇의 역할 순서가 낮아 해당 권한을 수정할 수 없습니다.

가능한 해결:
1. 봇 역할을 더 위로 올리기
2. 해당 채널 권한은 수동으로 건너뛰기
3. 전체 작업 롤백
```

---

## 12. Rollback Engine

롤백은 나중에 붙이는 기능이 아니라 처음부터 Operation 모델에 포함되어야 한다.

모든 위험 Operation은 실행 전 before_state를 저장해야 한다.

예시:

```yaml
operation:
  id: update_general_permission
  op: discord.channel.permission.update
  before_state:
    channel_id: "123"
    target_id: "everyone"
    allow:
      - VIEW_CHANNEL
    deny: []
  after_state:
    channel_id: "123"
    target_id: "everyone"
    allow: []
    deny:
      - VIEW_CHANNEL
```

롤백 요청 예시:

```text
방금 한 작업 되돌려줘.
```

Rollback Graph 예시:

```text
1. 인증 패널 메시지 삭제
2. #일반 권한 이전 상태로 복구
3. #질문 권한 이전 상태로 복구
4. #인증 채널 삭제
5. 인증됨 역할 삭제
```

---

## 13. 최종 기술 스택

```text
Discord Bot Runtime: Rust + twilight
Core Control Plane: Rust
Backend API: Rust + axum
Async Runtime: tokio
DB: PostgreSQL
DB Access: sqlx
Queue/Event Bus: NATS JetStream
Internal RPC: gRPC/protobuf
AI Inference: vLLM OpenAI-compatible server
AI Model: Gemma 계열, 추후 Qwen 등 교체 가능
Cache/Rate Limit: Redis
Observability: tracing + OpenTelemetry
Web: Next.js
App: SwiftUI
Container Registry: OCI Container Registry
Cloud: Oracle Cloud Infrastructure
Secrets: OCI Vault
Large Snapshots: OCI Object Storage
```

---

## 14. AI Service 구조

AI inference는 Rust로 직접 구현하지 않는다.

추천 구조:

```text
Rust Backend / Control Plane
  ├─ AI Gateway
  ├─ Prompt Builder
  ├─ Context Builder
  ├─ Desired State Validator
  └─ Retry / Repair Logic

AI Inference Runtime
  └─ vLLM OpenAI-Compatible Server
```

AI 요청 방식:

```text
Rust Backend → HTTP → vLLM /v1/chat/completions
```

AI Business Logic은 Rust에 둔다.

vLLM은 모델 추론만 담당한다.

---

## 15. NATS JetStream 사용 방식

NATS JetStream은 이벤트 버스와 durable job queue 역할을 한다.

NATS subject 예시:

```text
guild.*.events.discord.*
guild.*.state.changed
guild.*.ai.plan.requested
guild.*.ai.plan.generated
guild.*.approval.requested
guild.*.approval.approved
guild.*.jobs.created
guild.*.jobs.started
guild.*.jobs.step.completed
guild.*.jobs.completed
guild.*.jobs.failed
guild.*.rollback.requested
guild.*.audit.created
notifications.push.requested
```

Stream 구성 예시:

```text
DISCORD_EVENTS
- guild.*.events.discord.*

AI_PLANS
- guild.*.ai.*

APPROVALS
- guild.*.approval.*

JOBS
- guild.*.jobs.*

AUDIT
- guild.*.audit.*

NOTIFICATIONS
- notifications.*
```

역할 분리:

```text
NATS Core = 실시간 pub/sub
NATS JetStream = durable event/job stream
Redis = 캐시, rate limit, TTL, idempotency key
```

---

## 16. Internal RPC

gRPC/protobuf는 즉시 응답이 필요한 내부 호출에 사용한다.

예시 서비스:

```text
BotRuntimeService
- GetGuildLiveState
- CheckBotCapabilities
- GetShardStatus
- ExecuteOperationNode 선택

VerifierService
- VerifyJob
- VerifyPermissionState

ControlPlaneService
- SubmitDiscordEvent
```

기준:

```text
NATS = 비동기 이벤트/작업
gRPC = 동기 조회/즉시 명령
```

---

## 17. Git 레포 관리 방식

초기부터 monorepo를 추천한다.

이유:

```text
- 공통 도메인 타입 공유가 중요함
- Rust workspace 관리가 쉬움
- proto 공유가 쉬움
- CI/CD 통합이 쉬움
- AI 코딩 시 전체 컨텍스트 유지가 쉬움
```

추천 레포 구조:

```text
discord-ai-control-plane/
├─ apps/
│  ├─ web/                         # Next.js dashboard
│  └─ ios/                         # SwiftUI app
│
├─ services/
│  ├─ api/                         # Rust axum backend API
│  ├─ bot-runtime/                 # Rust twilight Discord bot
│  ├─ worker/                      # Rust background workers
│  ├─ verifier/                    # Rust verifier worker
│  └─ notification-worker/         # push/email/discord notifications
│
├─ crates/
│  ├─ domain/                      # 핵심 도메인 타입
│  ├─ discord-model/               # Discord 상태 추상화
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
│
├─ proto/
│  ├─ controlplane/v1/
│  │  ├─ bot.proto
│  │  ├─ jobs.proto
│  │  └─ guild_state.proto
│
├─ infra/
│  ├─ docker/
│  │  ├─ api.Dockerfile
│  │  ├─ bot-runtime.Dockerfile
│  │  ├─ worker.Dockerfile
│  │  └─ verifier.Dockerfile
│  │
│  ├─ compose/
│  │  ├─ docker-compose.dev.yml
│  │  └─ docker-compose.prod-lite.yml
│  │
│  ├─ terraform/
│  │  ├─ environments/
│  │  │  ├─ dev/
│  │  │  ├─ staging/
│  │  │  └─ prod/
│  │  └─ modules/
│  │     ├─ network/
│  │     ├─ compute/
│  │     ├─ database/
│  │     ├─ object-storage/
│  │     ├─ registry/
│  │     └─ vault/
│  │
│  └─ k8s/
│     ├─ base/
│     └─ overlays/
│        ├─ staging/
│        └─ prod/
│
├─ migrations/                     # sqlx migrations
├─ docs/
│  ├─ architecture.md
│  ├─ desired-state-spec.md
│  ├─ operation-graph-spec.md
│  ├─ policy-engine.md
│  ├─ deployment-oci.md
│  └─ runbook.md
│
├─ scripts/
│  ├─ dev.sh
│  ├─ migrate.sh
│  ├─ seed.sh
│  └─ deploy.sh
│
├─ .github/
│  └─ workflows/
│     ├─ rust-ci.yml
│     ├─ web-ci.yml
│     ├─ docker-build.yml
│     └─ deploy-staging.yml
│
├─ Cargo.toml
├─ Cargo.lock
├─ package.json
├─ pnpm-workspace.yaml
├─ Makefile
└─ README.md
```

---

## 18. Rust Workspace 구성

루트 `Cargo.toml` 예시:

```toml
[workspace]
members = [
  "services/api",
  "services/bot-runtime",
  "services/worker",
  "services/verifier",
  "services/notification-worker",

  "crates/domain",
  "crates/discord-model",
  "crates/desired-state",
  "crates/diff-engine",
  "crates/operation-graph",
  "crates/policy-engine",
  "crates/simulator",
  "crates/ai-gateway",
  "crates/event-bus",
  "crates/db",
  "crates/telemetry",
  "crates/config",
]
resolver = "2"
```

설계 원칙:

```text
services/* = 실행 바이너리
crates/* = 재사용 가능한 핵심 로직
```

서비스는 얇게 유지하고, 핵심 로직은 crates에 둔다.

---

## 19. Git 브랜치 전략

초기에는 단순한 브랜치 전략이 좋다.

```text
main       = 항상 배포 가능한 안정 버전
develop    = 통합 개발 브랜치
feature/*  = 기능 브랜치
fix/*      = 버그 수정
infra/*    = 인프라 변경
docs/*     = 문서 변경
```

예시:

```text
feature/desired-state-schema
feature/diff-engine-v1
feature/twilight-bot-runtime
feature/ai-gateway-vllm
infra/oci-dev-environment
```

PR 규칙:

```text
main 직접 push 금지
PR 필수
Rust CI 통과 필수
cargo fmt 통과 필수
cargo clippy 통과 필수
테스트 통과 필수
DB migration 있으면 리뷰 필수
infra 변경은 별도 리뷰
```

---

## 20. CI/CD

### Rust CI

```text
cargo fmt --check
cargo clippy --all-targets --all-features
cargo test
sqlx prepare check
docker build
```

### Web CI

```text
pnpm install
pnpm lint
pnpm typecheck
pnpm build
```

### Infra CI

```text
terraform fmt
terraform validate
tflint 선택
```

이미지는 OCI Container Registry에 push한다.

---

## 21. Oracle Cloud 배포 전략

Oracle 배포는 단계적으로 간다.

### Phase 1: Prod-lite / MVP

초기에는 Kubernetes 없이 OCI Compute + Docker Compose로 시작한다.

```text
OCI Compute Instance 1: app-node
- Rust API
- Bot Runtime
- Worker
- Verifier
- NATS
- Redis
- Reverse Proxy

OCI Compute Instance 2: ai-node
- vLLM
- model storage

OCI Compute Instance 3: db-node
- PostgreSQL
```

---

### Phase 2: Staging / Production 분리

```text
dev:
- local docker compose

staging:
- 작은 OCI Compute 1~2대
- staging DB
- staging Discord bot application
- staging domain

prod:
- app-node
- bot-node
- worker-node
- ai-node
- db-node or managed DB
- object storage
- vault
```

Discord 봇도 환경별로 분리한다.

```text
Discord App Dev
Discord App Staging
Discord App Production
```

---

### Phase 3: OKE 이전

서비스가 커지면 Oracle Kubernetes Engine으로 이전한다.

```text
OKE Cluster
├─ api deployment
├─ bot-runtime deployment
├─ worker deployment
├─ verifier deployment
├─ notification-worker deployment
├─ nats statefulset
├─ redis deployment
└─ ingress controller

Separate:
├─ PostgreSQL
├─ Object Storage
├─ Vault
└─ vLLM AI nodes
```

봇이 커지면 shard 단위로 bot-runtime을 나눈다.

```text
bot-runtime-shard-0
bot-runtime-shard-1
bot-runtime-shard-2
```

---

## 22. OCI 네트워크 구조

MVP 기준 추천 구조:

```text
VCN
├─ Public Subnet
│  ├─ Load Balancer 또는 Reverse Proxy VM
│  └─ Bastion 선택
│
├─ Private App Subnet
│  ├─ app-node
│  ├─ bot-node
│  └─ worker-node
│
├─ Private AI Subnet
│  └─ ai-node
│
└─ Private DB Subnet
   └─ postgres-node
```

초기 비용과 단순성을 위해 처음에는 app-node 하나에 여러 컨테이너를 같이 올려도 된다.

```text
app-node:
- api
- bot-runtime
- worker
- verifier
- nats
- redis
- caddy/nginx

ai-node:
- vLLM
- model files

db-node:
- PostgreSQL
```

---

## 23. OCI 네트워크 보안 원칙

기본 원칙:

```text
외부에 열리는 것은 HTTPS 443만
vLLM은 public internet에 노출 금지
PostgreSQL은 private subnet only
NATS는 private subnet only
Redis는 private subnet only
Bot Runtime은 outbound로 Discord Gateway/API 접근
```

포트 예시:

```text
443    public HTTPS
80     HTTP redirect only
8080   internal API container
4222   NATS client, private only
8222   NATS monitoring, private/admin only
6379   Redis, private only
5432   PostgreSQL, private only
8000   vLLM, private only
4317   OTLP gRPC, private only
4318   OTLP HTTP, private only
```

---

## 24. Container 배포 방식

이미지 빌드 흐름:

```text
GitHub Actions
  ↓
Docker build
  ↓
Push to OCI Container Registry
  ↓
OCI Compute pulls image
  ↓
Docker Compose restart
```

초기 배포 명령:

```bash
ssh app-node
docker compose pull
docker compose up -d
docker compose logs -f
```

---

## 25. docker-compose.prod-lite.yml 예시

```yaml
services:
  api:
    image: <ocir>/discord-ai/api:${VERSION}
    restart: unless-stopped
    env_file: .env
    depends_on:
      - nats
      - redis
    ports:
      - "127.0.0.1:8080:8080"

  bot-runtime:
    image: <ocir>/discord-ai/bot-runtime:${VERSION}
    restart: unless-stopped
    env_file: .env
    depends_on:
      - nats

  worker:
    image: <ocir>/discord-ai/worker:${VERSION}
    restart: unless-stopped
    env_file: .env
    depends_on:
      - nats
      - redis

  verifier:
    image: <ocir>/discord-ai/verifier:${VERSION}
    restart: unless-stopped
    env_file: .env
    depends_on:
      - nats

  nats:
    image: nats:latest
    restart: unless-stopped
    command: ["-js", "-sd", "/data"]
    volumes:
      - nats_data:/data
    ports:
      - "127.0.0.1:4222:4222"
      - "127.0.0.1:8222:8222"

  redis:
    image: redis:latest
    restart: unless-stopped
    command: ["redis-server", "--appendonly", "yes"]
    volumes:
      - redis_data:/data
    ports:
      - "127.0.0.1:6379:6379"

volumes:
  nats_data:
  redis_data:
```

PostgreSQL은 실제 운영에서는 compose에 넣기보다 별도 VM 또는 관리형 DB로 분리하는 것을 추천한다.

---

## 26. AI Node 구성

AI node는 별도로 둔다.

```text
ai-node:
- vLLM container
- model cache volume
- private IP only
- app-node에서만 접근 허용
```

vLLM 실행 예시:

```bash
vllm serve google/gemma-2-2b-it \
  --host 0.0.0.0 \
  --port 8000 \
  --api-key "$VLLM_API_KEY"
```

Rust Backend 환경변수:

```text
AI_BASE_URL=http://ai-node-private-ip:8000/v1
AI_API_KEY=...
AI_MODEL=google/gemma-2-2b-it
```

---

## 27. PostgreSQL 운영

초기에는 별도 DB VM으로 시작한다.

```text
db-node:
- PostgreSQL
- private subnet
- encrypted block volume
- daily backup
- WAL archive 선택
```

DB 예시:

```text
app_prod
app_staging
```

Migration은 `sqlx migrate`로 관리한다.

중요한 데이터:

```text
users
guilds
guild_memberships
guild_settings
guild_roles
guild_channels
ai_conversations
ai_messages
desired_states
action_plans
operation_graphs
jobs
job_steps
audit_logs
snapshots
rollback_records
usage_records
```

---

## 28. Object Storage 사용

큰 데이터는 DB가 아니라 Object Storage에 저장한다.

사용 예:

```text
guild snapshots
rollback archive
large audit export
model artifacts 선택
backup files
```

Object key 예시:

```text
snapshots/guild_id/job_id/snapshot.json.zst
audit/guild_id/yyyy/mm/dd/events.jsonl.zst
```

DB에는 object key만 저장한다.

---

## 29. Secrets 관리

GitHub repo나 일반 `.env`에 실제 secret을 넣지 않는다.

Secret 예시:

```text
DISCORD_BOT_TOKEN
DISCORD_CLIENT_SECRET
DATABASE_URL
NATS_PASSWORD
REDIS_PASSWORD
VLLM_API_KEY
JWT_SECRET
APPLE_PUSH_KEY
OAUTH_SECRET
```

운영 단계에서는 OCI Vault에서 secret을 가져오는 구조를 사용한다.

초기에는 제한적으로 `.env`를 사용할 수 있지만, production에서는 Vault를 기본으로 한다.

---

## 30. Observability

Rust 서비스에는 처음부터 `tracing`을 넣는다.

로그 필드:

```text
request_id
guild_id
user_id
job_id
operation_id
action_plan_id
discord_request_id
risk_level
```

OpenTelemetry trace 흐름:

```text
App request
→ API
→ AI Gateway
→ Diff Engine
→ NATS Job
→ Bot Runtime
→ Discord API
→ Verifier
```

이 trace가 연결되어야 문제 발생 시 디버깅이 가능하다.

---

## 31. 환경 분리

환경은 최소 3개로 분리한다.

```text
local
staging
production
```

각 환경별 분리 대상:

```text
Discord Application
Database
NATS stream
Redis
vLLM API key
OAuth redirect URI
Apple push environment
Domain
Object Storage bucket
Vault secrets
```

도메인 예:

```text
api-dev.example.com
api-staging.example.com
api.example.com

app-staging.example.com
dashboard.example.com
```

---

## 32. 실제 Oracle 운영 순서

### 1단계: Local 개발 환경

```text
docker compose dev:
- postgres
- nats
- redis
- mock-vllm or local vLLM
- api
- bot-runtime
```

---

### 2단계: OCI 기본 인프라

```text
VCN 생성
Public subnet 생성
Private subnet 생성
Security list/NSG 설정
app-node 생성
ai-node 생성
db-node 생성
Object Storage bucket 생성
Vault 생성
OCIR repository 생성
```

---

### 3단계: 컨테이너 배포

```text
GitHub Actions에서 Docker build
OCIR push
app-node에서 pull
docker compose up -d
```

---

### 4단계: DB 설정

```text
PostgreSQL 설치
DB/user 생성
migration 실행
backup policy 설정
```

---

### 5단계: Discord 설정

```text
Discord Developer Portal
- production bot 생성
- OAuth redirect URI 등록
- bot permissions 설정
- privileged intents 설정
- token을 Vault에 저장
```

---

### 6단계: vLLM 설정

```text
ai-node에 vLLM container 실행
모델 다운로드/cache
private endpoint로만 노출
backend에서 health check
```

---

### 7단계: Observability 설정

```text
structured logs
OpenTelemetry collector
metrics endpoint
alert rules
```

---

## 33. 초기 MVP 배포 형태

처음에는 아래 구성이 현실적이다.

```text
app-node:
- api
- bot-runtime
- worker
- verifier
- nats
- redis
- caddy

db-node:
- postgresql

ai-node:
- vLLM
```

---

## 34. 장기 Production 형태

나중에는 다음 구조로 확장한다.

```text
Load Balancer
  ↓
OKE
  ├─ api replicas
  ├─ worker replicas
  ├─ verifier replicas
  ├─ notification-worker replicas
  ├─ bot-runtime shard workers
  ├─ nats cluster
  └─ redis

Separate:
- PostgreSQL HA
- vLLM AI nodes
- Object Storage
- Vault
- Monitoring stack
```

---

## 35. 반드시 지켜야 할 설계 원칙

```text
AI는 Discord API를 직접 호출하지 않는다.
AI는 DB를 직접 읽거나 쓰지 않는다.
AI 출력은 Desired State Draft일 뿐이다.
백엔드가 모든 AI 출력을 검증한다.
실행은 Operation Graph만 가능하다.
Operation Graph는 승인 없이는 실행될 수 없다.
모든 Operation은 audit log를 남긴다.
모든 위험 Operation은 policy engine을 통과한다.
권한/역할/채널 변경은 before_state를 저장한다.
실행 후 verifier가 실제 Discord 상태를 확인한다.
롤백은 나중에 붙이는 기능이 아니라 처음부터 포함한다.
```

---

## 36. 개발 우선순위

### Phase 1. Core State Model

```text
Guild
Role
Channel
PermissionOverwrite
Member
Feature
VerificationPanel
ModerationRule
LoggingRule
```

---

### Phase 2. Desired State Schema

```text
roles
channels
permissions
verification_panels
logging_rules
```

---

### Phase 3. Diff Engine

```text
create
update
delete
no-op
conflict
```

---

### Phase 4. Operation Graph Compiler

```text
depends_on
produces
consumes
rollback
retry_policy
timeout
```

---

### Phase 5. Policy Engine

```text
deny
warn
require_approval
require_second_approval
allow
```

---

### Phase 6. Simulator

```text
신규 유저는 어떤 채널을 볼 수 있는가?
인증된 유저는 어떤 채널에 메시지를 보낼 수 있는가?
관리자 역할은 영향 받는가?
```

---

### Phase 7. Executor + Verifier

```text
Operation 실행
실제 Discord 상태 재조회
성공/실패 검증
```

---

## 37. 최종 요약

이 프로젝트의 최종 형태:

```text
Rust Backend API + Rust Control Plane
Rust Discord Bot Runtime
vLLM AI Inference Server
NATS JetStream Event Bus
PostgreSQL State Store
Redis Cache/Rate Limit
OCI Object Storage Snapshot Archive
OCI Vault Secrets
Next.js Dashboard
SwiftUI App
```

Git은 monorepo로 관리하고, Rust core는 workspace crate로 강하게 분리한다.

Oracle 배포는 초기에는 다음 방식으로 시작한다.

```text
OCI Compute + Docker Compose + OCIR + Vault + Object Storage
```

서비스가 커지면 다음 구조로 이동한다.

```text
OKE + Scaled Rust Services + NATS Cluster + AI Nodes
```

가장 중요한 제품 철학:

```text
AI는 목표 상태를 설계한다.
Backend Control Plane은 그것을 검증 가능한 변경 그래프로 컴파일한다.
사용자는 변경 내용을 승인한다.
Bot Runtime은 승인된 작업만 실행한다.
Verifier는 실제 서버 상태를 검증한다.
Audit Logger와 Rollback Manager는 모든 변경을 추적하고 복구 가능하게 만든다.
```

이 구조를 지키면 본 서비스는 단순 Discord 봇이 아니라, **AI 기반 Discord 서버 운영 Control Plane**으로 발전할 수 있다.
