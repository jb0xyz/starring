use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimePendingDrainSelectionClassV4 {
    NoCandidate,
    Unclaimed,
    CurrentOwnerRouteAbsentClaimed,
    CurrentOwnerRoutedClaimed,
    CurrentOwnerRefenced,
    FreshPreviousOwnerRouteAbsentClaimed,
    ExpiredPreviousOwnerRouteAbsentClaimed,
    FreshPreviousOwnerRoutedClaimed,
    ExpiredPreviousOwnerRoutedClaimed,
    FreshPreviousOwnerRefenced,
    ExpiredPreviousOwnerRefenced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimePendingDrainActionStageV4 {
    RoutedClaim,
    RefenceProgress,
    SameProcessAcknowledgement,
    PreviousProcessTeardown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimePendingDrainJournalStageV4 {
    RoutedClaim,
    RefenceProgress,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimePendingDrainEvidenceDigestV4([u8; 32]);

impl RuntimePendingDrainEvidenceDigestV4 {
    pub fn new(value: [u8; 32]) -> Result<Self, RuntimePendingDrainV4Error> {
        if value == [0; 32] {
            return Err(RuntimePendingDrainV4Error::ZeroDigest);
        }
        Ok(Self(value))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Debug for RuntimePendingDrainEvidenceDigestV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainEvidenceDigestV4(<redacted>)")
    }
}

pub struct RuntimePendingDrainActionJournalEvidenceInputV4 {
    pub stage: RuntimePendingDrainJournalStageV4,
    pub intent_id: RuntimeDrainIntentIdV2,
    pub action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    pub owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub owner_revision: NonZeroU64,
    pub process_instance_id: ProcessInstanceId,
    pub claim_epoch: NonZeroU64,
    pub claim_revision: NonZeroU64,
    pub controller_fence: FencingToken,
    pub source_intent_revision: NonZeroU64,
    pub source_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub successor_intent_revision: NonZeroU64,
    pub successor_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    pub committed_at: DateTime<Utc>,
}

impl Debug for RuntimePendingDrainActionJournalEvidenceInputV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainActionJournalEvidenceInputV4(<redacted>)")
    }
}

pub struct RuntimePendingDrainActionJournalEvidenceV4 {
    pub(super) stage: RuntimePendingDrainJournalStageV4,
    pub(super) intent_id: RuntimeDrainIntentIdV2,
    pub(super) action_identity: RuntimeStartupRecoveryExecutionActionIdentityV2,
    pub(super) owner_lease_id: RuntimeGatewayOwnerLeaseIdV1,
    pub(super) owner_revision: NonZeroU64,
    pub(super) process_instance_id: ProcessInstanceId,
    pub(super) claim_epoch: NonZeroU64,
    pub(super) claim_revision: NonZeroU64,
    pub(super) controller_fence: FencingToken,
    pub(super) source_intent_revision: NonZeroU64,
    pub(super) source_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub(super) successor_intent_revision: NonZeroU64,
    pub(super) successor_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub(super) terminal_digest: RuntimeStartupRecoveryExecutionTerminalDigestV2,
    pub(super) committed_at: DateTime<Utc>,
}

impl RuntimePendingDrainActionJournalEvidenceV4 {
    pub fn new(
        input: RuntimePendingDrainActionJournalEvidenceInputV4,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        validate_persistence_value(input.owner_revision)?;
        validate_persistence_value(input.claim_epoch)?;
        validate_persistence_value(input.claim_revision)?;
        validate_fence(input.controller_fence)?;
        validate_persistence_value(input.source_intent_revision)?;
        validate_persistence_value(input.successor_intent_revision)?;
        validate_database_time(input.committed_at)?;
        if input
            .source_intent_revision
            .get()
            .checked_add(1)
            .and_then(NonZeroU64::new)
            != Some(input.successor_intent_revision)
        {
            return Err(RuntimePendingDrainV4Error::JournalRevisionMismatch);
        }
        if input.action_identity.class() != RuntimeStartupRecoveryClassV2::PendingRuntimeDrainIntent
        {
            return Err(RuntimePendingDrainV4Error::ActionClassMismatch);
        }
        if input.owner_lease_id.process_instance_id != input.process_instance_id {
            return Err(RuntimePendingDrainV4Error::JournalProcessMismatch);
        }
        Ok(Self {
            stage: input.stage,
            intent_id: input.intent_id,
            action_identity: input.action_identity,
            owner_lease_id: input.owner_lease_id,
            owner_revision: input.owner_revision,
            process_instance_id: input.process_instance_id,
            claim_epoch: input.claim_epoch,
            claim_revision: input.claim_revision,
            controller_fence: input.controller_fence,
            source_intent_revision: input.source_intent_revision,
            source_state_digest: input.source_state_digest,
            successor_intent_revision: input.successor_intent_revision,
            successor_state_digest: input.successor_state_digest,
            terminal_digest: input.terminal_digest,
            committed_at: input.committed_at,
        })
    }

    pub fn stage(&self) -> RuntimePendingDrainJournalStageV4 {
        self.stage
    }

    pub fn action_identity(&self) -> &RuntimeStartupRecoveryExecutionActionIdentityV2 {
        &self.action_identity
    }

    pub fn source_state_digest(&self) -> &RuntimeDrainCanonicalStateDigestV3 {
        &self.source_state_digest
    }

    pub fn successor_state_digest(&self) -> &RuntimeDrainCanonicalStateDigestV3 {
        &self.successor_state_digest
    }

    pub fn terminal_digest(&self) -> &RuntimeStartupRecoveryExecutionTerminalDigestV2 {
        &self.terminal_digest
    }

    pub fn committed_at(&self) -> DateTime<Utc> {
        self.committed_at
    }
}

impl Debug for RuntimePendingDrainActionJournalEvidenceV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainActionJournalEvidenceV4(<redacted>)")
    }
}

pub enum RuntimePendingDrainServingEvidenceV4 {
    Absent {
        observed_at: DateTime<Utc>,
        evidence_digest: RuntimePendingDrainEvidenceDigestV4,
    },
    Observed {
        receipt: Box<RuntimeServingReceiptV2>,
        database_now: DateTime<Utc>,
        evidence_digest: RuntimePendingDrainEvidenceDigestV4,
    },
}

impl RuntimePendingDrainServingEvidenceV4 {
    pub fn absent(
        observed_at: DateTime<Utc>,
        evidence_digest: RuntimePendingDrainEvidenceDigestV4,
    ) -> Self {
        Self::Absent {
            observed_at,
            evidence_digest,
        }
    }

    pub fn observed(
        receipt: RuntimeServingReceiptV2,
        database_now: DateTime<Utc>,
        evidence_digest: RuntimePendingDrainEvidenceDigestV4,
    ) -> Self {
        Self::Observed {
            receipt: Box::new(receipt),
            database_now,
            evidence_digest,
        }
    }

    pub fn evidence_digest(&self) -> RuntimePendingDrainEvidenceDigestV4 {
        match self {
            Self::Absent {
                evidence_digest, ..
            }
            | Self::Observed {
                evidence_digest, ..
            } => *evidence_digest,
        }
    }

