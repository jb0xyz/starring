use super::*;

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimePendingDrainTerminalIdentityV4 {
    pub(super) action_identity: RuntimePendingDrainActionIdentityV4,
    pub(super) source_intent_revision: NonZeroU64,
    pub(super) source_state_digest: RuntimeDrainCanonicalStateDigestV3,
}

impl RuntimePendingDrainTerminalIdentityV4 {
    pub(crate) fn new(
        action_identity: RuntimePendingDrainActionIdentityV4,
        source_intent_revision: NonZeroU64,
        source_state_digest: RuntimeDrainCanonicalStateDigestV3,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        validate_persistence_value(source_intent_revision)?;
        Ok(Self {
            action_identity,
            source_intent_revision,
            source_state_digest,
        })
    }
}

impl Debug for RuntimePendingDrainTerminalIdentityV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainTerminalIdentityV4(<redacted>)")
    }
}

pub enum RuntimeRoutedClaimMutationStageV4 {}

pub enum RuntimeRefenceProgressMutationStageV4 {}

pub enum RuntimeSameProcessAcknowledgementMutationStageV4 {}

pub enum RuntimePreviousProcessTeardownMutationStageV4 {}

pub struct RuntimePendingDrainUnknownResultV4<A, S> {
    pub(super) authorization: A,
    pub(super) terminal_identity: RuntimePendingDrainTerminalIdentityV4,
    pub(super) stage: PhantomData<fn() -> S>,
}

impl<A, S> RuntimePendingDrainUnknownResultV4<A, S> {
    pub(crate) fn new(
        authorization: A,
        terminal_identity: RuntimePendingDrainTerminalIdentityV4,
    ) -> Self {
        Self {
            authorization,
            terminal_identity,
            stage: PhantomData,
        }
    }

    pub fn terminal_identity(&self) -> &RuntimePendingDrainTerminalIdentityV4 {
        &self.terminal_identity
    }

    pub(crate) fn accept_observation(
        self,
        observation: RuntimePendingDrainTerminalObservationV4,
    ) -> Result<RuntimePendingDrainUnknownResolutionV4<A, S>, RuntimePendingDrainV4Error> {
        if observation.terminal_identity != self.terminal_identity {
            return Err(RuntimePendingDrainV4Error::TerminalIdentityMismatch);
        }
        match observation.outcome {
            RuntimePendingDrainTerminalObservationOutcomeV4::Committed(receipt) => {
                validate_terminal_receipt(&self.terminal_identity, &receipt)?;
                Ok(RuntimePendingDrainUnknownResolutionV4::Committed(
                    RuntimePendingDrainCommittedStageV4 {
                        authorization: self.authorization,
                        receipt,
                        stage: PhantomData,
                    },
                ))
            }
            RuntimePendingDrainTerminalObservationOutcomeV4::NotCommitted => Ok(
                RuntimePendingDrainUnknownResolutionV4::Replay(RuntimePendingDrainOneReplayV4 {
                    authorization: self.authorization,
                    terminal_identity: self.terminal_identity,
                    stage: PhantomData,
                }),
            ),
            RuntimePendingDrainTerminalObservationOutcomeV4::Unknown => {
                Ok(RuntimePendingDrainUnknownResolutionV4::Closed(
                    RuntimePendingDrainTerminalUnknownV4 {
                        terminal_identity: self.terminal_identity,
                    },
                ))
            }
        }
    }

