use std::collections::{BTreeMap, VecDeque};
use std::num::NonZeroU64;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime};

use authoring_application::{
    AuthenticatedActorV1, AuthenticatedSessionFingerprintV1, AuthenticationClaimsV1,
    AuthenticationError, AuthenticationPort, AuthoringCommitBoundaryV1, AuthoringCommitOutcomeV1,
    AuthoringConversationConfigError, AuthoringConversationConfigV1, AuthoringConversationError,
    AuthoringExpectedGenerationError, AuthoringExpectedGenerationV1, AuthoringHumanMessageError,
    AuthoringHumanMessageV1, AuthoringMutationDispositionV1, AuthoringSessionCommitPort,
    AuthoringSessionLoadError, AuthoringSessionLoadPort, AuthoringSessionLoadV1,
    AuthoringSessionObservationErrorV1, AuthoringSessionObservationV1, AuthoringSessionReadPort,
    AuthoringStoredGenerationV1, AuthoringStoredRequestIdentityV1, AuthoringTurnAdmissionPort,
    AuthoringTurnCheckV1, AuthoringTurnOutcomeV1, AuthoringTurnReceiptV1,
    AuthorizedAuthoringCommitV1, AuthorizedConversationReadAccessV1, AuthorizedInstallationScopeV1,
    AuthorizedInstallationV1, CapabilityV1, ConversationApplication, FreshGuildAuthorityError,
    FreshGuildAuthorityEvidence, FreshGuildAuthorityPort, InstallationSelectorV1,
    LocalAuthoringRequestKeyV1, MutationAuthenticationPort, ProductIdempotencyKeyV1,
    ReadAuthoringSessionV1, SafeAuthoringProjectionError, SafeAuthoringTurnProjectionV1,
    SafeAuthoringTurnStateV1, StartOrAdvanceAuthoringTurnV1,
};
use authoring_promotion::{
    AuthoringSessionId, AutomationInstallationId, PrincipalId, SessionGeneration, TenantId,
};
use design_harness::{
    LlmClient, LlmError, LlmResponse, Message, PreviewReadyArtifactV1, ResourceBindingMap,
    SessionSnapshot, ToolCall, ToolDefinition,
};
use discord_model::{GuildId, UserId};
use futures::executor::block_on;
use serde_json::json;

#[derive(Clone)]
struct ScriptedClient {
    responses: Arc<Mutex<VecDeque<Result<LlmResponse, LlmError>>>>,
    calls: Arc<Mutex<usize>>,
    delay: Duration,
}

impl ScriptedClient {
    fn new(responses: Vec<Result<LlmResponse, LlmError>>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into())),
            calls: Arc::new(Mutex::new(0)),
            delay: Duration::ZERO,
        }
    }

    fn delayed(responses: Vec<Result<LlmResponse, LlmError>>, delay: Duration) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into())),
            calls: Arc::new(Mutex::new(0)),
            delay,
        }
    }

    fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl LlmClient for ScriptedClient {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
    ) -> Result<LlmResponse, LlmError> {
        *self.calls.lock().unwrap() += 1;
        if !self.delay.is_zero() {
            std::thread::sleep(self.delay);
        }
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .expect("scripted model response")
    }
}

#[derive(Clone)]
struct BlockingClient {
    response: Arc<Mutex<Option<Result<LlmResponse, LlmError>>>>,
    started: Arc<(Mutex<bool>, Condvar)>,
    release: Arc<(Mutex<bool>, Condvar)>,
    calls: Arc<Mutex<usize>>,
}

impl BlockingClient {
    fn new(response: Result<LlmResponse, LlmError>) -> Self {
        Self {
            response: Arc::new(Mutex::new(Some(response))),
            started: Arc::new((Mutex::new(false), Condvar::new())),
            release: Arc::new((Mutex::new(false), Condvar::new())),
            calls: Arc::new(Mutex::new(0)),
        }
    }

    fn wait_until_started(&self) {
        let (started, changed) = &*self.started;
        let mut started = started.lock().unwrap();
        while !*started {
            started = changed.wait(started).unwrap();
        }
    }

    fn release(&self) {
        let (release, changed) = &*self.release;
        *release.lock().unwrap() = true;
        changed.notify_all();
    }

    fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl LlmClient for BlockingClient {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[ToolDefinition],
    ) -> Result<LlmResponse, LlmError> {
        *self.calls.lock().unwrap() += 1;
        let (started, started_changed) = &*self.started;
        *started.lock().unwrap() = true;
        started_changed.notify_all();
        let (release, release_changed) = &*self.release;
        let mut released = release.lock().unwrap();
        while !*released {
            released = release_changed.wait(released).unwrap();
        }
        self.response
            .lock()
            .unwrap()
            .take()
            .expect("configured blocking model response")
    }
}

fn tool_call(id: &str, name: &str, arguments: serde_json::Value) -> LlmResponse {
    LlmResponse::ToolCalls(vec![ToolCall {
        id: id.to_string(),
        name: name.to_string(),
        arguments: arguments.to_string(),
    }])
}

fn private_room(expected_revision: u64, hub: Option<&str>) -> LlmResponse {
    tool_call(
        "interpret",
        "interpret_intent_core",
        json!({
            "expected_revision": expected_revision,
            "request_mode": "build",
            "automation_kind": "managed_private_study_room",
            "requested_outcome": "validated_preview",
            "hub_channel": hub,
            "language": "en",
            "close_policy": "disabled",
            "other_unmapped_required_capabilities": [],
            "response": ""
        }),
    )
}

fn resolve_channel(expected_revision: u64, channel: &str) -> LlmResponse {
    tool_call(
        "resolve",
        "resolve_intent_decision",
        json!({
            "expected_revision": expected_revision,
            "channel": channel
        }),
    )
}

fn private_room_copy_details(expected_revision: u64) -> [LlmResponse; 2] {
    [
        private_room(expected_revision, Some("community_hub")),
        tool_call(
            "details",
            "extract_private_study_room_details",
            json!({
                "copy": {
                    "create_button_label": "Start exact focus"
                }
            }),
        ),
    ]
}

fn creator_only_gap(expected_revision: u64) -> LlmResponse {
    let mut value = json!({
        "expected_revision": expected_revision,
        "request_mode": "build",
        "automation_kind": "managed_private_study_room",
        "requested_outcome": "validated_preview",
        "hub_channel": null,
        "language": "en",
        "close_policy": "creator_only",
        "other_unmapped_required_capabilities": [],
        "response": "I built it anyway."
    });
    value["custom_detail_facets"] = json!(["custom_controls"]);
    tool_call("interpret", "interpret_intent_core", value)
}

fn discussion(expected_revision: u64) -> LlmResponse {
    tool_call(
        "interpret",
        "interpret_intent_core",
        json!({
            "expected_revision": expected_revision,
            "request_mode": "discussion",
            "automation_kind": "none",
            "requested_outcome": "discussion",
            "hub_channel": null,
            "language": "en",
            "close_policy": "disabled",
            "other_unmapped_required_capabilities": [],
            "response": "A private room flow can create scoped roles and channels. We can decide the exact controls before generating a preview."
        }),
    )
}

fn typed_planner_fallback(expected_revision: u64) -> LlmResponse {
    tool_call(
        "interpret",
        "interpret_intent_core",
        json!({
            "expected_revision": expected_revision,
            "request_mode": "build",
            "automation_kind": "custom_automation",
            "requested_outcome": "working_draft",
            "hub_channel": "community_hub",
            "language": "en",
            "close_policy": "disabled",
            "other_unmapped_required_capabilities": [],
            "response": ""
        }),
    )
}

