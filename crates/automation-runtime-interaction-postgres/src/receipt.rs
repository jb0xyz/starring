use std::cmp::Ordering;
use std::fmt::{Debug, Formatter};
use std::num::NonZeroUsize;
use std::time::Duration;

use automation_instance::InstanceId;
use automation_runtime_convergence::ProcessInstanceId;
use automation_runtime_interaction::{
    build_interaction_token_authenticated_data_v1, EncryptedInteractionTokenV1,
    InteractionActionPlanDigestV1, InteractionExpectedRouteV1, InteractionReceiptClaimRootV1,
    InteractionReceiptIdentityV1, InteractionReceiptStateV1, InteractionRuntimeBuildRevisionV1,
    InteractionTokenAuthenticatedDataInputV1,
};
use chrono::{DateTime, TimeZone, Utc};
use discord_model::{ChannelId, UserId};

use crate::RuntimeInteractionPersistenceErrorV1;

pub const MIN_RUNTIME_INTERACTION_RECEIPT_CLAIM_LEASE: Duration = Duration::from_secs(1);
pub const DEFAULT_RUNTIME_INTERACTION_RECEIPT_CLAIM_LEASE: Duration = Duration::from_secs(30);
pub const MAX_RUNTIME_INTERACTION_RECEIPT_CLAIM_LEASE: Duration = Duration::from_secs(5 * 60);
pub const MAX_RUNTIME_INTERACTION_RECEIPT_RECOVERY_SCAN_BATCH: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionReceiptRequestKindV1 {
    MessageComponent,
    ModalSubmit,
}

impl RuntimeInteractionReceiptRequestKindV1 {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::MessageComponent => "message_component",
            Self::ModalSubmit => "modal_submit",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionReceiptRouteV1 {
    Static {
        route_key: String,
    },
    Instance {
        route_key: String,
        instance_id: InstanceId,
    },
}

impl RuntimeInteractionReceiptRouteV1 {
    pub fn static_route(
        route_key: impl Into<String>,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        let route_key = route_key.into();
        validate_route_key(&route_key)?;
        Ok(Self::Static { route_key })
    }

    pub fn instance_route(
        route_key: impl Into<String>,
        instance_id: InstanceId,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        let route_key = route_key.into();
        validate_route_key(&route_key)?;
        Ok(Self::Instance {
            route_key,
            instance_id,
        })
    }

    pub fn route_key(&self) -> &str {
        match self {
            Self::Static { route_key } | Self::Instance { route_key, .. } => route_key,
        }
    }

    pub fn instance_id(&self) -> Option<&InstanceId> {
        match self {
            Self::Static { .. } => None,
            Self::Instance { instance_id, .. } => Some(instance_id),
        }
    }

    pub(crate) const fn kind_code(&self) -> &'static str {
        match self {
            Self::Static { .. } => "static",
            Self::Instance { .. } => "instance",
        }
    }

    pub(crate) fn instance_id_parameter(&self) -> &str {
        self.instance_id().map(InstanceId::as_str).unwrap_or("")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeInteractionReceiptClaimLeaseV1(Duration);

impl RuntimeInteractionReceiptClaimLeaseV1 {
    pub fn new(duration: Duration) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        if duration < MIN_RUNTIME_INTERACTION_RECEIPT_CLAIM_LEASE
            || duration > MAX_RUNTIME_INTERACTION_RECEIPT_CLAIM_LEASE
            || !duration.subsec_nanos().is_multiple_of(1_000_000)
        {
            return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
        }
        Ok(Self(duration))
    }

    pub fn duration(self) -> Duration {
        self.0
    }

    pub(crate) fn milliseconds(self) -> i64 {
        i64::try_from(self.0.as_millis()).expect("bounded claim lease")
    }
}

impl Default for RuntimeInteractionReceiptClaimLeaseV1 {
    fn default() -> Self {
        Self(DEFAULT_RUNTIME_INTERACTION_RECEIPT_CLAIM_LEASE)
    }
}

pub struct RuntimeInteractionReceiptAuthorityV1 {
    claim_root: InteractionReceiptClaimRootV1,
    route: RuntimeInteractionReceiptRouteV1,
    observed_database_now: DateTime<Utc>,
}

impl RuntimeInteractionReceiptAuthorityV1 {
    pub fn claim_root(&self) -> &InteractionReceiptClaimRootV1 {
        &self.claim_root
    }

    pub fn route(&self) -> &RuntimeInteractionReceiptRouteV1 {
        &self.route
    }

    pub fn observed_database_now(&self) -> DateTime<Utc> {
        self.observed_database_now
    }

    pub(crate) fn new(
        claim_root: InteractionReceiptClaimRootV1,
        route: RuntimeInteractionReceiptRouteV1,
        observed_database_now: DateTime<Utc>,
    ) -> Self {
        Self {
            claim_root,
            route,
            observed_database_now,
        }
    }
}

impl Debug for RuntimeInteractionReceiptAuthorityV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptAuthorityV1(<redacted>)")
    }
}

pub struct RuntimeInteractionReceiptClaimRequestV1 {
    authority: RuntimeInteractionReceiptAuthorityV1,
    channel_id: ChannelId,
    actor_user_id: UserId,
    request_kind: RuntimeInteractionReceiptRequestKindV1,
    encrypted_token: EncryptedInteractionTokenV1,
    claim_lease: RuntimeInteractionReceiptClaimLeaseV1,
}

impl RuntimeInteractionReceiptClaimRequestV1 {
    pub fn new(
        authority: RuntimeInteractionReceiptAuthorityV1,
        channel_id: ChannelId,
        actor_user_id: UserId,
        request_kind: RuntimeInteractionReceiptRequestKindV1,
        encrypted_token: EncryptedInteractionTokenV1,
        claim_lease: RuntimeInteractionReceiptClaimLeaseV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        if channel_id.0 == 0 || actor_user_id.0 == 0 {
            return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
        }
        validate_envelope_authenticated_data(&authority.claim_root, &encrypted_token)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
        Ok(Self {
            authority,
            channel_id,
            actor_user_id,
            request_kind,
            encrypted_token,
            claim_lease,
        })
    }

    pub fn claim_root(&self) -> &InteractionReceiptClaimRootV1 {
        &self.authority.claim_root
    }

