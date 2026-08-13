use std::collections::BTreeMap;
use std::num::NonZeroU64;

use automation_runtime_interaction::{InteractionReceiptIdentityV1, InteractionRequestDigestV1};
use automation_stateful_spec::StateValueV1;
use sha2::{Digest, Sha256};

use crate::state::{append_state_value, zeroize_state_value};
use crate::{
    OutboxDispatchAuthorityV1, PreparedStatefulCommitV1, ResolvedStateKeyV1, ResolvedStateReadV1,
    ResolvedStateWriteV1, StateDeclarationDigestV1, StateRowRevisionV1, StateSnapshotRequestV1,
    StateSnapshotV1, StatefulExecutionPlanDigestV1, StatefulOutboxPayloadV1,
};

const STATE_VALUE_DIGEST_DOMAIN_V1: &[u8] = b"starring.stateful_state_value.v1\0";
pub const MAX_OUTBOX_SCAN_LIMIT_V1: usize = 256;
pub const MAX_OUTBOX_CLAIM_MILLISECONDS_V1: u64 = 5 * 60 * 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtomicCommitDispositionV1 {
    Applied,
    ExactReplay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutboxStateV1 {
    Queued,
    Claimed,
    /// Terminal handoff in this non-integrated R0 scaffold. No business dispatcher can claim or
    /// requeue it. A future adapter must consume typed per-effect journal observation/recovery
    /// proof before exposing any transition out of this state.
    WaitingEffectRecovery,
    Completed,
    RecoveryRequired,
}

macro_rules! nonzero_revision {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(NonZeroU64);

        impl $name {
            fn first() -> Self {
                Self(NonZeroU64::new(1).expect("one is non-zero"))
            }

            fn next(self) -> Option<Self> {
                self.0
                    .get()
                    .checked_add(1)
                    .and_then(NonZeroU64::new)
                    .map(Self)
            }

            pub fn get(self) -> u64 {
                self.0.get()
            }
        }
    };
}

nonzero_revision!(OutboxHeadRevisionV1);
nonzero_revision!(OutboxClaimRevisionV1);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutboxClaimantIdV1(String);

impl OutboxClaimantIdV1 {
    pub fn parse(value: impl Into<String>) -> Result<Self, StatefulStoreErrorV1> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/')
            })
        {
            return Err(StatefulStoreErrorV1::InvalidInput);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Durable state transition ledger entry. Values are represented only by domain-separated
/// digests, keeping user text out of diagnostics and operational logs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateTransitionLedgerEntryV1 {
    receipt_identity: InteractionReceiptIdentityV1,
    transition_index: u16,
    state_action_node_id: String,
    source_ordinal: u16,
    key: ResolvedStateKeyV1,
    before_revision: Option<StateRowRevisionV1>,
    after_revision: StateRowRevisionV1,
    before_value_digest: String,
    after_value_digest: String,
    changed: bool,
}

impl StateTransitionLedgerEntryV1 {
    pub fn state_action_node_id(&self) -> &str {
        &self.state_action_node_id
    }

    pub fn source_ordinal(&self) -> u16 {
        self.source_ordinal
    }

    pub fn key(&self) -> &ResolvedStateKeyV1 {
        &self.key
    }

    pub fn before_revision(&self) -> Option<StateRowRevisionV1> {
        self.before_revision
    }

    pub fn after_revision(&self) -> StateRowRevisionV1 {
        self.after_revision
    }

    pub fn changed(&self) -> bool {
        self.changed
    }
}

/// A durable commit receipt has no `Debug`: it contains the full exact prepared request used for
/// replay-drift detection, including state and effect payload material.
#[derive(Clone, PartialEq, Eq)]
pub struct StatefulCommitReceiptV1 {
    prepared: PreparedStatefulCommitV1,
    transition_count: u16,
}

impl StatefulCommitReceiptV1 {
    pub fn receipt_identity(&self) -> InteractionReceiptIdentityV1 {
        self.prepared.receipt_identity()
    }

    pub fn request_digest(&self) -> &InteractionRequestDigestV1 {
        self.prepared.request_digest()
    }

    pub fn plan_digest(&self) -> &StatefulExecutionPlanDigestV1 {
        self.prepared.plan_digest()
    }

    pub fn transition_count(&self) -> u16 {
        self.transition_count
    }
}