    #[allow(clippy::manual_async_fn)]
    pub fn observe<'a, P>(
        self,
        port: &'a P,
        operation_cutoff: Instant,
    ) -> impl Future<
        Output = Result<
            RuntimePendingDrainUnknownResolutionV4<A, S>,
            RuntimePendingDrainBoundaryErrorV4<P::Error>,
        >,
    > + Send
           + 'a
    where
        P: RuntimePendingDrainTerminalObservationPortV4 + Sync + 'a,
        A: Send + 'a,
        S: Send + 'a,
    {
        async move {
            let observation = port
                .observe_pending_drain_terminal_v4(&self.terminal_identity, operation_cutoff)
                .await
                .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
            if observation.terminal_identity != self.terminal_identity {
                return Err(RuntimePendingDrainBoundaryErrorV4::Contract(
                    RuntimePendingDrainV4Error::TerminalIdentityMismatch,
                ));
            }
            let outcome = match observation.outcome {
                RuntimePendingDrainTerminalPortOutcomeV4::Committed(receipt) => {
                    RuntimePendingDrainTerminalObservationOutcomeV4::Committed(Box::new(
                        (*receipt)
                            .into_receipt()
                            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?,
                    ))
                }
                RuntimePendingDrainTerminalPortOutcomeV4::NotCommitted => {
                    RuntimePendingDrainTerminalObservationOutcomeV4::NotCommitted
                }
                RuntimePendingDrainTerminalPortOutcomeV4::Unknown => {
                    RuntimePendingDrainTerminalObservationOutcomeV4::Unknown
                }
            };
            let observation = RuntimePendingDrainTerminalObservationV4::new(
                observation.terminal_identity,
                outcome,
            );
            self.accept_observation(observation)
                .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)
        }
    }
}

impl<A, S> Debug for RuntimePendingDrainUnknownResultV4<A, S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainUnknownResultV4(<redacted>)")
    }
}

pub struct RuntimePendingDrainCommittedStageV4<A, S> {
    pub(super) authorization: A,
    pub(super) receipt: Box<RuntimePendingDrainMutationReceiptV4>,
    pub(super) stage: PhantomData<fn() -> S>,
}

impl<A, S> RuntimePendingDrainCommittedStageV4<A, S> {
    pub fn authorization(&self) -> &A {
        &self.authorization
    }

    pub fn receipt(&self) -> &RuntimePendingDrainMutationReceiptV4 {
        &self.receipt
    }
}

impl<A, S> Debug for RuntimePendingDrainCommittedStageV4<A, S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainCommittedStageV4(<redacted>)")
    }
}

pub struct RuntimePendingDrainDeterminateStageV4<A, S> {
    pub(super) authorization: A,
    pub(super) stage: PhantomData<fn() -> S>,
}

impl<A, S> RuntimePendingDrainDeterminateStageV4<A, S> {
    pub fn authorization(&self) -> &A {
        &self.authorization
    }

    pub fn into_authorization(self) -> A {
        self.authorization
    }
}

impl<A, S> Debug for RuntimePendingDrainDeterminateStageV4<A, S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainDeterminateStageV4(<redacted>)")
    }
}

pub enum RuntimePendingDrainUnknownResolutionV4<A, S> {
    Committed(RuntimePendingDrainCommittedStageV4<A, S>),
    Replay(RuntimePendingDrainOneReplayV4<A, S>),
    Closed(RuntimePendingDrainTerminalUnknownV4),
}

impl<A, S> Debug for RuntimePendingDrainUnknownResolutionV4<A, S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainUnknownResolutionV4(<redacted>)")
    }
}

pub struct RuntimePendingDrainOneReplayV4<A, S> {
    pub(super) authorization: A,
    pub(super) terminal_identity: RuntimePendingDrainTerminalIdentityV4,
    pub(super) stage: PhantomData<fn() -> S>,
}

impl<A, S> RuntimePendingDrainOneReplayV4<A, S> {
    pub fn authorization(&self) -> &A {
        &self.authorization
    }

    pub fn terminal_identity(&self) -> &RuntimePendingDrainTerminalIdentityV4 {
        &self.terminal_identity
    }