    pub fn retry_after(&self, owner_expires_at: DateTime<Utc>) -> Option<Duration> {
        let Self::Observed {
            receipt,
            database_now,
            ..
        } = self
        else {
            return None;
        };
        if !receipt.connected || receipt.expires_at <= *database_now {
            return None;
        }
        positive_duration(receipt.expires_at, *database_now)
            .zip(positive_duration(owner_expires_at, *database_now))
            .map(|(serving, owner)| Duration::from_secs(1).min(serving).min(owner))
    }
}

impl Debug for RuntimePendingDrainServingEvidenceV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainServingEvidenceV4(<redacted>)")
    }
}

pub enum RuntimePendingDrainCertificationEvidenceV4 {
    NoOperationReserved {
        evidence_digest: RuntimePendingDrainEvidenceDigestV4,
    },
    NoAttestationForReservedOperation {
        operation_id: RuntimeCertificationOperationIdV2,
        intent_fingerprint: RuntimeCertificationIntentFingerprintV2,
        evidence_digest: RuntimePendingDrainEvidenceDigestV4,
    },
    Committed {
        serving_identity: Box<RuntimeServingIdentityV2>,
        evidence_digest: RuntimePendingDrainEvidenceDigestV4,
    },
}

impl RuntimePendingDrainCertificationEvidenceV4 {
    pub fn no_operation_reserved(evidence_digest: RuntimePendingDrainEvidenceDigestV4) -> Self {
        Self::NoOperationReserved { evidence_digest }
    }

    pub fn no_attestation_for_reserved_operation(
        operation_id: RuntimeCertificationOperationIdV2,
        intent_fingerprint: RuntimeCertificationIntentFingerprintV2,
        evidence_digest: RuntimePendingDrainEvidenceDigestV4,
    ) -> Self {
        Self::NoAttestationForReservedOperation {
            operation_id,
            intent_fingerprint,
            evidence_digest,
        }
    }

    pub fn committed(
        serving_identity: RuntimeServingIdentityV2,
        evidence_digest: RuntimePendingDrainEvidenceDigestV4,
    ) -> Self {
        Self::Committed {
            serving_identity: Box::new(serving_identity),
            evidence_digest,
        }
    }

    pub fn evidence_digest(&self) -> RuntimePendingDrainEvidenceDigestV4 {
        match self {
            Self::NoOperationReserved { evidence_digest }
            | Self::NoAttestationForReservedOperation {
                evidence_digest, ..
            }
            | Self::Committed {
                evidence_digest, ..
            } => *evidence_digest,
        }
    }
}

impl Debug for RuntimePendingDrainCertificationEvidenceV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainCertificationEvidenceV4(<redacted>)")
    }
}

pub struct RuntimePendingDrainCandidateEvidenceInputV4 {
    pub source_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub source_deployment_fence: FencingToken,
    pub selection_database_now: DateTime<Utc>,
    pub current_owner: RuntimeGatewayOwnerLeaseReceiptV1,
    pub claim_journal: Option<RuntimePendingDrainActionJournalEvidenceV4>,
    pub refence_journal: Option<RuntimePendingDrainActionJournalEvidenceV4>,
    pub serving: RuntimePendingDrainServingEvidenceV4,
    pub certification: RuntimePendingDrainCertificationEvidenceV4,
}

impl Debug for RuntimePendingDrainCandidateEvidenceInputV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainCandidateEvidenceInputV4(<redacted>)")
    }
}

pub(super) enum RuntimePendingDrainCanonicalSourceV4 {
    Unclaimed(RuntimePersistedUnclaimedPendingDrainIntentV2),
    RouteAbsentClaimed(RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2),
    RoutedClaimed(RuntimePersistedRoutedClaimedPendingDrainIntentV2),
    Refenced(RuntimePersistedRefencedPendingDrainIntentV2),
}

impl RuntimePendingDrainCanonicalSourceV4 {
    pub(super) fn canonical(
        &self,
    ) -> &automation_runtime_controller::RuntimeCanonicalDrainIntentStateV2 {
        match self {
            Self::Unclaimed(source) => source.canonical(),
            Self::RouteAbsentClaimed(source) => source.canonical(),
            Self::RoutedClaimed(source) => source.canonical(),
            Self::Refenced(source) => source.canonical(),
        }
    }
}

pub(super) struct RuntimePendingDrainCandidateCommonV4 {
    pub(super) source: RuntimePendingDrainCanonicalSourceV4,
    pub(super) source_state_digest: RuntimeDrainCanonicalStateDigestV3,
    pub(super) source_deployment_fence: FencingToken,
    pub(super) selection_database_now: DateTime<Utc>,
    pub(super) current_owner: RuntimeGatewayOwnerLeaseReceiptV1,
    pub(super) claim_journal: Option<RuntimePendingDrainActionJournalEvidenceV4>,
    pub(super) refence_journal: Option<RuntimePendingDrainActionJournalEvidenceV4>,
    pub(super) serving: RuntimePendingDrainServingEvidenceV4,
    pub(super) certification: RuntimePendingDrainCertificationEvidenceV4,
}

impl RuntimePendingDrainCandidateCommonV4 {
    pub(super) fn new(
        source: RuntimePendingDrainCanonicalSourceV4,
        input: RuntimePendingDrainCandidateEvidenceInputV4,
        minimum_intent_successors: u64,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        let canonical = source.canonical();
        let intent = canonical.intent();
        if RuntimeDrainCanonicalStateDigestV3::from_state_bytes(canonical.state_bytes())
            != input.source_state_digest
        {
            return Err(RuntimePendingDrainV4Error::SourceDigestMismatch);
        }
        if input.selection_database_now != input.current_owner.database_now {
            return Err(RuntimePendingDrainV4Error::DatabaseClockMismatch);
        }
        validate_database_time(input.selection_database_now)?;
        validate_owner_current(&input.current_owner)?;
        validate_fence(input.source_deployment_fence)?;
        validate_successor_budget(intent.intent_revision(), minimum_intent_successors)?;
        validate_serving(
            &input.serving,
            intent.key().scope.clone(),
            &intent.key().expected_target,
            intent.state().pending_claim(),
            &input.current_owner,
        )?;
        validate_certification(
            &input.certification,
            &input.serving,
            intent.key().scope.clone(),
            &intent.key().expected_target,
            intent.state().pending_claim(),
            &input.current_owner,
        )?;
        Ok(Self {
            source,
            source_state_digest: input.source_state_digest,
            source_deployment_fence: input.source_deployment_fence,
            selection_database_now: input.selection_database_now,
            current_owner: input.current_owner,
            claim_journal: input.claim_journal,
            refence_journal: input.refence_journal,
            serving: input.serving,
            certification: input.certification,
        })
    }

    pub(super) fn canonical(
        &self,
    ) -> &automation_runtime_controller::RuntimeCanonicalDrainIntentStateV2 {
        self.source.canonical()
    }