    pub(crate) fn authority(&self) -> &RuntimeInteractionReceiptAuthorityV1 {
        &self.authority
    }

    pub(crate) const fn channel_id(&self) -> ChannelId {
        self.channel_id
    }

    pub(crate) const fn actor_user_id(&self) -> UserId {
        self.actor_user_id
    }

    pub(crate) const fn request_kind(&self) -> RuntimeInteractionReceiptRequestKindV1 {
        self.request_kind
    }

    pub(crate) fn encrypted_token(&self) -> &EncryptedInteractionTokenV1 {
        &self.encrypted_token
    }

    pub(crate) const fn claim_lease(&self) -> RuntimeInteractionReceiptClaimLeaseV1 {
        self.claim_lease
    }
}

impl Debug for RuntimeInteractionReceiptClaimRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptClaimRequestV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionReceiptOpaqueDigestV1([u8; 32]);

impl RuntimeInteractionReceiptOpaqueDigestV1 {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        <[u8; 32]>::try_from(bytes)
            .map(Self)
            .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Debug for RuntimeInteractionReceiptOpaqueDigestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptOpaqueDigestV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionReceiptInitialResponseKindV1 {
    DeferEphemeral,
    RespondEphemeral,
    RespondMessage,
    OpenModal,
    UpdateMessage,
}

impl RuntimeInteractionReceiptInitialResponseKindV1 {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::DeferEphemeral => "defer_ephemeral",
            Self::RespondEphemeral => "respond_ephemeral",
            Self::RespondMessage => "respond_message",
            Self::OpenModal => "open_modal",
            Self::UpdateMessage => "update_message",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionReceiptInitialResponseIntentV1 {
    kind: RuntimeInteractionReceiptInitialResponseKindV1,
    digest: RuntimeInteractionReceiptOpaqueDigestV1,
}

impl RuntimeInteractionReceiptInitialResponseIntentV1 {
    pub const fn new(
        kind: RuntimeInteractionReceiptInitialResponseKindV1,
        digest: RuntimeInteractionReceiptOpaqueDigestV1,
    ) -> Self {
        Self { kind, digest }
    }

    pub const fn kind(&self) -> RuntimeInteractionReceiptInitialResponseKindV1 {
        self.kind
    }

    pub const fn digest(&self) -> &RuntimeInteractionReceiptOpaqueDigestV1 {
        &self.digest
    }
}

impl Debug for RuntimeInteractionReceiptInitialResponseIntentV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptInitialResponseIntentV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionReceiptInitialResponseResultKindV1 {
    Succeeded,
    DefinitiveFailure,
    Indeterminate,
}

impl RuntimeInteractionReceiptInitialResponseResultKindV1 {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::DefinitiveFailure => "definitive_failure",
            Self::Indeterminate => "indeterminate",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionReceiptInitialResponseResultV1 {
    intent_digest: RuntimeInteractionReceiptOpaqueDigestV1,
    result: RuntimeInteractionReceiptInitialResponseResultKindV1,
    result_digest: RuntimeInteractionReceiptOpaqueDigestV1,
}

impl RuntimeInteractionReceiptInitialResponseResultV1 {
    pub const fn new(
        intent_digest: RuntimeInteractionReceiptOpaqueDigestV1,
        result: RuntimeInteractionReceiptInitialResponseResultKindV1,
        result_digest: RuntimeInteractionReceiptOpaqueDigestV1,
    ) -> Self {
        Self {
            intent_digest,
            result,
            result_digest,
        }
    }

    pub const fn intent_digest(&self) -> &RuntimeInteractionReceiptOpaqueDigestV1 {
        &self.intent_digest
    }

    pub const fn result(&self) -> RuntimeInteractionReceiptInitialResponseResultKindV1 {
        self.result
    }

    pub const fn result_digest(&self) -> &RuntimeInteractionReceiptOpaqueDigestV1 {
        &self.result_digest
    }
}

impl Debug for RuntimeInteractionReceiptInitialResponseResultV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptInitialResponseResultV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionReceiptTerminalStateV1 {
    Completed,
    Failed,
    RecoveryRequired,
}

impl RuntimeInteractionReceiptTerminalStateV1 {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::RecoveryRequired => "recovery_required",
        }
    }