    pub(crate) fn accept_replay(
        self,
        result: RuntimePendingDrainReplayResultV4,
    ) -> Result<RuntimePendingDrainReplayResolutionV4<A, S>, RuntimePendingDrainV4Error> {
        match result {
            RuntimePendingDrainReplayResultV4::Committed(receipt) => {
                let receipt = Box::new((*receipt).into_receipt()?);
                validate_terminal_receipt(&self.terminal_identity, &receipt)?;
                Ok(RuntimePendingDrainReplayResolutionV4::Committed(
                    RuntimePendingDrainCommittedStageV4 {
                        authorization: self.authorization,
                        receipt,
                        stage: PhantomData,
                    },
                ))
            }
            RuntimePendingDrainReplayResultV4::DeterminateNotCommitted => Ok(
                RuntimePendingDrainReplayResolutionV4::DeterminateNotCommitted(
                    RuntimePendingDrainDeterminateStageV4 {
                        authorization: self.authorization,
                        stage: PhantomData,
                    },
                ),
            ),
            RuntimePendingDrainReplayResultV4::Unknown => {
                Ok(RuntimePendingDrainReplayResolutionV4::Closed(
                    RuntimePendingDrainTerminalUnknownV4 {
                        terminal_identity: self.terminal_identity,
                    },
                ))
            }
        }
    }

    #[allow(clippy::manual_async_fn)]
    pub fn replay<'a, P>(
        self,
        port: &'a P,
        operation_cutoff: Instant,
    ) -> impl Future<
        Output = Result<
            RuntimePendingDrainReplayResolutionV4<A, S>,
            RuntimePendingDrainBoundaryErrorV4<P::Error>,
        >,
    > + Send
           + 'a
    where
        P: RuntimePendingDrainReplayPortV4<A, S> + Sync + 'a,
        A: Send + 'a,
        S: Send + 'a,
    {
        async move {
            let result = port
                .replay_pending_drain_v4(&self, operation_cutoff)
                .await
                .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
            self.accept_replay(result)
                .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)
        }
    }
}

impl<A, S> Debug for RuntimePendingDrainOneReplayV4<A, S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainOneReplayV4(<redacted>)")
    }
}

pub enum RuntimePendingDrainReplayResultV4 {
    Committed(Box<RuntimePendingDrainMutationPortReceiptV4>),
    DeterminateNotCommitted,
    Unknown,
}

pub trait RuntimePendingDrainReplayPortV4<A, S> {
    type Error;

    fn replay_pending_drain_v4(
        &self,
        authorization: &RuntimePendingDrainOneReplayV4<A, S>,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimePendingDrainReplayResultV4, Self::Error>> + Send;
}

pub enum RuntimePendingDrainReplayResolutionV4<A, S> {
    Committed(RuntimePendingDrainCommittedStageV4<A, S>),
    DeterminateNotCommitted(RuntimePendingDrainDeterminateStageV4<A, S>),
    Closed(RuntimePendingDrainTerminalUnknownV4),
}

pub struct RuntimePendingDrainTerminalUnknownV4 {
    pub(super) terminal_identity: RuntimePendingDrainTerminalIdentityV4,
}

impl RuntimePendingDrainTerminalUnknownV4 {
    pub fn terminal_identity(&self) -> &RuntimePendingDrainTerminalIdentityV4 {
        &self.terminal_identity
    }
}

impl Debug for RuntimePendingDrainTerminalUnknownV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainTerminalUnknownV4(<redacted>)")
    }
}

pub(crate) enum RuntimePendingDrainTerminalObservationOutcomeV4 {
    Committed(Box<RuntimePendingDrainMutationReceiptV4>),
    NotCommitted,
    Unknown,
}

pub(crate) struct RuntimePendingDrainTerminalObservationV4 {
    pub(super) terminal_identity: RuntimePendingDrainTerminalIdentityV4,
    pub(super) outcome: RuntimePendingDrainTerminalObservationOutcomeV4,
}

impl RuntimePendingDrainTerminalObservationV4 {
    pub(crate) fn new(
        terminal_identity: RuntimePendingDrainTerminalIdentityV4,
        outcome: RuntimePendingDrainTerminalObservationOutcomeV4,
    ) -> Self {
        Self {
            terminal_identity,
            outcome,
        }
    }
}

pub enum RuntimePendingDrainTerminalPortOutcomeV4 {
    Committed(Box<RuntimePendingDrainMutationPortReceiptV4>),
    NotCommitted,
    Unknown,
}