    pub(super) fn claim(&self) -> Option<&RuntimeDrainClaimV2> {
        self.canonical().intent().state().pending_claim()
    }

    fn validate_no_journals(&self) -> Result<(), RuntimePendingDrainV4Error> {
        if self.claim_journal.is_some() || self.refence_journal.is_some() {
            return Err(RuntimePendingDrainV4Error::UnexpectedJournal);
        }
        Ok(())
    }

    fn validate_claim_journal(&self) -> Result<(), RuntimePendingDrainV4Error> {
        let claim = self
            .claim()
            .ok_or(RuntimePendingDrainV4Error::ClaimMissing)?;
        let journal = self
            .claim_journal
            .as_ref()
            .ok_or(RuntimePendingDrainV4Error::ClaimJournalMissing)?;
        let (successor_intent_revision, successor_state_digest, claim_revision) =
            match claim.progress().kind() {
                RuntimeDrainClaimProgressKindV2::Claimed => (
                    self.canonical().intent().intent_revision(),
                    &self.source_state_digest,
                    claim.claim_revision(),
                ),
                RuntimeDrainClaimProgressKindV2::Refenced => {
                    let refence = self
                        .refence_journal
                        .as_ref()
                        .ok_or(RuntimePendingDrainV4Error::RefenceJournalMissing)?;
                    let claim_revision = claim
                        .claim_revision()
                        .get()
                        .checked_sub(1)
                        .and_then(NonZeroU64::new)
                        .ok_or(RuntimePendingDrainV4Error::ClaimJournalMismatch)?;
                    (
                        refence.source_intent_revision,
                        &refence.source_state_digest,
                        claim_revision,
                    )
                }
            };
        if journal.stage != RuntimePendingDrainJournalStageV4::RoutedClaim
            || journal.intent_id != self.canonical().intent().key().intent_id
            || journal.successor_intent_revision != successor_intent_revision
            || &journal.successor_state_digest != successor_state_digest
            || journal.owner_lease_id != *claim.gateway_owner_lease_id()
            || journal.owner_revision != claim.observed_owner_revision()
            || journal.process_instance_id != *claim.process_instance_id()
            || journal.claim_epoch != claim.claim_epoch()
            || journal.claim_revision != claim_revision
            || journal.controller_fence != claim.controller_fencing_token()
            || journal.committed_at > self.selection_database_now
        {
            return Err(RuntimePendingDrainV4Error::ClaimJournalMismatch);
        }
        Ok(())
    }

    fn validate_refence_journal(&self) -> Result<(), RuntimePendingDrainV4Error> {
        let claim = self
            .claim()
            .ok_or(RuntimePendingDrainV4Error::ClaimMissing)?;
        let claim_journal = self
            .claim_journal
            .as_ref()
            .ok_or(RuntimePendingDrainV4Error::ClaimJournalMissing)?;
        let refence_journal = self
            .refence_journal
            .as_ref()
            .ok_or(RuntimePendingDrainV4Error::RefenceJournalMissing)?;
        if refence_journal.stage != RuntimePendingDrainJournalStageV4::RefenceProgress
            || refence_journal.intent_id != self.canonical().intent().key().intent_id
            || refence_journal.source_intent_revision != claim_journal.successor_intent_revision
            || refence_journal.source_state_digest != claim_journal.successor_state_digest
            || refence_journal.successor_intent_revision
                != self.canonical().intent().intent_revision()
            || refence_journal.successor_state_digest != self.source_state_digest
            || refence_journal.owner_lease_id != *claim.gateway_owner_lease_id()
            || refence_journal.owner_revision != claim.observed_owner_revision()
            || refence_journal.process_instance_id != *claim.process_instance_id()
            || refence_journal.claim_epoch != claim.claim_epoch()
            || refence_journal.claim_revision != claim.claim_revision()
            || refence_journal.controller_fence != claim.controller_fencing_token()
            || refence_journal.committed_at > self.selection_database_now
            || claim_journal.action_identity == refence_journal.action_identity
            || claim_journal.terminal_digest == refence_journal.terminal_digest
            || refence_journal.committed_at < claim_journal.committed_at
        {
            return Err(RuntimePendingDrainV4Error::RefenceJournalMismatch);
        }
        Ok(())
    }

    pub(super) fn intent_id(&self) -> &RuntimeDrainIntentIdV2 {
        &self.canonical().intent().key().intent_id
    }

    pub(super) fn slot(&self) -> &RuntimeServingSlotV2 {
        &self.canonical().intent().key().slot
    }

    pub(super) fn expected_target(&self) -> &RuntimeDeploymentTargetV1 {
        &self.canonical().intent().key().expected_target
    }
}

macro_rules! define_candidate_accessors_v4 {
    ($name:ident) => {
        impl $name {
            pub fn intent_id(&self) -> &RuntimeDrainIntentIdV2 {
                self.common.intent_id()
            }

            pub fn slot(&self) -> &RuntimeServingSlotV2 {
                self.common.slot()
            }

            pub fn expected_target(&self) -> &RuntimeDeploymentTargetV1 {
                self.common.expected_target()
            }

            pub fn source_intent_revision(&self) -> NonZeroU64 {
                self.common.canonical().intent().intent_revision()
            }

            pub fn source_state_bytes(&self) -> &[u8] {
                self.common.canonical().state_bytes()
            }

            pub fn source_state_digest(&self) -> &RuntimeDrainCanonicalStateDigestV3 {
                &self.common.source_state_digest
            }

            pub fn source_deployment_fence(&self) -> FencingToken {
                self.common.source_deployment_fence
            }

            pub fn selection_database_now(&self) -> DateTime<Utc> {
                self.common.selection_database_now
            }

            pub fn current_owner(&self) -> &RuntimeGatewayOwnerLeaseReceiptV1 {
                &self.common.current_owner
            }

            pub fn serving_evidence(&self) -> &RuntimePendingDrainServingEvidenceV4 {
                &self.common.serving
            }

            pub fn certification_evidence(&self) -> &RuntimePendingDrainCertificationEvidenceV4 {
                &self.common.certification
            }
        }

        impl Debug for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(concat!(stringify!($name), "(<redacted>)"))
            }
        }
    };
}

pub struct RuntimeUnclaimedPendingDrainCandidateV4 {
    pub(super) common: RuntimePendingDrainCandidateCommonV4,
}

impl RuntimeUnclaimedPendingDrainCandidateV4 {
    pub fn new(
        source: RuntimePersistedUnclaimedPendingDrainIntentV2,
        input: RuntimePendingDrainCandidateEvidenceInputV4,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        let common = RuntimePendingDrainCandidateCommonV4::new(
            RuntimePendingDrainCanonicalSourceV4::Unclaimed(source),
            input,
            3,
        )?;
        common.validate_no_journals()?;
        common
            .source_deployment_fence
            .next()
            .map_err(|_| RuntimePendingDrainV4Error::ControllerFenceOverflow)?;
        Ok(Self { common })
    }
}

define_candidate_accessors_v4!(RuntimeUnclaimedPendingDrainCandidateV4);

