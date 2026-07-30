use std::num::NonZeroU64;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use authoring_application::{
    AuthenticatedActorV1, AuthenticatedSessionFingerprintV1, AuthenticationClaimsV1,
    AuthenticationError, AuthenticationPort, AuthoringCommitOutcomeV1,
    AuthoringConversationConfigV1, AuthoringSessionCommitPort, AuthoringSessionLoadError,
    AuthoringSessionLoadPort, AuthoringSessionLoadV1, AuthoringStoredGenerationV1,
    AuthoringStoredRequestIdentityV1, AuthoringTurnAdmissionPort, AuthoringTurnCheckV1,
    AuthorizedAuthoringCommitV1, AuthorizedInstallationScopeV1, AuthorizedInstallationV1,
    CapabilityV1, ConversationApplication, FreshGuildAuthorityError, FreshGuildAuthorityEvidence,
    FreshGuildAuthorityPort, InstallationSelectorV1, MutationAuthenticationPort,
};
use authoring_promotion::{AutomationInstallationId, PrincipalId, SessionGeneration, TenantId};
use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::{Request, Response, StatusCode};
use axum::response::{IntoResponse, Response as AxumResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use design_harness_codex_worker_client::CodexWorkerClient;
use discord_model::{GuildId, UserId};
use product_control_http::{
    product_control_router_with_authoring_v1, ApplyCommand, ApplyView, ApprovalPreviewView,
    AuthoringHttpBoundaryConfigV1, AuthoringSessionViewV1, AuthoringTurnCommandV1,
    AuthoringTurnViewV1, CsrfSecret, CurrentPrincipal, DecisionCommand, DecisionView,
    DeploymentView, FacadeError, FacadeErrorCode, HttpBoundaryConfig, OAuthCallbackCommand,
    OAuthCallbackResult, OAuthStartCommand, OAuthStartResult, ProductControlAuthoringFacadeV1,
    ProductControlFacade, PromoteCommand, PromotionView, RejectCommand, SessionCredential,
};
use serde_json::{json, Value};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tower::ServiceExt;

use starring_api::{
    map_authoring_conversation_error, map_authoring_turn_command, project_authoring_turn,
    AuthoringAdmissionConfigV1, AuthoringAdmissionV1,
};

const HOST: &str = "starring.example";
const ORIGIN: &str = "https://starring.example";
const SESSION: &str = "sssssssssssssssssssssssssssssssssssssssssss";
const CSRF: &str = "ccccccccccccccccccccccccccccccccccccccccccc";
const IDEMPOTENCY: &str = "iiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiii";

#[derive(Clone)]
struct Evidence {
    tenant_id: TenantId,
    installation_id: AutomationInstallationId,
}

impl Evidence {
    fn stable() -> Self {
        Self {
            tenant_id: TenantId::parse("tenant-1").unwrap(),
            installation_id: AutomationInstallationId::parse("installation-1").unwrap(),
        }
    }
}

impl FreshGuildAuthorityEvidence for Evidence {
    fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    fn installation_id(&self) -> &AutomationInstallationId {
        &self.installation_id
    }

    fn discord_application_id(&self) -> NonZeroU64 {
        NonZeroU64::new(30).unwrap()
    }

    fn guild_id(&self) -> GuildId {
        GuildId(10)
    }

    fn acting_user_id(&self) -> UserId {
        UserId(20)
    }

    fn capability(&self) -> CapabilityV1 {
        CapabilityV1::Author
    }

    fn guild_owner(&self) -> bool {
        true
    }

    fn effective_permissions_bits(&self) -> u64 {
        0
    }

    fn installation_authority_revision(&self) -> NonZeroU64 {
        NonZeroU64::new(7).unwrap()
    }

    fn installation_authority_digest(&self) -> &str {
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    }

    fn observation_digest(&self) -> &str {
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    }

    fn observed_at(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(100)
    }

    fn expires_at(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(105)
    }
}

struct Authentication;

impl AuthenticationPort for Authentication {
    type Credential = str;

    async fn authenticate(
        &self,
        credential: &Self::Credential,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        if credential != SESSION {
            return Err(AuthenticationError::InvalidCredential);
        }
        Ok(AuthenticationClaimsV1::from_authentication(
            PrincipalId::parse("principal-1").unwrap(),
            AuthenticatedSessionFingerprintV1::from_sha256_digest([7; 32]),
        ))
    }
}

impl MutationAuthenticationPort for Authentication {
    type CsrfProof = str;

    async fn authenticate_mutation(
        &self,
        credential: &Self::Credential,
        csrf: &Self::CsrfProof,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        if csrf != CSRF {
            return Err(AuthenticationError::InvalidCsrf);
        }
        self.authenticate(credential).await
    }
}

struct Authority;

impl FreshGuildAuthorityPort for Authority {
    type Evidence = Evidence;

    async fn authorize_installation(
        &self,
        actor: &AuthenticatedActorV1,
        installation: &InstallationSelectorV1,
        capability: CapabilityV1,
    ) -> Result<AuthorizedInstallationV1<Self::Evidence>, FreshGuildAuthorityError> {
        if actor.principal_id().as_str() != "principal-1"
            || installation.installation_id().as_str() != "installation-1"
            || capability != CapabilityV1::Author
        {
            return Err(FreshGuildAuthorityError::Forbidden);
        }
        Ok(AuthorizedInstallationV1::from_fresh_authority(
            AuthorizedInstallationScopeV1::from_fresh_authority(
                TenantId::parse("tenant-1").unwrap(),
                AutomationInstallationId::parse("installation-1").unwrap(),
                GuildId(10),
                UserId(20),
            ),
            Evidence::stable(),
        ))
    }
}

#[derive(Clone)]
struct StoredRecord {
    session_id: String,
    idempotency_key: String,
    expected_generation: u64,
    human_message: String,
    identity: AuthoringStoredRequestIdentityV1,
    generation: SessionGeneration,
    projection: authoring_application::SafeAuthoringTurnProjectionV1,
}

#[derive(Default)]
struct StoreState {
    head_generation: Option<SessionGeneration>,
    records: Vec<StoredRecord>,
    checks: usize,
    loads: usize,
    commits: usize,
}

struct Store {
    state: Mutex<StoreState>,
}

impl Store {
    fn new() -> Self {
        Self {
            state: Mutex::new(StoreState::default()),
        }
    }

    fn counts(&self) -> (usize, usize, usize) {
        let state = self.state.lock().unwrap();
        (state.checks, state.loads, state.commits)
    }
}

impl AuthoringSessionLoadPort<Evidence> for Store {
    async fn check_replay_or_head(
        &self,
        access: &authoring_application::AuthorizedConversationAccessV1<'_, Evidence>,
    ) -> Result<AuthoringTurnCheckV1, AuthoringSessionLoadError> {
        let command = access.command();
        let mut state = self.state.lock().unwrap();
        state.checks += 1;
        if let Some(record) = state.records.iter().find(|record| {
            record.session_id == command.session_id().as_str()
                && record.idempotency_key == command.idempotency_key().as_str()
        }) {
            if record.expected_generation == command.expected_generation().get()
                && record.human_message == command.human_message().as_str()
            {
                return Ok(AuthoringTurnCheckV1::ExactReplay(
                    AuthoringStoredGenerationV1::from_storage(
                        record.identity.clone(),
                        record.generation,
                        record.projection.clone(),
                        None,
                    )
                    .unwrap(),
                ));
            }
            return Ok(AuthoringTurnCheckV1::IdempotencyConflict);
        }
        let current_generation = state.head_generation;
        if current_generation.map(SessionGeneration::get).unwrap_or(0)
            == command.expected_generation().get()
        {
            Ok(AuthoringTurnCheckV1::Proceed)
        } else {
            Ok(AuthoringTurnCheckV1::GenerationConflict { current_generation })
        }
    }

    async fn load_exact_generation(
        &self,
        _access: &authoring_application::AuthorizedConversationAccessV1<'_, Evidence>,
    ) -> Result<AuthoringSessionLoadV1, AuthoringSessionLoadError> {
        let mut state = self.state.lock().unwrap();
        state.loads += 1;
        assert!(state.head_generation.is_none());
        AuthoringSessionLoadV1::from_storage(None, None, Default::default())
    }
}

impl AuthoringSessionCommitPort<Evidence> for Store {
    async fn commit_authorized_generation(
        &self,
        request: AuthorizedAuthoringCommitV1<'_, Evidence>,
    ) -> Result<AuthoringCommitOutcomeV1, AuthoringSessionLoadError> {
        let command = request.access().command();
        let identity = AuthoringStoredRequestIdentityV1::from_verified_storage_match(
            request.access().scope().clone(),
            request.access().actor().principal_id().clone(),
            command.session_id().clone(),
            command.expected_generation(),
            command.idempotency_key().clone(),
            command.human_message().clone(),
        );
        let mut state = self.state.lock().unwrap();
        state.commits += 1;
        let current_generation = state.head_generation;
        if current_generation.map(SessionGeneration::get).unwrap_or(0)
            != command.expected_generation().get()
        {
            return Ok(AuthoringCommitOutcomeV1::GenerationConflict { current_generation });
        }
        let generation = SessionGeneration::new(command.expected_generation().get() + 1).unwrap();
        let projection = request.projection().clone();
        assert!(request.preview_ready_artifact().is_none());
        state.head_generation = Some(generation);
        state.records.push(StoredRecord {
            session_id: command.session_id().as_str().to_string(),
            idempotency_key: command.idempotency_key().as_str().to_string(),
            expected_generation: command.expected_generation().get(),
            human_message: command.human_message().as_str().to_string(),
            identity: identity.clone(),
            generation,
            projection: projection.clone(),
        });
        Ok(AuthoringCommitOutcomeV1::Created(
            AuthoringStoredGenerationV1::from_storage(identity, generation, projection, None)
                .unwrap(),
        ))
    }
}

#[derive(Clone, Copy)]
enum WorkerMode {
    Immediate,
    Delayed(Duration),
    Blocked,
}

struct WorkerState {
    mode: WorkerMode,
    health_available: bool,
    health_calls: AtomicUsize,
    calls: AtomicUsize,
    settled: AtomicUsize,
    started: Notify,
    settlement: Notify,
    released: AtomicBool,
    release: Notify,
}

struct WorkerServer {
    base_url: String,
    state: Arc<WorkerState>,
    task: JoinHandle<()>,
}

impl WorkerServer {
    async fn start(mode: WorkerMode, health_available: bool) -> Self {
        let state = Arc::new(WorkerState {
            mode,
            health_available,
            health_calls: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
            settled: AtomicUsize::new(0),
            started: Notify::new(),
            settlement: Notify::new(),
            released: AtomicBool::new(false),
            release: Notify::new(),
        });
        let router = Router::new()
            .route("/health", get(worker_health))
            .route("/v1/frontier-completions", post(worker_completion))
            .with_state(Arc::clone(&state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self {
            base_url: format!("http://{address}"),
            state,
            task,
        }
    }

    fn client(&self) -> CodexWorkerClient {
        CodexWorkerClient::new(self.base_url.clone(), "test-worker-token".to_string()).unwrap()
    }

    fn health_calls(&self) -> usize {
        self.state.health_calls.load(Ordering::SeqCst)
    }

    fn calls(&self) -> usize {
        self.state.calls.load(Ordering::SeqCst)
    }

    fn settled(&self) -> usize {
        self.state.settled.load(Ordering::SeqCst)
    }

    async fn wait_started(&self) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let started = self.state.started.notified();
                if self.calls() != 0 {
                    return;
                }
                started.await;
            }
        })
        .await
        .expect("worker request did not start");
    }

    async fn wait_settled(&self) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let settlement = self.state.settlement.notified();
                if self.settled() != 0 {
                    return;
                }
                settlement.await;
            }
        })
        .await
        .expect("worker request did not settle");
    }

    fn release(&self) {
        self.state.released.store(true, Ordering::SeqCst);
        self.state.release.notify_waiters();
    }
}

