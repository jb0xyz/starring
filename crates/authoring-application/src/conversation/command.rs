use std::fmt::{Debug, Formatter};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use authoring_promotion::{
    AuthoringSessionId, AutomationInstallationId, PrincipalId, SessionGeneration, TenantId,
};
use design_harness::SessionConfig;

use crate::{AuthorizedInstallationScopeV1, ProductIdempotencyKeyV1};

const MAX_SAFE_JSON_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_EXPECTED_GENERATION: u64 = MAX_SAFE_JSON_INTEGER - 1;
const MAX_HUMAN_MESSAGE_BYTES: usize = 8_000;
const MAX_HUMAN_MESSAGE_SCALARS: usize = 2_000;
const MIN_CONTEXT_CHARS: usize = 8_000;
const MAX_CONTEXT_CHARS: usize = 64_000;
const DEFAULT_CONTEXT_CHARS: usize = 44_000;
pub const AUTHORING_MAX_MODEL_CALLS_V1: u32 = 2;
const AUTHORING_COMMIT_OPEN: u8 = 0;
const AUTHORING_COMMIT_STARTED: u8 = 1;
const AUTHORING_COMMIT_CANCELLED: u8 = 2;

#[derive(Clone)]
pub struct AuthoringCommitBoundaryV1 {
    state: Arc<AtomicU8>,
}

impl AuthoringCommitBoundaryV1 {
    pub fn new() -> Self {
        Self {
            state: Arc::new(AtomicU8::new(AUTHORING_COMMIT_OPEN)),
        }
    }