pub struct RuntimeRouteAbsentClaimedPendingDrainCandidateV4 {
    pub(super) common: RuntimePendingDrainCandidateCommonV4,
}

impl RuntimeRouteAbsentClaimedPendingDrainCandidateV4 {
    pub fn new(
        source: RuntimePersistedRouteAbsentClaimedPendingDrainIntentV2,
        input: RuntimePendingDrainCandidateEvidenceInputV4,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        let common = RuntimePendingDrainCandidateCommonV4::new(
            RuntimePendingDrainCanonicalSourceV4::RouteAbsentClaimed(source),
            input,
            1,
        )?;
        common.validate_claim_journal()?;
        if common.refence_journal.is_some() {
            return Err(RuntimePendingDrainV4Error::UnexpectedJournal);
        }
        validate_claim_fence(&common)?;
        Ok(Self { common })
    }

    pub fn claim(&self) -> &RuntimeDrainClaimV2 {
        self.common.claim().expect("checked claimed candidate")
    }
}

define_candidate_accessors_v4!(RuntimeRouteAbsentClaimedPendingDrainCandidateV4);

pub struct RuntimeRoutedClaimedPendingDrainCandidateV4 {
    pub(super) common: RuntimePendingDrainCandidateCommonV4,
}

impl RuntimeRoutedClaimedPendingDrainCandidateV4 {
    pub fn new(
        source: RuntimePersistedRoutedClaimedPendingDrainIntentV2,
        input: RuntimePendingDrainCandidateEvidenceInputV4,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        let common = RuntimePendingDrainCandidateCommonV4::new(
            RuntimePendingDrainCanonicalSourceV4::RoutedClaimed(source),
            input,
            2,
        )?;
        common.validate_claim_journal()?;
        if common.refence_journal.is_some() {
            return Err(RuntimePendingDrainV4Error::UnexpectedJournal);
        }
        validate_claim_fence(&common)?;
        let claim = common
            .claim()
            .ok_or(RuntimePendingDrainV4Error::ClaimMissing)?;
        let route = claim
            .progress()
            .seal()
            .expected_route()
            .ok_or(RuntimePendingDrainV4Error::RouteMissing)?;
        if route.identity.target != *common.expected_target()
            || route.identity.process_instance_id != *claim.process_instance_id()
            || route.controller_fencing_token.next().ok() != Some(claim.controller_fencing_token())
        {
            return Err(RuntimePendingDrainV4Error::RouteLineageMismatch);
        }
        validate_successor_budget(claim.claim_revision(), 1)?;
        Ok(Self { common })
    }

    pub fn claim(&self) -> &RuntimeDrainClaimV2 {
        self.common.claim().expect("checked routed claim")
    }

    pub fn source_route(&self) -> &RuntimeExactLocalRouteIdentityV2 {
        self.claim()
            .progress()
            .seal()
            .expected_route()
            .expect("checked routed claim")
    }

    pub fn claim_terminal_digest(&self) -> &RuntimeStartupRecoveryExecutionTerminalDigestV2 {
        &self
            .common
            .claim_journal
            .as_ref()
            .expect("checked claim journal")
            .terminal_digest
    }
}

define_candidate_accessors_v4!(RuntimeRoutedClaimedPendingDrainCandidateV4);

pub struct RuntimeRefencedPendingDrainCandidateV4 {
    pub(super) common: RuntimePendingDrainCandidateCommonV4,
}

impl RuntimeRefencedPendingDrainCandidateV4 {
    pub fn new(
        source: RuntimePersistedRefencedPendingDrainIntentV2,
        input: RuntimePendingDrainCandidateEvidenceInputV4,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        let common = RuntimePendingDrainCandidateCommonV4::new(
            RuntimePendingDrainCanonicalSourceV4::Refenced(source),
            input,
            1,
        )?;
        common.validate_claim_journal()?;
        common.validate_refence_journal()?;
        validate_claim_fence(&common)?;
        let claim = common
            .claim()
            .ok_or(RuntimePendingDrainV4Error::ClaimMissing)?;
        if claim.progress().kind() != RuntimeDrainClaimProgressKindV2::Refenced {
            return Err(RuntimePendingDrainV4Error::ProgressMismatch);
        }
        let source_route = claim
            .progress()
            .old_route()
            .ok_or(RuntimePendingDrainV4Error::RouteMissing)?;
        let removal_target = claim
            .progress()
            .removal_target()
            .ok_or(RuntimePendingDrainV4Error::RouteMissing)?;
        if source_route.identity != removal_target.identity
            || source_route.route_incarnation != removal_target.route_incarnation
            || source_route.controller_fencing_token.next().ok()
                != Some(removal_target.controller_fencing_token)
            || removal_target.controller_fencing_token != claim.controller_fencing_token()
        {
            return Err(RuntimePendingDrainV4Error::RouteLineageMismatch);
        }
        claim
            .controller_fencing_token()
            .next()
            .map_err(|_| RuntimePendingDrainV4Error::ControllerFenceOverflow)?;
        Ok(Self { common })
    }

    pub fn claim(&self) -> &RuntimeDrainClaimV2 {
        self.common.claim().expect("checked refenced claim")
    }

    pub fn source_route(&self) -> &RuntimeExactLocalRouteIdentityV2 {
        self.claim()
            .progress()
            .old_route()
            .expect("checked refenced source route")
    }

    pub fn removal_target(&self) -> &RuntimeExactLocalRouteIdentityV2 {
        self.claim()
            .progress()
            .removal_target()
            .expect("checked refenced removal target")
    }

    pub fn claim_terminal_digest(&self) -> &RuntimeStartupRecoveryExecutionTerminalDigestV2 {
        &self
            .common
            .claim_journal
            .as_ref()
            .expect("checked claim journal")
            .terminal_digest
    }

    pub fn refence_terminal_digest(&self) -> &RuntimeStartupRecoveryExecutionTerminalDigestV2 {
        &self
            .common
            .refence_journal
            .as_ref()
            .expect("checked refence journal")
            .terminal_digest
    }
}

define_candidate_accessors_v4!(RuntimeRefencedPendingDrainCandidateV4);

pub enum RuntimePendingDrainSelectionOutcomeV4 {
    NoCandidate(RuntimeGatewayOwnerLeaseReceiptV1),
    Unclaimed(RuntimeUnclaimedPendingDrainCandidateV4),
    CurrentOwnerRouteAbsentClaimed(RuntimeRouteAbsentClaimedPendingDrainCandidateV4),
    CurrentOwnerRoutedClaimed(RuntimeRoutedClaimedPendingDrainCandidateV4),
    CurrentOwnerRefenced(RuntimeRefencedPendingDrainCandidateV4),
    FreshPreviousOwnerRouteAbsentClaimed(RuntimeRouteAbsentClaimedPendingDrainCandidateV4),
    ExpiredPreviousOwnerRouteAbsentClaimed(RuntimeRouteAbsentClaimedPendingDrainCandidateV4),
    FreshPreviousOwnerRoutedClaimed(RuntimeRoutedClaimedPendingDrainCandidateV4),
    ExpiredPreviousOwnerRoutedClaimed(RuntimeRoutedClaimedPendingDrainCandidateV4),
    FreshPreviousOwnerRefenced(RuntimeRefencedPendingDrainCandidateV4),
    ExpiredPreviousOwnerRefenced(RuntimeRefencedPendingDrainCandidateV4),
}