impl Drop for WorkerServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn worker_health(State(state): State<Arc<WorkerState>>) -> AxumResponse {
    state.health_calls.fetch_add(1, Ordering::SeqCst);
    if !state.health_available {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    Json(json!({
        "schema_version": 1,
        "status": "ok",
        "provider": "codex_chatgpt",
        "model": "gpt-5.6-luna",
        "reasoning_effort": "medium",
        "auth_mode": "chatgpt",
        "codex_cli_version": "codex-cli 0.146.0-alpha.3.1",
        "instance_id": "authoring-http-integration",
        "worker_source_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "concurrency_limit": 1,
        "queue_capacity": 0,
        "request_timeout_ms": 55000,
        "active_requests": 0,
        "queued_requests": 0,
        "accepted_requests_total": 0,
        "settled_requests_total": 0
    }))
    .into_response()
}

async fn worker_completion(
    State(state): State<Arc<WorkerState>>,
    Json(request): Json<Value>,
) -> Json<Value> {
    state.calls.fetch_add(1, Ordering::SeqCst);
    state.started.notify_waiters();
    match state.mode {
        WorkerMode::Immediate => {
            state.settled.fetch_add(1, Ordering::SeqCst);
            state.settlement.notify_waiters();
        }
        WorkerMode::Delayed(delay) => {
            let settled = Arc::clone(&state);
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                settled.settled.fetch_add(1, Ordering::SeqCst);
                settled.settlement.notify_waiters();
            });
            tokio::time::sleep(delay).await;
        }
        WorkerMode::Blocked => {
            loop {
                let released = state.release.notified();
                if state.released.load(Ordering::SeqCst) {
                    break;
                }
                released.await;
            }
            state.settled.fetch_add(1, Ordering::SeqCst);
            state.settlement.notify_waiters();
        }
    }
    let frontier = request["frontier"]["name"].as_str().unwrap();
    Json(json!({
        "schema_version": 1,
        "request_id": "request-1",
        "provider": "codex_chatgpt",
        "model": "gpt-5.6-luna",
        "reasoning_effort": "medium",
        "auth_mode": "chatgpt",
        "codex_cli_version": "codex-cli 0.146.0-alpha.3.1",
        "tool_call": {
            "id": "interpret",
            "name": frontier,
            "arguments": json!({
            "expected_revision": 0,
            "request_mode": "discussion",
            "automation_kind": "none",
            "requested_outcome": "discussion",
            "hub_channel": null,
            "language": "en",
            "close_policy": "disabled",
            "other_unmapped_required_capabilities": [],
            "response": "We can decide the controls before generating a preview."
            }).to_string()
        },
        "usage": {
            "input_tokens": 100,
            "cached_input_tokens": 0,
            "output_tokens": 20,
            "reasoning_output_tokens": 10
        },
        "duration_ms": 10
    }))
}