pub struct RuntimePendingDrainTerminalPortObservationV4 {
    pub terminal_identity: RuntimePendingDrainTerminalIdentityV4,
    pub outcome: RuntimePendingDrainTerminalPortOutcomeV4,
}

impl Debug for RuntimePendingDrainTerminalPortObservationV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainTerminalPortObservationV4(<redacted>)")
    }
}

pub trait RuntimePendingDrainTerminalObservationPortV4 {
    type Error;

    fn observe_pending_drain_terminal_v4(
        &self,
        identity: &RuntimePendingDrainTerminalIdentityV4,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimePendingDrainTerminalPortObservationV4, Self::Error>> + Send;
}

#[derive(Clone, PartialEq, Eq)]
pub struct RuntimePendingDrainFinalizerIdentityV4 {
    pub(super) process_instance_id: ProcessInstanceId,
    pub(super) intent_id: RuntimeDrainIntentIdV2,
    pub(super) source_intent_revision: NonZeroU64,
    pub(super) source_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub(super) owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub(super) owner_revision: NonZeroU64,
    pub(super) action_identity: RuntimePendingDrainActionIdentityV4,
    pub(super) seal_generation: NonZeroU64,
    pub(super) route_incarnation: NonZeroU64,
    pub(super) controller_fence: FencingToken,
    pub(super) registry_lifetime_digest: RuntimePendingDrainEvidenceDigestV4,
}

impl RuntimePendingDrainFinalizerIdentityV4 {
    pub(super) fn for_claim(authorization: &RuntimeAuthorizedRoutedDrainClaimV4) -> Self {
        Self {
            process_instance_id: authorization.seal.process_instance_id.clone(),
            intent_id: authorization.candidate.intent_id().clone(),
            source_intent_revision: authorization.candidate.source_intent_revision(),
            source_state_digest: authorization.candidate.source_state_digest().clone(),
            owner_lease_id: authorization.candidate.current_owner().lease_id.clone(),
            owner_revision: authorization.candidate.current_owner().owner_revision,
            action_identity: authorization.action_identity.clone(),
            seal_generation: authorization.seal.seal_generation,
            route_incarnation: authorization.seal.route.route_incarnation,
            controller_fence: authorization
                .seal
                .route
                .controller_fencing_token
                .next()
                .expect("checked claim fence"),
            registry_lifetime_digest: authorization.seal.registry_lifetime_digest,
        }
    }

    pub(super) fn for_refence(authorization: &RuntimeAuthorizedDrainRefenceProgressV4) -> Self {
        let (
            intent_id,
            source_intent_revision,
            source_state_digest,
            owner_lease_id,
            owner_revision,
        ) = match &authorization.source {
            RuntimeRefenceAuthorizationSourceV4::Current(candidate) => (
                candidate.intent_id().clone(),
                candidate.source_intent_revision(),
                candidate.source_state_digest().clone(),
                candidate.current_owner().lease_id.clone(),
                candidate.current_owner().owner_revision,
            ),
            RuntimeRefenceAuthorizationSourceV4::Applied(applied) => (
                applied.source_common.intent_id().clone(),
                applied
                    .receipt
                    .result
                    .canonical()
                    .intent()
                    .intent_revision(),
                RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                    applied.receipt.result.canonical().state_bytes(),
                ),
                applied.source_common.current_owner.lease_id.clone(),
                applied.source_common.current_owner.owner_revision,
            ),
        };
        Self {
            process_instance_id: authorization
                .seal
                .claimed
                .routed_seal
                .process_instance_id
                .clone(),
            intent_id,
            source_intent_revision,
            source_state_digest,
            owner_lease_id,
            owner_revision,
            action_identity: authorization.action_identity.clone(),
            seal_generation: authorization.seal.claimed.routed_seal.seal_generation,
            route_incarnation: authorization
                .seal
                .claimed
                .routed_seal
                .route
                .route_incarnation,
            controller_fence: authorization.seal.claimed.claim_fence,
            registry_lifetime_digest: authorization
                .seal
                .claimed
                .routed_seal
                .registry_lifetime_digest,
        }
    }

