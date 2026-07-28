use std::fmt::{Debug, Formatter};
use std::future::Future;

use automation_runtime_controller::{
    RuntimeIngressOpenAcknowledgementReceiptV2, RuntimeIngressOpenAcknowledgementV2,
    RuntimeObserveIngressOpenAcknowledgementV2, RuntimeObservedIngressOpenAcknowledgementV2,
    RuntimePublishIngressOpenAcknowledgementOutcomeV2, RuntimePublishIngressOpenAcknowledgementV2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeIngressOpenAcknowledgementAuthorizationErrorV2 {
    #[error("runtime ingress acknowledgement open gate is not the exact successor")]
    OpenGateMismatch,
    #[error("runtime ingress acknowledgement predecessor does not match")]
    PreviousAcknowledgementMismatch,
    #[error("runtime ingress acknowledgement request is invalid")]
    InvalidRequest,
}

pub struct RuntimeAuthorizedIngressOpenAcknowledgementV2 {
    request: RuntimePublishIngressOpenAcknowledgementV2,
}

impl RuntimeAuthorizedIngressOpenAcknowledgementV2 {
    pub(crate) fn from_request(request: RuntimePublishIngressOpenAcknowledgementV2) -> Self {
        Self { request }
    }

    pub fn request(&self) -> &RuntimePublishIngressOpenAcknowledgementV2 {
        &self.request
    }

    pub fn observation_request(&self) -> RuntimeObserveIngressOpenAcknowledgementV2 {
        self.request.observation_request()
    }
}

impl Debug for RuntimeAuthorizedIngressOpenAcknowledgementV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAuthorizedIngressOpenAcknowledgementV2(<redacted>)")
    }
}

pub enum RuntimeIngressOpenAcknowledgementMutationErrorV2<E> {
    DefinitelyNotApplied { source: E },
    OutcomeUnknown { source: E },
}

impl<E> RuntimeIngressOpenAcknowledgementMutationErrorV2<E> {
    pub fn source(&self) -> &E {
        match self {
            Self::DefinitelyNotApplied { source } | Self::OutcomeUnknown { source } => source,
        }
    }

    pub fn outcome_is_unknown(&self) -> bool {
        matches!(self, Self::OutcomeUnknown { .. })
    }
}

impl<E> Debug for RuntimeIngressOpenAcknowledgementMutationErrorV2<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DefinitelyNotApplied { .. } => formatter.write_str(
                "RuntimeIngressOpenAcknowledgementMutationErrorV2::DefinitelyNotApplied(<redacted>)",
            ),
            Self::OutcomeUnknown { .. } => formatter.write_str(
                "RuntimeIngressOpenAcknowledgementMutationErrorV2::OutcomeUnknown(<redacted>)",
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeIngressOpenAcknowledgementObservationErrorClassV2 {
    Retryable,
    AuthorityLost,
    ProtocolViolation,
}

pub trait RuntimeIngressOpenAcknowledgementPortV2 {
    type Error;

    fn classify_observation_error(
        error: &Self::Error,
    ) -> RuntimeIngressOpenAcknowledgementObservationErrorClassV2;

    fn publish_ingress_open_acknowledgement(
        &self,
        authorization: &RuntimeAuthorizedIngressOpenAcknowledgementV2,
    ) -> impl Future<
        Output = Result<
            RuntimePublishIngressOpenAcknowledgementOutcomeV2,
            RuntimeIngressOpenAcknowledgementMutationErrorV2<Self::Error>,
        >,
    > + Send;

    fn observe_ingress_open_acknowledgement(
        &self,
        authorization: &RuntimeAuthorizedIngressOpenAcknowledgementV2,
    ) -> impl Future<Output = Result<RuntimeObservedIngressOpenAcknowledgementV2, Self::Error>> + Send;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeIngressOpenAcknowledgementProtocolViolationV2 {
    ShardMismatch,
    SourceRevisionMismatch,
    RequestDigestMismatch,
    AcknowledgementRevisionMismatch,
    WriterFenceMismatch,
    MaintenanceGateMismatch,
    OwnerMismatch,
    GatewaySnapshotMismatch,
    AcknowledgementBeforeOwnerObservation,
    AcknowledgementLeaseMismatch,
    AcknowledgementExceedsOwnerLease,
    ObservationClockMismatch,
}

pub struct RuntimeAcceptedIngressOpenAcknowledgementV2 {
    receipt: RuntimeIngressOpenAcknowledgementReceiptV2,
}

impl RuntimeAcceptedIngressOpenAcknowledgementV2 {
    pub fn receipt(&self) -> &RuntimeIngressOpenAcknowledgementReceiptV2 {
        &self.receipt
    }

    pub fn acknowledgement(&self) -> &RuntimeIngressOpenAcknowledgementV2 {
        self.receipt.acknowledgement()
    }

    pub fn into_acknowledgement(self) -> RuntimeIngressOpenAcknowledgementV2 {
        self.receipt.into_acknowledgement()
    }
}

impl Debug for RuntimeAcceptedIngressOpenAcknowledgementV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAcceptedIngressOpenAcknowledgementV2(<redacted>)")
    }
}

