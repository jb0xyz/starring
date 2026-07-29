use super::*;

pub enum RuntimePendingDrainBoundaryErrorV4<E> {
    Port(E),
    Contract(RuntimePendingDrainV4Error),
}

impl<E: Debug> Debug for RuntimePendingDrainBoundaryErrorV4<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Port(error) => formatter
                .debug_tuple("RuntimePendingDrainBoundaryErrorV4::Port")
                .field(error)
                .finish(),
            Self::Contract(error) => formatter
                .debug_tuple("RuntimePendingDrainBoundaryErrorV4::Contract")
                .field(error)
                .finish(),
        }
    }
}

pub struct RuntimeRoutedSealPortObservationV4 {
    pub registry_lifetime_digest: RuntimePendingDrainEvidenceDigestV4,
    pub process_instance_id: ProcessInstanceId,
    pub intent_id: RuntimeDrainIntentIdV2,
    pub slot: RuntimeServingSlotV2,
    pub seal_key: [u8; 16],
    pub seal_generation: NonZeroU64,
    pub admission_generation: NonZeroU64,
    pub route: RuntimeExactLocalRouteIdentityV2,
    pub slot_observation_sequence: NonZeroU64,
    pub registry_observation_sequence: NonZeroU64,
    pub active_guards: u64,
}

impl RuntimeRoutedSealPortObservationV4 {
    fn into_witness(self) -> Result<RuntimeRoutedSealedWitnessV4, RuntimePendingDrainV4Error> {
        RuntimeRoutedSealedWitnessV4::new(RuntimeRoutedSealedWitnessInputV4 {
            registry_lifetime_digest: self.registry_lifetime_digest,
            process_instance_id: self.process_instance_id,
            intent_id: self.intent_id,
            slot: self.slot,
            seal_key: self.seal_key,
            seal_generation: self.seal_generation,
            admission_generation: self.admission_generation,
            route: self.route,
            slot_observation_sequence: self.slot_observation_sequence,
            registry_observation_sequence: self.registry_observation_sequence,
            active_guards: self.active_guards,
        })
    }
}

impl Debug for RuntimeRoutedSealPortObservationV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutedSealPortObservationV4(<redacted>)")
    }
}

pub struct RuntimeRoutedClaimedSealPortObservationV4 {
    pub routed: RuntimeRoutedSealPortObservationV4,
    pub claim_fence: FencingToken,
    pub claim_receipt_digest: RuntimePendingDrainEvidenceDigestV4,
}

impl RuntimeRoutedClaimedSealPortObservationV4 {
    fn into_witness(
        self,
    ) -> Result<RuntimeRoutedClaimedSealedWitnessV4, RuntimePendingDrainV4Error> {
        RuntimeRoutedClaimedSealedWitnessV4::new(RuntimeRoutedClaimedSealedWitnessInputV4 {
            routed_seal: self.routed.into_witness()?,
            claim_fence: self.claim_fence,
            claim_receipt_digest: self.claim_receipt_digest,
        })
    }
}

impl Debug for RuntimeRoutedClaimedSealPortObservationV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutedClaimedSealPortObservationV4(<redacted>)")
    }
}

pub struct RuntimeLocalRefencePortObservationV4 {
    pub claimed: RuntimeRoutedClaimedSealPortObservationV4,
    pub old_route: RuntimeExactLocalRouteIdentityV2,
    pub removal_target: RuntimeExactLocalRouteIdentityV2,
    pub provenance: RuntimeRouteMutationProvenanceV2,
    pub registry_observation_sequence: NonZeroU64,
    pub refenced_at: DateTime<Utc>,
    pub active_guards: u64,
}

impl RuntimeLocalRefencePortObservationV4 {
    fn into_witness(
        self,
    ) -> Result<RuntimeLocallyRefencedSealedWitnessV4, RuntimePendingDrainV4Error> {
        RuntimeLocallyRefencedSealedWitnessV4::new(RuntimeLocallyRefencedSealedWitnessInputV4 {
            claimed: self.claimed.into_witness()?,
            old_route: self.old_route,
            removal_target: self.removal_target,
            provenance: self.provenance,
            registry_observation_sequence: self.registry_observation_sequence,
            refenced_at: self.refenced_at,
            active_guards: self.active_guards,
        })
    }
}

impl Debug for RuntimeLocalRefencePortObservationV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeLocalRefencePortObservationV4(<redacted>)")
    }
}

pub struct RuntimeDurableRefencePortObservationV4 {
    pub local: RuntimeLocalRefencePortObservationV4,
    pub refence_receipt_digest: RuntimePendingDrainEvidenceDigestV4,
}

impl RuntimeDurableRefencePortObservationV4 {
    fn into_witness(
        self,
    ) -> Result<RuntimeDurablyRefencedSealedWitnessV4, RuntimePendingDrainV4Error> {
        Ok(RuntimeDurablyRefencedSealedWitnessV4::new(
            RuntimeDurablyRefencedSealedWitnessInputV4 {
                locally_refenced: self.local.into_witness()?,
                refence_receipt_digest: self.refence_receipt_digest,
            },
        ))
    }
}

impl Debug for RuntimeDurableRefencePortObservationV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDurableRefencePortObservationV4(<redacted>)")
    }
}

pub struct RuntimeRouteAbsentPortObservationV4 {
    pub registry_lifetime_digest: RuntimePendingDrainEvidenceDigestV4,
    pub process_instance_id: ProcessInstanceId,
    pub intent_id: RuntimeDrainIntentIdV2,
    pub slot: RuntimeServingSlotV2,
    pub seal_key: [u8; 16],
    pub seal_generation: NonZeroU64,
    pub admission_generation: NonZeroU64,
    pub source_route: RuntimeExactLocalRouteIdentityV2,
    pub removed_route: RuntimeExactLocalRouteIdentityV2,
    pub claim_receipt_digest: RuntimePendingDrainEvidenceDigestV4,
    pub refence_receipt_digest: RuntimePendingDrainEvidenceDigestV4,
    pub slot_observation_sequence: NonZeroU64,
    pub registry_observation_sequence: NonZeroU64,
    pub active_guards: u64,
}

impl RuntimeRouteAbsentPortObservationV4 {
    fn into_witness(self) -> Result<RuntimeRouteAbsentSealedWitnessV4, RuntimePendingDrainV4Error> {
        RuntimeRouteAbsentSealedWitnessV4::new(RuntimeRouteAbsentSealedWitnessInputV4 {
            registry_lifetime_digest: self.registry_lifetime_digest,
            process_instance_id: self.process_instance_id,
            intent_id: self.intent_id,
            slot: self.slot,
            seal_key: self.seal_key,
            seal_generation: self.seal_generation,
            admission_generation: self.admission_generation,
            source_route: self.source_route,
            removed_route: self.removed_route,
            claim_receipt_digest: self.claim_receipt_digest,
            refence_receipt_digest: self.refence_receipt_digest,
            slot_observation_sequence: self.slot_observation_sequence,
            registry_observation_sequence: self.registry_observation_sequence,
            active_guards: self.active_guards,
        })
    }
}

impl Debug for RuntimeRouteAbsentPortObservationV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRouteAbsentPortObservationV4(<redacted>)")
    }
}

pub struct RuntimeEmptySuccessionPortObservationV4 {
    pub registry_lifetime_digest: RuntimePendingDrainEvidenceDigestV4,
    pub process_instance_id: ProcessInstanceId,
    pub successor_identity: RuntimeProcessIdentityV1,
    pub intent_id: RuntimeDrainIntentIdV2,
    pub slot: RuntimeServingSlotV2,
    pub seal_key: [u8; 16],
    pub seal_generation: NonZeroU64,
    pub admission_generation: NonZeroU64,
    pub predecessor_route: RuntimeExactLocalRouteIdentityV2,
    pub possible_route_fence_ceiling: FencingToken,
    pub successor_fence: FencingToken,
    pub slot_observation_sequence: NonZeroU64,
    pub registry_observation_sequence: NonZeroU64,
    pub active_guards: u64,
}

impl RuntimeEmptySuccessionPortObservationV4 {
    fn into_witness(
        self,
    ) -> Result<RuntimeEmptySuccessionSealedWitnessV4, RuntimePendingDrainV4Error> {
        RuntimeEmptySuccessionSealedWitnessV4::new(RuntimeEmptySuccessionSealedWitnessInputV4 {
            registry_lifetime_digest: self.registry_lifetime_digest,
            process_instance_id: self.process_instance_id,
            successor_identity: self.successor_identity,
            intent_id: self.intent_id,
            slot: self.slot,
            seal_key: self.seal_key,
            seal_generation: self.seal_generation,
            admission_generation: self.admission_generation,
            predecessor_route: self.predecessor_route,
            possible_route_fence_ceiling: self.possible_route_fence_ceiling,
            successor_fence: self.successor_fence,
            slot_observation_sequence: self.slot_observation_sequence,
            registry_observation_sequence: self.registry_observation_sequence,
            active_guards: self.active_guards,
        })
    }
}

impl Debug for RuntimeEmptySuccessionPortObservationV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeEmptySuccessionPortObservationV4(<redacted>)")
    }
}

pub trait RuntimePendingDrainRegistryTransitionPortV4 {
    type Error;
    type RoutedObserved;
    type RoutedSealed;
    type RoutedClaimedSealed;
    type LocallyRefencedSealed;
    type DurablyRefencedSealed;
    type DrainingRefencedSealed;
    type RouteAbsentSealed;
    type EmptySuccessionSealed;
    type AcknowledgedEmpty;