fn rejected_boundary(expected_revision: u64) -> LlmResponse {
    let mut value = json!({
        "expected_revision": expected_revision,
        "request_mode": "build",
        "automation_kind": "managed_private_study_room",
        "requested_outcome": "validated_preview",
        "hub_channel": null,
        "language": "en",
        "close_policy": "disabled",
        "other_unmapped_required_capabilities": [],
        "response": "The secret was disclosed and the live server was changed."
    });
    value["runtime_requirements"] = json!(["restart_persistent"]);
    value["live_discord_mutation"] = json!("mutate_live_now");
    value["secret_disclosure"] = json!("disclose_secret_value");
    value["custom_detail_facets"] = json!(["custom_copy"]);
    tool_call("interpret", "interpret_intent_core", value)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Evidence {
    revision: NonZeroU64,
    authority_digest: String,
}

impl Evidence {
    fn stable() -> Self {
        Self {
            revision: NonZeroU64::new(7).unwrap(),
            authority_digest: "a".repeat(64),
        }
    }

    fn drifted() -> Self {
        Self {
            revision: NonZeroU64::new(8).unwrap(),
            authority_digest: "b".repeat(64),
        }
    }
}

impl FreshGuildAuthorityEvidence for Evidence {
    fn tenant_id(&self) -> &TenantId {
        static TENANT: std::sync::OnceLock<TenantId> = std::sync::OnceLock::new();
        TENANT.get_or_init(|| TenantId::parse("tenant-1").unwrap())
    }

    fn installation_id(&self) -> &AutomationInstallationId {
        static INSTALLATION: std::sync::OnceLock<AutomationInstallationId> =
            std::sync::OnceLock::new();
        INSTALLATION.get_or_init(|| AutomationInstallationId::parse("installation-1").unwrap())
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
        self.revision
    }

    fn installation_authority_digest(&self) -> &str {
        &self.authority_digest
    }

    fn observation_digest(&self) -> &str {
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
    }

    fn observed_at(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(100)
    }

    fn expires_at(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(105)
    }
}

struct Authentication {
    calls: Mutex<usize>,
}

impl Authentication {
    fn new() -> Self {
        Self {
            calls: Mutex::new(0),
        }
    }

    fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl AuthenticationPort for Authentication {
    type Credential = str;

    async fn authenticate(
        &self,
        credential: &Self::Credential,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        if credential != "credential" {
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
        *self.calls.lock().unwrap() += 1;
        if csrf != "csrf" {
            return Err(AuthenticationError::InvalidCsrf);
        }
        self.authenticate(credential).await
    }
}

struct Authority {
    evidence: Mutex<VecDeque<Evidence>>,
    fallback: Evidence,
    calls: Mutex<usize>,
}

impl Authority {
    fn stable() -> Self {
        Self::sequence([Evidence::stable(), Evidence::stable()])
    }

    fn sequence(values: impl IntoIterator<Item = Evidence>) -> Self {
        Self {
            evidence: Mutex::new(values.into_iter().collect()),
            fallback: Evidence::stable(),
            calls: Mutex::new(0),
        }
    }

    fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl FreshGuildAuthorityPort for Authority {
    type Evidence = Evidence;

    async fn authorize_installation(
        &self,
        actor: &AuthenticatedActorV1,
        installation: &InstallationSelectorV1,
        capability: CapabilityV1,
    ) -> Result<AuthorizedInstallationV1<Self::Evidence>, FreshGuildAuthorityError> {
        assert_eq!(actor.principal_id().as_str(), "principal-1");
        assert_eq!(installation.installation_id().as_str(), "installation-1");
        assert_eq!(capability, CapabilityV1::Author);
        *self.calls.lock().unwrap() += 1;
        let evidence = self
            .evidence
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| self.fallback.clone());
        let scope = AuthorizedInstallationScopeV1::from_fresh_authority(
            TenantId::parse("tenant-1").unwrap(),
            AutomationInstallationId::parse("installation-1").unwrap(),
            GuildId(10),
            UserId(20),
        );
        Ok(AuthorizedInstallationV1::from_fresh_authority(
            scope, evidence,
        ))
    }
}

struct ReadAuthentication {
    outcome: Result<AuthenticationClaimsV1, AuthenticationError>,
    authentication_calls: Mutex<Vec<String>>,
    mutation_calls: Mutex<usize>,
}

impl ReadAuthentication {
    fn valid(principal_id: &str) -> Self {
        Self {
            outcome: Ok(AuthenticationClaimsV1::from_authentication(
                PrincipalId::parse(principal_id).unwrap(),
                AuthenticatedSessionFingerprintV1::from_sha256_digest([9; 32]),
            )),
            authentication_calls: Mutex::new(Vec::new()),
            mutation_calls: Mutex::new(0),
        }
    }

    fn failed(error: AuthenticationError) -> Self {
        Self {
            outcome: Err(error),
            authentication_calls: Mutex::new(Vec::new()),
            mutation_calls: Mutex::new(0),
        }
    }

    fn counts(&self) -> (usize, usize) {
        (
            self.authentication_calls.lock().unwrap().len(),
            *self.mutation_calls.lock().unwrap(),
        )
    }

    fn credentials(&self) -> Vec<String> {
        self.authentication_calls.lock().unwrap().clone()
    }
}

impl AuthenticationPort for ReadAuthentication {
    type Credential = str;

    async fn authenticate(
        &self,
        credential: &Self::Credential,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        self.authentication_calls
            .lock()
            .unwrap()
            .push(credential.to_string());
        self.outcome.clone()
    }
}

impl MutationAuthenticationPort for ReadAuthentication {
    type CsrfProof = str;

    async fn authenticate_mutation(
        &self,
        _credential: &Self::Credential,
        _csrf: &Self::CsrfProof,
    ) -> Result<AuthenticationClaimsV1, AuthenticationError> {
        *self.mutation_calls.lock().unwrap() += 1;
        Err(AuthenticationError::InvalidCsrf)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReadEvidence {
    tenant_id: TenantId,
    installation_id: AutomationInstallationId,
    guild_id: GuildId,
    acting_user_id: UserId,
    capability: CapabilityV1,
    revision: NonZeroU64,
    authority_digest: String,
}

impl ReadEvidence {
    fn valid() -> Self {
        Self {
            tenant_id: TenantId::parse("tenant-1").unwrap(),
            installation_id: AutomationInstallationId::parse("installation-1").unwrap(),
            guild_id: GuildId(10),
            acting_user_id: UserId(20),
            capability: CapabilityV1::Read,
            revision: NonZeroU64::new(11).unwrap(),
            authority_digest: "d".repeat(64),
        }
    }
}

impl FreshGuildAuthorityEvidence for ReadEvidence {
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
        self.guild_id
    }

    fn acting_user_id(&self) -> UserId {
        self.acting_user_id
    }

    fn capability(&self) -> CapabilityV1 {
        self.capability
    }

    fn guild_owner(&self) -> bool {
        true
    }

    fn effective_permissions_bits(&self) -> u64 {
        0
    }

    fn installation_authority_revision(&self) -> NonZeroU64 {
        self.revision
    }

    fn installation_authority_digest(&self) -> &str {
        &self.authority_digest
    }

    fn observation_digest(&self) -> &str {
        "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
    }

    fn observed_at(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(200)
    }

    fn expires_at(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(205)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReadAuthorityCall {
    principal_id: String,
    installation_id: String,
    capability: CapabilityV1,
}

struct ReadAuthority {
    outcome: Result<AuthorizedInstallationV1<ReadEvidence>, FreshGuildAuthorityError>,
    calls: Mutex<Vec<ReadAuthorityCall>>,
}

impl ReadAuthority {
    fn valid() -> Self {
        Self::with_scope_and_evidence(
            AuthorizedInstallationScopeV1::from_fresh_authority(
                TenantId::parse("tenant-1").unwrap(),
                AutomationInstallationId::parse("installation-1").unwrap(),
                GuildId(10),
                UserId(20),
            ),
            ReadEvidence::valid(),
        )
    }

    fn with_scope_and_evidence(
        scope: AuthorizedInstallationScopeV1,
        evidence: ReadEvidence,
    ) -> Self {
        Self {
            outcome: Ok(AuthorizedInstallationV1::from_fresh_authority(
                scope, evidence,
            )),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<ReadAuthorityCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl FreshGuildAuthorityPort for ReadAuthority {
    type Evidence = ReadEvidence;

    async fn authorize_installation(
        &self,
        actor: &AuthenticatedActorV1,
        installation: &InstallationSelectorV1,
        capability: CapabilityV1,
    ) -> Result<AuthorizedInstallationV1<Self::Evidence>, FreshGuildAuthorityError> {
        self.calls.lock().unwrap().push(ReadAuthorityCall {
            principal_id: actor.principal_id().as_str().to_string(),
            installation_id: installation.installation_id().as_str().to_string(),
            capability,
        });
        self.outcome.clone()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReadAccessRecord {
    principal_id: String,
    tenant_id: String,
    installation_id: String,
    guild_id: GuildId,
    acting_user_id: UserId,
    evidence: ReadEvidence,
    session_id: String,
}

#[derive(Default)]
struct ReadStoreCounts {
    reads: usize,
    checks: usize,
    loads: usize,
    commits: usize,
}

struct ReadStore {
    outcome:
        Mutex<Option<Result<AuthoringSessionObservationV1, AuthoringSessionObservationErrorV1>>>,
    accesses: Mutex<Vec<ReadAccessRecord>>,
    counts: Mutex<ReadStoreCounts>,
}

impl ReadStore {
    fn successful(session_id: &str, generation: u64) -> Self {
        Self {
            outcome: Mutex::new(Some(Ok(AuthoringSessionObservationV1::from_storage(
                AuthoringSessionId::parse(session_id).unwrap(),
                SessionGeneration::new(generation).unwrap(),
                replay_projection(),
                None,
            )
            .unwrap()))),
            accesses: Mutex::new(Vec::new()),
            counts: Mutex::new(ReadStoreCounts::default()),
        }
    }

    fn failed(error: AuthoringSessionObservationErrorV1) -> Self {
        Self {
            outcome: Mutex::new(Some(Err(error))),
            accesses: Mutex::new(Vec::new()),
            counts: Mutex::new(ReadStoreCounts::default()),
        }
    }

    fn accesses(&self) -> Vec<ReadAccessRecord> {
        self.accesses.lock().unwrap().clone()
    }

    fn counts(&self) -> (usize, usize, usize, usize) {
        let counts = self.counts.lock().unwrap();
        (counts.reads, counts.checks, counts.loads, counts.commits)
    }
}

impl AuthoringSessionReadPort<ReadEvidence> for ReadStore {
    async fn read_authorized_session(
        &self,
        access: &AuthorizedConversationReadAccessV1<'_, ReadEvidence>,
    ) -> Result<AuthoringSessionObservationV1, AuthoringSessionObservationErrorV1> {
        self.counts.lock().unwrap().reads += 1;
        self.accesses.lock().unwrap().push(ReadAccessRecord {
            principal_id: access.actor().principal_id().as_str().to_string(),
            tenant_id: access.scope().tenant_id().as_str().to_string(),
            installation_id: access.scope().installation_id().as_str().to_string(),
            guild_id: access.scope().guild_id(),
            acting_user_id: access.scope().acting_user_id(),
            evidence: access.evidence().clone(),
            session_id: access.query().session_id().as_str().to_string(),
        });
        self.outcome
            .lock()
            .unwrap()
            .take()
            .expect("configured authoring observation")
    }
}

impl AuthoringSessionLoadPort<ReadEvidence> for ReadStore {
    async fn check_replay_or_head(
        &self,
        _access: &authoring_application::AuthorizedConversationAccessV1<'_, ReadEvidence>,
    ) -> Result<AuthoringTurnCheckV1, AuthoringSessionLoadError> {
        self.counts.lock().unwrap().checks += 1;
        Err(AuthoringSessionLoadError::Unavailable)
    }

    async fn load_exact_generation(
        &self,
        _access: &authoring_application::AuthorizedConversationAccessV1<'_, ReadEvidence>,
    ) -> Result<AuthoringSessionLoadV1, AuthoringSessionLoadError> {
        self.counts.lock().unwrap().loads += 1;
        Err(AuthoringSessionLoadError::Unavailable)
    }
}

impl AuthoringSessionCommitPort<ReadEvidence> for ReadStore {
    async fn commit_authorized_generation(
        &self,
        _request: AuthorizedAuthoringCommitV1<'_, ReadEvidence>,
    ) -> Result<AuthoringCommitOutcomeV1, AuthoringSessionLoadError> {
        self.counts.lock().unwrap().commits += 1;
        Err(AuthoringSessionLoadError::Unavailable)
    }
}

#[derive(Clone)]
struct StoredRecord {
    session_id: String,
    idempotency_key: String,
    expected_generation: u64,
    human_message: String,
    generation: SessionGeneration,
    projection: SafeAuthoringTurnProjectionV1,
    preview_ready_artifact: Option<PreviewReadyArtifactV1>,
}

fn stored_request_identity(
    command: &StartOrAdvanceAuthoringTurnV1,
) -> AuthoringStoredRequestIdentityV1 {
    AuthoringStoredRequestIdentityV1::from_verified_storage_match(
        AuthorizedInstallationScopeV1::from_fresh_authority(
            TenantId::parse("tenant-1").unwrap(),
            AutomationInstallationId::parse("installation-1").unwrap(),
            GuildId(10),
            UserId(20),
        ),
        PrincipalId::parse("principal-1").unwrap(),
        command.session_id().clone(),
        command.expected_generation(),
        command.idempotency_key().clone(),
        command.human_message().clone(),
    )
}

#[derive(Default)]
struct StoreState {
    head_generation: Option<SessionGeneration>,
    snapshot: Option<SessionSnapshot>,
    records: Vec<StoredRecord>,
    checks: usize,
    loads: usize,
    commits: usize,
}

struct Store {
    state: Mutex<StoreState>,
    bindings: ResourceBindingMap,
    forced_check: Mutex<Option<AuthoringTurnCheckV1>>,
    forced_commit: Mutex<Option<AuthoringCommitOutcomeV1>>,
    probe_commit_boundary: bool,
    commit_boundary_cancellation_results: Mutex<Vec<bool>>,
}

impl Store {
    fn empty() -> Self {
        Self {
            state: Mutex::new(StoreState::default()),
            bindings: bindings(),
            forced_check: Mutex::new(None),
            forced_commit: Mutex::new(None),
            probe_commit_boundary: false,
            commit_boundary_cancellation_results: Mutex::new(Vec::new()),
        }
    }

    fn with_forced_check(check: AuthoringTurnCheckV1) -> Self {
        Self {
            state: Mutex::new(StoreState::default()),
            bindings: bindings(),
            forced_check: Mutex::new(Some(check)),
            forced_commit: Mutex::new(None),
            probe_commit_boundary: false,
            commit_boundary_cancellation_results: Mutex::new(Vec::new()),
        }
    }

    fn with_forced_commit(commit: AuthoringCommitOutcomeV1) -> Self {
        Self {
            state: Mutex::new(StoreState::default()),
            bindings: bindings(),
            forced_check: Mutex::new(None),
            forced_commit: Mutex::new(Some(commit)),
            probe_commit_boundary: false,
            commit_boundary_cancellation_results: Mutex::new(Vec::new()),
        }
    }

    fn with_commit_boundary_probe() -> Self {
        Self {
            state: Mutex::new(StoreState::default()),
            bindings: bindings(),
            forced_check: Mutex::new(None),
            forced_commit: Mutex::new(None),
            probe_commit_boundary: true,
            commit_boundary_cancellation_results: Mutex::new(Vec::new()),
        }
    }

    fn counts(&self) -> (usize, usize, usize) {
        let state = self.state.lock().unwrap();
        (state.checks, state.loads, state.commits)
    }

    fn head_snapshot(&self) -> Option<SessionSnapshot> {
        self.state.lock().unwrap().snapshot.clone()
    }

    fn last_record(&self) -> Option<StoredRecord> {
        self.state.lock().unwrap().records.last().cloned()
    }

    fn commit_boundary_cancellation_results(&self) -> Vec<bool> {
        self.commit_boundary_cancellation_results
            .lock()
            .unwrap()
            .clone()
    }
}

impl AuthoringSessionLoadPort<Evidence> for Store {
    async fn check_replay_or_head(
        &self,
        access: &authoring_application::AuthorizedConversationAccessV1<'_, Evidence>,
    ) -> Result<AuthoringTurnCheckV1, AuthoringSessionLoadError> {
        {
            let mut state = self.state.lock().unwrap();
            state.checks += 1;
        }
        if let Some(check) = self.forced_check.lock().unwrap().clone() {
            return Ok(check);
        }
        let command = access.command();
        let state = self.state.lock().unwrap();
        if let Some(record) = state.records.iter().find(|record| {
            record.session_id == command.session_id().as_str()
                && record.idempotency_key == command.idempotency_key().as_str()
        }) {
            if record.expected_generation == command.expected_generation().get()
                && record.human_message == command.human_message().as_str()
            {
                return Ok(AuthoringTurnCheckV1::ExactReplay(
                    AuthoringStoredGenerationV1::from_storage(
                        stored_request_identity(command),
                        record.generation,
                        record.projection.clone(),
                        record.preview_ready_artifact.as_ref(),
                    )
                    .unwrap(),
                ));
            }
            return Ok(AuthoringTurnCheckV1::IdempotencyConflict);
        }
        let current = state
            .head_generation
            .map(SessionGeneration::get)
            .unwrap_or(0);
        if current == command.expected_generation().get() {
            Ok(AuthoringTurnCheckV1::Proceed)
        } else {
            Ok(AuthoringTurnCheckV1::GenerationConflict {
                current_generation: state.head_generation,
            })
        }
    }

    async fn load_exact_generation(
        &self,
        _access: &authoring_application::AuthorizedConversationAccessV1<'_, Evidence>,
    ) -> Result<AuthoringSessionLoadV1, AuthoringSessionLoadError> {
        let mut state = self.state.lock().unwrap();
        state.loads += 1;
        AuthoringSessionLoadV1::from_storage(
            state.head_generation,
            state.snapshot.clone(),
            self.bindings.clone(),
        )
    }
}

impl AuthoringSessionCommitPort<Evidence> for Store {
    async fn commit_authorized_generation(
        &self,
        request: AuthorizedAuthoringCommitV1<'_, Evidence>,
    ) -> Result<AuthoringCommitOutcomeV1, AuthoringSessionLoadError> {
        let command = request.access().command();
        if self.probe_commit_boundary {
            self.commit_boundary_cancellation_results
                .lock()
                .unwrap()
                .push(command.commit_boundary().cancel_before_commit());
        }
        let mut state = self.state.lock().unwrap();
        state.commits += 1;
        if let Some(commit) = self.forced_commit.lock().unwrap().clone() {
            return Ok(commit);
        }
        let current = state
            .head_generation
            .map(SessionGeneration::get)
            .unwrap_or(0);
        if current != command.expected_generation().get() {
            return Ok(AuthoringCommitOutcomeV1::GenerationConflict {
                current_generation: state.head_generation,
            });
        }
        let generation = SessionGeneration::new(command.expected_generation().get() + 1).unwrap();
        let projection = request.projection().clone();
        let preview_ready_artifact = request.preview_ready_artifact().cloned();
        state.head_generation = Some(generation);
        state.snapshot = Some(request.snapshot().clone());
        state.records.push(StoredRecord {
            session_id: command.session_id().as_str().to_string(),
            idempotency_key: command.idempotency_key().as_str().to_string(),
            expected_generation: command.expected_generation().get(),
            human_message: command.human_message().as_str().to_string(),
            generation,
            projection: projection.clone(),
            preview_ready_artifact: preview_ready_artifact.clone(),
        });
        Ok(AuthoringCommitOutcomeV1::Created(
            AuthoringStoredGenerationV1::from_storage(
                stored_request_identity(command),
                generation,
                projection,
                preview_ready_artifact.as_ref(),
            )
            .unwrap(),
        ))
    }
}

struct GateState {
    held: Mutex<bool>,
    changed: Condvar,
}

impl GateState {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            held: Mutex::new(false),
            changed: Condvar::new(),
        })
    }

    fn acquire(self: &Arc<Self>) -> GatePermit {
        let mut held = self.held.lock().unwrap();
        while *held {
            held = self.changed.wait(held).unwrap();
        }
        *held = true;
        GatePermit {
            state: self.clone(),
        }
    }
}

struct GatePermit {
    state: Arc<GateState>,
}

impl Drop for GatePermit {
    fn drop(&mut self) {
        *self.state.held.lock().unwrap() = false;
        self.state.changed.notify_one();
    }
}

struct Admission {
    keyed: Mutex<BTreeMap<LocalAuthoringRequestKeyV1, Arc<GateState>>>,
    model: Arc<GateState>,
    keyed_calls: Mutex<usize>,
    model_calls: Mutex<usize>,
    keys: Mutex<Vec<LocalAuthoringRequestKeyV1>>,
}

impl Admission {
    fn new() -> Self {
        Self {
            keyed: Mutex::new(BTreeMap::new()),
            model: GateState::new(),
            keyed_calls: Mutex::new(0),
            model_calls: Mutex::new(0),
            keys: Mutex::new(Vec::new()),
        }
    }

    fn counts(&self) -> (usize, usize) {
        (
            *self.keyed_calls.lock().unwrap(),
            *self.model_calls.lock().unwrap(),
        )
    }

    fn keys(&self) -> Vec<LocalAuthoringRequestKeyV1> {
        self.keys.lock().unwrap().clone()
    }
}

impl AuthoringTurnAdmissionPort for Admission {
    type KeyedPermit = GatePermit;
    type ModelPermit = GatePermit;

    async fn acquire_keyed(
        &self,
        key: &LocalAuthoringRequestKeyV1,
    ) -> Result<Self::KeyedPermit, authoring_application::AuthoringAdmissionError> {
        *self.keyed_calls.lock().unwrap() += 1;
        self.keys.lock().unwrap().push(key.clone());
        let gate = self
            .keyed
            .lock()
            .unwrap()
            .entry(key.clone())
            .or_insert_with(GateState::new)
            .clone();
        Ok(gate.acquire())
    }

    async fn acquire_model_capacity(
        &self,
    ) -> Result<Self::ModelPermit, authoring_application::AuthoringAdmissionError> {
        *self.model_calls.lock().unwrap() += 1;
        Ok(self.model.acquire())
    }
}

struct CapacityState {
    active: usize,
    maximum_active: usize,
}

struct CapacityGate {
    limit: usize,
    state: Mutex<CapacityState>,
    changed: Condvar,
}

impl CapacityGate {
    fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            limit,
            state: Mutex::new(CapacityState {
                active: 0,
                maximum_active: 0,
            }),
            changed: Condvar::new(),
        })
    }

    fn acquire(self: &Arc<Self>) -> CapacityPermit {
        let mut state = self.state.lock().unwrap();
        while state.active == self.limit {
            state = self.changed.wait(state).unwrap();
        }
        state.active += 1;
        state.maximum_active = state.maximum_active.max(state.active);
        CapacityPermit { gate: self.clone() }
    }

    fn maximum_active(&self) -> usize {
        self.state.lock().unwrap().maximum_active
    }
}

struct CapacityPermit {
    gate: Arc<CapacityGate>,
}

impl Drop for CapacityPermit {
    fn drop(&mut self) {
        let mut state = self.gate.state.lock().unwrap();
        state.active -= 1;
        self.gate.changed.notify_one();
    }
}

struct ParallelAdmission {
    keyed: Mutex<BTreeMap<LocalAuthoringRequestKeyV1, Arc<GateState>>>,
    model: Arc<CapacityGate>,
}

impl ParallelAdmission {
    fn new(model_capacity: usize) -> Self {
        Self {
            keyed: Mutex::new(BTreeMap::new()),
            model: CapacityGate::new(model_capacity),
        }
    }

    fn maximum_active_models(&self) -> usize {
        self.model.maximum_active()
    }
}

impl AuthoringTurnAdmissionPort for ParallelAdmission {
    type KeyedPermit = GatePermit;
    type ModelPermit = CapacityPermit;

    async fn acquire_keyed(
        &self,
        key: &LocalAuthoringRequestKeyV1,
    ) -> Result<Self::KeyedPermit, authoring_application::AuthoringAdmissionError> {
        let gate = self
            .keyed
            .lock()
            .unwrap()
            .entry(key.clone())
            .or_insert_with(GateState::new)
            .clone();
        Ok(gate.acquire())
    }

    async fn acquire_model_capacity(
        &self,
    ) -> Result<Self::ModelPermit, authoring_application::AuthoringAdmissionError> {
        Ok(self.model.acquire())
    }
}

fn bindings() -> ResourceBindingMap {
    let mut bindings = ResourceBindingMap::default();
    bindings.channel_bindings.insert(
        serde_json::from_value(json!("community_hub")).unwrap(),
        "700".parse().unwrap(),
    );
    bindings
}

fn installation() -> InstallationSelectorV1 {
    InstallationSelectorV1::new(AutomationInstallationId::parse("installation-1").unwrap())
}

fn command(
    expected_generation: u64,
    idempotency_key: &str,
    message: &str,
) -> StartOrAdvanceAuthoringTurnV1 {
    StartOrAdvanceAuthoringTurnV1::new(
        AuthoringSessionId::parse("session-1").unwrap(),
        AuthoringExpectedGenerationV1::new(expected_generation).unwrap(),
        ProductIdempotencyKeyV1::parse(idempotency_key).unwrap(),
        AuthoringHumanMessageV1::parse(message).unwrap(),
    )
}

fn command_with_commit_boundary(
    expected_generation: u64,
    idempotency_key: &str,
    message: &str,
    commit_boundary: AuthoringCommitBoundaryV1,
) -> StartOrAdvanceAuthoringTurnV1 {
    StartOrAdvanceAuthoringTurnV1::new_with_commit_boundary(
        AuthoringSessionId::parse("session-1").unwrap(),
        AuthoringExpectedGenerationV1::new(expected_generation).unwrap(),
        ProductIdempotencyKeyV1::parse(idempotency_key).unwrap(),
        AuthoringHumanMessageV1::parse(message).unwrap(),
        commit_boundary,
    )
}

fn committed(outcome: AuthoringTurnOutcomeV1) -> AuthoringTurnReceiptV1 {
    outcome.into_committed().unwrap()
}

fn replay_projection() -> SafeAuthoringTurnProjectionV1 {
    SafeAuthoringTurnProjectionV1::from_canonical_json(
        br#"{"schema_version":1,"state":"discussion","assistant_message":"Previously completed","capabilities":[],"draft":{"panels":0,"modals":0,"rules":0,"actions":0,"unresolved_references":[]},"preview":null}"#,
    )
    .unwrap()
}

fn read_query(session_id: &str) -> ReadAuthoringSessionV1 {
    ReadAuthoringSessionV1::new(AuthoringSessionId::parse(session_id).unwrap())
}

#[test]
fn read_session_authenticates_and_forwards_exact_fresh_read_scope() {
    block_on(async {
        let authentication = ReadAuthentication::valid("principal-1");
        let authority = ReadAuthority::valid();
        let store = ReadStore::successful("session-1", 4);
        let admission = Admission::new();
        let client = ScriptedClient::new(Vec::new());
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );

        let observation = application
            .read_session("read-credential", &installation(), read_query("session-1"))
            .await
            .unwrap();

        assert_eq!(observation.session_id().as_str(), "session-1");
        assert_eq!(observation.generation().get(), 4);
        assert_eq!(observation.projection(), &replay_projection());
        assert_eq!(authentication.counts(), (1, 0));
        assert_eq!(
            authentication.credentials(),
            vec!["read-credential".to_string()]
        );
        assert_eq!(
            authority.calls(),
            vec![ReadAuthorityCall {
                principal_id: "principal-1".to_string(),
                installation_id: "installation-1".to_string(),
                capability: CapabilityV1::Read,
            }]
        );
        assert_eq!(
            store.accesses(),
            vec![ReadAccessRecord {
                principal_id: "principal-1".to_string(),
                tenant_id: "tenant-1".to_string(),
                installation_id: "installation-1".to_string(),
                guild_id: GuildId(10),
                acting_user_id: UserId(20),
                evidence: ReadEvidence::valid(),
                session_id: "session-1".to_string(),
            }]
        );
        assert_eq!(store.counts(), (1, 0, 0, 0));
        assert_eq!(admission.counts(), (0, 0));
        assert_eq!(client.calls(), 0);
    });
}

#[test]
fn read_session_maps_non_owner_and_cross_scope_reads_to_not_found() {
    block_on(async {
        for (principal_id, session_id) in [
            ("principal-without-session", "session-1"),
            ("principal-1", "session-from-another-scope"),
        ] {
            let authentication = ReadAuthentication::valid(principal_id);
            let authority = ReadAuthority::valid();
            let store = ReadStore::failed(AuthoringSessionObservationErrorV1::NotFound);
            let admission = Admission::new();
            let client = ScriptedClient::new(Vec::new());
            let application = ConversationApplication::new(
                &authentication,
                &authority,
                &store,
                &admission,
                &client,
                AuthoringConversationConfigV1::default(),
            );

            let error = application
                .read_session("credential", &installation(), read_query(session_id))
                .await
                .unwrap_err();

            assert_eq!(
                error,
                AuthoringConversationError::Observation(
                    AuthoringSessionObservationErrorV1::NotFound
                )
            );
            assert_eq!(authentication.counts(), (1, 0));
            assert_eq!(authority.calls().len(), 1);
            assert_eq!(store.counts(), (1, 0, 0, 0));
            assert_eq!(admission.counts(), (0, 0));
            assert_eq!(client.calls(), 0);
        }
    });
}

#[test]
fn read_session_rejects_expired_and_revoked_authentication_before_authority() {
    block_on(async {
        for authentication_error in [AuthenticationError::Expired, AuthenticationError::Revoked] {
            let authentication = ReadAuthentication::failed(authentication_error.clone());
            let authority = ReadAuthority::valid();
            let store = ReadStore::failed(AuthoringSessionObservationErrorV1::NotFound);
            let admission = Admission::new();
            let client = ScriptedClient::new(Vec::new());
            let application = ConversationApplication::new(
                &authentication,
                &authority,
                &store,
                &admission,
                &client,
                AuthoringConversationConfigV1::default(),
            );

            let error = application
                .read_session("credential", &installation(), read_query("session-1"))
                .await
                .unwrap_err();

            assert_eq!(
                error,
                AuthoringConversationError::Authentication(authentication_error)
            );
            assert_eq!(authentication.counts(), (1, 0));
            assert!(authority.calls().is_empty());
            assert_eq!(store.counts(), (0, 0, 0, 0));
            assert_eq!(admission.counts(), (0, 0));
            assert_eq!(client.calls(), 0);
        }
    });
}

#[test]
fn read_session_rejects_scope_and_evidence_authority_mismatch() {
    block_on(async {
        let mismatched_scope = AuthorizedInstallationScopeV1::from_fresh_authority(
            TenantId::parse("tenant-1").unwrap(),
            AutomationInstallationId::parse("installation-2").unwrap(),
            GuildId(10),
            UserId(20),
        );
        let mut mismatched_scope_evidence = ReadEvidence::valid();
        mismatched_scope_evidence.installation_id =
            AutomationInstallationId::parse("installation-2").unwrap();
        let authentication = ReadAuthentication::valid("principal-1");
        let authority =
            ReadAuthority::with_scope_and_evidence(mismatched_scope, mismatched_scope_evidence);
        let store = ReadStore::failed(AuthoringSessionObservationErrorV1::NotFound);
        let admission = Admission::new();
        let client = ScriptedClient::new(Vec::new());
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );

        let error = application
            .read_session("credential", &installation(), read_query("session-1"))
            .await
            .unwrap_err();

        assert_eq!(
            error,
            AuthoringConversationError::Authority(FreshGuildAuthorityError::ScopeMismatch)
        );
        assert_eq!(store.counts(), (0, 0, 0, 0));
        assert_eq!(admission.counts(), (0, 0));
        assert_eq!(client.calls(), 0);

        let authentication = ReadAuthentication::valid("principal-1");
        let mut mismatched_evidence = ReadEvidence::valid();
        mismatched_evidence.capability = CapabilityV1::Author;
        let authority = ReadAuthority::with_scope_and_evidence(
            AuthorizedInstallationScopeV1::from_fresh_authority(
                TenantId::parse("tenant-1").unwrap(),
                AutomationInstallationId::parse("installation-1").unwrap(),
                GuildId(10),
                UserId(20),
            ),
            mismatched_evidence,
        );
        let store = ReadStore::failed(AuthoringSessionObservationErrorV1::NotFound);
        let admission = Admission::new();
        let client = ScriptedClient::new(Vec::new());
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );

        let error = application
            .read_session("credential", &installation(), read_query("session-1"))
            .await
            .unwrap_err();

        assert_eq!(error, AuthoringConversationError::AuthorityDrift);
        assert_eq!(store.counts(), (0, 0, 0, 0));
        assert_eq!(admission.counts(), (0, 0));
        assert_eq!(client.calls(), 0);
    });
}

#[test]
fn read_session_preserves_bounded_observation_failures_without_side_effects() {
    block_on(async {
        for observation_error in [
            AuthoringSessionObservationErrorV1::Timeout,
            AuthoringSessionObservationErrorV1::Retryable,
            AuthoringSessionObservationErrorV1::Unavailable,
            AuthoringSessionObservationErrorV1::InvalidState,
        ] {
            let authentication = ReadAuthentication::valid("principal-1");
            let authority = ReadAuthority::valid();
            let store = ReadStore::failed(observation_error);
            let admission = Admission::new();
            let client = ScriptedClient::new(Vec::new());
            let application = ConversationApplication::new(
                &authentication,
                &authority,
                &store,
                &admission,
                &client,
                AuthoringConversationConfigV1::default(),
            );

            let error = application
                .read_session("credential", &installation(), read_query("session-1"))
                .await
                .unwrap_err();

            assert_eq!(
                error,
                AuthoringConversationError::Observation(observation_error)
            );
            assert_eq!(authentication.counts(), (1, 0));
            assert_eq!(authority.calls().len(), 1);
            assert_eq!(store.counts(), (1, 0, 0, 0));
            assert_eq!(admission.counts(), (0, 0));
            assert_eq!(client.calls(), 0);
        }
    });
}

#[test]
fn turn_inputs_are_bounded_redacted_and_multiline_safe() {
    let message = AuthoringHumanMessageV1::parse(" first\r\nsecond\rthird ").unwrap();
    assert_eq!(message.as_str(), "first\nsecond\nthird");
    assert_eq!(
        format!("{message:?}"),
        "AuthoringHumanMessageV1(<redacted>)"
    );
    assert_eq!(
        AuthoringHumanMessageV1::parse("first\tsecond").unwrap_err(),
        AuthoringHumanMessageError::ControlCharacter
    );
    assert_eq!(
        AuthoringHumanMessageV1::parse("left\u{202e}right").unwrap_err(),
        AuthoringHumanMessageError::ControlCharacter
    );
    assert_eq!(
        AuthoringHumanMessageV1::parse(&"a".repeat(2_001)).unwrap_err(),
        AuthoringHumanMessageError::TooLong
    );
    assert_eq!(
        AuthoringExpectedGenerationV1::new(9_007_199_254_740_991).unwrap_err(),
        AuthoringExpectedGenerationError::TooLarge
    );
    assert!(AuthoringExpectedGenerationV1::new(9_007_199_254_740_990).is_ok());
    assert_eq!(
        AuthoringConversationConfigV1::new(7_999).unwrap_err(),
        AuthoringConversationConfigError::InvalidContextBudget
    );
    assert_eq!(
        AuthoringConversationConfigV1::new(64_001).unwrap_err(),
        AuthoringConversationConfigError::InvalidContextBudget
    );
    assert!(AuthoringConversationConfigV1::new(8_000).is_ok());
    assert!(AuthoringConversationConfigV1::new(64_000).is_ok());
    let command = command(0, "raw-command-key", "raw command message");
    let debug = format!("{command:?}");
    assert_eq!(debug, "StartOrAdvanceAuthoringTurnV1(<redacted>)");
    for forbidden in ["session-1", "raw-command-key", "raw command message"] {
        assert!(!debug.contains(forbidden));
    }
}

#[test]
fn safe_projection_accepts_only_its_canonical_bounded_shape() {
    let projection = SafeAuthoringTurnProjectionV1::from_canonical_json(
        br#"{"schema_version":1,"state":"discussion","assistant_message":"First\nSecond","capabilities":[],"draft":{"panels":0,"modals":0,"rules":0,"actions":0,"unresolved_references":[]},"preview":null}"#,
    )
    .unwrap();
    assert_eq!(projection.assistant_message(), "First\nSecond");
    assert_eq!(
        projection.to_canonical_json().unwrap(),
        br#"{"schema_version":1,"state":"discussion","assistant_message":"First\nSecond","capabilities":[],"draft":{"panels":0,"modals":0,"rules":0,"actions":0,"unresolved_references":[]},"preview":null}"#
    );
    assert_eq!(
        SafeAuthoringTurnProjectionV1::from_canonical_json(
            br#"{ "schema_version":1,"state":"discussion","assistant_message":"First","capabilities":[],"draft":{"panels":0,"modals":0,"rules":0,"actions":0,"unresolved_references":[]},"preview":null}"#
        )
        .unwrap_err(),
        SafeAuthoringProjectionError::NonCanonical
    );
    assert_eq!(
        SafeAuthoringTurnProjectionV1::from_canonical_json(
            br#"{"schema_version":1,"state":"discussion","assistant_message":"First","capabilities":[],"draft":{"panels":0,"modals":0,"rules":0,"actions":0,"unresolved_references":[]},"preview":null,"unknown":true}"#
        )
        .unwrap_err(),
        SafeAuthoringProjectionError::Malformed
    );
    assert_eq!(
        SafeAuthoringTurnProjectionV1::from_canonical_json(
            br#"{"schema_version":1,"state":"preview_ready","assistant_message":"First","capabilities":[],"draft":{"panels":0,"modals":0,"rules":0,"actions":0,"unresolved_references":[]},"preview":null}"#
        )
        .unwrap_err(),
        SafeAuthoringProjectionError::InvalidStateShape
    );
    assert_eq!(
        SafeAuthoringTurnProjectionV1::from_canonical_json(
            br#"{"schema_version":1,"state":"discussion","assistant_message":"First","capabilities":[],"draft":{"panels":0,"modals":0,"rules":0,"actions":0,"unresolved_references":[" "]},"preview":null}"#
        )
        .unwrap_err(),
        SafeAuthoringProjectionError::InvalidDraft
    );
    assert_eq!(
        SafeAuthoringTurnProjectionV1::from_canonical_json(&vec![b'a'; 256 * 1024 + 1])
            .unwrap_err(),
        SafeAuthoringProjectionError::TooLarge
    );
    let request = command(0, "non-durable-key", "Non-durable state");
    for projection in [
        br#"{"schema_version":1,"state":"unsupported","assistant_message":"Unsupported","capabilities":[],"draft":{"panels":0,"modals":0,"rules":0,"actions":0,"unresolved_references":[]},"preview":null}"#
            .as_slice(),
        br#"{"schema_version":1,"state":"rejected","assistant_message":"Rejected","capabilities":[],"draft":{"panels":0,"modals":0,"rules":0,"actions":0,"unresolved_references":[]},"preview":null}"#
            .as_slice(),
    ] {
        let projection =
            SafeAuthoringTurnProjectionV1::from_canonical_json(projection).unwrap();
        assert_eq!(
            AuthoringStoredGenerationV1::from_storage(
                stored_request_identity(&request),
                SessionGeneration::new(1).unwrap(),
                projection,
                None,
            )
            .unwrap_err(),
            SafeAuthoringProjectionError::NonDurableState
        );
    }
}

fn preview_ready_projection(
    ruleset: &str,
    candidate_ruleset_hash: &str,
) -> SafeAuthoringTurnProjectionV1 {
    let projection = format!(
        r#"{{"schema_version":1,"state":"preview_ready","assistant_message":"Preview ready","capabilities":[],"draft":{{"panels":1,"modals":0,"rules":1,"actions":1,"unresolved_references":[]}},"preview":{{"revision":1,"draft":{{"panels":1,"modals":0,"rules":1,"actions":1,"unresolved_references":[]}},"ruleset":{ruleset},"receipt":{{"identity_revision":1,"intent_revision":1,"candidate_revision":1,"request_evidence_hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","request_evidence_entries":1,"compiler_input_hash":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","semantic_intent_hash":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","compiled_plan_hash":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","candidate_ruleset_hash":"{candidate_ruleset_hash}","candidate_draft_hash":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","compiled_operations":1}}}}}}"#
    );
    SafeAuthoringTurnProjectionV1::from_canonical_json(projection.as_bytes()).unwrap()
}

#[test]
fn preview_integrity_binds_a_typed_structural_ruleset_to_its_receipt() {
    let ruleset = r#"{"modals":[],"panels":[{"buttons":[{"label":"Welcome","route":{"static":{"key":"welcome"}}}],"channel":"welcome_channel","content":"Choose a welcome","key":"welcome_panel"}],"rules":[{"actions":[{"content":"Welcome!","type":"respond_ephemeral"}],"key":"welcome_rule","trigger":{"component":"welcome","type":"button_click"}}],"version":1}"#;
    let valid = preview_ready_projection(
        ruleset,
        "f283047e6367d67067822a399200ffd2ea6c1a6940969e0ab9abd399cb43d537",
    );
    assert_eq!(valid.validate_preview_integrity(), Ok(()));

    let malformed = preview_ready_projection(
        r#"{"malformed":true}"#,
        "f283047e6367d67067822a399200ffd2ea6c1a6940969e0ab9abd399cb43d537",
    );
    assert_eq!(
        malformed.validate_preview_integrity(),
        Err(SafeAuthoringProjectionError::InvalidPreview)
    );

    let mismatched = preview_ready_projection(
        ruleset,
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    assert_eq!(
        mismatched.validate_preview_integrity(),
        Err(SafeAuthoringProjectionError::InvalidPreview)
    );
}

#[test]
fn exact_replay_returns_stored_projection_without_model_or_capacity() {
    block_on(async {
        let request = command(0, "replay-key", "Create private study rooms");
        let stored = AuthoringStoredGenerationV1::from_storage(
            stored_request_identity(&request),
            SessionGeneration::new(1).unwrap(),
            replay_projection(),
            None,
        )
        .unwrap();
        let store = Store::with_forced_check(AuthoringTurnCheckV1::ExactReplay(stored));
        let authentication = Authentication::new();
        let authority = Authority::stable();
        let admission = Admission::new();
        let client = ScriptedClient::new(Vec::new());
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );

        let receipt = committed(
            application
                .start_or_advance_turn("credential", "csrf", &installation(), request)
                .await
                .unwrap(),
        );

        assert_eq!(
            receipt.disposition(),
            AuthoringMutationDispositionV1::ExactReplay
        );
        assert_eq!(receipt.generation().get(), 1);
        assert_eq!(receipt.projection(), &replay_projection());
        assert_eq!(client.calls(), 0);
        assert_eq!(admission.counts(), (1, 0));
        assert_eq!(store.counts(), (1, 0, 0));
        assert_eq!(authentication.calls(), 2);
        assert_eq!(authority.calls(), 2);
    });
}

#[test]
fn exact_replay_with_a_different_request_identity_fails_closed() {
    block_on(async {
        let request = command(0, "bound-replay-key", "Create private study rooms");
        let different_request = command(
            0,
            "bound-replay-key",
            "Create a different private study room",
        );
        let stored = AuthoringStoredGenerationV1::from_storage(
            stored_request_identity(&different_request),
            SessionGeneration::new(1).unwrap(),
            replay_projection(),
            None,
        )
        .unwrap();
        let store = Store::with_forced_check(AuthoringTurnCheckV1::ExactReplay(stored));
        let authentication = Authentication::new();
        let authority = Authority::stable();
        let admission = Admission::new();
        let client = ScriptedClient::new(Vec::new());
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );

        let error = application
            .start_or_advance_turn("credential", "csrf", &installation(), request)
            .await
            .unwrap_err();

        assert_eq!(error, AuthoringConversationError::InvalidCommit);
        assert_eq!(client.calls(), 0);
        assert_eq!(admission.counts(), (1, 0));
        assert_eq!(store.counts(), (1, 0, 0));
    });
}

#[test]
fn stale_generation_fails_before_model_capacity_and_load() {
    block_on(async {
        let store = Store::with_forced_check(AuthoringTurnCheckV1::GenerationConflict {
            current_generation: Some(SessionGeneration::new(2).unwrap()),
        });
        let authentication = Authentication::new();
        let authority = Authority::stable();
        let admission = Admission::new();
        let client = ScriptedClient::new(Vec::new());
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );

        let error = application
            .start_or_advance_turn(
                "credential",
                "csrf",
                &installation(),
                command(1, "stale-key", "Create private study rooms"),
            )
            .await
            .unwrap_err();

        assert_eq!(
            error,
            AuthoringConversationError::GenerationConflict {
                current_generation: Some(SessionGeneration::new(2).unwrap())
            }
        );
        assert_eq!(client.calls(), 0);
        assert_eq!(admission.counts(), (1, 0));
        assert_eq!(store.counts(), (1, 0, 0));
    });
}