    pub(super) fn for_acknowledgement(
        authorization: &RuntimeAuthorizedSameProcessDrainAcknowledgementV4,
    ) -> Self {
        let common = authorization.source.common();
        let canonical = authorization.source.canonical();
        Self {
            process_instance_id: authorization.route_absent.process_instance_id.clone(),
            intent_id: common.intent_id().clone(),
            source_intent_revision: canonical.intent().intent_revision(),
            source_state_digest: RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                canonical.state_bytes(),
            ),
            owner_lease_id: common.current_owner.lease_id.clone(),
            owner_revision: common.current_owner.owner_revision,
            action_identity: authorization.action_identity.clone(),
            seal_generation: authorization.route_absent.seal_generation,
            route_incarnation: authorization.route_absent.removed_route.route_incarnation,
            controller_fence: authorization
                .route_absent
                .removed_route
                .controller_fencing_token,
            registry_lifetime_digest: authorization.route_absent.registry_lifetime_digest,
        }
    }

    pub(super) fn for_teardown(
        authorization: &RuntimeAuthorizedPreviousProcessDrainTeardownV4,
    ) -> Self {
        let common = authorization.source.common();
        Self {
            process_instance_id: authorization.seal.process_instance_id.clone(),
            intent_id: common.intent_id().clone(),
            source_intent_revision: common.canonical().intent().intent_revision(),
            source_state_digest: common.source_state_digest.clone(),
            owner_lease_id: common.current_owner.lease_id.clone(),
            owner_revision: common.current_owner.owner_revision,
            action_identity: authorization.action_identity.clone(),
            seal_generation: authorization.seal.seal_generation,
            route_incarnation: authorization.seal.predecessor_route.route_incarnation,
            controller_fence: authorization.seal.successor_fence,
            registry_lifetime_digest: authorization.seal.registry_lifetime_digest,
        }
    }

    pub fn process_instance_id(&self) -> &ProcessInstanceId {
        &self.process_instance_id
    }

    pub fn intent_id(&self) -> &RuntimeDrainIntentIdV2 {
        &self.intent_id
    }

    pub fn source_intent_revision(&self) -> NonZeroU64 {
        self.source_intent_revision
    }

    pub fn source_state_digest(&self) -> &RuntimeDrainCanonicalStateDigestV3 {
        &self.source_state_digest
    }

    pub fn owner_lease_id(&self) -> &RuntimeGatewayOwnerLeaseIdV1 {
        &self.owner_lease_id
    }

    pub fn owner_revision(&self) -> NonZeroU64 {
        self.owner_revision
    }

    pub fn action_identity(&self) -> &RuntimePendingDrainActionIdentityV4 {
        &self.action_identity
    }

    pub fn seal_generation(&self) -> NonZeroU64 {
        self.seal_generation
    }

    pub fn route_incarnation(&self) -> NonZeroU64 {
        self.route_incarnation
    }

    pub fn controller_fence(&self) -> FencingToken {
        self.controller_fence
    }

    pub fn registry_lifetime_digest(&self) -> RuntimePendingDrainEvidenceDigestV4 {
        self.registry_lifetime_digest
    }
}

impl Debug for RuntimePendingDrainFinalizerIdentityV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainFinalizerIdentityV4(<redacted>)")
    }
}

pub struct RuntimePendingDrainFinalizerRegistrationV4<A> {
    pub(super) identity: RuntimePendingDrainFinalizerIdentityV4,
    pub(super) authorization: A,
}

impl<A> RuntimePendingDrainFinalizerRegistrationV4<A> {
    pub(super) fn new(identity: RuntimePendingDrainFinalizerIdentityV4, authorization: A) -> Self {
        Self {
            identity,
            authorization,
        }
    }

    pub fn identity(&self) -> &RuntimePendingDrainFinalizerIdentityV4 {
        &self.identity
    }
}