pub enum RuntimeIngressOpenAcknowledgementResolutionV2 {
    AppliedExact(RuntimeAcceptedIngressOpenAcknowledgementV2),
    ReplayedExact(RuntimeAcceptedIngressOpenAcknowledgementV2),
    AdoptExact(RuntimeAcceptedIngressOpenAcknowledgementV2),
    ReplaySameRequest,
    Stale,
    Divergent,
    ProtocolViolation(RuntimeIngressOpenAcknowledgementProtocolViolationV2),
}

impl RuntimeIngressOpenAcknowledgementResolutionV2 {
    pub fn accepted(&self) -> Option<&RuntimeAcceptedIngressOpenAcknowledgementV2> {
        match self {
            Self::AppliedExact(accepted)
            | Self::ReplayedExact(accepted)
            | Self::AdoptExact(accepted) => Some(accepted),
            Self::ReplaySameRequest
            | Self::Stale
            | Self::Divergent
            | Self::ProtocolViolation(_) => None,
        }
    }
}

impl Debug for RuntimeIngressOpenAcknowledgementResolutionV2 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::AppliedExact(_) => "AppliedExact",
            Self::ReplayedExact(_) => "ReplayedExact",
            Self::AdoptExact(_) => "AdoptExact",
            Self::ReplaySameRequest => "ReplaySameRequest",
            Self::Stale => "Stale",
            Self::Divergent => "Divergent",
            Self::ProtocolViolation(_) => "ProtocolViolation",
        };
        write!(
            formatter,
            "RuntimeIngressOpenAcknowledgementResolutionV2::{name}(<redacted>)"
        )
    }
}

pub fn classify_ingress_open_acknowledgement_outcome_v2(
    authorization: &RuntimeAuthorizedIngressOpenAcknowledgementV2,
    outcome: RuntimePublishIngressOpenAcknowledgementOutcomeV2,
) -> RuntimeIngressOpenAcknowledgementResolutionV2 {
    match outcome {
        RuntimePublishIngressOpenAcknowledgementOutcomeV2::Applied(receipt) => {
            match accept_exact_receipt(authorization.request(), receipt) {
                Ok(accepted) => {
                    RuntimeIngressOpenAcknowledgementResolutionV2::AppliedExact(accepted)
                }
                Err(error) => {
                    RuntimeIngressOpenAcknowledgementResolutionV2::ProtocolViolation(error)
                }
            }
        }
        RuntimePublishIngressOpenAcknowledgementOutcomeV2::Replayed(receipt) => {
            match accept_exact_receipt(authorization.request(), receipt) {
                Ok(accepted) => {
                    RuntimeIngressOpenAcknowledgementResolutionV2::ReplayedExact(accepted)
                }
                Err(error) => {
                    RuntimeIngressOpenAcknowledgementResolutionV2::ProtocolViolation(error)
                }
            }
        }
        RuntimePublishIngressOpenAcknowledgementOutcomeV2::NotCurrent(observation) => {
            classify_unknown_ingress_open_acknowledgement_v2(authorization, observation)
        }
    }
}