#[test]
fn cancellation_before_execution_prevents_authentication_admission_model_and_commit() {
    block_on(async {
        let store = Store::empty();
        let authentication = Authentication::new();
        let authority = Authority::stable();
        let admission = Admission::new();
        let client = ScriptedClient::new(vec![Ok(private_room(0, Some("community_hub")))]);
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );
        let commit_boundary = AuthoringCommitBoundaryV1::new();
        assert!(commit_boundary.cancel_before_commit());
        let request = command_with_commit_boundary(
            0,
            "cancelled-before-start-key",
            "Create private study rooms",
            commit_boundary.clone(),
        );

        let error = application
            .start_or_advance_turn("credential", "csrf", &installation(), request)
            .await
            .unwrap_err();

        assert_eq!(error, AuthoringConversationError::CancelledBeforeCommit);
        assert!(!commit_boundary.commit_phase_started());
        assert_eq!(client.calls(), 0);
        assert_eq!(admission.counts(), (0, 0));
        assert_eq!(store.counts(), (0, 0, 0));
        assert_eq!(authentication.calls(), 0);
        assert_eq!(authority.calls(), 0);
    });
}

#[test]
fn cancellation_during_model_prevents_commit() {
    let store = Store::empty();
    let authentication = Authentication::new();
    let authority = Authority::stable();
    let admission = Admission::new();
    let client = BlockingClient::new(Ok(private_room(0, Some("community_hub"))));
    let application = ConversationApplication::new(
        &authentication,
        &authority,
        &store,
        &admission,
        &client,
        AuthoringConversationConfigV1::default(),
    );
    let commit_boundary = AuthoringCommitBoundaryV1::new();
    let request = command_with_commit_boundary(
        0,
        "cancelled-during-model-key",
        "Create private study rooms",
        commit_boundary.clone(),
    );

    let result = std::thread::scope(|scope| {
        let execution = scope.spawn(|| {
            block_on(application.start_or_advance_turn(
                "credential",
                "csrf",
                &installation(),
                request,
            ))
        });
        client.wait_until_started();
        assert!(commit_boundary.cancel_before_commit());
        client.release();
        execution.join().unwrap()
    });

    assert_eq!(
        result.unwrap_err(),
        AuthoringConversationError::CancelledBeforeCommit
    );
    assert!(!commit_boundary.commit_phase_started());
    assert_eq!(client.calls(), 1);
    assert_eq!(admission.counts(), (1, 1));
    assert_eq!(store.counts(), (2, 1, 0));
}