    fn seal_routed(
        &self,
        source: Self::RoutedObserved,
        authorization: &RuntimeSelectedUnclaimedPendingDrainV4,
    ) -> Result<(Self::RoutedSealed, RuntimeRoutedSealPortObservationV4), Self::Error>;

    fn recover_routed_claimed(
        &self,
        authorization: &RuntimeSelectedCurrentRoutedClaimedV4,
    ) -> Result<
        (
            Self::RoutedClaimedSealed,
            RuntimeRoutedClaimedSealPortObservationV4,
        ),
        Self::Error,
    >;

    fn bind_claim(
        &self,
        source: Self::RoutedSealed,
        receipt: &RuntimeDurableRoutedClaimReceiptV4,
    ) -> Result<
        (
            Self::RoutedClaimedSealed,
            RuntimeRoutedClaimedSealPortObservationV4,
        ),
        Self::Error,
    >;

    fn refence<J: Send, S: Send, C: Send>(
        &self,
        source: Self::RoutedClaimedSealed,
        authorization: &RuntimeAuthorizedRegistryRefenceEvidenceV4<J, S, C>,
    ) -> Result<
        (
            Self::LocallyRefencedSealed,
            RuntimeLocalRefencePortObservationV4,
        ),
        Self::Error,
    >;

    fn bind_refence(
        &self,
        source: Self::LocallyRefencedSealed,
        receipt: &RuntimeDurableRefenceReceiptV4,
    ) -> Result<
        (
            Self::DurablyRefencedSealed,
            RuntimeDurableRefencePortObservationV4,
        ),
        Self::Error,
    >;

    fn recover_durable_refence(
        &self,
        authorization: &RuntimeSelectedCurrentRefencedV4,
    ) -> Result<
        (
            Self::DurablyRefencedSealed,
            RuntimeDurableRefencePortObservationV4,
        ),
        Self::Error,
    >;

    fn begin_drain(
        &self,
        source: Self::DurablyRefencedSealed,
    ) -> Result<Self::DrainingRefencedSealed, Self::Error>;

    fn remove(
        &self,
        source: Self::DrainingRefencedSealed,
    ) -> Result<(Self::RouteAbsentSealed, RuntimeRouteAbsentPortObservationV4), Self::Error>;

    fn recover_route_absent(
        &self,
        authorization: &RuntimeSelectedCurrentRefencedV4,
    ) -> Result<(Self::RouteAbsentSealed, RuntimeRouteAbsentPortObservationV4), Self::Error>;

    fn seal_empty_succession(
        &self,
        authorization: &RuntimeSelectedExpiredPreviousOwnerV4,
    ) -> Result<
        (
            Self::EmptySuccessionSealed,
            RuntimeEmptySuccessionPortObservationV4,
        ),
        Self::Error,
    >;

    fn consume_acknowledgement(
        &self,
        source: Self::RouteAbsentSealed,
        receipt: &RuntimeDurableSameProcessDrainAcknowledgementV4,
    ) -> Result<Self::AcknowledgedEmpty, Self::Error>;

    fn consume_succession_acknowledgement(
        &self,
        source: Self::EmptySuccessionSealed,
        receipt: &RuntimeDurablePreviousProcessDrainTeardownV4,
    ) -> Result<Self::AcknowledgedEmpty, Self::Error>;
}

pub struct RuntimeRoutedSealedClaimV4<R> {
    authorization: RuntimeAuthorizedRoutedDrainClaimV4,
    registry_state: R,
}

impl<R> RuntimeRoutedSealedClaimV4<R> {
    pub fn candidate(&self) -> &RuntimeUnclaimedPendingDrainCandidateV4 {
        self.authorization.candidate()
    }

    pub fn action_identity(&self) -> &RuntimePendingDrainActionIdentityV4 {
        self.authorization.action_identity()
    }

    pub fn into_finalizer_registration(self) -> RuntimePendingDrainFinalizerRegistrationV4<Self> {
        let identity = RuntimePendingDrainFinalizerIdentityV4::for_claim(&self.authorization);
        RuntimePendingDrainFinalizerRegistrationV4::new(identity, self)
    }
}

impl<R> Debug for RuntimeRoutedSealedClaimV4<R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutedSealedClaimV4(<redacted>)")
    }
}

impl RuntimeSelectedUnclaimedPendingDrainV4 {
    pub fn seal_routed<P>(
        self,
        port: &P,
        source: P::RoutedObserved,
    ) -> Result<
        RuntimeRoutedSealedClaimV4<P::RoutedSealed>,
        RuntimePendingDrainBoundaryErrorV4<P::Error>,
    >
    where
        P: RuntimePendingDrainRegistryTransitionPortV4,
    {
        let (registry_state, observation) = port
            .seal_routed(source, &self)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
        let witness = observation
            .into_witness()
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let authorization = self
            .bind_routed_seal(witness)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        Ok(RuntimeRoutedSealedClaimV4 {
            authorization,
            registry_state,
        })
    }
}

pub trait RuntimePreviousProcessTeardownEvidencePortV4 {
    type Error;
    type Boundary: Send;

    fn resolve_previous_process_teardown_v4(
        &self,
        authorization: &RuntimeSelectedExpiredPreviousOwnerV4,
        operation_cutoff: Instant,
    ) -> impl Future<
        Output = Result<(Self::Boundary, RuntimeDrainCertificationResolutionV2), Self::Error>,
    > + Send;
}

pub struct RuntimePreparedPreviousProcessTeardownV4<E> {
    selection: RuntimeSelectedExpiredPreviousOwnerV4,
    evidence_boundary: E,
    certification: RuntimeDrainCertificationResolutionV2,
}

impl<E> Debug for RuntimePreparedPreviousProcessTeardownV4<E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePreparedPreviousProcessTeardownV4(<redacted>)")
    }
}

impl RuntimeSelectedExpiredPreviousOwnerV4 {
    #[allow(clippy::manual_async_fn)]
    pub fn prepare_previous_process_teardown<'a, P>(
        self,
        port: &'a P,
        operation_cutoff: Instant,
    ) -> impl Future<
        Output = Result<
            RuntimePreparedPreviousProcessTeardownV4<P::Boundary>,
            RuntimePendingDrainBoundaryErrorV4<P::Error>,
        >,
    > + Send
           + 'a
    where
        P: RuntimePreviousProcessTeardownEvidencePortV4 + Sync + 'a,
    {
        async move {
            if matches!(self, Self::RouteAbsentClaimed { .. }) {
                return Err(RuntimePendingDrainBoundaryErrorV4::Contract(
                    RuntimePendingDrainV4Error::LegacyRouteAbsentHandoffRequired,
                ));
            }
            let (evidence_boundary, certification) = port
                .resolve_previous_process_teardown_v4(&self, operation_cutoff)
                .await
                .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
            Ok(RuntimePreparedPreviousProcessTeardownV4 {
                selection: self,
                evidence_boundary,
                certification,
            })
        }
    }
}

pub struct RuntimePreviousProcessTeardownV4<R, E> {
    authorization: RuntimeAuthorizedPreviousProcessDrainTeardownV4,
    registry_state: R,
    evidence_boundary: E,
}

impl<R, E> RuntimePreviousProcessTeardownV4<R, E> {
    pub fn action_identity(&self) -> &RuntimePendingDrainActionIdentityV4 {
        self.authorization.action_identity()
    }

    pub fn evidence_boundary(&self) -> &E {
        &self.evidence_boundary
    }
}

impl<R, E> Debug for RuntimePreviousProcessTeardownV4<R, E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePreviousProcessTeardownV4(<redacted>)")
    }
}

pub type RuntimePreviousProcessTeardownRegistrationV4<R, E> =
    RuntimePendingDrainFinalizerRegistrationV4<RuntimePreviousProcessTeardownV4<R, E>>;

impl<E> RuntimePreparedPreviousProcessTeardownV4<E> {
    pub fn seal_empty_succession<P>(
        self,
        port: &P,
    ) -> Result<
        RuntimePreviousProcessTeardownRegistrationV4<P::EmptySuccessionSealed, E>,
        RuntimePendingDrainBoundaryErrorV4<P::Error>,
    >
    where
        P: RuntimePendingDrainRegistryTransitionPortV4,
    {
        let (registry_state, observation) = port
            .seal_empty_succession(&self.selection)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
        let witness = observation
            .into_witness()
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let authorization = self
            .selection
            .bind_empty_succession_seal(witness, self.certification)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let identity = RuntimePendingDrainFinalizerIdentityV4::for_teardown(&authorization);
        Ok(RuntimePendingDrainFinalizerRegistrationV4::new(
            identity,
            RuntimePreviousProcessTeardownV4 {
                authorization,
                registry_state,
                evidence_boundary: self.evidence_boundary,
            },
        ))
    }
}

pub struct RuntimePendingDrainMutationPortReceiptV4 {
    pub action_identity: RuntimePendingDrainActionIdentityV4,
    pub source_intent_revision: NonZeroU64,
    pub source_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub result_intent_revision: NonZeroU64,
    pub result_state_bytes: Box<[u8]>,
    pub result_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub owner_receipt: RuntimeGatewayOwnerLeaseReceiptV1,
    pub terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    pub committed_at: DateTime<Utc>,
}

impl RuntimePendingDrainMutationPortReceiptV4 {
    pub(super) fn into_receipt(
        self,
    ) -> Result<RuntimePendingDrainMutationReceiptV4, RuntimePendingDrainV4Error> {
        RuntimePendingDrainMutationReceiptV4::new(RuntimePendingDrainMutationReceiptInputV4 {
            action_identity: self.action_identity,
            source_intent_revision: self.source_intent_revision,
            source_state_digest: self.source_state_digest,
            result_intent_revision: self.result_intent_revision,
            result_state_bytes: self.result_state_bytes,
            result_state_digest: self.result_state_digest,
            owner_receipt: self.owner_receipt,
            terminal_digest: self.terminal_digest,
            committed_at: self.committed_at,
        })
    }
}