impl RuntimePendingDrainSelectionOutcomeV4 {
    pub fn class(&self) -> RuntimePendingDrainSelectionClassV4 {
        match self {
            Self::NoCandidate(_) => RuntimePendingDrainSelectionClassV4::NoCandidate,
            Self::Unclaimed(_) => RuntimePendingDrainSelectionClassV4::Unclaimed,
            Self::CurrentOwnerRouteAbsentClaimed(_) => {
                RuntimePendingDrainSelectionClassV4::CurrentOwnerRouteAbsentClaimed
            }
            Self::CurrentOwnerRoutedClaimed(_) => {
                RuntimePendingDrainSelectionClassV4::CurrentOwnerRoutedClaimed
            }
            Self::CurrentOwnerRefenced(_) => {
                RuntimePendingDrainSelectionClassV4::CurrentOwnerRefenced
            }
            Self::FreshPreviousOwnerRouteAbsentClaimed(_) => {
                RuntimePendingDrainSelectionClassV4::FreshPreviousOwnerRouteAbsentClaimed
            }
            Self::ExpiredPreviousOwnerRouteAbsentClaimed(_) => {
                RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRouteAbsentClaimed
            }
            Self::FreshPreviousOwnerRoutedClaimed(_) => {
                RuntimePendingDrainSelectionClassV4::FreshPreviousOwnerRoutedClaimed
            }
            Self::ExpiredPreviousOwnerRoutedClaimed(_) => {
                RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRoutedClaimed
            }
            Self::FreshPreviousOwnerRefenced(_) => {
                RuntimePendingDrainSelectionClassV4::FreshPreviousOwnerRefenced
            }
            Self::ExpiredPreviousOwnerRefenced(_) => {
                RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRefenced
            }
        }
    }
}

impl Debug for RuntimePendingDrainSelectionOutcomeV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("RuntimePendingDrainSelectionOutcomeV4")
            .field(&self.class())
            .finish()
    }
}

pub struct RuntimePendingDrainSelectionReceiptV4 {
    pub(super) correlation: RuntimeStartupRecoveryExecutionCorrelationV2,
    pub(super) outcome: RuntimePendingDrainSelectionOutcomeV4,
}

impl RuntimePendingDrainSelectionReceiptV4 {
    pub fn new(
        correlation: RuntimeStartupRecoveryExecutionCorrelationV2,
        outcome: RuntimePendingDrainSelectionOutcomeV4,
    ) -> Self {
        Self {
            correlation,
            outcome,
        }
    }
}

impl Debug for RuntimePendingDrainSelectionReceiptV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimePendingDrainSelectionReceiptV4(<redacted>)")
    }
}

pub trait RuntimePendingDrainSelectionPortV4 {
    type Error;

    fn select_pending_drain_v4(
        &self,
        authorization: &RuntimeAuthorizedPendingDrainSelectionV4,
        operation_cutoff: Instant,
    ) -> impl Future<Output = Result<RuntimePendingDrainSelectionReceiptV4, Self::Error>> + Send;
}

pub struct RuntimeAuthorizedPendingDrainSelectionV4 {
    pub(super) authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
}

impl RuntimeAuthorizedPendingDrainSelectionV4 {
    pub fn request(&self) -> &crate::RuntimeStartupRecoveryExecutionRequestV2 {
        self.authorization.request()
    }

    pub fn accept_selection(
        self,
        receipt: RuntimePendingDrainSelectionReceiptV4,
    ) -> Result<RuntimeAcceptedPendingDrainSelectionV4, RuntimePendingDrainV4Error> {
        if receipt.correlation != *self.authorization.request().correlation() {
            return Err(RuntimePendingDrainV4Error::CorrelationMismatch);
        }
        match receipt.outcome {
            RuntimePendingDrainSelectionOutcomeV4::NoCandidate(owner) => {
                validate_request_owner(self.authorization.request(), &owner)?;
                Ok(RuntimeAcceptedPendingDrainSelectionV4::NoCandidate(
                    RuntimeSelectedPendingDrainNoCandidateV4 {
                        authorization: self.authorization,
                        owner,
                    },
                ))
            }
            RuntimePendingDrainSelectionOutcomeV4::Unclaimed(candidate) => {
                validate_candidate_request(self.authorization.request(), &candidate.common)?;
                Ok(RuntimeAcceptedPendingDrainSelectionV4::Unclaimed(
                    RuntimeSelectedUnclaimedPendingDrainV4 {
                        authorization: self.authorization,
                        candidate,
                    },
                ))
            }
            RuntimePendingDrainSelectionOutcomeV4::CurrentOwnerRouteAbsentClaimed(candidate) => {
                validate_candidate_request(self.authorization.request(), &candidate.common)?;
                validate_current_claim(candidate.claim(), candidate.current_owner())?;
                Ok(
                    RuntimeAcceptedPendingDrainSelectionV4::CurrentOwnerRouteAbsentClaimed(
                        RuntimeSelectedCurrentRouteAbsentClaimedV4 {
                            authorization: self.authorization,
                            candidate,
                        },
                    ),
                )
            }
            RuntimePendingDrainSelectionOutcomeV4::CurrentOwnerRoutedClaimed(candidate) => {
                validate_candidate_request(self.authorization.request(), &candidate.common)?;
                validate_current_claim(candidate.claim(), candidate.current_owner())?;
                Ok(
                    RuntimeAcceptedPendingDrainSelectionV4::CurrentOwnerRoutedClaimed(
                        RuntimeSelectedCurrentRoutedClaimedV4 {
                            authorization: self.authorization,
                            candidate,
                        },
                    ),
                )
            }
            RuntimePendingDrainSelectionOutcomeV4::CurrentOwnerRefenced(candidate) => {
                validate_candidate_request(self.authorization.request(), &candidate.common)?;
                validate_current_claim(candidate.claim(), candidate.current_owner())?;
                Ok(
                    RuntimeAcceptedPendingDrainSelectionV4::CurrentOwnerRefenced(
                        RuntimeSelectedCurrentRefencedV4 {
                            authorization: self.authorization,
                            candidate,
                        },
                    ),
                )
            }
            RuntimePendingDrainSelectionOutcomeV4::FreshPreviousOwnerRouteAbsentClaimed(
                candidate,
            ) => {
                validate_candidate_request(self.authorization.request(), &candidate.common)?;
                validate_previous_claim(candidate.claim(), candidate.current_owner(), false)?;
                Ok(
                    RuntimeAcceptedPendingDrainSelectionV4::FreshPreviousOwnerRouteAbsentClaimed(
                        RuntimeSelectedFreshPreviousOwnerV4::new_route_absent(
                            self.authorization,
                            candidate,
                        )?,
                    ),
                )
            }
            RuntimePendingDrainSelectionOutcomeV4::ExpiredPreviousOwnerRouteAbsentClaimed(
                candidate,
            ) => {
                validate_candidate_request(self.authorization.request(), &candidate.common)?;
                validate_previous_claim(candidate.claim(), candidate.current_owner(), true)?;
                Ok(
                    RuntimeAcceptedPendingDrainSelectionV4::ExpiredPreviousOwnerRouteAbsentClaimed(
                        RuntimeSelectedExpiredPreviousOwnerV4::RouteAbsentClaimed {
                            authorization: self.authorization,
                            candidate,
                        },
                    ),
                )
            }
            RuntimePendingDrainSelectionOutcomeV4::FreshPreviousOwnerRoutedClaimed(candidate) => {
                validate_candidate_request(self.authorization.request(), &candidate.common)?;
                validate_previous_claim(candidate.claim(), candidate.current_owner(), false)?;
                Ok(
                    RuntimeAcceptedPendingDrainSelectionV4::FreshPreviousOwnerRoutedClaimed(
                        RuntimeSelectedFreshPreviousOwnerV4::new_routed(
                            self.authorization,
                            candidate,
                        )?,
                    ),
                )
            }
            RuntimePendingDrainSelectionOutcomeV4::ExpiredPreviousOwnerRoutedClaimed(candidate) => {
                validate_candidate_request(self.authorization.request(), &candidate.common)?;
                validate_previous_claim(candidate.claim(), candidate.current_owner(), true)?;
                Ok(
                    RuntimeAcceptedPendingDrainSelectionV4::ExpiredPreviousOwnerRoutedClaimed(
                        RuntimeSelectedExpiredPreviousOwnerV4::RoutedClaimed {
                            authorization: self.authorization,
                            candidate,
                        },
                    ),
                )
            }
            RuntimePendingDrainSelectionOutcomeV4::FreshPreviousOwnerRefenced(candidate) => {
                validate_candidate_request(self.authorization.request(), &candidate.common)?;
                validate_previous_claim(candidate.claim(), candidate.current_owner(), false)?;
                Ok(
                    RuntimeAcceptedPendingDrainSelectionV4::FreshPreviousOwnerRefenced(
                        RuntimeSelectedFreshPreviousOwnerV4::new_refenced(
                            self.authorization,
                            candidate,
                        )?,
                    ),
                )
            }
            RuntimePendingDrainSelectionOutcomeV4::ExpiredPreviousOwnerRefenced(candidate) => {
                validate_candidate_request(self.authorization.request(), &candidate.common)?;
                validate_previous_claim(candidate.claim(), candidate.current_owner(), true)?;
                Ok(
                    RuntimeAcceptedPendingDrainSelectionV4::ExpiredPreviousOwnerRefenced(
                        RuntimeSelectedExpiredPreviousOwnerV4::Refenced {
                            authorization: self.authorization,
                            candidate,
                        },
                    ),
                )
            }
        }
    }
}

