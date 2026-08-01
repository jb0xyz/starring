use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::sync::broadcast;

use crate::Config;

const MAX_OWNED_IDENTITIES: usize = 128;
const ARM_LIFETIME: Duration = Duration::from_secs(60);

pub struct SharedState {
    run_id: String,
    guild_id: String,
    actor_id: String,
    bot_user_id: String,
    instance_id: String,
    partitioned: AtomicBool,
    gateway_listener_ready: AtomicBool,
    effect_http_listener_ready: AtomicBool,
    inner: Mutex<StateInner>,
    partition_tx: broadcast::Sender<()>,
}

struct StateInner {
    gateway_connections: u64,
    gateway_ready_rewrites: u64,
    gateway_partition_events: u64,
    gateway_identity_rejections: u64,
    duplicate_arm: Option<TimedArm>,
    duplicate_claim: Option<DuplicateClaimState>,
    duplicate_claim_deliveries: u8,
    duplicate_injections: u64,
    duplicate_failed_attempts: u64,
    last_failed_duplicate_operation_id: Option<String>,
    duplicate_delivery_count: u64,
    last_duplicate_interaction_id: Option<String>,
    last_duplicate_operation_id: Option<String>,
    forwarded_http_requests: u64,
    rejected_http_requests: u64,
    indeterminate_arm: Option<TimedArm>,
    indeterminate_claim: Option<IndeterminateTarget>,
    indeterminate_injections: u64,
    last_indeterminate_audit_reason_sha256: Option<String>,
    last_indeterminate_operation_id: Option<String>,
    last_indeterminate_upstream_status: Option<u16>,
    owned_role_ids: BTreeSet<String>,
    owned_channel_ids: BTreeSet<String>,
    owned_message_ids: BTreeSet<(String, String)>,
    pending_role_slots: usize,
    pending_channel_slots: usize,
    pending_message_slots: usize,
}

struct IndeterminateTarget {
    operation_id: String,
    audit_reason_sha256: String,
}

struct TimedArm {
    operation_id: String,
    deadline: Instant,
}

struct DuplicateClaimState {
    operation_id: String,
    interaction_id: String,
}

pub struct IndeterminateClaim {
    audit_reason_sha256: String,
}