impl Debug for RuntimePendingDrainMutationPortReceiptV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainMutationPortReceiptV4(<redacted>)")
    }
}

pub struct RuntimeRoutedDrainDeterminateNonCommitPortObservationV4 {
    pub action_identity: RuntimePendingDrainActionIdentityV4,
    pub source: RuntimePersistedUnclaimedPendingDrainIntentV2,
    pub source_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub owner: RuntimeGatewayOwnerLeaseReceiptV1,
    pub registry_lifetime_digest: RuntimePendingDrainEvidenceDigestV4,
    pub seal_generation: NonZeroU64,
    pub route: RuntimeExactLocalRouteIdentityV2,
    pub slot_observation_sequence: NonZeroU64,
    pub registry_observation_sequence: NonZeroU64,
    pub observation_digest: RuntimePendingDrainEvidenceDigestV4,
    pub observed_at: DateTime<Utc>,
}

impl RuntimeRoutedDrainDeterminateNonCommitPortObservationV4 {
    fn into_observation(
        self,
    ) -> Result<RuntimeRoutedDrainDeterminateNonCommitObservationV4, RuntimePendingDrainV4Error>
    {
        RuntimeRoutedDrainDeterminateNonCommitObservationV4::new(
            RuntimeRoutedDrainDeterminateNonCommitObservationInputV4 {
                action_identity: self.action_identity,
                source: self.source,
                source_state_digest: self.source_state_digest,
                owner: self.owner,
                registry_lifetime_digest: self.registry_lifetime_digest,
                seal_generation: self.seal_generation,
                route: self.route,
                slot_observation_sequence: self.slot_observation_sequence,
                registry_observation_sequence: self.registry_observation_sequence,
                observation_digest: self.observation_digest,
                observed_at: self.observed_at,
            },
        )
    }
}

impl Debug for RuntimeRoutedDrainDeterminateNonCommitPortObservationV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutedDrainDeterminateNonCommitPortObservationV4(<redacted>)")
    }
}

pub enum RuntimeRoutedDrainClaimPortOutcomeV4 {
    Committed {
        mutation: RuntimePendingDrainMutationPortReceiptV4,
        result: RuntimePersistedRoutedClaimedPendingDrainIntentV2,
    },
    DeterminateNotCommitted(RuntimeRoutedDrainDeterminateNonCommitPortObservationV4),
    Unknown,
}

impl Debug for RuntimeRoutedDrainClaimPortOutcomeV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutedDrainClaimPortOutcomeV4(<redacted>)")
    }
}

pub trait RuntimeRoutedDrainClaimExecutionPortV4 {
    type Error;

    fn execute_routed_drain_claim_v4<R: Send>(
        &self,
        authorization: &RuntimeRegisteredPendingDrainFinalizerV4<RuntimeRoutedSealedClaimV4<R>>,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimeRoutedDrainClaimPortOutcomeV4, Self::Error>> + Send;
}

pub struct RuntimeDurableRoutedClaimBoundaryV4<R> {
    durable: RuntimeDurableRoutedClaimReceiptV4,
    registry_state: R,
}

impl<R> RuntimeDurableRoutedClaimBoundaryV4<R> {
    pub fn durable_receipt(&self) -> &RuntimeDurableRoutedClaimReceiptV4 {
        &self.durable
    }
}

impl<R> Debug for RuntimeDurableRoutedClaimBoundaryV4<R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDurableRoutedClaimBoundaryV4(<redacted>)")
    }
}

impl<R> RuntimeRegisteredPendingDrainFinalizerV4<RuntimeDurableRoutedClaimBoundaryV4<R>> {
    pub fn bind_registry_claim<P>(
        self,
        port: &P,
    ) -> Result<
        RuntimePendingDrainFinalizerRegistrationV4<
            RuntimeRoutedClaimedContinuationV4<P::RoutedClaimedSealed>,
        >,
        RuntimePendingDrainBoundaryErrorV4<P::Error>,
    >
    where
        P: RuntimePendingDrainRegistryTransitionPortV4<RoutedSealed = R>,
    {
        let (_, boundary) = self.into_parts();
        let RuntimeDurableRoutedClaimBoundaryV4 {
            durable,
            registry_state,
        } = boundary;
        let (claimed_state, observation) = port
            .bind_claim(registry_state, &durable)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
        let witness = observation
            .into_witness()
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let durable = durable
            .bind_registry_claim(witness)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let action_identity = RuntimePendingDrainActionIdentityV4::successor(
            durable.authorization.request().action_identity(),
            RuntimePendingDrainActionStageV4::RefenceProgress,
            2,
        )
        .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let continuation = RuntimeRoutedClaimedContinuationV4 {
            authorization: durable.authorization,
            source: RuntimeRoutedClaimedContinuationSourceV4::Applied(Box::new(
                RuntimeAppliedRoutedClaimSourceV4 {
                    source_common: durable.source_common,
                    receipt: durable.claim_receipt,
                },
            )),
            claimed: durable.witness,
            action_identity,
            registry_state: claimed_state,
        };
        Ok(continuation.into_finalizer_registration())
    }
}

pub struct RuntimeRoutedDrainRollbackAuthorizationV4<R> {
    permit: RuntimeRoutedDrainRollbackPermitV4,
    registry_state: R,
}

impl<R> RuntimeRoutedDrainRollbackAuthorizationV4<R> {
    pub fn permit(&self) -> &RuntimeRoutedDrainRollbackPermitV4 {
        &self.permit
    }

    pub fn rollback<P>(
        self,
        port: &P,
    ) -> Result<P::Unsealed, RuntimePendingDrainBoundaryErrorV4<P::Error>>
    where
        P: RuntimeRoutedDrainRollbackPortV4<RoutedSealed = R>,
    {
        port.rollback_routed_seal_v4(self.registry_state, self.permit)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Port)
    }
}

impl<R> Debug for RuntimeRoutedDrainRollbackAuthorizationV4<R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutedDrainRollbackAuthorizationV4(<redacted>)")
    }
}

pub trait RuntimeRoutedDrainRollbackPortV4 {
    type Error;
    type RoutedSealed;
    type Unsealed;

    fn rollback_routed_seal_v4(
        &self,
        source: Self::RoutedSealed,
        permit: RuntimeRoutedDrainRollbackPermitV4,
    ) -> Result<Self::Unsealed, Self::Error>;
}

pub enum RuntimeRoutedDrainClaimExecutionResolutionV4<R> {
    Committed(
        Box<RuntimeRegisteredPendingDrainFinalizerV4<RuntimeDurableRoutedClaimBoundaryV4<R>>>,
    ),
    DeterminateNotCommitted(Box<RuntimeRoutedDrainRollbackAuthorizationV4<R>>),
    Unknown(
        Box<
            RuntimePendingDrainUnknownResultV4<
                RuntimeRegisteredPendingDrainFinalizerV4<RuntimeRoutedSealedClaimV4<R>>,
                RuntimeRoutedClaimMutationStageV4,
            >,
        >,
    ),
}

impl<R> Debug for RuntimeRoutedDrainClaimExecutionResolutionV4<R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutedDrainClaimExecutionResolutionV4(<redacted>)")
    }
}

impl<R: Send> RuntimeRegisteredPendingDrainFinalizerV4<RuntimeRoutedSealedClaimV4<R>> {
    #[allow(clippy::manual_async_fn)]
    pub fn execute_routed_claim<'a, P>(
        self,
        port: &'a P,
        operation_cutoff: Instant,
    ) -> impl Future<
        Output = Result<
            RuntimeRoutedDrainClaimExecutionResolutionV4<R>,
            RuntimePendingDrainBoundaryErrorV4<P::Error>,
        >,
    > + Send
           + 'a
    where
        P: RuntimeRoutedDrainClaimExecutionPortV4 + Sync + 'a,
        R: 'a,
    {
        async move {
            let outcome = port
                .execute_routed_drain_claim_v4(&self, operation_cutoff)
                .await
                .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
            match outcome {
                RuntimeRoutedDrainClaimPortOutcomeV4::Committed { mutation, result } => {
                    let receipt = RuntimeRoutedDrainClaimReceiptV4::new(
                        mutation
                            .into_receipt()
                            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?,
                        result,
                    )
                    .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
                    let (identity, sealed) = self.into_parts();
                    let RuntimeRoutedSealedClaimV4 {
                        authorization,
                        registry_state,
                    } = sealed;
                    let durable = authorization
                        .accept_durable_receipt(receipt)
                        .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
                    Ok(RuntimeRoutedDrainClaimExecutionResolutionV4::Committed(
                        Box::new(RuntimeRegisteredPendingDrainFinalizerV4::from_parts(
                            identity,
                            RuntimeDurableRoutedClaimBoundaryV4 {
                                durable,
                                registry_state,
                            },
                        )),
                    ))
                }
                RuntimeRoutedDrainClaimPortOutcomeV4::DeterminateNotCommitted(observation) => {
                    let (_, sealed) = self.into_parts();
                    let RuntimeRoutedSealedClaimV4 {
                        authorization,
                        registry_state,
                    } = sealed;
                    let permit = authorization
                        .authorize_determinate_non_commit_rollback(
                            observation
                                .into_observation()
                                .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?,
                        )
                        .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
                    Ok(
                        RuntimeRoutedDrainClaimExecutionResolutionV4::DeterminateNotCommitted(
                            Box::new(RuntimeRoutedDrainRollbackAuthorizationV4 {
                                permit,
                                registry_state,
                            }),
                        ),
                    )
                }
                RuntimeRoutedDrainClaimPortOutcomeV4::Unknown => {
                    let identity = terminal_identity_for_registered_claim(&self)
                        .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
                    Ok(RuntimeRoutedDrainClaimExecutionResolutionV4::Unknown(
                        Box::new(RuntimePendingDrainUnknownResultV4::new(self, identity)),
                    ))
                }
            }
        }
    }
}