struct IntegratedFacade {
    authentication: Authentication,
    authority: Authority,
    store: Store,
    admission: AuthoringAdmissionV1,
    worker: Option<CodexWorkerClient>,
    readiness_calls: AtomicUsize,
}

impl IntegratedFacade {
    fn with_worker(worker: CodexWorkerClient) -> Self {
        Self::new(Some(worker))
    }

    async fn from_worker_preflight(worker: CodexWorkerClient) -> Self {
        let available = worker.preflight_contract().await.is_ok();
        Self::new(available.then_some(worker))
    }

    fn new(worker: Option<CodexWorkerClient>) -> Self {
        Self {
            authentication: Authentication,
            authority: Authority,
            store: Store::new(),
            admission: AuthoringAdmissionV1::new(AuthoringAdmissionConfigV1::new(64, 1).unwrap()),
            worker,
            readiness_calls: AtomicUsize::new(0),
        }
    }
}

fn internal<T>() -> Result<T, FacadeError> {
    Err(FacadeError::new(FacadeErrorCode::Internal))
}

#[async_trait]
impl ProductControlFacade for IntegratedFacade {
    async fn oauth_start(
        &self,
        _command: OAuthStartCommand,
    ) -> Result<OAuthStartResult, FacadeError> {
        internal()
    }