impl Debug for RuntimeAuthorizedPendingDrainSelectionV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeAuthorizedPendingDrainSelectionV4(<redacted>)")
    }
}

pub enum RuntimeAcceptedPendingDrainSelectionV4 {
    NoCandidate(RuntimeSelectedPendingDrainNoCandidateV4),
    Unclaimed(RuntimeSelectedUnclaimedPendingDrainV4),
    CurrentOwnerRouteAbsentClaimed(RuntimeSelectedCurrentRouteAbsentClaimedV4),
    CurrentOwnerRoutedClaimed(RuntimeSelectedCurrentRoutedClaimedV4),
    CurrentOwnerRefenced(RuntimeSelectedCurrentRefencedV4),
    FreshPreviousOwnerRouteAbsentClaimed(RuntimeSelectedFreshPreviousOwnerV4),
    ExpiredPreviousOwnerRouteAbsentClaimed(RuntimeSelectedExpiredPreviousOwnerV4),
    FreshPreviousOwnerRoutedClaimed(RuntimeSelectedFreshPreviousOwnerV4),
    ExpiredPreviousOwnerRoutedClaimed(RuntimeSelectedExpiredPreviousOwnerV4),
    FreshPreviousOwnerRefenced(RuntimeSelectedFreshPreviousOwnerV4),
    ExpiredPreviousOwnerRefenced(RuntimeSelectedExpiredPreviousOwnerV4),
}

impl RuntimeAcceptedPendingDrainSelectionV4 {
    pub fn class(&self) -> RuntimePendingDrainSelectionClassV4 {
        match self {
            Self::NoCandidate(_) => RuntimePendingDrainSelectionClassV4::NoCandidate,
            Self::Unclaimed(_) => RuntimePendingDrainSelectionClassV4::Unclaimed,
            Self::CurrentOwnerRouteAbsentClaimed(_) => {
                RuntimePendingDrainSelectionClassV4::CurrentOwnerRouteAbsentClaimed
            }
            Self::CurrentOwnerRoutedClaimed(_) => {
                RuntimePendingDrainSelectionClassV4::CurrentOwnerRoutedClaimed
            }
            Self::CurrentOwnerRefenced(_) => {
                RuntimePendingDrainSelectionClassV4::CurrentOwnerRefenced
            }
            Self::FreshPreviousOwnerRouteAbsentClaimed(_) => {
                RuntimePendingDrainSelectionClassV4::FreshPreviousOwnerRouteAbsentClaimed
            }
            Self::ExpiredPreviousOwnerRouteAbsentClaimed(_) => {
                RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRouteAbsentClaimed
            }
            Self::FreshPreviousOwnerRoutedClaimed(_) => {
                RuntimePendingDrainSelectionClassV4::FreshPreviousOwnerRoutedClaimed
            }
            Self::ExpiredPreviousOwnerRoutedClaimed(_) => {
                RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRoutedClaimed
            }
            Self::FreshPreviousOwnerRefenced(_) => {
                RuntimePendingDrainSelectionClassV4::FreshPreviousOwnerRefenced
            }
            Self::ExpiredPreviousOwnerRefenced(_) => {
                RuntimePendingDrainSelectionClassV4::ExpiredPreviousOwnerRefenced
            }
        }
    }
}

impl Debug for RuntimeAcceptedPendingDrainSelectionV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("RuntimeAcceptedPendingDrainSelectionV4")
            .field(&self.class())
            .finish()
    }
}

pub struct RuntimeSelectedPendingDrainNoCandidateV4 {
    pub(super) authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    pub(super) owner: RuntimeGatewayOwnerLeaseReceiptV1,
}

impl RuntimeSelectedPendingDrainNoCandidateV4 {
    pub fn owner(&self) -> &RuntimeGatewayOwnerLeaseReceiptV1 {
        &self.owner
    }

    pub fn request(&self) -> &crate::RuntimeStartupRecoveryExecutionRequestV2 {
        self.authorization.request()
    }
}

impl Debug for RuntimeSelectedPendingDrainNoCandidateV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeSelectedPendingDrainNoCandidateV4(<redacted>)")
    }
}