pub fn classify_unknown_ingress_open_acknowledgement_v2(
    authorization: &RuntimeAuthorizedIngressOpenAcknowledgementV2,
    observation: RuntimeObservedIngressOpenAcknowledgementV2,
) -> RuntimeIngressOpenAcknowledgementResolutionV2 {
    let request = authorization.request();
    if observation.gateway_shard_id() != &request.owner_receipt().lease_id.gateway_shard_id {
        return RuntimeIngressOpenAcknowledgementResolutionV2::ProtocolViolation(
            RuntimeIngressOpenAcknowledgementProtocolViolationV2::ShardMismatch,
        );
    }
    match observation {
        RuntimeObservedIngressOpenAcknowledgementV2::Missing { .. } => {
            if request.source_acknowledgement_revision().is_none() {
                RuntimeIngressOpenAcknowledgementResolutionV2::ReplaySameRequest
            } else {
                RuntimeIngressOpenAcknowledgementResolutionV2::Stale
            }
        }
        RuntimeObservedIngressOpenAcknowledgementV2::Present(receipt) => {
            classify_present_after_unknown(request, *receipt)
        }
    }
}

fn classify_present_after_unknown(
    request: &RuntimePublishIngressOpenAcknowledgementV2,
    receipt: RuntimeIngressOpenAcknowledgementReceiptV2,
) -> RuntimeIngressOpenAcknowledgementResolutionV2 {
    if receipt.observed_database_now() >= receipt.acknowledgement().expires_at() {
        return RuntimeIngressOpenAcknowledgementResolutionV2::Stale;
    }
    let observed_revision = receipt.acknowledgement().acknowledgement_revision();
    let expected_revision = request
        .source_acknowledgement_revision()
        .and_then(|revision| revision.get().checked_add(1))
        .unwrap_or(1);
    if observed_revision.get() == expected_revision {
        let same_source =
            receipt.source_acknowledgement_revision() == request.source_acknowledgement_revision();
        let same_digest = receipt.request_digest() == request.request_digest();
        let same_scope = same_operation_scope(request, receipt.acknowledgement());
        if same_source && same_scope && !same_digest {
            return RuntimeIngressOpenAcknowledgementResolutionV2::Divergent;
        }
        if same_source && same_digest {
            return match accept_exact_receipt(request, receipt) {
                Ok(accepted) => RuntimeIngressOpenAcknowledgementResolutionV2::AdoptExact(accepted),
                Err(error) => {
                    RuntimeIngressOpenAcknowledgementResolutionV2::ProtocolViolation(error)
                }
            };
        }
        return RuntimeIngressOpenAcknowledgementResolutionV2::Stale;
    }
    if request
        .source_acknowledgement_revision()
        .is_some_and(|source| source == observed_revision)
    {
        return if same_operation_scope(request, receipt.acknowledgement()) {
            RuntimeIngressOpenAcknowledgementResolutionV2::ReplaySameRequest
        } else {
            RuntimeIngressOpenAcknowledgementResolutionV2::Stale
        };
    }
    if observed_revision.get() > expected_revision {
        RuntimeIngressOpenAcknowledgementResolutionV2::Stale
    } else {
        RuntimeIngressOpenAcknowledgementResolutionV2::ProtocolViolation(
            RuntimeIngressOpenAcknowledgementProtocolViolationV2::AcknowledgementRevisionMismatch,
        )
    }
}

fn accept_exact_receipt(
    request: &RuntimePublishIngressOpenAcknowledgementV2,
    receipt: RuntimeIngressOpenAcknowledgementReceiptV2,
) -> Result<
    RuntimeAcceptedIngressOpenAcknowledgementV2,
    RuntimeIngressOpenAcknowledgementProtocolViolationV2,