    async fn oauth_callback(
        &self,
        _command: OAuthCallbackCommand,
    ) -> Result<OAuthCallbackResult, FacadeError> {
        internal()
    }

    async fn current_principal(
        &self,
        credential: &SessionCredential,
    ) -> Result<CurrentPrincipal, FacadeError> {
        if credential.expose_secret() != SESSION {
            return Err(FacadeError::new(FacadeErrorCode::AuthenticationRequired));
        }
        Ok(CurrentPrincipal {
            principal_id: "principal-1".to_string(),
            display_name: "Manager".to_string(),
        })
    }

    async fn authority_check(
        &self,
        _credential: &SessionCredential,
        _installation_id: &str,
    ) -> Result<(), FacadeError> {
        internal()
    }

    async fn revoke_session(
        &self,
        _credential: &SessionCredential,
        _csrf: &CsrfSecret,
    ) -> Result<(), FacadeError> {
        internal()
    }

    async fn promote(
        &self,
        _credential: &SessionCredential,
        _csrf: &CsrfSecret,
        _command: PromoteCommand,
    ) -> Result<PromotionView, FacadeError> {
        internal()
    }

    async fn status(
        &self,
        _credential: &SessionCredential,
        _installation_id: &str,
        _promotion_id: &str,
    ) -> Result<DecisionView, FacadeError> {
        internal()
    }