pub struct DuplicateClaim {
    operation_id: String,
    interaction_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmOutcome {
    Armed,
    Replayed,
    Busy,
}

#[derive(Debug, Error)]
#[error("instance_identity_unavailable")]
pub struct StateError;

pub enum ResourceKind {
    Role,
    Channel,
    Message { channel_id: String },
}

pub struct ResourceReservation {
    state: ArcState,
    kind: Option<ResourceKind>,
}

type ArcState = std::sync::Arc<SharedState>;

#[derive(Clone, Serialize)]
pub struct Snapshot {
    version: u8,
    ready: bool,
    instance_id: String,
    run_id: String,
    guild_id: String,
    actor_id: String,
    bot_user_id: String,
    gateway: GatewaySnapshot,
    effect_http: EffectHttpSnapshot,
}

#[derive(Clone, Serialize)]
struct GatewaySnapshot {
    partitioned: bool,
    connections: u64,
    ready_rewrites: u64,
    partition_events: u64,
    identity_rejections: u64,
    duplicate_armed: bool,
    armed_duplicate_operation_id: Option<String>,
    duplicate_claimed: bool,
    claimed_duplicate_operation_id: Option<String>,
    duplicate_injections: u64,
    duplicate_failed_attempts: u64,
    last_failed_duplicate_operation_id: Option<String>,
    duplicate_delivery_count: u64,
    last_duplicate_interaction_id: Option<String>,
    last_duplicate_operation_id: Option<String>,
}

#[derive(Clone, Serialize)]
struct EffectHttpSnapshot {
    forwarded_requests: u64,
    rejected_requests: u64,
    indeterminate_armed: bool,
    armed_indeterminate_operation_id: Option<String>,
    indeterminate_claimed: bool,
    claimed_indeterminate_operation_id: Option<String>,
    indeterminate_injections: u64,
    last_indeterminate_audit_reason_sha256: Option<String>,
    last_indeterminate_operation_id: Option<String>,
    last_indeterminate_upstream_status: Option<u16>,
    owned_role_count: usize,
    owned_channel_count: usize,
    owned_message_count: usize,
}

impl SharedState {
    pub fn new(config: &Config) -> Result<Self, StateError> {
        let (partition_tx, _) = broadcast::channel(8);
        let instance_id = process_instance_id()?;
        Ok(Self {
            run_id: config.run_id().to_owned(),
            guild_id: config.guild_id().to_owned(),
            actor_id: config.actor_id().to_owned(),
            bot_user_id: config.bot_user_id().to_owned(),
            instance_id,
            partitioned: AtomicBool::new(false),
            gateway_listener_ready: AtomicBool::new(false),
            effect_http_listener_ready: AtomicBool::new(false),
            inner: Mutex::new(StateInner {
                gateway_connections: 0,
                gateway_ready_rewrites: 0,
                gateway_partition_events: 0,
                gateway_identity_rejections: 0,
                duplicate_arm: None,
                duplicate_claim: None,
                duplicate_claim_deliveries: 0,
                duplicate_injections: 0,
                duplicate_failed_attempts: 0,
                last_failed_duplicate_operation_id: None,
                duplicate_delivery_count: 0,
                last_duplicate_interaction_id: None,
                last_duplicate_operation_id: None,
                forwarded_http_requests: 0,
                rejected_http_requests: 0,
                indeterminate_arm: None,
                indeterminate_claim: None,
                indeterminate_injections: 0,
                last_indeterminate_audit_reason_sha256: None,
                last_indeterminate_operation_id: None,
                last_indeterminate_upstream_status: None,
                owned_role_ids: BTreeSet::new(),
                owned_channel_ids: BTreeSet::new(),
                owned_message_ids: BTreeSet::new(),
                pending_role_slots: 0,
                pending_channel_slots: 0,
                pending_message_slots: 0,
            }),
            partition_tx,
        })
    }

    pub fn identities_match(
        &self,
        run_id: &str,
        guild_id: &str,
        actor_id: &str,
        bot_user_id: &str,
    ) -> bool {
        self.run_id == run_id
            && self.guild_id == guild_id
            && self.actor_id == actor_id
            && self.bot_user_id == bot_user_id
    }

    pub fn is_partitioned(&self) -> bool {
        self.partitioned.load(Ordering::Acquire)
    }

    pub fn subscribe_partition(&self) -> broadcast::Receiver<()> {
        self.partition_tx.subscribe()
    }

    pub fn partition(&self) -> bool {
        if self.partitioned.swap(true, Ordering::AcqRel) {
            return false;
        }
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        inner.gateway_partition_events = inner.gateway_partition_events.saturating_add(1);
        drop(inner);
        let _ = self.partition_tx.send(());
        true
    }

    pub fn heal(&self) -> bool {
        self.partitioned.swap(false, Ordering::AcqRel)
    }

    pub fn record_gateway_connection(&self) {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        inner.gateway_connections = inner.gateway_connections.saturating_add(1);
    }

    pub fn mark_gateway_listener_ready(&self) {
        self.gateway_listener_ready.store(true, Ordering::Release);
    }

    pub fn mark_effect_http_listener_ready(&self) {
        self.effect_http_listener_ready
            .store(true, Ordering::Release);
    }

    pub fn record_ready_rewrite(&self) {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        inner.gateway_ready_rewrites = inner.gateway_ready_rewrites.saturating_add(1);
    }

    pub fn record_gateway_identity_rejection(&self) {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        inner.gateway_identity_rejections = inner.gateway_identity_rejections.saturating_add(1);
    }

