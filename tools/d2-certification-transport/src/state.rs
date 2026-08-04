use std::collections::{BTreeMap, BTreeSet};
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
    hub_channel_id: String,
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
    gateway_active_connections: u64,
    gateway_completed_connections: u64,
    gateway_clean_close_relays: u64,
    gateway_relay_failures: u64,
    gateway_connection_aborts: u64,
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
    resource_history: BTreeMap<ResourceIdentity, ResourceLifecycleState>,
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ResourceIdentity {
    Role {
        resource_id: String,
    },
    Channel {
        resource_id: String,
    },
    Message {
        channel_id: String,
        resource_id: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum ResourceLifecycleState {
    Created,
    Deleted,
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
    hub_channel_id: String,
    actor_id: String,
    bot_user_id: String,
    gateway: GatewaySnapshot,
    effect_http: EffectHttpSnapshot,
}

#[derive(Clone, Serialize)]
struct GatewaySnapshot {
    partitioned: bool,
    connections: u64,
    active_connections: u64,
    completed_connections: u64,
    clean_close_relays: u64,
    relay_failures: u64,
    connection_aborts: u64,
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

#[derive(Clone, Serialize)]
pub struct ResourceInventory {
    version: u8,
    kind: &'static str,
    instance_id: String,
    run_id: String,
    guild_id: String,
    hub_channel_id: String,
    actor_id: String,
    bot_user_id: String,
    history_limit: usize,
    history: Vec<ResourceInventoryHistoryEntry>,
    created: Vec<ResourceInventoryIdentity>,
    deleted: Vec<ResourceInventoryIdentity>,
    active: Vec<ResourceInventoryIdentity>,
    digest_sha256: String,
}

#[derive(Clone, Serialize)]
struct ResourceInventoryPayload {
    version: u8,
    kind: &'static str,
    instance_id: String,
    run_id: String,
    guild_id: String,
    hub_channel_id: String,
    actor_id: String,
    bot_user_id: String,
    history_limit: usize,
    history: Vec<ResourceInventoryHistoryEntry>,
    created: Vec<ResourceInventoryIdentity>,
    deleted: Vec<ResourceInventoryIdentity>,
    active: Vec<ResourceInventoryIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct ResourceInventoryIdentity {
    kind: &'static str,
    resource_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct ResourceInventoryHistoryEntry {
    #[serde(flatten)]
    identity: ResourceInventoryIdentity,
    state: ResourceLifecycleState,
}

impl SharedState {
    pub fn new(config: &Config) -> Result<Self, StateError> {
        let (partition_tx, _) = broadcast::channel(8);
        let instance_id = process_instance_id()?;
        Ok(Self {
            run_id: config.run_id().to_owned(),
            guild_id: config.guild_id().to_owned(),
            hub_channel_id: config.hub_channel_id().to_owned(),
            actor_id: config.actor_id().to_owned(),
            bot_user_id: config.bot_user_id().to_owned(),
            instance_id,
            partitioned: AtomicBool::new(false),
            gateway_listener_ready: AtomicBool::new(false),
            effect_http_listener_ready: AtomicBool::new(false),
            inner: Mutex::new(StateInner {
                gateway_connections: 0,
                gateway_active_connections: 0,
                gateway_completed_connections: 0,
                gateway_clean_close_relays: 0,
                gateway_relay_failures: 0,
                gateway_connection_aborts: 0,
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
                resource_history: BTreeMap::new(),
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

    pub fn begin_gateway_connection(self: &std::sync::Arc<Self>) -> GatewayConnectionLease {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        inner.gateway_connections = inner.gateway_connections.saturating_add(1);
        inner.gateway_active_connections = inner.gateway_active_connections.saturating_add(1);
        drop(inner);
        GatewayConnectionLease {
            state: std::sync::Arc::clone(self),
            finished: false,
        }
    }

    pub fn mark_gateway_listener_ready(&self) {
        self.gateway_listener_ready.store(true, Ordering::Release);
    }

    #[cfg(test)]
    pub fn gateway_listener_ready(&self) -> bool {
        self.gateway_listener_ready.load(Ordering::Acquire)
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
        let pending_slots = inner
            .pending_role_slots
            .saturating_add(inner.pending_channel_slots)
            .saturating_add(inner.pending_message_slots);
        if !self.resource_state_consistent(&inner)
            || inner.resource_history.len().saturating_add(pending_slots) >= MAX_OWNED_IDENTITIES
        {
            return None;
        }
        let available = match &kind {
            ResourceKind::Role | ResourceKind::Channel => true,
            ResourceKind::Message { channel_id } => {
                inner.owned_channel_ids.contains(channel_id) || *channel_id == self.hub_channel_id
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
        let inner = self.inner.lock().expect("state mutex poisoned");
        self.resource_state_consistent(&inner) && inner.owned_role_ids.contains(id)
    }

    pub fn owns_channel(&self, id: &str) -> bool {
        let inner = self.inner.lock().expect("state mutex poisoned");
        self.resource_state_consistent(&inner) && inner.owned_channel_ids.contains(id)
    }

    pub fn admits_message_creation(&self, channel_id: &str) -> bool {
        let inner = self.inner.lock().expect("state mutex poisoned");
        self.resource_state_consistent(&inner)
            && (channel_id == self.hub_channel_id || inner.owned_channel_ids.contains(channel_id))
    }

    pub fn owns_message(&self, channel_id: &str, id: &str) -> bool {
        let inner = self.inner.lock().expect("state mutex poisoned");
        self.resource_state_consistent(&inner)
            && inner
                .owned_message_ids
                .contains(&(channel_id.to_owned(), id.to_owned()))
    }

    pub fn remove_role(&self, id: &str) -> bool {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        if !self.resource_state_consistent(&inner) {
            return false;
        }
        let identity = ResourceIdentity::Role {
            resource_id: id.to_owned(),
        };
        if inner.resource_history.get(&identity) != Some(&ResourceLifecycleState::Created)
            || !inner.owned_role_ids.remove(id)
        {
            return false;
        }
        inner
            .resource_history
            .insert(identity, ResourceLifecycleState::Deleted);
        self.resource_state_consistent(&inner)
    }

    pub fn remove_channel(&self, id: &str) -> bool {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        if !self.resource_state_consistent(&inner) {
            return false;
        }
        let identity = ResourceIdentity::Channel {
            resource_id: id.to_owned(),
        };
        if inner.resource_history.get(&identity) != Some(&ResourceLifecycleState::Created)
            || !inner.owned_channel_ids.remove(id)
        {
            return false;
        }
        inner
            .resource_history
            .insert(identity, ResourceLifecycleState::Deleted);
        let messages = inner
            .owned_message_ids
            .iter()
            .filter(|(channel_id, _)| channel_id == id)
            .cloned()
            .collect::<Vec<_>>();
        for (channel_id, resource_id) in messages {
            inner
                .owned_message_ids
                .remove(&(channel_id.clone(), resource_id.clone()));
            inner.resource_history.insert(
                ResourceIdentity::Message {
                    channel_id,
                    resource_id,
                },
                ResourceLifecycleState::Deleted,
            );
        }
        self.resource_state_consistent(&inner)
    }

    pub fn remove_message(&self, channel_id: &str, id: &str) -> bool {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        if !self.resource_state_consistent(&inner) {
            return false;
        }
        let identity = ResourceIdentity::Message {
            channel_id: channel_id.to_owned(),
            resource_id: id.to_owned(),
        };
        if inner.resource_history.get(&identity) != Some(&ResourceLifecycleState::Created)
            || !inner
                .owned_message_ids
                .remove(&(channel_id.to_owned(), id.to_owned()))
        {
            return false;
        }
        inner
            .resource_history
            .insert(identity, ResourceLifecycleState::Deleted);
        self.resource_state_consistent(&inner)
    }

    fn finish_resource_reservation(&self, kind: ResourceKind, id: Option<String>) -> bool {
        let mut inner = self.inner.lock().expect("state mutex poisoned");
        let identity = match kind {
            ResourceKind::Role => {
                inner.pending_role_slots = inner.pending_role_slots.saturating_sub(1);
                id.map(|resource_id| ResourceIdentity::Role { resource_id })
            }
            ResourceKind::Channel => {
                inner.pending_channel_slots = inner.pending_channel_slots.saturating_sub(1);
                id.map(|resource_id| ResourceIdentity::Channel { resource_id })
            }
            ResourceKind::Message { channel_id } => {
                inner.pending_message_slots = inner.pending_message_slots.saturating_sub(1);
                id.map(|resource_id| ResourceIdentity::Message {
                    channel_id,
                    resource_id,
                })
            }
        };
        let Some(identity) = identity else {
            return self.resource_state_consistent(&inner);
        };
        if !self.resource_state_consistent(&inner)
            || inner.resource_history.len() >= MAX_OWNED_IDENTITIES
            || !self.resource_identity_allowed(&identity)
            || inner.resource_history.contains_key(&identity)
            || inner
                .resource_history
                .keys()
                .any(|existing| existing.resource_id() == identity.resource_id())
        {
            return false;
        }
        let state = match &identity {
            ResourceIdentity::Role { resource_id } => {
                if !inner.owned_role_ids.insert(resource_id.clone()) {
                    return false;
                }
                ResourceLifecycleState::Created
            }
            ResourceIdentity::Channel { resource_id } => {
                if !inner.owned_channel_ids.insert(resource_id.clone()) {
                    return false;
                }
                ResourceLifecycleState::Created
            }
            ResourceIdentity::Message {
                channel_id,
                resource_id,
            } => {
                let parent_state = if *channel_id == self.hub_channel_id {
                    Some(ResourceLifecycleState::Created)
                } else {
                    inner
                        .resource_history
                        .get(&ResourceIdentity::Channel {
                            resource_id: channel_id.clone(),
                        })
                        .copied()
                };
                let Some(parent_state) = parent_state else {
                    return false;
                };
                if parent_state == ResourceLifecycleState::Created
                    && !inner
                        .owned_message_ids
                        .insert((channel_id.clone(), resource_id.clone()))
                {
                    return false;
                }
                parent_state
            }
        };
        inner.resource_history.insert(identity, state);
        self.resource_state_consistent(&inner)
    }

    pub fn resource_inventory(&self) -> Option<ResourceInventory> {
        let inner = self.inner.lock().expect("state mutex poisoned");
        if !self.resource_state_consistent(&inner) {
            return None;
        }
        let mut history = inner
            .resource_history
            .iter()
            .map(|(identity, state)| ResourceInventoryHistoryEntry {
                identity: identity.inventory_identity(),
                state: *state,
            })
            .collect::<Vec<_>>();
        history.sort();
        let mut active = inner
            .resource_history
            .iter()
            .filter(|(_, state)| **state == ResourceLifecycleState::Created)
            .map(|(identity, _)| identity.inventory_identity())
            .collect::<Vec<_>>();
        active.sort();
        let mut created = inner
            .resource_history
            .keys()
            .map(ResourceIdentity::inventory_identity)
            .collect::<Vec<_>>();
        created.sort();
        let mut deleted = inner
            .resource_history
            .iter()
            .filter(|(_, state)| **state == ResourceLifecycleState::Deleted)
            .map(|(identity, _)| identity.inventory_identity())
            .collect::<Vec<_>>();
        deleted.sort();
        let payload = ResourceInventoryPayload {
            version: 1,
            kind: "starring.d2.run-owned-resource-inventory.v1",
            instance_id: self.instance_id.clone(),
            run_id: self.run_id.clone(),
            guild_id: self.guild_id.clone(),
            hub_channel_id: self.hub_channel_id.clone(),
            actor_id: self.actor_id.clone(),
            bot_user_id: self.bot_user_id.clone(),
            history_limit: MAX_OWNED_IDENTITIES,
            history,
            created,
            deleted,
            active,
        };
        let encoded = serde_json::to_vec(&payload).ok()?;
        Some(ResourceInventory {
            version: payload.version,
            kind: payload.kind,
            instance_id: payload.instance_id,
            run_id: payload.run_id,
            guild_id: payload.guild_id,
            hub_channel_id: payload.hub_channel_id,
            actor_id: payload.actor_id,
            bot_user_id: payload.bot_user_id,
            history_limit: payload.history_limit,
            history: payload.history,
            created: payload.created,
            deleted: payload.deleted,
            active: payload.active,
            digest_sha256: hex_sha256(&encoded),
        })
    }

    fn resource_identity_allowed(&self, identity: &ResourceIdentity) -> bool {
        let (resource_id, channel_id) = match identity {
            ResourceIdentity::Role { resource_id } | ResourceIdentity::Channel { resource_id } => {
                (resource_id, None)
            }
            ResourceIdentity::Message {
                channel_id,
                resource_id,
            } => (resource_id, Some(channel_id)),
        };
        crate::config::valid_snowflake(resource_id)
            && channel_id.is_none_or(|channel_id| crate::config::valid_snowflake(channel_id))
            && ![
                self.guild_id.as_str(),
                self.hub_channel_id.as_str(),
                self.actor_id.as_str(),
                self.bot_user_id.as_str(),
            ]
            .contains(&resource_id.as_str())
    }

    fn resource_state_consistent(&self, inner: &StateInner) -> bool {
        if inner.resource_history.len() > MAX_OWNED_IDENTITIES {
            return false;
        }
        let mut roles = BTreeSet::new();
        let mut channels = BTreeSet::new();
        let mut messages = BTreeSet::new();
        let mut resource_ids = BTreeSet::new();
        for (identity, state) in &inner.resource_history {
            if !self.resource_identity_allowed(identity)
                || !resource_ids.insert(identity.resource_id().to_owned())
            {
                return false;
            }
            let active = *state == ResourceLifecycleState::Created;
            match identity {
                ResourceIdentity::Role { resource_id } => {
                    if active {
                        roles.insert(resource_id.clone());
                    }
                }
                ResourceIdentity::Channel { resource_id } => {
                    if active {
                        channels.insert(resource_id.clone());
                    }
                }
                ResourceIdentity::Message {
                    channel_id,
                    resource_id,
                } => {
                    let parent_state = if *channel_id == self.hub_channel_id {
                        Some(ResourceLifecycleState::Created)
                    } else {
                        inner
                            .resource_history
                            .get(&ResourceIdentity::Channel {
                                resource_id: channel_id.clone(),
                            })
                            .copied()
                    };
                    if parent_state.is_none()
                        || (active && parent_state != Some(ResourceLifecycleState::Created))
                    {
                        return false;
                    }
                    if active {
                        messages.insert((channel_id.clone(), resource_id.clone()));
                    }
                }
            }
        }
        roles == inner.owned_role_ids
            && channels == inner.owned_channel_ids
            && messages == inner.owned_message_ids
    }

    pub fn snapshot(&self) -> Snapshot {
        let inner = self.inner.lock().expect("state mutex poisoned");
        let now = Instant::now();
        Snapshot {
            version: 3,
            ready: self.gateway_listener_ready.load(Ordering::Acquire)
                && self.effect_http_listener_ready.load(Ordering::Acquire)
                && inner.gateway_relay_failures == 0
                && inner.gateway_connection_aborts == 0,
            instance_id: self.instance_id.clone(),
            run_id: self.run_id.clone(),
            guild_id: self.guild_id.clone(),
            hub_channel_id: self.hub_channel_id.clone(),
            actor_id: self.actor_id.clone(),
            bot_user_id: self.bot_user_id.clone(),
            gateway: GatewaySnapshot {
                partitioned: self.is_partitioned(),
                connections: inner.gateway_connections,
                active_connections: inner.gateway_active_connections,
                completed_connections: inner.gateway_completed_connections,
                clean_close_relays: inner.gateway_clean_close_relays,
                relay_failures: inner.gateway_relay_failures,
                connection_aborts: inner.gateway_connection_aborts,
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

impl ResourceIdentity {
    fn resource_id(&self) -> &str {
        match self {
            Self::Role { resource_id }
            | Self::Channel { resource_id }
            | Self::Message { resource_id, .. } => resource_id,
        }
    }

    fn inventory_identity(&self) -> ResourceInventoryIdentity {
        match self {
            Self::Role { resource_id } => ResourceInventoryIdentity {
                kind: "role",
                resource_id: resource_id.clone(),
                channel_id: None,
            },
            Self::Channel { resource_id } => ResourceInventoryIdentity {
                kind: "channel",
                resource_id: resource_id.clone(),
                channel_id: None,
            },
            Self::Message {
                channel_id,
                resource_id,
            } => ResourceInventoryIdentity {
                kind: "message",
                resource_id: resource_id.clone(),
                channel_id: Some(channel_id.clone()),
            },
        }
    }
}

pub struct GatewayConnectionLease {
    state: ArcState,
    finished: bool,
}

impl GatewayConnectionLease {
    pub fn complete_clean_close(mut self) {
        self.finish(true, false, false);
    }

    pub fn complete(mut self) {
        self.finish(false, false, false);
    }

    pub fn fail(mut self) {
        self.finish(false, true, false);
    }

    fn finish(&mut self, clean_close: bool, failed: bool, aborted: bool) {
        if self.finished {
            return;
        }
        let mut inner = self.state.inner.lock().expect("state mutex poisoned");
        if inner.gateway_active_connections == 0 {
            inner.gateway_relay_failures = inner.gateway_relay_failures.saturating_add(1);
        } else {
            inner.gateway_active_connections -= 1;
        }
        inner.gateway_completed_connections = inner.gateway_completed_connections.saturating_add(1);
        if clean_close {
            inner.gateway_clean_close_relays = inner.gateway_clean_close_relays.saturating_add(1);
        }
        if failed {
            inner.gateway_relay_failures = inner.gateway_relay_failures.saturating_add(1);
        }
        if aborted {
            inner.gateway_connection_aborts = inner.gateway_connection_aborts.saturating_add(1);
        }
        self.finished = true;
    }
}

impl Drop for GatewayConnectionLease {
    fn drop(&mut self) {
        self.finish(false, false, true);
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
            "5",
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
        for id in 100..100 + MAX_OWNED_IDENTITIES {
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
        assert!(state.remove_role("100"));
        assert!(state.reserve_resource(ResourceKind::Role).is_none());
        let inventory = serde_json::to_value(state.resource_inventory().unwrap()).unwrap();
        assert_eq!(inventory["history"].as_array().unwrap().len(), 128);
        assert_eq!(inventory["active"].as_array().unwrap().len(), 127);
        assert_eq!(inventory["history"][0]["state"], "deleted");
    }

    #[test]
    fn resource_identity_collisions_and_boundary_ownership_fail_closed() {
        let (_root, state) = state();
        let state = std::sync::Arc::new(state);
        assert!(!state
            .reserve_resource(ResourceKind::Role)
            .unwrap()
            .commit("7".to_owned()));
        assert!(!state
            .reserve_resource(ResourceKind::Channel)
            .unwrap()
            .commit("5".to_owned()));
        assert!(state
            .reserve_resource(ResourceKind::Role)
            .unwrap()
            .commit("11".to_owned()));
        assert!(!state
            .reserve_resource(ResourceKind::Channel)
            .unwrap()
            .commit("11".to_owned()));
        let inventory = serde_json::to_value(state.resource_inventory().unwrap()).unwrap();
        assert_eq!(inventory["history"].as_array().unwrap().len(), 1);
        assert_eq!(inventory["active"].as_array().unwrap().len(), 1);
        assert_eq!(inventory["active"][0]["kind"], "role");
        assert_eq!(inventory["active"][0]["resource_id"], "11");
    }

    #[test]
    fn gateway_connection_failures_are_counted_and_remove_readiness() {
        let (_root, state) = state();
        let state = std::sync::Arc::new(state);
        state.mark_gateway_listener_ready();
        state.mark_effect_http_listener_ready();
        assert!(serde_json::to_value(state.snapshot()).unwrap()["ready"]
            .as_bool()
            .unwrap());
        let failed = state.begin_gateway_connection();
        assert_eq!(
            serde_json::to_value(state.snapshot()).unwrap()["gateway"]["active_connections"],
            1
        );
        failed.fail();
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(snapshot["ready"], false);
        assert_eq!(snapshot["gateway"]["active_connections"], 0);
        assert_eq!(snapshot["gateway"]["completed_connections"], 1);
        assert_eq!(snapshot["gateway"]["relay_failures"], 1);
        assert_eq!(snapshot["gateway"]["connection_aborts"], 0);
        let aborted = state.begin_gateway_connection();
        drop(aborted);
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(snapshot["gateway"]["completed_connections"], 2);
        assert_eq!(snapshot["gateway"]["relay_failures"], 1);
        assert_eq!(snapshot["gateway"]["connection_aborts"], 1);
    }

    #[test]
    fn pinned_hub_admits_only_owned_messages_without_becoming_an_owned_channel() {
        let (_root, state) = state();
        let state = std::sync::Arc::new(state);
        assert!(state.admits_message_creation("5"));
        assert!(!state.owns_channel("5"));
        assert!(state
            .reserve_resource(ResourceKind::Message {
                channel_id: "5".to_owned()
            })
            .unwrap()
            .commit("13".to_owned()));
        assert!(state.owns_message("5", "13"));
        assert!(state
            .reserve_resource(ResourceKind::Message {
                channel_id: "10".to_owned()
            })
            .is_none());
        assert!(!state.remove_channel("5"));
        assert!(state.owns_message("5", "13"));
        let snapshot = serde_json::to_value(state.snapshot()).unwrap();
        assert_eq!(snapshot["version"], 3);
        assert_eq!(snapshot["hub_channel_id"], "5");
        assert_eq!(snapshot["effect_http"]["owned_channel_count"], 0);
        assert_eq!(snapshot["effect_http"]["owned_message_count"], 1);
    }
}