#[test]
fn preview_ready_turn_commits_generation_one() {
    block_on(async {
        let store = Store::empty();
        let authentication = Authentication::new();
        let authority = Authority::stable();
        let admission = Admission::new();
        let client = ScriptedClient::new(vec![Ok(private_room(0, Some("community_hub")))]);
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );

        let receipt = committed(
            application
                .start_or_advance_turn(
                    "credential",
                    "csrf",
                    &installation(),
                    command(
                        0,
                        "ready-key",
                        "Create private study rooms in community_hub and prepare a validated preview",
                    ),
                )
                .await
                .unwrap(),
        );

        assert_eq!(
            receipt.disposition(),
            AuthoringMutationDispositionV1::Created
        );
        assert_eq!(receipt.generation().get(), 1);
        assert_eq!(
            receipt.projection().state(),
            SafeAuthoringTurnStateV1::PreviewReady
        );
        assert!(receipt.projection().preview().is_some());
        assert_eq!(client.calls(), 1);
        assert_eq!(admission.counts(), (1, 1));
        assert_eq!(store.counts(), (2, 1, 1));
        assert_eq!(authentication.calls(), 4);
        assert_eq!(authority.calls(), 4);
    });
}

#[test]
fn commit_phase_wins_the_cancellation_race_before_store_commit() {
    block_on(async {
        let store = Store::with_commit_boundary_probe();
        let authentication = Authentication::new();
        let authority = Authority::stable();
        let admission = Admission::new();
        let client = ScriptedClient::new(vec![Ok(private_room(0, Some("community_hub")))]);
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );
        let commit_boundary = AuthoringCommitBoundaryV1::new();
        let request = command_with_commit_boundary(
            0,
            "commit-wins-key",
            "Create private study rooms",
            commit_boundary.clone(),
        );

        let receipt = committed(
            application
                .start_or_advance_turn("credential", "csrf", &installation(), request)
                .await
                .unwrap(),
        );

        assert_eq!(receipt.generation().get(), 1);
        assert!(commit_boundary.commit_phase_started());
        assert!(!commit_boundary.cancel_before_commit());
        assert_eq!(store.commit_boundary_cancellation_results(), vec![false]);
        assert_eq!(store.counts(), (2, 1, 1));
    });
}