/// Public outbox metadata deliberately excludes the private durable effect-plan bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxEntryMetadataV1 {
    receipt_identity: InteractionReceiptIdentityV1,
    plan_digest: String,
    payload_digest: String,
    state: OutboxStateV1,
    head_revision: OutboxHeadRevisionV1,
    claim_revision: Option<OutboxClaimRevisionV1>,
    available_at_ms: u64,
    acquired_at_ms: Option<u64>,
    expires_at_ms: Option<u64>,
}

impl OutboxEntryMetadataV1 {
    pub fn receipt_identity(&self) -> InteractionReceiptIdentityV1 {
        self.receipt_identity
    }

    pub fn state(&self) -> OutboxStateV1 {
        self.state
    }

    pub fn head_revision(&self) -> OutboxHeadRevisionV1 {
        self.head_revision
    }

    pub fn claim_revision(&self) -> Option<OutboxClaimRevisionV1> {
        self.claim_revision
    }

    pub fn available_at_ms(&self) -> u64 {
        self.available_at_ms
    }
}

/// Claim token binds one exact head/claim revision, claimant and route fence. A stale worker
/// cannot finish a successor claim using only a receipt identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxClaimTokenV1 {
    receipt_identity: InteractionReceiptIdentityV1,
    head_revision: OutboxHeadRevisionV1,
    claim_revision: OutboxClaimRevisionV1,
    claimant_id: OutboxClaimantIdV1,
    authority: OutboxDispatchAuthorityV1,
    acquired_at_ms: u64,
    expires_at_ms: u64,
}

impl OutboxClaimTokenV1 {
    pub fn receipt_identity(&self) -> InteractionReceiptIdentityV1 {
        self.receipt_identity
    }

    pub fn expires_at_ms(&self) -> u64 {
        self.expires_at_ms
    }

    pub fn head_revision(&self) -> OutboxHeadRevisionV1 {
        self.head_revision
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxClaimRequestV1 {
    pub receipt_identity: InteractionReceiptIdentityV1,
    pub expected_head_revision: OutboxHeadRevisionV1,
    pub claimant_id: OutboxClaimantIdV1,
    pub authority: OutboxDispatchAuthorityV1,
    pub now_ms: u64,
    pub lease_duration_ms: u64,
}

/// A successful claim includes the immutable private payload. It intentionally has no `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct ClaimedOutboxWorkV1 {
    token: OutboxClaimTokenV1,
    payload: StatefulOutboxPayloadV1,
}

impl ClaimedOutboxWorkV1 {
    pub fn token(&self) -> &OutboxClaimTokenV1 {
        &self.token
    }