pub struct RuntimeSelectedUnclaimedPendingDrainV4 {
    pub(super) authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    pub(super) candidate: RuntimeUnclaimedPendingDrainCandidateV4,
}

impl RuntimeSelectedUnclaimedPendingDrainV4 {
    pub fn candidate(&self) -> &RuntimeUnclaimedPendingDrainCandidateV4 {
        &self.candidate
    }

    pub(super) fn bind_routed_seal(
        self,
        seal: RuntimeRoutedSealedWitnessV4,
    ) -> Result<RuntimeAuthorizedRoutedDrainClaimV4, RuntimePendingDrainV4Error> {
        validate_routed_seal(&self.candidate.common, &seal)?;
        let action_identity = RuntimePendingDrainActionIdentityV4::successor(
            self.authorization.request().action_identity(),
            RuntimePendingDrainActionStageV4::RoutedClaim,
            1,
        )?;
        Ok(RuntimeAuthorizedRoutedDrainClaimV4 {
            authorization: self.authorization,
            candidate: self.candidate,
            seal,
            action_identity,
        })
    }
}

impl Debug for RuntimeSelectedUnclaimedPendingDrainV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeSelectedUnclaimedPendingDrainV4(<redacted>)")
    }
}

pub struct RuntimeSelectedCurrentRouteAbsentClaimedV4 {
    pub(super) authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    pub(super) candidate: RuntimeRouteAbsentClaimedPendingDrainCandidateV4,
}

impl RuntimeSelectedCurrentRouteAbsentClaimedV4 {
    pub fn candidate(&self) -> &RuntimeRouteAbsentClaimedPendingDrainCandidateV4 {
        &self.candidate
    }

    pub fn request(&self) -> &crate::RuntimeStartupRecoveryExecutionRequestV2 {
        self.authorization.request()
    }
}

impl Debug for RuntimeSelectedCurrentRouteAbsentClaimedV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeSelectedCurrentRouteAbsentClaimedV4(<redacted>)")
    }
}

pub struct RuntimeSelectedCurrentRoutedClaimedV4 {
    pub(super) authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    pub(super) candidate: RuntimeRoutedClaimedPendingDrainCandidateV4,
}

impl RuntimeSelectedCurrentRoutedClaimedV4 {
    pub fn candidate(&self) -> &RuntimeRoutedClaimedPendingDrainCandidateV4 {
        &self.candidate
    }
}

impl Debug for RuntimeSelectedCurrentRoutedClaimedV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeSelectedCurrentRoutedClaimedV4(<redacted>)")
    }
}

pub struct RuntimeSelectedCurrentRefencedV4 {
    pub(super) authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    pub(super) candidate: RuntimeRefencedPendingDrainCandidateV4,
}

impl RuntimeSelectedCurrentRefencedV4 {
    pub fn candidate(&self) -> &RuntimeRefencedPendingDrainCandidateV4 {
        &self.candidate
    }

    pub(super) fn reconstruct_durable_refence(
        self,
        durable: RuntimeDurablyRefencedSealedWitnessV4,
    ) -> Result<RuntimeReconstructedDurablyRefencedV4, RuntimePendingDrainV4Error> {
        validate_durable_refence_witness(&self.candidate.common, &durable)?;
        Ok(RuntimeReconstructedDurablyRefencedV4 {
            authorization: self.authorization,
            candidate: self.candidate,
            durable,
        })
    }

    pub(super) fn bind_recovered_route_absent(
        self,
        route_absent: RuntimeRouteAbsentSealedWitnessV4,
        certification: RuntimeDrainCertificationResolutionV2,
    ) -> Result<RuntimeAuthorizedSameProcessDrainAcknowledgementV4, RuntimePendingDrainV4Error>
    {
        validate_route_absent(&self.candidate.common, &route_absent)?;
        validate_resolution(&self.candidate.common, &certification)?;
        validate_resolved_serving(&self.candidate.common, &certification)?;
        let action_identity = RuntimePendingDrainActionIdentityV4::successor(
            self.authorization.request().action_identity(),
            RuntimePendingDrainActionStageV4::SameProcessAcknowledgement,
            1,
        )?;
        Ok(RuntimeAuthorizedSameProcessDrainAcknowledgementV4 {
            _authorization: self.authorization,
            source: RuntimeAcknowledgementAuthorizationSourceV4::Selected(Box::new(self.candidate)),
            route_absent,
            certification,
            action_identity,
        })
    }
}

impl Debug for RuntimeSelectedCurrentRefencedV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeSelectedCurrentRefencedV4(<redacted>)")
    }
}

pub struct RuntimeReconstructedDurablyRefencedV4 {
    pub(super) authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
    pub(super) candidate: RuntimeRefencedPendingDrainCandidateV4,
    pub(super) durable: RuntimeDurablyRefencedSealedWitnessV4,
}

impl RuntimeReconstructedDurablyRefencedV4 {
    #[cfg(test)]
    pub fn candidate(&self) -> &RuntimeRefencedPendingDrainCandidateV4 {
        &self.candidate
    }

    #[cfg(test)]
    pub fn durable_witness(&self) -> &RuntimeDurablyRefencedSealedWitnessV4 {
        &self.durable
    }

    pub(super) fn bind_route_absent(
        self,
        route_absent: RuntimeRouteAbsentSealedWitnessV4,
        certification: RuntimeDrainCertificationResolutionV2,
    ) -> Result<RuntimeAuthorizedSameProcessDrainAcknowledgementV4, RuntimePendingDrainV4Error>
    {
        validate_route_absent(&self.candidate.common, &route_absent)?;
        validate_resolution(&self.candidate.common, &certification)?;
        validate_resolved_serving(&self.candidate.common, &certification)?;
        if self.durable.locally_refenced.old_route != route_absent.source_route
            || self.durable.locally_refenced.removal_target != route_absent.removed_route
            || self.durable.refence_receipt_digest != route_absent.refence_receipt_digest
        {
            return Err(RuntimePendingDrainV4Error::RegistryWitnessMismatch);
        }
        let action_identity = RuntimePendingDrainActionIdentityV4::successor(
            self.authorization.request().action_identity(),
            RuntimePendingDrainActionStageV4::SameProcessAcknowledgement,
            1,
        )?;
        Ok(RuntimeAuthorizedSameProcessDrainAcknowledgementV4 {
            _authorization: self.authorization,
            source: RuntimeAcknowledgementAuthorizationSourceV4::Selected(Box::new(self.candidate)),
            route_absent,
            certification,
            action_identity,
        })
    }
}

impl Debug for RuntimeReconstructedDurablyRefencedV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeReconstructedDurablyRefencedV4(<redacted>)")
    }
}

pub enum RuntimeSelectedFreshPreviousOwnerV4 {
    RouteAbsentClaimed {
        authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
        candidate: RuntimeRouteAbsentClaimedPendingDrainCandidateV4,
        retry_after: Duration,
    },
    RoutedClaimed {
        authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
        candidate: RuntimeRoutedClaimedPendingDrainCandidateV4,
        retry_after: Duration,
    },
    Refenced {
        authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
        candidate: RuntimeRefencedPendingDrainCandidateV4,
        retry_after: Duration,
    },
}