impl<A: Send> RuntimePendingDrainFinalizerRegistrationV4<A> {
    #[allow(clippy::manual_async_fn)]
    pub fn register<'a, P>(
        self,
        port: &'a P,
    ) -> impl Future<Output = Result<RuntimeRegisteredPendingDrainFinalizerV4<A>, P::Error>> + Send + 'a
    where
        P: RuntimePendingDrainFinalizerPortV4 + Sync + 'a,
        A: 'a,
    {
        async move {
            let registration = port.register(self).await?;
            Ok(RuntimeRegisteredPendingDrainFinalizerV4 {
                identity: registration.identity,
                authorization: registration.authorization,
            })
        }
    }
}

impl<A> Debug for RuntimePendingDrainFinalizerRegistrationV4<A> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainFinalizerRegistrationV4(<redacted>)")
    }
}

pub struct RuntimeRegisteredPendingDrainFinalizerV4<A> {
    pub(super) identity: RuntimePendingDrainFinalizerIdentityV4,
    pub(super) authorization: A,
}

impl<A> RuntimeRegisteredPendingDrainFinalizerV4<A> {
    pub(crate) fn from_parts(
        identity: RuntimePendingDrainFinalizerIdentityV4,
        authorization: A,
    ) -> Self {
        Self {
            identity,
            authorization,
        }
    }

    pub(crate) fn into_parts(self) -> (RuntimePendingDrainFinalizerIdentityV4, A) {
        (self.identity, self.authorization)
    }

    pub fn identity(&self) -> &RuntimePendingDrainFinalizerIdentityV4 {
        &self.identity
    }

    pub fn authorization(&self) -> &A {
        &self.authorization
    }

    pub fn into_authorization(self) -> A {
        self.authorization
    }

    pub fn into_join(self) -> RuntimePendingDrainFinalizerJoinV4<A> {
        RuntimePendingDrainFinalizerJoinV4 {
            identity: self.identity,
            authorization: self.authorization,
        }
    }
}

impl<A> Debug for RuntimeRegisteredPendingDrainFinalizerV4<A> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRegisteredPendingDrainFinalizerV4(<redacted>)")
    }
}

pub struct RuntimePendingDrainFinalizerJoinV4<A> {
    pub(super) identity: RuntimePendingDrainFinalizerIdentityV4,
    pub(super) authorization: A,
}

impl<A> RuntimePendingDrainFinalizerJoinV4<A> {
    pub fn identity(&self) -> &RuntimePendingDrainFinalizerIdentityV4 {
        &self.identity
    }
}

pub struct RuntimePendingDrainFinalizerTransferV4<A> {
    pub(super) identity: RuntimePendingDrainFinalizerIdentityV4,
    pub(super) authorization: A,
    pub(super) shutdown_generation: NonZeroU64,
}

impl<A> RuntimePendingDrainFinalizerTransferV4<A> {
    pub fn new(
        join: RuntimePendingDrainFinalizerJoinV4<A>,
        shutdown_generation: NonZeroU64,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        validate_persistence_value(shutdown_generation)?;
        Ok(Self {
            identity: join.identity,
            authorization: join.authorization,
            shutdown_generation,
        })
    }

    pub fn identity(&self) -> &RuntimePendingDrainFinalizerIdentityV4 {
        &self.identity
    }

    pub fn authorization(&self) -> &A {
        &self.authorization
    }

    pub fn shutdown_generation(&self) -> NonZeroU64 {
        self.shutdown_generation
    }
}

pub trait RuntimePendingDrainFinalizerPortV4 {
    type Error;

    fn register<A: Send>(
        &self,
        registration: RuntimePendingDrainFinalizerRegistrationV4<A>,
    ) -> impl Future<Output = Result<RuntimePendingDrainFinalizerRegistrationV4<A>, Self::Error>> + Send;

    fn join<A: Send>(
        &self,
        join: RuntimePendingDrainFinalizerJoinV4<A>,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimePendingDrainFinalizerJoinV4<A>, Self::Error>> + Send;

    fn transfer<A: Send>(
        &self,
        transfer: RuntimePendingDrainFinalizerTransferV4<A>,
    ) -> impl Future<Output = Result<RuntimePendingDrainFinalizerTransferV4<A>, Self::Error>> + Send;
}