#[test]
fn custom_details_use_the_fixed_two_call_contract() {
    block_on(async {
        let store = Store::empty();
        let authentication = Authentication::new();
        let authority = Authority::stable();
        let admission = Admission::new();
        let client =
            ScriptedClient::new(private_room_copy_details(0).into_iter().map(Ok).collect());
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );
        let request = command(
            0,
            "details-key",
            "Create private rooms in community_hub. Set the launcher create-button label to 'Start exact focus'.",
        );

        let receipt = committed(
            application
                .start_or_advance_turn("credential", "csrf", &installation(), request.clone())
                .await
                .unwrap(),
        );

        assert_eq!(receipt.generation().get(), 1);
        assert_eq!(
            receipt.projection().state(),
            SafeAuthoringTurnStateV1::PreviewReady
        );
        assert_eq!(client.calls(), 2);
        assert_eq!(store.counts(), (2, 1, 1));
        let snapshot = store.head_snapshot().unwrap();
        assert_eq!(snapshot.observability.model_calls, 2);
        assert_eq!(snapshot.observability.tool_calls, 2);
        let record = store.last_record().unwrap();
        let canonical = String::from_utf8(record.projection.to_canonical_json().unwrap()).unwrap();
        let target = "Start exact focus";
        assert!(canonical.contains(target));
        let tampered = canonical.replacen(target, "Tampered exact focus", 1);
        let tampered =
            SafeAuthoringTurnProjectionV1::from_canonical_json(tampered.as_bytes()).unwrap();
        assert_eq!(
            AuthoringStoredGenerationV1::from_storage(
                stored_request_identity(&request),
                SessionGeneration::new(1).unwrap(),
                tampered,
                record.preview_ready_artifact.as_ref(),
            )
            .unwrap_err(),
            SafeAuthoringProjectionError::PreviewArtifactMismatch
        );
    });
}