    pub(crate) const fn state(self) -> InteractionReceiptStateV1 {
        match self {
            Self::Completed => InteractionReceiptStateV1::Completed,
            Self::Failed => InteractionReceiptStateV1::Failed,
            Self::RecoveryRequired => InteractionReceiptStateV1::RecoveryRequired,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionReceiptTerminalOutcomeV1 {
    state: RuntimeInteractionReceiptTerminalStateV1,
    outcome_code: String,
    result_digest: RuntimeInteractionReceiptOpaqueDigestV1,
}

impl RuntimeInteractionReceiptTerminalOutcomeV1 {
    pub fn new(
        state: RuntimeInteractionReceiptTerminalStateV1,
        outcome_code: impl Into<String>,
        result_digest: RuntimeInteractionReceiptOpaqueDigestV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        let outcome_code = outcome_code.into();
        if outcome_code.is_empty()
            || outcome_code.len() > 64
            || outcome_code == "exact_replay"
            || !outcome_code
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
        }
        Ok(Self {
            state,
            outcome_code,
            result_digest,
        })
    }

    pub const fn state(&self) -> RuntimeInteractionReceiptTerminalStateV1 {
        self.state
    }

    pub fn outcome_code(&self) -> &str {
        &self.outcome_code
    }

    pub const fn result_digest(&self) -> &RuntimeInteractionReceiptOpaqueDigestV1 {
        &self.result_digest
    }
}

impl Debug for RuntimeInteractionReceiptTerminalOutcomeV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptTerminalOutcomeV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionReceiptMutationDispositionV1 {
    Applied,
    ExactReplay,
}

impl RuntimeInteractionReceiptMutationDispositionV1 {
    pub const fn is_applied(self) -> bool {
        matches!(self, Self::Applied)
    }

    pub const fn is_exact_replay(self) -> bool {
        matches!(self, Self::ExactReplay)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionReceiptInitialResponseIntentDispositionV1 {
    ExternalCallAuthorized,
    ExactReplaySuppressed,
}

impl RuntimeInteractionReceiptInitialResponseIntentDispositionV1 {
    pub const fn external_call_authorized(self) -> bool {
        matches!(self, Self::ExternalCallAuthorized)
    }

    pub(crate) const fn from_mutation(
        disposition: RuntimeInteractionReceiptMutationDispositionV1,
    ) -> Self {
        match disposition {
            RuntimeInteractionReceiptMutationDispositionV1::Applied => Self::ExternalCallAuthorized,
            RuntimeInteractionReceiptMutationDispositionV1::ExactReplay => {
                Self::ExactReplaySuppressed
            }
        }
    }
}

pub struct RuntimeInteractionReceiptExclusiveClaimV1 {
    claim_root: InteractionReceiptClaimRootV1,
    state: InteractionReceiptStateV1,
    head_revision: u64,
    claim_revision: u64,
    claim_process_instance_id: ProcessInstanceId,
    claim_expires_at: DateTime<Utc>,
    observed_database_now: DateTime<Utc>,
    action_plan_digest: Option<InteractionActionPlanDigestV1>,
    acknowledgement_intent: Option<RuntimeInteractionReceiptInitialResponseIntentV1>,
}

impl RuntimeInteractionReceiptExclusiveClaimV1 {
    pub fn claim_root(&self) -> &InteractionReceiptClaimRootV1 {
        &self.claim_root
    }

    pub const fn state(&self) -> InteractionReceiptStateV1 {
        self.state
    }

    pub const fn head_revision(&self) -> u64 {
        self.head_revision
    }

    pub const fn claim_revision(&self) -> u64 {
        self.claim_revision
    }

    pub fn claim_process_instance_id(&self) -> &ProcessInstanceId {
        &self.claim_process_instance_id
    }

    pub const fn claim_expires_at(&self) -> DateTime<Utc> {
        self.claim_expires_at
    }

    pub const fn observed_database_now(&self) -> DateTime<Utc> {
        self.observed_database_now
    }

    pub fn action_plan_digest(&self) -> Option<&InteractionActionPlanDigestV1> {
        self.action_plan_digest.as_ref()
    }

    pub(crate) fn new(
        claim_root: InteractionReceiptClaimRootV1,
        state: InteractionReceiptStateV1,
        head_revision: u64,
        claim_revision: u64,
        claim_process_instance_id: ProcessInstanceId,
        claim_expires_at: DateTime<Utc>,
        observed_database_now: DateTime<Utc>,
    ) -> Self {
        Self {
            claim_root,
            state,
            head_revision,
            claim_revision,
            claim_process_instance_id,
            claim_expires_at,
            observed_database_now,
            action_plan_digest: None,
            acknowledgement_intent: None,
        }
    }

    pub(crate) fn update_checkpoint(
        &mut self,
        state: InteractionReceiptStateV1,
        head_revision: u64,
        claim_revision: u64,
        claim_expires_at: DateTime<Utc>,
        observed_database_now: DateTime<Utc>,
    ) {
        self.state = state;
        self.head_revision = head_revision;
        self.claim_revision = claim_revision;
        self.claim_expires_at = claim_expires_at;
        self.observed_database_now = observed_database_now;
    }

    pub(crate) fn set_action_plan(&mut self, digest: InteractionActionPlanDigestV1) {
        self.action_plan_digest = Some(digest);
    }

    pub(crate) fn acknowledgement_intent(
        &self,
    ) -> Option<&RuntimeInteractionReceiptInitialResponseIntentV1> {
        self.acknowledgement_intent.as_ref()
    }

    pub(crate) fn set_acknowledgement_intent(
        &mut self,
        intent: RuntimeInteractionReceiptInitialResponseIntentV1,
    ) {
        self.acknowledgement_intent = Some(intent);
    }
}

impl Debug for RuntimeInteractionReceiptExclusiveClaimV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptExclusiveClaimV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionReceiptClaimDuplicateV1 {
    identity: InteractionReceiptIdentityV1,
    state: InteractionReceiptStateV1,
    head_revision: u64,
    claim_revision: u64,
    claim_expires_at: DateTime<Utc>,
    observed_database_now: DateTime<Utc>,
}

impl Debug for RuntimeInteractionReceiptClaimDuplicateV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptClaimDuplicateV1(<redacted>)")
    }
}

impl RuntimeInteractionReceiptClaimDuplicateV1 {
    pub const fn identity(&self) -> InteractionReceiptIdentityV1 {
        self.identity
    }

    pub const fn state(&self) -> InteractionReceiptStateV1 {
        self.state
    }

    pub const fn head_revision(&self) -> u64 {
        self.head_revision
    }

    pub const fn claim_revision(&self) -> u64 {
        self.claim_revision
    }

    pub const fn claim_expires_at(&self) -> DateTime<Utc> {
        self.claim_expires_at
    }

    pub const fn observed_database_now(&self) -> DateTime<Utc> {
        self.observed_database_now
    }

    pub(crate) const fn new(
        identity: InteractionReceiptIdentityV1,
        state: InteractionReceiptStateV1,
        head_revision: u64,
        claim_revision: u64,
        claim_expires_at: DateTime<Utc>,
        observed_database_now: DateTime<Utc>,
    ) -> Self {
        Self {
            identity,
            state,
            head_revision,
            claim_revision,
            claim_expires_at,
            observed_database_now,
        }
    }
}

pub enum RuntimeInteractionReceiptClaimOutcomeV1 {
    Acquired(Box<RuntimeInteractionReceiptExclusiveClaimV1>),
    CompletedDuplicate(RuntimeInteractionReceiptClaimDuplicateV1),
    InFlightDuplicate(RuntimeInteractionReceiptClaimDuplicateV1),
    TerminalDuplicate(RuntimeInteractionReceiptClaimDuplicateV1),
    RecoveryRequired(RuntimeInteractionReceiptClaimDuplicateV1),
}

impl RuntimeInteractionReceiptClaimOutcomeV1 {
    pub fn into_exclusive_claim(self) -> Option<RuntimeInteractionReceiptExclusiveClaimV1> {
        match self {
            Self::Acquired(claim) => Some(*claim),
            Self::CompletedDuplicate(_)
            | Self::InFlightDuplicate(_)
            | Self::TerminalDuplicate(_)
            | Self::RecoveryRequired(_) => None,
        }
    }
}

impl Debug for RuntimeInteractionReceiptClaimOutcomeV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Acquired(_) => formatter.write_str("Acquired(<redacted>)"),
            Self::CompletedDuplicate(_) => formatter.write_str("CompletedDuplicate(<redacted>)"),
            Self::InFlightDuplicate(_) => formatter.write_str("InFlightDuplicate(<redacted>)"),
            Self::TerminalDuplicate(_) => formatter.write_str("TerminalDuplicate(<redacted>)"),
            Self::RecoveryRequired(_) => formatter.write_str("RecoveryRequired(<redacted>)"),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionReceiptRecoveryScanKeyV1 {
    claim_expires_at: DateTime<Utc>,
    identity: InteractionReceiptIdentityV1,
}

impl RuntimeInteractionReceiptRecoveryScanKeyV1 {
    pub fn new(
        claim_expires_at: DateTime<Utc>,
        identity: InteractionReceiptIdentityV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        validate_database_time(claim_expires_at, false)?;
        Ok(Self {
            claim_expires_at,
            identity,
        })
    }

    pub const fn claim_expires_at(&self) -> DateTime<Utc> {
        self.claim_expires_at
    }

    pub const fn identity(&self) -> InteractionReceiptIdentityV1 {
        self.identity
    }

    pub(crate) fn cmp_c(&self, other: &Self) -> Ordering {
        self.claim_expires_at
            .cmp(&other.claim_expires_at)
            .then_with(|| {
                self.identity
                    .application_id()
                    .get()
                    .to_string()
                    .cmp(&other.identity.application_id().get().to_string())
            })
            .then_with(|| {
                self.identity
                    .interaction_id()
                    .get()
                    .to_string()
                    .cmp(&other.identity.interaction_id().get().to_string())
            })
    }
}

impl Debug for RuntimeInteractionReceiptRecoveryScanKeyV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptRecoveryScanKeyV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq, Default)]
pub struct RuntimeInteractionReceiptRecoveryScanCursorV1 {
    after: Option<RuntimeInteractionReceiptRecoveryScanKeyV1>,
    through: Option<RuntimeInteractionReceiptRecoveryScanKeyV1>,
}

impl RuntimeInteractionReceiptRecoveryScanCursorV1 {
    pub fn new(
        after: Option<RuntimeInteractionReceiptRecoveryScanKeyV1>,
        through: Option<RuntimeInteractionReceiptRecoveryScanKeyV1>,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        if through.is_none() && after.is_some()
            || after
                .as_ref()
                .zip(through.as_ref())
                .is_some_and(|(after, through)| after.cmp_c(through).is_ge())
        {
            return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
        }
        Ok(Self { after, through })
    }