fn terminal_identity_for_registered_claim<R>(
    registered: &RuntimeRegisteredPendingDrainFinalizerV4<RuntimeRoutedSealedClaimV4<R>>,
) -> Result<RuntimePendingDrainTerminalIdentityV4, RuntimePendingDrainV4Error> {
    let authorization = &registered.authorization().authorization;
    RuntimePendingDrainTerminalIdentityV4::new(
        authorization.action_identity.clone(),
        authorization.candidate.source_intent_revision(),
        authorization.candidate.source_state_digest().clone(),
    )
}

enum RuntimeRoutedClaimedContinuationSourceV4 {
    Current(Box<RuntimeRoutedClaimedPendingDrainCandidateV4>),
    Applied(Box<RuntimeAppliedRoutedClaimSourceV4>),
}

impl RuntimeRoutedClaimedContinuationSourceV4 {
    fn common(&self) -> &RuntimePendingDrainCandidateCommonV4 {
        match self {
            Self::Current(candidate) => &candidate.common,
            Self::Applied(applied) => &applied.source_common,
        }
    }

    fn claim(&self) -> &RuntimeDrainClaimV2 {
        match self {
            Self::Current(candidate) => candidate.claim(),
            Self::Applied(applied) => applied
                .receipt
                .result
                .canonical()
                .intent()
                .state()
                .pending_claim()
                .expect("checked routed claim receipt"),
        }
    }

    fn into_refence_source(self) -> RuntimeRefenceAuthorizationSourceV4 {
        match self {
            Self::Current(candidate) => RuntimeRefenceAuthorizationSourceV4::Current(candidate),
            Self::Applied(applied) => RuntimeRefenceAuthorizationSourceV4::Applied(applied),
        }
    }
}

pub struct RuntimeRoutedClaimedContinuationV4<R> {
    authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    source: RuntimeRoutedClaimedContinuationSourceV4,
    claimed: RuntimeRoutedClaimedSealedWitnessV4,
    action_identity: RuntimePendingDrainActionIdentityV4,
    registry_state: R,
}

impl<R> RuntimeRoutedClaimedContinuationV4<R> {
    pub fn candidate_intent_id(&self) -> &RuntimeDrainIntentIdV2 {
        self.source.common().intent_id()
    }

    pub fn action_identity(&self) -> &RuntimePendingDrainActionIdentityV4 {
        &self.action_identity
    }

    pub fn into_finalizer_registration(self) -> RuntimePendingDrainFinalizerRegistrationV4<Self> {
        let identity = self.finalizer_identity();
        RuntimePendingDrainFinalizerRegistrationV4::new(identity, self)
    }

    fn finalizer_identity(&self) -> RuntimePendingDrainFinalizerIdentityV4 {
        let common = self.source.common();
        let (source_intent_revision, source_state_digest) = match &self.source {
            RuntimeRoutedClaimedContinuationSourceV4::Current(candidate) => (
                candidate.source_intent_revision(),
                candidate.source_state_digest().clone(),
            ),
            RuntimeRoutedClaimedContinuationSourceV4::Applied(applied) => (
                applied
                    .receipt
                    .result
                    .canonical()
                    .intent()
                    .intent_revision(),
                RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                    applied.receipt.result.canonical().state_bytes(),
                ),
            ),
        };
        RuntimePendingDrainFinalizerIdentityV4 {
            process_instance_id: self.claimed.routed_seal.process_instance_id.clone(),
            intent_id: common.intent_id().clone(),
            source_intent_revision,
            source_state_digest,
            owner_lease_id: common.current_owner.lease_id.clone(),
            owner_revision: common.current_owner.owner_revision,
            action_identity: self.action_identity.clone(),
            seal_generation: self.claimed.routed_seal.seal_generation,
            route_incarnation: self.claimed.routed_seal.route.route_incarnation,
            controller_fence: self.claimed.claim_fence,
            registry_lifetime_digest: self.claimed.routed_seal.registry_lifetime_digest,
        }
    }
}

impl<R> Debug for RuntimeRoutedClaimedContinuationV4<R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRoutedClaimedContinuationV4(<redacted>)")
    }
}

impl RuntimeSelectedCurrentRoutedClaimedV4 {
    pub fn recover_routed_claimed<P>(
        self,
        port: &P,
    ) -> Result<
        RuntimeRoutedClaimedContinuationV4<P::RoutedClaimedSealed>,
        RuntimePendingDrainBoundaryErrorV4<P::Error>,
    >
    where
        P: RuntimePendingDrainRegistryTransitionPortV4,
    {
        let (registry_state, observation) = port
            .recover_routed_claimed(&self)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
        let claimed = observation
            .into_witness()
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        validate_routed_claimed_seal(&self.candidate.common, &claimed)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let action_identity = RuntimePendingDrainActionIdentityV4::successor(
            self.authorization.request().action_identity(),
            RuntimePendingDrainActionStageV4::RefenceProgress,
            1,
        )
        .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        Ok(RuntimeRoutedClaimedContinuationV4 {
            authorization: self.authorization,
            source: RuntimeRoutedClaimedContinuationSourceV4::Current(Box::new(self.candidate)),
            claimed,
            action_identity,
            registry_state,
        })
    }
}

pub type RuntimeRecoveredRouteAbsentRegistrationV4<R> =
    RuntimePendingDrainFinalizerRegistrationV4<RuntimeRouteAbsentAcknowledgementV4<R>>;

impl RuntimeSelectedCurrentRefencedV4 {
    pub fn recover_durable_refence_and_remove<P>(
        self,
        port: &P,
        certification: RuntimeDrainCertificationResolutionV2,
    ) -> Result<
        RuntimeRecoveredRouteAbsentRegistrationV4<P::RouteAbsentSealed>,
        RuntimePendingDrainBoundaryErrorV4<P::Error>,
    >
    where
        P: RuntimePendingDrainRegistryTransitionPortV4,
    {
        validate_resolution(&self.candidate.common, &certification)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        validate_resolved_serving(&self.candidate.common, &certification)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let (durable_state, observation) = port
            .recover_durable_refence(&self)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
        let durable = observation
            .into_witness()
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let reconstructed = self
            .reconstruct_durable_refence(durable)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let draining = port
            .begin_drain(durable_state)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
        let (route_absent_state, observation) = port
            .remove(draining)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
        let route_absent = observation
            .into_witness()
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let authorization = reconstructed
            .bind_route_absent(route_absent, certification)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        Ok(route_absent_finalizer_registration(
            authorization,
            route_absent_state,
        ))
    }

    pub fn recover_route_absent<P>(
        self,
        port: &P,
        certification: RuntimeDrainCertificationResolutionV2,
    ) -> Result<
        RuntimeRecoveredRouteAbsentRegistrationV4<P::RouteAbsentSealed>,
        RuntimePendingDrainBoundaryErrorV4<P::Error>,
    >
    where
        P: RuntimePendingDrainRegistryTransitionPortV4,
    {
        validate_resolution(&self.candidate.common, &certification)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        validate_resolved_serving(&self.candidate.common, &certification)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let (route_absent_state, observation) = port
            .recover_route_absent(&self)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
        let route_absent = observation
            .into_witness()
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let authorization = self
            .bind_recovered_route_absent(route_absent, certification)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        Ok(route_absent_finalizer_registration(
            authorization,
            route_absent_state,
        ))
    }
}

pub struct RuntimePendingDrainLaneJoinedV4<R, J> {
    registered: RuntimeRegisteredPendingDrainFinalizerV4<RuntimeRoutedClaimedContinuationV4<R>>,
    joined: J,
    joined_at: DateTime<Utc>,
}

impl<R, J> RuntimePendingDrainLaneJoinedV4<R, J> {
    pub fn joined_at(&self) -> DateTime<Utc> {
        self.joined_at
    }

    pub fn joined_boundary(&self) -> &J {
        &self.joined
    }

    pub fn finalizer_identity(&self) -> &RuntimePendingDrainFinalizerIdentityV4 {
        self.registered.identity()
    }
}

impl<R, J> Debug for RuntimePendingDrainLaneJoinedV4<R, J> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainLaneJoinedV4(<redacted>)")
    }
}

pub trait RuntimePendingDrainServingLanePortV4 {
    type Error;
    type Joined: Send;