    async fn approval_preview(
        &self,
        _credential: &SessionCredential,
        _installation_id: &str,
        _promotion_id: &str,
    ) -> Result<ApprovalPreviewView, FacadeError> {
        internal()
    }

    async fn approve(
        &self,
        _credential: &SessionCredential,
        _csrf: &CsrfSecret,
        _command: DecisionCommand,
    ) -> Result<DecisionView, FacadeError> {
        internal()
    }

    async fn reject(
        &self,
        _credential: &SessionCredential,
        _csrf: &CsrfSecret,
        _command: RejectCommand,
    ) -> Result<DecisionView, FacadeError> {
        internal()
    }

    async fn apply(
        &self,
        _credential: &SessionCredential,
        _csrf: &CsrfSecret,
        _command: ApplyCommand,
    ) -> Result<ApplyView, FacadeError> {
        internal()
    }

    async fn deployment(
        &self,
        _credential: &SessionCredential,
        _installation_id: &str,
        _promotion_id: &str,
    ) -> Result<DeploymentView, FacadeError> {
        internal()
    }

    async fn readiness(&self) -> Result<(), FacadeError> {
        self.readiness_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[async_trait]
impl ProductControlAuthoringFacadeV1 for IntegratedFacade {
    async fn authoring_turn(
        &self,
        credential: &SessionCredential,
        csrf: &CsrfSecret,
        command: AuthoringTurnCommandV1,
    ) -> Result<AuthoringTurnViewV1, FacadeError> {
        let worker = self
            .worker
            .as_ref()
            .ok_or_else(|| FacadeError::new(FacadeErrorCode::DependencyUnavailable))?;
        let (_request_id, installation, command) =
            map_authoring_turn_command(command)?.into_parts();
        let session_id = command.session_id().clone();
        let outcome = ConversationApplication::new(
            &self.authentication,
            &self.authority,
            &self.store,
            &self.admission,
            worker,
            AuthoringConversationConfigV1::default(),
        )
        .start_or_advance_turn(
            credential.expose_secret(),
            csrf.expose_secret(),
            &installation,
            command,
        )
        .await
        .map_err(map_authoring_conversation_error)?;
        project_authoring_turn(&session_id, &outcome)
    }

    async fn authoring_session(
        &self,
        _credential: &SessionCredential,
        _installation_id: &str,
        _session_id: &str,
    ) -> Result<AuthoringSessionViewV1, FacadeError> {
        Err(FacadeError::new(FacadeErrorCode::DependencyUnavailable))
    }
}

fn app(
    facade: Arc<IntegratedFacade>,
    worker_timeout: Duration,
    coordination_timeout: Duration,
    max_in_flight: usize,
) -> Router {
    product_control_router_with_authoring_v1(
        facade,
        HttpBoundaryConfig::new(
            ORIGIN,
            1_024,
            8,
            Duration::from_secs(1),
            ["/app".to_string()],
        )
        .unwrap(),
        AuthoringHttpBoundaryConfigV1::new(worker_timeout, coordination_timeout, 9)
            .unwrap()
            .with_max_in_flight(max_in_flight)
            .unwrap(),
    )
}

fn authoring_request(idempotency_key: &str) -> Request<Body> {
    authoring_request_with_message(idempotency_key, "Discuss the design before building")
}

fn authoring_request_with_message(idempotency_key: &str, message: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/installations/installation-1/authoring/sessions/session-1/turns")
        .header("host", HOST)
        .header("origin", ORIGIN)
        .header("content-type", "application/json")
        .header("x-csrf-token", CSRF)
        .header("idempotency-key", idempotency_key)
        .header(
            "cookie",
            format!("__Host-starring_session={SESSION}; __Host-starring_csrf={CSRF}"),
        )
        .body(Body::from(
            serde_json::to_vec(&json!({
                "expected_generation": 0,
                "message": message
            }))
            .unwrap(),
        ))
        .unwrap()
}

fn current_principal_request() -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri("/v1/me")
        .header("host", HOST)
        .header("cookie", format!("__Host-starring_session={SESSION}"))
        .body(Body::empty())
        .unwrap()
}

fn readiness_request() -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri("/health/ready")
        .header("host", HOST)
        .body(Body::empty())
        .unwrap()
}