    pub fn after(&self) -> Option<&RuntimeInteractionReceiptRecoveryScanKeyV1> {
        self.after.as_ref()
    }

    pub fn through(&self) -> Option<&RuntimeInteractionReceiptRecoveryScanKeyV1> {
        self.through.as_ref()
    }
}

impl Debug for RuntimeInteractionReceiptRecoveryScanCursorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptRecoveryScanCursorV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionReceiptRecoveryCandidateV1 {
    key: RuntimeInteractionReceiptRecoveryScanKeyV1,
    state: InteractionReceiptStateV1,
    head_revision: u64,
    claim_revision: u64,
    token_expires_at: Option<DateTime<Utc>>,
}

impl RuntimeInteractionReceiptRecoveryCandidateV1 {
    pub fn key(&self) -> &RuntimeInteractionReceiptRecoveryScanKeyV1 {
        &self.key
    }

    pub const fn state(&self) -> InteractionReceiptStateV1 {
        self.state
    }

    pub const fn head_revision(&self) -> u64 {
        self.head_revision
    }

    pub const fn claim_revision(&self) -> u64 {
        self.claim_revision
    }

    pub const fn token_expires_at(&self) -> Option<DateTime<Utc>> {
        self.token_expires_at
    }

    pub(crate) const fn new(
        key: RuntimeInteractionReceiptRecoveryScanKeyV1,
        state: InteractionReceiptStateV1,
        head_revision: u64,
        claim_revision: u64,
        token_expires_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            key,
            state,
            head_revision,
            claim_revision,
            token_expires_at,
        }
    }
}

impl Debug for RuntimeInteractionReceiptRecoveryCandidateV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptRecoveryCandidateV1(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionReceiptRecoveryScanPageV1 {
    candidates: Vec<RuntimeInteractionReceiptRecoveryCandidateV1>,
    through: Option<RuntimeInteractionReceiptRecoveryScanKeyV1>,
    observed_database_now: Option<DateTime<Utc>>,
    requested_limit: NonZeroUsize,
}

impl Debug for RuntimeInteractionReceiptRecoveryScanPageV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptRecoveryScanPageV1(<redacted>)")
    }
}

impl RuntimeInteractionReceiptRecoveryScanPageV1 {
    pub fn candidates(&self) -> &[RuntimeInteractionReceiptRecoveryCandidateV1] {
        &self.candidates
    }

    pub fn through(&self) -> Option<&RuntimeInteractionReceiptRecoveryScanKeyV1> {
        self.through.as_ref()
    }

    pub const fn observed_database_now(&self) -> Option<DateTime<Utc>> {
        self.observed_database_now
    }

    pub fn next_cursor(&self) -> Option<RuntimeInteractionReceiptRecoveryScanCursorV1> {
        let through = self.through.clone()?;
        let after = self
            .candidates
            .last()
            .map(|candidate| candidate.key.clone());
        RuntimeInteractionReceiptRecoveryScanCursorV1::new(after, Some(through)).ok()
    }

    pub fn exhausted(&self) -> bool {
        self.candidates.is_empty()
            || self.candidates.len() < self.requested_limit.get()
            || self.candidates.last().map(|candidate| &candidate.key) == self.through.as_ref()
    }