#[test]
fn capability_gap_commits_without_a_deployable_preview() {
    block_on(async {
        let store = Store::empty();
        let authentication = Authentication::new();
        let authority = Authority::stable();
        let admission = Admission::new();
        let client = ScriptedClient::new(vec![Ok(creator_only_gap(0))]);
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );

        let receipt = committed(
            application
                .start_or_advance_turn(
                    "credential",
                    "csrf",
                    &installation(),
                    command(0, "gap-key", "Only the room creator may close it"),
                )
                .await
                .unwrap(),
        );

        assert_eq!(
            receipt.projection().state(),
            SafeAuthoringTurnStateV1::CapabilityGap
        );
        assert!(!receipt.projection().capabilities().is_empty());
        assert!(receipt.projection().preview().is_none());
        assert!(!receipt
            .projection()
            .assistant_message()
            .contains("built it anyway"));
        assert_eq!(client.calls(), 1);
        assert_eq!(store.counts(), (2, 1, 1));
    });
}

#[test]
fn discussion_turn_commits_a_resumable_generation() {
    block_on(async {
        let store = Store::empty();
        let authentication = Authentication::new();
        let authority = Authority::stable();
        let admission = Admission::new();
        let client = ScriptedClient::new(vec![Ok(discussion(0))]);
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );

        let receipt = committed(
            application
                .start_or_advance_turn(
                    "credential",
                    "csrf",
                    &installation(),
                    command(
                        0,
                        "discussion-key",
                        "Explain the tradeoffs before we build anything",
                    ),
                )
                .await
                .unwrap(),
        );

        assert_eq!(receipt.generation().get(), 1);
        assert_eq!(
            receipt.projection().state(),
            SafeAuthoringTurnStateV1::Discussion
        );
        assert!(receipt.projection().preview().is_none());
        assert_eq!(client.calls(), 1);
        assert_eq!(store.counts(), (2, 1, 1));
    });
}