    pub fn arm_next_duplicate(&self, operation_id: &str) -> ArmOutcome {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        let now = Instant::now();
        if !valid_operation_id(operation_id) {
            return ArmOutcome::Busy;
        }
        if inner.last_duplicate_operation_id.as_deref() == Some(operation_id) {
            return ArmOutcome::Replayed;
        }
        if inner.last_failed_duplicate_operation_id.as_deref() == Some(operation_id) {
            return ArmOutcome::Replayed;
        }
        if inner.duplicate_injections > 0 || inner.duplicate_failed_attempts > 0 {
            return ArmOutcome::Busy;
        }
        if let Some(claim) = inner.duplicate_claim.as_ref() {
            return if claim.operation_id == operation_id {
                ArmOutcome::Replayed
            } else {
                ArmOutcome::Busy
            };
        }
        if let Some(arm) = inner
            .duplicate_arm
            .as_ref()
            .filter(|arm| arm.deadline > now)
        {
            return if arm.operation_id == operation_id {
                ArmOutcome::Replayed
            } else {
                ArmOutcome::Busy
            };
        }
        let Some(deadline) = now.checked_add(ARM_LIFETIME) else {
            return ArmOutcome::Busy;
        };
        inner.duplicate_arm = Some(TimedArm {
            operation_id: operation_id.to_owned(),
            deadline,
        });
        ArmOutcome::Armed
    }

    pub fn disarm_duplicate(&self) -> bool {
        self.inner
            .lock()
            .expect("state mutex poisoned")
            .duplicate_arm
            .take()
            .is_some()
    }

    pub fn claim_duplicate(
        &self,
        interaction_id: &str,
        guild_id: &str,
        actor_id: &str,
    ) -> Option<DuplicateClaim> {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        let now = Instant::now();
        if inner
            .duplicate_arm
            .as_ref()
            .is_none_or(|arm| arm.deadline <= now)
        {
            inner.duplicate_arm = None;
            return None;
        }
        if self.guild_id != guild_id
            || self.actor_id != actor_id
            || !crate::config::valid_snowflake(interaction_id)
        {
            inner.gateway_identity_rejections = inner.gateway_identity_rejections.saturating_add(1);
            return None;
        }
        let operation_id = inner
            .duplicate_arm
            .take()
            .expect("active duplicate arm")
            .operation_id;
        inner.duplicate_claim = Some(DuplicateClaimState {
            operation_id: operation_id.clone(),
            interaction_id: interaction_id.to_owned(),
        });
        inner.duplicate_claim_deliveries = 0;
        Some(DuplicateClaim {
            operation_id,
            interaction_id: interaction_id.to_owned(),
        })
    }

    pub fn record_duplicate_delivery(&self, claim: &DuplicateClaim) -> bool {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        if inner.duplicate_claim.as_ref().is_none_or(|active| {
            active.interaction_id != claim.interaction_id
                || active.operation_id != claim.operation_id
        }) || inner.duplicate_claim_deliveries >= 2
        {
            return false;
        }
        inner.duplicate_claim_deliveries += 1;
        inner.duplicate_delivery_count = inner.duplicate_delivery_count.saturating_add(1);
        true
    }

    pub fn finish_duplicate(&self, claim: DuplicateClaim) -> bool {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        if inner.duplicate_claim.as_ref().is_none_or(|active| {
            active.interaction_id != claim.interaction_id
                || active.operation_id != claim.operation_id
        }) || inner.duplicate_claim_deliveries != 2
        {
            return false;
        }
        inner.duplicate_claim = None;
        inner.duplicate_claim_deliveries = 0;
        inner.duplicate_injections = inner.duplicate_injections.saturating_add(1);
        inner.last_duplicate_operation_id = Some(claim.operation_id);
        inner.last_duplicate_interaction_id = Some(claim.interaction_id);
        true
    }

    pub fn abort_duplicate(&self, claim: DuplicateClaim) -> bool {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        if inner.duplicate_claim.as_ref().is_none_or(|active| {
            active.interaction_id != claim.interaction_id
                || active.operation_id != claim.operation_id
        }) {
            return false;
        }
        inner.duplicate_claim = None;
        inner.duplicate_claim_deliveries = 0;
        inner.duplicate_failed_attempts = inner.duplicate_failed_attempts.saturating_add(1);
        inner.last_failed_duplicate_operation_id = Some(claim.operation_id);
        true
    }