    pub(crate) fn new(
        candidates: Vec<RuntimeInteractionReceiptRecoveryCandidateV1>,
        through: Option<RuntimeInteractionReceiptRecoveryScanKeyV1>,
        observed_database_now: Option<DateTime<Utc>>,
        requested_limit: NonZeroUsize,
    ) -> Self {
        Self {
            candidates,
            through,
            observed_database_now,
            requested_limit,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionReceiptRecoveryObservationKindV1 {
    Unacknowledged,
    Acknowledged,
    MutationsReconciled,
    ResponseNotObservable,
}

impl RuntimeInteractionReceiptRecoveryObservationKindV1 {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Unacknowledged => "unacknowledged",
            Self::Acknowledged => "acknowledged",
            Self::MutationsReconciled => "mutations_reconciled",
            Self::ResponseNotObservable => "response_not_observable",
        }
    }
}

#[derive(Clone)]
pub struct RuntimeInteractionReceiptRecoveryRequestV1 {
    candidate: RuntimeInteractionReceiptRecoveryCandidateV1,
    expected_route: InteractionExpectedRouteV1,
    observation_kind: RuntimeInteractionReceiptRecoveryObservationKindV1,
    observation_digest: RuntimeInteractionReceiptOpaqueDigestV1,
    claim_lease: RuntimeInteractionReceiptClaimLeaseV1,
}

impl Debug for RuntimeInteractionReceiptRecoveryRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptRecoveryRequestV1(<redacted>)")
    }
}

impl RuntimeInteractionReceiptRecoveryRequestV1 {
    pub fn new(
        candidate: RuntimeInteractionReceiptRecoveryCandidateV1,
        expected_route: InteractionExpectedRouteV1,
        observation_kind: RuntimeInteractionReceiptRecoveryObservationKindV1,
        observation_digest: RuntimeInteractionReceiptOpaqueDigestV1,
        claim_lease: RuntimeInteractionReceiptClaimLeaseV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        if !candidate.state.is_in_flight() {
            return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
        }
        Ok(Self {
            candidate,
            expected_route,
            observation_kind,
            observation_digest,
            claim_lease,
        })
    }

    pub(crate) fn candidate(&self) -> &RuntimeInteractionReceiptRecoveryCandidateV1 {
        &self.candidate
    }

    pub(crate) fn expected_route(&self) -> &InteractionExpectedRouteV1 {
        &self.expected_route
    }

    pub(crate) const fn observation_kind(
        &self,
    ) -> RuntimeInteractionReceiptRecoveryObservationKindV1 {
        self.observation_kind
    }

    pub(crate) const fn observation_digest(&self) -> &RuntimeInteractionReceiptOpaqueDigestV1 {
        &self.observation_digest
    }

    pub(crate) const fn claim_lease(&self) -> RuntimeInteractionReceiptClaimLeaseV1 {
        self.claim_lease
    }
}

pub struct RuntimeInteractionReceiptRecoveredClaimV1 {
    exclusive_claim: RuntimeInteractionReceiptExclusiveClaimV1,
    route: RuntimeInteractionReceiptRouteV1,
    encrypted_token: EncryptedInteractionTokenV1,
    acknowledgement_observed: bool,
}

impl RuntimeInteractionReceiptRecoveredClaimV1 {
    pub fn exclusive_claim(&self) -> &RuntimeInteractionReceiptExclusiveClaimV1 {
        &self.exclusive_claim
    }

    pub fn encrypted_token(&self) -> &EncryptedInteractionTokenV1 {
        &self.encrypted_token
    }

    pub fn route(&self) -> &RuntimeInteractionReceiptRouteV1 {
        &self.route
    }

    pub const fn acknowledgement_observed(&self) -> bool {
        self.acknowledgement_observed
    }

    pub fn into_parts(
        self,
    ) -> (
        RuntimeInteractionReceiptExclusiveClaimV1,
        RuntimeInteractionReceiptRouteV1,
        EncryptedInteractionTokenV1,
    ) {
        (self.exclusive_claim, self.route, self.encrypted_token)
    }

    pub(crate) fn new(
        exclusive_claim: RuntimeInteractionReceiptExclusiveClaimV1,
        route: RuntimeInteractionReceiptRouteV1,
        encrypted_token: EncryptedInteractionTokenV1,
        acknowledgement_observed: bool,
    ) -> Self {
        Self {
            exclusive_claim,
            route,
            encrypted_token,
            acknowledgement_observed,
        }
    }
}

impl Debug for RuntimeInteractionReceiptRecoveredClaimV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptRecoveredClaimV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionReceiptRecoveryRequiredReasonV1 {
    AlreadyRequired,
    ResponseUnrecoverable,
    TokenUnavailable,
    UnsafeToResume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionReceiptRecoveryDeferredReasonV1 {
    SuccessorProcess,
}

pub enum RuntimeInteractionReceiptRecoveryOutcomeV1 {
    Recovered(Box<RuntimeInteractionReceiptRecoveredClaimV1>),
    CompletedDuplicate(RuntimeInteractionReceiptClaimDuplicateV1),
    InFlightDuplicate(RuntimeInteractionReceiptClaimDuplicateV1),
    TerminalDuplicate(RuntimeInteractionReceiptClaimDuplicateV1),
    RecoveryRequired {
        receipt: RuntimeInteractionReceiptClaimDuplicateV1,
        reason: RuntimeInteractionReceiptRecoveryRequiredReasonV1,
    },
    RecoveryDeferred {
        receipt: RuntimeInteractionReceiptClaimDuplicateV1,
        reason: RuntimeInteractionReceiptRecoveryDeferredReasonV1,
    },
}

impl Debug for RuntimeInteractionReceiptRecoveryOutcomeV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Recovered(_) => formatter.write_str("Recovered(<redacted>)"),
            Self::CompletedDuplicate(_) => formatter.write_str("CompletedDuplicate(<redacted>)"),
            Self::InFlightDuplicate(_) => formatter.write_str("InFlightDuplicate(<redacted>)"),
            Self::TerminalDuplicate(_) => formatter.write_str("TerminalDuplicate(<redacted>)"),
            Self::RecoveryRequired { .. } => formatter.write_str("RecoveryRequired(<redacted>)"),
            Self::RecoveryDeferred { .. } => formatter.write_str("RecoveryDeferred(<redacted>)"),
        }
    }
}