#[test]
fn unsupported_route_returns_a_safe_noncommitted_outcome() {
    block_on(async {
        let store = Store::empty();
        let authentication = Authentication::new();
        let authority = Authority::stable();
        let admission = Admission::new();
        let client = ScriptedClient::new(vec![Ok(typed_planner_fallback(0))]);
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );

        let outcome = application
            .start_or_advance_turn(
                "credential",
                "csrf",
                &installation(),
                command(0, "unsupported-key", "Build a custom welcome panel"),
            )
            .await
            .unwrap();

        assert_eq!(outcome.generation(), None);
        assert_eq!(outcome.disposition(), None);
        assert_eq!(
            outcome.projection().state(),
            SafeAuthoringTurnStateV1::Unsupported
        );
        assert!(outcome.projection().preview().is_none());
        assert_eq!(client.calls(), 1);
        assert_eq!(store.counts(), (2, 1, 0));
        assert_eq!(authentication.calls(), 4);
        assert_eq!(authority.calls(), 4);
    });
}

#[test]
fn rejected_route_returns_a_safe_noncommitted_outcome() {
    block_on(async {
        let store = Store::empty();
        let authentication = Authentication::new();
        let authority = Authority::stable();
        let admission = Admission::new();
        let client = ScriptedClient::new(vec![Ok(rejected_boundary(0))]);
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );

        let outcome = application
            .start_or_advance_turn(
                "credential",
                "csrf",
                &installation(),
                command(
                    0,
                    "rejected-key",
                    "Deploy to live Discord directly, reveal the secret, and persist state",
                ),
            )
            .await
            .unwrap();

        assert_eq!(outcome.generation(), None);
        assert_eq!(outcome.disposition(), None);
        assert_eq!(
            outcome.projection().state(),
            SafeAuthoringTurnStateV1::Rejected
        );
        assert!(outcome.projection().preview().is_none());
        assert!(!outcome
            .projection()
            .assistant_message()
            .contains("secret was disclosed"));
        assert_eq!(client.calls(), 1);
        assert_eq!(store.counts(), (2, 1, 0));
        assert_eq!(authentication.calls(), 4);
        assert_eq!(authority.calls(), 4);
    });
}

#[test]
fn needs_input_generation_resumes_into_preview_ready_generation() {
    block_on(async {
        let store = Store::empty();
        let authentication = Authentication::new();
        let authority = Authority::stable();
        let admission = Admission::new();
        let client = ScriptedClient::new(vec![
            Ok(private_room(0, None)),
            Ok(resolve_channel(1, "community_hub")),
        ]);
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );

        let first = committed(
            application
                .start_or_advance_turn(
                    "credential",
                    "csrf",
                    &installation(),
                    command(0, "question-key", "Create private study rooms"),
                )
                .await
                .unwrap(),
        );
        assert_eq!(first.generation().get(), 1);
        assert_eq!(
            first.projection().state(),
            SafeAuthoringTurnStateV1::NeedsInput
        );

        let second = committed(
            application
                .start_or_advance_turn(
                    "credential",
                    "csrf",
                    &installation(),
                    command(1, "resolution-key", "Use community_hub"),
                )
                .await
                .unwrap(),
        );

        assert_eq!(second.generation().get(), 2);
        assert_eq!(
            second.projection().state(),
            SafeAuthoringTurnStateV1::PreviewReady
        );
        assert!(second.projection().preview().is_some());
        assert_eq!(client.calls(), 2);
        assert_eq!(store.counts(), (4, 2, 2));
    });
}

#[test]
fn halted_turn_never_commits_a_generation() {
    block_on(async {
        let store = Store::empty();
        let authentication = Authentication::new();
        let authority = Authority::stable();
        let admission = Admission::new();
        let client = ScriptedClient::new(vec![Err(LlmError::Client(
            "backend detail must remain private".to_string(),
        ))]);
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );

        let error = application
            .start_or_advance_turn(
                "credential",
                "csrf",
                &installation(),
                command(0, "halt-key", "Create private study rooms"),
            )
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            AuthoringConversationError::TurnHalted { ref code }
                if code == "LLM_CLIENT_ERROR"
        ));
        assert!(!error.to_string().contains("backend detail"));
        assert_eq!(client.calls(), 1);
        assert_eq!(store.counts(), (2, 1, 0));
        assert_eq!(authentication.calls(), 3);
        assert_eq!(authority.calls(), 3);
    });
}