    pub fn cancel_before_commit(&self) -> bool {
        self.state
            .compare_exchange(
                AUTHORING_COMMIT_OPEN,
                AUTHORING_COMMIT_CANCELLED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    pub fn enter_commit_phase(&self) -> bool {
        self.state
            .compare_exchange(
                AUTHORING_COMMIT_OPEN,
                AUTHORING_COMMIT_STARTED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    pub fn commit_phase_started(&self) -> bool {
        self.state.load(Ordering::Acquire) == AUTHORING_COMMIT_STARTED
    }

    pub(crate) fn cancelled_before_commit(&self) -> bool {
        self.state.load(Ordering::Acquire) == AUTHORING_COMMIT_CANCELLED
    }
}

impl Default for AuthoringCommitBoundaryV1 {
    fn default() -> Self {
        Self::new()
    }
}

impl Debug for AuthoringCommitBoundaryV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthoringCommitBoundaryV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthoringExpectedGenerationError {
    #[error("expected authoring generation exceeds the safe JSON integer range")]
    TooLarge,
    #[error("expected authoring generation cannot advance")]
    Overflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AuthoringExpectedGenerationV1(u64);

impl AuthoringExpectedGenerationV1 {
    pub fn new(value: u64) -> Result<Self, AuthoringExpectedGenerationError> {
        (value <= MAX_EXPECTED_GENERATION)
            .then_some(Self(value))
            .ok_or(AuthoringExpectedGenerationError::TooLarge)
    }

    pub fn get(self) -> u64 {
        self.0
    }

    pub(crate) fn successor(self) -> Result<SessionGeneration, AuthoringExpectedGenerationError> {
        self.0
            .checked_add(1)
            .filter(|value| *value <= MAX_SAFE_JSON_INTEGER)
            .and_then(|value| SessionGeneration::new(value).ok())
            .ok_or(AuthoringExpectedGenerationError::Overflow)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthoringHumanMessageError {
    #[error("authoring message must not be empty")]
    Empty,
    #[error(
        "authoring message exceeds {MAX_HUMAN_MESSAGE_SCALARS} Unicode scalars or {MAX_HUMAN_MESSAGE_BYTES} bytes"
    )]
    TooLong,
    #[error("authoring message contains control characters")]
    ControlCharacter,
}

#[derive(Clone, PartialEq, Eq)]
pub struct AuthoringHumanMessageV1(String);

impl AuthoringHumanMessageV1 {
    pub fn parse(value: &str) -> Result<Self, AuthoringHumanMessageError> {
        let value = value.trim().replace("\r\n", "\n").replace('\r', "\n");
        if value.is_empty() {
            return Err(AuthoringHumanMessageError::Empty);
        }
        if value.len() > MAX_HUMAN_MESSAGE_BYTES
            || value.chars().count() > MAX_HUMAN_MESSAGE_SCALARS
        {
            return Err(AuthoringHumanMessageError::TooLong);
        }
        if value.chars().any(is_forbidden_human_control) {
            return Err(AuthoringHumanMessageError::ControlCharacter);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_forbidden_human_control(character: char) -> bool {
    (character.is_control() && character != '\n')
        || matches!(
            character,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

impl Debug for AuthoringHumanMessageV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthoringHumanMessageV1(<redacted>)")
    }
}

#[derive(Clone)]
pub struct StartOrAdvanceAuthoringTurnV1 {
    session_id: AuthoringSessionId,
    expected_generation: AuthoringExpectedGenerationV1,
    idempotency_key: ProductIdempotencyKeyV1,
    human_message: AuthoringHumanMessageV1,
    commit_boundary: AuthoringCommitBoundaryV1,
}

impl Debug for StartOrAdvanceAuthoringTurnV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("StartOrAdvanceAuthoringTurnV1(<redacted>)")
    }
}

impl StartOrAdvanceAuthoringTurnV1 {
    pub fn new(
        session_id: AuthoringSessionId,
        expected_generation: AuthoringExpectedGenerationV1,
        idempotency_key: ProductIdempotencyKeyV1,
        human_message: AuthoringHumanMessageV1,
    ) -> Self {
        Self::new_with_commit_boundary(
            session_id,
            expected_generation,
            idempotency_key,
            human_message,
            AuthoringCommitBoundaryV1::new(),
        )
    }

    pub fn new_with_commit_boundary(
        session_id: AuthoringSessionId,
        expected_generation: AuthoringExpectedGenerationV1,
        idempotency_key: ProductIdempotencyKeyV1,
        human_message: AuthoringHumanMessageV1,
        commit_boundary: AuthoringCommitBoundaryV1,
    ) -> Self {
        Self {
            session_id,
            expected_generation,
            idempotency_key,
            human_message,
            commit_boundary,
        }
    }

    pub fn session_id(&self) -> &AuthoringSessionId {
        &self.session_id
    }

    pub fn expected_generation(&self) -> AuthoringExpectedGenerationV1 {
        self.expected_generation
    }

    pub fn idempotency_key(&self) -> &ProductIdempotencyKeyV1 {
        &self.idempotency_key
    }

    pub fn human_message(&self) -> &AuthoringHumanMessageV1 {
        &self.human_message
    }

    pub fn commit_boundary(&self) -> &AuthoringCommitBoundaryV1 {
        &self.commit_boundary
    }
}

impl PartialEq for StartOrAdvanceAuthoringTurnV1 {
    fn eq(&self, other: &Self) -> bool {
        self.session_id == other.session_id
            && self.expected_generation == other.expected_generation
            && self.idempotency_key == other.idempotency_key
            && self.human_message == other.human_message
    }
}

impl Eq for StartOrAdvanceAuthoringTurnV1 {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadAuthoringSessionV1 {
    session_id: AuthoringSessionId,
}

impl ReadAuthoringSessionV1 {
    pub fn new(session_id: AuthoringSessionId) -> Self {
        Self { session_id }
    }

    pub fn session_id(&self) -> &AuthoringSessionId {
        &self.session_id
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthoringConversationConfigError {
    #[error("authoring context budget must be between 8000 and 64000 characters")]
    InvalidContextBudget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthoringConversationConfigV1 {
    context_char_budget: usize,
}

impl AuthoringConversationConfigV1 {
    pub fn new(context_char_budget: usize) -> Result<Self, AuthoringConversationConfigError> {
        (MIN_CONTEXT_CHARS..=MAX_CONTEXT_CHARS)
            .contains(&context_char_budget)
            .then_some(Self {
                context_char_budget,
            })
            .ok_or(AuthoringConversationConfigError::InvalidContextBudget)
    }

    pub fn context_char_budget(&self) -> usize {
        self.context_char_budget
    }

    pub(crate) fn session_config(&self) -> SessionConfig {
        SessionConfig {
            max_model_calls: AUTHORING_MAX_MODEL_CALLS_V1 as usize,
            max_tool_calls: 2,
            max_gate_failures: 1,
            context_char_budget: self.context_char_budget,
        }
    }
}

impl Default for AuthoringConversationConfigV1 {
    fn default() -> Self {
        Self::new(DEFAULT_CONTEXT_CHARS)
            .expect("default authoring conversation configuration must be valid")
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalAuthoringRequestKeyV1 {
    tenant_id: TenantId,
    installation_id: AutomationInstallationId,
    principal_id: PrincipalId,
    session_id: AuthoringSessionId,
    idempotency_key: ProductIdempotencyKeyV1,
}

impl LocalAuthoringRequestKeyV1 {
    pub fn from_authorized_scope(
        principal_id: PrincipalId,
        scope: &AuthorizedInstallationScopeV1,
        command: &StartOrAdvanceAuthoringTurnV1,
    ) -> Self {
        Self {
            tenant_id: scope.tenant_id().clone(),
            installation_id: scope.installation_id().clone(),
            principal_id,
            session_id: command.session_id().clone(),
            idempotency_key: command.idempotency_key().clone(),
        }
    }
}

impl Debug for LocalAuthoringRequestKeyV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LocalAuthoringRequestKeyV1(<redacted>)")
    }
}