#[derive(PartialEq, Eq)]
pub struct RuntimeInteractionReceiptTokenExpiryRequestV1 {
    identity: InteractionReceiptIdentityV1,
    expected_head_revision: u64,
    expected_claim_revision: u64,
    observation_digest: RuntimeInteractionReceiptOpaqueDigestV1,
}

impl RuntimeInteractionReceiptTokenExpiryRequestV1 {
    pub fn new(
        identity: InteractionReceiptIdentityV1,
        expected_head_revision: u64,
        expected_claim_revision: u64,
        observation_digest: RuntimeInteractionReceiptOpaqueDigestV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        if expected_head_revision == 0
            || expected_head_revision >= i64::MAX as u64
            || expected_claim_revision == 0
            || expected_claim_revision > i64::MAX as u64
            || expected_claim_revision > expected_head_revision
        {
            return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
        }
        Ok(Self {
            identity,
            expected_head_revision,
            expected_claim_revision,
            observation_digest,
        })
    }

    pub fn from_recovery_candidate(
        candidate: &RuntimeInteractionReceiptRecoveryCandidateV1,
        observation_digest: RuntimeInteractionReceiptOpaqueDigestV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        Self::new(
            candidate.key().identity(),
            candidate.head_revision(),
            candidate.claim_revision(),
            observation_digest,
        )
    }

    pub(crate) const fn identity(&self) -> InteractionReceiptIdentityV1 {
        self.identity
    }

    pub(crate) const fn expected_head_revision(&self) -> u64 {
        self.expected_head_revision
    }

    pub(crate) const fn expected_claim_revision(&self) -> u64 {
        self.expected_claim_revision
    }

    pub(crate) const fn observation_digest(&self) -> &RuntimeInteractionReceiptOpaqueDigestV1 {
        &self.observation_digest
    }
}