> {
    if receipt.source_acknowledgement_revision() != request.source_acknowledgement_revision() {
        return Err(RuntimeIngressOpenAcknowledgementProtocolViolationV2::SourceRevisionMismatch);
    }
    if receipt.request_digest() != request.request_digest() {
        return Err(RuntimeIngressOpenAcknowledgementProtocolViolationV2::RequestDigestMismatch);
    }
    let acknowledgement = receipt.acknowledgement();
    let expected_revision = request
        .source_acknowledgement_revision()
        .and_then(|revision| revision.get().checked_add(1))
        .unwrap_or(1);
    if acknowledgement.acknowledgement_revision().get() != expected_revision {
        return Err(
            RuntimeIngressOpenAcknowledgementProtocolViolationV2::AcknowledgementRevisionMismatch,
        );
    }
    if acknowledgement.fence_generation() != request.fence_generation() {
        return Err(RuntimeIngressOpenAcknowledgementProtocolViolationV2::WriterFenceMismatch);
    }
    if acknowledgement.maintenance_gate_generation() != request.maintenance_gate_generation() {
        return Err(RuntimeIngressOpenAcknowledgementProtocolViolationV2::MaintenanceGateMismatch);
    }
    if acknowledgement.gateway_owner_lease_id() != &request.owner_receipt().lease_id
        || acknowledgement.observed_owner_revision() != request.owner_receipt().owner_revision
        || acknowledgement.process_instance_id() != &request.gateway_ready().process_instance_id
    {
        return Err(RuntimeIngressOpenAcknowledgementProtocolViolationV2::OwnerMismatch);
    }
    if acknowledgement.connection_epoch() != request.gateway_ready().connection_epoch
        || acknowledgement.admission_revision() != request.gateway_ready().admission_revision
        || acknowledgement.connected_event_sequence()
            != request.gateway_ready().connected_event_sequence
        || acknowledgement.resume_sequence() != request.gateway_ready().resume_sequence
    {
        return Err(RuntimeIngressOpenAcknowledgementProtocolViolationV2::GatewaySnapshotMismatch);
    }
    if acknowledgement.acknowledged_at() < request.owner_receipt().database_now {
        return Err(
            RuntimeIngressOpenAcknowledgementProtocolViolationV2::AcknowledgementBeforeOwnerObservation,
        );
    }
    let acknowledged_lease = acknowledgement
        .expires_at()
        .signed_duration_since(acknowledgement.acknowledged_at())
        .to_std()
        .map_err(|_| {
            RuntimeIngressOpenAcknowledgementProtocolViolationV2::AcknowledgementLeaseMismatch
        })?;
    if acknowledged_lease.is_zero() || acknowledged_lease > request.lease_for().duration() {
        return Err(
            RuntimeIngressOpenAcknowledgementProtocolViolationV2::AcknowledgementLeaseMismatch,
        );
    }
    if acknowledgement.expires_at() > request.owner_receipt().expires_at {
        return Err(
            RuntimeIngressOpenAcknowledgementProtocolViolationV2::AcknowledgementExceedsOwnerLease,
        );
    }
    if receipt.observed_database_now() < acknowledgement.acknowledged_at()
        || receipt.observed_database_now() >= acknowledgement.expires_at()
    {
        return Err(RuntimeIngressOpenAcknowledgementProtocolViolationV2::ObservationClockMismatch);
    }
    Ok(RuntimeAcceptedIngressOpenAcknowledgementV2 { receipt })
}

fn same_operation_scope(
    request: &RuntimePublishIngressOpenAcknowledgementV2,
    acknowledgement: &RuntimeIngressOpenAcknowledgementV2,
) -> bool {
    acknowledgement.fence_generation() == request.fence_generation()
        && acknowledgement.maintenance_gate_generation() == request.maintenance_gate_generation()
        && acknowledgement.gateway_owner_lease_id() == &request.owner_receipt().lease_id
        && acknowledgement.observed_owner_revision() == request.owner_receipt().owner_revision
        && acknowledgement.process_instance_id() == &request.gateway_ready().process_instance_id
        && acknowledgement.connection_epoch() == request.gateway_ready().connection_epoch
        && acknowledgement.admission_revision() == request.gateway_ready().admission_revision
        && acknowledgement.connected_event_sequence()
            == request.gateway_ready().connected_event_sequence
        && acknowledgement.resume_sequence() == request.gateway_ready().resume_sequence
}