    pub fn payload(&self) -> &StatefulOutboxPayloadV1 {
        &self.payload
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutboxReleaseReasonV1 {
    RetryableBeforeExternalIntent,
    LeaseAbandonedBeforeExternalIntent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryRequiredReasonV1 {
    ObservationUnavailable,
    CompensationUnavailable,
    OperatorDecisionRequired,
}

/// Atomic commit result has no `Debug` because its receipt retains full private prepared material.
#[derive(Clone, PartialEq, Eq)]
pub struct StatefulAtomicCommitResultV1 {
    disposition: AtomicCommitDispositionV1,
    receipt: StatefulCommitReceiptV1,
    outbox: OutboxEntryMetadataV1,
}

impl StatefulAtomicCommitResultV1 {
    pub fn disposition(&self) -> AtomicCommitDispositionV1 {
        self.disposition
    }

    pub fn receipt(&self) -> &StatefulCommitReceiptV1 {
        &self.receipt
    }

    pub fn outbox(&self) -> &OutboxEntryMetadataV1 {
        &self.outbox
    }
}

#[derive(Clone, PartialEq, Eq)]
struct StoredStateV1 {
    value: StateValueV1,
    revision: StateRowRevisionV1,
    declaration_digest: StateDeclarationDigestV1,
}

impl Drop for StoredStateV1 {
    fn drop(&mut self) {
        zeroize_state_value(&mut self.value);
    }
}

#[derive(Clone, PartialEq, Eq)]
struct StoredOutboxEntryV1 {
    receipt_identity: InteractionReceiptIdentityV1,
    request_digest: InteractionRequestDigestV1,
    plan_digest: StatefulExecutionPlanDigestV1,
    payload: StatefulOutboxPayloadV1,
    required_authority: OutboxDispatchAuthorityV1,
    state: OutboxStateV1,
    head_revision: OutboxHeadRevisionV1,
    claim_revision: Option<OutboxClaimRevisionV1>,
    claimant_id: Option<OutboxClaimantIdV1>,
    acquired_at_ms: Option<u64>,
    expires_at_ms: Option<u64>,
    available_at_ms: u64,
    external_intent_started: bool,
}

impl StoredOutboxEntryV1 {
    fn metadata(&self) -> OutboxEntryMetadataV1 {
        OutboxEntryMetadataV1 {
            receipt_identity: self.receipt_identity,
            plan_digest: self.plan_digest.as_str().to_string(),
            payload_digest: self.payload.digest().as_str().to_string(),
            state: self.state,
            head_revision: self.head_revision,
            claim_revision: self.claim_revision,
            available_at_ms: self.available_at_ms,
            acquired_at_ms: self.acquired_at_ms,
            expires_at_ms: self.expires_at_ms,
        }
    }
}

#[derive(Default)]
pub struct InMemoryAtomicStateOutboxStoreV1 {
    state: BTreeMap<ResolvedStateKeyV1, StoredStateV1>,
    receipts: BTreeMap<InteractionReceiptIdentityV1, StatefulCommitReceiptV1>,
    outbox: BTreeMap<InteractionReceiptIdentityV1, StoredOutboxEntryV1>,
    transitions: Vec<StateTransitionLedgerEntryV1>,
}

impl InMemoryAtomicStateOutboxStoreV1 {
    pub fn read_snapshot_v1(
        &self,
        request: &StateSnapshotRequestV1,
    ) -> Result<StateSnapshotV1, StatefulStoreErrorV1> {
        let reads = request
            .entries()
            .map(
                |(_, key, declaration_digest, default_value)| match self.state.get(key) {
                    None => Ok(ResolvedStateReadV1::from_snapshot(
                        key.clone(),
                        None,
                        declaration_digest.clone(),
                        default_value.clone(),
                    )),
                    Some(stored) if stored.declaration_digest == *declaration_digest => {
                        Ok(ResolvedStateReadV1::from_snapshot(
                            key.clone(),
                            Some(stored.revision),
                            declaration_digest.clone(),
                            stored.value.clone(),
                        ))
                    }
                    Some(_) => Err(StatefulStoreErrorV1::DefinitionMismatch),
                },
            )
            .collect::<Result<Vec<_>, _>>()?;
        Ok(StateSnapshotV1::from_request(request, reads))
    }

    pub fn state_value_v1(
        &self,
        key: &ResolvedStateKeyV1,
    ) -> Option<(StateRowRevisionV1, &StateValueV1, &StateDeclarationDigestV1)> {
        self.state
            .get(key)
            .map(|stored| (stored.revision, &stored.value, &stored.declaration_digest))
    }

    pub fn atomic_commit_v1(
        &mut self,
        prepared: PreparedStatefulCommitV1,
    ) -> Result<StatefulAtomicCommitResultV1, StatefulStoreErrorV1> {
        if !prepared.verify() {
            return Err(StatefulStoreErrorV1::InvalidInput);
        }
        if let Some(receipt) = self.receipts.get(&prepared.receipt_identity()) {
            let outbox = self
                .outbox
                .get(&prepared.receipt_identity())
                .ok_or(StatefulStoreErrorV1::Corrupt)?;
            if receipt.prepared == prepared {
                return Ok(StatefulAtomicCommitResultV1 {
                    disposition: AtomicCommitDispositionV1::ExactReplay,
                    receipt: receipt.clone(),
                    outbox: outbox.metadata(),
                });
            }
            return Err(StatefulStoreErrorV1::ReceiptConflict);
        }

        for read in prepared.reads() {
            match (self.state.get(read.key()), read.revision()) {
                (None, None) => {}
                (Some(stored), Some(revision))
                    if stored.revision == revision
                        && stored.declaration_digest == *read.declaration_digest()
                        && stored.value == *read.value() => {}
                _ => return Err(StatefulStoreErrorV1::StateConflict),
            }
        }

        let mut next_state = self.state.clone();
        let mut next_receipts = self.receipts.clone();
        let mut next_outbox = self.outbox.clone();
        let mut next_transitions = self.transitions.clone();
        for (index, write) in prepared.writes().iter().enumerate() {
            let after_revision = match write.expected_revision() {
                None => StateRowRevisionV1::new(1).expect("one is non-zero"),
                Some(revision) => revision
                    .next()
                    .ok_or(StatefulStoreErrorV1::RevisionExhausted)?,
            };
            next_state.insert(
                write.key().clone(),
                StoredStateV1 {
                    value: write.after().clone(),
                    revision: after_revision,
                    declaration_digest: write.declaration_digest().clone(),
                },
            );
            next_transitions.push(transition_entry(
                prepared.receipt_identity(),
                index,
                write,
                after_revision,
            )?);
        }
        let receipt = StatefulCommitReceiptV1 {
            prepared: prepared.clone(),
            transition_count: u16::try_from(prepared.writes().len())
                .map_err(|_| StatefulStoreErrorV1::InvalidInput)?,
        };
        let outbox = StoredOutboxEntryV1 {
            receipt_identity: prepared.receipt_identity(),
            request_digest: prepared.request_digest().clone(),
            plan_digest: prepared.plan_digest().clone(),
            payload: prepared.outbox_payload().clone(),
            required_authority: prepared.dispatch_authority().clone(),
            state: OutboxStateV1::Queued,
            head_revision: OutboxHeadRevisionV1::first(),
            claim_revision: None,
            claimant_id: None,
            acquired_at_ms: None,
            expires_at_ms: None,
            available_at_ms: 0,
            external_intent_started: false,
        };
        next_receipts.insert(receipt.receipt_identity(), receipt.clone());
        next_outbox.insert(outbox.receipt_identity, outbox.clone());

        // The reference implementation swaps every collection only after all validation and
        // fallible construction succeeds, modeling a single database transaction.
        self.state = next_state;
        self.receipts = next_receipts;
        self.outbox = next_outbox;
        self.transitions = next_transitions;
        Ok(StatefulAtomicCommitResultV1 {
            disposition: AtomicCommitDispositionV1::Applied,
            receipt,
            outbox: outbox.metadata(),
        })
    }

    pub fn due_outbox_v1(
        &self,
        now_ms: u64,
        limit: usize,
    ) -> Result<Vec<OutboxEntryMetadataV1>, StatefulStoreErrorV1> {
        if limit == 0 || limit > MAX_OUTBOX_SCAN_LIMIT_V1 {
            return Err(StatefulStoreErrorV1::InvalidInput);
        }
        Ok(self
            .outbox
            .values()
            .filter(|entry| {
                (entry.state == OutboxStateV1::Queued && entry.available_at_ms <= now_ms)
                    || (entry.state == OutboxStateV1::Claimed
                        && entry.expires_at_ms.is_some_and(|expiry| expiry <= now_ms))
            })
            .take(limit)
            .map(StoredOutboxEntryV1::metadata)
            .collect())
    }

    pub fn claim_outbox_v1(
        &mut self,
        request: OutboxClaimRequestV1,
    ) -> Result<ClaimedOutboxWorkV1, StatefulStoreErrorV1> {
        if request.lease_duration_ms == 0
            || request.lease_duration_ms > MAX_OUTBOX_CLAIM_MILLISECONDS_V1
        {
            return Err(StatefulStoreErrorV1::InvalidInput);
        }
        let entry = self
            .outbox
            .get_mut(&request.receipt_identity)
            .ok_or(StatefulStoreErrorV1::NotFound)?;
        if request.authority != entry.required_authority {
            return Err(StatefulStoreErrorV1::AuthorityMismatch);
        }
        if request.expected_head_revision != entry.head_revision {
            return Err(StatefulStoreErrorV1::StaleClaim);
        }
        let reclaiming = entry.state == OutboxStateV1::Claimed
            && entry
                .expires_at_ms
                .is_some_and(|expiry| request.now_ms >= expiry);
        if !((entry.state == OutboxStateV1::Queued && request.now_ms >= entry.available_at_ms)
            || reclaiming)
        {
            return Err(StatefulStoreErrorV1::OutboxConflict);
        }
        if reclaiming && entry.external_intent_started {
            return Err(StatefulStoreErrorV1::RecoveryRequired);
        }
        let expires_at_ms = request
            .now_ms
            .checked_add(request.lease_duration_ms)
            .ok_or(StatefulStoreErrorV1::InvalidInput)?;
        entry.head_revision = entry
            .head_revision
            .next()
            .ok_or(StatefulStoreErrorV1::RevisionExhausted)?;
        let claim_revision = match entry.claim_revision {
            None => OutboxClaimRevisionV1::first(),
            Some(revision) => revision
                .next()
                .ok_or(StatefulStoreErrorV1::RevisionExhausted)?,
        };
        entry.claim_revision = Some(claim_revision);
        entry.state = OutboxStateV1::Claimed;
        entry.claimant_id = Some(request.claimant_id.clone());
        entry.acquired_at_ms = Some(request.now_ms);
        entry.expires_at_ms = Some(expires_at_ms);
        entry.external_intent_started = false;
        let token = OutboxClaimTokenV1 {
            receipt_identity: entry.receipt_identity,
            head_revision: entry.head_revision,
            claim_revision,
            claimant_id: request.claimant_id,
            authority: request.authority,
            acquired_at_ms: request.now_ms,
            expires_at_ms,
        };
        Ok(ClaimedOutboxWorkV1 {
            token,
            payload: entry.payload.clone(),
        })
    }

    pub fn record_external_intent_v1(
        &mut self,
        token: &OutboxClaimTokenV1,
        now_ms: u64,
    ) -> Result<(), StatefulStoreErrorV1> {
        // This whole-payload marker only separates safe pre-intent release from terminal recovery
        // handoff in R0. It is deliberately insufficient for live completion; future integration
        // must replace it with typed per-effect intent/result progress from the effect journal.
        let entry = self.valid_claim_mut(token, now_ms)?;
        entry.external_intent_started = true;
        Ok(())
    }

    pub fn release_before_external_intent_v1(
        &mut self,
        token: &OutboxClaimTokenV1,
        now_ms: u64,
        available_at_ms: u64,
        _reason: OutboxReleaseReasonV1,
    ) -> Result<OutboxEntryMetadataV1, StatefulStoreErrorV1> {
        let entry = self.valid_claim_mut(token, now_ms)?;
        if entry.external_intent_started || available_at_ms < now_ms {
            return Err(StatefulStoreErrorV1::OutboxConflict);
        }
        reset_claim(entry, OutboxStateV1::Queued, available_at_ms)?;
        Ok(entry.metadata())
    }

    pub fn wait_for_effect_recovery_v1(
        &mut self,
        token: &OutboxClaimTokenV1,
        now_ms: u64,
        available_at_ms: u64,
    ) -> Result<OutboxEntryMetadataV1, StatefulStoreErrorV1> {
        // WaitingEffectRecovery is terminal in this reference scaffold. There is intentionally no
        // generic resume/requeue API that could blindly repeat an indeterminate external effect.
        let entry = self.valid_claim_mut(token, now_ms)?;
        if !entry.external_intent_started || available_at_ms < now_ms {
            return Err(StatefulStoreErrorV1::OutboxConflict);
        }
        reset_claim(entry, OutboxStateV1::WaitingEffectRecovery, available_at_ms)?;
        Ok(entry.metadata())
    }

    pub fn expire_claim_v1(
        &mut self,
        receipt_identity: InteractionReceiptIdentityV1,
        expected_head_revision: OutboxHeadRevisionV1,
        authority: &OutboxDispatchAuthorityV1,
        now_ms: u64,
        recovery_available_at_ms: u64,
    ) -> Result<OutboxEntryMetadataV1, StatefulStoreErrorV1> {
        let entry = self
            .outbox
            .get_mut(&receipt_identity)
            .ok_or(StatefulStoreErrorV1::NotFound)?;
        if &entry.required_authority != authority {
            return Err(StatefulStoreErrorV1::AuthorityMismatch);
        }
        if entry.state != OutboxStateV1::Claimed
            || entry.head_revision != expected_head_revision
            || !entry.expires_at_ms.is_some_and(|expiry| now_ms >= expiry)
            || recovery_available_at_ms < now_ms
        {
            return Err(StatefulStoreErrorV1::OutboxConflict);
        }
        let next_state = if entry.external_intent_started {
            OutboxStateV1::WaitingEffectRecovery
        } else {
            OutboxStateV1::Queued
        };
        reset_claim(entry, next_state, recovery_available_at_ms)?;
        Ok(entry.metadata())
    }

    pub fn complete_outbox_v1(
        &mut self,
        token: &OutboxClaimTokenV1,
        now_ms: u64,
    ) -> Result<OutboxEntryMetadataV1, StatefulStoreErrorV1> {
        let entry = self.valid_claim_mut(token, now_ms)?;
        if !entry.external_intent_started {
            return Err(StatefulStoreErrorV1::OutboxConflict);
        }
        reset_claim(entry, OutboxStateV1::Completed, now_ms)?;
        Ok(entry.metadata())
    }

    pub fn require_recovery_v1(
        &mut self,
        token: &OutboxClaimTokenV1,
        now_ms: u64,
        _reason: RecoveryRequiredReasonV1,
    ) -> Result<OutboxEntryMetadataV1, StatefulStoreErrorV1> {
        let entry = self.valid_claim_mut(token, now_ms)?;
        if !entry.external_intent_started {
            return Err(StatefulStoreErrorV1::OutboxConflict);
        }
        reset_claim(entry, OutboxStateV1::RecoveryRequired, now_ms)?;
        Ok(entry.metadata())
    }

    pub fn state_transition_ledger(&self) -> &[StateTransitionLedgerEntryV1] {
        &self.transitions
    }

    fn valid_claim_mut(
        &mut self,
        token: &OutboxClaimTokenV1,
        now_ms: u64,
    ) -> Result<&mut StoredOutboxEntryV1, StatefulStoreErrorV1> {
        let entry = self
            .outbox
            .get_mut(&token.receipt_identity)
            .ok_or(StatefulStoreErrorV1::NotFound)?;
        if entry.state != OutboxStateV1::Claimed
            || entry.head_revision != token.head_revision
            || entry.claim_revision != Some(token.claim_revision)
            || entry.claimant_id.as_ref() != Some(&token.claimant_id)
            || entry.required_authority != token.authority
            || entry.acquired_at_ms != Some(token.acquired_at_ms)
            || entry.expires_at_ms != Some(token.expires_at_ms)
        {
            return Err(StatefulStoreErrorV1::StaleClaim);
        }
        if now_ms < token.acquired_at_ms || now_ms >= token.expires_at_ms {
            return Err(StatefulStoreErrorV1::ClaimExpired);
        }
        Ok(entry)
    }

    #[cfg(test)]
    pub(crate) fn stored_payload(
        &self,
        identity: InteractionReceiptIdentityV1,
    ) -> Option<&StatefulOutboxPayloadV1> {
        self.outbox.get(&identity).map(|entry| &entry.payload)
    }
}

fn reset_claim(
    entry: &mut StoredOutboxEntryV1,
    state: OutboxStateV1,
    available_at_ms: u64,
) -> Result<(), StatefulStoreErrorV1> {
    entry.head_revision = entry
        .head_revision
        .next()
        .ok_or(StatefulStoreErrorV1::RevisionExhausted)?;
    entry.state = state;
    entry.claimant_id = None;
    entry.acquired_at_ms = None;
    entry.expires_at_ms = None;
    entry.available_at_ms = available_at_ms;
    Ok(())
}

fn transition_entry(
    receipt_identity: InteractionReceiptIdentityV1,
    index: usize,
    write: &ResolvedStateWriteV1,
    after_revision: StateRowRevisionV1,
) -> Result<StateTransitionLedgerEntryV1, StatefulStoreErrorV1> {
    Ok(StateTransitionLedgerEntryV1 {
        receipt_identity,
        transition_index: u16::try_from(index).map_err(|_| StatefulStoreErrorV1::InvalidInput)?,
        state_action_node_id: write.state_action_node_id().to_string(),
        source_ordinal: write.source_ordinal(),
        key: write.key().clone(),
        before_revision: write.expected_revision(),
        after_revision,
        before_value_digest: state_value_digest(write.before()),
        after_value_digest: state_value_digest(write.after()),
        changed: write.before() != write.after(),
    })
}

fn state_value_digest(value: &StateValueV1) -> String {
    let mut hasher = Sha256::new();
    hasher.update(STATE_VALUE_DIGEST_DOMAIN_V1);
    append_state_value(&mut hasher, value);
    lower_hex(hasher.finalize().as_slice())
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StatefulStoreErrorV1 {
    #[error("stateful store input is invalid")]
    InvalidInput,
    #[error("stateful receipt conflicts with an earlier exact plan")]
    ReceiptConflict,
    #[error("stateful read set changed before atomic commit")]
    StateConflict,
    #[error("stored state definition differs from the compiled declaration")]
    DefinitionMismatch,
    #[error("stateful store is internally inconsistent")]
    Corrupt,
    #[error("stateful revision is exhausted")]
    RevisionExhausted,
    #[error("stateful outbox entry was not found")]
    NotFound,
    #[error("stateful outbox transition conflicts with its current state")]
    OutboxConflict,
    #[error("stateful outbox claimant authority does not match the durable route fence")]
    AuthorityMismatch,
    #[error("stateful outbox claim token is stale")]
    StaleClaim,
    #[error("stateful outbox claim has expired")]
    ClaimExpired,
    #[error("expired work has an external intent and requires effect recovery")]
    RecoveryRequired,
}