impl Debug for RuntimeInteractionReceiptTokenExpiryRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptTokenExpiryRequestV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionReceiptTokenExpiryDispositionV1 {
    TokenAbsent,
    TokenNotExpired,
    TerminalTokenDeleted,
    RecoveryRequired,
    EffectsCompleted,
    ResponseUnconfirmed,
    ResponseUnrecoverable,
    EffectRecoveryPending,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeInteractionReceiptTokenExpiryOutcomeV1 {
    disposition: RuntimeInteractionReceiptTokenExpiryDispositionV1,
    state: InteractionReceiptStateV1,
    head_revision: u64,
    claim_revision: u64,
    observed_database_now: DateTime<Utc>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimeInteractionReceiptTerminalizeExpiredRequestV1 {
    identity: InteractionReceiptIdentityV1,
    expected_head_revision: u64,
    expected_claim_revision: u64,
    expected_process_instance_id: ProcessInstanceId,
    expected_runtime_build_revision: InteractionRuntimeBuildRevisionV1,
    observation_digest: RuntimeInteractionReceiptOpaqueDigestV1,
}

impl RuntimeInteractionReceiptTerminalizeExpiredRequestV1 {
    pub fn new(
        identity: InteractionReceiptIdentityV1,
        expected_head_revision: u64,
        expected_claim_revision: u64,
        expected_process_instance_id: ProcessInstanceId,
        expected_runtime_build_revision: InteractionRuntimeBuildRevisionV1,
        observation_digest: RuntimeInteractionReceiptOpaqueDigestV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        if expected_head_revision == 0
            || expected_head_revision >= i64::MAX as u64
            || expected_claim_revision == 0
            || expected_claim_revision > i64::MAX as u64
            || expected_claim_revision > expected_head_revision
        {
            return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
        }
        Ok(Self {
            identity,
            expected_head_revision,
            expected_claim_revision,
            expected_process_instance_id,
            expected_runtime_build_revision,
            observation_digest,
        })
    }

    pub fn from_recovery_candidate(
        candidate: &RuntimeInteractionReceiptRecoveryCandidateV1,
        expected_route: &InteractionExpectedRouteV1,
        observation_digest: RuntimeInteractionReceiptOpaqueDigestV1,
    ) -> Result<Self, RuntimeInteractionPersistenceErrorV1> {
        Self::new(
            candidate.key().identity(),
            candidate.head_revision(),
            candidate.claim_revision(),
            expected_route
                .process_identity()
                .process_instance_id
                .clone(),
            expected_route.runtime_build_revision().clone(),
            observation_digest,
        )
    }

    pub(crate) const fn identity(&self) -> InteractionReceiptIdentityV1 {
        self.identity
    }

    pub(crate) const fn expected_head_revision(&self) -> u64 {
        self.expected_head_revision
    }

    pub(crate) const fn expected_claim_revision(&self) -> u64 {
        self.expected_claim_revision
    }

    pub(crate) fn expected_process_instance_id(&self) -> &ProcessInstanceId {
        &self.expected_process_instance_id
    }

    pub(crate) fn expected_runtime_build_revision(&self) -> &InteractionRuntimeBuildRevisionV1 {
        &self.expected_runtime_build_revision
    }

    pub(crate) const fn observation_digest(&self) -> &RuntimeInteractionReceiptOpaqueDigestV1 {
        &self.observation_digest
    }
}

impl Debug for RuntimeInteractionReceiptTerminalizeExpiredRequestV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeInteractionReceiptTerminalizeExpiredRequestV1(<redacted>)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeInteractionReceiptTerminalizeExpiredDispositionV1 {
    RecoveryRequired,
    PristineClaimAbandoned,
    TerminalReceipt,
    ClaimRenewed,
    RevisionRace,
    RouteAuthorityStale,
    EffectsCompleted,
    ResponseUnconfirmed,
    ResponseUnrecoverable,
    EffectRecoveryPending,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeInteractionReceiptTerminalizeExpiredOutcomeV1 {
    disposition: RuntimeInteractionReceiptTerminalizeExpiredDispositionV1,
    state: InteractionReceiptStateV1,
    head_revision: u64,
    claim_revision: u64,
    claim_expires_at: DateTime<Utc>,
    observed_database_now: DateTime<Utc>,
}

impl RuntimeInteractionReceiptTerminalizeExpiredOutcomeV1 {
    pub const fn disposition(&self) -> RuntimeInteractionReceiptTerminalizeExpiredDispositionV1 {
        self.disposition
    }

    pub const fn state(&self) -> InteractionReceiptStateV1 {
        self.state
    }

    pub const fn head_revision(&self) -> u64 {
        self.head_revision
    }

    pub const fn claim_revision(&self) -> u64 {
        self.claim_revision
    }

    pub const fn claim_expires_at(&self) -> DateTime<Utc> {
        self.claim_expires_at
    }

    pub const fn observed_database_now(&self) -> DateTime<Utc> {
        self.observed_database_now
    }

    pub(crate) const fn new(
        disposition: RuntimeInteractionReceiptTerminalizeExpiredDispositionV1,
        state: InteractionReceiptStateV1,
        head_revision: u64,
        claim_revision: u64,
        claim_expires_at: DateTime<Utc>,
        observed_database_now: DateTime<Utc>,
    ) -> Self {
        Self {
            disposition,
            state,
            head_revision,
            claim_revision,
            claim_expires_at,
            observed_database_now,
        }
    }
}

impl RuntimeInteractionReceiptTokenExpiryOutcomeV1 {
    pub const fn disposition(&self) -> RuntimeInteractionReceiptTokenExpiryDispositionV1 {
        self.disposition
    }

    pub const fn state(&self) -> InteractionReceiptStateV1 {
        self.state
    }

    pub const fn head_revision(&self) -> u64 {
        self.head_revision
    }

    pub const fn claim_revision(&self) -> u64 {
        self.claim_revision
    }

    pub const fn observed_database_now(&self) -> DateTime<Utc> {
        self.observed_database_now
    }

    pub(crate) const fn new(
        disposition: RuntimeInteractionReceiptTokenExpiryDispositionV1,
        state: InteractionReceiptStateV1,
        head_revision: u64,
        claim_revision: u64,
        observed_database_now: DateTime<Utc>,
    ) -> Self {
        Self {
            disposition,
            state,
            head_revision,
            claim_revision,
            observed_database_now,
        }
    }
}

pub(crate) fn validate_envelope_authenticated_data(
    claim_root: &InteractionReceiptClaimRootV1,
    envelope: &EncryptedInteractionTokenV1,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    let authenticated_data =
        build_interaction_token_authenticated_data_v1(InteractionTokenAuthenticatedDataInputV1 {
            claim_root,
            encryption_key_id: envelope.encryption_key_id(),
            encryption_suite: envelope.encryption_suite(),
            encryption_suite_version: envelope.encryption_suite_version(),
            time: envelope.time(),
        })
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)?;
    if authenticated_data.digest() != envelope.authenticated_data_digest() {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

pub(crate) fn datetime_from_unix_milliseconds(
    milliseconds: u64,
) -> Result<DateTime<Utc>, RuntimeInteractionPersistenceErrorV1> {
    let milliseconds = i64::try_from(milliseconds)
        .map_err(|_| RuntimeInteractionPersistenceErrorV1::InvalidInput)?;
    Utc.timestamp_millis_opt(milliseconds)
        .single()
        .ok_or(RuntimeInteractionPersistenceErrorV1::InvalidInput)
}

pub(crate) fn unix_milliseconds(
    value: DateTime<Utc>,
) -> Result<u64, RuntimeInteractionPersistenceErrorV1> {
    u64::try_from(value.timestamp_millis())
        .ok()
        .filter(|value| *value > 0)
        .ok_or(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt)
}

pub(crate) fn validate_database_time(
    value: DateTime<Utc>,
    allow_epoch: bool,
) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    let milliseconds = value.timestamp_millis();
    if milliseconds < 0 || (!allow_epoch && milliseconds == 0) {
        return Err(RuntimeInteractionPersistenceErrorV1::PersistenceCorrupt);
    }
    Ok(())
}

fn validate_route_key(value: &str) -> Result<(), RuntimeInteractionPersistenceErrorV1> {
    if value.is_empty()
        || value.len() > 100
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(RuntimeInteractionPersistenceErrorV1::InvalidInput);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use automation_ruleset::{RuleSetContentHash, RuleSetKey, RuleSetVersionId};
    use automation_runtime_convergence::{
        BindingRevision, DeploymentId, InstallationId, RuntimeDeploymentTargetV1,
        RuntimeGeneration, RuntimeProcessIdentityV1, TenantId,
    };
    use automation_runtime_interaction::{
        DiscordApplicationIdV1, DiscordInteractionIdV1, InteractionGatewayShardIdentityV1,
        InteractionProductScopeV1, InteractionRouteIncarnationV1,
    };
    use discord_model::GuildId;
    use resource_resolution::ResourceBindingFingerprint;

    fn identity(application_id: u64, interaction_id: u64) -> InteractionReceiptIdentityV1 {
        InteractionReceiptIdentityV1::new(
            DiscordApplicationIdV1::new(application_id).unwrap(),
            DiscordInteractionIdV1::new(interaction_id).unwrap(),
        )
    }

    fn expected_route() -> InteractionExpectedRouteV1 {
        InteractionExpectedRouteV1::new(
            InteractionProductScopeV1::new(
                TenantId::parse("tenant-debug").unwrap(),
                InstallationId::parse("installation-debug").unwrap(),
                DeploymentId::parse("deployment-debug").unwrap(),
            ),
            RuntimeProcessIdentityV1 {
                target: RuntimeDeploymentTargetV1 {
                    guild_id: GuildId(33),
                    ruleset_key: RuleSetKey::parse("debug").unwrap(),
                    version: RuleSetVersionId::FIRST,
                    content_hash: RuleSetContentHash::parse_hex(&"a".repeat(64)).unwrap(),
                    binding_revision: BindingRevision::new(1).unwrap(),
                    binding_fingerprint: ResourceBindingFingerprint::parse(&"b".repeat(64))
                        .unwrap(),
                },
                runtime_generation: RuntimeGeneration::new(1).unwrap(),
                process_instance_id: ProcessInstanceId::parse("process-debug").unwrap(),
            },
            InteractionGatewayShardIdentityV1::parse("gateway-debug").unwrap(),
            InteractionRuntimeBuildRevisionV1::parse("build-debug").unwrap(),
            automation_runtime_convergence::FencingToken::new(1).unwrap(),
            InteractionRouteIncarnationV1::new(1).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn route_and_claim_lease_inputs_are_bounded() {
        assert!(RuntimeInteractionReceiptRouteV1::static_route("join").is_ok());
        assert!(RuntimeInteractionReceiptRouteV1::static_route(" join").is_err());
        assert!(RuntimeInteractionReceiptRouteV1::static_route("a".repeat(101)).is_err());
        assert!(RuntimeInteractionReceiptClaimLeaseV1::new(Duration::from_secs(1)).is_ok());
        assert!(RuntimeInteractionReceiptClaimLeaseV1::new(Duration::from_secs(300)).is_ok());
        assert!(RuntimeInteractionReceiptClaimLeaseV1::new(Duration::from_millis(999)).is_err());
        assert!(RuntimeInteractionReceiptClaimLeaseV1::new(Duration::from_secs(301)).is_err());
        assert!(
            RuntimeInteractionReceiptClaimLeaseV1::new(Duration::from_nanos(1_000_000_001))
                .is_err()
        );
    }

    #[test]
    fn recovery_cursor_uses_database_text_collation_order() {
        let expires = Utc.timestamp_millis_opt(1_000).single().unwrap();
        let ten =
            RuntimeInteractionReceiptRecoveryScanKeyV1::new(expires, identity(10, 1)).unwrap();
        let two = RuntimeInteractionReceiptRecoveryScanKeyV1::new(expires, identity(2, 1)).unwrap();
        assert!(ten.cmp_c(&two).is_lt());
        assert!(RuntimeInteractionReceiptRecoveryScanCursorV1::new(
            Some(two.clone()),
            Some(ten.clone())
        )
        .is_err());
        assert!(RuntimeInteractionReceiptRecoveryScanCursorV1::new(Some(ten), Some(two)).is_ok());
    }

    #[test]
    fn initial_response_intent_disposition_authorizes_only_new_intents() {
        let applied = RuntimeInteractionReceiptInitialResponseIntentDispositionV1::from_mutation(
            RuntimeInteractionReceiptMutationDispositionV1::Applied,
        );
        let replay = RuntimeInteractionReceiptInitialResponseIntentDispositionV1::from_mutation(
            RuntimeInteractionReceiptMutationDispositionV1::ExactReplay,
        );
        assert!(applied.external_call_authorized());
        assert!(!replay.external_call_authorized());
    }

    #[test]
    fn terminal_outcome_rejects_reserved_exact_replay_code() {
        assert_eq!(
            RuntimeInteractionReceiptTerminalOutcomeV1::new(
                RuntimeInteractionReceiptTerminalStateV1::Completed,
                "exact_replay",
                RuntimeInteractionReceiptOpaqueDigestV1::new([8; 32]),
            ),
            Err(RuntimeInteractionPersistenceErrorV1::InvalidInput)
        );
    }

    #[test]
    fn secret_bearing_debug_outputs_are_redacted() {
        let digest = RuntimeInteractionReceiptOpaqueDigestV1::new([7; 32]);
        let intent = RuntimeInteractionReceiptInitialResponseIntentV1::new(
            RuntimeInteractionReceiptInitialResponseKindV1::OpenModal,
            digest.clone(),
        );
        let terminal = RuntimeInteractionReceiptTerminalOutcomeV1::new(
            RuntimeInteractionReceiptTerminalStateV1::Failed,
            "discord_failure",
            digest,
        )
        .unwrap();
        assert_eq!(
            format!("{intent:?}"),
            "RuntimeInteractionReceiptInitialResponseIntentV1(<redacted>)"
        );
        assert_eq!(
            format!("{terminal:?}"),
            "RuntimeInteractionReceiptTerminalOutcomeV1(<redacted>)"
        );
    }

    #[test]
    fn receipt_identity_debug_outputs_are_redacted() {
        let expires = Utc.timestamp_millis_opt(2_000).single().unwrap();
        let observed = Utc.timestamp_millis_opt(1_000).single().unwrap();
        let receipt_identity = identity(123_456_789, 987_654_321);
        let duplicate = RuntimeInteractionReceiptClaimDuplicateV1::new(
            receipt_identity,
            InteractionReceiptStateV1::Prepared,
            2,
            1,
            expires,
            observed,
        );
        let key =
            RuntimeInteractionReceiptRecoveryScanKeyV1::new(expires, receipt_identity).unwrap();
        let cursor =
            RuntimeInteractionReceiptRecoveryScanCursorV1::new(None, Some(key.clone())).unwrap();
        let candidate = RuntimeInteractionReceiptRecoveryCandidateV1::new(
            key.clone(),
            InteractionReceiptStateV1::Prepared,
            2,
            1,
            Some(expires),
        );
        let page = RuntimeInteractionReceiptRecoveryScanPageV1::new(
            vec![candidate.clone()],
            Some(key.clone()),
            Some(observed),
            NonZeroUsize::new(1).unwrap(),
        );
        let request = RuntimeInteractionReceiptRecoveryRequestV1::new(
            candidate,
            expected_route(),
            RuntimeInteractionReceiptRecoveryObservationKindV1::MutationsReconciled,
            RuntimeInteractionReceiptOpaqueDigestV1::new([9; 32]),
            RuntimeInteractionReceiptClaimLeaseV1::default(),
        )
        .unwrap();
        let outputs = [
            format!("{duplicate:?}"),
            format!(
                "{:?}",
                RuntimeInteractionReceiptClaimOutcomeV1::InFlightDuplicate(duplicate.clone())
            ),
            format!("{key:?}"),
            format!("{cursor:?}"),
            format!("{:?}", request.candidate()),
            format!("{page:?}"),
            format!("{request:?}"),
            format!(
                "{:?}",
                RuntimeInteractionReceiptRecoveryOutcomeV1::RecoveryRequired {
                    receipt: duplicate.clone(),
                    reason: RuntimeInteractionReceiptRecoveryRequiredReasonV1::UnsafeToResume,
                }
            ),
            format!(
                "{:?}",
                RuntimeInteractionReceiptRecoveryOutcomeV1::RecoveryDeferred {
                    receipt: duplicate,
                    reason: RuntimeInteractionReceiptRecoveryDeferredReasonV1::SuccessorProcess,
                }
            ),
        ];
        for output in outputs {
            assert!(output.contains("<redacted>"));
            assert!(!output.contains("123456789"));
            assert!(!output.contains("987654321"));
        }
    }
}