    fn close_and_join_serving_lane_v4<R: Send>(
        &self,
        authorization: &RuntimeRegisteredPendingDrainFinalizerV4<
            RuntimeRoutedClaimedContinuationV4<R>,
        >,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<(Self::Joined, DateTime<Utc>), Self::Error>> + Send;
}

impl<R: Send> RuntimeRegisteredPendingDrainFinalizerV4<RuntimeRoutedClaimedContinuationV4<R>> {
    #[allow(clippy::manual_async_fn)]
    pub fn close_and_join_serving_lane<'a, P>(
        self,
        port: &'a P,
        operation_cutoff: Instant,
    ) -> impl Future<
        Output = Result<
            RuntimePendingDrainLaneJoinedV4<R, P::Joined>,
            RuntimePendingDrainBoundaryErrorV4<P::Error>,
        >,
    > + Send
           + 'a
    where
        P: RuntimePendingDrainServingLanePortV4 + Sync + 'a,
        R: 'a,
    {
        async move {
            let (joined, joined_at) = port
                .close_and_join_serving_lane_v4(&self, operation_cutoff)
                .await
                .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
            validate_database_time(joined_at)
                .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
            if joined_at < self.authorization().source.common().selection_database_now {
                return Err(RuntimePendingDrainBoundaryErrorV4::Contract(
                    RuntimePendingDrainV4Error::DatabaseClockRegressed,
                ));
            }
            Ok(RuntimePendingDrainLaneJoinedV4 {
                registered: self,
                joined,
                joined_at,
            })
        }
    }
}

pub struct RuntimePendingDrainServingResolvedV4<R, J, S> {
    lane: RuntimePendingDrainLaneJoinedV4<R, J>,
    resolution: S,
    evidence: RuntimePendingDrainServingEvidenceV4,
}

impl<R, J, S> RuntimePendingDrainServingResolvedV4<R, J, S> {
    pub fn evidence(&self) -> &RuntimePendingDrainServingEvidenceV4 {
        &self.evidence
    }
}

impl<R, J, S> Debug for RuntimePendingDrainServingResolvedV4<R, J, S> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainServingResolvedV4(<redacted>)")
    }
}

pub trait RuntimePendingDrainServingObservationPortV4 {
    type Error;
    type Resolution: Send;

    fn observe_and_disconnect_serving_v2<R: Send, J: Send>(
        &self,
        authorization: &RuntimePendingDrainLaneJoinedV4<R, J>,
        operation_cutoff: Instant,
    ) -> impl Future<
        Output = Result<(Self::Resolution, RuntimePendingDrainServingEvidenceV4), Self::Error>,
    > + Send;
}

impl<R: Send, J: Send> RuntimePendingDrainLaneJoinedV4<R, J> {
    #[allow(clippy::manual_async_fn)]
    pub fn observe_and_disconnect_serving<'a, P>(
        self,
        port: &'a P,
        operation_cutoff: Instant,
    ) -> impl Future<
        Output = Result<
            RuntimePendingDrainServingResolvedV4<R, J, P::Resolution>,
            RuntimePendingDrainBoundaryErrorV4<P::Error>,
        >,
    > + Send
           + 'a
    where
        P: RuntimePendingDrainServingObservationPortV4 + Sync + 'a,
        R: 'a,
        J: 'a,
    {
        async move {
            let (resolution, evidence) = port
                .observe_and_disconnect_serving_v2(&self, operation_cutoff)
                .await
                .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
            let continuation = self.lane_continuation();
            let common = continuation.source.common();
            validate_serving(
                &evidence,
                common.canonical().intent().key().scope.clone(),
                common.expected_target(),
                Some(continuation.source.claim()),
                &common.current_owner,
            )
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
            let observed_at = match &evidence {
                RuntimePendingDrainServingEvidenceV4::Absent { observed_at, .. } => *observed_at,
                RuntimePendingDrainServingEvidenceV4::Observed { database_now, .. } => {
                    *database_now
                }
            };
            if observed_at < self.joined_at {
                return Err(RuntimePendingDrainBoundaryErrorV4::Contract(
                    RuntimePendingDrainV4Error::DatabaseClockRegressed,
                ));
            }
            if let RuntimePendingDrainServingEvidenceV4::Observed {
                receipt,
                database_now,
                ..
            } = &evidence
            {
                if receipt.connected && receipt.expires_at > *database_now {
                    return Err(RuntimePendingDrainBoundaryErrorV4::Contract(
                        RuntimePendingDrainV4Error::ServingEvidenceMismatch,
                    ));
                }
            }
            Ok(RuntimePendingDrainServingResolvedV4 {
                lane: self,
                resolution,
                evidence,
            })
        }
    }

    fn lane_continuation(&self) -> &RuntimeRoutedClaimedContinuationV4<R> {
        self.registered.authorization()
    }
}

pub struct RuntimeAuthorizedRegistryRefenceV4<R, J, S, C> {
    serving: RuntimePendingDrainServingResolvedV4<R, J, S>,
    certification_boundary: C,
    certification: RuntimeDrainCertificationResolutionV2,
}

pub struct RuntimeAuthorizedRegistryRefenceEvidenceV4<J, S, C> {
    finalizer_identity: RuntimePendingDrainFinalizerIdentityV4,
    joined: J,
    joined_at: DateTime<Utc>,
    serving_resolution: S,
    serving_evidence: RuntimePendingDrainServingEvidenceV4,
    certification_boundary: C,
    certification: RuntimeDrainCertificationResolutionV2,
    provenance: RuntimeRouteMutationProvenanceV2,
    minimum_refenced_at: DateTime<Utc>,
    owner_expires_at: DateTime<Utc>,
}

impl<J, S, C> RuntimeAuthorizedRegistryRefenceEvidenceV4<J, S, C> {
    pub fn finalizer_identity(&self) -> &RuntimePendingDrainFinalizerIdentityV4 {
        &self.finalizer_identity
    }

    pub fn joined_at(&self) -> DateTime<Utc> {
        self.joined_at
    }

    pub fn joined_boundary(&self) -> &J {
        &self.joined
    }

    pub fn serving_evidence(&self) -> &RuntimePendingDrainServingEvidenceV4 {
        &self.serving_evidence
    }

    pub fn serving_resolution_boundary(&self) -> &S {
        &self.serving_resolution
    }

    pub fn certification(&self) -> &RuntimeDrainCertificationResolutionV2 {
        &self.certification
    }

    pub fn certification_boundary(&self) -> &C {
        &self.certification_boundary
    }

    pub fn provenance(&self) -> &RuntimeRouteMutationProvenanceV2 {
        &self.provenance
    }

    pub fn minimum_refenced_at(&self) -> DateTime<Utc> {
        self.minimum_refenced_at
    }

    pub fn owner_expires_at(&self) -> DateTime<Utc> {
        self.owner_expires_at
    }
}

impl<J, S, C> Debug for RuntimeAuthorizedRegistryRefenceEvidenceV4<J, S, C> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAuthorizedRegistryRefenceEvidenceV4(<redacted>)")
    }
}

impl<R, J, S, C> RuntimeAuthorizedRegistryRefenceV4<R, J, S, C> {
    pub fn finalizer_identity(&self) -> &RuntimePendingDrainFinalizerIdentityV4 {
        self.serving.lane.finalizer_identity()
    }

    pub fn serving_evidence(&self) -> &RuntimePendingDrainServingEvidenceV4 {
        &self.serving.evidence
    }

    pub fn certification(&self) -> &RuntimeDrainCertificationResolutionV2 {
        &self.certification
    }
}

impl<R, J, S, C> Debug for RuntimeAuthorizedRegistryRefenceV4<R, J, S, C> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAuthorizedRegistryRefenceV4(<redacted>)")
    }
}

pub trait RuntimePendingDrainCertificationResolutionPortV4 {
    type Error;
    type Resolution: Send;

    fn resolve_certification_v4<R: Send, J: Send, S: Send>(
        &self,
        authorization: &RuntimePendingDrainServingResolvedV4<R, J, S>,
        operation_cutoff: Instant,
    ) -> impl Future<
        Output = Result<(Self::Resolution, RuntimeDrainCertificationResolutionV2), Self::Error>,
    > + Send;
}

impl<R: Send, J: Send, S: Send> RuntimePendingDrainServingResolvedV4<R, J, S> {
    #[allow(clippy::manual_async_fn, clippy::type_complexity)]
    pub fn resolve_certification<'a, P>(
        self,
        port: &'a P,
        operation_cutoff: Instant,
    ) -> impl Future<
        Output = Result<
            RuntimeAuthorizedRegistryRefenceV4<R, J, S, P::Resolution>,
            RuntimePendingDrainBoundaryErrorV4<P::Error>,
        >,
    > + Send
           + 'a
    where
        P: RuntimePendingDrainCertificationResolutionPortV4 + Sync + 'a,
        R: 'a,
        J: 'a,
        S: 'a,
    {
        async move {
            let (certification_boundary, certification) = port
                .resolve_certification_v4(&self, operation_cutoff)
                .await
                .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
            let common = self.lane.registered.authorization().source.common();
            validate_resolution(common, &certification)
                .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
            validate_resolved_serving_evidence(&self.evidence, &certification)
                .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
            Ok(RuntimeAuthorizedRegistryRefenceV4 {
                serving: self,
                certification_boundary,
                certification,
            })
        }
    }
}

pub struct RuntimeLocalRefenceProgressV4<L> {
    authorization: RuntimeAuthorizedDrainRefenceProgressV4,
    registry_state: L,
    certification: RuntimeDrainCertificationResolutionV2,
}

impl<L> RuntimeLocalRefenceProgressV4<L> {
    pub fn authorization(&self) -> &RuntimeAuthorizedDrainRefenceProgressV4 {
        &self.authorization
    }
}

impl<L> Debug for RuntimeLocalRefenceProgressV4<L> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeLocalRefenceProgressV4(<redacted>)")
    }
}

pub enum RuntimeDrainRefenceProgressPortOutcomeV4 {
    Committed {
        mutation: Box<RuntimePendingDrainMutationPortReceiptV4>,
        result: Box<RuntimePersistedRefencedPendingDrainIntentV2>,
    },
    DeterminateNotCommitted,
    Unknown,
}

impl Debug for RuntimeDrainRefenceProgressPortOutcomeV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDrainRefenceProgressPortOutcomeV4(<redacted>)")
    }
}