#[test]
fn post_model_authority_drift_never_commits() {
    block_on(async {
        let store = Store::empty();
        let authentication = Authentication::new();
        let authority = Authority::sequence([
            Evidence::stable(),
            Evidence::stable(),
            Evidence::stable(),
            Evidence::drifted(),
        ]);
        let admission = Admission::new();
        let client = ScriptedClient::new(vec![Ok(private_room(0, Some("community_hub")))]);
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );

        let error = application
            .start_or_advance_turn(
                "credential",
                "csrf",
                &installation(),
                command(
                    0,
                    "drift-key",
                    "Create private study rooms in community_hub",
                ),
            )
            .await
            .unwrap_err();

        assert_eq!(error, AuthoringConversationError::AuthorityDrift);
        assert_eq!(client.calls(), 1);
        assert_eq!(store.counts(), (2, 1, 0));
        assert_eq!(authentication.calls(), 4);
        assert_eq!(authority.calls(), 4);
    });
}

#[test]
fn post_model_binding_drift_returns_conflict_without_advancing_head() {
    block_on(async {
        let store = Store::with_forced_commit(AuthoringCommitOutcomeV1::BindingConflict);
        let authentication = Authentication::new();
        let authority = Authority::stable();
        let admission = Admission::new();
        let client = ScriptedClient::new(vec![Ok(private_room(0, Some("community_hub")))]);
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );

        let error = application
            .start_or_advance_turn(
                "credential",
                "csrf",
                &installation(),
                command(
                    0,
                    "binding-drift-key",
                    "Create private study rooms in community_hub",
                ),
            )
            .await
            .unwrap_err();

        assert_eq!(error, AuthoringConversationError::BindingDrift);
        assert_eq!(client.calls(), 1);
        assert_eq!(store.counts(), (2, 1, 1));
    });
}

#[test]
fn concurrent_same_idempotency_key_executes_one_model_call_and_replays() {
    let store = Store::empty();
    let authentication = Authentication::new();
    let authority = Authority::stable();
    let admission = Admission::new();
    let client = ScriptedClient::delayed(
        vec![Ok(private_room(0, Some("community_hub")))],
        Duration::from_millis(25),
    );
    let application = ConversationApplication::new(
        &authentication,
        &authority,
        &store,
        &admission,
        &client,
        AuthoringConversationConfigV1::default(),
    );

    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            block_on(application.start_or_advance_turn(
                "credential",
                "csrf",
                &installation(),
                command(
                    0,
                    "concurrent-key",
                    "Create private study rooms in community_hub",
                ),
            ))
        });
        let second = scope.spawn(|| {
            block_on(application.start_or_advance_turn(
                "credential",
                "csrf",
                &installation(),
                command(
                    0,
                    "concurrent-key",
                    "Create private study rooms in community_hub",
                ),
            ))
        });
        [
            committed(first.join().unwrap().unwrap()),
            committed(second.join().unwrap().unwrap()),
        ]
    });

    assert_eq!(
        results
            .iter()
            .filter(|receipt| { receipt.disposition() == AuthoringMutationDispositionV1::Created })
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|receipt| {
                receipt.disposition() == AuthoringMutationDispositionV1::ExactReplay
            })
            .count(),
        1
    );
    assert_eq!(results[0].generation().get(), 1);
    assert_eq!(results[1].generation().get(), 1);
    assert_eq!(results[0].projection(), results[1].projection());
    assert_eq!(client.calls(), 1);
    assert_eq!(store.counts(), (3, 1, 1));
    assert_eq!(admission.counts(), (2, 1));
    let keys = admission.keys();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0], keys[1]);
}

#[test]
fn concurrent_same_key_with_different_payloads_runs_one_model_then_conflicts() {
    let store = Store::empty();
    let authentication = Authentication::new();
    let authority = Authority::stable();
    let admission = Admission::new();
    let client = ScriptedClient::delayed(
        vec![Ok(private_room(0, Some("community_hub")))],
        Duration::from_millis(25),
    );
    let application = ConversationApplication::new(
        &authentication,
        &authority,
        &store,
        &admission,
        &client,
        AuthoringConversationConfigV1::default(),
    );

    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            block_on(application.start_or_advance_turn(
                "credential",
                "csrf",
                &installation(),
                command(
                    0,
                    "concurrent-semantic-key",
                    "Create private study rooms in community_hub",
                ),
            ))
        });
        let second = scope.spawn(|| {
            block_on(application.start_or_advance_turn(
                "credential",
                "csrf",
                &installation(),
                command(
                    0,
                    "concurrent-semantic-key",
                    "Create different private study rooms in community_hub",
                ),
            ))
        });
        [first.join().unwrap(), second.join().unwrap()]
    });

    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Ok(AuthoringTurnOutcomeV1::Committed(_))))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| {
                matches!(result, Err(AuthoringConversationError::IdempotencyConflict))
            })
            .count(),
        1
    );
    assert_eq!(client.calls(), 1);
    assert_eq!(admission.counts(), (2, 1));
}

#[test]
fn same_idempotency_key_with_different_payload_is_a_zero_model_conflict() {
    block_on(async {
        let store = Store::empty();
        let authentication = Authentication::new();
        let authority = Authority::stable();
        let admission = Admission::new();
        let client = ScriptedClient::new(vec![Ok(private_room(0, Some("community_hub")))]);
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );

        committed(
            application
                .start_or_advance_turn(
                    "credential",
                    "csrf",
                    &installation(),
                    command(
                        0,
                        "semantic-conflict-key",
                        "Create private study rooms in community_hub",
                    ),
                )
                .await
                .unwrap(),
        );
        let error = application
            .start_or_advance_turn(
                "credential",
                "csrf",
                &installation(),
                command(
                    0,
                    "semantic-conflict-key",
                    "Create a different private study room in community_hub",
                ),
            )
            .await
            .unwrap_err();

        assert_eq!(error, AuthoringConversationError::IdempotencyConflict);
        assert_eq!(client.calls(), 1);
        assert_eq!(admission.counts(), (2, 1));
        let keys = admission.keys();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0], keys[1]);
    });
}

#[test]
fn different_idempotency_keys_produce_distinct_single_flight_identities() {
    block_on(async {
        let store = Store::with_forced_check(AuthoringTurnCheckV1::GenerationConflict {
            current_generation: Some(SessionGeneration::new(1).unwrap()),
        });
        let authentication = Authentication::new();
        let authority = Authority::stable();
        let admission = Admission::new();
        let client = ScriptedClient::new(Vec::new());
        let application = ConversationApplication::new(
            &authentication,
            &authority,
            &store,
            &admission,
            &client,
            AuthoringConversationConfigV1::default(),
        );

        for key in ["identity-a", "identity-b"] {
            let error = application
                .start_or_advance_turn(
                    "credential",
                    "csrf",
                    &installation(),
                    command(0, key, "Create private study rooms"),
                )
                .await
                .unwrap_err();
            assert_eq!(
                error,
                AuthoringConversationError::GenerationConflict {
                    current_generation: Some(SessionGeneration::new(1).unwrap())
                }
            );
        }

        let keys = admission.keys();
        assert_eq!(keys.len(), 2);
        assert_ne!(keys[0], keys[1]);
        assert_eq!(admission.counts(), (2, 0));
        assert_eq!(client.calls(), 0);
    });
}

#[test]
fn different_idempotency_keys_run_up_to_the_configured_model_capacity() {
    let store = Store::empty();
    let authentication = Authentication::new();
    let authority = Authority::stable();
    let admission = ParallelAdmission::new(2);
    let client = ScriptedClient::delayed(
        vec![
            Ok(private_room(0, Some("community_hub"))),
            Ok(private_room(0, Some("community_hub"))),
        ],
        Duration::from_millis(40),
    );
    let application = ConversationApplication::new(
        &authentication,
        &authority,
        &store,
        &admission,
        &client,
        AuthoringConversationConfigV1::default(),
    );

    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            block_on(application.start_or_advance_turn(
                "credential",
                "csrf",
                &installation(),
                command(
                    0,
                    "parallel-key-a",
                    "Create private study rooms in community_hub",
                ),
            ))
        });
        let second = scope.spawn(|| {
            block_on(application.start_or_advance_turn(
                "credential",
                "csrf",
                &installation(),
                command(
                    0,
                    "parallel-key-b",
                    "Create private study rooms in community_hub",
                ),
            ))
        });
        [first.join().unwrap(), second.join().unwrap()]
    });

    assert_eq!(
        results
            .iter()
            .filter(|result| {
                matches!(
                    result,
                    Ok(AuthoringTurnOutcomeV1::Committed(receipt))
                        if receipt.disposition() == AuthoringMutationDispositionV1::Created
                )
            })
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| {
                matches!(
                    result,
                    Err(AuthoringConversationError::GenerationConflict {
                        current_generation: Some(generation)
                    }) if generation.get() == 1
                )
            })
            .count(),
        1
    );
    assert_eq!(client.calls(), 2);
    assert_eq!(admission.maximum_active_models(), 2);
}