async fn body_text(response: Response<Body>) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 1_048_576)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

#[tokio::test]
async fn real_admission_saturation_maps_to_retryable_503_without_worker_entry() {
    let worker = WorkerServer::start(WorkerMode::Immediate, true).await;
    let facade = Arc::new(IntegratedFacade::with_worker(worker.client()));
    let held_model = facade.admission.acquire_model_capacity().await.unwrap();
    let response = app(
        Arc::clone(&facade),
        Duration::from_secs(1),
        Duration::from_secs(1),
        4,
    )
    .oneshot(authoring_request(IDEMPOTENCY))
    .await
    .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()["retry-after"], "9");
    assert!(body_text(response)
        .await
        .contains("\"code\":\"authoring_saturated\""));
    assert_eq!(worker.calls(), 0);
    assert_eq!(facade.store.counts(), (1, 0, 0));
    drop(held_model);
}

#[tokio::test]
async fn http_timeout_cancels_the_application_future_before_any_commit() {
    let worker = WorkerServer::start(WorkerMode::Delayed(Duration::from_millis(500)), true).await;
    let facade = Arc::new(IntegratedFacade::with_worker(worker.client()));
    let response = tokio::time::timeout(
        Duration::from_secs(5),
        app(
            Arc::clone(&facade),
            Duration::from_millis(100),
            Duration::from_millis(100),
            4,
        )
        .oneshot(authoring_request(IDEMPOTENCY)),
    )
    .await
    .expect("authoring request did not complete")
    .unwrap();

    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    assert!(body_text(response)
        .await
        .contains("\"code\":\"request_timeout\""));
    worker.wait_started().await;
    assert_eq!(worker.calls(), 1);
    assert_eq!(facade.store.counts(), (2, 1, 0));
    worker.wait_settled().await;
    assert_eq!(worker.settled(), 1);
    assert_eq!(facade.store.counts(), (2, 1, 0));
}