pub trait RuntimeDrainRefenceProgressExecutionPortV4 {
    type Error;

    fn execute_drain_refence_progress_v4<L: Send>(
        &self,
        authorization: &RuntimeRegisteredPendingDrainFinalizerV4<RuntimeLocalRefenceProgressV4<L>>,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimeDrainRefenceProgressPortOutcomeV4, Self::Error>> + Send;
}

pub struct RuntimeDurableRefenceBoundaryV4<L> {
    durable: RuntimeDurableRefenceReceiptV4,
    registry_state: L,
    certification: RuntimeDrainCertificationResolutionV2,
}

impl<L> RuntimeDurableRefenceBoundaryV4<L> {
    pub fn durable_receipt(&self) -> &RuntimeDurableRefenceReceiptV4 {
        &self.durable
    }
}

impl<L> Debug for RuntimeDurableRefenceBoundaryV4<L> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDurableRefenceBoundaryV4(<redacted>)")
    }
}

pub enum RuntimeDrainRefenceProgressExecutionResolutionV4<L> {
    Committed(Box<RuntimeRegisteredPendingDrainFinalizerV4<RuntimeDurableRefenceBoundaryV4<L>>>),
    DeterminateNotCommitted(
        Box<RuntimeRegisteredPendingDrainFinalizerV4<RuntimeLocalRefenceProgressV4<L>>>,
    ),
    Unknown(
        Box<
            RuntimePendingDrainUnknownResultV4<
                RuntimeRegisteredPendingDrainFinalizerV4<RuntimeLocalRefenceProgressV4<L>>,
                RuntimeRefenceProgressMutationStageV4,
            >,
        >,
    ),
}

impl<L> Debug for RuntimeDrainRefenceProgressExecutionResolutionV4<L> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDrainRefenceProgressExecutionResolutionV4(<redacted>)")
    }
}

impl<L: Send> RuntimeRegisteredPendingDrainFinalizerV4<RuntimeLocalRefenceProgressV4<L>> {
    #[allow(clippy::manual_async_fn)]
    pub fn execute_refence_progress<'a, P>(
        self,
        port: &'a P,
        operation_cutoff: Instant,
    ) -> impl Future<
        Output = Result<
            RuntimeDrainRefenceProgressExecutionResolutionV4<L>,
            RuntimePendingDrainBoundaryErrorV4<P::Error>,
        >,
    > + Send
           + 'a
    where
        P: RuntimeDrainRefenceProgressExecutionPortV4 + Sync + 'a,
        L: 'a,
    {
        async move {
            let outcome = port
                .execute_drain_refence_progress_v4(&self, operation_cutoff)
                .await
                .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
            match outcome {
                RuntimeDrainRefenceProgressPortOutcomeV4::Committed { mutation, result } => {
                    let receipt = RuntimeDrainRefenceProgressReceiptV4::new(
                        (*mutation)
                            .into_receipt()
                            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?,
                        *result,
                    )
                    .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
                    let (identity, progress) = self.into_parts();
                    let RuntimeLocalRefenceProgressV4 {
                        authorization,
                        registry_state,
                        certification,
                    } = progress;
                    let durable = authorization
                        .accept_durable_receipt(receipt)
                        .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
                    Ok(RuntimeDrainRefenceProgressExecutionResolutionV4::Committed(
                        Box::new(RuntimeRegisteredPendingDrainFinalizerV4::from_parts(
                            identity,
                            RuntimeDurableRefenceBoundaryV4 {
                                durable,
                                registry_state,
                                certification,
                            },
                        )),
                    ))
                }
                RuntimeDrainRefenceProgressPortOutcomeV4::DeterminateNotCommitted => Ok(
                    RuntimeDrainRefenceProgressExecutionResolutionV4::DeterminateNotCommitted(
                        Box::new(self),
                    ),
                ),
                RuntimeDrainRefenceProgressPortOutcomeV4::Unknown => {
                    let identity = terminal_identity_for_registered_refence(&self)
                        .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
                    Ok(RuntimeDrainRefenceProgressExecutionResolutionV4::Unknown(
                        Box::new(RuntimePendingDrainUnknownResultV4::new(self, identity)),
                    ))
                }
            }
        }
    }
}

pub struct RuntimeDurablyRefencedBoundaryV4<D> {
    durable: RuntimeDurablyRefencedDrainV4,
    registry_state: D,
    certification: RuntimeDrainCertificationResolutionV2,
}

impl<D> RuntimeDurablyRefencedBoundaryV4<D> {
    pub fn durable(&self) -> &RuntimeDurablyRefencedDrainV4 {
        &self.durable
    }
}

impl<D> Debug for RuntimeDurablyRefencedBoundaryV4<D> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDurablyRefencedBoundaryV4(<redacted>)")
    }
}

impl<L> RuntimeRegisteredPendingDrainFinalizerV4<RuntimeDurableRefenceBoundaryV4<L>> {
    pub fn bind_registry_refence<P>(
        self,
        port: &P,
    ) -> Result<
        RuntimeRegisteredPendingDrainFinalizerV4<
            RuntimeDurablyRefencedBoundaryV4<P::DurablyRefencedSealed>,
        >,
        RuntimePendingDrainBoundaryErrorV4<P::Error>,
    >
    where
        P: RuntimePendingDrainRegistryTransitionPortV4<LocallyRefencedSealed = L>,
    {
        let (identity, boundary) = self.into_parts();
        let RuntimeDurableRefenceBoundaryV4 {
            durable,
            registry_state,
            certification,
        } = boundary;
        let (durable_state, observation) = port
            .bind_refence(registry_state, &durable)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
        let witness = observation
            .into_witness()
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let durable = durable
            .bind_registry_refence(witness)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        Ok(RuntimeRegisteredPendingDrainFinalizerV4::from_parts(
            identity,
            RuntimeDurablyRefencedBoundaryV4 {
                durable,
                registry_state: durable_state,
                certification,
            },
        ))
    }
}

pub struct RuntimeRouteAbsentAcknowledgementV4<R> {
    authorization: RuntimeAuthorizedSameProcessDrainAcknowledgementV4,
    registry_state: R,
}

impl<R> RuntimeRouteAbsentAcknowledgementV4<R> {
    pub fn action_identity(&self) -> &RuntimePendingDrainActionIdentityV4 {
        self.authorization.action_identity()
    }
}

impl<R> Debug for RuntimeRouteAbsentAcknowledgementV4<R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeRouteAbsentAcknowledgementV4(<redacted>)")
    }
}

fn route_absent_finalizer_registration<R>(
    authorization: RuntimeAuthorizedSameProcessDrainAcknowledgementV4,
    registry_state: R,
) -> RuntimeRecoveredRouteAbsentRegistrationV4<R> {
    let identity = RuntimePendingDrainFinalizerIdentityV4::for_acknowledgement(&authorization);
    RuntimePendingDrainFinalizerRegistrationV4::new(
        identity,
        RuntimeRouteAbsentAcknowledgementV4 {
            authorization,
            registry_state,
        },
    )
}

impl<D> RuntimeRegisteredPendingDrainFinalizerV4<RuntimeDurablyRefencedBoundaryV4<D>> {
    pub fn drain_and_remove<P>(
        self,
        port: &P,
    ) -> Result<
        RuntimePendingDrainFinalizerRegistrationV4<
            RuntimeRouteAbsentAcknowledgementV4<P::RouteAbsentSealed>,
        >,
        RuntimePendingDrainBoundaryErrorV4<P::Error>,
    >
    where
        P: RuntimePendingDrainRegistryTransitionPortV4<DurablyRefencedSealed = D>,
    {
        let (_, boundary) = self.into_parts();
        let RuntimeDurablyRefencedBoundaryV4 {
            durable,
            registry_state,
            certification,
        } = boundary;
        let draining = port
            .begin_drain(registry_state)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
        let (route_absent_state, observation) = port
            .remove(draining)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
        let route_absent = observation
            .into_witness()
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let authorization = durable
            .bind_route_absent(route_absent, certification)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let identity = RuntimePendingDrainFinalizerIdentityV4::for_acknowledgement(&authorization);
        Ok(RuntimePendingDrainFinalizerRegistrationV4::new(
            identity,
            RuntimeRouteAbsentAcknowledgementV4 {
                authorization,
                registry_state: route_absent_state,
            },
        ))
    }
}

pub enum RuntimeSameProcessDrainAcknowledgementPortOutcomeV4 {
    Committed {
        mutation: Box<RuntimePendingDrainMutationPortReceiptV4>,
        result: Box<RuntimeCanonicalDrainIntentStateV2>,
    },
    DeterminateNotCommitted,
    Unknown,
}

impl Debug for RuntimeSameProcessDrainAcknowledgementPortOutcomeV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeSameProcessDrainAcknowledgementPortOutcomeV4(<redacted>)")
    }
}

pub trait RuntimeSameProcessDrainAcknowledgementExecutionPortV4 {
    type Error;

    fn execute_same_process_drain_acknowledgement_v4<R: Send>(
        &self,
        authorization: &RuntimeRegisteredPendingDrainFinalizerV4<
            RuntimeRouteAbsentAcknowledgementV4<R>,
        >,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimeSameProcessDrainAcknowledgementPortOutcomeV4, Self::Error>>
           + Send;
}

pub struct RuntimeDurableSameProcessAcknowledgementBoundaryV4<R> {
    durable: RuntimeDurableSameProcessDrainAcknowledgementV4,
    registry_state: R,
}

impl<R> RuntimeDurableSameProcessAcknowledgementBoundaryV4<R> {
    pub fn durable(&self) -> &RuntimeDurableSameProcessDrainAcknowledgementV4 {
        &self.durable
    }
}

impl<R> Debug for RuntimeDurableSameProcessAcknowledgementBoundaryV4<R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDurableSameProcessAcknowledgementBoundaryV4(<redacted>)")
    }
}