    pub fn arm_next_indeterminate(&self, operation_id: &str) -> ArmOutcome {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        let now = Instant::now();
        if !valid_operation_id(operation_id) {
            return ArmOutcome::Busy;
        }
        if inner.last_indeterminate_operation_id.as_deref() == Some(operation_id) {
            return ArmOutcome::Replayed;
        }
        if inner.indeterminate_injections > 0 {
            return ArmOutcome::Busy;
        }
        if let Some(claim) = inner.indeterminate_claim.as_ref() {
            return if claim.operation_id == operation_id {
                ArmOutcome::Replayed
            } else {
                ArmOutcome::Busy
            };
        }
        if let Some(arm) = inner
            .indeterminate_arm
            .as_ref()
            .filter(|arm| arm.deadline > now)
        {
            return if arm.operation_id == operation_id {
                ArmOutcome::Replayed
            } else {
                ArmOutcome::Busy
            };
        }
        let Some(deadline) = now.checked_add(ARM_LIFETIME) else {
            return ArmOutcome::Busy;
        };
        inner.indeterminate_arm = Some(TimedArm {
            operation_id: operation_id.to_owned(),
            deadline,
        });
        ArmOutcome::Armed
    }

    pub fn disarm_indeterminate(&self) -> bool {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        if inner.indeterminate_claim.is_some() {
            return false;
        }
        inner.indeterminate_arm.take().is_some()
    }

    pub fn claim_indeterminate(&self, audit_reason: &str) -> Option<IndeterminateClaim> {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        let now = Instant::now();
        if inner
            .indeterminate_arm
            .as_ref()
            .is_none_or(|arm| arm.deadline <= now)
        {
            inner.indeterminate_arm = None;
            return None;
        }
        if !valid_audit_reason(audit_reason) || inner.indeterminate_claim.is_some() {
            return None;
        }
        let operation_id = inner
            .indeterminate_arm
            .take()
            .expect("active indeterminate arm")
            .operation_id;
        let digest = hex_sha256(audit_reason.as_bytes());
        let target = IndeterminateTarget {
            operation_id,
            audit_reason_sha256: digest.clone(),
        };
        let claim = IndeterminateClaim {
            audit_reason_sha256: digest,
        };
        inner.indeterminate_claim = Some(target);
        Some(claim)
    }

    pub fn finish_indeterminate(&self, claim: IndeterminateClaim, status: Option<u16>) -> bool {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        let Some(target) = inner.indeterminate_claim.take() else {
            return false;
        };
        if target.audit_reason_sha256 != claim.audit_reason_sha256 {
            inner.indeterminate_arm = rearm(&target.operation_id);
            return false;
        }
        if status.is_some_and(|status| (200..300).contains(&status)) {
            inner.indeterminate_injections = inner.indeterminate_injections.saturating_add(1);
            inner.last_indeterminate_operation_id = Some(target.operation_id);
            inner.last_indeterminate_audit_reason_sha256 = Some(claim.audit_reason_sha256);
            inner.last_indeterminate_upstream_status = status;
            true
        } else {
            inner.indeterminate_arm = rearm(&target.operation_id);
            false
        }
    }

    pub fn record_forwarded_http(&self) {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        inner.forwarded_http_requests = inner.forwarded_http_requests.saturating_add(1);
    }

    pub fn record_rejected_http(&self) {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        inner.rejected_http_requests = inner.rejected_http_requests.saturating_add(1);
    }

    pub fn reserve_resource(
        self: &std::sync::Arc<Self>,
        kind: ResourceKind,
    ) -> Option<ResourceReservation> {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        let available = match &kind {
            ResourceKind::Role => {
                inner.owned_role_ids.len() + inner.pending_role_slots < MAX_OWNED_IDENTITIES
            }
            ResourceKind::Channel => {
                inner.owned_channel_ids.len() + inner.pending_channel_slots < MAX_OWNED_IDENTITIES
            }
            ResourceKind::Message { channel_id } => {
                inner.owned_channel_ids.contains(channel_id)
                    && inner.owned_message_ids.len() + inner.pending_message_slots
                        < MAX_OWNED_IDENTITIES
            }
        };
        if !available {
            return None;
        }
        match &kind {
            ResourceKind::Role => inner.pending_role_slots += 1,
            ResourceKind::Channel => inner.pending_channel_slots += 1,
            ResourceKind::Message { .. } => inner.pending_message_slots += 1,
        }
        drop(inner);
        Some(ResourceReservation {
            state: std::sync::Arc::clone(self),
            kind: Some(kind),
        })
    }

