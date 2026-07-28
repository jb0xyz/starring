use std::fmt::{Debug, Formatter};
use std::num::NonZeroU64;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    ApprovalPayloadDigestV1, AuthenticatedActorV1, AuthenticatedSessionFingerprintV1,
    AuthorizedInstallationScopeV1, ProductControlPortError, ProductDecisionProjectionV1,
    ProductIdempotencyKeyV1, ProductMutationContextV1, ProductRequestIdV1, ProductRevisionV1,
    PromotionSelectorV1,
};

const RUNTIME_ID_HEX_BYTES: usize = 32;
const SHA256_HEX_BYTES: usize = 64;
const CANCELLATION_REASON_MAX_SCALARS: usize = 1_000;
const CANCELLATION_REASON_MAX_BYTES: usize = 4_000;
const DATABASE_COUNTER_MAX: u64 = i64::MAX as u64;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductDrainSelectorError {
    #[error("runtime drain intent ID must be lowercase 128-bit hexadecimal")]
    InvalidDrainIntentId,
    #[error("acknowledged runtime drain intent revision is invalid")]
    InvalidAcknowledgedIntentRevision,
    #[error("acknowledged runtime drain state digest must be lowercase SHA-256 hexadecimal")]
    InvalidAcknowledgedStateDigest,
    #[error("runtime Product operation ID must be lowercase 128-bit hexadecimal")]
    InvalidProductOperationId,
    #[error("runtime drain and Product operation IDs must be distinct")]
    IdentityCollision,
    #[error("expected runtime deployment revision is invalid")]
    InvalidExpectedRuntimeDeploymentRevision,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ProductDrainSelectorV1 {
    drain_intent_id: String,
    acknowledged_intent_revision: NonZeroU64,
    acknowledged_state_digest: String,
    product_operation_id: String,
    expected_runtime_deployment_revision: NonZeroU64,
}

impl ProductDrainSelectorV1 {
    pub fn from_server_projection(
        drain_intent_id: impl Into<String>,
        acknowledged_intent_revision: u64,
        acknowledged_state_digest: impl Into<String>,
        product_operation_id: impl Into<String>,
        expected_runtime_deployment_revision: u64,
    ) -> Result<Self, ProductDrainSelectorError> {
        let drain_intent_id = drain_intent_id.into();
        let acknowledged_state_digest = acknowledged_state_digest.into();
        let product_operation_id = product_operation_id.into();
        if !is_lower_hex(&drain_intent_id, RUNTIME_ID_HEX_BYTES) {
            return Err(ProductDrainSelectorError::InvalidDrainIntentId);
        }
        let acknowledged_intent_revision =
            checked_database_counter(acknowledged_intent_revision)
                .ok_or(ProductDrainSelectorError::InvalidAcknowledgedIntentRevision)?;
        if !is_lower_hex(&acknowledged_state_digest, SHA256_HEX_BYTES) {
            return Err(ProductDrainSelectorError::InvalidAcknowledgedStateDigest);
        }
        if !is_lower_hex(&product_operation_id, RUNTIME_ID_HEX_BYTES) {
            return Err(ProductDrainSelectorError::InvalidProductOperationId);
        }
        if drain_intent_id == product_operation_id {
            return Err(ProductDrainSelectorError::IdentityCollision);
        }
        let expected_runtime_deployment_revision =
            checked_database_counter(expected_runtime_deployment_revision)
                .ok_or(ProductDrainSelectorError::InvalidExpectedRuntimeDeploymentRevision)?;
        Ok(Self {
            drain_intent_id,
            acknowledged_intent_revision,
            acknowledged_state_digest,
            product_operation_id,
            expected_runtime_deployment_revision,
        })
    }

    pub fn drain_intent_id(&self) -> &str {
        &self.drain_intent_id
    }

    pub fn acknowledged_intent_revision(&self) -> NonZeroU64 {
        self.acknowledged_intent_revision
    }

    pub fn acknowledged_state_digest(&self) -> &str {
        &self.acknowledged_state_digest
    }

    pub fn product_operation_id(&self) -> &str {
        &self.product_operation_id
    }

    pub fn expected_runtime_deployment_revision(&self) -> NonZeroU64 {
        self.expected_runtime_deployment_revision
    }
}

impl Debug for ProductDrainSelectorV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductDrainSelectorV1(<opaque>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductLifecycleCancellationReasonError {
    #[error("Product lifecycle cancellation reason must not be empty")]
    Empty,
    #[error(
        "Product lifecycle cancellation reason exceeds {CANCELLATION_REASON_MAX_SCALARS} Unicode scalars or {CANCELLATION_REASON_MAX_BYTES} bytes"
    )]
    TooLong,
    #[error("Product lifecycle cancellation reason contains control characters")]
    ControlCharacter,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProductLifecycleCancellationReasonV1(String);

impl ProductLifecycleCancellationReasonV1 {
    pub fn parse(value: &str) -> Result<Self, ProductLifecycleCancellationReasonError> {
        let value = value.trim();
        if value.is_empty() {
            return Err(ProductLifecycleCancellationReasonError::Empty);
        }
        if value.len() > CANCELLATION_REASON_MAX_BYTES
            || value.chars().count() > CANCELLATION_REASON_MAX_SCALARS
        {
            return Err(ProductLifecycleCancellationReasonError::TooLong);
        }
        if value.chars().any(char::is_control) {
            return Err(ProductLifecycleCancellationReasonError::ControlCharacter);
        }
        Ok(Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for ProductLifecycleCancellationReasonV1 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ProductLifecycleCancellationReasonV1(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CancelProductLifecycleMutationV1 {
    pub promotion: PromotionSelectorV1,
    pub expected_payload_digest: ApprovalPayloadDigestV1,
    pub expected_revision: ProductRevisionV1,
    pub drain_selector: ProductDrainSelectorV1,
    pub idempotency_key: ProductIdempotencyKeyV1,
    pub reason: ProductLifecycleCancellationReasonV1,
}

pub struct AuthorizedCancelProductLifecycleV1<'a, E> {
    context: ProductMutationContextV1<'a, E>,
    command: CancelProductLifecycleMutationV1,
}

impl<'a, E> AuthorizedCancelProductLifecycleV1<'a, E> {
    pub(crate) fn new(
        request_id: &'a ProductRequestIdV1,
        actor: &'a AuthenticatedActorV1,
        scope: &'a AuthorizedInstallationScopeV1,
        evidence: &'a E,
        command: CancelProductLifecycleMutationV1,
    ) -> Self {
        Self {
            context: ProductMutationContextV1::new(request_id, actor, scope, evidence),
            command,
        }
    }

    pub fn context(&self) -> &ProductMutationContextV1<'a, E> {
        &self.context
    }

    pub fn request_id(&self) -> &ProductRequestIdV1 {
        self.context.request_id()
    }

    pub fn actor(&self) -> &AuthenticatedActorV1 {
        self.context.actor()
    }

    pub fn session_fingerprint(&self) -> &AuthenticatedSessionFingerprintV1 {
        self.context.session_fingerprint()
    }

    pub fn scope(&self) -> &AuthorizedInstallationScopeV1 {
        self.context.scope()
    }

    pub fn evidence(&self) -> &E {
        self.context.evidence()
    }

    pub fn command(&self) -> &CancelProductLifecycleMutationV1 {
        &self.command
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProductLifecycleCancellationReceiptError {
    #[error("resulting runtime deployment revision is invalid")]
    InvalidResultingRuntimeDeploymentRevision,
    #[error("terminal runtime drain intent revision is invalid")]
    InvalidTerminalIntentRevision,
    #[error("terminal runtime drain state digest must be lowercase SHA-256 hexadecimal")]
    InvalidTerminalStateDigest,
    #[error("source slot writer epoch is invalid")]
    InvalidSourceSlotWriterEpoch,
    #[error("successor slot writer epoch is invalid")]
    InvalidSuccessorSlotWriterEpoch,
    #[error("Product lifecycle cancellation timestamp is not canonical")]
    InvalidCancellationTime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductLifecycleCancellationDeploymentProjectionV1 {
    resulting_runtime_deployment_revision: NonZeroU64,
}

impl ProductLifecycleCancellationDeploymentProjectionV1 {
    pub fn from_server_projection(
        resulting_runtime_deployment_revision: u64,
    ) -> Result<Self, ProductLifecycleCancellationReceiptError> {
        let resulting_runtime_deployment_revision =
            checked_database_counter(resulting_runtime_deployment_revision).ok_or(
                ProductLifecycleCancellationReceiptError::InvalidResultingRuntimeDeploymentRevision,
            )?;
        Ok(Self {
            resulting_runtime_deployment_revision,
        })
    }

    pub fn resulting_runtime_deployment_revision(&self) -> NonZeroU64 {
        self.resulting_runtime_deployment_revision
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductLifecycleCancellationDrainProjectionV1 {
    source_selector: ProductDrainSelectorV1,
    terminal_intent_revision: NonZeroU64,
    terminal_state_digest: String,
}

impl ProductLifecycleCancellationDrainProjectionV1 {
    pub fn from_server_projection(
        source_selector: ProductDrainSelectorV1,
        terminal_intent_revision: u64,
        terminal_state_digest: impl Into<String>,
    ) -> Result<Self, ProductLifecycleCancellationReceiptError> {
        let terminal_intent_revision = checked_database_counter(terminal_intent_revision)
            .ok_or(ProductLifecycleCancellationReceiptError::InvalidTerminalIntentRevision)?;
        let terminal_state_digest = terminal_state_digest.into();
        if !is_lower_hex(&terminal_state_digest, SHA256_HEX_BYTES) {
            return Err(ProductLifecycleCancellationReceiptError::InvalidTerminalStateDigest);
        }
        Ok(Self {
            source_selector,
            terminal_intent_revision,
            terminal_state_digest,
        })
    }

    pub fn source_selector(&self) -> &ProductDrainSelectorV1 {
        &self.source_selector
    }

    pub fn terminal_intent_revision(&self) -> NonZeroU64 {
        self.terminal_intent_revision
    }

    pub fn terminal_state_digest(&self) -> &str {
        &self.terminal_state_digest
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductLifecycleCancellationSlotProjectionV1 {
    source_slot_writer_epoch: NonZeroU64,
    successor_slot_writer_epoch: NonZeroU64,
}

impl ProductLifecycleCancellationSlotProjectionV1 {
    pub fn from_server_projection(
        source_slot_writer_epoch: u64,
        successor_slot_writer_epoch: u64,
    ) -> Result<Self, ProductLifecycleCancellationReceiptError> {
        let source_slot_writer_epoch = checked_database_counter(source_slot_writer_epoch)
            .ok_or(ProductLifecycleCancellationReceiptError::InvalidSourceSlotWriterEpoch)?;
        let successor_slot_writer_epoch = checked_database_counter(successor_slot_writer_epoch)
            .ok_or(ProductLifecycleCancellationReceiptError::InvalidSuccessorSlotWriterEpoch)?;
        Ok(Self {
            source_slot_writer_epoch,
            successor_slot_writer_epoch,
        })
    }

    pub fn source_slot_writer_epoch(&self) -> NonZeroU64 {
        self.source_slot_writer_epoch
    }

    pub fn successor_slot_writer_epoch(&self) -> NonZeroU64 {
        self.successor_slot_writer_epoch
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductLifecycleCancellationReceiptV1 {
    decision: ProductDecisionProjectionV1,
    deployment: ProductLifecycleCancellationDeploymentProjectionV1,
    drain: ProductLifecycleCancellationDrainProjectionV1,
    slot: ProductLifecycleCancellationSlotProjectionV1,
    cancelled_at: SystemTime,
    exact_replay: bool,
}

impl ProductLifecycleCancellationReceiptV1 {
    pub fn from_server_projection(
        decision: ProductDecisionProjectionV1,
        deployment: ProductLifecycleCancellationDeploymentProjectionV1,
        drain: ProductLifecycleCancellationDrainProjectionV1,
        slot: ProductLifecycleCancellationSlotProjectionV1,
        cancelled_at: SystemTime,
        exact_replay: bool,
    ) -> Result<Self, ProductLifecycleCancellationReceiptError> {
        let cancellation_duration = cancelled_at
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ProductLifecycleCancellationReceiptError::InvalidCancellationTime)?;
        if cancellation_duration.subsec_nanos() % 1_000 != 0 {
            return Err(ProductLifecycleCancellationReceiptError::InvalidCancellationTime);
        }
        Ok(Self {
            decision,
            deployment,
            drain,
            slot,
            cancelled_at,
            exact_replay,
        })
    }

    pub fn decision(&self) -> &ProductDecisionProjectionV1 {
        &self.decision
    }

    pub fn source_drain_selector(&self) -> &ProductDrainSelectorV1 {
        self.drain.source_selector()
    }

    pub fn resulting_runtime_deployment_revision(&self) -> NonZeroU64 {
        self.deployment.resulting_runtime_deployment_revision()
    }

    pub fn terminal_intent_revision(&self) -> NonZeroU64 {
        self.drain.terminal_intent_revision()
    }

    pub fn terminal_state_digest(&self) -> &str {
        self.drain.terminal_state_digest()
    }

    pub fn source_slot_writer_epoch(&self) -> NonZeroU64 {
        self.slot.source_slot_writer_epoch()
    }

    pub fn successor_slot_writer_epoch(&self) -> NonZeroU64 {
        self.slot.successor_slot_writer_epoch()
    }

    pub fn cancelled_at(&self) -> SystemTime {
        self.cancelled_at
    }

    pub fn exact_replay(&self) -> bool {
        self.exact_replay
    }
}

#[allow(async_fn_in_trait)]
pub trait ProductLifecycleCancellationPort<E> {
    async fn cancel_lifecycle_idempotent(
        &self,
        request: AuthorizedCancelProductLifecycleV1<'_, E>,
    ) -> Result<ProductLifecycleCancellationReceiptV1, ProductControlPortError>;
}

fn checked_database_counter(value: u64) -> Option<NonZeroU64> {
    NonZeroU64::new(value).filter(|value| value.get() <= DATABASE_COUNTER_MAX)
}

fn is_lower_hex(value: &str, expected_bytes: usize) -> bool {
    value.len() == expected_bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