pub enum RuntimeSameProcessDrainAcknowledgementExecutionResolutionV4<R> {
    Committed(
        RuntimeRegisteredPendingDrainFinalizerV4<
            RuntimeDurableSameProcessAcknowledgementBoundaryV4<R>,
        >,
    ),
    DeterminateNotCommitted(
        RuntimeRegisteredPendingDrainFinalizerV4<RuntimeRouteAbsentAcknowledgementV4<R>>,
    ),
    Unknown(
        RuntimePendingDrainUnknownResultV4<
            RuntimeRegisteredPendingDrainFinalizerV4<RuntimeRouteAbsentAcknowledgementV4<R>>,
            RuntimeSameProcessAcknowledgementMutationStageV4,
        >,
    ),
}

impl<R> Debug for RuntimeSameProcessDrainAcknowledgementExecutionResolutionV4<R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .write_str("RuntimeSameProcessDrainAcknowledgementExecutionResolutionV4(<redacted>)")
    }
}

impl<R: Send> RuntimeRegisteredPendingDrainFinalizerV4<RuntimeRouteAbsentAcknowledgementV4<R>> {
    #[allow(clippy::manual_async_fn)]
    pub fn execute_same_process_acknowledgement<'a, P>(
        self,
        port: &'a P,
        operation_cutoff: Instant,
    ) -> impl Future<
        Output = Result<
            RuntimeSameProcessDrainAcknowledgementExecutionResolutionV4<R>,
            RuntimePendingDrainBoundaryErrorV4<P::Error>,
        >,
    > + Send
           + 'a
    where
        P: RuntimeSameProcessDrainAcknowledgementExecutionPortV4 + Sync + 'a,
        R: 'a,
    {
        async move {
            let outcome = port
                .execute_same_process_drain_acknowledgement_v4(&self, operation_cutoff)
                .await
                .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
            match outcome {
                RuntimeSameProcessDrainAcknowledgementPortOutcomeV4::Committed {
                    mutation,
                    result,
                } => {
                    let receipt = RuntimeSameProcessDrainAcknowledgementReceiptV4::new(
                        (*mutation)
                            .into_receipt()
                            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?,
                        *result,
                    )
                    .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
                    let (identity, acknowledgement) = self.into_parts();
                    let RuntimeRouteAbsentAcknowledgementV4 {
                        authorization,
                        registry_state,
                    } = acknowledgement;
                    let durable = authorization
                        .accept_durable_receipt(receipt)
                        .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
                    Ok(
                        RuntimeSameProcessDrainAcknowledgementExecutionResolutionV4::Committed(
                            RuntimeRegisteredPendingDrainFinalizerV4::from_parts(
                                identity,
                                RuntimeDurableSameProcessAcknowledgementBoundaryV4 {
                                    durable,
                                    registry_state,
                                },
                            ),
                        ),
                    )
                }
                RuntimeSameProcessDrainAcknowledgementPortOutcomeV4::DeterminateNotCommitted => {
                    Ok(
                        RuntimeSameProcessDrainAcknowledgementExecutionResolutionV4::DeterminateNotCommitted(
                            self,
                        ),
                    )
                }
                RuntimeSameProcessDrainAcknowledgementPortOutcomeV4::Unknown => {
                    let identity = terminal_identity_for_registered_acknowledgement(&self)
                        .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
                    Ok(
                        RuntimeSameProcessDrainAcknowledgementExecutionResolutionV4::Unknown(
                            RuntimePendingDrainUnknownResultV4::new(self, identity),
                        ),
                    )
                }
            }
        }
    }
}

pub struct RuntimeAcknowledgedPendingDrainV4<A> {
    registry_state: A,
    result: RuntimeCanonicalDrainIntentStateV2,
}

impl<A> RuntimeAcknowledgedPendingDrainV4<A> {
    pub fn registry_state(&self) -> &A {
        &self.registry_state
    }

    pub fn result(&self) -> &RuntimeCanonicalDrainIntentStateV2 {
        &self.result
    }
}

impl<A> Debug for RuntimeAcknowledgedPendingDrainV4<A> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAcknowledgedPendingDrainV4(<redacted>)")
    }
}

impl<R>
    RuntimeRegisteredPendingDrainFinalizerV4<RuntimeDurableSameProcessAcknowledgementBoundaryV4<R>>
{
    pub fn consume_registry_acknowledgement<P>(
        self,
        port: &P,
    ) -> Result<
        RuntimeAcknowledgedPendingDrainV4<P::AcknowledgedEmpty>,
        RuntimePendingDrainBoundaryErrorV4<P::Error>,
    >
    where
        P: RuntimePendingDrainRegistryTransitionPortV4<RouteAbsentSealed = R>,
    {
        let (_, boundary) = self.into_parts();
        let RuntimeDurableSameProcessAcknowledgementBoundaryV4 {
            durable,
            registry_state,
        } = boundary;
        let result = durable.result().clone();
        let registry_state = port
            .consume_acknowledgement(registry_state, &durable)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
        Ok(RuntimeAcknowledgedPendingDrainV4 {
            registry_state,
            result,
        })
    }
}

fn terminal_identity_for_registered_acknowledgement<R>(
    registered: &RuntimeRegisteredPendingDrainFinalizerV4<RuntimeRouteAbsentAcknowledgementV4<R>>,
) -> Result<RuntimePendingDrainTerminalIdentityV4, RuntimePendingDrainV4Error> {
    let authorization = &registered.authorization().authorization;
    RuntimePendingDrainTerminalIdentityV4::new(
        authorization.action_identity.clone(),
        authorization.source.canonical().intent().intent_revision(),
        RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
            authorization.source.canonical().state_bytes(),
        ),
    )
}

pub enum RuntimePreviousProcessDrainTeardownPortOutcomeV4 {
    Committed {
        mutation: Box<RuntimePendingDrainMutationPortReceiptV4>,
        result: Box<RuntimeCanonicalDrainIntentStateV3>,
    },
    DeterminateNotCommitted,
    Unknown,
}

impl Debug for RuntimePreviousProcessDrainTeardownPortOutcomeV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePreviousProcessDrainTeardownPortOutcomeV4(<redacted>)")
    }
}

pub trait RuntimePreviousProcessDrainTeardownExecutionPortV4 {
    type Error;

    fn execute_previous_process_drain_teardown_v4<R: Send, E: Send>(
        &self,
        authorization: &RuntimeRegisteredPendingDrainFinalizerV4<
            RuntimePreviousProcessTeardownV4<R, E>,
        >,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimePreviousProcessDrainTeardownPortOutcomeV4, Self::Error>> + Send;
}

pub struct RuntimeDurablePreviousProcessTeardownBoundaryV4<R> {
    durable: RuntimeDurablePreviousProcessDrainTeardownV4,
    registry_state: R,
}

impl<R> RuntimeDurablePreviousProcessTeardownBoundaryV4<R> {
    pub fn durable(&self) -> &RuntimeDurablePreviousProcessDrainTeardownV4 {
        &self.durable
    }
}

impl<R> Debug for RuntimeDurablePreviousProcessTeardownBoundaryV4<R> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeDurablePreviousProcessTeardownBoundaryV4(<redacted>)")
    }
}

pub enum RuntimePreviousProcessDrainTeardownExecutionResolutionV4<R, E> {
    Committed(
        RuntimeRegisteredPendingDrainFinalizerV4<
            RuntimeDurablePreviousProcessTeardownBoundaryV4<R>,
        >,
    ),
    DeterminateNotCommitted(
        RuntimeRegisteredPendingDrainFinalizerV4<RuntimePreviousProcessTeardownV4<R, E>>,
    ),
    Unknown(
        RuntimePendingDrainUnknownResultV4<
            RuntimeRegisteredPendingDrainFinalizerV4<RuntimePreviousProcessTeardownV4<R, E>>,
            RuntimePreviousProcessTeardownMutationStageV4,
        >,
    ),
}

impl<R, E> Debug for RuntimePreviousProcessDrainTeardownExecutionResolutionV4<R, E> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePreviousProcessDrainTeardownExecutionResolutionV4(<redacted>)")
    }
}