#[tokio::test]
async fn concurrent_identical_posts_wait_then_recheck_and_replay_one_model_result() {
    let worker = WorkerServer::start(WorkerMode::Blocked, true).await;
    let facade = Arc::new(IntegratedFacade::with_worker(worker.client()));
    let router = app(
        Arc::clone(&facade),
        Duration::from_secs(1),
        Duration::from_secs(1),
        2,
    );
    let first = tokio::spawn(router.clone().oneshot(authoring_request(IDEMPOTENCY)));
    worker.wait_started().await;
    assert_eq!(facade.store.counts(), (2, 1, 0));
    let second = tokio::spawn(router.clone().oneshot(authoring_request(IDEMPOTENCY)));
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!second.is_finished());
    assert_eq!(worker.calls(), 1);
    assert_eq!(facade.store.counts(), (2, 1, 0));

    worker.release();
    let first_response = first.await.unwrap().unwrap();
    let second_response = second.await.unwrap().unwrap();
    assert_eq!(first_response.status(), StatusCode::CREATED);
    assert_eq!(second_response.status(), StatusCode::OK);
    assert!(body_text(first_response)
        .await
        .contains("\"disposition\":\"created\""));
    assert!(body_text(second_response)
        .await
        .contains("\"disposition\":\"exact_replay\""));
    assert_eq!(worker.calls(), 1);
    assert_eq!(worker.settled(), 1);
    assert_eq!(facade.store.counts(), (3, 1, 1));
}

#[tokio::test]
async fn concurrent_same_key_different_payload_waits_then_conflicts_without_second_model_call() {
    let worker = WorkerServer::start(WorkerMode::Blocked, true).await;
    let facade = Arc::new(IntegratedFacade::with_worker(worker.client()));
    let router = app(
        Arc::clone(&facade),
        Duration::from_secs(1),
        Duration::from_secs(1),
        2,
    );
    let first = tokio::spawn(router.clone().oneshot(authoring_request_with_message(
        IDEMPOTENCY,
        "Discuss the first design",
    )));
    worker.wait_started().await;
    let second = tokio::spawn(router.clone().oneshot(authoring_request_with_message(
        IDEMPOTENCY,
        "Discuss a different design",
    )));
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(!second.is_finished());
    assert_eq!(worker.calls(), 1);
    assert_eq!(facade.store.counts(), (2, 1, 0));

    worker.release();
    let first_response = first.await.unwrap().unwrap();
    let second_response = second.await.unwrap().unwrap();
    assert_eq!(first_response.status(), StatusCode::CREATED);
    assert_eq!(second_response.status(), StatusCode::CONFLICT);
    assert!(body_text(first_response)
        .await
        .contains("\"disposition\":\"created\""));
    assert!(body_text(second_response)
        .await
        .contains("\"code\":\"idempotency_conflict\""));
    assert_eq!(worker.calls(), 1);
    assert_eq!(worker.settled(), 1);
    assert_eq!(facade.store.counts(), (3, 1, 1));
}

#[tokio::test]
async fn failed_worker_preflight_leaves_core_ready_and_only_authoring_unavailable() {
    let worker = WorkerServer::start(WorkerMode::Immediate, false).await;
    let facade = Arc::new(IntegratedFacade::from_worker_preflight(worker.client()).await);
    assert_eq!(facade.readiness().await, Ok(()));
    let router = app(
        Arc::clone(&facade),
        Duration::from_secs(1),
        Duration::from_secs(1),
        4,
    );

    let ready = router.clone().oneshot(readiness_request()).await.unwrap();
    assert_eq!(ready.status(), StatusCode::OK);
    let core = router
        .clone()
        .oneshot(current_principal_request())
        .await
        .unwrap();
    assert_eq!(core.status(), StatusCode::OK);
    let authoring = router
        .oneshot(authoring_request(IDEMPOTENCY))
        .await
        .unwrap();
    assert_eq!(authoring.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(body_text(authoring)
        .await
        .contains("\"code\":\"dependency_unavailable\""));
    assert_eq!(facade.readiness_calls.load(Ordering::SeqCst), 1);
    assert!(facade.worker.is_none());
    assert_eq!(worker.health_calls(), 1);
    assert_eq!(worker.calls(), 0);
    assert_eq!(facade.store.counts(), (0, 0, 0));
}