impl RuntimeSelectedFreshPreviousOwnerV4 {
    fn new_route_absent(
        authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
        candidate: RuntimeRouteAbsentClaimedPendingDrainCandidateV4,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        let retry_after = previous_retry(candidate.claim(), candidate.current_owner())?;
        Ok(Self::RouteAbsentClaimed {
            authorization,
            candidate,
            retry_after,
        })
    }

    fn new_routed(
        authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
        candidate: RuntimeRoutedClaimedPendingDrainCandidateV4,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        let retry_after = previous_retry(candidate.claim(), candidate.current_owner())?;
        Ok(Self::RoutedClaimed {
            authorization,
            candidate,
            retry_after,
        })
    }

    fn new_refenced(
        authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
        candidate: RuntimeRefencedPendingDrainCandidateV4,
    ) -> Result<Self, RuntimePendingDrainV4Error> {
        let retry_after = previous_retry(candidate.claim(), candidate.current_owner())?;
        Ok(Self::Refenced {
            authorization,
            candidate,
            retry_after,
        })
    }

    pub fn retry_after(&self) -> Duration {
        match self {
            Self::RouteAbsentClaimed { retry_after, .. }
            | Self::RoutedClaimed { retry_after, .. }
            | Self::Refenced { retry_after, .. } => *retry_after,
        }
    }
}

impl Debug for RuntimeSelectedFreshPreviousOwnerV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeSelectedFreshPreviousOwnerV4(<redacted>)")
    }
}

pub enum RuntimeSelectedExpiredPreviousOwnerV4 {
    RouteAbsentClaimed {
        authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
        candidate: RuntimeRouteAbsentClaimedPendingDrainCandidateV4,
    },
    RoutedClaimed {
        authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
        candidate: RuntimeRoutedClaimedPendingDrainCandidateV4,
    },
    Refenced {
        authorization: RuntimeAuthorizedStartupRecoveryExecutionV2,
        candidate: RuntimeRefencedPendingDrainCandidateV4,
    },
}

pub struct RuntimeEmptySuccessionSealRequestV4<'a> {
    pub(super) intent_id: &'a RuntimeDrainIntentIdV2,
    pub(super) slot: &'a RuntimeServingSlotV2,
    pub(super) predecessor_route: &'a RuntimeExactLocalRouteIdentityV2,
    pub(super) possible_route_fence_ceiling: FencingToken,
    pub(super) successor_process_instance_id: &'a ProcessInstanceId,
    pub(super) successor_target: &'a RuntimeDeploymentTargetV1,
    pub(super) successor_fence: FencingToken,
}

impl RuntimeEmptySuccessionSealRequestV4<'_> {
    pub fn intent_id(&self) -> &RuntimeDrainIntentIdV2 {
        self.intent_id
    }

    pub fn slot(&self) -> &RuntimeServingSlotV2 {
        self.slot
    }

    pub fn seal_key(&self) -> [u8; 16] {
        self.intent_id.canonical_bytes()
    }

    pub fn predecessor_route(&self) -> &RuntimeExactLocalRouteIdentityV2 {
        self.predecessor_route
    }

    pub fn possible_route_fence_ceiling(&self) -> FencingToken {
        self.possible_route_fence_ceiling
    }

    pub fn successor_process_instance_id(&self) -> &ProcessInstanceId {
        self.successor_process_instance_id
    }

    pub fn successor_target(&self) -> &RuntimeDeploymentTargetV1 {
        self.successor_target
    }

    pub fn successor_fence(&self) -> FencingToken {
        self.successor_fence
    }
}

impl Debug for RuntimeEmptySuccessionSealRequestV4<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeEmptySuccessionSealRequestV4(<redacted>)")
    }
}

impl RuntimeSelectedExpiredPreviousOwnerV4 {
    pub fn empty_succession_seal_request(
        &self,
    ) -> Result<RuntimeEmptySuccessionSealRequestV4<'_>, RuntimePendingDrainV4Error> {
        let common = match self {
            Self::RouteAbsentClaimed { .. } => {
                return Err(RuntimePendingDrainV4Error::LegacyRouteAbsentHandoffRequired)
            }
            Self::RoutedClaimed { candidate, .. } => &candidate.common,
            Self::Refenced { candidate, .. } => &candidate.common,
        };
        let claim = common
            .claim()
            .ok_or(RuntimePendingDrainV4Error::ClaimMissing)?;
        let predecessor_route = claim
            .progress()
            .seal()
            .expected_route()
            .ok_or(RuntimePendingDrainV4Error::RouteMissing)?;
        let successor_fence = claim
            .controller_fencing_token()
            .next()
            .map_err(|_| RuntimePendingDrainV4Error::ControllerFenceOverflow)?;
        Ok(RuntimeEmptySuccessionSealRequestV4 {
            intent_id: common.intent_id(),
            slot: common.slot(),
            predecessor_route,
            possible_route_fence_ceiling: claim.controller_fencing_token(),
            successor_process_instance_id: &common.current_owner.lease_id.process_instance_id,
            successor_target: common.expected_target(),
            successor_fence,
        })
    }

    pub(super) fn bind_empty_succession_seal(
        self,
        seal: RuntimeEmptySuccessionSealedWitnessV4,
        certification: RuntimeDrainCertificationResolutionV2,
    ) -> Result<RuntimeAuthorizedPreviousProcessDrainTeardownV4, RuntimePendingDrainV4Error> {
        let (authorization, source) = match self {
            Self::RouteAbsentClaimed {
                authorization: _,
                candidate: _,
            } => return Err(RuntimePendingDrainV4Error::LegacyRouteAbsentHandoffRequired),
            Self::RoutedClaimed {
                authorization,
                candidate,
            } => (
                authorization,
                RuntimePreviousProcessTeardownSourceV4::RoutedClaimed(candidate),
            ),
            Self::Refenced {
                authorization,
                candidate,
            } => (
                authorization,
                RuntimePreviousProcessTeardownSourceV4::Refenced(candidate),
            ),
        };
        validate_empty_succession_seal(source.common(), &seal)?;
        if let Some(retry_after) = source
            .common()
            .serving
            .retry_after(source.common().current_owner.expires_at)
        {
            return Err(RuntimePendingDrainV4Error::ServingLeaseFresh(retry_after));
        }
        validate_resolution(source.common(), &certification)?;
        validate_resolved_serving(source.common(), &certification)?;
        let action_identity = RuntimePendingDrainActionIdentityV4::successor(
            authorization.request().action_identity(),
            RuntimePendingDrainActionStageV4::PreviousProcessTeardown,
            1,
        )?;
        Ok(RuntimeAuthorizedPreviousProcessDrainTeardownV4 {
            authorization,
            source,
            seal,
            certification,
            action_identity,
        })
    }
}

impl Debug for RuntimeSelectedExpiredPreviousOwnerV4 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RuntimeSelectedExpiredPreviousOwnerV4(<redacted>)")
    }
}