impl<R: Send, E: Send>
    RuntimeRegisteredPendingDrainFinalizerV4<RuntimePreviousProcessTeardownV4<R, E>>
{
    #[allow(clippy::manual_async_fn)]
    pub fn execute_previous_process_teardown<'a, P>(
        self,
        port: &'a P,
        operation_cutoff: Instant,
    ) -> impl Future<
        Output = Result<
            RuntimePreviousProcessDrainTeardownExecutionResolutionV4<R, E>,
            RuntimePendingDrainBoundaryErrorV4<P::Error>,
        >,
    > + Send
           + 'a
    where
        P: RuntimePreviousProcessDrainTeardownExecutionPortV4 + Sync + 'a,
        R: 'a,
        E: 'a,
    {
        async move {
            let outcome = port
                .execute_previous_process_drain_teardown_v4(&self, operation_cutoff)
                .await
                .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
            match outcome {
                RuntimePreviousProcessDrainTeardownPortOutcomeV4::Committed {
                    mutation,
                    result,
                } => {
                    let receipt = RuntimePreviousProcessDrainTeardownReceiptV4::new(
                        (*mutation)
                            .into_receipt()
                            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?,
                        *result,
                    )
                    .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
                    let (identity, teardown) = self.into_parts();
                    let RuntimePreviousProcessTeardownV4 {
                        authorization,
                        registry_state,
                        evidence_boundary: _,
                    } = teardown;
                    let durable = authorization
                        .accept_durable_receipt(receipt)
                        .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
                    Ok(
                        RuntimePreviousProcessDrainTeardownExecutionResolutionV4::Committed(
                            RuntimeRegisteredPendingDrainFinalizerV4::from_parts(
                                identity,
                                RuntimeDurablePreviousProcessTeardownBoundaryV4 {
                                    durable,
                                    registry_state,
                                },
                            ),
                        ),
                    )
                }
                RuntimePreviousProcessDrainTeardownPortOutcomeV4::DeterminateNotCommitted => {
                    Ok(
                        RuntimePreviousProcessDrainTeardownExecutionResolutionV4::DeterminateNotCommitted(
                            self,
                        ),
                    )
                }
                RuntimePreviousProcessDrainTeardownPortOutcomeV4::Unknown => {
                    let identity = terminal_identity_for_registered_teardown(&self)
                        .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
                    Ok(
                        RuntimePreviousProcessDrainTeardownExecutionResolutionV4::Unknown(
                            RuntimePendingDrainUnknownResultV4::new(self, identity),
                        ),
                    )
                }
            }
        }
    }
}

pub struct RuntimeSuccessionAcknowledgedPendingDrainV4<A> {
    registry_state: A,
    result: RuntimeCanonicalDrainIntentStateV3,
}

impl<A> RuntimeSuccessionAcknowledgedPendingDrainV4<A> {
    pub fn registry_state(&self) -> &A {
        &self.registry_state
    }

    pub fn result(&self) -> &RuntimeCanonicalDrainIntentStateV3 {
        &self.result
    }
}

impl<A> Debug for RuntimeSuccessionAcknowledgedPendingDrainV4<A> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeSuccessionAcknowledgedPendingDrainV4(<redacted>)")
    }
}

impl<R>
    RuntimeRegisteredPendingDrainFinalizerV4<RuntimeDurablePreviousProcessTeardownBoundaryV4<R>>
{
    pub fn consume_succession_acknowledgement<P>(
        self,
        port: &P,
    ) -> Result<
        RuntimeSuccessionAcknowledgedPendingDrainV4<P::AcknowledgedEmpty>,
        RuntimePendingDrainBoundaryErrorV4<P::Error>,
    >
    where
        P: RuntimePendingDrainRegistryTransitionPortV4<EmptySuccessionSealed = R>,
    {
        let (_, boundary) = self.into_parts();
        let RuntimeDurablePreviousProcessTeardownBoundaryV4 {
            durable,
            registry_state,
        } = boundary;
        let result = durable.result().clone();
        let registry_state = port
            .consume_succession_acknowledgement(registry_state, &durable)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
        Ok(RuntimeSuccessionAcknowledgedPendingDrainV4 {
            registry_state,
            result,
        })
    }
}

fn terminal_identity_for_registered_teardown<R, E>(
    registered: &RuntimeRegisteredPendingDrainFinalizerV4<RuntimePreviousProcessTeardownV4<R, E>>,
) -> Result<RuntimePendingDrainTerminalIdentityV4, RuntimePendingDrainV4Error> {
    let authorization = &registered.authorization().authorization;
    RuntimePendingDrainTerminalIdentityV4::new(
        authorization.action_identity.clone(),
        authorization
            .source
            .common()
            .canonical()
            .intent()
            .intent_revision(),
        authorization.source.common().source_state_digest.clone(),
    )
}

impl<R, J, S, C> RuntimeAuthorizedRegistryRefenceV4<R, J, S, C>
where
    R: Send,
    J: Send,
    S: Send,
    C: Send,
{
    pub fn refence<P>(
        self,
        port: &P,
    ) -> Result<
        RuntimeRegisteredPendingDrainFinalizerV4<
            RuntimeLocalRefenceProgressV4<P::LocallyRefencedSealed>,
        >,
        RuntimePendingDrainBoundaryErrorV4<P::Error>,
    >
    where
        P: RuntimePendingDrainRegistryTransitionPortV4<RoutedClaimedSealed = R>,
    {
        let RuntimeAuthorizedRegistryRefenceV4 {
            serving,
            certification_boundary,
            certification,
        } = self;
        let RuntimePendingDrainServingResolvedV4 {
            lane,
            resolution,
            evidence,
        } = serving;
        let RuntimePendingDrainLaneJoinedV4 {
            registered,
            joined,
            joined_at,
        } = lane;
        let (identity, continuation) = registered.into_parts();
        let RuntimeRoutedClaimedContinuationV4 {
            authorization,
            source,
            claimed,
            action_identity,
            registry_state,
        } = continuation;
        let provenance = expected_teardown_provenance(authorization.request(), &action_identity)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let owner_expires_at = source.common().current_owner.expires_at;
        let proof = RuntimeAuthorizedRegistryRefenceEvidenceV4 {
            finalizer_identity: identity.clone(),
            joined,
            joined_at,
            serving_resolution: resolution,
            serving_evidence: evidence,
            certification_boundary,
            certification,
            provenance,
            minimum_refenced_at: joined_at,
            owner_expires_at,
        };
        let (local_state, local_observation) = port
            .refence(registry_state, &proof)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Port)?;
        let local = local_observation
            .into_witness()
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        if claimed.routed_seal.registry_lifetime_digest
            != local.claimed.routed_seal.registry_lifetime_digest
            || claimed.claim_receipt_digest != local.claimed.claim_receipt_digest
            || local.provenance != proof.provenance
            || local.refenced_at < proof.minimum_refenced_at
            || local.refenced_at >= proof.owner_expires_at
        {
            return Err(RuntimePendingDrainBoundaryErrorV4::Contract(
                RuntimePendingDrainV4Error::RegistryWitnessMismatch,
            ));
        }
        validate_local_refence_for_claim(source.common(), source.claim(), &local)
            .map_err(RuntimePendingDrainBoundaryErrorV4::Contract)?;
        let authorization = RuntimeAuthorizedDrainRefenceProgressV4 {
            authorization,
            source: source.into_refence_source(),
            seal: local,
            action_identity,
        };
        Ok(RuntimeRegisteredPendingDrainFinalizerV4::from_parts(
            identity,
            RuntimeLocalRefenceProgressV4 {
                authorization,
                registry_state: local_state,
                certification: proof.certification,
            },
        ))
    }
}

fn terminal_identity_for_registered_refence<L>(
    registered: &RuntimeRegisteredPendingDrainFinalizerV4<RuntimeLocalRefenceProgressV4<L>>,
) -> Result<RuntimePendingDrainTerminalIdentityV4, RuntimePendingDrainV4Error> {
    let authorization = &registered.authorization().authorization;
    let (source_intent_revision, source_state_digest) = match &authorization.source {
        RuntimeRefenceAuthorizationSourceV4::Current(candidate) => (
            candidate.source_intent_revision(),
            candidate.source_state_digest().clone(),
        ),
        RuntimeRefenceAuthorizationSourceV4::Applied(applied) => (
            applied
                .receipt
                .result
                .canonical()
                .intent()
                .intent_revision(),
            RuntimeDrainCanonicalStateDigestV3::from_state_bytes(
                applied.receipt.result.canonical().state_bytes(),
            ),
        ),
    };
    RuntimePendingDrainTerminalIdentityV4::new(
        authorization.action_identity.clone(),
        source_intent_revision,
        source_state_digest,
    )
}

fn validate_resolved_serving_evidence(
    evidence: &RuntimePendingDrainServingEvidenceV4,
    certification: &RuntimeDrainCertificationResolutionV2,
) -> Result<(), RuntimePendingDrainV4Error> {
    if certification.kind()
        != automation_runtime_controller::RuntimeDrainCertificationResolutionKindV2::CommittedAndDisconnected
    {
        return Ok(());
    }
    match evidence {
        RuntimePendingDrainServingEvidenceV4::Absent { .. } => Ok(()),
        RuntimePendingDrainServingEvidenceV4::Observed {
            receipt,
            database_now,
            ..
        } => {
            if certification.serving_identity() != Some(&receipt.identity)
                || (receipt.connected && receipt.expires_at > *database_now)
            {
                Err(RuntimePendingDrainV4Error::ServingEvidenceMismatch)
            } else {
                Ok(())
            }
        }
    }
}

fn validate_local_refence_for_claim(
    common: &RuntimePendingDrainCandidateCommonV4,
    claim: &RuntimeDrainClaimV2,
    local: &RuntimeLocallyRefencedSealedWitnessV4,
) -> Result<(), RuntimePendingDrainV4Error> {
    if common.intent_id() != &local.claimed.routed_seal.intent_id
        || common.slot() != &local.claimed.routed_seal.slot
        || local.claimed.claim_fence != claim.controller_fencing_token()
        || local.claimed.claim_receipt_digest.as_bytes()
            != common
                .claim_journal
                .as_ref()
                .ok_or(RuntimePendingDrainV4Error::ClaimJournalMissing)?
                .terminal_digest
                .as_bytes()
        || local.old_route
            != *claim
                .progress()
                .seal()
                .expected_route()
                .ok_or(RuntimePendingDrainV4Error::RouteMissing)?
        || local.removal_target.controller_fencing_token != claim.controller_fencing_token()
        || provenance_process(&local.provenance) != Some(claim.process_instance_id())
    {
        return Err(RuntimePendingDrainV4Error::RegistryWitnessMismatch);
    }
    Ok(())
}