    pub fn owns_role(&self, id: &str) -> bool {
        self.inner
            .lock()
            .expect("state mutex poisoned")
            .owned_role_ids
            .contains(id)
    }

    pub fn owns_channel(&self, id: &str) -> bool {
        self.inner
            .lock()
            .expect("state mutex poisoned")
            .owned_channel_ids
            .contains(id)
    }

    pub fn owns_message(&self, channel_id: &str, id: &str) -> bool {
        self.inner
            .lock()
            .expect("state mutex poisoned")
            .owned_message_ids
            .contains(&(channel_id.to_owned(), id.to_owned()))
    }

    pub fn remove_role(&self, id: &str) -> bool {
        self.inner
            .lock()
            .expect("state mutex poisoned")
            .owned_role_ids
            .remove(id)
    }

    pub fn remove_channel(&self, id: &str) -> bool {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        let removed = inner.owned_channel_ids.remove(id);
        inner
            .owned_message_ids
            .retain(|(channel_id, _)| channel_id != id);
        removed
    }

    pub fn remove_message(&self, channel_id: &str, id: &str) -> bool {
        self.inner
            .lock()
            .expect("state mutex poisoned")
            .owned_message_ids
            .remove(&(channel_id.to_owned(), id.to_owned()))
    }

    fn finish_resource_reservation(&self, kind: ResourceKind, id: Option<String>) -> bool {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        match kind {
            ResourceKind::Role => {
                inner.pending_role_slots = inner.pending_role_slots.saturating_sub(1);
                id.is_none_or(|id| inner.owned_role_ids.insert(id))
            }
            ResourceKind::Channel => {
                inner.pending_channel_slots = inner.pending_channel_slots.saturating_sub(1);
                id.is_none_or(|id| inner.owned_channel_ids.insert(id))
            }
            ResourceKind::Message { channel_id } => {
                inner.pending_message_slots = inner.pending_message_slots.saturating_sub(1);
                id.is_none_or(|id| inner.owned_message_ids.insert((channel_id, id)))
            }
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let inner = self.inner.lock().expect("state mutex poisoned");
        let now = Instant::now();
        Snapshot {
            version: 1,
            ready: self.gateway_listener_ready.load(Ordering::Acquire)
                && self.effect_http_listener_ready.load(Ordering::Acquire),
            instance_id: self.instance_id.clone(),
            run_id: self.run_id.clone(),
            guild_id: self.guild_id.clone(),
            actor_id: self.actor_id.clone(),
            bot_user_id: self.bot_user_id.clone(),
            gateway: GatewaySnapshot {
                partitioned: self.is_partitioned(),
                connections: inner.gateway_connections,
                ready_rewrites: inner.gateway_ready_rewrites,
                partition_events: inner.gateway_partition_events,
                identity_rejections: inner.gateway_identity_rejections,
                duplicate_armed: inner
                    .duplicate_arm
                    .as_ref()
                    .is_some_and(|arm| arm.deadline > now),
                armed_duplicate_operation_id: inner
                    .duplicate_arm
                    .as_ref()
                    .filter(|arm| arm.deadline > now)
                    .map(|arm| arm.operation_id.clone()),
                duplicate_claimed: inner.duplicate_claim.is_some(),
                claimed_duplicate_operation_id: inner
                    .duplicate_claim
                    .as_ref()
                    .map(|claim| claim.operation_id.clone()),
                duplicate_injections: inner.duplicate_injections,
                duplicate_failed_attempts: inner.duplicate_failed_attempts,
                last_failed_duplicate_operation_id: inner
                    .last_failed_duplicate_operation_id
                    .clone(),
                duplicate_delivery_count: inner.duplicate_delivery_count,
                last_duplicate_interaction_id: inner.last_duplicate_interaction_id.clone(),
                last_duplicate_operation_id: inner.last_duplicate_operation_id.clone(),
            },
            effect_http: EffectHttpSnapshot {
                forwarded_requests: inner.forwarded_http_requests,
                rejected_requests: inner.rejected_http_requests,
                indeterminate_armed: inner
                    .indeterminate_arm
                    .as_ref()
                    .is_some_and(|arm| arm.deadline > now),
                armed_indeterminate_operation_id: inner
                    .indeterminate_arm
                    .as_ref()
                    .filter(|arm| arm.deadline > now)
                    .map(|arm| arm.operation_id.clone()),
                indeterminate_claimed: inner.indeterminate_claim.is_some(),
                claimed_indeterminate_operation_id: inner
                    .indeterminate_claim
                    .as_ref()
                    .map(|claim| claim.operation_id.clone()),
                indeterminate_injections: inner.indeterminate_injections,
                last_indeterminate_audit_reason_sha256: inner
                    .last_indeterminate_audit_reason_sha256
                    .clone(),
                last_indeterminate_operation_id: inner.last_indeterminate_operation_id.clone(),
                last_indeterminate_upstream_status: inner.last_indeterminate_upstream_status,
                owned_role_count: inner.owned_role_ids.len(),
                owned_channel_count: inner.owned_channel_ids.len(),
                owned_message_count: inner.owned_message_ids.len(),
            },
        }
    }
}

impl ResourceReservation {
    pub fn commit(mut self, id: String) -> bool {
        let Some(kind) = self.kind.take() else {
            return false;
        };
        self.state.finish_resource_reservation(kind, Some(id))
    }
}

impl Drop for ResourceReservation {
    fn drop(&mut self) {
        if let Some(kind) = self.kind.take() {
            let _ = self.state.finish_resource_reservation(kind, None);
        }
    }
}

pub fn valid_audit_reason(value: &str) -> bool {
    let Some(digest) = value.strip_prefix("starring-effect-v1:") else {
        return false;
    };
    digest.len() == 64
        && digest
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

pub fn valid_operation_id(value: &str) -> bool {
    (8..=96).contains(&value.len())
        && value.as_bytes().iter().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'_' | b':' | b'.')
        })
        && value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
}

fn rearm(operation_id: &str) -> Option<TimedArm> {
    Some(TimedArm {
        operation_id: operation_id.to_owned(),
        deadline: Instant::now().checked_add(ARM_LIFETIME)?,
    })
}

fn hex_sha256(value: &[u8]) -> String {
    let digest = Sha256::digest(value);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("write to string");
    }
    output
}

fn process_instance_id() -> Result<String, StateError> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|_| StateError)?;
    Ok(format!("d2ti-{}", hex_bytes(&random)))
}

fn hex_bytes(value: &[u8]) -> String {
    let mut output = String::with_capacity(value.len() * 2);
    for byte in value {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("write to string");
    }
    output
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use tempfile::TempDir;

    use super::*;

    fn state() -> (TempDir, SharedState) {
        let root = TempDir::new().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let config = Config::for_test(
            root.path().to_path_buf(),
            "7",
            "8",
            "6",
            "127.0.0.1:21001".parse().unwrap(),
            "127.0.0.1:21002".parse().unwrap(),
            "ws://127.0.0.1:22001".to_owned(),
            "http://127.0.0.1:22002".to_owned(),
        );
        (root, SharedState::new(&config).unwrap())
    }

    #[test]
    fn duplicate_claim_is_exact_and_single_use() {
        let (_root, state) = state();
        assert_eq!(
            state.arm_next_duplicate("d2:duplicate:1"),
            ArmOutcome::Armed
        );
        assert_eq!(
            state.arm_next_duplicate("d2:duplicate:1"),
            ArmOutcome::Replayed
        );
        assert_eq!(state.arm_next_duplicate("d2:duplicate:2"), ArmOutcome::Busy);
        assert!(state.claim_duplicate("9", "7", "10").is_none());
        let claim = state.claim_duplicate("9", "7", "8").unwrap();
        assert_eq!(
            serde_json::to_value(state.snapshot()).unwrap()["gateway"]
                ["claimed_duplicate_operation_id"],
            "d2:duplicate:1"
        );
        assert!(state.record_duplicate_delivery(&claim));
        assert!(state.record_duplicate_delivery(&claim));
        assert!(state.finish_duplicate(claim));
        assert!(state.claim_duplicate("9", "7", "8").is_none());
        assert_eq!(
            state.arm_next_duplicate("d2:duplicate:1"),
            ArmOutcome::Replayed
        );
        assert_eq!(state.arm_next_duplicate("d2:duplicate:2"), ArmOutcome::Busy);
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(snapshot["gateway"]["duplicate_injections"], 1);
        assert_eq!(snapshot["gateway"]["identity_rejections"], 1);
    }

    #[test]
    fn partial_duplicate_delivery_is_never_reported_as_injected() {
        let (_root, state) = state();
        assert_eq!(
            state.arm_next_duplicate("d2:duplicate:1"),
            ArmOutcome::Armed
        );
        let claim = state.claim_duplicate("9", "7", "8").unwrap();
        assert!(state.record_duplicate_delivery(&claim));
        assert!(state.abort_duplicate(claim));
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(snapshot["gateway"]["duplicate_delivery_count"], 1);
        assert_eq!(snapshot["gateway"]["duplicate_injections"], 0);
        assert_eq!(snapshot["gateway"]["duplicate_failed_attempts"], 1);
        assert_eq!(snapshot["gateway"]["duplicate_claimed"], false);
        assert_eq!(
            state.arm_next_duplicate("d2:duplicate:1"),
            ArmOutcome::Replayed
        );
        assert_eq!(state.arm_next_duplicate("d2:duplicate:2"), ArmOutcome::Busy);
    }

    #[test]
    fn unsuccessful_indeterminate_claim_rearms_without_evidence_leak() {
        let (_root, state) = state();
        let reason = format!("starring-effect-v1:{}", "a".repeat(64));
        assert_eq!(
            state.arm_next_indeterminate("d2:indeterminate:1"),
            ArmOutcome::Armed
        );
        let claim = state.claim_indeterminate(&reason).unwrap();
        assert_eq!(
            serde_json::to_value(state.snapshot()).unwrap()["effect_http"]
                ["claimed_indeterminate_operation_id"],
            "d2:indeterminate:1"
        );
        assert!(!state.finish_indeterminate(claim, Some(503)));
        let claim = state.claim_indeterminate(&reason).unwrap();
        assert!(state.finish_indeterminate(claim, Some(201)));
        assert_eq!(
            state.arm_next_indeterminate("d2:indeterminate:1"),
            ArmOutcome::Replayed
        );
        assert_eq!(
            state.arm_next_indeterminate("d2:indeterminate:2"),
            ArmOutcome::Busy
        );
        let serialized = serde_json::to_string(&state.snapshot()).unwrap();
        assert!(!serialized.contains(&reason));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&serialized).unwrap()["effect_http"]
                ["indeterminate_injections"],
            1
        );
    }

    #[test]
    fn instance_identity_and_resource_reservations_are_bounded() {
        let (_root, state) = state();
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        let instance_id = snapshot["instance_id"].as_str().unwrap();
        assert_eq!(instance_id.len(), 37);
        assert!(instance_id.starts_with("d2ti-"));
        assert!(instance_id.as_bytes()[5..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')));
        let state = std::sync::Arc::new(state);
        for id in 1..=MAX_OWNED_IDENTITIES {
            assert!(state
                .reserve_resource(ResourceKind::Role)
                .unwrap()
                .commit(id.to_string()));
        }
        assert!(state.reserve_resource(ResourceKind::Role).is_none());
        assert_eq!(
            serde_json::to_value(state.snapshot()).unwrap()["effect_http"]["owned_role_count"],
            MAX_OWNED_IDENTITIES
        );
    }
}
